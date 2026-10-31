//! Pact lists: `/pacts/latest`, the provider lists under `/pacts/provider/{p}/...` and
//! the integration lists under `/pacts/provider/{p}/consumer/{c}/...`. Shapes and
//! ordering follow the Phase 2 capture (tests/retrieval.rs).
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::app::AppState;
use crate::db::pact_lists::Selection;
use crate::db::pacts::PactPublication;
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_href;
use crate::http::versions::{pacticipant_href, version_href};

const READ_ALLOWED: &str = "GET, OPTIONS";
const VERSIONS_ALLOWED: &str = "GET, DELETE, OPTIONS";

fn short_date(created_at: &str) -> String {
    match created_at.get(0..10) {
        Some(ymd) if ymd.len() == 10 => format!("{}/{}/{}", &ymd[8..10], &ymd[5..7], &ymd[0..4]),
        _ => created_at.to_owned(),
    }
}

fn pact_title(p: &PactPublication) -> String {
    format!(
        "Pact between {} ({}) and {}",
        p.consumer_name, p.consumer_version_number, p.provider_name
    )
}

fn href_of(l: &Links, p: &PactPublication) -> String {
    pact_href(
        l,
        &p.provider_name,
        &p.consumer_name,
        &p.consumer_version_number,
    )
}

