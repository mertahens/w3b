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
            "text/markdown" | "text/x-markdown" | "application/markdown" | "application/x-markdown" => {
                Kind::Markdown
            }
            // The hosts that hand out a markdown file as a download call it
            // `text/plain`, so the address is what tells a README from a log
            // file. The body is never sniffed for markup: half the net looks
            // like a document, and a `.txt` said to be plain stays plain.
            "text/plain" | "application/octet-stream" if ends_in_markdown(&self.url) => Kind::Markdown,
            "text/plain" => Kind::Text,
            "application/json" | "text/json" => Kind::Json,
            // `application/ld+json`, `application/geo+json`, `application/manifest+json`:
            // structured data with a base type written in front of the `+`.
            other if other.ends_with("+json") => Kind::Json,
            other if other.starts_with("text/") => Kind::Text,
            "image/png" | "image/jpeg" | "image/gif" => Kind::Image,
            "application/pdf" => Kind::Pdf,
            "text/css" => Kind::Css,
            // A server that calls JSON `application/octet-stream` is common enough
            // that the bytes get a look before w3b calls the body binary. The scan
            // gives up on the first character, so a real download costs nothing.
            "application/octet-stream" if json_body(&self.bytes) => Kind::Json,
            _ => Kind::Binary,
        }
    }
}

/// Where a body's text is written in the `text/*` styles w3b can put on screen.
/// Whether an address names a markdown file. The query and the fragment come off
/// first: a host that serves `README.md?raw=1`, or redirects to `README.md#L10`,
/// is still serving that file.
fn ends_in_markdown(url: &str) -> bool {
    let last = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)))
        .unwrap_or_default();
    [".md", ".markdown", ".mdown", ".mkd"].iter().any(|ext| last.to_ascii_lowercase().ends_with(ext))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Html,
    /// A markdown document, turned into HTML before it is parsed.
    Markdown,
    Text,
    /// Structured data: laid out by nesting and coloured by part.
    Json,
    Image,
    Pdf,
    Css,
    Binary,
}

/// Whether the bytes read as one JSON value.
fn json_body(bytes: &[u8]) -> bool {
    matches!(std::str::from_utf8(bytes), Ok(text) if crate::json::is_json(text))
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
    let candidate = match half_typed_scheme(input) {
        // A scheme typed with a faltering slash is still the scheme that was meant.
        Some((scheme, rest)) => format!("{scheme}://{rest}"),
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

/// A mistyped scheme such as `https:/example.com` still means a url. Returns the
/// scheme that was meant and the part after it, or `None` when there is no scheme.
fn half_typed_scheme(input: &str) -> Option<(&'static str, &str)> {
    let lower = input.to_ascii_lowercase();
    for (prefix, scheme) in [("https:/", "https"), ("https:", "https"), ("http:/", "http"), ("http:", "http")]
    {
        if lower.starts_with(prefix) {
            let rest = input[prefix.len()..].trim_start_matches('/');
            if !rest.is_empty() {
                return Some((scheme, rest));
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
    pub fn with_html(self, url: &str, status: u16, body: &str) -> FakeFetcher {
        self.with_type(url, status, "text/html; charset=utf-8", body)
    }

    /// Register a response served under a chosen content type, so a test can be
    /// handed JSON or css rather than only html.
    pub fn with_type(mut self, url: &str, status: u16, content_type: &str, body: &str) -> FakeFetcher {
        self.responses.insert(
            url.to_string(),
            FakeReply::Page(Fetched {
                status,
                url: url.to_string(),
                content_type: Some(content_type.to_string()),
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
