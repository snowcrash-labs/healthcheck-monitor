//! Bounded native billing query jobs use ADC and stable database-owned job identities.
use monitor_core::config::settings::Settings;
use monitor_costs::{
    config::Gcp,
    model::{Charge, Provider},
    query::Period,
};
use monitor_integrations::{
    http_pool::Pools,
    transport::{Error, Http, bounded, response_error},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
pub struct Reader {
    http: Http,
    credentials: google_cloud_auth::credentials::AccessTokenCredentials,
    source: Gcp,
    settings: Settings,
}
pub struct Page {
    pub rows: Vec<Charge>,
    pub next: Option<String>,
    pub total: usize,
}
#[derive(Deserialize)]
struct WirePage {
    #[serde(default)]
    rows: Vec<WireRow>,
    #[serde(rename = "pageToken")]
    next: Option<String>,
    #[serde(rename = "totalRows")]
    total: String,
}
#[derive(Deserialize)]
struct WireRow {
    f: Vec<Cell>,
}
#[derive(Deserialize)]
struct Cell {
    v: Option<String>,
}
impl Reader {
    pub async fn new(source: Gcp, pools: Arc<Pools>) -> Result<Self, Error> {
        let mut settings = Settings::default();
        settings.response_bytes = 1024 * 1024;
        Ok(Self {
            http: Http::shared(pools, &settings)?,
            credentials: crate::google_credentials::load(None).await?,
            source,
            settings,
        })
    }
    fn endpoint(&self, suffix: &str) -> String {
        format!(
            "https://bigquery.googleapis.com/bigquery/v2/projects/{}/{}",
            self.source.project, suffix
        )
    }
    async fn request(
        &self,
        method: reqwest::Method,
        suffix: &str,
        body: Option<Value>,
        stop: &CancellationToken,
    ) -> Result<Value, Error> {
        // The only POST bodies are generated internally by the fixed billing statement.
        let token = self
            .credentials
            .access_token()
            .await
            .map_err(|_| Error::Authentication)?;
        let mut request = self
            .http
            .client()
            .request(method, self.endpoint(suffix))
            .bearer_auth(token.token)
            .timeout(self.settings.attempt_timeout.duration());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let request = request.build().map_err(|_| Error::Malformed)?;
        for attempt in 0..3 {
            let request = request.try_clone().ok_or(Error::Malformed)?;
            let response = tokio::select! { _=stop.cancelled()=>return Err(Error::Cancelled), response=self.http.client().execute(request)=>response.map_err(|_| Error::Unavailable)? };
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(Error::Missing);
            }
            if status == reqwest::StatusCode::CONFLICT {
                return Ok(json!({"existing":true}));
            }
            let bytes = bounded(response, self.settings.response_bytes).await?;
            if status.is_success() {
                return serde_json::from_slice(&bytes).map_err(|_| Error::Malformed);
            }
            let error = response_error(status, &bytes);
            if !error.retryable() || attempt == 2 {
                return Err(error);
            }
            tokio::select! { _=stop.cancelled()=>return Err(Error::Cancelled), _=tokio::time::sleep(std::time::Duration::from_secs(1 << attempt))=>{} }
        }
        Err(Error::Unavailable)
    }
    /// Resume a named job after timeout rather than submitting a second paid query.
    pub async fn start(
        &self,
        job: &str,
        scope: &str,
        period: Period,
        bytes: u64,
        rows: usize,
        stop: &CancellationToken,
    ) -> Result<(), Error> {
        if !monitor_costs::config::identifier(job) {
            return Err(Error::Forbidden);
        }
        let suffix = format!("jobs/{job}?location={}", self.source.location);
        match self
            .request(reqwest::Method::GET, &suffix, None, stop)
            .await
        {
            Ok(_) => {}
            Err(Error::Missing) => {
                let dry = monitor_costs::gcp_query::configuration(
                    &self.source,
                    scope,
                    period,
                    bytes,
                    rows,
                    true,
                )
                .map_err(|_| Error::Forbidden)?;
                let estimate = self
                    .request(
                        reqwest::Method::POST,
                        "jobs",
                        Some(json!({"configuration":dry})),
                        stop,
                    )
                    .await?;
                let estimated = estimate
                    .pointer("/statistics/totalBytesProcessed")
                    .and_then(Value::as_str)
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or(Error::Malformed)?;
                if estimated > bytes {
                    return Err(Error::Limit);
                }
                let config = monitor_costs::gcp_query::configuration(
                    &self.source,
                    scope,
                    period,
                    bytes,
                    rows,
                    false,
                )
                .map_err(|_| Error::Forbidden)?;
                self.request(reqwest::Method::POST, "jobs", Some(json!({"jobReference":{"projectId":self.source.project,"jobId":job,"location":self.source.location},"configuration":config})), stop).await?;
            }
            Err(error) => return Err(error),
        }
        for _ in 0..120 {
            let result = self
                .request(reqwest::Method::GET, &suffix, None, stop)
                .await?;
            if result.pointer("/status/errorResult").is_some() {
                return Err(Error::Malformed);
            }
            if result.pointer("/status/state").and_then(Value::as_str) == Some("DONE") {
                return Ok(());
            }
            tokio::select! { _=stop.cancelled()=>return Err(Error::Cancelled), _=tokio::time::sleep(std::time::Duration::from_secs(2))=>{} }
        }
        Err(Error::Timeout)
    }
    pub async fn page(
        &self,
        job: &str,
        cursor: Option<&str>,
        stop: &CancellationToken,
    ) -> Result<Page, Error> {
        let mut url = url::Url::parse(&self.endpoint(&format!("queries/{job}")))
            .map_err(|_| Error::Malformed)?;
        url.query_pairs_mut()
            .append_pair("location", &self.source.location)
            .append_pair("maxResults", "500")
            .append_pair("timeoutMs", "0");
        if let Some(cursor) = cursor {
            if cursor.len() > 8192 {
                return Err(Error::Limit);
            }
            url.query_pairs_mut().append_pair("pageToken", cursor);
        }
        let suffix = url
            .as_str()
            .strip_prefix(&self.endpoint(""))
            .ok_or(Error::Forbidden)?;
        let value = self
            .request(reqwest::Method::GET, suffix, None, stop)
            .await?;
        let page: WirePage = serde_json::from_value(value).map_err(|_| Error::Malformed)?;
        if page.rows.len() > 500 {
            return Err(Error::Limit);
        }
        let rows = page
            .rows
            .into_iter()
            .map(decode)
            .collect::<Result<_, _>>()?;
        Ok(Page {
            rows,
            next: page.next,
            total: page.total.parse().map_err(|_| Error::Malformed)?,
        })
    }
}
fn decode(row: WireRow) -> Result<Charge, Error> {
    if row.f.len() != 9 {
        return Err(Error::Malformed);
    }
    let mut cells = row.f.into_iter();
    let mut next = || cells.next().and_then(|c| c.v).ok_or(Error::Malformed);
    let charge = Charge {
        day: next()?.parse().map_err(|_| Error::Malformed)?,
        invoice_month: next()?,
        provider: Provider::Gcp,
        scope: next()?,
        region: next()?,
        product: next()?,
        resource: next()?,
        category: next()?,
        currency: next()?,
        billed: next()?.try_into().map_err(|_| Error::Malformed)?,
        effective: None,
    };
    charge.validate().map_err(|_| Error::Malformed)?;
    Ok(charge)
}
