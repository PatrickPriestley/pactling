//! Publishing pacts: the modern `contracts/publish` request and the legacy `PUT`. Both
//! store through `store_pact`; the shapes and strings come from the Phase 2 capture.
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::db::{Db, pacticipants::Pacticipant, pacts::PactPublication, versions::Version};
use crate::error::{Error, Result};
use crate::service::content;
use crate::service::versions::{self as versions, find_or_create_pacticipant};

pub fn content_changed_message(consumer: &str, version: &str, provider: &str) -> String {
    format!(
        "Cannot change the content of the pact for {consumer} version {version} and provider {provider}, \
         as race conditions will cause unreliable results for can-i-deploy. Each pact must be published \
         with a unique consumer version number. Some Pact libraries generate random data when a concrete \
         value for a type matcher is not specified, and this can cause the contract to mutate - ensure \
         you have given example values for all type matchers. For more information see \
         https://docs.pact.io/go/versioning"
    )
}

/// Stores content for a consumer version and provider. `created` is false when the same
/// content was already published for that version. Different content is a conflict.
pub async fn store_pact(
    db: &Db,
    consumer: &Pacticipant,
    version: &Version,
    provider: &Pacticipant,
    content: &Value,
) -> Result<(PactPublication, bool)> {
    let sha = content::pact_version_sha(content);
    if let Some(existing) = db.find_publication(version.id, provider.id).await? {
        if existing.sha != sha {
            return Err(Error::Conflict(content_changed_message(
                &consumer.name,
                &version.number,
                &provider.name,
            )));
        }
        db.touch_publication(existing.id).await?;
        return Ok((existing, false));
    }
    let serialised = serde_json::to_string(content).expect("json");
    let pact_version_id = db
        .find_or_create_pact_version(consumer.id, provider.id, &sha, &serialised)
        .await?;
    let publication = db
        .create_publication(version.id, provider.id, pact_version_id)
        .await?;
    Ok((publication, true))
}

// --- legacy PUT -------------------------------------------------------------------------

/// `PUT /pacts/provider/{p}/consumer/{c}/version/{v}`. Names in the body must match the
/// URL when present and are filled in when absent (verified).
pub async fn put_pact(
    db: &Db,
    provider: &str,
    consumer: &str,
    version: &str,
    body: Value,
) -> Result<(PactPublication, bool)> {
    let mut errors = Map::new();
    for (key, expected) in [("consumer", consumer), ("provider", provider)] {
        if let Some(name) = body
            .get(key)
            .and_then(|c| c.get("name"))
            .and_then(|n| n.as_str())
            && name != expected
        {
            errors.insert(
                format!("{key}.name"),
                json!([format!(
                    "name in pact '{name}' does not match name in URL path '{expected}'."
                )]),
            );
        }
    }
    if !errors.is_empty() {
        return Err(Error::Validation(Value::Object(errors)));
    }
    let content = with_names(body, consumer, provider);
    let c = find_or_create_pacticipant(db, consumer).await?;
    let p = find_or_create_pacticipant(db, provider).await?;
    let (v, created) = db.find_or_create_version(c.id, version).await?;
    let v = if created {
        v
    } else {
        db.touch_version(v.id, None).await?
    };
    store_pact(db, &c, &v, &p, &content).await
}

