use serde_json::{Value, json};

fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../docs/api/deployment-explanations-v1.schema.json"
    ))
    .unwrap();
    jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap()
}

fn example(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(format!(
        "{}/tests/fixtures/pactling/explanations/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    )).unwrap()).unwrap()
}

#[test]
fn explanation_schema_accepts_examples_and_open_codes() {
    let validator = validator();
    for name in ["allowed", "blocked", "unknown"] {
        let value = example(name);
        assert!(validator.validate(&value).is_ok(), "{name}: {:?}", validator.validate(&value));
    }
    let mut value = example("unknown");
    value["reasons"][0]["code"] = json!("future.reason");
    value["nextActions"][0]["type"] = json!("future.action");
    value["nextActions"][0]["target"] = json!({"future": true});
    value["futureField"] = json!(true);
    assert!(validator.validate(&value).is_ok());
    assert!(validator.validate(&json!({"schemaVersion":"1.0", "error":{"code":"INVALID_QUERY", "details":{"errors":["bad query"]}}})).is_ok());
}

#[test]
fn explanation_schema_rejects_invalid_documents() {
    let validator = validator();
    for (pointer, invalid) in [
        ("/schemaVersion", json!("2.0")),
        ("/decision", json!("maybe")),
        ("/compatibility/deployable", json!(false)),
        ("/compatibility/success", json!(-1)),
        ("/evaluation/id", json!("not-a-uuid")),
        ("/evaluation/evaluatedAt", json!("yesterday")),
        ("/evaluation/snapshot/retained", json!(true)),
        ("/evidence/currentness", json!("current")),
        ("/resolution/target", json!({"kind":"environment"})),
        ("/resolution/candidates/0/sources/0/kind", json!("imagined")),
        ("/rows/0/contract/hash/value", json!("not-a-sha")),
        ("/rows/0/disposition", json!("excluded")),
        ("/rows/0/verification", json!({"success":true})),
        ("/reasons/0/code", json!("")),
        ("/reasons/0/role", json!("blocker")),
        ("/reasons/0/evidenceRefs", json!(["/bad~pointer"])),
    ] {
        let mut value = example("allowed");
        *value.pointer_mut(pointer).unwrap() = invalid;
        assert!(validator.validate(&value).is_err(), "accepted invalid {pointer}");
    }
    let mut value = example("allowed");
    value["error"] = json!({"code":"INVALID_QUERY","details":[]});
    assert!(validator.validate(&value).is_err(), "accepted mixed envelope");
    for key in example("allowed").as_object().unwrap().keys() {
        let mut value = example("allowed");
        value.as_object_mut().unwrap().remove(key);
        assert!(validator.validate(&value).is_err(), "accepted missing {key}");
    }
    let mut value = example("unknown");
    value["nextActions"][0]["target"] = json!({});
    assert!(validator.validate(&value).is_err(), "accepted incomplete known action");
    value["nextActions"][0]["type"] = json!("");
    assert!(validator.validate(&value).is_err(), "accepted empty action code");
}
