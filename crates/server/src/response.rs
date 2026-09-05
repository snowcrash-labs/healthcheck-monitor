//! Bounded JSON serialization and fixed errors keep HTTP output predictable.
use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use std::io::Write;
#[derive(Debug)]
pub enum ApiError {
    Waiting,
    Unavailable,
    BadQuery,
    NotFound,
    Unauthorized,
    Busy,
    Capacity,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Waiting => (StatusCode::SERVICE_UNAVAILABLE, "waiting"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::BadQuery => (StatusCode::BAD_REQUEST, "bad_query"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
            Self::Capacity => (StatusCode::SERVICE_UNAVAILABLE, "response_limit"),
        };
        (
            status,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            format!("{{\"error\":{{\"code\":\"{code}\"}}}}"),
        )
            .into_response()
    }
}
impl From<monitor_history::error::Error> for ApiError {
    fn from(_: monitor_history::error::Error) -> Self {
        Self::Unavailable
    }
}
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("response limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn json(value: &impl serde::Serialize, limit: usize) -> Result<Response, ApiError> {
    let mut output = Limited {
        bytes: Vec::with_capacity(limit.min(65536)),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| ApiError::Capacity)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        output.bytes,
    )
        .into_response())
}
