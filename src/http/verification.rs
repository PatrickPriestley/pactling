//! Pacts for verification and verification results. Shapes and texts follow the Phase 3
//! capture (tests/verification.rs).
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::{pacts::PactPublication, verifications::Verification};
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_document;
use crate::service::verification::{
    self as service, ForVerificationRequest, Reason, ResultRequest, VerifiablePact,
};

const FOR_VERIFICATION_ALLOWED: &str = "GET, POST, OPTIONS";
const RESULTS_ALLOWED: &str = "POST, OPTIONS";
const RESULT_ALLOWED: &str = "GET, OPTIONS, DELETE";

const BETA_WARNING: &str = "WARNING - this version of the Pact library uses a beta version of the API which will be removed in the future. Please upgrade your Pact library. See https://docs.pact.io/pact_broker/advanced_topics/provider_verification_results/#pacts-for-verification for minimum required versions.";

fn parse_body(body: &Bytes) -> Result<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))
}

fn pact_version_href(l: &Links, p: &PactPublication) -> String {
    l.href(&format!(
        "/pacts/provider/{}/consumer/{}/pact-version/{}",
        encode(&p.provider_name),
        encode(&p.consumer_name),
        p.sha
    ))
}

fn encode_metadata(query: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(query)
}

/// Decodes the metadata of a pact link and returns the consumer version id it names, if any.
pub fn consumer_version_from_metadata(m: &str) -> Option<i64> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(m.trim_end_matches('='))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(m))
        .ok()?;
    let query = String::from_utf8(bytes).ok()?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        let k = k.replace("%5B", "[").replace("%5D", "]");
        (k == "cv" || k == "s[][cv]")
            .then(|| v.parse().ok())
            .flatten()
    })
}

/// The Ruby broker's metadata for a selected pact: one `s[]` group per reason, each
/// naming the consumer version it matched, plus the pending flag. Values are
/// form-encoded so a branch or tag name cannot inject a pair.
fn selection_metadata(v: &VerifiablePact) -> String {
    let mut parts: Vec<String> = vec![];
    for r in &v.reasons {
        for (k, val) in &r.metadata {
            parts.push(format!("s%5B%5D%5B{k}%5D={}", encode(val)));
        }
        parts.push(format!("s%5B%5D%5Bcv%5D={}", r.consumer_version_id));
    }
    parts.push(format!("p={}", v.pending.unwrap_or(false)));
    encode_metadata(&parts.join("&"))
}

