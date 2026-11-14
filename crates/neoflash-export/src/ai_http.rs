//! Fail-closed HTTPS/TLS streaming and JSON transport for AI provider connections (`T-305`).
//!
//! Built on `ureq` + `rustls` with zero redirect following, strict byte/line caps, cancellable
//! reads, and mandatory `https://` enforcement for cloud providers (`LocalOpenAiCompatible` may
//! use `http://` strictly on verified loopback addresses).

use std::{
    fmt,
    io::{BufRead, BufReader, Read},
    net::{IpAddr, ToSocketAddrs},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use url::{Host, Url};

/// Maximum cumulative response payload accepted from an AI provider (4 MiB).
pub const MAX_AI_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum single SSE or HTTP line length accepted from an AI provider (1 MiB).
pub const MAX_AI_HTTP_LINE_BYTES: usize = 1024 * 1024;
/// Maximum diagnostic body read from a non-2xx AI provider response (64 KiB).
pub const MAX_AI_ERROR_BYTES: usize = 64 * 1024;

/// Transport security policy for an AI endpoint URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiEndpointSecurity {
    /// Cloud providers (`OpenAI`, `Anthropic`, `Gemini`): `https://` is mandatory.
    HttpsRequired,
    /// Local OpenAI-compatible servers: `https://` anywhere, or `http://` strictly on loopback
    /// (`127.0.0.1`, `localhost`, `[::1]`) when every resolved socket address is loopback.
    HttpsOrLoopbackHttp,
}

/// Transport-layer errors returned by [`send_ai_get`], [`send_ai_post_json`], and
/// [`stream_ai_post_lines`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiHttpError {
    InvalidEndpoint(String),
    Offline(String),
    Timeout,
    RateLimited {
        retry_after_ms: Option<u64>,
        body: String,
    },
    HttpStatus {
        status: u16,
        retry_after_ms: Option<u64>,
        body: String,
    },
    RedirectRejected {
        status: u16,
        location: String,
    },
    Protocol(String),
    Cancelled,
}

impl fmt::Display for AiHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(msg) => write!(f, "Invalid endpoint: {msg}"),
            Self::Offline(msg) => write!(f, "Provider unreachable (offline): {msg}"),
            Self::Timeout => f.write_str("Connection timed out."),
            Self::RateLimited {
                retry_after_ms: Some(ms),
                body,
            } => write!(f, "Rate limited (retry after {ms} ms): {body}"),
            Self::RateLimited {
                retry_after_ms: None,
                body,
            } => write!(f, "Rate limited: {body}"),
            Self::HttpStatus { status, body, .. } => {
                if body.is_empty() {
                    write!(f, "HTTP {status}")
                } else {
                    write!(f, "HTTP {status}: {body}")
                }
            }
            Self::RedirectRejected { status, location } => {
                if location.is_empty() {
                    write!(f, "Redirect HTTP {status} rejected.")
                } else {
                    write!(f, "Redirect HTTP {status} to '{location}' rejected.")
                }
            }
            Self::Protocol(msg) => write!(f, "Protocol error: {msg}"),
            Self::Cancelled => f.write_str("Request cancelled."),
        }
    }
}

impl std::error::Error for AiHttpError {}

/// Validated origin and base path for an AI provider endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedAiEndpoint {
    pub scheme: String,
    pub host: String,
    pub host_display: String,
    pub port: Option<u16>,
    pub base_path: String,
    pub is_https: bool,
    pub is_loopback: bool,
}

impl ValidatedAiEndpoint {
    fn origin(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.host_display),
            None => format!("{}://{}", self.scheme, self.host_display),
        }
    }
}

