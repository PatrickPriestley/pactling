//! The captured Ruby corpus supplies the verdict oracle, not the explanation renderer.
mod common;

use pactling::{
    config::Config,
    db::Db,
    http::{deployment_explanations, hal::Links, matrix::document},
    service::matrix::{self, MatrixQuery},
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const COMPATIBILITY_HTTP_ELIGIBLE: &[&str] = &[
    "provider_side_ignore_wrong_consumer_version",
    "provider_side_ignore_matching_consumer_version",
    "released_versions_ignore_only_success",
    "released_versions_ignore_only_failure",
    "released_versions_ignore_whole_provider",
    "single_provider_environment",
    "to_tag_option_no_environment_warning",
    "environment_with_unverified_release",
    "environment_with_unverified_release_ignored",
    "latestby_cvp_consumer_environment",
    "latestby_cvpv_consumer_environment",
    "latestby_cp_consumer_environment",
    "latestby_none_consumer_environment",
    "latestby_cvp_ignore_new_provider",
    "latestby_cvp_ignore_old_provider",
];

const HTTP_ELIGIBLE: &[&str] = &[
    "provider_side_ignore_wrong_consumer_version",
    "provider_side_ignore_matching_consumer_version",
    "released_versions_ignore_only_success",
    "released_versions_ignore_only_failure",
    "released_versions_ignore_whole_provider",
    "single_provider_environment",
    "to_tag_option_no_environment_warning",
    "environment_with_unverified_release",
    "environment_with_unverified_release_ignored",
    "latestby_cvp_consumer_environment",
    "latestby_cvp_provider_environment",
    "latestby_cvpv_consumer_environment",
    "latestby_cvpv_provider_environment",
    "latestby_cp_consumer_environment",
    "latestby_cp_provider_environment",
    "latestby_none_consumer_environment",
    "latestby_none_provider_environment",
    "latestby_cvp_ignore_new_provider",
    "latestby_cvp_ignore_old_provider",
    "latestby_cvp_multiple_consumers",
    "latestby_cvpv_multiple_consumers",
    "latestby_cp_multiple_consumers",
    "latestby_none_multiple_consumers",
];

fn legacy_projection(doc: &Value) -> Value {
    let rows: Vec<_> = doc["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            json!({
                "consumer": row["consumer"]["name"],
                "consumerVersion": row["consumer"]["version"]["number"],
                "provider": row["provider"]["name"],
                "providerVersion": row["provider"]["version"]["number"],
                "success": row["verificationResult"]["success"],
                "ignored": row.get("ignored").cloned().unwrap_or(json!(false))
            })
        })
        .collect();
    json!({"summary":doc["summary"], "rows": rows})
}

fn check_refs(root: &Value, node: &Value, action_ids: &HashSet<&str>) {
    match node {
        Value::Object(object) => {
            for (key, value) in object {
                match key.as_str() {
                    "evidenceRefs" | "candidateRefs" | "ignoreRefs" => {
                        for reference in value.as_array().unwrap() {
                            let pointer = reference.as_str().unwrap();
                            assert!(
                                root.pointer(pointer).is_some(),
                                "unresolved {key}: {pointer}"
                            );
                        }
                    }
                    "selectedRowRef" if !value.is_null() => {
                        let pointer = value.as_str().unwrap();
                        assert!(
                            pointer.starts_with("/rows/"),
                            "not a selected row: {pointer}"
                        );
                        assert!(
                            root.pointer(pointer).is_some(),
                            "unresolved selected row: {pointer}"
                        );
                    }
                    "actionIds" => {
                        for id in value.as_array().unwrap() {
                            assert!(
                                action_ids.contains(id.as_str().unwrap()),
                                "unresolved action {id}"
                            );
                        }
                    }
                    _ => {}
                }
                check_refs(root, value, action_ids);
            }
        }
        Value::Array(values) => {
            for value in values {
                check_refs(root, value, action_ids);
            }
        }
        _ => {}
    }
}

