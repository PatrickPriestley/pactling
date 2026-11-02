use axum::{
    extract::State,
    http::{Method, StatusCode},
    response::Response,
};
use maud::html;

use super::{ALLOWED, layout};
use crate::{app::AppState, db::Db, http::hal::encode};

pub async fn index(method: Method, State(state): State<AppState>) -> Response {
    match method {
        Method::GET => match load(&state.db).await {
            Ok(pacts) => layout::response(
                StatusCode::OK,
                layout::page(
                    "Integrations",
                    html! {
                        @if pacts.is_empty() {
                            p { "No integrations have been published." }
                        } @else {
                            div class="table-scroll" {
                                table {
                                    thead { tr {
                                        th scope="col" { "Consumer" }
                                        th scope="col" { "Provider" }
                                        th scope="col" { "Latest consumer version" }
                                        th scope="col" { "Inspect" }
                                    } }
                                    tbody {
                                        @for pact in &pacts {
                                            @let provider = encode(&pact.provider_name);
                                            @let consumer = encode(&pact.consumer_name);
                                            @let version = encode(&pact.consumer_version_number);
                                            tr {
                                                td { (&pact.consumer_name) }
                                                td { (&pact.provider_name) }
                                                td { (&pact.consumer_version_number) }
                                                td {
                                                    a href=(format!("/ui/pacts/provider/{provider}/consumer/{consumer}/version/{version}")) { "Pact" }
                                                    " · "
                                                    a href=(format!("/ui/matrix/provider/{provider}/consumer/{consumer}")) { "Matrix" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                ),
            ),
            Err(error) => {
                tracing::error!(%error, "loading UI integrations");
                layout::response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    layout::page(
                        "Unable to load integrations",
                        html! {
                            p { "Unable to load integrations." }
                        },
                    ),
                )
            }
        },
        Method::OPTIONS => crate::http::method::options(ALLOWED),
        _ => crate::http::method::method_not_allowed(ALLOWED),
    }
}

async fn load(db: &Db) -> sqlx::Result<Vec<crate::db::pacts::PactPublication>> {
    let snapshot = db.begin_read().await?;
    let pacts = snapshot.latest_pacts().await?;
    snapshot.commit().await?;
    Ok(pacts)
}
