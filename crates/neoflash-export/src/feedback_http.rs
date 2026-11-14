//! HTTPS `multipart/form-data` sender and attachment validator for Beta feedback (`R-103`, `T-317`).
//!
//! Privacy rule (`R-103` item 6): only the fields and images visible in the `"Beta feedback"`
//! dialog are transmitted — the `message` text and up to five validated image attachments
//! (`PNG`, `JPEG`, `GIF`, `WebP`, ≤ 10 MB each). No build version, OS, machine identifier, or
//! open `.nflash` document metadata is attached to headers or form fields.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use std::{fmt, time::Duration};

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

const RESPONSE_SNIPPET_BYTES: usize = 16 * 1024;

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
    let lower = host.to_ascii_lowercase();
    lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "[::1]"
}

/// Validate a feedback endpoint URL.
///
/// Remote hosts MUST use `https://`. Plain `http://` is permitted strictly on loopback
/// (`127.0.0.1`, `localhost`, `[::1]`) for local development and deterministic loopback tests.
pub fn validate_feedback_url(raw: &str) -> Result<(url::Url, bool), BetaFeedbackHttpError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "endpoint URL is empty".into(),
        ));
    }
    let parsed =
        url::Url::parse(trimmed).map_err(|e| BetaFeedbackHttpError::InvalidUrl(e.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "URL must not contain credentials".into(),
        ));
    }
    let Some(host) = parsed.host_str() else {
        return Err(BetaFeedbackHttpError::InvalidUrl(
            "URL is missing a host".into(),
        ));
    };
    let loopback = is_loopback_host(host);
    match parsed.scheme() {
        "https" => Ok((parsed, loopback)),
        "http" if loopback => Ok((parsed, true)),
        "http" => Err(BetaFeedbackHttpError::InvalidUrl(format!(
            "remote feedback endpoint '{host}' must use https://"
        ))),
        other => Err(BetaFeedbackHttpError::InvalidUrl(format!(
            "unsupported URL scheme '{other}'"
        ))),
    }
}

/// Resolve the active feedback endpoint URL from an explicit override, [`BETA_FEEDBACK_URL_ENV`],
/// or [`DEFAULT_BETA_FEEDBACK_URL`].
pub fn resolve_feedback_url(
    explicit_override: Option<&str>,
) -> Result<(url::Url, bool), BetaFeedbackHttpError> {
    if let Some(raw) = explicit_override {
        return validate_feedback_url(raw);
    }
    if let Ok(env_val) = std::env::var(BETA_FEEDBACK_URL_ENV) {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return validate_feedback_url(trimmed);
        }
    }
    validate_feedback_url(DEFAULT_BETA_FEEDBACK_URL)
}

fn choose_boundary(message: &str, attachments: &[FeedbackAttachment]) -> String {
    let mut counter: u64 = 1;
    loop {
        let candidate = format!("----NeoFlashFeedbackBoundary{counter:016x}");
        let needle = candidate.as_bytes();
        let collides_msg = message
            .as_bytes()
            .windows(needle.len())
            .any(|w| w == needle);
        let collides_att = attachments.iter().any(|att| {
            att.filename
                .as_bytes()
                .windows(needle.len())
                .any(|w| w == needle)
                || att.bytes.windows(needle.len()).any(|w| w == needle)
        });
        if !collides_msg && !collides_att {
            return candidate;
        }
        counter = counter.wrapping_add(1);
    }
}

