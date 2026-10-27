//! Fetching, behind a trait so the browser can be tested without a network.
use std::collections::BTreeMap;

use ureq::config::AutoHeaderValue;
use ureq::{Agent, ResponseExt};

use crate::config::Behavior;
use crate::error::Error;

/// A fetched resource. Body stays raw bytes: charset decoding happens later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    /// Final URL after redirects.
    pub url: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl Fetched {
    pub fn text(url: &str, body: &str) -> Fetched {
        Fetched {
            status: 200,
            url: url.to_string(),
            content_type: Some("text/html; charset=utf-8".to_string()),
            bytes: body.as_bytes().to_vec(),
        }
    }

    pub fn is_html(&self) -> bool {
        self.kind() == Kind::Html
    }

    pub fn kind(&self) -> Kind {
        let ct = self.content_type.as_deref().unwrap_or("text/html");
        let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        match ct.as_str() {
            "text/html" | "application/xhtml+xml" => Kind::Html,
            "text/plain" => Kind::Text,
            other if other.starts_with("text/") => Kind::Text,
            "image/png" | "image/jpeg" | "image/gif" => Kind::Image,
            "application/pdf" => Kind::Pdf,
            "text/css" => Kind::Css,
            _ => Kind::Binary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Html,
    Text,
    Image,
    Pdf,
    Css,
    Binary,
}

pub fn is_supported_scheme(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Turn something typed by the user into a URL to fetch.
pub fn normalize_target(input: &str) -> Result<String, Error> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::BadUrl("empty url".into()));
    }
    if is_supported_scheme(input) {
        let parsed = url::Url::parse(input).map_err(|e| Error::BadUrl(e.to_string()))?;
        return Ok(parsed.into());
    }
    // No scheme: an address gets `https://` in front of it (plain `http://` for
    // localhost, where that is what is actually running), anything else is a
    // search phrase.
    let hostless = half_typed_scheme(input);
    let candidate = match hostless {
        Some(rest) => format!("https://{rest}"),
        None if looks_like_host(input) => {
            let scheme = if is_local_authority(input) { "http" } else { "https" };
            format!("{scheme}://{input}")
        }
        None => {
            // Not a host: search for it, and percent-encode whatever was typed.
            let search = url::Url::parse_with_params("https://duckduckgo.com/", &[("q", input)])
                .map_err(|e| Error::BadUrl(e.to_string()))?;
            return Ok(search.into());
        }
    };
    if let Ok(parsed) = url::Url::parse(&candidate) {
        return Ok(parsed.into());
    }
    let search = url::Url::parse_with_params("https://duckduckgo.com/", &[("q", input)])
        .map_err(|e| Error::BadUrl(e.to_string()))?;
    Ok(search.into())
}

/// `example.com:8080/x` and `localhost` name a machine; `rust browser` does not.
fn looks_like_host(input: &str) -> bool {
    if input.is_empty() || input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if input.eq_ignore_ascii_case("localhost") || input.starts_with("localhost:") {
        return true;
    }
    // `mailto:nowhere` names a scheme, `example.com:8080` names a port.
    if let Some((_, rest)) = input.split_once(':') {
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
    }
    input.contains('.') || input.contains(':') || input.contains('/')
}

/// A mistyped scheme such as `https:/example.com` still means a url. Returns
/// the part after the scheme, or `None` when there is no scheme at all.
fn half_typed_scheme(input: &str) -> Option<&str> {
    let lower = input.to_ascii_lowercase();
    for prefix in ["https:/", "https:", "http:/", "http:"] {
        if lower.starts_with(prefix) {
            let rest = input[prefix.len()..].trim_start_matches('/');
            if !rest.is_empty() {
                return Some(rest);
            }
        }
    }
    None
}

/// Development servers on localhost are served over plain http.
fn is_local_authority(input: &str) -> bool {
    let authority = input.split(['/', '?', '#']).next().unwrap_or(input);
    let host = authority.split_once(':').map_or(authority, |(h, _)| h);
    host.eq_ignore_ascii_case("localhost") || host.starts_with("127.") || matches!(host, "::1" | "0.0.0.0")
}

pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Fetched, Error>;
}

pub struct HttpFetcher {
    agent: Agent,
    max_bytes: u64,
}

impl HttpFetcher {
    pub fn new(behavior: &Behavior) -> HttpFetcher {
        let config = Agent::config_builder()
            .user_agent(AutoHeaderValue::Provided(std::sync::Arc::new(behavior.user_agent.clone())))
            .timeout_global(Some(std::time::Duration::from_secs(behavior.timeout_secs)))
            .max_redirects(u32::from(behavior.max_redirects))
            .save_redirect_history(true)
            .http_status_as_error(false)
            .build();
        HttpFetcher { agent: config.into(), max_bytes: behavior.max_response_bytes }
    }
}

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str) -> Result<Fetched, Error> {
        let response = self.agent.get(url).call()?;
        let status = response.status().as_u16();
        let final_url = response.get_uri().to_string();
        let content_type =
            response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);

        let bytes = response.into_body().into_with_config().limit(self.max_bytes).read_to_vec()?;
        Ok(Fetched { status, url: final_url, content_type, bytes })
    }
}

/// In-memory fetcher for tests and offline demos.
#[derive(Debug, Default, Clone)]
pub struct FakeFetcher {
    responses: BTreeMap<String, FakeReply>,
}

/// What a [`FakeFetcher`] should answer for a URL.
#[derive(Debug, Clone)]
pub enum FakeReply {
    Page(Fetched),
    Fail(crate::error::NetError, String),
}

impl FakeFetcher {
    pub fn new() -> FakeFetcher {
        FakeFetcher::default()
    }

    /// Register a response. `status` is the HTTP status the body is served with.
    pub fn with_html(mut self, url: &str, status: u16, body: &str) -> FakeFetcher {
        self.responses.insert(
            url.to_string(),
            FakeReply::Page(Fetched {
                status,
                url: url.to_string(),
                content_type: Some("text/html; charset=utf-8".to_string()),
                bytes: body.as_bytes().to_vec(),
            }),
        );
        self
    }

    pub fn with_error(mut self, url: &str, kind: crate::error::NetError, detail: &str) -> FakeFetcher {
        self.responses.insert(url.to_string(), FakeReply::Fail(kind, detail.to_string()));
        self
    }
}

impl Fetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Result<Fetched, Error> {
        match self.responses.get(url).cloned() {
            Some(FakeReply::Page(page)) => Ok(page),
            Some(FakeReply::Fail(kind, detail)) => Err(Error::Net(kind, detail)),
            None => Err(Error::BadUrl(format!("no fake response for {url}"))),
        }
    }
}
