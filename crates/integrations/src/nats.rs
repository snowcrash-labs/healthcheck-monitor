//! Aggregate-only JetStream state; no subscriptions to application message subjects.
use super::{projection::observation, transport::Error};
use futures::StreamExt;
use monitor_core::{
    config::resolve::Job,
    model::{Data, Observation},
};
use tokio_util::sync::CancellationToken;

pub struct Nats {
    client: async_nats::Client,
}
impl Nats {
    pub async fn connect(url: &str) -> Result<Self, Error> {
        let client = async_nats::ConnectOptions::new()
            .require_tls(true)
            .max_reconnects(Some(3))
            .connect(url)
            .await
            .map_err(|_| Error::Authentication)?;
        Ok(Self { client })
    }
    pub async fn collect(
        &self,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Vec<Observation>, Error> {
        let context = async_nats::jetstream::new(self.client.clone());
        let mut streams = context.streams();
        let mut result = Vec::new();
        loop {
            let next = tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), r = streams.next() => r };
            let Some(info) = next else { break };
            let info = info.map_err(|_| Error::Unavailable)?;
            if result.len() >= job.settings.max_series {
                return Err(Error::Limit);
            }
            result.push(observation(
                job,
                "nats-streams",
                &info.config.name,
                Data::Metric {
                    name: "stored-messages".into(),
                    value: info.state.messages as f64,
                    capacity: None,
                    warning: None,
                    error: None,
                    window_seconds: 0,
                },
            ));
        }
        Ok(result)
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