/// `GET | POST /pacts/provider/{p}/for-verification`
pub async fn for_verification(
    m: Method,
    State(state): State<AppState>,
    Path(provider): Path<String>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET | Method::POST => {
            let p = state.db.find_pacticipant(&provider).await?.ok_or_else(|| {
                Error::NotFound(format!("No provider with name '{provider}' found"))
            })?;
            let req = if m == Method::GET {
                ForVerificationRequest {
                    selectors: vec![],
                    include_pending_status: true,
                    provider_version_branch: None,
                    provider_version_tags: vec![],
                }
            } else {
                ForVerificationRequest::parse(&parse_body(&body)?)?
            };
            let pacts = service::pacts_for_verification(&state.db, &p, &req).await?;
            let entries: Vec<Value> = pacts
                .iter()
                .map(|v| verifiable_pact_document(&l, v, &req, &p.name, m == Method::GET))
                .collect();
            Ok(Hal(json!({
                "_embedded": { "pacts": entries },
                "_links": { "self": { "href": l.href(&format!("/pacts/provider/{}/for-verification", encode(&provider))), "title": "Pacts to be verified" } }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(FOR_VERIFICATION_ALLOWED)),
        _ => Ok(method::method_not_allowed(FOR_VERIFICATION_ALLOWED)),
    }
}

fn verifiable_pact_document(
    l: &Links,
    v: &VerifiablePact,
    req: &ForVerificationRequest,
    provider: &str,
    beta: bool,
) -> Value {
    let p = &v.publication;
    let short: Vec<&str> = v.reasons.iter().map(|r| r.short.as_str()).collect();
    let mut notices: Vec<Value> = vec![];
    if beta {
        notices.push(json!({ "when": "before_verification", "text": BETA_WARNING }));
    }
    notices.push(json!({ "when": "before_verification", "text": selection_notice(&pact_version_href(l, p), &v.reasons) }));
    if let Some(pending) = v.pending {
        notices.extend(pending_notices(provider, pending, req));
    }
    let mut props = Map::new();
    if let Some(pending) = v.pending {
        props.insert("pending".into(), json!(pending));
    }
    props.insert("notices".into(), Value::Array(notices));
    json!({
        "shortDescription": short.join(", "),
        "verificationProperties": Value::Object(props),
        "_links": { "self": {
            "href": format!("{}/metadata/{}", pact_version_href(l, p), selection_metadata(v)),
            "name": format!("Pact between {} ({}) and {}", p.consumer_name, p.consumer_version_number, p.provider_name)
        } }
    })
}

fn selection_notice(href: &str, reasons: &[Reason]) -> String {
    let (noun, criteria) = if reasons.len() == 1 {
        ("version", "criterion")
    } else {
        ("versions", "criteria")
    };
    let bullets: Vec<String> = reasons
        .iter()
        .map(|r| format!("    * {}", r.bullet))
        .collect();
    format!(
        "The pact at {href} is being verified because the pact content belongs to the consumer {noun} matching the following {criteria}:\n{}",
        bullets.join("\n")
    )
}

/// The pending notices, all verified against fixtures (branch, tag and no-branch forms).
fn pending_notices(provider: &str, pending: bool, req: &ForVerificationRequest) -> Vec<Value> {
    let n = |when: &str, text: String| json!({ "when": when, "text": text });
    if !req.provider_version_tags.is_empty() {
        let tags: Vec<String> = req
            .provider_version_tags
            .iter()
            .map(|t| format!("'{t}'"))
            .collect();
        let tag = tags.join(", ");
        if !pending {
            return vec![n(
                "before_verification",
                format!(
                    "This pact has previously been successfully verified by a version of {provider} with tag {tag}. If this verification fails, it will fail the build. Read more at https://docs.pact.io/go/pending"
                ),
            )];
        }
        return vec![
            n(
                "before_verification",
                format!(
                    "This pact is in pending state for this version of {provider} because a successful verification result for a version of {provider} with tag {tag} has not yet been published. If this verification fails, it will not cause the overall build to fail. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_true_published_false",
                format!(
                    "This pact is still in pending state for any version of {provider} with tag {tag} as the successful verification results with this tag have not yet been published."
                ),
            ),
            n(
                "after_verification:success_false_published_false",
                format!(
                    "This pact is still in pending state for any version of {provider} with tag {tag} as a successful verification result with this tag has not yet been published"
                ),
            ),
            n(
                "after_verification:success_true_published_true",
                format!(
                    "This pact is no longer in pending state for any version of {provider} with tag {tag}, as a successful verification result with this tag has been published. If a verification for a version of {provider} with this tag fails in the future, it will fail the build. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_false_published_true",
                format!(
                    "This pact is still in pending state for any version of {provider} with tag {tag} as a successful verification result with this tag has not yet been published"
                ),
            ),
        ];
    }
    match &req.provider_version_branch {
        Some(b) if !pending => vec![n(
            "before_verification",
            format!(
                "This pact has previously been successfully verified by a version of {provider} from branch '{b}'. If this verification fails, it will fail the build. Read more at https://docs.pact.io/go/pending"
            ),
        )],
        None if !pending => vec![n(
            "before_verification",
            format!(
                "This pact has previously been successfully verified by {provider}. If this verification fails, it will fail the build. Read more at https://docs.pact.io/go/pending"
            ),
        )],
        Some(b) => vec![
            n(
                "before_verification",
                format!(
                    "This pact is in pending state for this version of {provider} because a successful verification result for a version of {provider} from branch '{b}' has not yet been published. If this verification fails, it will not cause the overall build to fail. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_true_published_false",
                format!(
                    "This pact is still in pending state for any version of {provider} from branch '{b}' as the successful verification results have not yet been published."
                ),
            ),
            n(
                "after_verification:success_false_published_false",
                format!(
                    "This pact is still in pending state for any version of {provider} from branch '{b}' as a successful verification result has not yet been published"
                ),
            ),
            n(
                "after_verification:success_true_published_true",
                format!(
                    "This pact is no longer in pending state for any version of {provider} from branch '{b}', as a successful verification result has been published. If a verification for a version of {provider} fails in the future, it will fail the build. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_false_published_true",
                format!(
                    "This pact is still in pending state for any version of {provider} from branch '{b}' as a successful verification result has not yet been published"
                ),
            ),
        ],
        None => vec![
            n(
                "before_verification",
                format!(
                    "This pact is in pending state for this version of {provider} because a successful verification result for {provider} has not yet been published. If this verification fails, it will not cause the overall build to fail. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_true_published_false",
                format!(
                    "This pact is still in pending state for {provider} as the successful verification results have not yet been published."
                ),
            ),
            n(
                "after_verification:success_false_published_false",
                format!(
                    "This pact is still in pending state for {provider} as a successful verification result has not yet been published"
                ),
            ),
            n(
                "after_verification:success_true_published_true",
                format!(
                    "This pact is no longer in pending state for {provider}, as a successful verification result has been published. If a verification for a version of {provider} fails in the future, it will fail the build. Read more at https://docs.pact.io/go/pending"
                ),
            ),
            n(
                "after_verification:success_false_published_true",
                format!(
                    "This pact is still in pending state for {provider} as a successful verification result has not yet been published"
                ),
            ),
        ],
    }
}

// --- pact by sha with metadata --------------------------------------------------------

/// `GET /pacts/provider/{p}/consumer/{c}/pact-version/{sha}/metadata/{m}`: the pact
/// document whose verification-results href carries the same metadata.
pub async fn pact_with_metadata(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha, metadata)): Path<(String, String, String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .find_pact_by_sha(&provider, &consumer, &sha)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(Hal(pact_document(&l, &p, Some(&metadata))).into_response())
        }
        Method::OPTIONS => Ok(method::options("GET, OPTIONS")),
        _ => Ok(method::method_not_allowed("GET, OPTIONS")),
    }
}

// --- results ----------------------------------------------------------------------------

async fn publish(
    state: &AppState,
    l: &Links,
    provider: &str,
    consumer: &str,
    sha: &str,
    metadata: Option<&str>,
    body: &Bytes,
) -> Result<Response> {
    let pact = state
        .db
        .find_pact_by_sha(provider, consumer, sha)
        .await?
        .ok_or(Error::NoRoute)?;
    let req = ResultRequest::parse(&parse_body(body)?)?;
    // Only honour a consumer version that actually published this pact content.
    let consumer_version_id = match metadata.and_then(consumer_version_from_metadata) {
        Some(id) => state
            .db
            .publications_for_pact_version(pact.pact_version_id)
            .await?
            .iter()
            .any(|p| p.consumer_version_id == id)
            .then_some(id),
        None => None,
    };
    let tx = state.db.begin().await?;
    let v = service::publish_result(&tx, &pact, &req, consumer_version_id).await?;
    crate::service::webhook_trigger::trigger_verification_events(
        &tx,
        &state.config,
        &l.href(""),
        &pact,
        &v,
    )
    .await?;
    tx.commit().await?;
    let doc = verification_document(l, &v, &pact, state).await?;
    let mut res = (StatusCode::CREATED, Hal(doc)).into_response();
    let location = format!(
        "{}/verification-results/{}",
        pact_version_href(l, &pact),
        v.number
    );
    res.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("ascii"),
    );
    Ok(res)
}

/// `POST .../pact-version/{sha}/verification-results`
pub async fn results(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha)): Path<(String, String, String)>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::POST => publish(&state, &l, &provider, &consumer, &sha, None, &body).await,
        Method::OPTIONS => Ok(method::options(RESULTS_ALLOWED)),
        _ => Ok(method::method_not_allowed(RESULTS_ALLOWED)),
    }
}

/// `POST .../pact-version/{sha}/metadata/{m}/verification-results`
pub async fn results_with_metadata(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha, metadata)): Path<(String, String, String, String)>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::POST => {
            publish(
                &state,
                &l,
                &provider,
                &consumer,
                &sha,
                Some(&metadata),
                &body,
            )
            .await
        }
        Method::OPTIONS => Ok(method::options(RESULTS_ALLOWED)),
        _ => Ok(method::method_not_allowed(RESULTS_ALLOWED)),
    }
}

