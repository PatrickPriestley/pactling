//! Basic auth as the Ruby broker does it. Verified against 2.121.2: an unauthenticated or
//! wrongly authenticated request gets 401, `text/plain`, an empty body and
//! `www-authenticate: Basic realm="Restricted area"`. The read-only user gets the same 401
//! on writes. OPTIONS counts as a read. When no credentials are configured everything is
//! open. The bearer token is a pactling extension.
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;

use crate::config::Config;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub read_write: Option<(String, String)>,
    pub read_only: Option<(String, String)>,
    pub allow_public_read: bool,
    pub bearer_token: Option<String>,
}

fn present(s: &Option<String>) -> Option<String> {
    s.as_deref().filter(|v| !v.is_empty()).map(str::to_owned)
}

fn pair(user: &Option<String>, pass: &Option<String>) -> Option<(String, String)> {
    Some((present(user)?, present(pass)?))
}

impl Credentials {
    pub fn from_config(c: &Config) -> Self {
        Self {
            read_write: pair(&c.basic_auth_username, &c.basic_auth_password),
            read_only: pair(
                &c.basic_auth_read_only_username,
                &c.basic_auth_read_only_password,
            ),
            allow_public_read: c.allow_public_read,
            bearer_token: present(&c.bearer_token),
        }
    }

    pub fn enabled(&self) -> bool {
        self.read_write.is_some() || self.read_only.is_some() || self.bearer_token.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

pub fn access_for(method: &Method) -> Access {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        Access::Read
    } else {
        Access::Write
    }
}

/// What the request's `Authorization` header grants, if anything.
pub fn granted(creds: &Credentials, authorization: Option<&str>) -> Option<Access> {
    let value = authorization?;
    if let Some(token) = value.strip_prefix("Bearer ") {
        return (creds.bearer_token.as_deref() == Some(token)).then_some(Access::Write);
    }
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    let matches =
        |p: &Option<(String, String)>| p.as_ref().is_some_and(|(u, pw)| u == user && pw == pass);
    if matches(&creds.read_write) {
        Some(Access::Write)
    } else if matches(&creds.read_only) {
        Some(Access::Read)
    } else {
        None
    }
}

pub fn authorised(creds: &Credentials, method: &Method, authorization: Option<&str>) -> bool {
    if !creds.enabled() {
        return true;
    }
    let needed = access_for(method);
    if needed == Access::Read && creds.allow_public_read {
        return true;
    }
    match granted(creds, authorization) {
        Some(Access::Write) => true,
        Some(Access::Read) => needed == Access::Read,
        None => false,
    }
}

fn unauthorised() -> Response {
    let mut res = (StatusCode::UNAUTHORIZED, Body::empty()).into_response();
    res.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"Restricted area\""),
    );
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    res
}

pub async fn require(State(creds): State<Credentials>, req: Request, next: Next) -> Response {
    let authorization = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if authorised(&creds, req.method(), authorization.as_deref()) {
        next.run(req).await
    } else {
        unauthorised()
    }
}
