//! Classify SDK failures using metadata codes without retaining service messages.
use monitor_integrations::transport::Error;
/// Connector bounds and deadlines remain explicit after native SDK error wrapping.
pub fn sdk<E>(
    error: aws_smithy_runtime_api::client::result::SdkError<
        E,
        aws_smithy_runtime_api::client::orchestrator::HttpResponse,
    >,
) -> Error
where
    E: aws_smithy_types::error::metadata::ProvideErrorMetadata
        + std::error::Error
        + Send
        + Sync
        + 'static,
{
    use aws_smithy_runtime_api::client::result::SdkError;
    use std::error::Error as _;
    let mut source = error.source();
    while let Some(inner) = source {
        if let Some(error) = inner.downcast_ref::<Error>() {
            return *error;
        }
        source = inner.source();
    }
    match &error {
        SdkError::TimeoutError(_) => Error::Timeout,
        SdkError::ResponseError(_) | SdkError::ConstructionFailure(_) => Error::Malformed,
        _ => classify(error.as_service_error().and_then(|error| error.code())),
    }
}
pub fn classify(code: Option<&str>) -> Error {
    match code {
        Some(
            "ExpiredToken"
            | "ExpiredTokenException"
            | "InvalidClientTokenId"
            | "UnrecognizedClientException"
            | "InvalidIdentityToken",
        ) => Error::Authentication,
        Some(
            "AccessDenied" | "AccessDeniedException" | "UnauthorizedOperation" | "OptInRequired",
        ) => Error::Denied,
        Some(
            "Throttling"
            | "ThrottlingException"
            | "TooManyRequestsException"
            | "RequestLimitExceeded",
        ) => Error::Throttled,
        Some("ResourceNotFound" | "ResourceNotFoundException") => Error::Missing,
        Some("InvalidParameterValue" | "InvalidParameterCombination" | "ValidationException") => {
            Error::Malformed
        }
        _ => Error::Unavailable,
    }
}