/// The verification result document. The consumer version named is the one the result
/// was recorded for, falling back to the pact's newest consumer version.
pub async fn verification_document(
    l: &Links,
    v: &Verification,
    pact: &PactPublication,
    state: &AppState,
) -> Result<Value> {
    let consumer_version = match v.consumer_version_id {
        Some(id) => state
            .db
            .find_version_by_id(id)
            .await?
            .map(|cv| cv.number)
            .unwrap_or_else(|| pact.consumer_version_number.clone()),
        None => pact.consumer_version_number.clone(),
    };
    let name = format!(
        "Pact between {} ({}) and {}",
        pact.consumer_name, consumer_version, pact.provider_name
    );
    let base = pact_version_href(l, pact);
    let mut doc = Map::new();
    doc.insert("providerName".into(), json!(v.provider_name));
    doc.insert(
        "providerApplicationVersion".into(),
        json!(v.provider_version_number),
    );
    doc.insert("success".into(), json!(v.success));
    doc.insert("verificationDate".into(), json!(v.created_at));
    if let Some(results) = &v.test_results
        && let Ok(parsed) = serde_json::from_str::<Value>(results)
    {
        doc.insert("testResults".into(), parsed);
    }
    doc.insert(
        "verifiedBy".into(),
        flatten_verified_by(v.verified_by.as_deref()),
    );
    doc.insert("_links".into(), json!({
        "self": { "title": "Verification result", "name": format!("Verification result {} for {name}", v.number), "href": format!("{base}/verification-results/{}", v.number) },
        "pb:pact-version": { "title": "Pact", "name": name, "href": format!("{base}/metadata/{}", encode_metadata(&format!("cvn={consumer_version}"))) },
        "pb:triggered-webhooks": { "title": "Webhooks triggered by the publication of this verification result", "href": format!("{base}/verification-results/{}/triggered-webhooks", v.number) }
    }));
    Ok(Value::Object(doc))
}

