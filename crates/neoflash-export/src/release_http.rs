//! Fail-closed HTTP(S) client helpers for GitHub Releases metadata and checksum-verified downloads.

use neoflash_core::sha256::Sha256;
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_REDIRECTS: usize = 6;
const CHUNK_SIZE: usize = 32 * 1024;
const ERROR_SNIPPET_BYTES: usize = 16 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Errors returned while fetching release metadata, checksum manifests, or installer archives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseHttpError {
    InvalidUrl(String),
    OfflineOrNetwork(String),
    RateLimited {
        retry_after_secs: Option<u64>,
        detail: String,
    },
    HttpStatus {
        status: u16,
        detail: String,
    },
    SizeExceeded(String),
    Cancelled,
    Sha256Mismatch {
        expected: String,
        actual: String,
    },
    Io(String),
}

impl fmt::Display for ReleaseHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "Invalid release URL: {msg}"),
            Self::OfflineOrNetwork(msg) => write!(f, "Network error: {msg}"),
            Self::RateLimited {
                retry_after_secs: Some(secs),
                ..
            } => write!(
                f,
                "GitHub rate limit reached; retry in {secs} s or open Release notes."
            ),
            Self::RateLimited {
                retry_after_secs: None,
                detail,
            } => {
                if detail.is_empty() {
                    write!(f, "GitHub rate limit reached; try again later.")
                } else {
                    write!(f, "{detail}")
                }
            }
            Self::HttpStatus { status, detail } => {
                if detail.is_empty() {
                    write!(f, "Server returned HTTP {status}.")
                } else {
                    write!(f, "Server returned HTTP {status}: {detail}")
                }
            }
            Self::SizeExceeded(msg) | Self::Io(msg) => f.write_str(msg),
            Self::Cancelled => f.write_str("Download cancelled."),
            Self::Sha256Mismatch { expected, actual } => write!(
                f,
                "SHA-256 verification failed (expected {expected}, got {actual}); partial file removed."
            ),
        }
    }
}

impl std::error::Error for ReleaseHttpError {}

/// Validate a URL for GitHub Releases metadata, release-notes browser opening, or asset download.
///
/// In production (`allowed_loopback_base == None`), only HTTPS URLs on `api.github.com`,
/// `github.com`, and `*.githubusercontent.com` are accepted. When `allowed_loopback_base` is
/// supplied by isolated local-fixture tests, only URLs matching that loopback HTTP origin are
/// permitted.
pub fn validate_release_url(
    raw: &str,
    allowed_loopback_base: Option<&str>,
) -> Result<url::Url, ReleaseHttpError> {
    let parsed =
        url::Url::parse(raw.trim()).map_err(|e| ReleaseHttpError::InvalidUrl(e.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ReleaseHttpError::InvalidUrl(
            "URL must not contain credentials".into(),
        ));
    }

    if let Some(base_raw) = allowed_loopback_base {
        let base = url::Url::parse(base_raw.trim())
            .map_err(|e| ReleaseHttpError::InvalidUrl(format!("invalid fixture base: {e}")))?;
        if base.scheme() != "http" {
            return Err(ReleaseHttpError::InvalidUrl(
                "fixture override must use http:// on loopback".into(),
            ));
        }
        let Some(base_host) = base.host_str() else {
            return Err(ReleaseHttpError::InvalidUrl(
                "fixture override missing host".into(),
            ));
        };
        if !is_loopback_host(base_host) {
            return Err(ReleaseHttpError::InvalidUrl(
                "fixture override host must be loopback (127.0.0.1, localhost, or [::1])".into(),
            ));
        }
        let matches_origin = parsed.scheme() == base.scheme()
            && parsed.host_str() == Some(base_host)
            && parsed.port_or_known_default() == base.port_or_known_default();
        if !matches_origin {
            return Err(ReleaseHttpError::InvalidUrl(format!(
                "URL origin '{}' does not match fixture base '{base_raw}'",
                redacted_url(&parsed)
            )));
        }
        return Ok(parsed);
    }

    if parsed.scheme() != "https" {
        return Err(ReleaseHttpError::InvalidUrl(format!(
            "only https:// URLs are allowed (got '{}')",
            parsed.scheme()
        )));
    }
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return Err(ReleaseHttpError::InvalidUrl("URL is missing a host".into()));
    };
    let host = host.trim_end_matches('.');
    let allowed = host == "api.github.com"
        || host == "github.com"
        || host.ends_with(".githubusercontent.com");
    if !allowed {
        return Err(ReleaseHttpError::InvalidUrl(format!(
            "host '{host}' is not an allowlisted GitHub domain"
        )));
    }
    Ok(parsed)
}

fn is_loopback_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "[::1]"
}

fn redacted_url(url: &url::Url) -> String {
    let mut copy = url.clone();
    copy.set_query(None);
    copy.set_fragment(None);
    copy.into()
}

