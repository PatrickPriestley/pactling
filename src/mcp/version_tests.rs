use super::error::ToolError;
use crate::broker_client::{ExplanationQuery, ReadRoute, Selector, Target};
use serde_json::json;

#[test]
fn v1_error_minor_versions_and_malformed_versions_have_safe_details() {
    let route = ReadRoute::Explanation(ExplanationQuery {
        pacticipant: "C".into(),
        selector: Selector::MainBranch,
        target: Target::Environment("test".into()),
        ignores: vec![],
    });
    for (version, recognized) in [
        (json!("1.0"), true),
        (json!("1.9"), true),
        (json!("1.100"), true),
        (json!("2.0"), false),
        (json!("1.x"), false),
        (json!("1.9suffix"), false),
        (json!(""), false),
        (json!(null), false),
        (json!(1), false),
    ] {
        let body = serde_json::to_vec(&json!({"schemaVersion":version,"error":{"code":"INVALID_QUERY","details":["Environment with name 'test' does not exist"]}})).unwrap();
        let result = ToolError::http(400, &body, &route).0;
        assert_eq!(result["error"]["code"], "INVALID_INPUT");
        assert_eq!(
            result["error"]["details"],
            if recognized {
                json!({"reasonCode":"ENVIRONMENT_NOT_FOUND","field":"/target/name"})
            } else {
                json!({"reasonCode":"BROKER_REJECTED_QUERY"})
            }
        );
    }
}