/// Puts `consumer` and `provider` first, filling them from the URL when missing, and adds
/// an empty `metadata` when absent, as the Ruby broker serves such a pact (verified).
fn with_names(body: Value, consumer: &str, provider: &str) -> Value {
    let mut out = Map::new();
    let body = match body {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    out.insert(
        "consumer".into(),
        body.get("consumer")
            .cloned()
            .unwrap_or_else(|| json!({ "name": consumer })),
    );
    out.insert(
        "provider".into(),
        body.get("provider")
            .cloned()
            .unwrap_or_else(|| json!({ "name": provider })),
    );
    for (k, v) in body {
        if k != "consumer" && k != "provider" {
            out.insert(k, v);
        }
    }
    out.entry("metadata").or_insert_with(|| json!({}));
    Value::Object(out)
}

// --- contracts/publish ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractRequest {
    pub consumer: String,
    pub provider: String,
    pub content: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRequest {
    pub pacticipant: String,
    pub version: String,
    pub branch: Option<String>,
    pub tags: Vec<String>,
    pub build_url: Option<String>,
    pub contracts: Vec<ContractRequest>,
}

impl PublishRequest {
    /// Validates the request body, collecting every error the Ruby broker reports.
    pub fn parse(body: &Value) -> Result<PublishRequest> {
        let mut errors: Map<String, Value> = Map::new();
        let mut push = |field: &str, msg: String| {
            errors
                .entry(field)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(json!(msg));
        };
        let string_field = |body: &Value,
                            field: &str,
                            required: bool,
                            push: &mut dyn FnMut(&str, String)|
         -> Option<String> {
            match body.get(field) {
                None | Some(Value::Null) => {
                    if required {
                        push(field, "is missing".into());
                    }
                    None
                }
                Some(Value::String(s)) => {
                    if s.trim().is_empty() {
                        push(field, "cannot be blank".into());
                        None
                    } else {
                        Some(s.clone())
                    }
                }
                Some(_) => {
                    push(field, "must be a string".into());
                    None
                }
            }
        };
        let pacticipant = string_field(body, "pacticipantName", true, &mut push);
        let version = string_field(body, "pacticipantVersionNumber", true, &mut push);
        let tags: Vec<String> = match body.get("tags") {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(a)) => {
                let mut out = vec![];
                for t in a {
                    match t.as_str() {
                        Some(s) => out.push(s.to_owned()),
                        None => push("tags", "must be an array of strings".into()),
                    }
                }
                out
            }
            Some(_) => {
                push("tags", "must be an array".into());
                vec![]
            }
        };
        let branch = string_field(body, "branch", false, &mut push);
        let build_url = body
            .get("buildUrl")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let mut contracts = vec![];
        match body.get("contracts") {
            None | Some(Value::Null) => push("contracts", "is missing".into()),
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    if let Some(c) = parse_contract(item, i, pacticipant.as_deref(), &mut push) {
                        contracts.push(c);
                    }
                }
            }
            Some(_) => push("contracts", "must be an array".into()),
        }
        if !errors.is_empty() {
            return Err(Error::Validation(Value::Object(errors)));
        }
        // The Ruby broker answers an empty array with a 500 after validation; pactling
        // reports it, but only once everything else is valid, so other fixtures match.
        if contracts.is_empty() {
            return Err(Error::Validation(
                json!({ "contracts": ["cannot be empty"] }),
            ));
        }
        Ok(PublishRequest {
            pacticipant: pacticipant.expect("validated"),
            version: version.expect("validated"),
            branch,
            tags,
            build_url,
            contracts,
        })
    }
}

