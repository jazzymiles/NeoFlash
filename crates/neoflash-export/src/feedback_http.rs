//! HTTPS `multipart/form-data` sender and attachment validator for Beta feedback (`R-103`, `T-317`).
//!
//! Privacy rule (`R-103` item 6): only the fields and images visible in the `"Beta feedback"`
//! dialog are transmitted — the `message` text, up to five validated image attachments
//! (`PNG`, `JPEG`, `GIF`, `WebP`, ≤ 10 MB each and ≤ 10 MB combined), and the constant FormSubmit
//! protocol flag `_captcha=false` strictly when posting to `formsubmit.co` / `*.formsubmit.co`.
//! No build version, OS, machine identifier, or open `.nflash` document metadata is attached to
//! headers or form fields.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use neoflash_core::sha256::Sha256;
use serde_json::Value;
use std::{
    fmt,
    io::Read as _,
    net::{IpAddr, ToSocketAddrs},
    time::Duration,
};

/// Default FormSubmit AJAX endpoint delivering directly to `developer@redinger.cc`.
pub const DEFAULT_BETA_FEEDBACK_URL: &str = "https://formsubmit.co/ajax/developer@redinger.cc";

/// Optional environment variable overriding [`DEFAULT_BETA_FEEDBACK_URL`].
pub const BETA_FEEDBACK_URL_ENV: &str = "NEOFLASH_FEEDBACK_URL";

/// Fixed origin header required by FormSubmit AJAX (carries zero user/device telemetry).
pub const DEFAULT_FEEDBACK_ORIGIN: &str = "https://redinger.cc";

/// Fixed referer header required by FormSubmit AJAX (carries zero user/device telemetry).
pub const DEFAULT_FEEDBACK_REFERER: &str = "https://redinger.cc/";

/// Generic constant `User-Agent` that intentionally omits OS, CPU, and NeoFlash version metadata.
pub const FEEDBACK_USER_AGENT: &str = "NeoFlash-Feedback/1";

/// Maximum number of attached images per feedback submission (`R-103` item 4).
pub const MAX_FEEDBACK_ATTACHMENTS: usize = 5;

/// Maximum size of a single attached image in bytes (`10 MB`, `R-103` item 4).
pub const MAX_FEEDBACK_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum combined size of all attached images in bytes (`10 MB`, FormSubmit multipart limit).
pub const MAX_FEEDBACK_TOTAL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum size of the feedback `message` text in UTF-8 bytes (`64 KiB`).
pub const MAX_FEEDBACK_MESSAGE_BYTES: usize = 64 * 1024;

/// Actionable, typo-free error message shown when FormSubmit awaits one-time form activation.
pub const FORMSUBMIT_ACTIVATION_ERROR: &str = "Feedback form is awaiting one-time activation: open the FormSubmit email sent to developer@redinger.cc (check Spam), click 'Activate Form', then click Send again.";

const RESPONSE_SNIPPET_BYTES: usize = 16 * 1024;
const MULTIPART_BOUNDARY_PREFIX: &str = "----NeoFlashFeedbackBoundary";
const MULTIPART_BOUNDARY_DIGEST_HEX_LEN: usize = 40;
const MAX_MULTIPART_BOUNDARY_ATTEMPTS: u32 = 8;

/// A validated image attachment held in session memory for `"Beta feedback"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackAttachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Errors produced when validating or sending beta feedback over HTTP(S).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BetaFeedbackHttpError {
    InvalidUrl(String),
    Validation(String),
    OfflineOrNetwork(String),
    ServerRejected { status: Option<u16>, detail: String },
}

impl fmt::Display for BetaFeedbackHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "Invalid feedback endpoint URL: {msg}"),
            Self::Validation(msg) => f.write_str(msg),
            Self::OfflineOrNetwork(msg) => write!(f, "Could not send feedback: {msg}"),
            Self::ServerRejected { detail, .. } if detail == FORMSUBMIT_ACTIVATION_ERROR => {
                f.write_str(detail)
            }
            Self::ServerRejected {
                status: Some(code),
                detail,
            } => {
                if detail.is_empty() {
                    write!(f, "Could not send feedback (HTTP {code}).")
                } else {
                    write!(f, "Could not send feedback (HTTP {code}): {detail}")
                }
            }
            Self::ServerRejected {
                status: None,
                detail,
            } => {
                if detail.is_empty() {
                    f.write_str("Could not send feedback: server rejected the submission.")
                } else {
                    write!(f, "Could not send feedback: {detail}")
                }
            }
        }
    }
}

impl std::error::Error for BetaFeedbackHttpError {}

/// Inspect magic bytes and return the canonical MIME type if `bytes` is a supported image
/// (`PNG`, `JPEG`, `GIF`, or `WebP`; `R-103` item 4).
pub fn detect_feedback_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Decode a `data:image/...;base64,...` URL into raw bytes if well-formed.
pub fn decode_feedback_image_data_url(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("data:image/")?;
    let (_, b64) = rest.split_once(";base64,")?;
    let clean: String = b64.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    BASE64.decode(clean).ok()
}

/// Format an attachment byte length for the `32 pt` row (`"412 KB"`, `"1.2 MB"`, `"850 B"`).
pub fn format_attachment_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if (bytes as f64) < MB {
        let kb = ((bytes as f64) / KB).round().max(1.0) as usize;
        format!("{kb} KB")
    } else {
        format!("{:.1} MB", (bytes as f64) / MB)
    }
}

/// Validate one candidate image attachment by size (`1..=10 MB`) and magic bytes
/// (`PNG`, `JPEG`, `GIF`, `WebP`), returning a user-facing error naming the file and reason on failure.
pub fn validate_feedback_attachment(
    filename: &str,
    bytes: &[u8],
) -> Result<FeedbackAttachment, String> {
    let display = clean_display_filename(filename);
    if bytes.is_empty() {
        return Err(format!("“{display}” could not be attached: file is empty."));
    }
    if bytes.len() > MAX_FEEDBACK_ATTACHMENT_BYTES {
        return Err(format!(
            "“{display}” exceeds the 10 MB limit ({}).",
            format_attachment_size(bytes.len())
        ));
    }
    let Some(mime) = detect_feedback_image_mime(bytes) else {
        return Err(format!(
            "“{display}” is not a supported image (PNG, JPEG, GIF or WebP required)."
        ));
    };
    Ok(FeedbackAttachment {
        filename: display,
        content_type: mime.to_owned(),
        bytes: bytes.to_vec(),
    })
}