/// `verifiedBy` as the Ruby broker serves it: `clientLanguage` flattened into
/// `client_implementation`, `client_test_framework` and `client_version`.
fn flatten_verified_by(raw: Option<&str>) -> Value {
    let Some(parsed) = raw.and_then(|s| serde_json::from_str::<Value>(s).ok()) else {
        return json!({});
    };
    let mut out = Map::new();
    if let Some(i) = parsed.get("implementation") {
        out.insert("implementation".into(), i.clone());
    }
    if let Some(v) = parsed.get("version") {
        out.insert("version".into(), v.clone());
    }
    if let Some(cl) = parsed.get("clientLanguage") {
        for (from, to) in [
            ("name", "client_implementation"),
            ("testFramework", "client_test_framework"),
            ("version", "client_version"),
        ] {
            if let Some(v) = cl.get(from) {
                out.insert(to.into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

/// `GET .../pact-version/{sha}/verification-results/{n}` where `n` is a number or `latest`
pub async fn result(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha, n)): Path<(String, String, String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let pact = state
                .db
                .find_pact_by_sha(&provider, &consumer, &sha)
                .await?
                .ok_or(Error::NoRoute)?;
            let v = if n == "latest" {
                state
                    .db
                    .latest_verification_for_pact_version(pact.pact_version_id)
                    .await?
            } else {
                match n.parse::<i64>() {
                    Ok(number) => state
                        .db
                        .find_verification_by_number(number)
                        .await?
                        .filter(|v| v.pact_version_id == pact.pact_version_id),
                    Err(_) => None,
                }
            }
            .ok_or(Error::NoRoute)?;
            Ok(Hal(verification_document(&l, &v, &pact, &state).await?).into_response())
        }
        Method::OPTIONS => Ok(method::options(RESULT_ALLOWED)),
        _ => Ok(method::method_not_allowed(RESULT_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/version/{v}/verification-results/latest`
pub async fn latest_result_for_version(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, version)): Path<(String, String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let pact = state
                .db
                .find_pact(&provider, &consumer, &version)
                .await?
                .ok_or(Error::NoRoute)?;
            let v = state
                .db
                .latest_verification_for_pact_version(pact.pact_version_id)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(Hal(verification_document(&l, &v, &pact, &state).await?).into_response())
        }
        Method::OPTIONS => Ok(method::options(RESULT_ALLOWED)),
        _ => Ok(method::method_not_allowed(RESULT_ALLOWED)),
    }
}

/// `GET /verification-results/consumer/{c}/version/{v}/latest`: the latest result of each
/// provider's pact for the consumer version, with a summary.
pub async fn latest_results_for_consumer_version(
    m: Method,
    State(state): State<AppState>,
    Path((consumer, version)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let cv = state
                .db
                .find_version(&consumer, &version)
                .await?
                .ok_or(Error::NoRoute)?;
            let mut successful = vec![];
            let mut failed = vec![];
            let mut unknown = vec![];
            let mut results = vec![];
            for (provider, v) in state
                .db
                .latest_verifications_for_consumer_version(cv.id)
                .await?
            {
                match v {
                    Some(v) => {
                        if v.success {
                            successful.push(provider.clone())
                        } else {
                            failed.push(provider.clone())
                        }
                        let pact = state
                            .db
                            .find_pact(&provider, &consumer, &version)
                            .await?
                            .ok_or(Error::NoRoute)?;
                        results.push(verification_document(&l, &v, &pact, &state).await?);
                    }
                    None => unknown.push(provider),
                }
            }
            Ok(Hal(json!({
                "success": failed.is_empty() && unknown.is_empty(),
                "providerSummary": { "successful": successful, "failed": failed, "unknown": unknown },
                "_embedded": { "verificationResults": results },
                "_links": { "self": {
                    "href": l.href(&format!("/verification-results/consumer/{}/version/{}/latest", encode(&consumer), encode(&version))),
                    "title": format!("Latest verification results for consumer {consumer} version {version}")
                } }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options("GET, OPTIONS")),
        _ => Ok(method::method_not_allowed("GET, OPTIONS")),
    }
}
