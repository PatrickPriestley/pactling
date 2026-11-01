//! Pair matrix: one service evaluation, rendered without changing its row semantics.
use super::{ALLOWED, layout, links::link, query};
use crate::{app::AppState, error::Error, http::hal::encode, service::matrix};
use axum::{
    extract::{Path, RawQuery, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use maud::{Markup, html};

pub async fn matrix(
    method: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    RawQuery(raw): RawQuery,
) -> Response {
    match method {
        Method::GET => {
            let query =
                query::matrix_query(raw.as_deref().unwrap_or_default(), &consumer, &provider);
            match query {
                Ok((q, canonical)) => match matrix::decision(&state.db, &q).await {
                    Ok(evaluation) => {
                        let rows = &evaluation.matrix().rows;
                        layout::response(
                            StatusCode::OK,
                            layout::page(
                                "Matrix",
                                html! {
                                    p { (&consumer) " → " (&provider) }
                                    (form(&consumer,&provider))
                                    p { "Query: " code { (&canonical) } }
                                    @for notice in &evaluation.matrix().notices { p { (&notice.text) } }
                                    @if rows.is_empty() { p { "No matrix rows match these selectors." } }
                                    div class="table-scroll" { table {
                                        thead { tr { th scope="col" { "Consumer" } th scope="col" { "Consumer version" } th scope="col" { "Provider" } th scope="col" { "Provider version" } th scope="col" { "Verification" } th scope="col" { "Disposition" } th scope="col" { "Evidence" } } }
                                        tbody { @for row in rows {
                                            @let cv=row.pact.consumer_version_number.as_str();
                                            @let pv=row.provider.version.as_ref().map(|v|v.number.as_str()).unwrap_or("");
                                            @let result=row.verification.as_ref().map(|v|if v.success {"Successful"}else{"Failed"}).unwrap_or("Unknown");
                                            tr data-consumer-version=(cv) data-provider-version=(pv) {
                                                td { (&row.consumer.pacticipant.name) } td { (cv) }
                                                td { (&row.provider.pacticipant.name) } td { (pv) }
                                                td { (result) } td { @if row.ignored { "Ignored" } @else { "Considered" } }
                                                td { (link(&format!("/ui/pacts/provider/{}/consumer/{}/version/{}",encode(&row.pact.provider_name),encode(&row.pact.consumer_name),encode(&row.pact.consumer_version_number)),"Pact")) }
                                            }
                                        } }
                                    } }
                                },
                            ),
                        )
                    }
                    Err(e) => error(e),
                },
                Err(e) => error(e),
            }
        }
        Method::OPTIONS => crate::http::method::options(ALLOWED),
        _ => crate::http::method::method_not_allowed(ALLOWED),
    }
}

fn form(consumer: &str, provider: &str) -> Markup {
    html! { form method="get" action=(format!("/ui/matrix/provider/{}/consumer/{}",encode(provider),encode(consumer))) {
        input type="hidden" name="ui_form" value="matrix";
        @for (side,label) in [("consumer","Consumer"),("provider","Provider")] {
            label for=(format!("{side}_mode")) { (label) " selector" }
            select id=(format!("{side}_mode")) name=(format!("{side}_mode")) {
                @for (value,label) in [("all","All versions"),("version","Exact version"),("latest","Latest version"),("tag","Latest with tag"),("branch","Branch head"),("main","Main branch head")] {
                    option value=(value) { (label) }
                }
            }
            label for=(format!("{side}_value")) { (label) " version, tag or branch (when selected)" }
            input id=(format!("{side}_value")) name=(format!("{side}_value"));
        }
        label for="latestby" { "Grouping" }
        select id="latestby" name="latestby" {
            option value="none" { "All results" }
            option value="cvpv" { "Consumer version / provider version" }
            option value="cvp" { "Consumer version / provider" }
            option value="cp" { "Consumer / provider" }
        }
        button type="submit" { "Show matrix" }
    } }
}

fn error(error: Error) -> Response {
    let detail = match &error {
        Error::BadRequest(message) => message.clone(),
        Error::Validation(details) => details.to_string(),
        _ => "Unable to load matrix.".into(),
    };
    let status = error.into_response().status();
    layout::response(
        status,
        layout::page("Unable to load matrix", html! {p{(detail)}}),
    )
}