fn clean_display_filename(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let trimmed = base.trim();
    if trimmed.is_empty() {
        "image".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn escape_multipart_filename(raw: &str) -> String {
    let cleaned = clean_display_filename(raw);
    let mut out = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        match ch {
            '"' => out.push_str("%22"),
            '\\' => out.push('_'),
            '\r' | '\n' | '\0' => {}
            other => out.push(other),
        }
    }
    if out.is_empty() {
        "image".to_owned()
    } else {
        out
    }
}

fn is_loopback_host(host: &str) -> bool {
    let trimmed = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    trimmed.eq_ignore_ascii_case("localhost")
        || trimmed.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Pure helper verifying both the host name and every resolved socket IP address are loopback.
pub fn validate_feedback_loopback_ips(
    host: &str,
    resolved_ips: &[IpAddr],
) -> Result<(), BetaFeedbackHttpError> {
    if !is_loopback_host(host) || resolved_ips.is_empty() {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "plain http:// is only allowed for loopback hosts (127.0.0.1, localhost, [::1]); use https:// for remote endpoints".into(),
        ));
    }
    if resolved_ips.iter().any(|ip| !ip.is_loopback()) {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "plain http:// feedback endpoint resolved to a non-loopback address".into(),
        ));
    }
    Ok(())
}

/// Return `true` when `url` targets `formsubmit.co` or a `*.formsubmit.co` subdomain.
pub fn is_formsubmit_endpoint(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    lower == "formsubmit.co" || lower.ends_with(".formsubmit.co")
}

fn is_bare_formsubmit_token(raw: &str) -> bool {
    // FormSubmit replacement tokens are bare alphanumeric/hex/UUID-like identifiers
    // (for example, a 32-character alphanumeric token). Require 16..=128 characters
    // from `[A-Za-z0-9_-]` so invalid URLs or schemes still fail validation cleanly.
    (16..=128).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && raw.bytes().any(|b| b.is_ascii_alphanumeric())
}

/// Validate and normalize a feedback endpoint URL or bare FormSubmit token.
///
/// - Bare FormSubmit tokens (16..=128 alphanumeric/`-`/`_` chars) normalize to
///   `https://formsubmit.co/ajax/<token>`.
/// - Non-AJAX `https://formsubmit.co/<target>` URLs normalize to
///   `https://formsubmit.co/ajax/<target>` while preserving already-`/ajax/` paths.
/// - Remote hosts MUST use `https://`. Plain `http://` is permitted strictly on loopback
///   (`127.0.0.1`, `localhost`, `[::1]`) when every resolved socket IP address is loopback.
pub fn validate_feedback_url(raw: &str) -> Result<(url::Url, bool), BetaFeedbackHttpError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_control)
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.contains('\\')
    {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "endpoint URL is empty or contains whitespace, control, or backslash characters".into(),
        ));
    }
    let candidate = if is_bare_formsubmit_token(trimmed) {
        format!("https://formsubmit.co/ajax/{trimmed}")
    } else {
        trimmed.to_owned()
    };
    let mut parsed = url::Url::parse(&candidate)
        .map_err(|e| BetaFeedbackHttpError::InvalidUrl(e.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "URL must not contain credentials".into(),
        ));
    }
    let Some(host) = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .map(str::to_owned)
    else {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "URL is missing a host".into(),
        ));
    };
    let loopback = is_loopback_host(&host);
    match parsed.scheme() {
        "https" => {
            if is_formsubmit_endpoint(&parsed) {
                let path = parsed.path().to_owned();
                let lower_path = path.to_ascii_lowercase();
                if path.is_empty() || path == "/" || lower_path == "/ajax" || lower_path == "/ajax/"
                {
                    return Err(BetaFeedbackHttpError::InvalidUrl(
                        "FormSubmit endpoint URL is missing a target recipient or token".into(),
                    ));
                }
                if !lower_path.starts_with("/ajax/") {
                    parsed.set_path(&format!("/ajax{path}"));
                } else if &path[..6] != "/ajax/" {
                    parsed.set_path(&format!("/ajax/{}", &path[6..]));
                }
            }
            Ok((parsed, loopback))
        }
        "http" if loopback => {
            let unbracketed_host = match parsed.host() {
                Some(url::Host::Ipv6(addr)) => addr.to_string(),
                _ => host.clone(),
            };
            let effective_port = parsed.port_or_known_default().unwrap_or(80);
            if let Ok(ip) = unbracketed_host.parse::<IpAddr>() {
                validate_feedback_loopback_ips(&unbracketed_host, &[ip])?;
            } else {
                let addrs: Vec<IpAddr> = (unbracketed_host.as_str(), effective_port)
                    .to_socket_addrs()
                    .map_err(|e| {
                        BetaFeedbackHttpError::InvalidUrl(format!(
                            "could not resolve loopback host '{unbracketed_host}:{effective_port}': {e}"
                        ))
                    })?
                    .map(|sa| sa.ip())
                    .collect();
                validate_feedback_loopback_ips(&unbracketed_host, &addrs)?;
            }
            Ok((parsed, true))
        }
        "http" => Err(BetaFeedbackHttpError::InvalidUrl(format!(
            "remote feedback endpoint '{host}' must use https://"
        ))),
        other => Err(BetaFeedbackHttpError::InvalidUrl(format!(
            "unsupported URL scheme '{other}'"
        ))),
    }
}

/// Resolve the active feedback endpoint URL from an explicit override, an environment override
/// (`NEOFLASH_FEEDBACK_URL`), or [`DEFAULT_BETA_FEEDBACK_URL`].
pub fn resolve_feedback_url_with_env(
    explicit_override: Option<&str>,
    env_override: Option<&str>,
) -> Result<(url::Url, bool), BetaFeedbackHttpError> {
    if let Some(raw) = explicit_override {
        return validate_feedback_url(raw);
    }
    if let Some(env_val) = env_override {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return validate_feedback_url(trimmed);
        }
    }
    validate_feedback_url(DEFAULT_BETA_FEEDBACK_URL)
}

/// Resolve the active feedback endpoint URL from an explicit override, [`BETA_FEEDBACK_URL_ENV`],
/// or [`DEFAULT_BETA_FEEDBACK_URL`].
pub fn resolve_feedback_url(
    explicit_override: Option<&str>,
) -> Result<(url::Url, bool), BetaFeedbackHttpError> {
    let env_val = std::env::var(BETA_FEEDBACK_URL_ENV).ok();
    resolve_feedback_url_with_env(explicit_override, env_val.as_deref())
}

fn hash_multipart_value(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn feedback_payload_digest(message: &str, attachments: &[FeedbackAttachment]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"NeoFlash feedback multipart payload v1\0");
    hash_multipart_value(&mut hasher, message.as_bytes());
    hasher.update(&(attachments.len() as u64).to_be_bytes());
    for attachment in attachments {
        hash_multipart_value(&mut hasher, attachment.filename.as_bytes());
        hash_multipart_value(&mut hasher, attachment.content_type.as_bytes());
        hash_multipart_value(&mut hasher, &attachment.bytes);
    }
    hasher.finalize()
}

