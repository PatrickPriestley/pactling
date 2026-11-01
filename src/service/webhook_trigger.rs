//! Which webhooks an event fires, the triggered webhooks it records, and their delivery
//! with the Ruby broker's schedule and retries (verified: `TriggerService`, `Job`).
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::db::{
    Db,
    pacts::PactPublication,
    verifications::Verification,
    versions::Version,
    webhooks::{TriggeredWebhook, Webhook},
};
use crate::error::Result;
use crate::service::webhook_delivery as delivery;

pub const RESOURCE_CREATION: &str = "resource_creation";

/// A provider version that still has to verify a pact, with the Ruby broker's
/// descriptions ("latest from main branch", "deployed in test", "released in prod").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredVerification {
    pub provider_version: Version,
    pub branch: Option<String>,
    pub descriptions: Vec<String>,
}

/// Verified: `calculate_required_verifications_for_pact`: the provider's main branch
/// head and every currently deployed or released provider version, when it has no
/// verification of this pact content yet; grouped per version.
pub async fn required_verifications(
    db: &Db,
    pact: &PactPublication,
) -> Result<Vec<RequiredVerification>> {
    let Some(provider) = db.find_pacticipant(&pact.provider_name).await? else {
        return Ok(vec![]);
    };
    let mut candidates: Vec<(Version, Option<String>, String)> = vec![];
    if let Some(branch) = &provider.main_branch
        && let Some(head) = db.latest_version_in_branch(provider.id, branch).await?
    {
        candidates.push((head, Some(branch.clone()), "latest from main branch".into()));
    }
    for d in db.currently_deployed_for_pacticipant(provider.id).await? {
        if let Some(v) = db.find_version_by_id(d.version_id).await?
            && let Some(e) = db.find_environment_by_id(d.environment_id).await?
        {
            candidates.push((v, None, format!("deployed in {}", e.name)));
        }
    }
    for r in db.currently_supported_for_pacticipant(provider.id).await? {
        if let Some(v) = db.find_version_by_id(r.version_id).await?
            && let Some(e) = db.find_environment_by_id(r.environment_id).await?
        {
            candidates.push((v, None, format!("released in {}", e.name)));
        }
    }
    let mut out: Vec<RequiredVerification> = vec![];
    for (version, branch, description) in candidates {
        if db
            .latest_verification_by_provider_version(pact.pact_version_id, version.id)
            .await?
            .is_some()
        {
            continue;
        }
        if let Some(existing) = out.iter_mut().find(|r| r.provider_version.id == version.id) {
            if !existing.descriptions.contains(&description) {
                existing.descriptions.push(description);
            }
            if existing.branch.is_none() {
                existing.branch = branch;
            }
        } else {
            let branch = match branch {
                Some(b) => Some(b),
                None => db
                    .branch_versions_for_version(version.id)
                    .await?
                    .last()
                    .map(|b| b.branch_name.clone()),
            };
            out.push(RequiredVerification {
                provider_version: version,
                branch,
                descriptions: vec![description],
            });
        }
    }
    Ok(out)
}

