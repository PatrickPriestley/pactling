//! Additive, versioned view of the same evaluated decision as can-i-deploy.
use crate::{
    app::AppState,
    error::Error,
    http::{
        hal::{Links, encode},
        method,
        pacts::pact_href,
    },
    service::{
        deployment_explanations::{explain, selector_document},
        matrix::{
            self, MatrixQuery, Row,
            evaluation::{Cause, DecisionEvaluation},
            provenance::{ExclusionKind, MembershipSource},
        },
    },
};
use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

const ALLOWED: &str = "GET, OPTIONS";

pub async fn explanation(
    m: Method,
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    links: Links,
) -> Response {
    let mut response = match m {
        Method::GET => {
            let q = MatrixQuery::parse(query.as_deref());
            match validate_explanation_query(&q) {
                Err(e) => error_response(e),
                Ok(()) => match matrix::decision(&state.db, &q).await {
                    Ok(evaluation) => Json(document(
                        &links,
                        &evaluation,
                        query.as_deref().unwrap_or_default(),
                    ))
                    .into_response(),
                    Err(e) => error_response(e),
                },
            }
        }
        Method::OPTIONS => method::options(ALLOWED),
        _ => method::method_not_allowed(ALLOWED),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The extension accepts matrix selectors, but requires a deployment target.
/// Compatibility can-i-deploy keeps its separate required version-number check.
pub(crate) fn validate_explanation_query(q: &MatrixQuery) -> crate::error::Result<()> {
    let mut errors = serde_json::Map::new();
    if q.selectors.is_empty() {
        errors.insert("pacticipant".into(), json!(["is missing"]));
    }
    if q.environment.is_none() && q.tag.is_none() {
        errors.insert(
            "".into(),
            json!(["Must specify either an environment or a 'to' tag."]),
        );
    }
    if !errors.is_empty() {
        return Err(Error::Validation(Value::Object(errors)));
    }
    Ok(())
}

fn error_response(error: Error) -> Response {
    match error {
        Error::Validation(details) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":details}})),
        )
            .into_response(),
        other => other.into_response(),
    }
}

fn candidate_refs(indices: &[usize]) -> Vec<String> {
    indices
        .iter()
        .map(|i| format!("/resolution/candidates/{i}"))
        .collect()
}

fn contract_href(l: &Links, row: &Row) -> String {
    l.href(&format!(
        "/pacts/provider/{}/consumer/{}/pact-version/{}",
        encode(&row.pact.provider_name),
        encode(&row.pact.consumer_name),
        row.pact.sha
    ))
}

fn verification_href(l: &Links, row: &Row) -> Option<String> {
    row.verification.as_ref().map(|v| {
        format!(
            "{}/verification-results/{}",
            contract_href(l, row),
            v.number
        )
    })
}

fn row_document(l: &Links, row: &Row, id: String, disposition: &str, indices: &[usize]) -> Value {
    json!({
        "id":id,"consumer":row.consumer.pacticipant.name,"consumerVersion":row.consumer.version.as_ref().map(|v| &v.number),
        "provider":row.provider.pacticipant.name,"providerVersion":row.provider.version.as_ref().map(|v| &v.number),
        "candidateRefs":candidate_refs(indices),"disposition":disposition,
        "contract":{"hash":{"algorithm":"sha1","value":row.pact.sha},"href":contract_href(l,row),"publicationHref":pact_href(l,&row.pact.provider_name,&row.pact.consumer_name,&row.pact.consumer_version_number)},
        "verification":row.verification.as_ref().map(|v| json!({"number":v.number,"success":v.success,"verifiedAt":v.created_at,"href":verification_href(l,row)}))
    })
}

/// Pure rendering: the decision and all referenced records have already been evaluated.
pub fn document(l: &Links, evaluation: &DecisionEvaluation, query: &str) -> Value {
    let projection = explain(evaluation);
    let matrix = evaluation.matrix();
    let provenance = evaluation.provenance();
    let q = evaluation.query();
    let selectors: Vec<_> = provenance.selectors.iter().map(|s| json!({"index":s.index,"pacticipant":s.requested.pacticipant,"versions":s.versions.iter().map(|v| &v.number).collect::<Vec<_>>(),"missingReason":s.missing.then_some("SELECTOR_VERSION_MISSING"),"requested":selector_document(&s.requested)})).collect();
    let target = if q.selectors.len() > 1 {
        json!({"kind":"selected_versions","selectorIndices":(0..q.selectors.len()).collect::<Vec<_>>()})
    } else if let Some(env) = &provenance.environment {
        json!({"kind":"environment","name":env.name,"uuid":env.uuid,"href":l.href(&format!("/environments/{}",encode(&env.uuid)))})
    } else if q.latest {
        json!({"kind":"latest","tag":q.tag})
    } else {
        json!({"kind":"any"})
    };
    let candidates: Vec<_> = provenance.candidates.iter().enumerate().map(|(i,c)| {
        let sources: Vec<_> = c.sources.iter().map(|source| match source {
            MembershipSource::Deployed(d) => json!({"kind":"deployed","uuid":d.uuid,"href":l.href(&format!("/deployed-versions/{}",encode(&d.uuid))),"applicationInstance":d.application_instance,"recordedAt":d.created_at}),
            MembershipSource::Released(r) => json!({"kind":"released","uuid":r.uuid,"href":l.href(&format!("/released-versions/{}",encode(&r.uuid))),"applicationInstance":null,"recordedAt":r.created_at}),
        }).collect();
        json!({"id":format!("candidate-{}",i+1),"pacticipant":c.version.pacticipant_name,"version":c.version.number,"selectorIndices":c.selector_indices,"sources":sources})
    }).collect();
    let rows: Vec<_> = matrix
        .rows
        .iter()
        .zip(&provenance.rows)
        .enumerate()
        .map(|(i, (row, origin))| {
            let mut doc = row_document(
                l,
                row,
                format!("row-{}", i + 1),
                if row.ignored { "ignored" } else { "considered" },
                &origin.candidate_indices,
            );
            doc["selectorIndices"] = json!(origin.selector_indices);
            doc["ignoreRefs"] = json!(
                origin
                    .ignore_indices
                    .iter()
                    .map(|i| format!("/resolution/ignores/{i}"))
                    .collect::<Vec<_>>()
            );
            doc
        })
        .collect();
    let excluded: Vec<_> = provenance.excluded.iter().enumerate().map(|(i,e)| json!({
        "kind":match e.kind {ExclusionKind::UnverifiedCandidate=>"unverified_candidate",ExclusionKind::Duplicate=>"duplicate",ExclusionKind::Latestby=>"latestby"},
        "evidence":row_document(l,&e.row,format!("excluded-{}",i+1),"excluded",&e.candidate_indices),
        "candidateRefs":candidate_refs(&e.candidate_indices),"selectorIndices":e.selector_indices,
        "selectedRowRef":e.selected_row.map(|i|format!("/rows/{i}"))
    })).collect();
    let ignores: Vec<_> = q
        .ignore
        .iter()
        .enumerate()
        .map(|(i, (participant, version))| {
            let status = matrix
                .notices
                .iter()
                .find_map(|n| match &n.cause {
                    Cause::IgnoreParticipantMissing(p) if p == participant => {
                        Some("participant_missing")
                    }
                    Cause::IgnoreVersionMissing {
                        participant: p,
                        version: v,
                    } if p == participant && Some(v) == version.as_ref() => Some("version_missing"),
                    _ => None,
                })
                .unwrap_or("resolved");
            json!({"index":i,"pacticipant":participant,"version":version,"status":status})
        })
        .collect();
    let reasons: Vec<_> = projection.reasons.iter().map(|r|json!({"code":r.code,"role":r.role,"message":r.message,"evidenceRefs":r.evidence_refs,"actionIds":r.action_ids})).collect();
    let actions: Vec<_> = projection.actions.iter().map(|a| {
        let mut target=a.target.clone();
        if let Some(i)=a.row_index { target["verificationHref"]=json!(verification_href(l,&matrix.rows[i])); }
        json!({"id":a.id,"type":a.kind,"message":a.message,"evidenceRefs":a.evidence_refs,"target":target})
    }).collect();
    let context = evaluation.context();
    json!({"schemaVersion":"1.0","decision":projection.decision,"summary":projection.summary,
        "compatibility":{"deployable":matrix.deployable,"success":matrix.success,"failed":matrix.failed,"unknown":matrix.unknown,"ignored":matrix.ignored,"hasIgnores":matrix.has_ignores},
        "request":{"query":query},"evaluation":{"id":context.id,"startedAt":context.started_at,"evaluatedAt":context.evaluated_at,"brokerVersion":env!("CARGO_PKG_VERSION"),"engineRevision":null,"snapshot":{"kind":"read_transaction","retained":false}},
        "evidence":{"completeness":projection.completeness,"coverage":"recorded_only","currentness":"not_checked"},
        "resolution":{"selectors":selectors,"target":target,"candidates":candidates,"excluded":excluded,"ignores":ignores},"rows":rows,"reasons":reasons,"nextActions":actions})
}