fn boundary_candidate(payload_digest: &[u8; 32], attempt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"NeoFlash feedback multipart boundary v1\0");
    hasher.update(payload_digest);
    hasher.update(&attempt.to_be_bytes());
    let digest_hex = hasher.finalize_hex();

    // A multipart boundary is limited to 70 characters by RFC 2046. The 28-character
    // product prefix plus 40 digest hex characters stays within that limit while retaining
    // 160 bits of collision resistance.
    format!(
        "{MULTIPART_BOUNDARY_PREFIX}{}",
        &digest_hex[..MULTIPART_BOUNDARY_DIGEST_HEX_LEN]
    )
}

fn boundary_collides(candidate: &str, message: &str, attachments: &[FeedbackAttachment]) -> bool {
    let needle = candidate.as_bytes();
    message
        .as_bytes()
        .windows(needle.len())
        .any(|window| window == needle)
        || attachments.iter().any(|attachment| {
            attachment
                .filename
                .as_bytes()
                .windows(needle.len())
                .any(|window| window == needle)
                || attachment
                    .bytes
                    .windows(needle.len())
                    .any(|window| window == needle)
        })
}

fn choose_boundary_from_digest(
    message: &str,
    attachments: &[FeedbackAttachment],
    payload_digest: &[u8; 32],
) -> Result<String, BetaFeedbackHttpError> {
    for attempt in 0..MAX_MULTIPART_BOUNDARY_ATTEMPTS {
        let candidate = boundary_candidate(payload_digest, attempt);
        if !boundary_collides(&candidate, message, attachments) {
            return Ok(candidate);
        }
    }

    Err(BetaFeedbackHttpError::Validation(
        "Could not construct a safe feedback request. Please edit the message or attachment names and try again."
            .into(),
    ))
}

fn choose_boundary(
    message: &str,
    attachments: &[FeedbackAttachment],
) -> Result<String, BetaFeedbackHttpError> {
    let payload_digest = feedback_payload_digest(message, attachments);
    choose_boundary_from_digest(message, attachments, &payload_digest)
}

/// Build an RFC 7578 `multipart/form-data` payload containing **only** the `message` text and
/// validated image attachments (`R-103` item 6).
///
/// Field naming for FormSubmit compatibility:
/// - `message` (RFC 7578 `FormData` text field without `Content-Type` header)
/// - `attachment` when 1 image is attached; `attachment_1`..`attachment_N` when 2..=5 images are attached.
pub fn build_feedback_multipart_body(
    message: &str,
    attachments: &[FeedbackAttachment],
) -> Result<(String, Vec<u8>), BetaFeedbackHttpError> {
    build_feedback_multipart_body_for_url(message, attachments, None)
}

/// Build an RFC 7578 `multipart/form-data` payload for `target_url`.
///
/// When `target_url` is a FormSubmit host (`formsubmit.co` or `*.formsubmit.co`), includes the
/// constant headless protocol field `_captcha=false` (carrying zero device/OS/version/document
/// telemetry) so FormSubmit's `/ajax/` handler returns JSON instead of an HTML reCAPTCHA wall.
pub fn build_feedback_multipart_body_for_url(
    message: &str,
    attachments: &[FeedbackAttachment],
    target_url: Option<&url::Url>,
) -> Result<(String, Vec<u8>), BetaFeedbackHttpError> {
    if message.trim().is_empty() {
        return Err(BetaFeedbackHttpError::Validation(
            "Feedback text cannot be empty.".into(),
        ));
    }
    if message.len() > MAX_FEEDBACK_MESSAGE_BYTES {
        return Err(BetaFeedbackHttpError::Validation(format!(
            "Feedback text exceeds the 64 KB limit ({}).",
            format_attachment_size(message.len())
        )));
    }
    if attachments.len() > MAX_FEEDBACK_ATTACHMENTS {
        return Err(BetaFeedbackHttpError::Validation(
            "At most 5 images can be attached.".into(),
        ));
    }
    let mut running_total_bytes: usize = 0;
    for att in attachments {
        let display_name = clean_display_filename(&att.filename);
        if att.bytes.is_empty() {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{display_name}” could not be attached: file is empty.",
            )));
        }
        if att.bytes.len() > MAX_FEEDBACK_ATTACHMENT_BYTES {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{display_name}” exceeds the 10 MB limit ({}).",
                format_attachment_size(att.bytes.len())
            )));
        }
        let Some(detected_mime) = detect_feedback_image_mime(&att.bytes) else {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{display_name}” is not a supported image (PNG, JPEG, GIF or WebP required).",
            )));
        };
        if att.content_type != detected_mime {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{display_name}” MIME type mismatch (expected {detected_mime}, got {}).",
                att.content_type
            )));
        }
        running_total_bytes = running_total_bytes.saturating_add(att.bytes.len());
        if running_total_bytes > MAX_FEEDBACK_TOTAL_ATTACHMENT_BYTES {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{display_name}” could not be attached: combined attachments exceed the 10 MB total limit ({}).",
                format_attachment_size(running_total_bytes)
            )));
        }
    }

    let include_formsubmit_captcha = target_url.is_some_and(is_formsubmit_endpoint);
    let boundary = choose_boundary(message, attachments)?;
    let total_bytes: usize = 384
        + message.len()
        + attachments
            .iter()
            .map(|a| 256 + a.filename.len() + a.bytes.len())
            .sum::<usize>();
    let mut body = Vec::with_capacity(total_bytes);

    // 1. User feedback text (`message`) — RFC 7578 `FormData` text part without `Content-Type`.
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"message\"\r\n\r\n");
    body.extend_from_slice(message.as_bytes());
    body.extend_from_slice(b"\r\n");

    // 2. Constant `_captcha=false` strictly for FormSubmit hosts (`formsubmit.co` / `*.formsubmit.co`).
    if include_formsubmit_captcha {
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(
            b"\r\nContent-Disposition: form-data; name=\"_captcha\"\r\n\r\nfalse\r\n",
        );
    }

    // 3. Attached images (`attachment` for single file, `attachment_1..N` for multiple files).
    let single = attachments.len() == 1;
    for (idx, att) in attachments.iter().enumerate() {
        let field_name = if single {
            "attachment".to_owned()
        } else {
            format!("attachment_{}", idx + 1)
        };
        let safe_filename = escape_multipart_filename(&att.filename);
        let Some(mime) = detect_feedback_image_mime(&att.bytes) else {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{}” is not a supported image.",
                clean_display_filename(&att.filename)
            )));
        };
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        body.extend_from_slice(field_name.as_bytes());
        body.extend_from_slice(b"\"; filename=\"");
        body.extend_from_slice(safe_filename.as_bytes());
        body.extend_from_slice(b"\"\r\nContent-Type: ");
        body.extend_from_slice(mime.as_bytes());
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(&att.bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    Ok((boundary, body))
}