/// Returns `true` when `host` is `localhost` or a literal IPv4/IPv6 loopback address.
pub fn is_loopback_host(host: &str) -> bool {
    let trimmed = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    trimmed.eq_ignore_ascii_case("localhost")
        || trimmed.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Pure helper verifying both the host name and every resolved IP address are loopback.
pub fn validate_loopback_ips(host: &str, resolved_ips: &[IpAddr]) -> Result<(), AiHttpError> {
    if !is_loopback_host(host) || resolved_ips.is_empty() {
        return Err(AiHttpError::InvalidEndpoint(
            "Plain http:// is only allowed for loopback hosts (127.0.0.1, localhost, [::1]); use https:// for remote endpoints.".into(),
        ));
    }
    if resolved_ips.iter().any(|ip| !ip.is_loopback()) {
        return Err(AiHttpError::InvalidEndpoint(
            "Plaintext AI endpoints are limited to loopback; resolved address is not loopback."
                .into(),
        ));
    }
    Ok(())
}

/// Validate a provider Base URL against [`AiEndpointSecurity`].
///
/// When `loopback_http_override` is `None` (all production calls), cloud providers MUST use
/// `https://` and reject all `http://` URLs (including loopback). Local providers may use
/// `https://` or verified loopback `http://`.
///
/// When `loopback_http_override` is `Some(origin)` (deterministic local test fixtures), `http://`
/// is permitted only when `origin` itself is a loopback `http://` origin and `base_url` matches
/// its exact scheme, host, and port.
pub fn validate_ai_base_url(
    base_url: &str,
    security: AiEndpointSecurity,
    loopback_http_override: Option<&str>,
) -> Result<ValidatedAiEndpoint, AiHttpError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_control)
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.contains('\\')
    {
        return Err(AiHttpError::InvalidEndpoint(
            "Endpoint contains empty, whitespace, control, or backslash characters.".into(),
        ));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|e| AiHttpError::InvalidEndpoint(format!("Malformed endpoint URL: {e}")))?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AiHttpError::InvalidEndpoint(
            "Endpoint URL must use https:// (or http:// on loopback for a local provider).".into(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AiHttpError::InvalidEndpoint(
            "Endpoint URL must not contain embedded credentials.".into(),
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AiHttpError::InvalidEndpoint(
            "Endpoint base URL must not contain a query or fragment.".into(),
        ));
    }

    let host_raw = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| AiHttpError::InvalidEndpoint("Endpoint URL is missing a host.".into()))?;
    let unbracketed_host = match parsed.host() {
        Some(Host::Ipv6(addr)) => addr.to_string(),
        _ => host_raw.to_owned(),
    };
    let host_display = match parsed.host() {
        Some(Host::Ipv6(addr)) => format!("[{addr}]"),
        _ => host_raw.to_owned(),
    };

    let is_https = parsed.scheme() == "https";
    let loopback_host = is_loopback_host(&unbracketed_host);

    if !is_https {
        if let Some(override_raw) = loopback_http_override {
            let override_url = Url::parse(override_raw.trim()).map_err(|e| {
                AiHttpError::InvalidEndpoint(format!("Invalid loopback override URL: {e}"))
            })?;
            let override_host = override_url.host_str().unwrap_or_default();
            if override_url.scheme() != "http" || !is_loopback_host(override_host) {
                return Err(AiHttpError::InvalidEndpoint(
                    "Loopback test override must be an http:// loopback origin.".into(),
                ));
            }
            let same_origin = parsed.scheme() == override_url.scheme()
                && parsed.host_str() == override_url.host_str()
                && parsed.port_or_known_default() == override_url.port_or_known_default();
            if !same_origin {
                return Err(AiHttpError::InvalidEndpoint(
                    "Cloud AI providers require https:// (origin did not match loopback fixture)."
                        .into(),
                ));
            }
        } else if security == AiEndpointSecurity::HttpsRequired {
            return Err(AiHttpError::InvalidEndpoint(
                "Cloud AI providers must use https://.".into(),
            ));
        }

        if !loopback_host {
            return Err(AiHttpError::InvalidEndpoint(
                "Plain http:// is only allowed on this computer (127.0.0.1, localhost, [::1]); cloud and remote providers must use https://.".into(),
            ));
        }
        let effective_port = parsed.port_or_known_default().unwrap_or(80);
        if let Ok(ip) = unbracketed_host.parse::<IpAddr>() {
            validate_loopback_ips(&unbracketed_host, &[ip])?;
        } else {
            let addrs: Vec<IpAddr> = (unbracketed_host.as_str(), effective_port)
                .to_socket_addrs()
                .map_err(|e| {
                    AiHttpError::Offline(format!(
                        "Could not resolve {unbracketed_host}:{effective_port}: {e}"
                    ))
                })?
                .map(|sa| sa.ip())
                .collect();
            validate_loopback_ips(&unbracketed_host, &addrs)?;
        }
    }

    let trimmed_path = parsed.path().trim_matches('/');
    let base_path = if trimmed_path.is_empty() {
        String::new()
    } else {
        format!("/{trimmed_path}")
    };

    Ok(ValidatedAiEndpoint {
        scheme: parsed.scheme().to_owned(),
        host: unbracketed_host,
        host_display,
        port: parsed.port(),
        base_path,
        is_https,
        is_loopback: loopback_host,
    })
}

