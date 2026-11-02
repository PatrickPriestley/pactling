//! Shared broker request construction; the MCP client has no write interface.
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
    pub ignores: Vec<crate::cli::Ignore>,
}
impl ExplanationQuery {
    pub fn pairs(&self) -> Vec<(String, String)> {
        vec![]
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
pub struct ReadClient;
#[cfg(feature = "mcp")]
#[derive(Debug)]
pub struct ReadResponse {
    pub status: reqwest::StatusCode,
    pub bytes: Vec<u8>,
    pub source_url: String,
}
#[cfg(feature = "mcp")]
impl ReadClient {
    pub fn new(_base: &str, _credentials: Credentials) -> Result<Self, ClientError> {
        Ok(Self)
    }
    pub fn source_url(&self, _route: &ReadRoute) -> Result<reqwest::Url, ClientError> {
        Err(ClientError::Unavailable)
    }
    pub async fn get(&self, _route: &ReadRoute) -> Result<ReadResponse, ClientError> {
        Err(ClientError::Unavailable)
    }
}