fn check_explanation(doc: &Value, expected: &Value, validator: &jsonschema::Validator, name: &str) {
    assert!(
        validator.is_valid(doc),
        "{name}: schema errors: {:?}",
        validator
            .iter_errors(doc)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    let summary = &expected["summary"];
    assert_eq!(
        doc["decision"],
        match summary["deployable"].as_bool() {
            Some(true) => "allowed",
            Some(false) => "blocked",
            None => "unknown",
        },
        "{name}"
    );
    for field in ["deployable", "success", "failed", "unknown"] {
        assert_eq!(
            doc["compatibility"][field], summary[field],
            "{name}: {field}"
        );
    }
    assert_eq!(
        doc["compatibility"]["ignored"],
        summary.get("ignored").cloned().unwrap_or(json!(0)),
        "{name}"
    );
    assert_eq!(
        doc["compatibility"]["hasIgnores"],
        summary.get("ignored").is_some(),
        "{name}"
    );
    let rows = doc["rows"].as_array().unwrap();
    let mut ids = HashSet::new();
    let projected: Vec<_> = rows
        .iter()
        .map(|row| {
            assert!(
                ids.insert(row["id"].as_str().unwrap()),
                "{name}: duplicate row id"
            );
            json!({
                "consumer": row["consumer"], "consumerVersion":row["consumerVersion"],
                "provider":row["provider"], "providerVersion":row["providerVersion"],
                "success":row["verification"]["success"], "ignored":row["disposition"] == "ignored"
            })
        })
        .collect();
    assert_eq!(
        json!(projected),
        expected["rows"],
        "{name}: row/null/verification projection"
    );
    let actions = doc["nextActions"].as_array().unwrap();
    let action_ids: HashSet<_> = actions
        .iter()
        .map(|action| action["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        action_ids.len(),
        actions.len(),
        "{name}: duplicate action id"
    );
    check_refs(doc, doc, &action_ids);
}

#[tokio::test]
async fn all_108_captured_decisions_share_one_evaluation_and_original_http_grammar() {
    let path = std::env::temp_dir().join(format!("explanation-corpus-{}.db", uuid::Uuid::new_v4()));
    let mut config = Config::in_memory();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Db::connect(&config.database_url).await.unwrap();
    let server = common::start_with(config).await;
    let links = Links::new(&server.base_url);
    let schema: Value = serde_json::from_str(include_str!(
        "../docs/api/deployment-explanations-v1.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap();
    let capture: Value = serde_json::from_str(include_str!(
        "fixtures/ruby-broker/2.121.2/phase4/matrix-regressions.json"
    ))
    .unwrap();
    let mut variables = HashMap::<String, String>::new();
    for op in capture["setup"].as_array().unwrap() {
        let mut path = op["path"].as_str().unwrap().to_owned();
        for (name, value) in &variables {
            path = path.replace(&format!("{{{name}}}"), value);
        }
        let response = server
            .client
            .request(
                op["method"].as_str().unwrap().parse().unwrap(),
                format!("{}{path}", server.base_url),
            )
            .json(&op["body"])
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "setup {path}: {}",
            response.status()
        );
        if let Some(name) = op["saveUuid"].as_str() {
            let body: Value = response.json().await.unwrap();
            variables.insert(name.to_owned(), body["uuid"].as_str().unwrap().to_owned());
        }
    }
    let cases = capture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 108);
    let mut accepted = Vec::new();
    let mut rejected = 0;
    let mut counts = [0; 3];
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let path = case["request"]["path"].as_str().unwrap();
        let (route, raw) = path.split_once('?').unwrap();
        let query = if route.starts_with("/matrix/provider/") {
            let parts: Vec<_> = route.split('/').collect();
            // This compatibility handler ignores the query and selects the path pair.
            MatrixQuery::parse(Some(&format!(
                "pacticipant={}&pacticipant={}",
                parts[5], parts[3]
            )))
        } else {
            MatrixQuery::parse(Some(raw))
        };
        // Exactly one decision supplies both pure renderers. No independent re-evaluation.
        let evaluation = matrix::decision(&db, &query).await.unwrap();
        let legacy = document(&links, &evaluation);
        assert_eq!(
            legacy_projection(&legacy),
            case["expected"],
            "{name}: pure legacy"
        );
        let explanation = deployment_explanations::document(&links, &evaluation, raw);
        check_explanation(&explanation, &case["expected"], &validator, name);
        let html = pactling::http::ui::explanation::render_document(&explanation).into_string();
        assert!(
            html.contains(&format!(
                "data-decision=\"{}\"",
                explanation["decision"].as_str().unwrap()
            )),
            "{name}: HTML verdict"
        );
        assert_eq!(
            html.matches("data-consumer-version=").count(),
            explanation["rows"].as_array().unwrap().len(),
            "{name}: HTML rows"
        );
        for item in explanation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .chain(explanation["nextActions"].as_array().unwrap())
        {
            for pointer in item["evidenceRefs"].as_array().unwrap() {
                assert!(
                    html.contains(&format!("id=\"{}\"", pointer.as_str().unwrap())),
                    "{name}: absent HTML evidence anchor {pointer}"
                );
            }
        }
        let mut offset = 0;
        for row in explanation["rows"].as_array().unwrap() {
            let marker = format!(
                "data-consumer-version=\"{}\" data-provider-version=\"{}\"",
                row["consumerVersion"].as_str().unwrap_or(""),
                row["providerVersion"].as_str().unwrap_or("")
            );
            let pos = html[offset..]
                .find(&marker)
                .unwrap_or_else(|| panic!("{name}: missing {marker}"));
            offset += pos + marker.len();
        }

        assert_eq!(explanation["request"]["query"], raw);
        assert_eq!(
            explanation,
            deployment_explanations::document(&links, &evaluation, raw),
            "{name}: stable projection, including row ids and evaluation metadata"
        );
        counts[match evaluation.matrix().deployable {
            Some(true) => 0,
            Some(false) => 1,
            None => 2,
        }] += 1;

        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{name}: original captured route");
        let original: Value = response.json().await.unwrap();
        assert_eq!(
            legacy_projection(&original),
            case["expected"],
            "{name}: original HTTP unchanged"
        );

        // Use each original raw query verbatim: integration path selectors are NOT
        // invented for either HTTP route, and missing targets are NOT repaired.
        // The extension accepts unpinned matrix selectors; compatibility still requires a pin.
        let compatibility = server
            .client
            .get(format!("{}/can-i-deploy?{raw}", server.base_url))
            .send()
            .await
            .unwrap();
        let response = server
            .client
            .get(format!(
                "{}/pactling/v1/can-i-deploy/explanation?{raw}",
                server.base_url
            ))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let compatibility_status = compatibility.status();
        let mut compatibility: Value = compatibility.json().await.unwrap();
        if COMPATIBILITY_HTTP_ELIGIBLE.contains(&name) {
            assert_eq!(
                compatibility_status, 200,
                "{name}: legacy acceptance unchanged"
            );
            assert_eq!(
                legacy_projection(&compatibility),
                case["expected"],
                "{name}: can-i-deploy oracle"
            );
        } else {
            assert_eq!(
                compatibility_status, 400,
                "{name}: legacy rejection unchanged"
            );
        }
        let explanation: Value = response.json().await.unwrap();
        if status.is_success() {
            accepted.push(name);
            assert!(
                HTTP_ELIGIBLE.contains(&name),
                "unexpected eligible case {name}"
            );
            check_explanation(&explanation, &case["expected"], &validator, name);
        } else {
            rejected += 1;
            assert_eq!(status, 400, "{name}");
            assert_eq!(explanation["error"]["code"], "INVALID_QUERY", "{name}");
            if let Some(errors) = compatibility["errors"].as_object_mut() {
                errors.remove("version");
            }
            assert_eq!(
                explanation["error"]["details"], compatibility["errors"],
                "{name}: validation parity"
            );
            assert!(
                validator.is_valid(&explanation),
                "{name}: invalid error schema"
            );
            assert!(
                explanation.get("decision").is_none(),
                "{name}: validation is not a verdict"
            );
        }
    }
    assert_eq!(counts, [39, 51, 18]);
    assert_eq!(accepted, HTTP_ELIGIBLE);
    assert_eq!(rejected, 85);
    eprintln!(
        "Explanation HTTP corpus coverage: {}/108 accepted original queries: {}",
        accepted.len(),
        accepted.join(", ")
    );
    db.pool().close().await;
    std::fs::remove_file(path).unwrap();
}