fn rfc3339(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Records one triggered webhook per (webhook, expanded context), scheduling the i-th
/// `initial + i * step` seconds out, as the Ruby broker's `schedule_webhooks` does.
#[allow(clippy::too_many_arguments)]
async fn create_triggered(
    db: &Db,
    config: &Config,
    webhooks: &[Webhook],
    pact: &PactPublication,
    verification: Option<&Verification>,
    event: &str,
    contexts: &[Map<String, Value>],
    index: &mut u64,
) -> Result<Vec<TriggeredWebhook>> {
    let mut out = vec![];
    for w in webhooks {
        for ctx in contexts {
            let delay = config.webhook_initial_delay + *index * config.webhook_delay_step;
            *index += 1;
            let due = rfc3339(chrono::Utc::now() + chrono::Duration::seconds(delay as i64));
            let t = db
                .create_triggered_webhook(
                    &uuid::Uuid::new_v4().to_string(),
                    w,
                    pact.consumer_id,
                    pact.provider_id,
                    Some(pact.id),
                    verification.map(|v| v.id),
                    RESOURCE_CREATION,
                    event,
                    &Value::Object(ctx.clone()).to_string(),
                    &due,
                )
                .await?;
            out.push(t);
        }
    }
    Ok(out)
}

/// Fires the webhooks for a publication's events, in event order. The context keys and
/// their order are the Ruby broker's (verified in the captured execution logs).
pub async fn trigger_publish_events(
    db: &Db,
    config: &Config,
    base_url: &str,
    pact: &PactPublication,
    events: &[&str],
    build_url: Option<&str>,
) -> Result<Vec<TriggeredWebhook>> {
    let tags: Vec<String> = db
        .tags_for_version(pact.consumer_version_id)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect();
    let branch = db
        .branch_versions_for_version(pact.consumer_version_id)
        .await?
        .last()
        .map(|b| b.branch_name.clone());
    // Verified (`EventListener#contract_published` / `#contract_content_changed`): those
    // two events describe the latest verification from the provider's main branch, fixed
    // when the webhook is triggered; the requiring-verification event carries none.
    let provider = db.find_pacticipant(&pact.provider_name).await?;
    let main_branch_verification = match provider.as_ref().and_then(|p| p.main_branch.clone()) {
        Some(branch) => {
            db.latest_verification_from_branch(pact.pact_version_id, pact.provider_id, &branch)
                .await?
        }
        None => None,
    };
    let mut index = 0u64;
    let mut out = vec![];
    for event in events {
        let webhooks = db
            .webhooks_to_trigger(pact.consumer_id, pact.provider_id, event)
            .await?;
        if webhooks.is_empty() {
            continue;
        }
        let mut base = Map::new();
        base.insert("base_url".into(), json!(base_url));
        base.insert("consumer_version_branch".into(), json!(branch));
        base.insert("build_url".into(), json!(build_url));
        base.insert("consumer_version_tags".into(), json!(tags));
        base.insert("event_name".into(), json!(event));
        let contexts: Vec<Map<String, Value>> =
            if *event == "contract_requiring_verification_published" {
                let mut cs = vec![];
                for r in required_verifications(db, pact).await? {
                    let mut c = base.clone();
                    c.insert(
                        "provider_version_number".into(),
                        json!(r.provider_version.number),
                    );
                    c.insert("provider_version_branch".into(), json!(r.branch));
                    c.insert(
                        "provider_version_descriptions".into(),
                        json!(r.descriptions),
                    );
                    cs.push(c);
                }
                cs
            } else {
                vec![base]
            };
        out.extend(
            create_triggered(
                db,
                config,
                &webhooks,
                pact,
                if *event == "contract_requiring_verification_published" {
                    None
                } else {
                    main_branch_verification.as_ref()
                },
                event,
                &contexts,
                &mut index,
            )
            .await?,
        );
    }
    Ok(out)
}

/// Fires provider_verification_published and then succeeded or failed (verified:
/// `Verifications::Service#broadcast_events`).
pub async fn trigger_verification_events(
    db: &Db,
    config: &Config,
    base_url: &str,
    pact: &PactPublication,
    verification: &Verification,
) -> Result<Vec<TriggeredWebhook>> {
    let tags: Vec<String> = db
        .tags_for_version(verification.provider_version_id)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect();
    let outcome = if verification.success {
        "provider_verification_succeeded"
    } else {
        "provider_verification_failed"
    };
    let mut index = 0u64;
    let mut out = vec![];
    for event in ["provider_verification_published", outcome] {
        let webhooks = db
            .webhooks_to_trigger(pact.consumer_id, pact.provider_id, event)
            .await?;
        if webhooks.is_empty() {
            continue;
        }
        let mut c = Map::new();
        c.insert("base_url".into(), json!(base_url));
        c.insert("provider_version_tags".into(), json!(tags));
        c.insert("event_name".into(), json!(event));
        out.extend(
            create_triggered(
                db,
                config,
                &webhooks,
                pact,
                Some(verification),
                event,
                &[c],
                &mut index,
            )
            .await?,
        );
    }
    Ok(out)
}

/// Whether any webhook's scope covers the pact at all (for the publish notices).
pub async fn any_webhook_for(db: &Db, pact: &PactPublication) -> Result<bool> {
    Ok(db.list_webhooks().await?.iter().any(|w| {
        w.consumer_id.is_none_or(|c| c == pact.consumer_id)
            && w.provider_id.is_none_or(|p| p == pact.provider_id)
    }))
}

/// Executes one triggered webhook now and records the outcome (verified: `Job`).
pub async fn deliver(db: &Db, config: &Config, t: &TriggeredWebhook) -> Result<()> {
    let Some(webhook) = (match t.webhook_id {
        Some(id) => db.find_webhook_by_id(id).await?,
        None => None,
    }) else {
        // Verified: a deleted webhook cannot run; the triggered webhook is marked failed.
        db.set_triggered_webhook_status(t.id, "failure", None)
            .await?;
        return Ok(());
    };
    let pact = match t.pact_publication_id {
        Some(id) => db.find_publication_by_id(id).await?,
        None => None,
    };
    let verification = match t.verification_id {
        Some(id) => db.find_verification_by_id(id).await?,
        None => match &pact {
            Some(p) => {
                db.latest_verification_for_pact_version(p.pact_version_id)
                    .await?
            }
            None => None,
        },
    };
    let context: Map<String, Value> = serde_json::from_str(&t.event_context).unwrap_or_default();
    let params = delivery::parameters(db, pact.as_ref(), verification.as_ref(), &context).await?;
    let built = delivery::build(&delivery::Template::of_webhook(&webhook), &params);
    let attempts = t.executions.len();
    let schedule = &config.webhook_retry_schedule;
    let messages = delivery::Messages {
        success: Some("Successfully executed webhook".into()),
        failure: Some(if attempts < schedule.len() {
            format!("Retrying webhook in {} seconds", schedule[attempts])
        } else {
            format!(
                "Webhook execution failed after {} attempts",
                schedule.len() + 1
            )
        }),
    };
    let result = delivery::execute(config, Some(&webhook.uuid), &built, &context, &messages).await;
    let (status, next) = if result.success {
        ("success", None)
    } else if attempts < schedule.len() {
        (
            "retrying",
            Some(rfc3339(
                chrono::Utc::now() + chrono::Duration::seconds(schedule[attempts] as i64),
            )),
        )
    } else {
        ("failure", None)
    };
    // One transaction, so a crash cannot leave a recorded attempt with a stale status.
    db.finish_execution(t.id, result.success, &result.logs, status, next.as_deref())
        .await?;
    Ok(())
}

/// Delivers every due triggered webhook, oldest first.
pub async fn deliver_due(db: &Db, config: &Config) -> Result<usize> {
    deliver_due_at(db, config, chrono::Utc::now()).await
}

/// Delivers everything due at `now`. The queue is the `triggered_webhooks` table, so a
/// new process picks up what an old one left.
pub async fn deliver_due_at(
    db: &Db,
    config: &Config,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<usize> {
    let due = db.due_triggered_webhooks(&rfc3339(now)).await?;
    let n = due.len();
    for t in &due {
        deliver(db, config, t).await?;
    }
    Ok(n)
}

/// The background delivery loop: polls every second so a restart loses nothing.
pub fn spawn_delivery_loop(db: Db, config: Config) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if let Err(e) = deliver_due(&db, &config).await {
                tracing::warn!("webhook delivery failed: {e}");
            }
        }
    });
}
