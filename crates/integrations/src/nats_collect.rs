//! Native JetStream aggregate collection never consumes application messages.
use crate::{
    projection::{observation, operation},
    transport::Error,
};
use futures::StreamExt;
use monitor_core::{config::resolve::Job, model::*};
use tokio_util::sync::CancellationToken;
pub async fn collect(
    client: &async_nats::Client,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    if client.server_info().max_payload > job.settings.response_bytes {
        result
            .operations
            .push(operation("nats-payload-limit", Err(&Error::Limit), 0, true));
        return result;
    }
    let mut context = async_nats::jetstream::new(client.clone());
    context.set_timeout(job.settings.attempt_timeout.duration());
    let mut streams = context.streams();
    let mut count = 0;
    loop {
        let next = tokio::select! {_ =cancel.cancelled()=>{result.operations.push(operation("nats-streams",Err(&Error::Cancelled),0,true));break;},next=streams.next()=>next};
        let Some(next) = next else {
            break;
        };
        let info = match next {
            Ok(info) => info,
            Err(_) => {
                result.operations.push(operation(
                    "nats-streams",
                    Err(&Error::Unavailable),
                    1,
                    true,
                ));
                break;
            }
        };
        if !job.target.resources.is_empty()
            && !job
                .target
                .resources
                .iter()
                .any(|selector| info.config.name.contains(selector))
        {
            continue;
        }
        if result.observations.len() + 2 > job.settings.max_series {
            result
                .operations
                .push(operation("nats-streams", Err(&Error::Limit), 1, true));
            break;
        }
        let id = format!("nats-stream/{}", info.config.name);
        for (name, value) in [
            ("stored-messages", info.state.messages as f64),
            ("stored-bytes", info.state.bytes as f64),
        ] {
            result
                .observations
                .push(observation(job, &id, name, metric(name, value)));
        }
        result.operations.push(operation(&id, Ok(2), 1, true));
        count += 1;
        let stream = match context.get_stream_no_info(&info.config.name).await {
            Ok(stream) => stream,
            Err(_) => {
                result.operations.push(operation(
                    &format!("nats-consumers/{}", info.config.name),
                    Err(&Error::Unavailable),
                    1,
                    true,
                ));
                continue;
            }
        };
        let mut consumers = stream.consumers();
        while let Some(consumer) =
            tokio::select! {_ =cancel.cancelled()=>None,consumer=consumers.next()=>consumer}
        {
            let consumer = match consumer {
                Ok(consumer) => consumer,
                Err(_) => {
                    result.operations.push(operation(
                        &format!("nats-consumers/{}", info.config.name),
                        Err(&Error::Unavailable),
                        1,
                        true,
                    ));
                    break;
                }
            };
            let id = format!("nats-consumer/{}/{}", info.config.name, consumer.name);
            if result.observations.len() + 5 > job.settings.max_series {
                result
                    .operations
                    .push(operation(&id, Err(&Error::Limit), 1, true));
                break;
            }
            for (name, value) in [
                ("pending", consumer.num_pending as f64),
                ("ack-pending", consumer.num_ack_pending as f64),
                ("redelivered", consumer.num_redelivered as f64),
                ("delivered", consumer.delivered.consumer_sequence as f64),
                ("acknowledged", consumer.ack_floor.consumer_sequence as f64),
            ] {
                result.observations.push(observation(
                    job,
                    &id,
                    name,
                    metric(
                        &format!("{}/{}/{name}", info.config.name, consumer.name),
                        value,
                    ),
                ));
            }
            result.operations.push(operation(&id, Ok(5), 1, true));
        }
    }
    if result.operations.is_empty() {
        result
            .operations
            .push(operation("nats-streams", Ok(count), 1, true));
    }
    result.finished_at = chrono::Utc::now();
    result
}
fn metric(name: &str, value: f64) -> Data {
    Data::Metric {
        name: crate::projection::identity(name),
        value,
        capacity: None,
        warning: None,
        error: None,
        window_seconds: 0,
    }
}
