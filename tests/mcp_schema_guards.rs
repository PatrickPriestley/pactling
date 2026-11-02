#![cfg(feature = "mcp")]
use serde_json::{Value, json};
fn schema(tool: &str, kind: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!("docs/api/mcp/pactling_{tool}.{kind}.schema.json"))
            .unwrap(),
    )
    .unwrap()
}
fn valid(tool: &str, args: &Value) {
    assert!(
        jsonschema::is_valid(&schema(tool, "input"), args),
        "valid {tool}: {args}"
    );
}
fn invalid(tool: &str, args: &Value) {
    assert!(
        !jsonschema::is_valid(&schema(tool, "input"), args),
        "invalid {tool}: {args}"
    );
}
#[test]
fn mcp_input_schema_guards_match_contract() {
    let bases = [
        (
            "get_pact",
            json!({"consumer":"C","provider":"P","consumerVersion":"1"}),
        ),
        ("matrix", json!({"participantA":"A","participantB":"B"})),
        (
            "explain_deployment",
            json!({"pacticipant":"C","selector":{"kind":"version","version":"1"},"target":{"kind":"environment","name":"test"}}),
        ),
    ];
    for (tool, base) in &bases {
        valid(tool, base);
        let mut a = base.clone();
        a["unknown"] = json!(true);
        invalid(tool, &a);
        for key in base.as_object().unwrap().keys() {
            let mut a = base.clone();
            a.as_object_mut().unwrap().remove(key);
            invalid(tool, &a);
        }
        for (key, val) in base.as_object().unwrap() {
            if val.is_string() {
                for bad in [json!(""), json!("x".repeat(1025)), json!(null)] {
                    let mut a = base.clone();
                    a[key] = bad;
                    invalid(tool, &a);
                }
            }
        }
    }
    for tool in ["list_integrations", "list_pacticipants"] {
        for a in [
            json!({}),
            json!({"pageSize":1}),
            json!({"pageSize":100}),
            json!({"pageSize":1.0}),
            json!({"cursor":"abc"}),
        ] {
            valid(tool, &a);
        }
        for a in [
            json!({"unknown":true}),
            json!({"pageSize":0}),
            json!({"pageSize":101}),
            json!({"pageSize":1.5}),
            json!({"pageSize":"1"}),
            json!({"cursor":""}),
            json!({"cursor":1}),
            json!({"cursor":"c","pageSize":1}),
        ] {
            invalid(tool, &a);
        }
    }
    let (tool, base) = &bases[2];
    for selector in [
        json!({"kind":"version","version":"1"}),
        json!({"kind":"latest"}),
        json!({"kind":"latest","tag":"dev"}),
        json!({"kind":"branch","branch":"main"}),
        json!({"kind":"mainBranch"}),
    ] {
        let mut a = base.clone();
        a["selector"] = selector.clone();
        valid(tool, &a);
        let mut b = a.clone();
        b["selector"]["unknown"] = json!(true);
        invalid(tool, &b);
        let mut b = a.clone();
        b["selector"]["kind"] = json!("other");
        invalid(tool, &b);
        let mut b = a.clone();
        b["selector"].as_object_mut().unwrap().remove("kind");
        invalid(tool, &b);
        for key in ["version", "branch", "tag"] {
            if selector.get(key).is_some() {
                for bad in [json!(""), json!("x".repeat(1025)), json!(1)] {
                    let mut b = a.clone();
                    b["selector"][key] = bad;
                    invalid(tool, &b);
                }
                if key != "tag" {
                    let mut b = a.clone();
                    b["selector"].as_object_mut().unwrap().remove(key);
                    invalid(tool, &b);
                }
            }
        }
    }
    for kind in ["environment", "tag"] {
        let mut a = base.clone();
        a["target"] = json!({"kind":kind,"name":"t"});
        valid(tool, &a);
        for (key, value) in [
            ("kind", json!("other")),
            ("name", json!("")),
            ("name", json!("x".repeat(1025))),
            ("name", json!(7)),
            ("unknown", json!(true)),
        ] {
            let mut b = a.clone();
            b["target"][key] = value;
            invalid(tool, &b);
        }
        for key in ["kind", "name"] {
            let mut b = a.clone();
            b["target"].as_object_mut().unwrap().remove(key);
            invalid(tool, &b);
        }
    }
    for ignore in [
        json!({"pacticipant":"P"}),
        json!({"pacticipant":"P","version":"1"}),
    ] {
        let mut a = base.clone();
        a["ignores"] = json!([ignore.clone()]);
        valid(tool, &a);
        let mut b = a.clone();
        b["ignores"][0]["unknown"] = json!(true);
        invalid(tool, &b);
        let mut b = a.clone();
        b["ignores"][0]
            .as_object_mut()
            .unwrap()
            .remove("pacticipant");
        invalid(tool, &b);
        for key in ignore.as_object().unwrap().keys() {
            for bad in [json!(""), json!("x".repeat(1025)), json!(7)] {
                let mut b = a.clone();
                b["ignores"][0][key] = bad;
                invalid(tool, &b);
            }
        }
    }
    let mut a = base.clone();
    a["ignores"] = json!(vec![json!({"pacticipant":"P"}); 101]);
    invalid(tool, &a);
    a["ignores"] = json!({});
    invalid(tool, &a);
    let mut a = bases[1].1.clone();
    a["latestby"] = json!("bad");
    invalid("matrix", &a);
    for tool in [
        "get_pact",
        "matrix",
        "explain_deployment",
        "list_integrations",
        "list_pacticipants",
    ] {
        invalid(tool, &json!([]));
    }
}
#[test]
fn mcp_output_schema_guards_match_contract() {
    let read =
        |p: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(p).unwrap()).unwrap();
    let fixtures = [
        ("get_pact", json!({"plugin":{"arbitrary":true}})),
        (
            "matrix",
            read("tests/fixtures/ruby-broker/2.121.2/phase4/GET_matrix_integration.json"),
        ),
        (
            "explain_deployment",
            read("tests/fixtures/pactling/explanations/allowed.json"),
        ),
        (
            "list_integrations",
            json!({"items":[{"consumer":"C","provider":"P","evidenceLinks":[{"rel":"matrix","href":"https://broker/matrix"}]}],"nextCursor":null,"retrievedAt":"2026-11-02T01:00:00Z"}),
        ),
        (
            "list_pacticipants",
            json!({"items":[{"name":"C","href":"https://broker/pacticipants/C","latestVersion":"1"}],"nextCursor":null,"retrievedAt":"2026-11-02T01:00:00Z"}),
        ),
    ];
    for (tool, data) in fixtures {
        let validator = jsonschema::validator_for(&schema(tool, "output")).unwrap();
        let envelope = json!({"adapterSchemaVersion":"1.0","data":data,"coverage":"recorded_only","evidenceLinks":[{"rel":"source","href":"https://broker/source"}]});
        assert!(validator.is_valid(&envelope));
        for key in ["adapterSchemaVersion", "data", "coverage", "evidenceLinks"] {
            let mut bad = envelope.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(!validator.is_valid(&bad), "{tool} missing {key}");
        }
        for (key, value) in [
            ("adapterSchemaVersion", json!("2.0")),
            ("coverage", json!("complete")),
            ("evidenceLinks", json!({})),
            ("data", json!([])),
            ("extra", json!(true)),
        ] {
            let mut bad = envelope.clone();
            bad[key] = value;
            assert!(!validator.is_valid(&bad), "{tool} invalid {key}");
        }
        for key in ["rel", "href"] {
            let mut bad = envelope.clone();
            bad["evidenceLinks"][0].as_object_mut().unwrap().remove(key);
            assert!(!validator.is_valid(&bad));
            let mut bad = envelope.clone();
            bad["evidenceLinks"][0][key] = json!(5);
            assert!(!validator.is_valid(&bad));
        }
        let mut bad = envelope.clone();
        bad["evidenceLinks"][0]["extra"] = json!(true);
        assert!(!validator.is_valid(&bad));
        if tool.starts_with("list_") {
            for key in ["items", "nextCursor", "retrievedAt"] {
                let mut bad = envelope.clone();
                bad["data"].as_object_mut().unwrap().remove(key);
                assert!(!validator.is_valid(&bad));
            }
            for (key, val) in [
                ("items", json!({})),
                ("nextCursor", json!(1)),
                ("retrievedAt", json!(5)),
                ("extra", json!(true)),
            ] {
                let mut bad = envelope.clone();
                bad["data"][key] = val;
                assert!(!validator.is_valid(&bad));
            }
            let item = &envelope["data"]["items"][0];
            for key in item.as_object().unwrap().keys() {
                let mut bad = envelope.clone();
                bad["data"]["items"][0].as_object_mut().unwrap().remove(key);
                assert!(!validator.is_valid(&bad));
                let mut bad = envelope.clone();
                bad["data"]["items"][0][key] = json!(5);
                assert!(!validator.is_valid(&bad));
            }
            let mut bad = envelope.clone();
            bad["data"]["items"][0]["extra"] = json!(true);
            assert!(!validator.is_valid(&bad));
        }
        if tool == "matrix" {
            for pointer in [
                "/data",
                "/data/summary",
                "/data/matrix/0",
                "/data/matrix/0/consumer",
                "/data/matrix/0/provider",
                "/data/notices/0",
            ] {
                for key in match pointer {
                    "/data" => vec!["summary", "notices", "matrix"],
                    "/data/summary" => vec!["deployable", "reason", "success", "failed", "unknown"],
                    "/data/matrix/0" => vec!["consumer", "provider", "pact", "verificationResult"],
                    "/data/notices/0" => vec!["type", "text"],
                    _ => vec!["name", "version"],
                } {
                    let mut bad = envelope.clone();
                    bad.pointer_mut(pointer)
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove(key);
                    assert!(!validator.is_valid(&bad), "missing {pointer}/{key}");
                    let mut bad = envelope.clone();
                    bad.pointer_mut(pointer).unwrap()[key] =
                        if pointer == "/data" && matches!(key, "matrix" | "notices") {
                            json!({})
                        } else {
                            json!([])
                        };
                    assert!(!validator.is_valid(&bad), "invalid {pointer}/{key}");
                }
            }
            for key in ["success", "failed", "unknown"] {
                let mut bad = envelope.clone();
                bad["data"]["summary"][key] = json!(-1);
                assert!(!validator.is_valid(&bad));
            }
        }
        let err = json!({"adapterSchemaVersion":"1.0","error":{"code":"AUTH_REQUIRED","message":"Broker authentication failed.","retryable":false}});
        assert!(validator.is_valid(&err));
        for key in ["code", "message", "retryable"] {
            let mut bad = err.clone();
            bad["error"].as_object_mut().unwrap().remove(key);
            assert!(!validator.is_valid(&bad));
        }
    }
}
