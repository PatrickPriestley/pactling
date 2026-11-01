use pactling::{
    db::Db,
    service::{
        deployment_explanations::{Explanation, explain},
        matrix::{self, MatrixQuery},
    },
};
use serde_json::json;

async fn decision(db: &Db, query: &str) -> Explanation {
    let evaluation = matrix::decision(db, &MatrixQuery::parse(Some(query)))
        .await
        .unwrap();
    explain(&evaluation)
}
fn reason<'a>(
    explanation: &'a Explanation,
    code: &str,
) -> &'a pactling::service::deployment_explanations::Reason {
    explanation
        .reasons
        .iter()
        .find(|r| r.code == code)
        .unwrap_or_else(|| panic!("missing {code}: {explanation:?}"))
}

#[tokio::test]
async fn explanations_preserve_verdict_precedence_and_exact_action_targets() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "1", json!({"interactions":[]}))
            .await
            .unwrap();
    let (provider, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    let query = "pacticipant=C&version=1&pacticipant=P&version=2";
    let unknown = decision(&db, query).await;
    assert_eq!(unknown.decision, "unknown");
    assert_eq!(unknown.completeness, "incomplete");
    assert_eq!(reason(&unknown, "VERIFICATION_MISSING").role, "decisive");
    assert!(
        !unknown
            .reasons
            .iter()
            .any(|r| r.code == "TARGET_VERSION_UNRESOLVED")
    );
    assert_eq!(unknown.actions.len(), 1);
    assert_eq!(unknown.actions[0].kind, "VERIFY_CONTRACT");
    assert_eq!(unknown.actions[0].target["providerVersion"], "2");
    assert_eq!(unknown.actions[0].target["consumerVersion"], "1");
    assert_eq!(unknown.actions[0].target["contractSha"], pact.sha);
    assert_eq!(unknown.actions[0].row_index, Some(0));
    assert_eq!(
        reason(&unknown, "VERIFICATION_MISSING").action_ids,
        vec![unknown.actions[0].id.clone()]
    );

    let unresolved = decision(&db, "pacticipant=C&version=1&latest=true&tag=absent").await;
    assert_eq!(
        reason(&unresolved, "TARGET_VERSION_UNRESOLVED").role,
        "supporting"
    );
    assert_eq!(unresolved.actions[0].kind, "RESOLVE_TARGET_VERSION");
    assert_eq!(unresolved.actions[0].target["providerVersion"], json!(null));
    assert_eq!(unresolved.actions[0].target["tag"], "absent");

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
    let blocked = decision(&db, query).await;
    assert_eq!(blocked.decision, "blocked");
    assert_eq!(blocked.completeness, "complete");
    assert_eq!(reason(&blocked, "VERIFICATION_FAILED").role, "decisive");
    assert_eq!(blocked.actions[0].kind, "FIX_FAILED_VERIFICATION");
    assert_eq!(blocked.actions[0].target["providerVersion"], "2");

    let missing = decision(&db, "pacticipant=C&version=1&pacticipant=P&version=999").await;
    assert_eq!(missing.decision, "blocked");
    assert_eq!(missing.completeness, "incomplete");
    assert_eq!(
        reason(&missing, "SELECTOR_VERSION_MISSING").role,
        "decisive"
    );
    assert_eq!(reason(&missing, "VERIFICATION_MISSING").role, "supporting");
    let action = missing
        .actions
        .iter()
        .find(|a| a.kind == "RESOLVE_SELECTOR")
        .unwrap();
    assert_eq!(action.target["selectorIndex"], 1);
    assert_eq!(action.target["requested"]["version"], "999");

    let ignored = decision(&db, &format!("{query}&ignore[][pacticipant]=P")).await;
    assert_eq!(ignored.decision, "allowed");
    assert_eq!(ignored.completeness, "complete");
    assert_eq!(reason(&ignored, "ROW_IGNORED").role, "advisory");
    assert!(
        reason(&ignored, "ROW_IGNORED")
            .evidence_refs
            .contains(&"/resolution/ignores/0".into())
    );
    assert_eq!(
        reason(&ignored, "NO_RECORDED_REQUIRED_ROWS").role,
        "decisive"
    );
    assert!(ignored.actions.is_empty());
    let warning = decision(
        &db,
        &format!(
            "{query}&ignore[][pacticipant]=Ghost&ignore[][pacticipant]=P&ignore[][version]=999"
        ),
    )
    .await;
    let corrections: Vec<_> = warning
        .actions
        .iter()
        .filter(|a| a.kind == "CORRECT_IGNORE_REQUEST")
        .collect();
    assert_eq!(corrections.len(), 2);
    assert_eq!(corrections[0].target["ignoreIndex"], 0);
    assert_eq!(corrections[1].target["version"], "999");

    let (other, _) = pactling::service::publish::put_pact(
        &db,
        "P",
        "D",
        "1",
        json!({"interactions":[{"description":"other"}]}),
    )
    .await
    .unwrap();
    let mixed = decision(
        &db,
        "pacticipant=P&version=2&pacticipant=C&version=1&pacticipant=D&version=1",
    )
    .await;
    assert_eq!(mixed.decision, "unknown");
    assert!(!mixed.summary.contains("failed verification"));
    assert!(mixed.summary.contains("No verification"));
    assert_eq!(reason(&mixed, "VERIFICATION_FAILED").role, "supporting");
    assert_eq!(reason(&mixed, "VERIFICATION_MISSING").role, "decisive");
    db.create_verification(
        other.pact_version_id,
        provider.id,
        None,
        true,
        None,
        None,
        None,
    )
    .await
    .unwrap();
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
    let allowed = decision(&db, query).await;
    assert_eq!(allowed.decision, "allowed");
    assert_eq!(
        allowed.summary,
        "All considered contract verifications succeeded."
    );
    assert_eq!(allowed.completeness, "complete");
    assert_eq!(
        reason(&allowed, "ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL").role,
        "decisive"
    );
    assert!(allowed.actions.is_empty());
}

