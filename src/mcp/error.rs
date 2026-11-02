//! Safe adapter-owned messages: no transport diagnostics or upstream text is reflected.
use crate::broker_client::{ClientError, ReadRoute, Target};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ToolError(pub Value);
impl ToolError {
    pub fn new(code: &str, message: &str, retryable: bool) -> Self {
        Self(
            json!({"adapterSchemaVersion":"1.0","error":{"code":code,"message":message,"retryable":retryable}}),
        )
    }
    pub fn invalid_input(field: &str) -> Self {
        let mut error = Self::new(
            "INVALID_INPUT",
            "Tool arguments do not match the input schema.",
            false,
        );
        error.0["error"]["details"] = json!({"reasonCode":"SCHEMA_VALIDATION","field":field});
        error
    }
    pub fn invalid_response() -> Self {
        Self::new(
            "INVALID_BROKER_RESPONSE",
            "Broker response does not match the expected document.",
            false,
        )
    }
    pub fn too_large(route: &ReadRoute, source: &str) -> Self {
        let message = match route {
            ReadRoute::Matrix { latestby: None, .. } => {
                "Retry with latestby=cp to request the broker's grouped matrix"
            }
            _ => {
                "Open the source in an authorized broker client; this adapter cannot return the complete document within its size limit"
            }
        };
        let mut error = Self::new("RESPONSE_TOO_LARGE", message, false);
        error.0["error"]["evidenceUrl"] = json!(source);
        error
    }
    pub fn client(error: ClientError, route: &ReadRoute, source: &str) -> Self {
        match error {
            ClientError::InvalidPath | ClientError::InvalidConfig => Self::invalid_input(""),
            ClientError::Timeout => Self::new("BROKER_TIMEOUT", "Broker request timed out.", true),
            ClientError::Unavailable => {
                Self::new("BROKER_UNAVAILABLE", "Broker is unavailable.", true)
            }
            ClientError::ResponseTooLarge => Self::too_large(route, source),
        }
    }
    pub fn http(status: u16, body: &[u8], route: &ReadRoute) -> Self {
        let document: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let mut error = match status {
            400 | 422 => {
                let mut e = Self::new("INVALID_INPUT", "Broker rejected the query.", false);
                let recognized = match route {
                    ReadRoute::Explanation(query) => match &query.target {
                        Target::Environment(name) => {
                            super::schema::supported_explanation_version(&document["schemaVersion"])
                                && document["error"]["code"] == "INVALID_QUERY"
                                && document["error"]["details"]
                                    == json!([format!(
                                        "Environment with name '{name}' does not exist"
                                    )])
                        }
                        _ => false,
                    },
                    _ => false,
                };
                e.0["error"]["details"] = if recognized {
                    json!({"reasonCode":"ENVIRONMENT_NOT_FOUND","field":"/target/name"})
                } else {
                    json!({"reasonCode":"BROKER_REJECTED_QUERY"})
                };
                e
            }
            401 => Self::new("AUTH_REQUIRED", "Broker authentication failed.", false),
            403 => Self::new("FORBIDDEN", "Broker access is forbidden.", false),
            404 if matches!(route, ReadRoute::Explanation(_)) => Self::new(
                "BROKER_CAPABILITY_UNAVAILABLE",
                "Configured broker does not expose the pactling explanation route",
                false,
            ),
            404 if recognized_not_found(&document) => {
                Self::new("NOT_FOUND", "Broker route or resource not found", false)
            }
            429 => Self::new("RATE_LIMITED", "Broker rate limit exceeded.", true),
            500..=599 => Self::new("BROKER_UNAVAILABLE", "Broker is unavailable.", true),
            300..=399 => Self::new(
                "BROKER_HTTP_ERROR",
                "Configured broker base URL redirected",
                false,
            ),
            414 | 431 => Self::new(
                "BROKER_HTTP_ERROR",
                "Broker or proxy rejected the request size; reduce selectors or ignores",
                false,
            ),
            _ => Self::new(
                "BROKER_HTTP_ERROR",
                "Broker returned an unexpected HTTP status.",
                false,
            ),
        };
        error.0["error"]["httpStatus"] = json!(status);
        error
    }
}
fn recognized_not_found(document: &Value) -> bool {
    match document.get("error").and_then(Value::as_str) {
        Some("The requested document was not found on this server.") => true,
        Some(message) => {
            message.starts_with("No pacticipant with name '") && message.ends_with("' found")
        }
        None => false,
    }
}