/// Construct and validate a full request URL from a [`ValidatedAiEndpoint`] and a path/query
/// suffix (e.g. `"/v1/messages"` or `"/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"`).
///
/// Fails closed if the query contains credential parameter names (`key`, `api_key`, `apikey`,
/// `access_token`, `x-goog-api-key`).
pub fn build_ai_request_url(
    endpoint: &ValidatedAiEndpoint,
    path_and_query: &str,
) -> Result<Url, AiHttpError> {
    if !path_and_query.starts_with('/')
        || path_and_query.chars().any(char::is_control)
        || path_and_query.chars().any(char::is_whitespace)
        || path_and_query.contains('\\')
    {
        return Err(AiHttpError::InvalidEndpoint(
            "Request path must start with '/' and contain no whitespace, control, or backslash characters.".into(),
        ));
    }

    let full = format!("{}{path_and_query}", endpoint.origin());
    let parsed = Url::parse(&full)
        .map_err(|e| AiHttpError::InvalidEndpoint(format!("Invalid request URL: {e}")))?;

    if parsed.scheme() != endpoint.scheme
        || parsed.host_str().map(str::to_ascii_lowercase)
            != Some(endpoint.host_display.to_ascii_lowercase())
        || parsed.port() != endpoint.port
    {
        return Err(AiHttpError::InvalidEndpoint(
            "Request path altered endpoint origin.".into(),
        ));
    }

    for (param_key, _) in parsed.query_pairs() {
        let lower = param_key.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "key" | "api_key" | "apikey" | "api-key" | "access_token" | "token" | "x-goog-api-key"
        ) {
            return Err(AiHttpError::InvalidEndpoint(
                "API credentials must never be passed in URL query parameters.".into(),
            ));
        }
    }

    Ok(parsed)
}

fn validate_headers(headers: &[(String, String)]) -> Result<(), AiHttpError> {
    for (name, value) in headers {
        if name.trim().is_empty()
            || value.is_empty()
            || name.chars().any(char::is_control)
            || value.chars().any(char::is_control)
        {
            return Err(AiHttpError::InvalidEndpoint(format!(
                "HTTP header '{name}' contains empty or control characters."
            )));
        }
    }
    Ok(())
}

