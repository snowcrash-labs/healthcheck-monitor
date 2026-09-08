//! Typed Azure ActualCost queries keep source scope and decimal tokens intact.
use crate::auth::Auth;
use monitor_core::{config::settings::Settings, model::Provider as CoreProvider};
use monitor_costs::{
    config::Source,
    model::{Amount, Charge, Provider},
    query::Period,
};
use monitor_integrations::{
    http_pool::Pools,
    process::Processes,
    transport::{Error, Http},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct Reader {
    http: Http,
    auth: Auth,
    settings: Settings,
    root: url::Url,
    scope: String,
}
pub struct Page {
    pub rows: Vec<Charge>,
    pub next: Option<String>,
}
#[derive(Deserialize)]
struct Response {
    properties: Properties,
}
#[derive(Deserialize)]
struct Properties {
    columns: Vec<Column>,
    rows: Vec<Vec<Value>>,
    #[serde(rename = "nextLink")]
    next: Option<String>,
}
#[derive(Deserialize)]
struct Column {
    name: String,
}
impl Reader {
    pub async fn new(source: &Source, pools: Arc<Pools>) -> Result<Self, Error> {
        let options = source.azure_query.as_ref().ok_or(Error::Forbidden)?;
        let settings = Settings {
            response_bytes: 8 * 1024 * 1024,
            ..Default::default()
        };
        let http = Http::shared(pools, &settings)?;
        let auth = Auth::new(
            CoreProvider::Azure,
            source.credential.as_ref(),
            None,
            Some(&source.billing_scope),
            &http,
            &settings,
            &Processes::new(1),
        )
        .await?;
        let root = format!("https://management.azure.com/subscriptions/{}/providers/Microsoft.CostManagement/query?api-version={}", source.billing_scope, options.api_version).parse().map_err(|_| Error::Malformed)?;
        Ok(Self {
            http,
            auth,
            settings,
            root,
            scope: source.billing_scope.clone(),
        })
    }
    pub async fn page(
        &self,
        period: Period,
        cursor: Option<&str>,
        stop: &CancellationToken,
    ) -> Result<Page, Error> {
        let url = match cursor {
            Some(cursor) => continuation(&self.root, cursor)?,
            None => self.root.clone(),
        };
        let last = period.to.pred_opt().ok_or(Error::Malformed)?;
        let body = json!({"type":"ActualCost","timeframe":"Custom","timePeriod":{"from":format!("{}T00:00:00Z",period.from),"to":format!("{last}T23:59:59Z")},"dataset":{"granularity":"Daily","aggregation":{"totalCost":{"name":"Cost","function":"Sum"}},"grouping":[{"type":"Dimension","name":"ServiceName"},{"type":"Dimension","name":"ResourceId"}]}});
        let token = self.auth.bearer().await?;
        let request = self
            .http
            .client()
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .build()
            .map_err(|_| Error::Malformed)?;
        let value = self.http.json(request, &self.settings, stop).await?;
        decode(value, &self.scope)
    }
}
pub(crate) fn continuation(root: &url::Url, cursor: &str) -> Result<url::Url, Error> {
    if cursor.len() > 8192 {
        return Err(Error::Limit);
    }
    let url: url::Url = cursor.parse().map_err(|_| Error::Malformed)?;
    if url.origin() != root.origin()
        || !url.path().eq_ignore_ascii_case(root.path())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url
            .query_pairs()
            .any(|(key, _)| key != "api-version" && key != "$skiptoken")
    {
        return Err(Error::Forbidden);
    }
    Ok(url)
}
pub(crate) fn decode(value: Value, scope: &str) -> Result<Page, Error> {
    let response: Response = serde_json::from_value(value).map_err(|_| Error::Malformed)?;
    let properties = response.properties;
    if properties.columns.len() > 32 || properties.rows.len() > 5000 {
        return Err(Error::Limit);
    }
    let column = |names: &[&str]| {
        properties
            .columns
            .iter()
            .position(|c| names.contains(&c.name.as_str()))
            .ok_or(Error::Missing)
    };
    let date = column(&["UsageDate"])?;
    let product = column(&["ServiceName"])?;
    let resource = column(&["ResourceId"])?;
    let currency = column(&["Currency"])?;
    let cost = column(&["Cost", "PreTaxCost"])?;
    let mut rows = Vec::new();
    for row in properties.rows {
        let get = |index: usize| row.get(index).ok_or(Error::Malformed);
        let day = get(date)?
            .as_u64()
            .map(|d| d.to_string())
            .or_else(|| get(date).ok()?.as_str().map(str::to_string))
            .ok_or(Error::Malformed)?;
        let day = chrono::NaiveDate::parse_from_str(&day, "%Y%m%d")
            .or_else(|_| day.parse())
            .map_err(|_| Error::Malformed)?;
        let raw = get(cost)?;
        let amount = Amount::provider(
            raw.as_str()
                .map(String::from)
                .unwrap_or_else(|| raw.to_string())
                .as_str(),
        )
        .map_err(|_| Error::Malformed)?;
        let text = |index| {
            get(index)?
                .as_str()
                .map(String::from)
                .ok_or(Error::Malformed)
        };
        let charge = Charge {
            target: None,
            day,
            invoice_month: None,
            provider: Provider::Azure,
            scope: Some(scope.into()),
            region: None,
            product: text(product)?,
            resource: get(resource)?
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase),
            category: None,
            currency: text(currency)?,
            billed: amount,
            effective: None,
        };
        charge.validate().map_err(|_| Error::Malformed)?;
        rows.push(charge);
    }
    Ok(Page {
        rows,
        next: properties.next,
    })
}
