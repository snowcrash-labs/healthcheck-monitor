//! Serve compile-time embedded bytes without filesystem access or runtime compression.
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "$OUT_DIR/dashboard"]
struct Assets;

pub async fn serve(request: Request) -> Response {
    let path = request.uri().path();
    let document = matches!(path, "/" | "/findings" | "/resources" | "/history")
        || path.starts_with("/resources/");
    let path = if document {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let Some(coding) = crate::encoding::select(
        request
            .headers()
            .get(header::ACCEPT_ENCODING)
            .and_then(|value| value.to_str().ok()),
    ) else {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    };
    let Some(file) = Assets::get(&format!("{}/{path}", coding.directory())) else {
        return crate::response::ApiError::NotFound.into_response();
    };
    // debug-embed is mandatory; an owned value would mean the filesystem-backed mode was enabled.
    let Cow::Borrowed(bytes) = file.data else {
        return crate::response::ApiError::Unavailable.into_response();
    };
    let etag = format!(
        "\"{}\"",
        file.metadata
            .sha256_hash()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let unchanged = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|tag| tag.trim() == etag || tag.trim() == "*")
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Response::new(Body::from(axum::body::Bytes::from_static(bytes)))
    };
    let headers = response.headers_mut();
    let content_type = mime_guess::from_path(path).first_or_octet_stream();
    if let Ok(value) = HeaderValue::from_str(content_type.as_ref()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&etag) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if document {
            "private, no-cache"
        } else {
            "private, max-age=31536000, immutable"
        }),
    );
    if let Some(coding) = coding.header() {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
    }
    if !unchanged && let Ok(value) = HeaderValue::from_str(&bytes.len().to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use std::io::Read;
    #[tokio::test]
    async fn compressed_assets_are_borrowed_embedded_bytes_and_have_variant_etags()
    -> Result<(), Box<dyn std::error::Error>> {
        let raw = Assets::get("raw/index.html").ok_or("embedded document")?;
        assert!(matches!(raw.data, Cow::Borrowed(_)));
        let mut tags = Vec::new();
        for encoding in ["gzip", "zstd", "identity"] {
            let response = serve(
                Request::builder()
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, encoding)
                    .body(Body::empty())?,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let tag = response.headers().get(header::ETAG).ok_or("etag")?.clone();
            tags.push(tag.clone());
            let bytes = to_bytes(response.into_body(), 65536).await?;
            let decoded = match encoding {
                "gzip" => {
                    let mut output = Vec::new();
                    flate2::read::GzDecoder::new(bytes.as_ref()).read_to_end(&mut output)?;
                    output
                }
                "zstd" => zstd::stream::decode_all(bytes.as_ref())?,
                _ => bytes.to_vec(),
            };
            assert_eq!(decoded, raw.data.as_ref());
            let cached = serve(
                Request::builder()
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, encoding)
                    .header(header::IF_NONE_MATCH, tag)
                    .body(Body::empty())?,
            )
            .await;
            assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
        }
        assert_ne!(tags[0], tags[1]);
        assert_ne!(tags[1], tags[2]);
        Ok(())
    }
}
