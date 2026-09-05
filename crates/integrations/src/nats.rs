//! Aggregate-only JetStream state; no subscriptions to application message subjects.
use super::transport::Error;
use monitor_core::config::resolve::Job;
use tokio_util::sync::CancellationToken;

pub struct Nats {
    client: async_nats::Client,
}
impl Nats {
    pub async fn connect(
        url: &str,
        credential: Option<&monitor_core::config::types::Credential>,
    ) -> Result<Self, Error> {
        let mut options = async_nats::ConnectOptions::new()
            .require_tls(true)
            .max_reconnects(Some(3));
        if let Some(path) = credential.and_then(|credential| credential.credential_file.as_ref()) {
            use tokio::io::AsyncReadExt;
            let mut bytes = Vec::new();
            tokio::fs::File::open(path)
                .await
                .map_err(|_| Error::Authentication)?
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::Authentication)?;
            if bytes.len() > 65536 {
                return Err(Error::Limit);
            }
            options = options
                .credentials(std::str::from_utf8(&bytes).map_err(|_| Error::Authentication)?)
                .map_err(|_| Error::Authentication)?;
        }
        if let Some(variable) = credential.and_then(|credential| credential.token_env.as_ref()) {
            options = options.token(std::env::var(variable).map_err(|_| Error::Authentication)?);
        }
        let client = options
            .connect(url)
            .await
            .map_err(|_| Error::Authentication)?;
        Ok(Self { client })
    }
    pub async fn collect(
        &self,
        job: &Job,
        cancel: &CancellationToken,
    ) -> monitor_core::model::CheckResult {
        super::nats_collect::collect(&self.client, job, cancel).await
    }
}
/// Fixed report parser exposes numeric aggregates and stream identity only.
pub fn parse_report(text: &str, limit: usize) -> Result<Vec<(String, u64, u64)>, Error> {
    let mut headers = None;
    let mut rows = Vec::new();
    for line in text.lines() {
        if !line.starts_with('│') || !line.ends_with('│') {
            continue;
        }
        let cells: Vec<_> = line.trim_matches('│').split('│').map(str::trim).collect();
        if cells.iter().any(|s| s.eq_ignore_ascii_case("Stream")) {
            headers = Some(
                cells
                    .iter()
                    .map(|s| s.to_ascii_lowercase())
                    .collect::<Vec<_>>(),
            );
            continue;
        }
        let Some(headers) = &headers else { continue };
        if headers.len() != cells.len() {
            return Err(Error::Malformed);
        }
        let field = |name: &str| {
            headers
                .iter()
                .position(|h| h == name)
                .and_then(|i| cells.get(i))
                .copied()
                .ok_or(Error::Malformed)
        };
        let stream = field("stream")?;
        let messages = field("messages")?.parse().map_err(|_| Error::Malformed)?;
        let bytes = field("bytes")?.parse().map_err(|_| Error::Malformed)?;
        if rows.len() >= limit {
            return Err(Error::Limit);
        }
        rows.push((super::projection::identity(stream), messages, bytes));
    }
    if headers.is_none() {
        return Err(Error::Malformed);
    }
    Ok(rows)
}
