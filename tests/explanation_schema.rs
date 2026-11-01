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
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/fixtures/pactling/explanations/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn explanation_schema_accepts_examples_and_open_codes() {
    let validator = validator();
    for name in ["allowed", "blocked", "unknown"] {
        let value = example(name);
        assert!(
            validator.validate(&value).is_ok(),
            "{name}: {:?}",
            validator.validate(&value)
        );
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
        assert!(
            validator.validate(&value).is_err(),
            "accepted invalid {pointer}"
        );
    }
    for (name, deployable) in [("blocked", json!(true)), ("unknown", json!(false))] {
        let mut value = example(name);
        value["compatibility"]["deployable"] = deployable;
        assert!(
            validator.validate(&value).is_err(),
            "accepted inconsistent {name} verdict"
        );
    }
    let mut value = example("allowed");
    value["error"] = json!({"code":"INVALID_QUERY","details":[]});
    assert!(
        validator.validate(&value).is_err(),
        "accepted mixed envelope"
    );
    for key in example("allowed").as_object().unwrap().keys() {
        let mut value = example("allowed");
        value.as_object_mut().unwrap().remove(key);
        assert!(
            validator.validate(&value).is_err(),
            "accepted missing {key}"
        );
    }
    let mut value = example("unknown");
    value["nextActions"][0]["target"] = json!({});
    assert!(
        validator.validate(&value).is_err(),
        "accepted incomplete known action"
    );
    value["nextActions"][0]["type"] = json!("");
    assert!(
        validator.validate(&value).is_err(),
        "accepted empty action code"
    );
}

#[test]
fn explanation_schema_requires_nested_evidence_and_action_fields() {
    fn omissions(value: &Value, path: &str, output: &mut Vec<(String, String)>) {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    output.push((path.to_owned(), key.clone()));
                    omissions(child, &format!("{path}/{key}"), output);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    omissions(child, &format!("{path}/{index}"), output);
                }
            }
            _ => {}
        }
    }
    let validator = validator();
    for name in ["allowed", "blocked", "unknown"] {
        let original = example(name);
        let mut paths = vec![];
        omissions(&original, "", &mut paths);
        for (path, key) in paths {
            let mut value = original.clone();
            value
                .pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&key);
            assert!(
                validator.validate(&value).is_err(),
                "accepted missing {path}/{key}"
            );
        }
    }
}

#[test]
fn explanation_schema_accepts_resolution_and_action_variants() {
    let validator = validator();
    for target in [
        json!({"kind":"any"}),
        json!({"kind":"latest","tag":null}),
        json!({"kind":"selected_versions","selectorIndices":[0,1]}),
    ] {
        let mut value = example("unknown");
        value["resolution"]["target"] = target;
        assert!(validator.validate(&value).is_ok());
    }
    for (kind, target) in [
        (
            "RESOLVE_SELECTOR",
            json!({"selectorIndex":0,"requested":example("unknown")["resolution"]["selectors"][0]["requested"]}),
        ),
        (
            "CORRECT_IGNORE_REQUEST",
            json!({"ignoreIndex":0,"pacticipant":"Missing","version":null}),
        ),
        (
            "RESOLVE_TARGET_VERSION",
            json!({"consumer":"Checkout","consumerVersion":null,"provider":"Payments","providerVersion":null,"contractSha":"1234567890abcdef1234567890abcdef12345678","environment":"production","tag":null}),
        ),
    ] {
        let mut value = example("unknown");
        value["nextActions"][0]["type"] = json!(kind);
        value["nextActions"][0]["target"] = target.clone();
        assert!(validator.validate(&value).is_ok(), "rejected {kind}");
        for key in target.as_object().unwrap().keys() {
            let mut incomplete = value.clone();
            incomplete["nextActions"][0]["target"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(
                validator.validate(&incomplete).is_err(),
                "accepted {kind} without {key}"
            );
        }
    }
    let mut value = example("allowed");
    let mut excluded_row = value["rows"][0].clone();
    excluded_row["id"] = json!("excluded-1");
    excluded_row["disposition"] = json!("excluded");
    value["resolution"]["excluded"] = json!([{"kind":"latestby", "evidence":excluded_row,"candidateRefs":["/resolution/candidates/0"],"selectorIndices":[0],"selectedRowRef":"/rows/0"}]);
    value["resolution"]["ignores"] =
        json!([{"index":0,"pacticipant":"Absent","version":null,"status":"participant_missing"}]);
    assert!(validator.validate(&value).is_ok());
    for key in ["index", "pacticipant", "version", "status"] {
        let mut incomplete = value.clone();
        incomplete["resolution"]["ignores"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(
            validator.validate(&incomplete).is_err(),
            "accepted ignore without {key}"
        );
    }
    for key in [
        "kind",
        "evidence",
        "candidateRefs",
        "selectorIndices",
        "selectedRowRef",
    ] {
        let mut incomplete = value.clone();
        incomplete["resolution"]["excluded"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(
            validator.validate(&incomplete).is_err(),
            "accepted exclusion without {key}"
        );
    }
}