/// `GET /pacts/latest`
pub async fn latest(m: Method, State(state): State<AppState>, l: Links) -> Result<Response> {
    let l = &l;
    match m {
        Method::GET => {
            let pacts = state.db.latest_pacts().await?;
            let entries: Vec<Value> = pacts
                .iter()
                .map(|p| {
                    let latest = l.href(&format!("/pacts/provider/{}/consumer/{}/latest", encode(&p.provider_name), encode(&p.consumer_name)));
                    json!({
                        "createdAt": p.created_at,
                        "_embedded": {
                            "consumer": {
                                "name": p.consumer_name,
                                "_embedded": { "version": version_stub_of(l, p) },
                                "_links": { "self": { "href": pacticipant_href(l, &p.consumer_name) } }
                            },
                            "provider": { "name": p.provider_name, "_links": { "self": { "href": pacticipant_href(l, &p.provider_name) } } }
                        },
                        "_links": { "self": [{ "href": latest }, { "href": href_of(l, p) }] }
                    })
                })
                .collect();
            let links: Vec<Value> = pacts
                .iter()
                .map(|p| json!({ "href": l.href(&format!("/pacts/provider/{}/consumer/{}/latest", encode(&p.provider_name), encode(&p.consumer_name))), "title": format!("Latest pact between {} and {}", p.consumer_name, p.provider_name) }))
                .collect();
            let deprecated: Vec<Value> = links
                .iter()
                .map(|v| {
                    let mut d = v.clone();
                    d["name"] = json!("DEPRECATED - please use the pb:pacts relation");
                    d
                })
                .collect();
            Ok(Hal(json!({
                "pacts": entries,
                "_links": {
                    "self": { "href": l.href("/pacts/latest") },
                    "pb:pacts": links,
                    "pacts": deprecated,
                    "curies": [{ "name": "pb", "href": l.href("/doc/{rel}?context=pacts"), "templated": true }]
                }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

fn version_stub_of(l: &Links, p: &PactPublication) -> Value {
    json!({
        "number": p.consumer_version_number,
        "_links": { "self": { "title": "Version", "name": p.consumer_version_number, "href": version_href(l, &p.consumer_name, &p.consumer_version_number) } }
    })
}

// --- provider lists -------------------------------------------------------------------

/// How a provider list route selects consumer versions and titles itself.
#[derive(Clone, Copy, Debug)]
pub enum ProviderList {
    All,
    Latest,
    LatestWithTag,
    WithTag,
    MainBranch,
    LatestMainBranch,
    Branch,
    LatestBranch,
}

async fn provider_list(
    state: &AppState,
    l: &Links,
    provider: &str,
    kind: ProviderList,
    arg: Option<&str>,
) -> Result<Response> {
    if state.db.find_pacticipant(provider).await?.is_none() {
        return Err(Error::NotFound(format!(
            "No provider with name '{provider}' found"
        )));
    }
    let (selection, latest, suffix, title) = match kind {
        ProviderList::All => (Selection::All, false, String::new(), "All"),
        ProviderList::Latest => (Selection::All, true, "/latest".into(), "Latest"),
        ProviderList::LatestWithTag => (
            Selection::Tag(arg.unwrap_or_default().into()),
            true,
            format!("/latest/{}", encode(arg.unwrap_or_default())),
            "Latest",
        ),
        ProviderList::WithTag => (
            Selection::Tag(arg.unwrap_or_default().into()),
            false,
            format!("/tag/{}", encode(arg.unwrap_or_default())),
            "All",
        ),
        ProviderList::MainBranch => (Selection::Branch(None), false, "/branch".into(), "All"),
        ProviderList::LatestMainBranch => (
            Selection::Branch(None),
            true,
            "/branch/latest".into(),
            "Latest",
        ),
        ProviderList::Branch => (
            Selection::Branch(arg.map(str::to_owned)),
            false,
            format!("/branch/{}", encode(arg.unwrap_or_default())),
            "All",
        ),
        ProviderList::LatestBranch => (
            Selection::Branch(arg.map(str::to_owned)),
            true,
            format!("/branch/{}/latest", encode(arg.unwrap_or_default())),
            "Latest",
        ),
    };
    let qualifier = match &selection {
        Selection::Tag(t) => format!(" with consumer version tag '{t}'"),
        Selection::Branch(Some(b)) => format!(" with consumer version branch '{b}'"),
        _ => String::new(),
    };
    let pacts = state
        .db
        .pacts_for_provider(provider, &selection, latest)
        .await?;
    let links: Vec<Value> = pacts
        .iter()
        .map(|p| json!({ "href": href_of(l, p), "title": pact_title(p), "name": p.consumer_name }))
        .collect();
    let deprecated: Vec<Value> = pacts
        .iter()
        .map(|p| json!({ "href": href_of(l, p), "title": format!("DEPRECATED - please use the pb:pacts relation. {}", pact_title(p)), "name": p.consumer_name }))
        .collect();
    Ok(Hal(json!({
        "_links": {
            "self": { "href": l.href(&format!("/pacts/provider/{}{suffix}", encode(provider))), "title": format!("{title} pact versions for the provider {provider}{qualifier}") },
            "pb:provider": { "href": pacticipant_href(l, provider), "name": provider },
            "pb:pacts": links,
            "provider": { "href": pacticipant_href(l, provider), "title": provider, "name": "DEPRECATED - please use the pb:provider relation" },
            "pacts": deprecated
        }
    }))
    .into_response())
}

macro_rules! provider_route {
    ($name:ident, $kind:expr, one) => {
        pub async fn $name(
            m: Method,
            State(state): State<AppState>,
            Path(provider): Path<String>,
            l: Links,
        ) -> Result<Response> {
            match m {
                Method::GET => provider_list(&state, &l, &provider, $kind, None).await,
                Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
                _ => Ok(method::method_not_allowed(READ_ALLOWED)),
            }
        }
    };
    ($name:ident, $kind:expr, two) => {
        pub async fn $name(
            m: Method,
            State(state): State<AppState>,
            Path((provider, arg)): Path<(String, String)>,
            l: Links,
        ) -> Result<Response> {
            match m {
                Method::GET => provider_list(&state, &l, &provider, $kind, Some(&arg)).await,
                Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
                _ => Ok(method::method_not_allowed(READ_ALLOWED)),
            }
        }
    };
}

provider_route!(provider_all, ProviderList::All, one);
provider_route!(provider_latest, ProviderList::Latest, one);
provider_route!(provider_latest_with_tag, ProviderList::LatestWithTag, two);
provider_route!(provider_with_tag, ProviderList::WithTag, two);
provider_route!(provider_main_branch, ProviderList::MainBranch, one);
provider_route!(
    provider_latest_main_branch,
    ProviderList::LatestMainBranch,
    one
);
provider_route!(provider_branch, ProviderList::Branch, two);
provider_route!(provider_latest_branch, ProviderList::LatestBranch, two);

// --- integration lists ----------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum IntegrationList {
    Versions,
    WithTag,
    Branch,
    LatestBranch,
    MainBranch,
    LatestMainBranch,
}

async fn integration_list(
    state: &AppState,
    l: &Links,
    provider: &str,
    consumer: &str,
    kind: IntegrationList,
    arg: Option<&str>,
) -> Result<Response> {
    if state.db.find_pacticipant(provider).await?.is_none()
        || state.db.find_pacticipant(consumer).await?.is_none()
    {
        return Err(Error::NoRoute);
    }
    let (selection, latest, suffix) = match kind {
        IntegrationList::Versions => (Selection::All, false, "/versions".to_owned()),
        IntegrationList::WithTag => (
            Selection::Tag(arg.unwrap_or_default().into()),
            false,
            format!("/tag/{}", encode(arg.unwrap_or_default())),
        ),
        IntegrationList::Branch => (
            Selection::Branch(arg.map(str::to_owned)),
            false,
            format!("/branch/{}", encode(arg.unwrap_or_default())),
        ),
        IntegrationList::LatestBranch => (
            Selection::Branch(arg.map(str::to_owned)),
            true,
            format!(
                "/branch/{}/latest/versions",
                encode(arg.unwrap_or_default())
            ),
        ),
        IntegrationList::MainBranch => (
            Selection::Branch(None),
            false,
            "/branch/versions".to_owned(),
        ),
        IntegrationList::LatestMainBranch => (
            Selection::Branch(None),
            true,
            "/branch/latest/versions".to_owned(),
        ),
    };
    let pacts = state
        .db
        .pacts_for_integration(provider, consumer, &selection, latest)
        .await?;
    let entries: Vec<Value> = pacts
        .iter()
        .map(|p| {
            json!({
                "createdAt": p.created_at,
                "_embedded": { "consumerVersion": version_stub_of(l, p) },
                "_links": { "self": { "href": href_of(l, p), "title": pact_title(p) } }
            })
        })
        .collect();
    let base = l.href(&format!(
        "/pacts/provider/{}/consumer/{}",
        encode(provider),
        encode(consumer)
    ));
    // The tag list prefixes its relations with `pb:` and titles entries "Pact version";
    // the others do not (verified).
    let tagged = matches!(kind, IntegrationList::WithTag);
    let (rel_consumer, rel_provider, rel_versions, entry_title) = if tagged {
        (
            "pb:consumer",
            "pb:provider",
            "pb:pact-versions",
            "Pact version",
        )
    } else {
        ("consumer", "provider", "pact-versions", "Pact")
    };
    let versions: Vec<Value> = pacts
        .iter()
        .map(|p| json!({ "href": href_of(l, p), "title": entry_title, "name": format!("Version {} - {}", p.consumer_version_number, short_date(&p.created_at)) }))
        .collect();
    let self_title = match &selection {
        Selection::Tag(t) => {
            format!("All versions of the pact between {consumer} and {provider} with tag {t}")
        }
        _ => format!("All versions of the pact between {consumer} and {provider}"),
    };
    Ok(Hal(json!({
        "_embedded": { "pacts": entries },
        "_links": {
            "self": { "href": format!("{base}{suffix}"), "title": self_title },
            rel_consumer: { "href": pacticipant_href(l, consumer), "title": consumer, "name": consumer },
            rel_provider: { "href": pacticipant_href(l, provider), "title": provider, "name": provider },
            rel_versions: versions
        }
    }))
    .into_response())
}

/// `GET | DELETE /pacts/provider/{p}/consumer/{c}/versions`
pub async fn integration_versions(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            integration_list(
                &state,
                &l,
                &provider,
                &consumer,
                IntegrationList::Versions,
                None,
            )
            .await
        }
        Method::DELETE => {
            // Verified: 204 even when nothing is left to delete; unknown names are the
            // HAL 404 naming the role.
            if state.db.find_pacticipant(&provider).await?.is_none() {
                return Err(Error::NotFound(format!(
                    "No provider with name '{provider}' found"
                )));
            }
            if state.db.find_pacticipant(&consumer).await?.is_none() {
                return Err(Error::NotFound(format!(
                    "No consumer with name '{consumer}' found"
                )));
            }
            state
                .db
                .delete_integration_pacts(&provider, &consumer)
                .await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(VERSIONS_ALLOWED)),
        _ => Ok(method::method_not_allowed(VERSIONS_ALLOWED)),
    }
}

macro_rules! integration_route {
    ($name:ident, $kind:expr, two) => {
        pub async fn $name(
            m: Method,
            State(state): State<AppState>,
            Path((provider, consumer)): Path<(String, String)>,
            l: Links,
        ) -> Result<Response> {
            match m {
                Method::GET => {
                    integration_list(&state, &l, &provider, &consumer, $kind, None).await
                }
                Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
                _ => Ok(method::method_not_allowed(READ_ALLOWED)),
            }
        }
    };
    ($name:ident, $kind:expr, three) => {
        pub async fn $name(
            m: Method,
            State(state): State<AppState>,
            Path((provider, consumer, arg)): Path<(String, String, String)>,
            l: Links,
        ) -> Result<Response> {
            match m {
                Method::GET => {
                    integration_list(&state, &l, &provider, &consumer, $kind, Some(&arg)).await
                }
                Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
                _ => Ok(method::method_not_allowed(READ_ALLOWED)),
            }
        }
    };
}

integration_route!(integration_with_tag, IntegrationList::WithTag, three);
integration_route!(integration_branch, IntegrationList::Branch, three);
integration_route!(
    integration_latest_branch,
    IntegrationList::LatestBranch,
    three
);
integration_route!(integration_main_branch, IntegrationList::MainBranch, two);
integration_route!(
    integration_latest_main_branch,
    IntegrationList::LatestMainBranch,
    two
);

/// `GET /integrations`
pub async fn integrations(m: Method, State(state): State<AppState>, l: Links) -> Result<Response> {
    match m {
        Method::GET => {
            let entries: Vec<Value> = state
                .db
                .integrations()
                .await?
                .iter()
                .map(|(c, p)| {
                    json!({
                        "consumer": { "name": c },
                        "provider": { "name": p },
                        "_links": {
                            "pb:dashboard": { "title": "BETA: Pacts to show on the dashboard", "href": l.href(&format!("/dashboard/provider/{}/consumer/{}", encode(p), encode(c))) },
                            // The Ruby broker renders this one href relative (verified).
                            "pb:matrix": { "title": format!("Matrix of pacts/verification results for {c} and {p}"), "href": format!("/matrix/provider/{}/consumer/{}", encode(p), encode(c)) },
                            "pb:group": { "href": pacticipant_href(&l, c) },
                            "pb:delete_integration": { "title": format!("Delete the integration between {c} and {p}"), "href": l.href(&format!("/integrations/provider/{}/consumer/{}", encode(p), encode(c))) }
                        }
                    })
                })
                .collect();
            Ok(Hal(json!({
                "_embedded": { "integrations": entries },
                "_links": { "self": { "href": l.href("/integrations"), "title": "All integrations" } }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}
