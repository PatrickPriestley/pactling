//! The HAL index at `/`. Every relation, title and quirk is copied from the Ruby broker
//! 2.121.2 fixture at `tests/fixtures/ruby-broker/2.121.2/GET_index.json`; the fidelity
//! test compares the two documents, so do not "tidy" the inconsistencies here.
use super::{
    hal::{Hal, Links},
    method,
};
use axum::{
    http::Method,
    response::{IntoResponse, Response},
};
use serde_json::json;

const ALLOWED: &str = "GET, OPTIONS";

/// The index is GET-only, as on the Ruby broker, which answers HEAD with 405.
pub async fn index(m: Method, links: Links) -> Response {
    if m == Method::GET {
        document(&links).into_response()
    } else if m == Method::OPTIONS {
        method::options(ALLOWED)
    } else {
        method::method_not_allowed(ALLOWED)
    }
}

fn document(l: &Links) -> Hal {
    Hal(json!({
        "_links": {
            "self": { "href": l.href(""), "title": "Index", "templated": false },
            "pb:publish-pact": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/version/{consumerApplicationVersion}"), "title": "Publish a pact", "templated": true },
            "pb:publish-contracts": { "href": l.href("/contracts/publish"), "title": "Publish contracts", "templated": false },
            "pb:latest-pact-versions": { "href": l.href("/pacts/latest"), "title": "Latest pact versions", "templated": false },
            "pb:tagged-pact-versions": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/tag/{tag}"), "title": "All versions of a pact for a given consumer, provider and consumer version tag", "templated": false },
            "pb:branch-pact-versions": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/branch/{branch}"), "title": "All versions of pacts for a provider, for a named consumers branch", "templated": true },
            "pb:latest-branch-pact-versions": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/branch/{branch}/latest/versions"), "title": "Latest version of pact for a provider, for a named consumers branch", "templated": true },
            "pb:main-branch-pact-versions": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/branch/versions"), "title": "All versions of pacts for a provider, for a named consumers main branch", "templated": true },
            "pb:latest-main-branch-pact-versions": { "href": l.href("/pacts/provider/{provider}/consumer/{consumer}/branch/latest/versions"), "title": "Latest version of pact for a provider, for a named consumers main branch", "templated": true },
            "pb:provider-pacts-with-main-branch": { "href": l.href("/pacts/provider/{provider}/branch"), "title": "All pacts for a provider, for any consumers main branch", "templated": true },
            "pb:provider-pacts-with-branch": { "href": l.href("/pacts/provider/{provider}/branch/{branch}"), "title": "All pacts for a provider, for any consumers named branch", "templated": true },
            "pb:latest-provider-pacts-with-main-branch": { "href": l.href("/pacts/provider/{provider}/branch/latest"), "title": "Latest pact for a provider, for any consumers main branch", "templated": true },
            "pb:latest-provider-pacts-with-branch": { "href": l.href("/pacts/provider/{provider}/branch/{branch}/latest"), "title": "Latest pacts for a provider, for any consumers named branch", "templated": true },
            "pb:pacticipants": { "href": l.href("/pacticipants"), "title": "Pacticipants", "templated": false },
            "pb:pacticipant": { "href": l.href("/pacticipants/{pacticipant}"), "title": "Fetch pacticipant by name", "templated": true },
            "pb:latest-provider-pacts": { "href": l.href("/pacts/provider/{provider}/latest"), "title": "Latest pacts by provider", "templated": true },
            "pb:latest-provider-pacts-with-tag": { "href": l.href("/pacts/provider/{provider}/latest/{tag}"), "title": "Latest pacts for provider with the specified tag", "templated": true },
            "pb:provider-pacts-with-tag": { "href": l.href("/pacts/provider/{provider}/tag/{tag}"), "title": "All pact versions for the provider with the specified consumer version tag", "templated": true },
            "pb:provider-pacts": { "href": l.href("/pacts/provider/{provider}"), "title": "All pact versions for the specified provider", "templated": true },
            "pb:latest-version": { "href": l.href("/pacticipants/{pacticipant}/latest-version"), "title": "Latest pacticipant version", "templated": true },
            "pb:latest-tagged-version": { "href": l.href("/pacticipants/{pacticipant}/latest-version/{tag}"), "title": "Latest pacticipant version with the specified tag", "templated": true },
            "pb:webhooks": { "href": l.href("/webhooks"), "title": "Webhooks", "templated": false },
            "pb:webhook": { "href": l.href("/webhooks/{uuid}"), "title": "Webhook", "templated": true },
            "pb:integrations": { "href": l.href("/integrations"), "title": "Integrations", "templated": false },
            "pb:pacticipant-version-tag": { "href": l.href("/pacticipants/{pacticipant}/versions/{version}/tags/{tag}"), "title": "Get, create or delete a tag for a pacticipant version", "templated": true },
            "pb:pacticipant-branch": { "href": l.href("/pacticipants/{pacticipant}/branches/{branch}"), "title": "Get or delete a pacticipant branch", "templated": true },
            "pb:pacticipant-branch-version": { "href": l.href("/pacticipants/{pacticipant}/branches/{branch}/versions/{version}"), "title": "Get or add/create a pacticipant version for a branch", "templated": true },
            "pb:pacticipant-version": { "href": l.href("/pacticipants/{pacticipant}/versions/{version}"), "title": "Get, create or delete a pacticipant version", "templated": true },
            "pb:metrics": { "href": l.href("/metrics"), "title": "Get Pact Broker metrics" },
            "pb:can-i-deploy-pacticipant-version-to-tag": { "href": l.href("/can-i-deploy?pacticipant={pacticipant}&version={version}&to={tag}"), "title": "Determine if an application version can be safely deployed to an environment identified by the given tag", "templated": true },
            "pb:can-i-deploy-pacticipant-version-to-environment": { "href": l.href("/can-i-deploy?pacticipant={pacticipant}&version={version}&environment={environment}"), "title": "Determine if an application version can be safely deployed to an environment", "templated": true },
            "pb:provider-pacts-for-verification": { "href": l.href("/pacts/provider/{provider}/for-verification"), "title": "Pact versions to be verified for the specified provider", "templated": true },
            "beta:provider-pacts-for-verification": { "name": "beta", "href": l.href("/pacts/provider/{provider}/for-verification"), "title": "DEPRECATED - please use pb:provider-pacts-for-verification", "templated": true },
            "curies": [{ "name": "pb", "href": l.href("/doc/{rel}?context=index"), "templated": true }, { "name": "beta", "href": l.href("/doc/{rel}?context=index"), "templated": true }],
            "pb:environments": { "title": "Environments", "href": l.href("/environments"), "templated": false },
            "pb:environment": { "title": "Environment", "href": l.href("/environments/{uuid}"), "templated": true }
        }
    }))
}
