//! Exact publication view. No integration-wide verification badge is inferred.
use super::{ALLOWED, layout, links::link};
use crate::{app::AppState, http::hal::encode};
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::Response,
};
use maud::html;
use serde_json::Value;

pub async fn pact(
    method: Method,
    State(state): State<AppState>,
    Path((provider, consumer, version)): Path<(String, String, String)>,
) -> Response {
    match method {
        Method::GET => match state.db.find_pact(&provider, &consumer, &version).await {
            Ok(Some(publication)) => {
                // Stored content is JSON accepted by the publish service.
                let content: Value =
                    serde_json::from_str(&publication.content).expect("stored pact JSON");
                let p = encode(&provider);
                let c = encode(&consumer);
                let v = encode(&version);
                let interactions = content["interactions"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                layout::response(
                    StatusCode::OK,
                    layout::page(
                        "Pact",
                        html! {
                            dl {
                                dt { "Consumer" } dd { (&consumer) }
                                dt { "Provider" } dd { (&provider) }
                                dt { "Consumer version" } dd { (&version) }
                                dt { "Published" } dd { (&publication.created_at) }
                            }
                            p { (link(&format!("/ui/matrix/provider/{p}/consumer/{c}"), "Matrix")) " · "
                                (link(&format!("/pacts/provider/{p}/consumer/{c}/version/{v}"), "API document")) }
                            h2 { "Interactions" }
                            @if interactions.is_empty() { p { "No interactions are recorded." } }
                            @for interaction in interactions {
                                section {
                                    h3 { (json(&interaction["description"])) }
                                    h4 { "Provider state" } pre { (json(&interaction["providerState"])) }
                                    h4 { "Provider states" } pre { (json(&interaction["providerStates"])) }
                                    h4 { "Request" } pre { (json(&interaction["request"])) }
                                    h4 { "Response" } pre { (json(&interaction["response"])) }
                                }
                            }
                            details { summary { "Full contract" } pre { (json(&content)) } }
                        },
                    ),
                )
            }
            Ok(None) => layout::response(
                StatusCode::NOT_FOUND,
                layout::page(
                    "Pact not found",
                    html! {p { "The requested pact was not found." }},
                ),
            ),
            Err(error) => {
                tracing::error!(%error,"loading UI pact");
                layout::response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    layout::page("Unable to load pact", html! {p { "Unable to load pact." }}),
                )
            }
        },
        Method::OPTIONS => crate::http::method::options(ALLOWED),
        _ => crate::http::method::method_not_allowed(ALLOWED),
    }
}

fn json(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON value serializes")
}