fn parse_contract(
    item: &Value,
    i: usize,
    pacticipant: Option<&str>,
    push: &mut dyn FnMut(&str, String),
) -> Option<ContractRequest> {
    let at = format!(" (at index {i})");
    let mut ok = true;
    let field = |name: &str| item.get(name).and_then(|v| v.as_str()).map(str::to_owned);
    let consumer = field("consumerName");
    let provider = field("providerName");
    for (name, value) in [("consumerName", &consumer), ("providerName", &provider)] {
        if value.is_none() {
            push("contracts", format!("{name} is missing{at}"));
            ok = false;
        }
    }
    let content_type = field("contentType");
    if content_type.as_deref() != Some("application/json") {
        push(
            "contracts",
            format!("contentType must be one of: application/json{at}"),
        );
        ok = false;
    }
    if field("specification").as_deref() != Some("pact") {
        push(
            "contracts",
            format!("specification must be one of: pact{at}"),
        );
        ok = false;
    }
    let raw = match item.get("content").and_then(|v| v.as_str()) {
        Some(s) => s.to_owned(),
        None => {
            push("contracts", format!("content is missing{at}"));
            return None;
        }
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .ok();
    if decoded.is_none() {
        push("contracts", format!("is not valid Base64{at}"));
    }
    if content_type.as_deref() != Some("application/json") {
        return None;
    }
    let content: Option<Value> = decoded.and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let Some(content) = content else {
        push(
            "contracts",
            format!("content could not be parsed as application/json{at}"),
        );
        return None;
    };
    if let (Some(consumer), Some(provider)) = (&consumer, &provider) {
        for (key, param_name, param) in [
            ("consumer", "consumerName", consumer),
            ("provider", "providerName", provider),
        ] {
            if let Some(in_content) = content
                .get(key)
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
                && in_content != param
            {
                push(
                    "contracts",
                    format!(
                        "{key} name in contract content ('{in_content}') must match {param_name} in contract params ('{param}'){at}"
                    ),
                );
                ok = false;
            }
        }
        if let Some(p) = pacticipant
            && consumer != p
        {
            push(
                "contracts",
                format!("consumerName ('{consumer}') must match pacticipantName ('{p}'){at}"),
            );
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    Some(ContractRequest {
        consumer: consumer?,
        provider: provider?,
        content,
    })
}

/// One published contract and what happened to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedContract {
    pub publication: PactPublication,
    /// False when the same content was already published for this version.
    pub created: bool,
    /// `contract_published` and, when the content is new, `contract_content_changed`.
    pub events: Vec<&'static str>,
    /// The Ruby broker's explanations, in the order of the tags looked at.
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishOutcome {
    pub pacticipant: Pacticipant,
    pub version: Version,
    pub version_created: bool,
    pub contracts: Vec<PublishedContract>,
}

/// `POST /contracts/publish`. Conflicts are detected before anything is written.
pub async fn publish_contracts(db: &Db, req: &PublishRequest) -> Result<PublishOutcome> {
    let pacticipant = find_or_create_pacticipant(db, &req.pacticipant).await?;
    if let Some(existing) = db.find_version(&pacticipant.name, &req.version).await? {
        for c in &req.contracts {
            if let Some(provider) = db.find_pacticipant(&c.provider).await?
                && let Some(publication) = db.find_publication(existing.id, provider.id).await?
                && publication.sha != content::pact_version_sha(&c.content)
            {
                return Err(Error::Conflict(content_changed_message(
                    &c.consumer,
                    &req.version,
                    &c.provider,
                )));
            }
        }
    }
    let (version, version_created) = db
        .find_or_create_version(pacticipant.id, &req.version)
        .await?;
    let version = if version_created && req.build_url.is_none() {
        version
    } else {
        db.touch_version(version.id, req.build_url.as_deref().map(Some))
            .await?
    };
    if let Some(branch) = &req.branch {
        versions::put_branch_version(db, &pacticipant.name, branch, &version.number).await?;
    }
    for tag in &req.tags {
        versions::put_tag(db, &pacticipant.name, &version.number, tag).await?;
    }
    let tag_names: Vec<String> = db
        .tags_for_version(version.id)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect();
    let mut contracts = vec![];
    for c in &req.contracts {
        let provider = find_or_create_pacticipant(db, &c.provider).await?;
        let (publication, created) =
            store_pact(db, &pacticipant, &version, &provider, &c.content).await?;
        let (events, reasons) = if created {
            detect_events(db, &publication, &tag_names).await?
        } else {
            (vec!["contract_published"], vec![])
        };
        contracts.push(PublishedContract {
            publication,
            created,
            events,
            reasons,
        });
    }
    // The pacticipant may have gained a main branch above.
    let pacticipant = db
        .find_pacticipant(&pacticipant.name)
        .await?
        .expect("exists");
    Ok(PublishOutcome {
        pacticipant,
        version,
        version_created,
        contracts,
    })
}

/// The Ruby broker compares the new content with the previous pact per consumer version
/// tag, or with the previous untagged version when there are no tags (verified: branches
/// play no part).
async fn detect_events(
    db: &Db,
    publication: &PactPublication,
    tags: &[String],
) -> Result<(Vec<&'static str>, Vec<String>)> {
    let mut reasons = vec![];
    let mut changed = false;
    if tags.is_empty() {
        let previous = db
            .previous_pact_sha_untagged(
                publication.consumer_id,
                publication.provider_id,
                publication.consumer_version_id,
            )
            .await?;
        match previous {
            None => {
                changed = true;
                reasons.push("first time untagged pact published".to_owned());
            }
            Some(sha) if sha != publication.sha => {
                changed = true;
                reasons.push("pact content has changed since previous untagged version".to_owned());
            }
            Some(_) => {}
        }
    } else {
        for tag in tags {
            let previous = db
                .previous_pact_sha_with_tag(
                    publication.consumer_id,
                    publication.provider_id,
                    publication.consumer_version_id,
                    tag,
                )
                .await?;
            match previous {
                None => {
                    changed = true;
                    reasons.push(format!("first time any pact published for this consumer with consumer version tagged {tag}"));
                }
                Some(sha) if sha != publication.sha => {
                    changed = true;
                    reasons.push(format!(
                        "pact content has changed since the last consumer version tagged with {tag}"
                    ));
                }
                Some(_) => {}
            }
        }
    }
    if changed {
        Ok((
            vec!["contract_published", "contract_content_changed"],
            reasons,
        ))
    } else {
        Ok((
            vec!["contract_published"],
            vec![format!(
                "pact content is the same as previous versions with tags {} and no new tags were applied",
                tags.join(", ")
            )],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_names_fills_missing_names_first_and_adds_metadata() {
        let out = with_names(json!({"interactions": []}), "C", "P");
        let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["consumer", "provider", "interactions", "metadata"]);
        assert_eq!(out["consumer"]["name"], "C");
        let kept = with_names(
            json!({"metadata": {"x": 1}, "consumer": {"name": "C"}}),
            "C",
            "P",
        );
        assert_eq!(kept["metadata"], json!({"x": 1}));
        assert_eq!(kept["provider"]["name"], "P");
    }

    #[test]
    fn parse_collects_every_error() {
        let err = PublishRequest::parse(&json!({})).unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"pacticipantName": ["is missing"], "pacticipantVersionNumber": ["is missing"], "contracts": ["is missing"]}))
        );
        let err = PublishRequest::parse(&json!({"pacticipantName": "C", "pacticipantVersionNumber": "1", "contracts": [{"consumerName": "C", "providerName": "P", "specification": "pact", "contentType": "application/json", "content": "!!"}]})).unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"contracts": ["is not valid Base64 (at index 0)", "content could not be parsed as application/json (at index 0)"]}))
        );
    }

    #[test]
    fn parse_accepts_a_valid_request() {
        let content = base64::engine::general_purpose::STANDARD
            .encode(r#"{"consumer":{"name":"C"},"provider":{"name":"P"},"interactions":[]}"#);
        let req = PublishRequest::parse(&json!({"pacticipantName": "C", "pacticipantVersionNumber": "1", "branch": "main", "tags": ["dev"], "buildUrl": "https://ci/1",
            "contracts": [{"consumerName": "C", "providerName": "P", "specification": "pact", "contentType": "application/json", "content": content}]})).unwrap();
        assert_eq!(req.branch.as_deref(), Some("main"));
        assert_eq!(req.tags, ["dev"]);
        assert_eq!(req.build_url.as_deref(), Some("https://ci/1"));
        assert_eq!(req.contracts[0].provider, "P");
        assert_eq!(req.contracts[0].content["interactions"], json!([]));
    }
}