fn build_agent(timeout: Duration, set_global_timeout: bool, bypass_proxy: bool) -> ureq::Agent {
    let connect_timeout = timeout.min(Duration::from_secs(15));
    let resolve_timeout = timeout.min(Duration::from_secs(10));
    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_response_header_size(32 * 1024)
        .timeout_resolve(Some(resolve_timeout))
        .timeout_connect(Some(connect_timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_recv_body(Some(timeout))
        .timeout_global(set_global_timeout.then_some(timeout));
    if bypass_proxy {
        builder = builder.proxy(None);
    }
    ureq::Agent::new_with_config(builder.build())
}

fn map_ureq_error(err: ureq::Error, cancel: &AtomicBool) -> AiHttpError {
    if cancel.load(Ordering::Acquire) {
        return AiHttpError::Cancelled;
    }
    match err {
        ureq::Error::Timeout(_) => AiHttpError::Timeout,
        ureq::Error::Io(io_err) => map_io_error(io_err, cancel),
        ureq::Error::Protocol(msg) => AiHttpError::Protocol(msg.to_string()),
        ureq::Error::BodyExceedsLimit(limit) => {
            AiHttpError::Protocol(format!("Provider response exceeds {limit} bytes."))
        }
        other => AiHttpError::Offline(other.to_string()),
    }
}

fn map_io_error(err: std::io::Error, cancel: &AtomicBool) -> AiHttpError {
    if cancel.load(Ordering::Acquire) {
        return AiHttpError::Cancelled;
    }
    if matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        return AiHttpError::Timeout;
    }
    if let Some(inner) = err
        .get_ref()
        .and_then(|source| source.downcast_ref::<ureq::Error>())
    {
        return match inner {
            ureq::Error::Timeout(_) => AiHttpError::Timeout,
            ureq::Error::Protocol(msg) => AiHttpError::Protocol(msg.to_string()),
            ureq::Error::BodyExceedsLimit(limit) => {
                AiHttpError::Protocol(format!("Provider response exceeds {limit} bytes."))
            }
            other => AiHttpError::Offline(other.to_string()),
        };
    }
    let text = err.to_string();
    let lower = text.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        return AiHttpError::Timeout;
    }
    if matches!(
        err.kind(),
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof
    ) || lower.contains("chunk")
        || lower.contains("invalid http")
    {
        return AiHttpError::Protocol(text);
    }
    AiHttpError::Offline(text)
}

fn validate_transfer_encoding(
    response: &ureq::http::Response<ureq::Body>,
) -> Result<(), AiHttpError> {
    let mut codings = Vec::new();
    for value in response.headers().get_all("transfer-encoding") {
        let raw = value
            .to_str()
            .map_err(|_| AiHttpError::Protocol("Non-ASCII Transfer-Encoding header.".into()))?;
        codings.extend(
            raw.split(',')
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty()),
        );
    }
    if codings.is_empty() {
        return Ok(());
    }
    let valid = codings.last().is_some_and(|c| c == "chunked")
        && codings.iter().filter(|c| *c == "chunked").count() == 1
        && codings
            .iter()
            .all(|c| matches!(c.as_str(), "identity" | "chunked"));
    if valid {
        Ok(())
    } else {
        Err(AiHttpError::Protocol(format!(
            "Unsupported provider Transfer-Encoding: {}",
            codings.join(", ")
        )))
    }
}

fn parse_retry_after_ms(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    if let Some(ms) = response
        .headers()
        .get("retry-after-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(ms.min(300_000));
    }
    if let Some(secs) = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(secs.saturating_mul(1_000).min(300_000));
    }
    None
}

fn redacted_redirect_location(response: &ureq::http::Response<ureq::Body>) -> String {
    let Some(raw) = response
        .headers()
        .get("location")
        .and_then(|h| h.to_str().ok())
    else {
        return String::new();
    };
    if let Ok(mut parsed) = Url::parse(raw.trim()) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        parsed.into()
    } else {
        raw.trim()
            .split('?')
            .next()
            .unwrap_or("")
            .chars()
            .take(128)
            .collect()
    }
}

