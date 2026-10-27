/// Short, status-line friendly classification of a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    Dns,
    Connect,
    Tls,
    Timeout,
    BadUrl,
    Redirects,
    TooLarge,
    Proxy,
    Io,
    Other,
}

impl NetError {
    /// Label shown in the status line (`ERROR DNS`, `ERROR TIMEOUT`, ...).
    pub fn label(&self) -> &'static str {
        match self {
            NetError::Dns => "DNS",
            NetError::Connect => "CONNECT",
            NetError::Tls => "TLS",
            NetError::Timeout => "TIMEOUT",
            NetError::BadUrl => "BAD URL",
            NetError::Redirects => "REDIRECTS",
            NetError::TooLarge => "TOO LARGE",
            NetError::Proxy => "PROXY",
            NetError::Io => "IO",
            NetError::Other => "ERROR",
        }
    }

    pub fn classify(err: &ureq::Error) -> NetError {
        use ureq::Error as U;
        match err {
            // getaddrinfo failures arrive as an opaque io error on most platforms.
            U::Io(io_err)
                if matches!(io_err.kind(), std::io::ErrorKind::NotFound) || looks_like_dns(io_err) =>
            {
                NetError::Dns
            }
            U::HostNotFound => NetError::Dns,
            U::ConnectionFailed | U::Io(_) | U::Protocol(_) => NetError::Connect,
            U::Tls(_) | U::Rustls(_) | U::Pem(_) | U::TlsRequired => NetError::Tls,
            U::Timeout(_) | U::BodyStalled => NetError::Timeout,
            U::BadUri(_) | U::InvalidProxyUrl => NetError::BadUrl,
            U::TooManyRedirects | U::RedirectFailed => NetError::Redirects,
            U::BodyExceedsLimit(_) | U::LargeResponseHeader(_, _) => NetError::TooLarge,
            U::ConnectProxyFailed(_) => NetError::Proxy,
            U::RequireHttpsOnly(_) => NetError::Other,
            _ => NetError::Other,
        }
    }
}

/// Resolve failures are reported by the resolver in so many words.
fn looks_like_dns(err: &std::io::Error) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    [
        "failed to lookup address",
        "name or service not known",
        "name does not resolve",
        "nodename nor servname",
        "no address associated",
        "temporary failure in name resolution",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] ureq::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("invalid url: {0}")]
    BadUrl(String),

    /// A classified transport failure (also used by the test fetcher).
    #[error("{0}: {1}")]
    Net(NetError, String),

    #[error("config error in {path}: {message}")]
    Config { path: String, message: String },

    #[error("response larger than {0} bytes")]
    TooLarge(u64),

    #[error("no displayable content for {0}")]
    Unrenderable(String),
}

pub fn summarize(err: &Error) -> String {
    match err {
        Error::Http(inner) => {
            let kind = NetError::classify(inner);
            format!("{}: {inner}", kind.label())
        }
        other => other.to_string(),
    }
}
