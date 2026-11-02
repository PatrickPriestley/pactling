#![cfg(feature = "mcp")]
use serde_json::{Value, json};
fn read(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
#[test]
fn mcp_schema_survivors_have_invalid_witnesses() {
    let tools = [
        "explain_deployment",
        "get_pact",
        "list_integrations",
        "list_pacticipants",
        "matrix",
    ];
    for tool in tools {
        for kind in ["input", "output"] {
            let schema = read(&format!("docs/api/mcp/pactling_{tool}.{kind}.schema.json"));
            assert_eq!(
                schema["type"], "object",
                "MCP root schemas must advertise object type"
            );
        }
        let schema = read(&format!("docs/api/mcp/pactling_{tool}.output.schema.json"));
        let validator = jsonschema::validator_for(&schema).unwrap();
        let error = json!({"adapterSchemaVersion":"1.0","error":{"code":"INVALID_INPUT","message":"Fixed safe text","retryable":false,"httpStatus":400,"evidenceUrl":"https://broker/source","details":{"reasonCode":"SCHEMA_VALIDATION","field":"/selector"}}});
        assert!(validator.is_valid(&error));
        for path in [
            "/adapterSchemaVersion",
            "/error",
            "/error/code",
            "/error/message",
            "/error/retryable",
            "/error/httpStatus",
            "/error/evidenceUrl",
            "/error/details",
            "/error/details/reasonCode",
            "/error/details/field",
        ] {
            let mut bad = error.clone();
            *bad.pointer_mut(path).unwrap() = json!([]);
            assert!(!validator.is_valid(&bad), "{tool} {path}");
        }
        let mut bad = error.clone();
        bad["adapterSchemaVersion"] = json!("2.0");
        assert!(!validator.is_valid(&bad));
        for (path, key) in [
            ("", "adapterSchemaVersion"),
            ("", "error"),
            ("/error/details", "reasonCode"),
        ] {
            let mut bad = error.clone();
            bad.pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(!validator.is_valid(&bad));
        }
        for path in ["", "/error", "/error/details"] {
            let mut bad = error.clone();
            bad.pointer_mut(path).unwrap()["extra"] = json!(true);
            assert!(!validator.is_valid(&bad));
        }
        let data = match tool {
            "get_pact" => json!({}),
            "explain_deployment" => read("tests/fixtures/pactling/explanations/allowed.json"),
            "matrix" => {
                read("tests/fixtures/ruby-broker/2.121.2/phase4/GET_matrix_integration.json")
            }
            "list_integrations" => {
                json!({"items":[{"consumer":"C","provider":"P","evidenceLinks":[{"rel":"matrix","href":"https://broker/matrix"}]}],"nextCursor":null,"retrievedAt":"2026-11-02T01:00:00Z"})
            }
            _ => {
                json!({"items":[{"name":"C","href":"https://broker/pacticipants/C","latestVersion":null}],"nextCursor":null,"retrievedAt":"2026-11-02T01:00:00Z"})
            }
        };
        let success = json!({"adapterSchemaVersion":"1.0","coverage":"recorded_only","data":data,"evidenceLinks":[{"rel":"source","href":"https://broker/source"}]});
        assert!(validator.is_valid(&success));
        let mut bad = success.clone();
        bad["evidenceLinks"][0] = Value::Null;
        assert!(!validator.is_valid(&bad));
        if tool.starts_with("list_") {
            let mut bad = success.clone();
            bad["data"]["items"][0] = Value::Null;
            assert!(!validator.is_valid(&bad));
        }
        if tool == "list_integrations" {
            let path = "/data/items/0/evidenceLinks/0";
            let mut bad = success.clone();
            *bad.pointer_mut(path).unwrap() = Value::Null;
            assert!(!validator.is_valid(&bad));
            for key in ["rel", "href"] {
                let mut bad = success.clone();
                bad.pointer_mut(path).unwrap()[key] = json!(1);
                assert!(!validator.is_valid(&bad));
                let mut bad = success.clone();
                bad.pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(!validator.is_valid(&bad));
            }
            let mut bad = success.clone();
            bad.pointer_mut(path).unwrap()["extra"] = json!(true);
            assert!(!validator.is_valid(&bad));
        }
        if tool == "matrix" {
            for path in ["/data/matrix/0", "/data/notices/0"] {
                let mut bad = success.clone();
                *bad.pointer_mut(path).unwrap() = Value::Null;
                assert!(!validator.is_valid(&bad));
            }
        }
        if tool == "explain_deployment" {
            let mut bad = success.clone();
            bad["data"] =
                json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":[]}});
            assert!(!validator.is_valid(&bad));
            for version in ["2.0", "1.x"] {
                let mut bad = success.clone();
                bad["data"]["schemaVersion"] = json!(version);
                assert!(!validator.is_valid(&bad));
            }
        }
    }
    let schema = read("docs/api/mcp/pactling_explain_deployment.input.schema.json");
    let validator = jsonschema::validator_for(&schema).unwrap();
    let valid = json!({"pacticipant":"C","selector":{"kind":"mainBranch"},"target":{"kind":"tag","name":"t"}});
    for (field, value) in [
        ("selector", json!(null)),
        ("target", json!(null)),
        ("ignores", json!([null])),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(!validator.is_valid(&bad));
    }
}