/// Build an RFC 7578 `multipart/form-data` payload containing **only** the `message` text and
/// validated image attachments (`R-103` item 6).
///
/// Field naming for FormSubmit compatibility:
/// - `message` (`text/plain; charset=utf-8`)
/// - `attachment` when 1 image is attached; `attachment_1`..`attachment_N` when 2..=5 images are attached.
pub fn build_feedback_multipart_body(
    message: &str,
    attachments: &[FeedbackAttachment],
) -> Result<(String, Vec<u8>), BetaFeedbackHttpError> {
    if message.trim().is_empty() {
        return Err(BetaFeedbackHttpError::Validation(
            "Feedback text cannot be empty.".into(),
        ));
    }
    if attachments.len() > MAX_FEEDBACK_ATTACHMENTS {
        return Err(BetaFeedbackHttpError::Validation(
            "At most 5 images can be attached.".into(),
        ));
    }
    for att in attachments {
        if att.bytes.is_empty() {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{}” could not be attached: file is empty.",
                att.filename
            )));
        }
        if att.bytes.len() > MAX_FEEDBACK_ATTACHMENT_BYTES {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{}” exceeds the 10 MB limit ({}).",
                att.filename,
                format_attachment_size(att.bytes.len())
            )));
        }
        let Some(detected_mime) = detect_feedback_image_mime(&att.bytes) else {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{}” is not a supported image (PNG, JPEG, GIF or WebP required).",
                att.filename
            )));
        };
        if att.content_type != detected_mime {
            return Err(BetaFeedbackHttpError::Validation(format!(
                "“{}” MIME type mismatch (expected {detected_mime}, got {}).",
                att.filename, att.content_type
            )));
        }
    }

    let boundary = choose_boundary(message, attachments);
    let total_bytes: usize = 256
        + message.len()
        + attachments
            .iter()
            .map(|a| 256 + a.filename.len() + a.bytes.len())
            .sum::<usize>();
    let mut body = Vec::with_capacity(total_bytes);

    // 1. User feedback text (`message`).
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(
        b"\r\nContent-Disposition: form-data; name=\"message\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n",
    );
    body.extend_from_slice(message.as_bytes());
    body.extend_from_slice(b"\r\n");

    // 2. Attached images (`attachment` for single file, `attachment_1..N` for multiple files).
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
                att.filename
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

fn read_body_snippet(body: &mut ureq::Body) -> String {
    match body
        .with_config()
        .limit(RESPONSE_SNIPPET_BYTES as u64)
        .read_to_string()
    {
        Ok(s) => s.trim().to_owned(),
        Err(_) => String::new(),
    }
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
    let (boundary, body) = build_feedback_multipart_body(message, attachments)?;
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
    let raw_body = read_body_snippet(response.body_mut());
    let parsed_json = if raw_body.starts_with('{') {
        serde_json::from_str::<Value>(&raw_body).ok()
    } else {
        None
    };

    if (300..400).contains(&status) {
        return Err(BetaFeedbackHttpError::ServerRejected {
            status: Some(status),
            detail: "server returned an unexpected redirect".into(),
        });
    }

    if (200..300).contains(&status) {
        if let Some(json) = &parsed_json
            && is_json_logical_failure(json)
        {
            let detail = extract_json_message(json)
                .unwrap_or_else(|| "server rejected the feedback submission".into());
            return Err(BetaFeedbackHttpError::ServerRejected {
                status: None,
                detail,
            });
        }
        return Ok(());
    }

    let detail = parsed_json
        .as_ref()
        .and_then(extract_json_message)
        .unwrap_or_else(|| raw_body.chars().take(240).collect());
    Err(BetaFeedbackHttpError::ServerRejected {
        status: Some(status),
        detail,
    })
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
        assert!(body_lossy.contains("name=\"message\""));
        assert!(body_lossy.contains("Playhead jumps when pressing Enter inside a motion tween."));
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
    }

    #[test]
    fn formsubmit_http_200_with_success_false_is_treated_as_error() {
        let (url, handle) = spawn_one_shot_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"success":"false","message":"This form needs Activation. We've sent you an email containing an 'Activate Form' link."}"#,
        );
        let err = send_beta_feedback_multipart("First test report", &[], Some(&url))
            .expect_err("200 OK with success:false must return Err");
        let _ = handle.join();
        let rendered = err.to_string();
        assert!(
            rendered.contains("This form needs Activation"),
            "unexpected error: {rendered}"
        );
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
}
