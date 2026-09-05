//! SPA deep links receive the application document; unknown asset paths remain 404.
use axum::{
    body::Body,
    extract::Request,
    response::{IntoResponse, Response},
};
use std::{convert::Infallible, path::PathBuf};
use tower::ServiceExt;
pub fn fallback(
    index: PathBuf,
) -> impl tower::Service<Request, Response = Response, Error = Infallible, Future: Send> + Clone {
    tower::service_fn(move |request: Request| {
        let index = index.clone();
        async move {
            let path = request.uri().path();
            if !(matches!(path, "/" | "/findings" | "/resources" | "/history")
                || path.starts_with("/resources/"))
            {
                return Ok(crate::response::ApiError::NotFound.into_response());
            }
            let response = tower_http::services::ServeFile::new(index)
                .oneshot(request)
                .await?;
            Ok(response.map(Body::new))
        }
    })
}