#[tokio::test]
async fn excluded_rows_are_advisories_and_duplicate_actions_are_shared() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "1", json!({"interactions":[]}))
            .await
            .unwrap();
    let (old, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    let (new, _) = db
        .find_or_create_version(pact.provider_id, "3")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_release(old.id, env.id).await.unwrap();
    db.record_deployment(new.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    db.create_verification(pact.pact_version_id, new.id, None, true, None, None, None)
        .await
        .unwrap();
    let excluded = decision(&db, "pacticipant=C&version=1&environment=prod").await;
    assert_eq!(excluded.decision, "allowed");
    assert_eq!(excluded.completeness, "complete");
    assert_eq!(reason(&excluded, "RESOLUTION_EXCLUDED").role, "advisory");
    assert!(
        reason(&excluded, "RESOLUTION_EXCLUDED")
            .evidence_refs
            .contains(&"/resolution/excluded/0".into())
    );
    assert!(excluded.actions.is_empty());
    db.create_verification(pact.pact_version_id, old.id, None, false, None, None, None)
        .await
        .unwrap();
    let latest = decision(&db, "pacticipant=C&version=1&environment=prod&latestby=cvp").await;
    assert_eq!(latest.decision, "allowed");
    assert_eq!(reason(&latest, "LATESTBY_EXCLUDED").role, "advisory");
    assert!(
        reason(&latest, "LATESTBY_EXCLUDED")
            .evidence_refs
            .contains(&"/rows/0".into())
    );
    let duplicate = decision(
        &db,
        "pacticipant=C&version=1&pacticipant=C&version=1&pacticipant=P&version=3",
    )
    .await;
    assert_eq!(reason(&duplicate, "RESOLUTION_EXCLUDED").role, "advisory");
    let missing_consumer = decision(&db, "pacticipant=C&version=999&pacticipant=P&version=2").await;
    assert_eq!(missing_consumer.decision, "blocked");
    assert_eq!(missing_consumer.completeness, "incomplete");
    assert_eq!(
        reason(&missing_consumer, "SELECTOR_VERSION_MISSING").role,
        "decisive"
    );
    let missing_and_failed = decision(
        &db,
        "pacticipant=C&version=1&pacticipant=P&version=2&pacticipant=C&version=999",
    )
    .await;
    assert_eq!(missing_and_failed.decision, "blocked");
    assert_eq!(
        reason(&missing_and_failed, "VERIFICATION_FAILED").role,
        "supporting"
    );

    let missing = decision(
        &db,
        "pacticipant=C&version=1&pacticipant=P&version=missing&pacticipant=P&version=missing",
    )
    .await;
    let targets: Vec<_> = missing
        .actions
        .iter()
        .filter(|a| a.kind == "RESOLVE_TARGET_VERSION")
        .collect();
    assert_eq!(targets.len(), 1);
}

#[tokio::test]
async fn reused_contract_content_keeps_each_consumer_publication_action_target() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (first, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "1", json!({"interactions":[]}))
            .await
            .unwrap();
    let (second, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "2", json!({"interactions":[]}))
            .await
            .unwrap();
    assert_eq!(first.sha, second.sha);
    db.find_or_create_version(first.provider_id, "3")
        .await
        .unwrap();
    let explanation = decision(&db, "pacticipant=C&pacticipant=P&version=3").await;
    assert_eq!(explanation.decision, "unknown");
    let mut versions: Vec<_> = explanation
        .actions
        .iter()
        .filter(|a| a.kind == "VERIFY_CONTRACT")
        .map(|a| {
            assert_eq!(a.target["contractSha"], first.sha);
            assert_eq!(a.target["providerVersion"], "3");
            a.target["consumerVersion"].as_str().unwrap()
        })
        .collect();
    versions.sort_unstable();
    assert_eq!(versions, vec!["1", "2"]);
}

#[tokio::test]
async fn unverified_exclusion_does_not_claim_a_successful_or_present_verification() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) =
        pactling::service::publish::put_pact(&db, "P", "C", "1", json!({"interactions":[]}))
            .await
            .unwrap();
    let (old, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    let (new, _) = db
        .find_or_create_version(pact.provider_id, "3")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_release(old.id, env.id).await.unwrap();
    db.record_deployment(new.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    let explanation = decision(&db, "pacticipant=C&version=1&environment=prod").await;
    assert_eq!(explanation.decision, "unknown");
    assert_eq!(
        reason(&explanation, "RESOLUTION_EXCLUDED").message,
        "Resolution retained other evidence instead of this unverified candidate."
    );
    assert_eq!(
        explanation
            .actions
            .iter()
            .filter(|action| action.kind == "VERIFY_CONTRACT")
            .count(),
        1
    );
    assert_eq!(
        explanation
            .actions
            .iter()
            .find(|action| action.kind == "VERIFY_CONTRACT")
            .unwrap()
            .target["providerVersion"],
        "3"
    );
}
