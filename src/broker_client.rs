//! Shared broker request construction; the MCP client has no write interface.
use reqwest::{ClientBuilder, RequestBuilder, Url};

/// Keep explain's existing redirect, timeout and decoding behavior, even with MCP enabled.
pub(crate) fn client_builder() -> ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .no_gzip()
        .no_deflate()
}

pub(crate) fn base_url(base: &str) -> Result<Url, String> {
    let url = Url::parse(base).map_err(|_| "invalid broker base URL")?;
    // HTTP(S) URLs necessarily have a host (enforced by url's parser).
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "broker base URL must be HTTP(S), without credentials, query or fragment".into(),
        );
    }
    Ok(url)
}

pub(crate) fn append_path(base: &Url, segments: &[&str]) -> Result<Url, ClientError> {
    if segments
        .iter()
        .any(|s| matches!(*s, "." | "..") || s.chars().any(char::is_control))
    {
        return Err(ClientError::InvalidPath);
    }
    let mut url = base.clone();
    url.set_path(base.path().trim_end_matches('/'));
    url.path_segments_mut()
        .map_err(|_| ClientError::InvalidConfig)?
        .pop_if_empty()
        .extend(segments.iter().copied());
    Ok(url)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignore {
    pub name: String,
    pub version: Option<String>,
}
#[derive(Clone, Default)]
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    InvalidConfig,
    InvalidPath,
    Timeout,
    Unavailable,
    ResponseTooLarge,
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Broker client error: {self:?}")
    }
}
impl std::error::Error for ClientError {}