fn build_agent(loopback_mode: bool) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_response_header_size(32 * 1024)
        .timeout_resolve(Some(Duration::from_secs(5)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(45)));
    if loopback_mode {
        builder = builder.proxy(None);
    }
    ureq::Agent::new_with_config(builder.build())
}

fn map_ureq_error(err: ureq::Error) -> BetaFeedbackHttpError {
    match err {
        ureq::Error::Timeout(kind) => {
            BetaFeedbackHttpError::OfflineOrNetwork(format!("request timed out ({kind:?})"))
        }
        other => BetaFeedbackHttpError::OfflineOrNetwork(other.to_string()),
    }
}

struct FeedbackResponseSnippet {
    text: String,
    truncated: bool,
}

fn read_body_snippet(
    body: &mut ureq::Body,
) -> Result<FeedbackResponseSnippet, BetaFeedbackHttpError> {
    // Read one byte beyond the retained prefix so an oversized JSON response cannot be
    // mistaken for a complete, successful response. Using `BodyWithConfig::limit` would lose
    // the useful prefix entirely when the body exceeds the limit.
    let mut bytes = Vec::with_capacity(RESPONSE_SNIPPET_BYTES + 1);
    let mut reader = body
        .with_config()
        .reader()
        .take((RESPONSE_SNIPPET_BYTES + 1) as u64);
    reader.read_to_end(&mut bytes).map_err(|error| {
        BetaFeedbackHttpError::OfflineOrNetwork(format!(
            "could not read feedback response body: {error}"
        ))
    })?;
    let truncated = bytes.len() > RESPONSE_SNIPPET_BYTES;
    bytes.truncate(RESPONSE_SNIPPET_BYTES);
    Ok(FeedbackResponseSnippet {
        text: String::from_utf8_lossy(&bytes).trim().to_owned(),
        truncated,
    })
}

fn looks_like_html_document(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    let prefix: String = trimmed
        .chars()
        .take(128)
        .collect::<String>()
        .to_ascii_lowercase();
    prefix.starts_with("<!doctype html")
        || prefix.starts_with("<html")
        || prefix.starts_with("<?xml") && prefix.contains("<html")
}

fn contains_html_markup(raw: &str) -> bool {
    if looks_like_html_document(raw) {
        return true;
    }
    let lower = raw.to_ascii_lowercase();
    lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<head")
        || lower.contains("<div")
        || lower.contains("<p>")
        || lower.contains("<p ")
        || lower.contains("<h1")
        || lower.contains("<h2")
        || lower.contains("<title")
        || lower.contains("<script")
        || lower.contains("<style")
        || lower.contains("</a>")
        || lower.contains("<br")
}

fn remove_case_insensitive_tag_block(input: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open_pat) else {
            out.push_str(rest);
            break;
        };
        let after_tag_idx = start + open_pat.len();
        let valid_boundary = rest
            .as_bytes()
            .get(after_tag_idx)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        if !valid_boundary {
            out.push_str(&rest[..after_tag_idx]);
            rest = &rest[after_tag_idx..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push(' ');
        let tail_lower = &lower[after_tag_idx..];
        if let Some(end_rel) = tail_lower.find(&close_pat) {
            let resume = after_tag_idx + end_rel + close_pat.len();
            rest = &rest[resume..];
        } else {
            break;
        }
    }
    out
}

/// Strip HTML comments, `<script>`/`<style>` blocks, and `<...>` tags into clean single-line text.
pub fn strip_html_markup(raw: &str) -> String {
    let without_scripts = remove_case_insensitive_tag_block(raw, "script");
    let without_styles = remove_case_insensitive_tag_block(&without_scripts, "style");

    let mut un_commented = String::with_capacity(without_styles.len());
    let mut rem = without_styles.as_str();
    while let Some(start) = rem.find("<!--") {
        un_commented.push_str(&rem[..start]);
        un_commented.push(' ');
        if let Some(end_rel) = rem[start + 4..].find("-->") {
            rem = &rem[start + 4 + end_rel + 3..];
        } else {
            rem = "";
            break;
        }
    }
    un_commented.push_str(rem);

    let mut plain = String::with_capacity(un_commented.len());
    let mut in_tag = false;
    for ch in un_commented.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                plain.push(' ');
            }
        } else if ch == '<' {
            in_tag = true;
            plain.push(' ');
        } else {
            plain.push(ch);
        }
    }

    let decoded = plain
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");

    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_formsubmit_activation_notice(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("needs activation")
        || lower.contains("activate form")
        || lower.contains("not actived")
        || lower.contains("be actived")
        || lower.contains("not activated")
        || lower.contains("form is not active")
}

fn is_captcha_challenge_notice(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("recaptcha")
        || lower.contains("hcaptcha")
        || lower.contains("cf-turnstile")
        || lower.contains("verify you are human")
        || lower.contains("complete the captcha")
}

fn sanitize_error_detail(raw: &str, max_chars: usize) -> String {
    let cleaned = if contains_html_markup(raw) {
        strip_html_markup(raw)
    } else {
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    cleaned.chars().take(max_chars).collect()
}

fn extract_json_message(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    for key in ["message", "error", "detail"] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.chars().take(320).collect());
            }
        }
    }
    None
}

