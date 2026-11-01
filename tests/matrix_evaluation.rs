//! Characterize the existing decision seam before extracting its pure evaluator.
mod common;
use pactling::{
    config::Config,
    db::Db,
    service::matrix::{self, Matrix, MatrixQuery},
};
use serde_json::{Value, json};

fn projection(m: &Matrix) -> Value {
    let mut summary = json!({
        "deployable": m.deployable,
        "reason": m.notices.iter().map(|n| n.text.as_str()).collect::<Vec<_>>().join("\n"),
        "success": m.success, "failed": m.failed, "unknown": m.unknown
    });
    if m.has_ignores {
        summary["ignored"] = json!(m.ignored);
    }
    let rows: Vec<_> = m
        .rows
        .iter()
        .map(|r| {
            json!({
                "consumer": r.consumer.pacticipant.name,
                "consumerVersion": r.consumer.version.as_ref().map(|v| &v.number),
                "provider": r.provider.pacticipant.name,
                "providerVersion": r.provider.version.as_ref().map(|v| &v.number),
                "success": r.verification.as_ref().map(|v| v.success), "ignored": r.ignored
            })
        })
        .collect();
    json!({"summary": summary, "rows": rows})
}

#[tokio::test]
async fn all_108_matrix_decisions_are_characterized() {
    let path = std::env::temp_dir().join(format!("matrix-evaluation-{}.db", uuid::Uuid::new_v4()));
    let mut config = Config::in_memory();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Db::connect(&config.database_url).await.unwrap();
    let s = common::start_with(config).await;
    let capture: Value = serde_json::from_str(include_str!(
        "fixtures/ruby-broker/2.121.2/phase4/matrix-regressions.json"
    ))
    .unwrap();
    let mut variables = std::collections::HashMap::<String, String>::new();
    for op in capture["setup"].as_array().unwrap() {
        let mut path = op["path"].as_str().unwrap().to_owned();
        for (name, value) in &variables {
            path = path.replace(&format!("{{{name}}}"), value);
        }
        let response = s
            .client
            .request(
                op["method"].as_str().unwrap().parse().unwrap(),
                format!("{}{path}", s.base_url),
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
    let mut counts = [0; 3];
    for case in cases {
        let path = case["request"]["path"].as_str().unwrap();
        let (route, raw) = path.split_once('?').unwrap();
        let query = if route.starts_with("/matrix/provider/") {
            let parts: Vec<_> = route.split('/').collect();
            // The integration handler builds path selectors and ignores RawQuery.
            MatrixQuery::parse(Some(&format!(
                "pacticipant={}&pacticipant={}",
                parts[5], parts[3]
            )))
        } else {
            MatrixQuery::parse(Some(raw))
        };
        let result = matrix::matrix(&db, &query)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}; query={query:?}", case["name"]));
        assert_eq!(projection(&result), case["expected"], "{}", case["name"]);
        counts[match result.deployable {
            Some(true) => 0,
            Some(false) => 1,
            None => 2,
        }] += 1;
    }
    assert_eq!(counts, [39, 51, 18]);
    db.pool().close().await;
    std::fs::remove_file(path).unwrap();
}

async fn seed_decision() -> (
    Db,
    pactling::db::pacts::PactPublication,
    pactling::db::versions::Version,
) {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "1", json!({"interactions": []}))
            .await
            .unwrap();
    let (provider, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    (db, pact, provider)
}

#[tokio::test]
async fn one_evaluation_owns_verdict_and_causes() {
    use pactling::service::matrix::evaluation::{Cause, RowOutcome};
    let (db, pact, provider) = seed_decision().await;
    db.create_verification(
        pact.pact_version_id,
        provider.id,
        None,
        false,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let query = MatrixQuery::parse(Some(
        "pacticipant=C&version=1&pacticipant=P&version=2&latestby=cvpv",
    ));
    let result = matrix::decision(&db, &query).await.unwrap();
    assert_eq!(result.matrix().deployable, Some(false));
    assert_eq!(result.row_outcomes(), &[RowOutcome::Failed]);
    assert_eq!(result.matrix().notices[0].cause, Cause::VerificationFailed);
    let missing = MatrixQuery::parse(Some("pacticipant=C&version=1&pacticipant=P&version=999"));
    let result = matrix::decision(&db, &missing).await.unwrap();
    assert_eq!(result.matrix().deployable, Some(false));
    assert_eq!(result.row_outcomes(), &[RowOutcome::Unverified]);
    assert!(
        matches!(&result.matrix().notices[0].cause, Cause::MissingSelector(s) if s.pacticipant == "P" && s.version.as_deref() == Some("999"))
    );
}

#[tokio::test]
async fn compatibility_renderer_needs_no_database() {
    use pactling::http::{hal::Links, matrix::document};
    use pactling::service::matrix::evaluation::evaluate;
    let (db, pact, provider) = seed_decision().await;
    db.create_verification(
        pact.pact_version_id,
        provider.id,
        None,
        true,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let query = MatrixQuery::parse(Some("pacticipant=C&version=1&pacticipant=P&version=2"));
    let resolved = matrix::resolve(&db, &query).await.unwrap();
    // Later database changes must not alter either the decision or its rendered evidence.
    db.create_verification(
        pact.pact_version_id,
        provider.id,
        None,
        false,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    db.pool().close().await;
    let first = evaluate(resolved.clone());
    assert_eq!(first.query(), &query);
    assert_eq!(
        first.row_outcomes(),
        &[pactling::service::matrix::evaluation::RowOutcome::Successful]
    );
    let second = evaluate(resolved);
    let links = Links::new("http://broker");
    let doc = document(&links, &first);
    assert_eq!(doc, document(&links, &second));
    assert_eq!(doc["summary"]["deployable"], true);
    assert_eq!(doc["matrix"][0]["verificationResult"]["success"], true);
    assert_eq!(doc["matrix"][0]["provider"]["version"]["number"], "2");
}

#[tokio::test]
async fn unverified_candidate_is_retained_without_changing_null_semantics() {
    use pactling::http::{hal::Links, matrix::document};
    use pactling::service::matrix::evaluation::RowOutcome;
    let (db, _, provider) = seed_decision().await;
    let query = MatrixQuery::parse(Some(
        "pacticipant=C&version=1&pacticipant=P&version=2&ignore[][pacticipant]=P&ignore[][version]=2",
    ));
    let result = matrix::decision(&db, &query).await.unwrap();
    assert_eq!(result.matrix().deployable, None);
    assert_eq!(result.row_outcomes(), &[RowOutcome::Unverified]);
    assert_eq!(result.matrix().rows[0].provider_candidate, Some(provider));
    assert!(result.matrix().rows[0].provider.version.is_none());
    assert!(!result.matrix().rows[0].ignored);
    let doc = document(&Links::new("http://broker"), &result);
    assert!(doc["matrix"][0]["provider"]["version"].is_null());
}

#[tokio::test]
async fn ignored_rows_have_a_typed_disposition_without_becoming_successful() {
    use pactling::service::matrix::evaluation::{Cause, RowOutcome};
    let (db, pact, provider) = seed_decision().await;
    db.create_verification(
        pact.pact_version_id,
        provider.id,
        None,
        false,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let query = MatrixQuery::parse(Some(
        "pacticipant=C&version=1&pacticipant=P&version=2&ignore[][pacticipant]=P",
    ));
    let result = matrix::decision(&db, &query).await.unwrap();
    assert_eq!(result.matrix().deployable, Some(true));
    assert_eq!(result.row_outcomes(), &[RowOutcome::Ignored]);
    assert_eq!(result.matrix().success, 0);
    assert_eq!(result.matrix().ignored, 1);
    assert_eq!(result.matrix().notices[0].cause, Cause::NoRequiredRows);
}

#[tokio::test]
async fn overlapping_selectors_are_deduplicated_without_latestby() {
    let (db, pact, provider) = seed_decision().await;
    db.create_verification(
        pact.pact_version_id,
        provider.id,
        None,
        true,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let query = MatrixQuery::parse(Some(
        "pacticipant=C&version=1&pacticipant=C&version=1&pacticipant=P&version=2",
    ));
    let result = matrix::decision(&db, &query).await.unwrap();
    assert_eq!(result.matrix().success, 1);
    assert_eq!(result.matrix().rows.len(), 1);
    assert_eq!(result.row_outcomes().len(), 1);
}
