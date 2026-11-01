//! Pacts for verification (consumer version selectors, pending), and verification results.
//! Behaviour and strings follow the Phase 3 capture in the implementation plan.
use serde_json::{Map, Value, json};

use crate::db::{
    Db, pacticipants::Pacticipant, pacts::PactPublication, verifications::Verification,
};
use crate::error::{Error, Result};

/// One parsed consumer version selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    MainBranch,
    Branch {
        branch: String,
        consumer: Option<String>,
    },
    MatchingBranch,
    Tag {
        tag: String,
        latest: bool,
        consumer: Option<String>,
    },
    Latest {
        consumer: Option<String>,
    },
    Deployed,
    Released,
    DeployedOrReleased,
    Environment {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForVerificationRequest {
    pub selectors: Vec<Selector>,
    pub include_pending_status: bool,
    pub provider_version_branch: Option<String>,
    pub provider_version_tags: Vec<String>,
}

impl ForVerificationRequest {
    /// Parses the POST body, collecting the Ruby broker's validation messages.
    pub fn parse(body: &Value) -> Result<ForVerificationRequest> {
        let mut errors: Map<String, Value> = Map::new();
        let mut push = |field: &str, msg: String| {
            errors
                .entry(field)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(json!(msg));
        };
        let include_pending_status = match body.get("includePendingStatus") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                push("includePendingStatus", "must be one of: true, false".into());
                false
            }
        };
        let provider_version_branch = match body.get("providerVersionBranch") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let provider_version_tags: Vec<String> = match body.get("providerVersionTags") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect(),
            _ => vec![],
        };
        let mut selectors = vec![];
        match body.get("consumerVersionSelectors") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    match parse_selector(item) {
                        Some(Selector::MatchingBranch) if provider_version_branch.is_none() => {
                            push(
                                "consumerVersionSelectors",
                                format!(
                                    "the providerVersionBranch must be specified when the selector matchingBranch=true is used (at index {i})"
                                ),
                            )
                        }
                        Some(s) => selectors.push(s),
                        None => push(
                            "consumerVersionSelectors",
                            format!(
                                "must specify a value for environment or tag or branch, or specify mainBranch=true, matchingBranch=true, latest=true, deployed=true, released=true or deployedOrReleased=true (at index {i})"
                            ),
                        ),
                    }
                }
            }
            Some(_) => push("consumerVersionSelectors", "must be an array".into()),
        }
        if !errors.is_empty() {
            return Err(Error::Validation(Value::Object(errors)));
        }
        Ok(ForVerificationRequest {
            selectors,
            include_pending_status,
            provider_version_branch,
            provider_version_tags,
        })
    }
}

fn parse_selector(item: &Value) -> Option<Selector> {
    let flag = |k: &str| item.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let text = |k: &str| {
        item.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let consumer = text("consumer");
    if flag("mainBranch") {
        return Some(Selector::MainBranch);
    }
    if flag("matchingBranch") {
        return Some(Selector::MatchingBranch);
    }
    if let Some(branch) = text("branch") {
        return Some(Selector::Branch { branch, consumer });
    }
    if let Some(tag) = text("tag") {
        return Some(Selector::Tag {
            tag,
            latest: flag("latest"),
            consumer,
        });
    }
    if let Some(name) = text("environment") {
        return Some(Selector::Environment { name });
    }
    if flag("deployedOrReleased") {
        return Some(Selector::DeployedOrReleased);
    }
    if flag("deployed") {
        return Some(Selector::Deployed);
    }
    if flag("released") {
        return Some(Selector::Released);
    }
    if flag("latest") {
        return Some(Selector::Latest { consumer });
    }
    None
}

/// Why a pact was selected: the short description and the notice bullet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub short: String,
    pub bullet: String,
    /// The `s[]` metadata keys the Ruby broker puts in the pact link for this reason.
    pub metadata: Vec<(&'static str, String)>,
    /// The consumer version this reason matched; set when the reason is attached.
    pub consumer_version_id: i64,
}

/// A pact to verify with everything the response needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiablePact {
    pub publication: PactPublication,
    pub reasons: Vec<Reason>,
    /// Always computed; the Ruby broker reports it regardless of includePendingStatus.
    pub pending: Option<bool>,
}