fn is_json_logical_failure(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    for key in ["success", "ok"] {
        match obj.get(key) {
            Some(Value::Bool(false)) => return true,
            Some(Value::String(s)) => {
                let lower = s.trim().to_ascii_lowercase();
                if lower == "false" || lower == "0" {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn interpret_feedback_http_response(
    status: u16,
    raw_body: &str,
) -> Result<(), BetaFeedbackHttpError> {
    if (300..400).contains(&status) {
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: Some(status),
            detail: "server returned an unexpected redirect".into(),
        });
    }

    if status == 413 {
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: Some(413),
            detail: "combined attachments exceed the 10 MB total limit; remove or shrink an image and try again.".into(),
        });
    }

    let trimmed = raw_body.trim();
    let looks_like_json = trimmed.starts_with('{') || trimmed.starts_with('[');
    let parsed_json = if looks_like_json {
        serde_json::from_str::<Value>(trimmed).ok()
    } else {
        None
    };

    if (200..300).contains(&status) {
        if looks_like_json && parsed_json.is_none() {
            return Err(BetaFeedbackHttpError::ServerRejected {
                status: Some(status),
                detail: "feedback endpoint returned malformed JSON; the submission result could not be verified."
                    .into(),
            });
        }
        if let Some(json) = &parsed_json
            && is_json_logical_failure(json)
        {
            let raw_detail = extract_json_message(json)
                .unwrap_or_else(|| "server rejected the feedback submission".into());
            if is_formsubmit_activation_notice(&raw_detail) {
                return Err(BetaFeedbackHttpError::ServerRejected {
                    status: None,
                    detail: FORMSUBMIT_ACTIVATION_ERROR.into(),
                });
            }
            let detail = sanitize_error_detail(&raw_detail, 240);
            return Err(BetaFeedbackHttpError::ServerRejected {
                status: None,
                detail: if detail.is_empty() {
                    "server rejected the feedback submission".into()
                } else {
                    detail
                },
            });
        }
        if parsed_json.is_none() && looks_like_html_document(trimmed) {
            let stripped = strip_html_markup(trimmed);
            if is_formsubmit_activation_notice(&stripped) {
                return Err(BetaFeedbackHttpError::ServerRejected {
                    status: None,
                    detail: FORMSUBMIT_ACTIVATION_ERROR.into(),
                });
            }
            if is_captcha_challenge_notice(&stripped) {
                return Err(BetaFeedbackHttpError::ServerRejected {
                    status: None,
                    detail: "feedback endpoint returned an HTML CAPTCHA challenge instead of JSON."
                        .into(),
                });
            }
            let summary: String = stripped.chars().take(200).collect();
            let detail = if summary.is_empty() {
                "feedback endpoint returned an unexpected HTML page instead of JSON.".into()
            } else {
                format!("feedback endpoint returned HTML instead of JSON: {summary}")
            };
            return Err(BetaFeedbackHttpError::ServerRejected {
                status: None,
                detail,
            });
        }
        return Ok(());
    }

    let raw_detail = parsed_json
        .as_ref()
        .and_then(extract_json_message)
        .unwrap_or_else(|| trimmed.to_owned());
    let cleaned_detail = sanitize_error_detail(&raw_detail, 240);
    if is_formsubmit_activation_notice(&raw_detail)
        || is_formsubmit_activation_notice(&cleaned_detail)
    {
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: None,
            detail: FORMSUBMIT_ACTIVATION_ERROR.into(),
        });
    }
    if is_captcha_challenge_notice(&cleaned_detail) && contains_html_markup(trimmed) {
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: Some(status),
            detail: "upstream challenge page blocked automated feedback submission.".into(),
        });
    }
    let detail = if cleaned_detail.is_empty() && contains_html_markup(trimmed) {
        "server returned an HTML error page.".into()
    } else {
        cleaned_detail
    };
    Err(BetaFeedbackHttpError::ServerRejected {
        status: Some(status),
        detail,
    })
}

fn interpret_feedback_http_response_snippet(
    status: u16,
    snippet: &FeedbackResponseSnippet,
) -> Result<(), BetaFeedbackHttpError> {
    if snippet.truncated {
        let stripped = strip_html_markup(&snippet.text);
        if contains_html_markup(&snippet.text)
            && (is_formsubmit_activation_notice(&stripped)
                || is_captcha_challenge_notice(&stripped))
        {
            return interpret_feedback_http_response(status, &snippet.text);
        }
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: Some(status),
            detail: format!(
                "feedback endpoint returned more than {} KB; the submission result could not be verified.",
                RESPONSE_SNIPPET_BYTES / 1024
            ),
        });
    }

    interpret_feedback_http_response(status, &snippet.text)
}