#[derive(Clone)]
pub enum Selector {
    Version(String),
    Latest(Option<String>),
    Branch(String),
    MainBranch,
}
#[derive(Clone)]
pub enum Target {
    Environment(String),
    Tag(String),
}
#[derive(Clone)]
pub struct ExplanationQuery {
    pub pacticipant: String,
    pub selector: Selector,
    pub target: Target,
    pub ignores: Vec<Ignore>,
}
impl ExplanationQuery {
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut pairs = vec![];
        let mut add = |key: &str, value: &str| pairs.push((key.to_owned(), value.to_owned()));
        add("q[][pacticipant]", &self.pacticipant);
        match &self.selector {
            Selector::Version(version) => add("q[][version]", version),
            Selector::Latest(tag) => {
                add("q[][latest]", "true");
                if let Some(tag) = tag {
                    add("q[][tag]", tag);
                }
            }
            Selector::Branch(branch) => {
                add("q[][latest]", "true");
                add("q[][branch]", branch);
            }
            Selector::MainBranch => {
                add("q[][latest]", "true");
                add("q[][mainBranch]", "true");
            }
        }
        add("latestby", "cvp");
        match &self.target {
            Target::Environment(environment) => add("environment", environment),
            Target::Tag(tag) => {
                add("latest", "true");
                add("tag", tag);
            }
        }
        for ignore in &self.ignores {
            add("ignore[][pacticipant]", &ignore.name);
            if let Some(version) = &ignore.version {
                add("ignore[][version]", version);
            }
        }
        pairs
    }
}
#[cfg(feature = "mcp")]
#[derive(Clone)]
pub enum ReadRoute {
    Integrations,
    Pacticipants,
    Pact {
        consumer: String,
        provider: String,
        version: String,
    },
    Matrix {
        participant_a: String,
        participant_b: String,
        latestby: Option<Grouping>,
    },
    Explanation(ExplanationQuery),
}
#[cfg(feature = "mcp")]
#[derive(Clone, Copy)]
pub enum Grouping {
    Cvpv,
    Cvp,
    Cp,
}
#[cfg(feature = "mcp")]
#[derive(Clone)]
/// Fixed broker reads only; neither the transport nor write methods are exposed.
///
/// ```compile_fail
/// use pactling::broker_client::ReadClient;
/// fn cannot_access_transport(client: ReadClient) { let _ = client.client; }
/// ```
/// ```compile_fail
/// use pactling::broker_client::ReadClient;
/// fn cannot_write(client: ReadClient) { let _ = client.post("http://example.com"); }
/// ```
pub struct ReadClient {
    client: reqwest::Client,
    base: Url,
    credentials: Credentials,
}
#[cfg(feature = "mcp")]
#[derive(Debug)]
pub struct ReadResponse {
    pub status: reqwest::StatusCode,
    pub bytes: Vec<u8>,
    pub source_url: String,
}
#[cfg(feature = "mcp")]
impl ReadClient {
    pub fn new(base: &str, credentials: Credentials) -> Result<Self, ClientError> {
        credentials.validate()?;
        let base = base_url(base).map_err(|_| ClientError::InvalidConfig)?;
        let client = mcp_client_builder()
            .build()
            .map_err(|_| ClientError::Unavailable)?;
        Ok(Self {
            client,
            base,
            credentials,
        })
    }
    pub fn source_url(&self, route: &ReadRoute) -> Result<Url, ClientError> {
        let mut url = match route {
            ReadRoute::Integrations => append_path(&self.base, &["integrations"]),
            ReadRoute::Pacticipants => append_path(&self.base, &["pacticipants"]),
            ReadRoute::Pact {
                consumer,
                provider,
                version,
            } => append_path(
                &self.base,
                &[
                    "pacts", "provider", provider, "consumer", consumer, "version", version,
                ],
            ),
            ReadRoute::Matrix { .. } => append_path(&self.base, &["matrix"]),
            ReadRoute::Explanation(_) => append_path(
                &self.base,
                &["pactling", "v1", "can-i-deploy", "explanation"],
            ),
        }?;
        match route {
            ReadRoute::Matrix {
                participant_a,
                participant_b,
                latestby,
            } => {
                let mut query = url.query_pairs_mut();
                query
                    .append_pair("q[][pacticipant]", participant_a)
                    .append_pair("q[][pacticipant]", participant_b);
                if let Some(grouping) = latestby {
                    query.append_pair(
                        "latestby",
                        match grouping {
                            Grouping::Cvpv => "cvpv",
                            Grouping::Cvp => "cvp",
                            Grouping::Cp => "cp",
                        },
                    );
                }
            }
            ReadRoute::Explanation(query) => {
                url.query_pairs_mut().extend_pairs(query.pairs());
            }
            _ => {}
        }
        Ok(url)
    }
    pub async fn get(&self, route: &ReadRoute) -> Result<ReadResponse, ClientError> {
        let url = self.source_url(route)?;
        let source_url = url.to_string();
        let accept = match route {
            ReadRoute::Pact { .. } | ReadRoute::Explanation(_) => "application/json",
            _ => "application/hal+json",
        };
        let request = self
            .credentials
            .apply(self.client.get(url).header(reqwest::header::ACCEPT, accept));
        let mut response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            // Count decoded bytes, without trusting Content-Length or buffering the whole body.
            if chunk.len() > 4 * 1024 * 1024 - bytes.len() {
                return Err(ClientError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(ReadResponse {
            status,
            bytes,
            source_url,
        })
    }
}

impl Credentials {
    #[cfg(feature = "mcp")]
    fn validate(&self) -> Result<(), ClientError> {
        if self.username.as_deref() == Some("") || self.token.as_deref() == Some("") {
            return Err(ClientError::InvalidConfig);
        }
        match (&self.username, &self.password, &self.token) {
            (None, None, _) | (Some(_), Some(_), None) => Ok(()),
            _ => Err(ClientError::InvalidConfig),
        }
    }
    pub(crate) fn apply(&self, request: RequestBuilder) -> RequestBuilder {
        if let Some(username) = &self.username {
            request.basic_auth(username, self.password.as_ref())
        } else if let Some(token) = &self.token {
            request.bearer_auth(token)
        } else {
            request
        }
    }
}
#[cfg(feature = "mcp")]
fn transport_error(error: reqwest::Error) -> ClientError {
    if error.is_timeout() {
        ClientError::Timeout
    } else {
        ClientError::Unavailable
    }
}

#[cfg(feature = "mcp")]
fn mcp_client_builder() -> ClientBuilder {
    client_builder().gzip(true).deflate(true)
}

#[cfg(all(test, feature = "mcp"))]
#[path = "broker_client_tests.rs"]
mod tests;