/// Resolves the request to the pacts to verify, one per pact content, with the reasons
/// each selector contributed. Order: by consumer name, then first match.
pub async fn pacts_for_verification(
    db: &Db,
    provider: &Pacticipant,
    req: &ForVerificationRequest,
) -> Result<Vec<VerifiablePact>> {
    // Verified in Ruby's PactsForVerificationRepository#default_selectors: with no
    // selectors, the latest main-branch (or overall latest) version of every consumer plus
    // the versions currently deployed or released; the capture lists the deployed reasons
    // first.
    let default_main_branch = req.selectors.is_empty();
    let selectors: Vec<Selector> = if default_main_branch {
        vec![Selector::Deployed, Selector::Released, Selector::MainBranch]
    } else {
        req.selectors.clone()
    };
    let mut out: Vec<VerifiablePact> = vec![];
    for selector in &selectors {
        let matches: Vec<(PactPublication, Reason)> = match selector {
            Selector::MainBranch => db
                .provider_pacts_latest_main_branch(provider.id)
                .await?
                .into_iter()
                .map(|p| {
                    let bullet = if default_main_branch {
                        format!(
                            "latest version of {} from the main branch '{}' ({})",
                            p.consumer_name,
                            main_branch_of(&p),
                            p.consumer_version_number
                        )
                    } else {
                        format!(
                            "latest version from the main branch '{}' ({})",
                            main_branch_of(&p),
                            p.consumer_version_number
                        )
                    };
                    let metadata = if default_main_branch {
                        vec![]
                    } else {
                        vec![("l", "true".to_owned())]
                    };
                    let consumer_version_id = p.consumer_version_id;
                    (
                        p,
                        Reason {
                            short: "latest from main branch".into(),
                            bullet,
                            metadata,
                            consumer_version_id,
                        },
                    )
                })
                .collect(),
            Selector::Branch { branch, consumer } => db
                .provider_pacts_latest_branch(provider.id, branch, consumer.as_deref())
                .await?
                .into_iter()
                .map(|p| {
                    // Naming a consumer changes the wording (verified).
                    let bullet = match consumer {
                        Some(c) => format!(
                            "latest version of {c} from branch '{branch}' ({})",
                            p.consumer_version_number
                        ),
                        None => format!(
                            "latest version from branch '{branch}' ({})",
                            p.consumer_version_number
                        ),
                    };
                    let r = Reason {
                        short: format!("latest from branch {branch}"),
                        bullet,
                        metadata: vec![("b", branch.clone()), ("l", "true".to_owned())],
                        consumer_version_id: p.consumer_version_id,
                    };
                    (p, r)
                })
                .collect(),
            Selector::MatchingBranch => {
                let branch = req.provider_version_branch.clone().expect("validated");
                db.provider_pacts_latest_branch(provider.id, &branch, None)
                    .await?
                    .into_iter()
                    .map(|p| {
                        let r = Reason {
                            short: format!("latest from branch {branch}"),
                            bullet: format!(
                                "latest version from branch '{branch}' ({})",
                                p.consumer_version_number
                            ),
                            metadata: vec![("b", branch.clone()), ("l", "true".to_owned())],
                            consumer_version_id: p.consumer_version_id,
                        };
                        (p, r)
                    })
                    .collect()
            }
            Selector::Tag {
                tag,
                latest,
                consumer,
            } => db
                .provider_pacts_with_tag(provider.id, tag, *latest, consumer.as_deref())
                .await?
                .into_iter()
                .map(|p| {
                    let r = if *latest {
                        Reason {
                            short: format!("latest with tag {tag}"),
                            bullet: format!(
                                "latest version tagged '{tag}' ({})",
                                p.consumer_version_number
                            ),
                            metadata: vec![("t", tag.clone()), ("l", "true".to_owned())],
                            consumer_version_id: p.consumer_version_id,
                        }
                    } else {
                        Reason {
                            short: format!("one of {tag}"),
                            bullet: format!(
                                "all consumer versions tagged '{tag}' ({})",
                                p.consumer_version_number
                            ),
                            metadata: vec![("t", tag.clone())],
                            consumer_version_id: p.consumer_version_id,
                        }
                    };
                    (p, r)
                })
                .collect(),
            Selector::Latest { consumer } => db
                .provider_pacts_latest(provider.id, consumer.as_deref())
                .await?
                .into_iter()
                .map(|p| {
                    let r = Reason {
                        short: "latest".into(),
                        bullet: format!(
                            "latest version of a consumer that has a pact with {} ({})",
                            provider.name, p.consumer_version_number
                        ),
                        metadata: vec![("l", "true".to_owned())],
                        consumer_version_id: p.consumer_version_id,
                    };
                    (p, r)
                })
                .collect(),
            Selector::Deployed
            | Selector::Released
            | Selector::DeployedOrReleased
            | Selector::Environment { .. } => {
                let (deployed, released, wanted) = match selector {
                    Selector::Deployed => (true, false, None),
                    Selector::Released => (false, true, None),
                    Selector::Environment { name } => {
                        match db.find_environment_by_name(name).await? {
                            Some(e) => (true, true, Some(e)),
                            None => continue,
                        }
                    }
                    _ => (true, true, None),
                };
                let ids = db
                    .version_ids_deployed_anywhere(
                        deployed,
                        released,
                        wanted.as_ref().map(|e| e.id),
                    )
                    .await?;
                let mut out = vec![];
                for id in ids {
                    for p in db.publications_for_consumer_version(id).await? {
                        if p.provider_id != provider.id {
                            continue;
                        }
                        // Verified (review finding): a deployed selector reports only the
                        // environments the version is deployed to, a released selector only
                        // those it is released in; Ruby encodes just the consumer version in
                        // the metadata of these reasons, and `e` for an environment selector.
                        let mut found: Vec<(i64, bool)> = vec![]; // (environment id, released)
                        if deployed {
                            for d in db.deployments_of_version(id).await? {
                                if d.currently_deployed {
                                    found.push((d.environment_id, false));
                                }
                            }
                        }
                        if released {
                            for r in db.releases_of_version(id).await? {
                                if r.currently_supported {
                                    found.push((r.environment_id, true));
                                }
                            }
                        }
                        for (environment_id, is_release) in found {
                            if let Some(w) = &wanted
                                && w.id != environment_id
                            {
                                continue;
                            }
                            // The environment can be deleted between the two reads; a
                            // record left behind is then not a reason (review finding).
                            let Some(e) = db.find_environment_by_id(environment_id).await? else {
                                continue;
                            };
                            let r = match selector {
                                Selector::Environment { .. } => Reason {
                                    short: format!("in {}", e.name),
                                    bullet: format!(
                                        "a consumer version in environment {} ({})",
                                        e.name, p.consumer_version_number
                                    ),
                                    metadata: vec![("e", e.name.clone())],
                                    consumer_version_id: p.consumer_version_id,
                                },
                                _ if is_release => Reason {
                                    short: format!("released in {}", e.name),
                                    bullet: format!(
                                        "consumer version(s) released and supported in {} ({})",
                                        e.name, p.consumer_version_number
                                    ),
                                    metadata: vec![],
                                    consumer_version_id: p.consumer_version_id,
                                },
                                _ => Reason {
                                    short: format!("deployed to {}", e.name),
                                    bullet: format!(
                                        "consumer version(s) currently deployed to {} ({})",
                                        e.name, p.consumer_version_number
                                    ),
                                    metadata: vec![],
                                    consumer_version_id: p.consumer_version_id,
                                },
                            };
                            out.push((p.clone(), r));
                        }
                    }
                }
                out
            }
        };
        for (publication, reason) in matches {
            if let Some(existing) = out
                .iter_mut()
                .find(|v| v.publication.pact_version_id == publication.pact_version_id)
            {
                if !existing.reasons.contains(&reason) {
                    existing.reasons.push(reason);
                }
            } else {
                out.push(VerifiablePact {
                    publication,
                    reasons: vec![reason],
                    pending: None,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.publication
            .consumer_name
            .cmp(&b.publication.consumer_name)
    });
    // Ruby's Selector#<=> (exact source, pact_broker/pacts/selector.rb): overall latest,
    // then latest-for-branch by branch name, latest-for-tag by tag, all-for-tag, currently
    // deployed by environment, currently supported by environment, then the rest (a main
    // branch selector carries no branch name, so it sorts after a deployment).
    for v in &mut out {
        v.reasons.sort_by_key(reason_rank);
    }
    // The Ruby broker reports pending whether or not includePendingStatus was sent (verified).
    for v in &mut out {
        v.pending = Some(is_pending(db, v.publication.pact_version_id, provider, req).await?);
    }
    Ok(out)
}

fn reason_rank(r: &Reason) -> (u8, String) {
    let short = r.short.as_str();
    if short == "latest" {
        (0, String::new())
    } else if let Some(b) = short.strip_prefix("latest from branch ") {
        (1, b.to_owned())
    } else if let Some(t) = short.strip_prefix("latest with tag ") {
        (2, t.to_owned())
    } else if let Some(t) = short.strip_prefix("one of ") {
        (3, t.to_owned())
    } else if let Some(e) = short.strip_prefix("deployed to ") {
        (4, e.to_owned())
    } else if let Some(e) = short.strip_prefix("released in ") {
        (5, e.to_owned())
    } else if let Some(e) = short.strip_prefix("in ") {
        (6, e.to_owned())
    } else {
        (7, String::new())
    }
}

fn main_branch_of(p: &PactPublication) -> String {
    // The publication was selected through the consumer's main branch, so the branch's
    // name is what the consumer's `mainBranch` holds; the query guarantees it exists.
    p.consumer_main_branch
        .clone()
        .unwrap_or_else(|| "main".to_owned())
}

/// The pending rule, verified by probing the Ruby broker: with a provider branch that has
/// versions, pending means no successful verification from that branch; a branch with no
/// versions, or no branch at all, falls back to any provider version; provider tags never
/// fall back.
pub async fn is_pending(
    db: &Db,
    pact_version_id: i64,
    provider: &Pacticipant,
    req: &ForVerificationRequest,
) -> Result<bool> {
    if !req.provider_version_tags.is_empty() {
        for tag in &req.provider_version_tags {
            if db
                .verified_by_provider_tag(pact_version_id, provider.id, tag)
                .await?
            {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if let Some(branch) = &req.provider_version_branch
        && db.provider_branch_has_versions(provider.id, branch).await?
    {
        return Ok(!db
            .verified_by_provider_branch(pact_version_id, provider.id, branch)
            .await?);
    }
    Ok(!db
        .verified_by_any_provider_version(pact_version_id, provider.id)
        .await?)
}

// --- verification results -------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultRequest {
    pub provider_version: String,
    pub success: bool,
    pub test_results: Option<Value>,
    pub verified_by: Option<Value>,
    pub build_url: Option<String>,
}

impl ResultRequest {
    pub fn parse(body: &Value) -> Result<ResultRequest> {
        let mut errors: Map<String, Value> = Map::new();
        let success = match body.get("success") {
            Some(Value::Bool(b)) => Some(*b),
            None | Some(Value::Null) => {
                errors.insert("success".into(), json!(["is missing"]));
                None
            }
            Some(_) => {
                errors.insert("success".into(), json!(["must be boolean"]));
                None
            }
        };
        let provider_version = match body.get("providerApplicationVersion") {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
            Some(Value::String(_)) => {
                errors.insert(
                    "providerApplicationVersion".into(),
                    json!(["must be filled"]),
                );
                None
            }
            None | Some(Value::Null) => {
                errors.insert("providerApplicationVersion".into(), json!(["is missing"]));
                None
            }
            Some(_) => {
                errors.insert(
                    "providerApplicationVersion".into(),
                    json!(["must be filled"]),
                );
                None
            }
        };
        if !errors.is_empty() {
            return Err(Error::Validation(Value::Object(errors)));
        }
        Ok(ResultRequest {
            provider_version: provider_version.expect("validated"),
            success: success.expect("validated"),
            test_results: body.get("testResults").cloned(),
            verified_by: body.get("verifiedBy").cloned(),
            build_url: body
                .get("buildUrl")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        })
    }
}

/// Records a result for the pact content, creating the provider version when missing.
pub async fn publish_result(
    db: &Db,
    pact: &PactPublication,
    req: &ResultRequest,
    consumer_version_id: Option<i64>,
) -> Result<Verification> {
    let provider = db
        .find_pacticipant(&pact.provider_name)
        .await?
        .ok_or(Error::NoRoute)?;
    let (version, _) = db
        .find_or_create_version(provider.id, &req.provider_version)
        .await?;
    let test_results = req
        .test_results
        .as_ref()
        .map(|v| serde_json::to_string(v).expect("json"));
    let verified_by = req
        .verified_by
        .as_ref()
        .map(|v| serde_json::to_string(v).expect("json"));
    Ok(db
        .create_verification(
            pact.pact_version_id,
            version.id,
            consumer_version_id.or(Some(pact.consumer_version_id)),
            req.success,
            test_results.as_deref(),
            verified_by.as_deref(),
            req.build_url.as_deref(),
        )
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_collects_selector_errors_with_indexes() {
        let err =
            ForVerificationRequest::parse(&json!({"consumerVersionSelectors": [{"bogus": true}]}))
                .unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"consumerVersionSelectors": ["must specify a value for environment or tag or branch, or specify mainBranch=true, matchingBranch=true, latest=true, deployed=true, released=true or deployedOrReleased=true (at index 0)"]}))
        );
        let err = ForVerificationRequest::parse(
            &json!({"consumerVersionSelectors": [{"matchingBranch": true}]}),
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"consumerVersionSelectors": ["the providerVersionBranch must be specified when the selector matchingBranch=true is used (at index 0)"]}))
        );
        let err = ForVerificationRequest::parse(
            &json!({"consumerVersionSelectors": "x", "includePendingStatus": "yes"}),
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"consumerVersionSelectors": ["must be an array"], "includePendingStatus": ["must be one of: true, false"]}))
        );
    }

    #[test]
    fn parse_reads_every_selector_kind() {
        let req = ForVerificationRequest::parse(&json!({
            "includePendingStatus": true, "providerVersionBranch": "main", "providerVersionTags": ["prod"],
            "consumerVersionSelectors": [
                {"mainBranch": true}, {"branch": "feat", "consumer": "Web"}, {"matchingBranch": true},
                {"tag": "dev"}, {"tag": "dev", "latest": true}, {"latest": true}, {"deployed": true},
                {"released": true}, {"deployedOrReleased": true}
            ]
        }))
        .unwrap();
        assert!(req.include_pending_status);
        assert_eq!(req.provider_version_branch.as_deref(), Some("main"));
        assert_eq!(req.provider_version_tags, ["prod"]);
        assert_eq!(
            req.selectors,
            [
                Selector::MainBranch,
                Selector::Branch {
                    branch: "feat".into(),
                    consumer: Some("Web".into())
                },
                Selector::MatchingBranch,
                Selector::Tag {
                    tag: "dev".into(),
                    latest: false,
                    consumer: None
                },
                Selector::Tag {
                    tag: "dev".into(),
                    latest: true,
                    consumer: None
                },
                Selector::Latest { consumer: None },
                Selector::Deployed,
                Selector::Released,
                Selector::DeployedOrReleased,
            ]
        );
        let empty = ForVerificationRequest::parse(&json!({})).unwrap();
        assert!(empty.selectors.is_empty());
        assert!(!empty.include_pending_status);
    }

    #[test]
    fn result_request_validation_matches_ruby() {
        let err = ResultRequest::parse(&json!({})).unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"success": ["is missing"], "providerApplicationVersion": ["is missing"]}))
        );
        let err =
            ResultRequest::parse(&json!({"providerApplicationVersion": "", "success": "yes"}))
                .unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"success": ["must be boolean"], "providerApplicationVersion": ["must be filled"]}))
        );
        let ok = ResultRequest::parse(&json!({"providerApplicationVersion": "7", "success": true}))
            .unwrap();
        assert_eq!(ok.provider_version, "7");
        assert!(ok.test_results.is_none());
    }
}
