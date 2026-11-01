use pactling::{
    db::Db,
    service::{
        matrix::{
            self, MatrixQuery,
            provenance::{ExclusionKind, MembershipSource},
        },
        publish,
    },
};
use serde_json::json;

async fn seed() -> (
    Db,
    pactling::db::pacts::PactPublication,
    pactling::db::versions::Version,
) {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) = publish::put_pact(&db, "P", "C", "1", json!({"interactions":[]}))
        .await
        .unwrap();
    let (pv, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    (db, pact, pv)
}
async fn decision(db: &Db, query: &str) -> matrix::evaluation::DecisionEvaluation {
    matrix::decision(db, &MatrixQuery::parse(Some(query)))
        .await
        .unwrap()
}

#[tokio::test]
async fn unverified_pin_retains_candidate_without_changing_null_row() {
    let (db, _, pv) = seed().await;
    let d = decision(&db, "pacticipant=C&version=1&pacticipant=P&version=2").await;
    assert_eq!(d.provenance().selectors.len(), 2);
    assert_eq!(d.provenance().selectors[1].versions, vec![pv.clone()]);
    assert_eq!(d.provenance().candidates[0].version, pv);
    assert_eq!(d.provenance().rows[0].candidate_indices, vec![0]);
    assert_eq!(d.provenance().rows[0].selector_indices, vec![0, 1]);
    assert!(d.matrix().rows[0].provider.version.is_none());
    assert!(!d.provenance().selectors[1].missing);
    assert_eq!(d.matrix().deployable, None);
    let context = d.context();
    uuid::Uuid::parse_str(&context.id).unwrap();
    assert!(context.started_at <= context.evaluated_at);
    chrono::DateTime::parse_from_rfc3339(&context.started_at).unwrap();
}

#[tokio::test]
async fn environment_membership_identifies_deployments_releases_and_instances() {
    let (db, pact, pv) = seed().await;
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    let other = db
        .create_environment("other", None, false, None)
        .await
        .unwrap();
    let (deployment, _) = db
        .record_deployment(pv.id, pact.provider_id, env.id, Some("blue"))
        .await
        .unwrap();
    let (release, _) = db.record_release(pv.id, env.id).await.unwrap();
    db.record_deployment(pv.id, pact.provider_id, other.id, None)
        .await
        .unwrap();
    db.record_release(pv.id, other.id).await.unwrap();
    let d = decision(&db, "pacticipant=C&version=1&environment=prod").await;
    assert_eq!(d.provenance().environment.as_ref(), Some(&env));
    let sources = &d.provenance().candidates[0].sources;
    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .any(|s| matches!(s, MembershipSource::Deployed(v) if v == &deployment))
    );
    assert!(
        sources
            .iter()
            .any(|s| matches!(s, MembershipSource::Released(v) if v == &release))
    );
}

#[tokio::test]
async fn latestby_exclusions_do_not_become_blockers() {
    let (db, pact, older) = seed().await;
    let (newer, _) = db
        .find_or_create_version(pact.provider_id, "3")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_release(older.id, env.id).await.unwrap();
    db.record_deployment(newer.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    for (pv, success) in [(&older, false), (&newer, true)] {
        db.create_verification(pact.pact_version_id, pv.id, None, success, None, None, None)
            .await
            .unwrap();
    }
    let d = decision(&db, "pacticipant=C&version=1&environment=prod&latestby=cvp").await;
    assert_eq!(d.matrix().deployable, Some(true));
    let excluded = &d.provenance().excluded;
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].kind, ExclusionKind::Latestby);
    assert_eq!(excluded[0].row.provider_candidate.as_ref(), Some(&older));
    assert_eq!(excluded[0].selected_row, Some(0));
    assert_eq!(
        d.provenance().candidates[d.provenance().rows[0].candidate_indices[0]].version,
        newer
    );
}

