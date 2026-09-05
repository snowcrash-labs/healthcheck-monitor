//! Classify SDK failures using metadata codes without retaining service messages.
use monitor_integrations::transport::Error;
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