/// Send beta feedback as an RFC 7578 `multipart/form-data` POST request.
///
/// Validates both HTTP status (`2xx`, redirects disabled) and JSON logical status
/// (detecting FormSubmit `200 OK` responses carrying `{"success":"false","message":"..."}`).
pub fn send_beta_feedback_multipart(
    message: &str,
    attachments: &[FeedbackAttachment],
    url_override: Option<&str>,
) -> Result<(), BetaFeedbackHttpError> {
    let (url, loopback) = resolve_feedback_url(url_override)?;
    let (boundary, body) = build_feedback_multipart_body_for_url(message, attachments, Some(&url))?;
    let content_type = format!("multipart/form-data; boundary={boundary}");
    let agent = build_agent(loopback);

    let mut response = agent
        .post(url.as_str())
        .header("Content-Type", &content_type)
        .header("Accept", "application/json")
        .header("Origin", DEFAULT_FEEDBACK_ORIGIN)
        .header("Referer", DEFAULT_FEEDBACK_REFERER)
        .header("User-Agent", FEEDBACK_USER_AGENT)
        .send(&body[..])
        .map_err(map_ureq_error)?;

    let status = response.status().as_u16();
    let snippet = read_body_snippet(response.body_mut())?;
    interpret_feedback_http_response_snippet(status, &snippet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn sample_png() -> Vec<u8> {
        vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
            b'D', b'R',
        ]
    }

    fn sample_jpeg() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F']
    }

    fn sample_gif() -> Vec<u8> {
        b"GIF89a\x01\x00\x01\x00\x00\xff\x00,".to_vec()
    }

    fn sample_webp() -> Vec<u8> {
        b"RIFF\x14\x00\x00\x00WEBPVP8 \x08\x00\x00\x00".to_vec()
    }

    #[test]
    fn crafted_multipart_boundary_collisions_retry_then_fail_closed() {
        let payload_digest = [0xA5; 32];
        let first_candidate = boundary_candidate(&payload_digest, 0);
        let message = format!("A crafted report containing {first_candidate}");

        let selected = choose_boundary_from_digest(&message, &[], &payload_digest)
            .expect("one colliding candidate must advance to the next digest-derived boundary");
        assert_eq!(selected, boundary_candidate(&payload_digest, 1));
        assert!(!message.contains(&selected));
        assert!(selected.len() <= 70, "RFC 2046 boundary limit exceeded");

        let all_candidates = (0..MAX_MULTIPART_BOUNDARY_ATTEMPTS)
            .map(|attempt| boundary_candidate(&payload_digest, attempt))
            .collect::<Vec<_>>()
            .join("\n");
        let err = choose_boundary_from_digest(&all_candidates, &[], &payload_digest)
            .expect_err("exhausting the bounded candidate set must fail closed");
        assert!(matches!(err, BetaFeedbackHttpError::Validation(_)));
        assert!(err.to_string().contains("safe feedback request"));
    }

    fn spawn_one_shot_server(
        status_line: &'static str,
        response_headers: &'static str,
        response_body: &'static str,
    ) -> (String, thread::JoinHandle<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");
            let mut buf = Vec::new();
            let mut header_end = None;
            let mut content_length = 0usize;
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).expect("read socket");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if header_end.is_none()
                    && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
                {
                    let end = pos + 4;
                    header_end = Some(end);
                    let hdr_str = String::from_utf8_lossy(&buf[..end]);
                    for line in hdr_str.lines() {
                        if let Some((k, v)) = line.split_once(':')
                            && k.trim().eq_ignore_ascii_case("content-length")
                        {
                            content_length = v.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
                if let Some(end) = header_end
                    && buf.len() >= end + content_length
                {
                    break;
                }
            }
            let end = header_end.unwrap_or(buf.len());
            let headers = String::from_utf8_lossy(&buf[..end]).into_owned();
            let body = buf[end..].to_vec();

            let reply = format!(
                "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n{response_headers}\r\n{response_body}",
                response_body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
            (headers, body)
        });
        (format!("http://{addr}/feedback"), handle)
    }

    #[test]
    fn default_url_and_https_policy_are_enforced() {
        let (default_url, loopback) = validate_feedback_url(DEFAULT_BETA_FEEDBACK_URL).unwrap();
        assert_eq!(default_url.scheme(), "https");
        assert_eq!(default_url.host_str(), Some("formsubmit.co"));
        assert_eq!(default_url.path(), "/ajax/developer@redinger.cc");
        assert!(!loopback);

        // Remote plain HTTP is strictly rejected.
        assert!(matches!(
            validate_feedback_url("http://formsubmit.co/ajax/developer@redinger.cc"),
            Err(BetaFeedbackHttpError::InvalidUrl(_))
        ));
        // Credentials in URL are rejected.
        assert!(matches!(
            validate_feedback_url("https://user:pass@example.com/feedback"),
            Err(BetaFeedbackHttpError::InvalidUrl(_))
        ));
        // Loopback HTTP is accepted for local testing.
        let (_, is_loop) = validate_feedback_url("http://127.0.0.1:8123/feedback").unwrap();
        assert!(is_loop);
    }

    #[test]
    fn image_validation_checks_magic_bytes_and_size_limits() {
        assert_eq!(
            validate_feedback_attachment("shot.png", &sample_png())
                .unwrap()
                .content_type,
            "image/png"
        );
        assert_eq!(
            validate_feedback_attachment("photo.jpg", &sample_jpeg())
                .unwrap()
                .content_type,
            "image/jpeg"
        );
        assert_eq!(
            validate_feedback_attachment("anim.gif", &sample_gif())
                .unwrap()
                .content_type,
            "image/gif"
        );
        assert_eq!(
            validate_feedback_attachment("clip.webp", &sample_webp())
                .unwrap()
                .content_type,
            "image/webp"
        );

        // Spoofed extension with text bytes is rejected and names the file.
        let spoof = validate_feedback_attachment("spoofed.png", b"plain text not a png")
            .expect_err("must reject spoofed png");
        assert!(spoof.contains("spoofed.png"));
        assert!(spoof.contains("PNG, JPEG, GIF or WebP"));

        // Empty file is rejected.
        let empty = validate_feedback_attachment("empty.png", &[]).expect_err("must reject empty");
        assert!(empty.contains("empty.png"));

        // Oversized (> 10 MB) image is rejected and names the file.
        let mut huge = sample_png();
        huge.resize(MAX_FEEDBACK_ATTACHMENT_BYTES + 1, 0);
        let over =
            validate_feedback_attachment("huge-capture.png", &huge).expect_err("must reject >10MB");
        assert!(over.contains("huge-capture.png"));
        assert!(over.contains("10 MB"));
    }

    #[test]
    fn loopback_multipart_sends_only_message_and_images_without_hidden_telemetry() {
        let (url, handle) = spawn_one_shot_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"success":"true","message":"The form was submitted successfully."}"#,
        );
        let a1 = validate_feedback_attachment("Screenshot 1.png", &sample_png()).unwrap();
        let a2 = validate_feedback_attachment("second.webp", &sample_webp()).unwrap();

        send_beta_feedback_multipart(
            "Playhead jumps when pressing Enter inside a motion tween.",
            &[a1.clone(), a2.clone()],
            Some(&url),
        )
        .expect("multipart feedback send should succeed");

        let (headers, body) = handle.join().expect("join server");
        let lower_headers = headers.to_ascii_lowercase();
        assert!(lower_headers.contains("accept: application/json"));
        assert!(lower_headers.contains("origin: https://redinger.cc\r\n"));
        assert!(lower_headers.contains("referer: https://redinger.cc/\r\n"));
        assert!(lower_headers.contains("user-agent: neoflash-feedback/1\r\n"));
        // Privacy check (R-103 item 6): no OS, version, or document telemetry in headers or body.
        assert!(!lower_headers.contains("0.1.0-beta"));
        assert!(!lower_headers.contains("macos"));

        let body_lossy = String::from_utf8_lossy(&body);
        assert!(
            body_lossy.contains(
                "Content-Disposition: form-data; name=\"message\"\r\n\r\nPlayhead jumps when pressing Enter inside a motion tween.\r\n"
            ),
            "RFC 7578 FormData text part must omit Content-Type header: {body_lossy}"
        );
        assert!(
            !body_lossy.contains("Content-Type: text/plain"),
            "text fields must not carry Content-Type: text/plain"
        );
        assert!(
            !body_lossy.contains("name=\"_captcha\""),
            "generic loopback endpoint must not receive FormSubmit-only _captcha field"
        );
        assert!(body_lossy.contains("name=\"attachment_1\"; filename=\"Screenshot 1.png\""));
        assert!(body_lossy.contains("Content-Type: image/png"));
        assert!(body_lossy.contains("name=\"attachment_2\"; filename=\"second.webp\""));
        assert!(body_lossy.contains("Content-Type: image/webp"));
        assert!(!body_lossy.contains("_subject"));
        assert!(!body_lossy.contains("version"));
        assert!(!body_lossy.contains(".nflash"));
    }

    #[test]
    fn single_attachment_uses_singular_attachment_part_name() {
        let att = validate_feedback_attachment("only.jpg", &sample_jpeg()).unwrap();
        let (_, body) = build_feedback_multipart_body("One image attached", &[att]).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"attachment\"; filename=\"only.jpg\""));
        assert!(!text.contains("attachment_1"));
        assert!(!text.contains("Content-Type: text/plain"));
    }

    #[test]
    fn formsubmit_http_200_with_success_false_is_treated_as_error() {
        let (url, handle) = spawn_one_shot_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"success":"false","message":"This form needs Activation. We've sent you an email containing an 'Activate Form' link. Just click it and your form will be actived!"}"#,
        );
        let err = send_beta_feedback_multipart("First test report", &[], Some(&url))
            .expect_err("200 OK with success:false must return Err");
        let _ = handle.join();
        let rendered = err.to_string();
        assert_eq!(rendered, FORMSUBMIT_ACTIVATION_ERROR);
        assert!(
            !rendered.contains("actived"),
            "upstream FormSubmit typo must never surface: {rendered}"
        );
    }

    #[test]
    fn oversized_http_200_html_keeps_its_prefix_and_fails_closed() {
        let response_body: &'static str = Box::leak(
            format!(
                "<!DOCTYPE html><html><body><h1>Activate Form</h1><p>This form needs Activation.</p>{}</body></html>",
                "padding".repeat(RESPONSE_SNIPPET_BYTES)
            )
            .into_boxed_str(),
        );
        assert!(response_body.len() > RESPONSE_SNIPPET_BYTES);
        let (url, handle) = spawn_one_shot_server(
            "200 OK",
            "Content-Type: text/html; charset=utf-8\r\n",
            response_body,
        );

        let err = send_beta_feedback_multipart("First test report", &[], Some(&url))
            .expect_err("large HTML activation response must not be treated as success");
        let _ = handle.join();
        assert_eq!(err.to_string(), FORMSUBMIT_ACTIVATION_ERROR);
    }

    #[test]
    fn oversized_http_200_json_cannot_report_false_success() {
        let response_body: &'static str = Box::leak(
            format!(
                r#"{{"success":"true","message":"{}"}}"#,
                "padding".repeat(RESPONSE_SNIPPET_BYTES)
            )
            .into_boxed_str(),
        );
        assert!(response_body.len() > RESPONSE_SNIPPET_BYTES);
        let (url, handle) = spawn_one_shot_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            response_body,
        );

        let err = send_beta_feedback_multipart("First test report", &[], Some(&url))
            .expect_err("truncated JSON must not be treated as a successful submission");
        let _ = handle.join();
        assert!(matches!(
            &err,
            BetaFeedbackHttpError::ServerRejected {
                status: Some(200),
                ..
            }
        ));
        assert!(err.to_string().contains("could not be verified"));
    }

    #[test]
    fn malformed_json_http_200_cannot_report_false_success() {
        for malformed in [r#"{"success":false}garbage"#, r#"[{"success":true}"#] {
            let err = interpret_feedback_http_response(200, malformed)
                .expect_err("JSON-looking malformed responses must fail closed");
            assert!(matches!(
                &err,
                BetaFeedbackHttpError::ServerRejected {
                    status: Some(200),
                    ..
                }
            ));
            assert!(err.to_string().contains("malformed JSON"));
        }

        assert!(
            interpret_feedback_http_response(200, r#"["accepted"]"#).is_ok(),
            "a complete valid JSON response must not be rejected as malformed"
        );
    }

    #[test]
    fn formsubmit_only_captcha_flag_and_rfc7578_text_parts_are_enforced() {
        let (fs_root, _) =
            validate_feedback_url("https://formsubmit.co/ajax/developer@redinger.cc").unwrap();
        let (fs_sub, _) =
            validate_feedback_url("https://api.formsubmit.co/ajax/developer@redinger.cc").unwrap();
        let (other_https, _) =
            validate_feedback_url("https://feedback.example.com/submit").unwrap();
        let (loopback, _) = validate_feedback_url("http://127.0.0.1:9000/feedback").unwrap();

        assert!(is_formsubmit_endpoint(&fs_root));
        assert!(is_formsubmit_endpoint(&fs_sub));
        assert!(!is_formsubmit_endpoint(&other_https));
        assert!(!is_formsubmit_endpoint(&loopback));

        for url in [&fs_root, &fs_sub] {
            let (_, body) =
                build_feedback_multipart_body_for_url("Hello FormSubmit", &[], Some(url)).unwrap();
            let s = String::from_utf8_lossy(&body);
            assert!(
                s.contains(
                    "Content-Disposition: form-data; name=\"message\"\r\n\r\nHello FormSubmit\r\n"
                ),
                "message part must omit Content-Type: {s}"
            );
            assert!(
                s.contains("Content-Disposition: form-data; name=\"_captcha\"\r\n\r\nfalse\r\n"),
                "FormSubmit target must include _captcha=false without Content-Type: {s}"
            );
            assert!(!s.contains("Content-Type: text/plain"));
        }

        for url in [&other_https, &loopback] {
            let (_, body) =
                build_feedback_multipart_body_for_url("Hello Custom", &[], Some(url)).unwrap();
            let s = String::from_utf8_lossy(&body);
            assert!(!s.contains("_captcha"));
            assert!(!s.contains("Content-Type: text/plain"));
        }
    }

    #[test]
    fn combined_ten_megabyte_attachment_limit_is_enforced_across_multiple_files() {
        let make_sized_png = |name: &str, len: usize| -> FeedbackAttachment {
            let mut bytes = sample_png();
            bytes.resize(len, 0);
            validate_feedback_attachment(name, &bytes).expect("single attachment <= 10 MB")
        };

        // Two 5 MB images equal exactly 10 MB combined -> accepted.
        let five_mb = 5 * 1024 * 1024;
        let exact_a = make_sized_png("part-a.png", five_mb);
        let exact_b = make_sized_png("part-b.png", five_mb);
        assert!(build_feedback_multipart_body("Two 5 MB captures", &[exact_a, exact_b]).is_ok());

        // Three 4 MB images (each <= 10 MB individually, 12 MB combined) -> rejected naming 3rd file and 10 MB limit.
        let four_mb = 4 * 1024 * 1024;
        let c1 = make_sized_png("one.png", four_mb);
        let c2 = make_sized_png("two.png", four_mb);
        let c3 = make_sized_png("three-over.png", four_mb);
        let err = build_feedback_multipart_body("Three 4 MB captures", &[c1, c2, c3])
            .expect_err("12 MB combined attachments must fail 10 MB total cap");
        assert!(matches!(err, BetaFeedbackHttpError::Validation(_)));
        let msg = err.to_string();
        assert!(msg.contains("three-over.png"), "got: {msg}");
        assert!(msg.contains("10 MB"), "got: {msg}");
    }

    #[test]
    fn bare_formsubmit_token_and_non_ajax_urls_normalize_cleanly() {
        // 32-char bare FormSubmit token -> https://formsubmit.co/ajax/<token>
        let (tok_url, tok_loop) =
            validate_feedback_url("a8f5f167f44f4964e6c998dee827110c").unwrap();
        assert!(!tok_loop);
        assert_eq!(
            tok_url.as_str(),
            "https://formsubmit.co/ajax/a8f5f167f44f4964e6c998dee827110c"
        );

        // Non-ajax https://formsubmit.co/<target> -> https://formsubmit.co/ajax/<target>
        let (non_ajax, _) =
            validate_feedback_url("https://formsubmit.co/developer@redinger.cc").unwrap();
        assert_eq!(
            non_ajax.as_str(),
            "https://formsubmit.co/ajax/developer@redinger.cc"
        );

        let (sub_non_ajax, _) =
            validate_feedback_url("https://www.formsubmit.co/a8f5f167f44f4964e6c998dee827110c")
                .unwrap();
        assert_eq!(
            sub_non_ajax.as_str(),
            "https://www.formsubmit.co/ajax/a8f5f167f44f4964e6c998dee827110c"
        );

        // Already-/ajax/ URLs and non-FormSubmit https:// URLs stay untouched.
        let (already_ajax, _) =
            validate_feedback_url("https://formsubmit.co/ajax/developer@redinger.cc").unwrap();
        assert_eq!(
            already_ajax.as_str(),
            "https://formsubmit.co/ajax/developer@redinger.cc"
        );
        let (custom, _) = validate_feedback_url("https://collector.example.org/v1/report").unwrap();
        assert_eq!(custom.as_str(), "https://collector.example.org/v1/report");

        // Plain HTTP FormSubmit URLs and short garbage strings still fail closed.
        assert!(matches!(
            validate_feedback_url("http://formsubmit.co/developer@redinger.cc"),
            Err(BetaFeedbackHttpError::InvalidUrl(_))
        ));
        assert!(matches!(
            validate_feedback_url("https://formsubmit.co/"),
            Err(BetaFeedbackHttpError::InvalidUrl(_))
        ));
        assert!(matches!(
            validate_feedback_url("not-a-url"),
            Err(BetaFeedbackHttpError::InvalidUrl(_))
        ));
    }

    #[test]
    fn http_413_and_upstream_html_responses_are_sanitized_without_raw_tags() {
        let (url_413, h_413) = spawn_one_shot_server(
            "413 Payload Too Large",
            "Content-Type: text/html\r\n",
            "<!DOCTYPE html><html><head><title>413 Request Entity Too Large</title></head><body><h1>413 Request Entity Too Large</h1></body></html>",
        );
        let err_413 = send_beta_feedback_multipart("Large bundle", &[], Some(&url_413))
            .expect_err("HTTP 413 must fail");
        let _ = h_413.join();
        let s_413 = err_413.to_string();
        assert!(s_413.contains("413"), "got: {s_413}");
        assert!(s_413.contains("10 MB"), "got: {s_413}");
        assert!(
            !s_413.contains('<') && !s_413.contains("DOCTYPE"),
            "got: {s_413}"
        );

        // Upstream HTML proxy/WAF error on 502 strips script/style/tags cleanly.
        let (url_html, h_html) = spawn_one_shot_server(
            "502 Bad Gateway",
            "Content-Type: text/html; charset=utf-8\r\n",
            "<!DOCTYPE html><html><head><style>body{color:red}</style><script>alert(1)</script></head><body><h1>502 Bad Gateway</h1><p>Upstream gateway &amp; origin unreachable.</p></body></html>",
        );
        let err_html = send_beta_feedback_multipart("Report", &[], Some(&url_html))
            .expect_err("HTTP 502 HTML must fail");
        let _ = h_html.join();
        let s_html = err_html.to_string();
        assert!(s_html.contains("502 Bad Gateway"), "got: {s_html}");
        assert!(
            s_html.contains("Upstream gateway & origin unreachable."),
            "got: {s_html}"
        );
        assert!(
            !s_html.contains("<!DOCTYPE")
                && !s_html.contains("<html")
                && !s_html.contains("alert(1)")
                && !s_html.contains("color:red"),
            "HTML tags/scripts/styles must be stripped: {s_html}"
        );

        // HTTP 200 returning an HTML activation or CAPTCHA wall instead of JSON fails closed cleanly.
        let html_activation = interpret_feedback_http_response(
            200,
            "<!DOCTYPE html><html><body><h1>Activate Form</h1><p>This form needs Activation before submissions work.</p></body></html>",
        )
        .expect_err("HTML activation wall on 200 OK must not be treated as success");
        assert_eq!(html_activation.to_string(), FORMSUBMIT_ACTIVATION_ERROR);

        let html_captcha = interpret_feedback_http_response(
            200,
            "<!DOCTYPE html><html><body><div class=\"g-recaptcha\">Please complete the captcha</div></body></html>",
        )
        .expect_err("HTML reCAPTCHA wall on 200 OK must not be treated as success");
        assert!(html_captcha.to_string().contains("CAPTCHA"));
        assert!(!html_captcha.to_string().contains("<div"));

        // Double-encoded entities (`&amp;lt;script&amp;gt;`) decode `&amp;` last and never become `<script>`.
        let stripped_double = strip_html_markup(
            "<p>Blocked payload: &amp;lt;script&amp;gt; &amp;quot;x&amp;quot;</p>",
        );
        assert_eq!(
            stripped_double,
            "Blocked payload: &lt;script&gt; &quot;x&quot;"
        );
        assert!(!stripped_double.contains("<script>"));
    }

    #[test]
    fn redirects_and_http_500_fail_closed() {
        let (redirect_url, redirect_handle) =
            spawn_one_shot_server("302 Found", "Location: http://example.com/steal\r\n", "");
        let err = send_beta_feedback_multipart("Report", &[], Some(&redirect_url))
            .expect_err("redirect must not be followed");
        let _ = redirect_handle.join();
        assert!(err.to_string().contains("redirect"));

        let (err_url, err_handle) = spawn_one_shot_server(
            "500 Internal Server Error",
            "Content-Type: application/json\r\n",
            r#"{"message":"Upstream mail relay unavailable"}"#,
        );
        let err500 = send_beta_feedback_multipart("Report", &[], Some(&err_url))
            .expect_err("500 must return Err");
        let _ = err_handle.join();
        assert!(err500.to_string().contains("500"));
        assert!(
            err500
                .to_string()
                .contains("Upstream mail relay unavailable")
        );
    }

    #[test]
    fn message_size_cap_and_loopback_dns_ip_checks_are_enforced() {
        // Exact 64 KiB message is accepted.
        let exact_max = "x".repeat(MAX_FEEDBACK_MESSAGE_BYTES);
        assert!(build_feedback_multipart_body(&exact_max, &[]).is_ok());

        // 64 KiB + 1 byte is rejected before multipart assembly.
        let over_max = "x".repeat(MAX_FEEDBACK_MESSAGE_BYTES + 1);
        let err = build_feedback_multipart_body(&over_max, &[])
            .expect_err("message exceeding 64 KiB must fail validation");
        assert!(matches!(err, BetaFeedbackHttpError::Validation(_)));
        assert!(err.to_string().contains("64 KB"));

        // Loopback DNS verification rejects empty or non-loopback resolved IPs.
        assert!(validate_feedback_loopback_ips("localhost", &[]).is_err());
        assert!(
            validate_feedback_loopback_ips(
                "localhost",
                &[
                    "127.0.0.1".parse().unwrap(),
                    "192.168.1.44".parse().unwrap()
                ],
            )
            .is_err()
        );
        assert!(
            validate_feedback_loopback_ips(
                "localhost",
                &["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
            )
            .is_ok()
        );
        // Verified `http://localhost` resolves via `to_socket_addrs()` on loopback.
        let (_, is_loop) = validate_feedback_url("http://localhost:8123/feedback").unwrap();
        assert!(is_loop);
    }
}
