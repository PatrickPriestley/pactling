//! HTML projection of the existing explanation document, from one shared evaluation.
use super::{ALLOWED, layout, links::link, query};
use crate::{
    app::AppState,
    error::Error,
    http::{
        deployment_explanations,
        hal::{Links, encode},
    },
    service::matrix,
};
use axum::{
    extract::{RawQuery, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use maud::{Markup, html};
use serde_json::Value;

pub async fn explanation(
    method: Method,
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
    links: Links,
) -> Response {
    match method {
        Method::GET => {
            let raw = raw.as_deref().unwrap_or_default();
            if raw.is_empty() {
                return layout::response(
                    StatusCode::OK,
                    layout::page("Deployment explanation", form(raw)),
                );
            }
            match query::explanation_query(raw) {
                Ok((q, canonical)) => match deployment_explanations::validate_explanation_query(&q)
                {
                    Ok(()) => match matrix::decision(&state.db, &q).await {
                        Ok(evaluation) => {
                            let doc =
                                deployment_explanations::document(&links, &evaluation, &canonical);
                            layout::response(
                                StatusCode::OK,
                                layout::page(
                                    "Deployment explanation",
                                    html! {(form(raw))(render_document(&doc))},
                                ),
                            )
                        }
                        Err(e) => error(e, raw),
                    },
                    Err(e) => error(e, raw),
                },
                Err(e) => error(e, raw),
            }
        }
        Method::OPTIONS => crate::http::method::options(ALLOWED),
        _ => crate::http::method::method_not_allowed(ALLOWED),
    }
}
fn error(error: Error, raw: &str) -> Response {
    let detail = match &error {
        Error::BadRequest(message) => message.clone(),
        Error::Validation(details) => details.to_string(),
        _ => "Unable to load explanation.".into(),
    };
    let status = error.into_response().status();
    layout::response(
        status,
        layout::page("Unable to load explanation", html! {p{(detail)}(form(raw))}),
    )
}

fn form(raw: &str) -> Markup {
    let mut fields = query::fields(raw);
    if query::field(&fields, "ui_form") != "explanation" {
        let q = matrix::MatrixQuery::parse(Some(raw));
        fields = vec![];
        if let Some(s) = q.selectors.first() {
            let (mode, value) = if let Some(v) = &s.version {
                ("version", v.as_str())
            } else if let Some(v) = &s.tag {
                ("tag", v.as_str())
            } else if let Some(v) = &s.branch {
                ("branch", v.as_str())
            } else if s.main_branch {
                ("main", "")
            } else if s.latest {
                ("latest", "")
            } else {
                ("all", "")
            };
            fields.extend([
                ("pacticipant".into(), s.pacticipant.clone()),
                ("selector_mode".into(), mode.into()),
                ("selector_value".into(), value.into()),
            ]);
        }
        if let Some(env) = q.environment {
            fields.extend([
                ("target_mode".into(), "environment".into()),
                ("target_value".into(), env),
            ]);
        } else if let Some(tag) = q.tag {
            fields.extend([
                ("target_mode".into(), "tag".into()),
                ("target_value".into(), tag),
            ]);
        }
        fields.extend(q.ignore.into_iter().map(|(n, v)| {
            (
                "ignore".into(),
                match v {
                    Some(v) => format!("{n}={v}"),
                    None => n,
                },
            )
        }));
    }
    html! { h2 {"New explanation"} form method="get" action="/ui/deployment-explanation" {
        input type="hidden" name="ui_form" value="explanation";
        label for="pacticipant" { "Pacticipant" }
        input id="pacticipant" name="pacticipant" value=(query::field(&fields,"pacticipant")) required;
        label for="selector_mode" { "Version selector" }
        select id="selector_mode" name="selector_mode" {
            @for (value,label) in [("version","Exact version"),("latest","Latest version"),("tag","Latest with tag"),("branch","Branch head"),("main","Main branch head"),("all","All versions")] {
                option value=(value) selected[query::field(&fields,"selector_mode")==value] {(label)}
            }
        }
        label for="selector_value" { "Version, tag or branch (only for the selected mode)" }
        input id="selector_value" name="selector_value" value=(query::field(&fields,"selector_value"));
        label for="target_mode" { "Target" }
        select id="target_mode" name="target_mode" {
            @for (value,label) in [("environment","Environment"),("tag","To tag")] {
                option value=(value) selected[query::field(&fields,"target_mode")==value] {(label)}
            }
        }
        label for="target_value" { "Target name" }
        input id="target_value" name="target_value" value=(query::field(&fields,"target_value")) required;
        p { "Optional ignores: NAME or NAME=VERSION. Blank entries are omitted. Submit to keep these entries and add another." }
        @for (i,(_,value)) in fields.iter().filter(|(key,value)|key=="ignore" && !value.is_empty()).chain(std::iter::once(&("ignore".into(),String::new()))).enumerate() {
            label for=(format!("ignore-{i}")) { "Ignore " (i+1) }
            input id=(format!("ignore-{i}")) name="ignore" value=(value);
        }
        button type="submit" { "Explain deployment" }
    } }
}

fn string(v: &Value) -> &str {
    v.as_str().expect("explanation string")
}
fn array(v: &Value) -> &[Value] {
    v.as_array().expect("explanation array")
}
fn nullable(v: &Value) -> &str {
    match v {
        Value::Null => "",
        _ => string(v),
    }
}

/// Pure HTML projection. The caller supplies the existing document from its single evaluation.
pub fn render_document(doc: &Value) -> Markup {
    html! {
        section data-decision=(string(&doc["decision"])) {
            h2 { "Decision: " (string(&doc["decision"])) }
            details { summary {"Evaluated query"} code {(string(&doc["request"]["query"]))} }
            p {(string(&doc["summary"]))}
            p { "Evaluated at " (string(&doc["evaluation"]["evaluatedAt"])) ". Coverage: " (string(&doc["evidence"]["coverage"])) "; currentness: " (string(&doc["evidence"]["currentness"])) ". This reports recorded evidence, not live environment health." }
            h2 {"Reasons"}
            @for reason in array(&doc["reasons"]) {
                section {
                    h3 {code{(string(&reason["code"]))} " · " (string(&reason["role"]))}
                    p {(string(&reason["message"]))}
                    (references(&reason["evidenceRefs"]))
                    @for id in array(&reason["actionIds"]) {a href=(format!("#{}",encode(string(id)))) {"Suggested action " (string(id))} " "}
                }
            }
            h2 {"Next actions"}
            @for (action_index,action) in array(&doc["nextActions"]).iter().enumerate() {
                section id=(encode(string(&action["id"]))) {
                    h3 {code{(string(&action["type"]))}}
                    p {(string(&action["message"]))}
                    (evidence(&action["target"],&format!("/nextActions/{action_index}/target")))
                    (references(&action["evidenceRefs"]))
                }
            }
            h2 {"Matrix evidence"}
            div class="table-scroll" {table {
                thead {tr {@for label in ["Consumer","Consumer version","Provider","Provider version","Verification","Disposition","Evidence"] {th scope="col" {(label)}}}}
                tbody {@for row in array(&doc["rows"]) {
                    tr data-consumer-version=(nullable(&row["consumerVersion"])) data-provider-version=(nullable(&row["providerVersion"])) {
                        td{(string(&row["consumer"]))}td{(nullable(&row["consumerVersion"]))}
                        td{(string(&row["provider"]))}td{(nullable(&row["providerVersion"]))}
                        td{ @match row["verification"]["success"].as_bool() {Some(true)=>{"Successful"}Some(false)=>{"Failed"}None=>{"Unknown"}} }
                        td{(string(&row["disposition"]))}
                        td{(link(string(&row["contract"]["publicationHref"]),"Contract API"))
                            @if let Some(href)=row["verification"]["href"].as_str() {" · "(link(href,"Verification API"))}
                        }
                    }
                }}
            }}
            p {"A blank provider version preserves the broker's null semantics; selected provider candidates are listed separately below."}
            h2 {"Candidate provenance"}
            @for candidate in array(&doc["resolution"]["candidates"]) {
                section data-candidate-version=(string(&candidate["version"])) {
                    h3 {(string(&candidate["pacticipant"])) " version " (string(&candidate["version"]))}
                    @for source in array(&candidate["sources"]) {
                        p {(string(&source["kind"])) " · " (link(string(&source["href"]),"Recorded membership")) " · " (string(&source["recordedAt"]))}
                    }
                }
            }
            details {summary{"Referenced evidence and resolution"}
                (evidence(&doc["rows"],"/rows"))
                (evidence(&doc["resolution"],"/resolution"))
            }
        }
    }
}
fn references(refs: &Value) -> Markup {
    html! {p {"Evidence: " @for pointer in array(refs) { a href=(format!("#{}",encode(string(pointer)))) {code {(string(pointer))}} " " } } }
}
fn evidence(value: &Value, path: &str) -> Markup {
    html! { div id=(path) {
        @match value {
            Value::Object(fields)=>{dl{@for(key,value) in fields {dt{(key)}dd{
                @if (key=="href" || key.ends_with("Href")) && value.is_string() {div id=(format!("{path}/{}",key.replace('~',"~0").replace('/',"~1"))) {(link(string(value),string(value)))}}
                @else {(evidence(value,&format!("{path}/{}",key.replace('~',"~0").replace('/',"~1"))))}
            }}}}
            Value::Array(values)=>{ol{@for(i,value) in values.iter().enumerate(){li{(evidence(value,&format!("{path}/{i}")))}}}}
            Value::String(value)=>{(value)}
            Value::Null=>{"Not recorded (null)"}
            other=>{(other.to_string())}
        }
    } }
}