fn read_bounded_bytes(
    mut reader: impl Read,
    max_bytes: usize,
    cancel: &AtomicBool,
    truncate_instead_of_err: bool,
) -> Result<(Vec<u8>, bool), AiHttpError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(AiHttpError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| map_io_error(e, cancel))?;
        if n == 0 {
            break;
        }
        if cancel.load(Ordering::Acquire) {
            return Err(AiHttpError::Cancelled);
        }
        if out.len().saturating_add(n) > max_bytes {
            if truncate_instead_of_err {
                let remain = max_bytes.saturating_sub(out.len());
                out.extend_from_slice(&buf[..remain]);
                return Ok((out, true));
            }
            return Err(AiHttpError::Protocol(format!(
                "Provider response exceeds the {} MiB limit.",
                max_bytes / (1024 * 1024)
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok((out, false))
}

fn read_error_body(
    response: ureq::http::Response<ureq::Body>,
    cancel: &AtomicBool,
) -> Result<String, AiHttpError> {
    let reader = response.into_body().into_reader();
    let (bytes, truncated) = read_bounded_bytes(reader, MAX_AI_ERROR_BYTES, cancel, true)?;
    let mut text = String::from_utf8_lossy(&bytes).trim().to_owned();
    if truncated {
        text.push_str("\n[diagnostic body truncated]");
    }
    Ok(text)
}

fn check_response_status(
    response: ureq::http::Response<ureq::Body>,
    cancel: &AtomicBool,
) -> Result<ureq::http::Response<ureq::Body>, AiHttpError> {
    if cancel.load(Ordering::Acquire) {
        return Err(AiHttpError::Cancelled);
    }
    validate_transfer_encoding(&response)?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        let location = redacted_redirect_location(&response);
        return Err(AiHttpError::RedirectRejected { status, location });
    }
    let retry_after_ms = parse_retry_after_ms(&response);
    if status == 429 {
        let body = read_error_body(response, cancel)?;
        return Err(AiHttpError::RateLimited {
            retry_after_ms,
            body,
        });
    }
    if !(200..300).contains(&status) {
        let body = read_error_body(response, cancel)?;
        return Err(AiHttpError::HttpStatus {
            status,
            retry_after_ms,
            body,
        });
    }
    if let Some(content_len) = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<usize>().ok())
        && content_len > MAX_AI_RESPONSE_BYTES
    {
        return Err(AiHttpError::Protocol(format!(
            "Provider response exceeds the {} MiB limit.",
            MAX_AI_RESPONSE_BYTES / (1024 * 1024)
        )));
    }
    Ok(response)
}

/// Execute a validated `GET` request (used by `Test connection`).
pub fn send_ai_get(
    endpoint: &ValidatedAiEndpoint,
    path_and_query: &str,
    headers: &[(String, String)],
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String, AiHttpError> {
    if cancel.load(Ordering::Acquire) {
        return Err(AiHttpError::Cancelled);
    }
    validate_headers(headers)?;
    let url = build_ai_request_url(endpoint, path_and_query)?;
    let agent = build_agent(
        timeout.max(Duration::from_millis(25)),
        true,
        endpoint.is_loopback,
    );

    let mut req = agent.get(url.as_str());
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let response = req.call().map_err(|e| map_ureq_error(e, cancel))?;
    let ok_response = check_response_status(response, cancel)?;
    let reader = ok_response.into_body().into_reader();
    let (bytes, _) = read_bounded_bytes(reader, MAX_AI_RESPONSE_BYTES, cancel, false)?;
    String::from_utf8(bytes)
        .map_err(|_| AiHttpError::Protocol("Provider response is not valid UTF-8.".into()))
}

/// Execute a validated non-streaming `POST` JSON request and return the full UTF-8 body.
pub fn send_ai_post_json(
    endpoint: &ValidatedAiEndpoint,
    path_and_query: &str,
    headers: &[(String, String)],
    body: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String, AiHttpError> {
    if cancel.load(Ordering::Acquire) {
        return Err(AiHttpError::Cancelled);
    }
    validate_headers(headers)?;
    let url = build_ai_request_url(endpoint, path_and_query)?;
    let agent = build_agent(
        timeout.max(Duration::from_millis(25)),
        true,
        endpoint.is_loopback,
    );

    let mut req = agent.post(url.as_str());
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let response = req
        .send(body.as_bytes())
        .map_err(|e| map_ureq_error(e, cancel))?;
    let ok_response = check_response_status(response, cancel)?;
    let reader = ok_response.into_body().into_reader();
    let (bytes, _) = read_bounded_bytes(reader, MAX_AI_RESPONSE_BYTES, cancel, false)?;
    String::from_utf8(bytes)
        .map_err(|_| AiHttpError::Protocol("Provider response is not valid UTF-8.".into()))
}

/// Execute a validated streaming `POST` request and invoke `on_line` for each bounded UTF-8 line.
///
/// `on_started` is invoked once immediately after HTTP 2xx status headers are validated.
/// Returning `Ok(false)` from `on_line` terminates reading cleanly (e.g. on `[DONE]` or
/// `message_stop`).
#[allow(clippy::too_many_arguments)]
pub fn stream_ai_post_lines<S, F>(
    endpoint: &ValidatedAiEndpoint,
    path_and_query: &str,
    headers: &[(String, String)],
    body: &str,
    timeout: Duration,
    cancel: &AtomicBool,
    on_started: S,
    mut on_line: F,
) -> Result<(), AiHttpError>
where
    S: FnOnce(),
    F: FnMut(&str) -> Result<bool, AiHttpError>,
{
    if cancel.load(Ordering::Acquire) {
        return Err(AiHttpError::Cancelled);
    }
    validate_headers(headers)?;
    let url = build_ai_request_url(endpoint, path_and_query)?;
    let agent = build_agent(
        timeout.max(Duration::from_millis(25)),
        false,
        endpoint.is_loopback,
    );

    let mut req = agent.post(url.as_str());
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let response = req
        .send(body.as_bytes())
        .map_err(|e| map_ureq_error(e, cancel))?;
    let ok_response = check_response_status(response, cancel)?;
    on_started();

    let mut reader = BufReader::with_capacity(8192, ok_response.into_body().into_reader());
    let mut total_bytes = 0usize;
    let mut line_bytes = Vec::new();

    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(AiHttpError::Cancelled);
        }
        line_bytes.clear();
        loop {
            let available = reader.fill_buf().map_err(|e| map_io_error(e, cancel))?;
            if cancel.load(Ordering::Acquire) {
                return Err(AiHttpError::Cancelled);
            }
            if available.is_empty() {
                break;
            }
            let take = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |idx| idx + 1);
            if line_bytes.len().saturating_add(take) > MAX_AI_HTTP_LINE_BYTES {
                return Err(AiHttpError::Protocol(format!(
                    "Provider HTTP line exceeds {MAX_AI_HTTP_LINE_BYTES} bytes."
                )));
            }
            total_bytes = total_bytes.saturating_add(take);
            if total_bytes > MAX_AI_RESPONSE_BYTES {
                return Err(AiHttpError::Protocol(format!(
                    "Provider response exceeds the {} MiB limit.",
                    MAX_AI_RESPONSE_BYTES / (1024 * 1024)
                )));
            }
            line_bytes.extend_from_slice(&available[..take]);
            reader.consume(take);
            if line_bytes.last() == Some(&b'\n') {
                break;
            }
        }
        if line_bytes.is_empty() {
            break;
        }
        let line = std::str::from_utf8(&line_bytes)
            .map_err(|_| AiHttpError::Protocol("Provider SSE line is not valid UTF-8.".into()))?;
        if !on_line(line)? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
    };

    fn drain_request(socket: &std::net::TcpStream) -> (String, Vec<String>) {
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut req_line = String::new();
        reader.read_line(&mut req_line).unwrap();
        let mut headers = Vec::new();
        let mut content_len = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line == "\n" || line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':')
                && k.eq_ignore_ascii_case("content-length")
            {
                content_len = v.trim().parse().unwrap_or(0);
            }
            headers.push(line.trim().to_owned());
        }
        if content_len > 0 {
            let mut body = vec![0u8; content_len];
            reader.read_exact(&mut body).unwrap();
        }
        (req_line, headers)
    }

    #[test]
    fn cloud_endpoints_require_https_and_reject_plain_http_even_on_loopback_without_override() {
        assert!(
            validate_ai_base_url(
                "https://api.openai.com",
                AiEndpointSecurity::HttpsRequired,
                None
            )
            .is_ok()
        );
        assert!(
            validate_ai_base_url(
                "https://api.anthropic.com",
                AiEndpointSecurity::HttpsRequired,
                None
            )
            .is_ok()
        );
        assert!(
            validate_ai_base_url(
                "https://generativelanguage.googleapis.com",
                AiEndpointSecurity::HttpsRequired,
                None
            )
            .is_ok()
        );

        for disallowed in [
            "http://api.openai.com",
            "http://127.0.0.1:8080",
            "http://localhost:11434",
            "https://user:pass@api.openai.com",
            "https://api.openai.com?key=secret",
        ] {
            assert!(
                matches!(
                    validate_ai_base_url(disallowed, AiEndpointSecurity::HttpsRequired, None),
                    Err(AiHttpError::InvalidEndpoint(_))
                ),
                "expected {disallowed} to be rejected"
            );
        }

        // LocalOpenAiCompatible allows loopback http:// and https://, rejects non-loopback http://
        assert!(
            validate_ai_base_url(
                "http://127.0.0.1:11434",
                AiEndpointSecurity::HttpsOrLoopbackHttp,
                None
            )
            .is_ok()
        );
        assert!(
            validate_ai_base_url(
                "http://[::1]:11434/v1",
                AiEndpointSecurity::HttpsOrLoopbackHttp,
                None
            )
            .is_ok()
        );
        assert!(matches!(
            validate_ai_base_url(
                "http://192.0.2.10:11434",
                AiEndpointSecurity::HttpsOrLoopbackHttp,
                None
            ),
            Err(AiHttpError::InvalidEndpoint(_))
        ));

        // DNS rebinding guard: localhost resolving to a public IP is rejected
        assert!(validate_loopback_ips("localhost", &["192.0.2.1".parse().unwrap()]).is_err());
        assert!(validate_loopback_ips("localhost", &["127.0.0.1".parse().unwrap()]).is_ok());
    }

    #[test]
    fn query_string_api_keys_and_http_redirects_are_rejected() {
        let ep = validate_ai_base_url(
            "https://generativelanguage.googleapis.com",
            AiEndpointSecurity::HttpsRequired,
            None,
        )
        .unwrap();
        assert!(
            build_ai_request_url(
                &ep,
                "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
            )
            .is_ok()
        );
        assert!(matches!(
            build_ai_request_url(
                &ep,
                "/v1beta/models/gemini-2.5-flash:generateContent?key=AIzaSecret"
            ),
            Err(AiHttpError::InvalidEndpoint(_))
        ));

        // Cross-host and HTTPS->HTTP downgrade redirects are rejected without following
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = drain_request(&sock);
            sock.write_all(
                b"HTTP/1.1 302 Found\r\nLocation: http://evil.example.com/v1/models?key=leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            sock.flush().unwrap();
        });
        let origin = format!("http://{addr}");
        let fixture_ep =
            validate_ai_base_url(&origin, AiEndpointSecurity::HttpsRequired, Some(&origin))
                .unwrap();
        let cancel = AtomicBool::new(false);
        let err = send_ai_get(
            &fixture_ep,
            "/v1/models",
            &[("Authorization".into(), "Bearer sk-secret".into())],
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(
            err,
            AiHttpError::RedirectRejected {
                status: 302,
                location: "http://evil.example.com/v1/models".into(),
            }
        );
    }
}
