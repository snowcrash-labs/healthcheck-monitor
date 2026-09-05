//! SSE carries coalesced revisions and heartbeat metadata rather than evidence payloads.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{
    extract::State,
    response::{
        Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures::{Stream, stream};
use serde::Serialize;
use std::{
    convert::Infallible,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
#[derive(Serialize)]
struct Announcement {
    generation: u64,
    heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    running: bool,
}
pub async fn events(
    State(app): State<Arc<App>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = app
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::Busy)?;
    let receiver = app.bus.changed.subscribe();
    let events = stream::unfold(
        (app, receiver, permit, true),
        |(app, mut receiver, permit, first)| async move {
            if !first {
                tokio::select! {_=app.stop.cancelled()=>return None,_=tokio::time::sleep(Duration::from_secs(5))=>{},changed=receiver.changed()=>{if changed.is_err(){return None;}}}
            }
            if app.stop.is_cancelled() {
                return None;
            }
            let generation = *receiver.borrow_and_update();
            let data = Announcement {
                generation,
                heartbeat_at: chrono::DateTime::from_timestamp_millis(
                    app.bus.heartbeat.load(Ordering::Acquire),
                ),
                running: app.bus.running.load(Ordering::Acquire),
            };
            let event = match Event::default()
                .event("revision")
                .id(generation.to_string())
                .json_data(data)
            {
                Ok(event) => event,
                Err(_) => return None,
            };
            Some((Ok(event), (app, receiver, permit, false)))
        },
    );
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
pub async fn health(State(app): State<Arc<App>>) -> Result<Response, ApiError> {
    let heartbeat = app.bus.heartbeat.load(Ordering::Acquire);
    let healthy = app.bus.running.load(Ordering::Acquire)
        && chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub(heartbeat)
            < 15000;
    if !healthy {
        return Err(ApiError::Unavailable);
    }
    json(&serde_json::json!({"running":true}), 1024)
}