fn build_agent(
    recv_body_timeout: Duration,
    global_timeout: Option<Duration>,
    loopback_mode: bool,
) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_response_header_size(32 * 1024)
        .timeout_resolve(Some(Duration::from_secs(5)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(recv_body_timeout))
        .timeout_global(global_timeout);
    if loopback_mode {
        builder = builder.proxy(None);
    }
    ureq::Agent::new_with_config(builder.build())
}

fn map_ureq_error(err: ureq::Error) -> ReleaseHttpError {
    match err {
        ureq::Error::BodyExceedsLimit(limit) => {
            ReleaseHttpError::SizeExceeded(format!("Response exceeds {limit}-byte safety limit."))
        }
        ureq::Error::Timeout(kind) => {
            ReleaseHttpError::OfflineOrNetwork(format!("Request timed out ({kind:?})."))
        }
        other => ReleaseHttpError::OfflineOrNetwork(other.to_string()),
    }
}

fn parse_retry_after(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    if let Some(val) = response
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(val);
    }
    if let Some(reset_unix) = response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if reset_unix >= now_unix {
            return Some(reset_unix - now_unix);
        }
    }
    None
}

fn read_error_snippet(body: &mut ureq::Body) -> String {
    match body
        .with_config()
        .limit(ERROR_SNIPPET_BYTES as u64)
        .read_to_string()
    {
        Ok(s) => s.trim().chars().take(240).collect(),
        Err(_) => String::new(),
    }
}

fn send_with_redirects(
    agent: &ureq::Agent,
    initial_url: &str,
    accept: &str,
    user_agent: &str,
    allowed_loopback_base: Option<&str>,
    cancel: Option<&AtomicBool>,
) -> Result<ureq::http::Response<ureq::Body>, ReleaseHttpError> {
    let mut current = validate_release_url(initial_url, allowed_loopback_base)?;
    for hop in 0..=MAX_REDIRECTS {
        if cancel.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(ReleaseHttpError::Cancelled);
        }
        let mut response = agent
            .get(current.as_str())
            .header("Accept", accept)
            .header("User-Agent", user_agent)
            .call()
            .map_err(map_ureq_error)?;

        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hop >= MAX_REDIRECTS {
                return Err(ReleaseHttpError::OfflineOrNetwork(format!(
                    "Exceeded maximum redirect limit ({MAX_REDIRECTS})."
                )));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
                .ok_or_else(|| ReleaseHttpError::HttpStatus {
                    status,
                    detail: "Redirect response missing valid Location header.".into(),
                })?;
            let next = current
                .join(location)
                .map_err(|e| ReleaseHttpError::InvalidUrl(format!("Invalid redirect: {e}")))?;
            current = validate_release_url(next.as_str(), allowed_loopback_base)?;
            continue;
        }

        if status == 429 || status == 403 {
            let remaining_zero = response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|h| h.to_str().ok())
                .is_some_and(|v| v.trim() == "0");
            let has_retry_after = response.headers().contains_key("retry-after");
            let retry_after_secs = parse_retry_after(&response);
            let snippet = read_error_snippet(response.body_mut());
            let body_rate_limit = snippet.to_ascii_lowercase().contains("rate limit");
            if status == 429 || remaining_zero || has_retry_after || body_rate_limit {
                return Err(ReleaseHttpError::RateLimited {
                    retry_after_secs,
                    detail: if snippet.is_empty() {
                        format!("GitHub API returned HTTP {status} rate limit.")
                    } else {
                        format!("GitHub rate limit ({status}): {snippet}")
                    },
                });
            }
            return Err(ReleaseHttpError::HttpStatus {
                status,
                detail: snippet,
            });
        }

        if !response.status().is_success() {
            let snippet = read_error_snippet(response.body_mut());
            return Err(ReleaseHttpError::HttpStatus {
                status,
                detail: snippet,
            });
        }

        return Ok(response);
    }
    Err(ReleaseHttpError::OfflineOrNetwork(
        "Redirect loop exited unexpectedly.".into(),
    ))
}

/// Fetch UTF-8 text (e.g. GitHub Releases API JSON or `SHA256SUMS.txt`) with strict URL/size limits.
pub fn fetch_text_with_policy(
    url: &str,
    accept: &str,
    user_agent: &str,
    max_bytes: usize,
    timeout: Duration,
    allowed_loopback_base: Option<&str>,
) -> Result<String, ReleaseHttpError> {
    let agent = build_agent(timeout, Some(timeout), allowed_loopback_base.is_some());
    let mut response =
        send_with_redirects(&agent, url, accept, user_agent, allowed_loopback_base, None)?;

    if let Some(content_length) = response
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        && content_length > max_bytes as u64
    {
        return Err(ReleaseHttpError::SizeExceeded(format!(
            "Response declares {content_length} bytes, exceeding {max_bytes}-byte limit."
        )));
    }

    let text = response
        .body_mut()
        .with_config()
        .limit(max_bytes as u64)
        .read_to_string()
        .map_err(map_ureq_error)?;
    Ok(text)
}

