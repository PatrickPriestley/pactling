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