#[tokio::test]
async fn ignore_targets_match_legacy_null_rules() {
    let (db, _, _) = seed().await;
    let d = decision(&db, "pacticipant=C&version=1&pacticipant=P&version=2&ignore[][pacticipant]=P&ignore[][version]=2&ignore[][pacticipant]=C&ignore[][version]=1&ignore[][pacticipant]=P&ignore[][pacticipant]=Other").await;
    assert_eq!(d.provenance().rows[0].ignore_indices, vec![1, 2]);
    assert_eq!(d.matrix().ignored, 1);
}

#[tokio::test]
async fn unverified_candidates_omitted_by_resolver_remain_excluded_evidence() {
    let (db, pact, older) = seed().await;
    let (newer, _) = db
        .find_or_create_version(pact.provider_id, "3")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_release(older.id, env.id).await.unwrap();
    db.record_deployment(newer.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    db.create_verification(pact.pact_version_id, newer.id, None, true, None, None, None)
        .await
        .unwrap();
    let d = decision(&db, "pacticipant=C&version=1&environment=prod").await;
    assert_eq!(d.matrix().rows.len(), 1);
    assert_eq!(d.provenance().excluded.len(), 1);
    assert_eq!(
        d.provenance().excluded[0].kind,
        ExclusionKind::UnverifiedCandidate
    );
    assert_eq!(
        d.provenance().excluded[0].row.provider_candidate.as_ref(),
        Some(&older)
    );
    assert_eq!(d.provenance().candidates.len(), 2);
}

#[tokio::test]
async fn duplicate_rows_merge_selector_provenance() {
    let (db, pact, pv) = seed().await;
    db.create_verification(pact.pact_version_id, pv.id, None, true, None, None, None)
        .await
        .unwrap();
    let d = decision(
        &db,
        "pacticipant=C&version=1&pacticipant=C&version=1&pacticipant=P&version=2",
    )
    .await;
    assert_eq!(d.provenance().rows[0].selector_indices, vec![0, 1, 2]);
    assert_eq!(d.provenance().excluded.len(), 1);
    assert_eq!(d.provenance().excluded[0].kind, ExclusionKind::Duplicate);
    assert_eq!(d.provenance().excluded[0].selected_row, Some(0));
}

#[tokio::test]
async fn provider_side_candidates_are_consumers_and_preserve_provider_target() {
    let (db, pact, pv) = seed().await;
    let cv = db.find_version("C", "1").await.unwrap().unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_deployment(cv.id, pact.consumer_id, env.id, Some("consumer-instance"))
        .await
        .unwrap();
    let d = decision(&db, "pacticipant=P&version=2&environment=prod").await;
    assert_eq!(d.provenance().candidates[0].version, cv);
    assert_eq!(d.matrix().rows[0].provider_candidate.as_ref(), Some(&pv));
    assert!(
        matches!(&d.provenance().candidates[0].sources[0], MembershipSource::Deployed(v) if v.application_instance.as_deref() == Some("consumer-instance"))
    );
}

#[tokio::test]
async fn selector_resolution_without_rows_and_missing_null_semantics() {
    let (db, _, _) = seed().await;
    let d = decision(&db, "pacticipant=P&version=2&pacticipant=C&version=absent").await;
    assert_eq!(d.provenance().selectors[0].versions[0].number, "2");
    assert!(!d.provenance().selectors[0].missing);
    assert!(d.provenance().selectors[1].versions.is_empty());
    assert!(d.provenance().selectors[1].missing);
    assert!(d.matrix().rows.is_empty());
    let (pact, _) = publish::put_pact(&db, "Unversioned", "C", "1", json!({"interactions":[]}))
        .await
        .unwrap();
    assert_ne!(pact.provider_id, 0);
    let d = decision(&db, "pacticipant=C&version=1&pacticipant=Unversioned").await;
    assert!(d.provenance().selectors[1].versions.is_empty());
    assert!(!d.provenance().selectors[1].missing);
    assert!(d.provenance().rows[0].candidate_indices.is_empty());
}

#[tokio::test]
async fn unverified_selection_retains_newest_and_excludes_older_for_environment_and_versions() {
    let (db, pact, older) = seed().await;
    let (newer, _) = db
        .find_or_create_version(pact.provider_id, "3")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_release(older.id, env.id).await.unwrap();
    db.record_deployment(newer.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    db.find_or_create_tag(older.id, "prod").await.unwrap();
    db.find_or_create_tag(newer.id, "prod").await.unwrap();
    for query in [
        "pacticipant=C&version=1&environment=prod",
        "q[][pacticipant]=C&q[][version]=1&q[][pacticipant]=P&q[][tag]=prod&environment=prod",
    ] {
        let d = decision(&db, query).await;
        assert_eq!(d.matrix().rows[0].provider_candidate.as_ref(), Some(&newer));
        assert_eq!(d.provenance().excluded.len(), 1);
        assert_eq!(
            d.provenance().excluded[0].row.provider_candidate.as_ref(),
            Some(&older)
        );
        assert_eq!(
            d.provenance().excluded[0].kind,
            ExclusionKind::UnverifiedCandidate
        );
        assert!(d.provenance().excluded[0].selected_row.is_none());
        if d.query().selectors.len() == 2 {
            assert!(
                d.provenance()
                    .candidates
                    .iter()
                    .all(|c| c.sources.is_empty())
            );
            assert_eq!(d.provenance().environment.as_ref(), Some(&env));
        }
    }
}

#[tokio::test]
async fn evaluation_context_and_provenance_are_frozen_before_evaluation() {
    let (db, _, pv) = seed().await;
    let q = MatrixQuery::parse(Some("pacticipant=C&version=1&pacticipant=P&version=2"));
    let resolved = matrix::resolve(&db, &q).await.unwrap();
    db.find_or_create_version(pv.pacticipant_id, "3")
        .await
        .unwrap();
    db.pool().close().await;
    let first = matrix::evaluation::evaluate(resolved.clone());
    let second = matrix::evaluation::evaluate(resolved);
    assert_eq!(first.context(), second.context());
    assert_eq!(first.provenance().selectors[1].versions, vec![pv]);
    assert_eq!(
        format!("{:?}", first.provenance()),
        format!("{:?}", second.provenance())
    );
}

#[tokio::test]
async fn ended_membership_is_not_a_current_candidate_source() {
    let (db, pact, pv) = seed().await;
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    let (old, _) = db
        .record_deployment(pv.id, pact.provider_id, env.id, Some("retired"))
        .await
        .unwrap();
    db.mark_undeployed(old.id).await.unwrap();
    let (active, _) = db
        .record_deployment(pv.id, pact.provider_id, env.id, Some("active"))
        .await
        .unwrap();
    let (release, _) = db.record_release(pv.id, env.id).await.unwrap();
    db.mark_support_ended(release.id).await.unwrap();
    let d = decision(&db, "pacticipant=C&version=1&environment=prod").await;
    assert_eq!(d.provenance().candidates[0].sources.len(), 1);
    assert!(
        matches!(&d.provenance().candidates[0].sources[0], MembershipSource::Deployed(v) if v == &active)
    );
}

#[tokio::test]
async fn reversed_explicit_selectors_still_have_provider_candidates() {
    let (db, _, pv) = seed().await;
    let d = decision(&db, "pacticipant=P&version=2&pacticipant=C&version=1").await;
    assert_eq!(d.provenance().candidates[0].version, pv);
}

#[tokio::test]
async fn self_integration_consumer_direction_preserves_distinct_target_version() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (pact, _) = publish::put_pact(&db, "Self", "Self", "1", json!({"interactions":[]}))
        .await
        .unwrap();
    let (target, _) = db
        .find_or_create_version(pact.provider_id, "2")
        .await
        .unwrap();
    let env = db
        .create_environment("prod", None, true, None)
        .await
        .unwrap();
    db.record_deployment(target.id, pact.provider_id, env.id, None)
        .await
        .unwrap();
    let d = decision(&db, "pacticipant=Self&version=1&environment=prod").await;
    assert_eq!(d.provenance().candidates[0].version, target);
}