/// Parameters for a streaming, SHA-256 verified archive download.
pub struct DownloadRequest<'a> {
    pub url: &'a str,
    pub dest_path: &'a Path,
    pub expected_sha256: &'a str,
    pub user_agent: &'a str,
    pub max_bytes: u64,
    pub allowed_loopback_base: Option<&'a str>,
    pub cancel: &'a AtomicBool,
}

struct TempFileGuard {
    path: Option<PathBuf>,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

fn is_valid_sha256_hex(raw: &str) -> bool {
    raw.len() == 64 && raw.bytes().all(|b| b.is_ascii_hexdigit())
}

fn constant_time_eq_ascii_lower(expected_hex_lower: &str, actual_hex_lower: &str) -> bool {
    let a = expected_hex_lower.as_bytes();
    let b = actual_hex_lower.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn create_sibling_temp_file(dest: &Path) -> Result<(PathBuf, File), ReleaseHttpError> {
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            ReleaseHttpError::Io("Destination path must have a valid parent directory.".into())
        })?;
    fs::create_dir_all(parent)
        .map_err(|e| ReleaseHttpError::Io(format!("Failed to create destination folder: {e}")))?;

    for _ in 0..32 {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".neoflash-update-{}-{seq}.part",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(ReleaseHttpError::Io(format!(
                    "Failed to create temporary download file: {e}"
                )));
            }
        }
    }
    Err(ReleaseHttpError::Io(
        "Could not allocate a unique temporary download file.".into(),
    ))
}

/// Stream a release asset to `req.dest_path`, verifying its SHA-256 digest before renaming.
///
/// Any error, cancellation, or hash mismatch leaves zero files at `req.dest_path` and removes
/// the temporary `.part` file.
pub fn download_verified_file(
    req: &DownloadRequest<'_>,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), ReleaseHttpError> {
    let expected_lower = req.expected_sha256.trim().to_ascii_lowercase();
    if !is_valid_sha256_hex(&expected_lower) {
        return Err(ReleaseHttpError::Io(
            "Release checksum manifest contains an invalid SHA-256 digest.".into(),
        ));
    }
    if req.cancel.load(Ordering::Acquire) {
        return Err(ReleaseHttpError::Cancelled);
    }

    let agent = build_agent(
        Duration::from_secs(30),
        None,
        req.allowed_loopback_base.is_some(),
    );
    let mut response = send_with_redirects(
        &agent,
        req.url,
        "application/octet-stream",
        req.user_agent,
        req.allowed_loopback_base,
        Some(req.cancel),
    )?;

    let total_bytes = response
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());
    if let Some(len) = total_bytes
        && len > req.max_bytes
    {
        return Err(ReleaseHttpError::SizeExceeded(format!(
            "Release asset is {len} bytes, exceeding the {}-byte download limit.",
            req.max_bytes
        )));
    }

    let (temp_path, file) = create_sibling_temp_file(req.dest_path)?;
    let mut guard = TempFileGuard::new(temp_path.clone());
    let mut writer = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    on_progress(0, total_bytes);

    let mut reader = response.body_mut().as_reader();
    let mut buf = [0u8; CHUNK_SIZE];
    loop {
        if req.cancel.load(Ordering::Acquire) {
            return Err(ReleaseHttpError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| {
            ReleaseHttpError::OfflineOrNetwork(format!("Download interrupted while reading: {e}"))
        })?;
        if n == 0 {
            break;
        }
        received = received.saturating_add(n as u64);
        if received > req.max_bytes {
            return Err(ReleaseHttpError::SizeExceeded(format!(
                "Downloaded payload exceeded the {}-byte safety limit.",
                req.max_bytes
            )));
        }
        let chunk = &buf[..n];
        writer
            .write_all(chunk)
            .map_err(|e| ReleaseHttpError::Io(format!("Failed writing download to disk: {e}")))?;
        hasher.update(chunk);
        on_progress(received, total_bytes);
    }

    if req.cancel.load(Ordering::Acquire) {
        return Err(ReleaseHttpError::Cancelled);
    }
    if received == 0 {
        return Err(ReleaseHttpError::OfflineOrNetwork(
            "Server returned an empty release asset.".into(),
        ));
    }

    writer
        .flush()
        .map_err(|e| ReleaseHttpError::Io(format!("Failed flushing download to disk: {e}")))?;
    let file = writer
        .into_inner()
        .map_err(|e| ReleaseHttpError::Io(format!("Failed finalizing buffered writer: {e}")))?;
    file.sync_all()
        .map_err(|e| ReleaseHttpError::Io(format!("Failed syncing download to disk: {e}")))?;
    drop(file);

    let actual_lower = hasher.finalize_hex();
    if !constant_time_eq_ascii_lower(&expected_lower, &actual_lower) {
        return Err(ReleaseHttpError::Sha256Mismatch {
            expected: expected_lower,
            actual: actual_lower,
        });
    }

    fs::rename(&temp_path, req.dest_path)
        .map_err(|e| ReleaseHttpError::Io(format!("Failed saving verified download: {e}")))?;
    guard.disarm();
    Ok(())
}
