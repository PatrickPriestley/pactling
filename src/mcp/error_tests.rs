use super::error::ToolError;
use crate::broker_client::{ClientError, ReadRoute};

#[test]
fn transport_error_mapping_is_fixed_and_safe() {
    for (error, code, retryable) in [
        (ClientError::Timeout, "BROKER_TIMEOUT", true),
        (ClientError::Unavailable, "BROKER_UNAVAILABLE", true),
        (ClientError::InvalidPath, "INVALID_INPUT", false),
        (ClientError::InvalidConfig, "INVALID_INPUT", false),
        (ClientError::ResponseTooLarge, "RESPONSE_TOO_LARGE", false),
    ] {
        let result = ToolError::client(
            error,
            &ReadRoute::Integrations,
            "https://broker/integrations",
        )
        .0;
        assert_eq!(result["error"]["code"], code);
        assert_eq!(result["error"]["retryable"], retryable);
    }
}
