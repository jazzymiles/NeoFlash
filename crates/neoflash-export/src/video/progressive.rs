use std::{
    collections::{HashMap, HashSet},
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::ExportError;
use neoflash_core::{
    DecodedImage, ElementKind, LayerRole, Project, ResolvedVideoPlacement, VideoSyncMode,
    layer_effectively_visible,
    timeline_video::{
        VideoDeploymentMode, VideoSourceSpec, resolve_video_resource_source, resolve_video_source,
        validate_external_video_path,
    },
};

use super::ffprobe_ffmpeg::decode_external_video_file;
use super::*;

#[derive(Clone)]
struct StagedProgressiveBytes {
    bytes: Arc<[u8]>,
    inserted_at: Instant,
}

#[derive(Default)]
struct StagedProgressiveByteCache {
    /// Explicitly staged bytes retain the pre-T-286 process-local behavior. Callers use this for
    /// deterministic tests and for bytes whose lifetime they already own.
    explicit: HashMap<String, StagedProgressiveBytes>,
    /// Network responses are short-lived and separately byte-bounded so a remote URL cannot pin
    /// sixteen 64 MiB responses for the entire editor process.
    downloaded: HashMap<String, StagedProgressiveBytes>,
}

/// Bounded on-demand byte cache for `VideoDeploymentMode::Progressive` sources (`T-274`).
fn staged_progressive_byte_cache() -> &'static Mutex<StagedProgressiveByteCache> {
    static CACHE: OnceLock<Mutex<StagedProgressiveByteCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(StagedProgressiveByteCache::default()))
}

pub(super) fn redacted_progressive_url(raw: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(raw) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string();
    }
    let safe = raw.split(['?', '#']).next().unwrap_or("<invalid URL>");
    safe.chars().take(256).collect()
}

fn private_progressive_networks_allowed() -> bool {
    env::var("NEOFLASH_ALLOW_PRIVATE_MEDIA_URLS").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn ipv4_in_prefix(address: Ipv4Addr, base: Ipv4Addr, prefix_bits: u32) -> bool {
    let mask = if prefix_bits == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_bits)
    };
    (u32::from(address) & mask) == (u32::from(base) & mask)
}

fn ipv6_in_prefix(address: Ipv6Addr, base: Ipv6Addr, prefix_bits: u32) -> bool {
    let mask = if prefix_bits == 0 {
        0
    } else {
        u128::MAX << (128 - prefix_bits)
    };
    (u128::from(address) & mask) == (u128::from(base) & mask)
}

pub(super) fn is_public_progressive_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !(ipv4_in_prefix(address, Ipv4Addr::new(0, 0, 0, 0), 8)
                || address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_multicast()
                || address.is_broadcast()
                || address.is_documentation()
                || ipv4_in_prefix(address, Ipv4Addr::new(100, 64, 0, 0), 10)
                || ipv4_in_prefix(address, Ipv4Addr::new(192, 0, 0, 0), 24)
                || ipv4_in_prefix(address, Ipv4Addr::new(192, 88, 99, 0), 24)
                || ipv4_in_prefix(address, Ipv4Addr::new(198, 18, 0, 0), 15)
                || ipv4_in_prefix(address, Ipv4Addr::new(240, 0, 0, 0), 4))
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_progressive_address(IpAddr::V4(mapped));
            }
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_multicast()
                || ipv6_in_prefix(address, Ipv6Addr::UNSPECIFIED, 96)
                || ipv6_in_prefix(address, Ipv6Addr::from(0x0064_ff9b_u128 << 96), 96)
                || ipv6_in_prefix(address, Ipv6Addr::from(0x0064_ff9b_0001_u128 << 80), 48)
                || ipv6_in_prefix(address, Ipv6Addr::from(0xfc00_u128 << 112), 7)
                || ipv6_in_prefix(address, Ipv6Addr::from(0xfe80_u128 << 112), 10)
                || ipv6_in_prefix(address, Ipv6Addr::from(0xfec0_u128 << 112), 10)
                // IETF protocol assignments occupy 2001::/23. Do not reject the whole
                // 2001::/32-looking textual family: ordinary globally routed networks such as
                // 2001:4860::/32 live outside this special-use prefix.
                || ipv6_in_prefix(address, Ipv6Addr::from(0x2001_u128 << 112), 23)
                || ipv6_in_prefix(address, Ipv6Addr::from(0x2002_u128 << 112), 16)
                || ipv6_in_prefix(
                    address,
                    Ipv6Addr::from(0x2001_0db8_0000_0000_0000_0000_0000_0000_u128),
                    32,
                )
                || ipv6_in_prefix(address, Ipv6Addr::from(0x3fff_u128 << 112), 20)
                || ipv6_in_prefix(
                    address,
                    Ipv6Addr::from(0x0100_0000_0000_0000_0000_0000_0000_0000_u128),
                    64,
                ))
        }
    }
}

#[derive(Debug)]
struct ProgressiveNetworkResolver {
    allow_private: bool,
}

impl ureq::unversioned::resolver::Resolver for ProgressiveNetworkResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let resolved = ureq::unversioned::resolver::DefaultResolver::default()
            .resolve(uri, config, timeout)?;
        if self.allow_private {
            return Ok(resolved);
        }
        let mut public = self.empty();
        for address in resolved.iter().copied() {
            if is_public_progressive_address(address.ip()) {
                public.push(address);
            }
        }
        if public.is_empty() {
            Err(ureq::Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Progressive media URL resolves only to a private, loopback, link-local, or reserved address; set NEOFLASH_ALLOW_PRIVATE_MEDIA_URLS=1 only for a trusted project and server",
            )))
        } else {
            Ok(public)
        }
    }
}

fn progressive_download_agent(remaining: Duration, allow_private: bool) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(remaining))
        .timeout_resolve(Some(remaining.min(Duration::from_secs(5))))
        .timeout_connect(Some(remaining.min(Duration::from_secs(5))))
        .timeout_recv_response(Some(remaining.min(Duration::from_secs(10))))
        .timeout_recv_body(Some(remaining.min(Duration::from_secs(20))))
        // Redirects are followed manually so every target passes the same DNS/address policy.
        .max_redirects(0)
        .max_response_header_size(32 * 1024)
        .user_agent("NeoFlash/0.1.0-beta.1");
    if !allow_private {
        // An environment proxy could resolve the target itself and bypass the checked resolver.
        // Trusted private/proxy workflows remain available through the explicit expert override.
        config = config.proxy(None);
    }
    ureq::Agent::with_parts(
        config.build(),
        ureq::unversioned::transport::DefaultConnector::default(),
        ProgressiveNetworkResolver { allow_private },
    )
}

/// Download a Progressive video source with bounded memory and explicit network timeouts.
///
/// This is deliberately separate from document serialization: downloaded bytes live only in the
/// process-local preview cache and are never silently embedded into the authored project.
pub fn fetch_progressive_video_bytes(url: &str) -> Result<Vec<u8>, String> {
    fetch_progressive_video_bytes_with_policy(
        url,
        MAX_PROGRESSIVE_VIDEO_DOWNLOAD_BYTES,
        cfg!(test) || private_progressive_networks_allowed(),
    )
}

/// Editor unit tests use a loopback HTTP fixture without weakening production URL policy.
#[doc(hidden)]
pub fn fetch_progressive_video_bytes_allowing_private_for_tests(
    url: &str,
) -> Result<Vec<u8>, String> {
    fetch_progressive_video_bytes_with_policy(url, MAX_PROGRESSIVE_VIDEO_DOWNLOAD_BYTES, true)
}

#[cfg(test)]
pub(super) fn fetch_progressive_video_bytes_with_limit(
    url: &str,
    max_download_bytes: usize,
) -> Result<Vec<u8>, String> {
    fetch_progressive_video_bytes_with_policy(url, max_download_bytes, true)
}

fn fetch_progressive_video_bytes_with_policy(
    url: &str,
    max_download_bytes: usize,
    allow_private: bool,
) -> Result<Vec<u8>, String> {
    let safe_initial = redacted_progressive_url(url);
    neoflash_core::validate_progressive_video_url(url)
        .map_err(|error| format!("Progressive video URL '{safe_initial}' is invalid: {error}"))?;
    let mut current = url::Url::parse(url)
        .map_err(|error| format!("Progressive video URL '{safe_initial}' is invalid: {error}"))?;
    let started = Instant::now();

    for redirect_count in 0..=MAX_PROGRESSIVE_VIDEO_REDIRECTS {
        let safe_url = redacted_progressive_url(current.as_str());
        neoflash_core::validate_progressive_video_url(current.as_str()).map_err(|error| {
            format!("Progressive video redirect '{safe_url}' is invalid: {error}")
        })?;
        let remaining = Duration::from_secs(30)
            .checked_sub(started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                format!("Progressive video download '{safe_initial}' exceeded the 30-second global timeout.")
            })?;
        let agent = progressive_download_agent(remaining, allow_private);
        let mut response = agent
            .get(current.as_str())
            .call()
            .map_err(|error| match error {
                ureq::Error::StatusCode(status) => format!(
                    "Progressive video download '{safe_url}' returned HTTP status {status}."
                ),
                ureq::Error::Timeout(kind) => {
                    format!("Progressive video download '{safe_url}' timed out while {kind:?}.")
                }
                other => format!("Progressive video download '{safe_url}' failed: {other}"),
            })?;

        if response.status().is_redirection() {
            if redirect_count >= MAX_PROGRESSIVE_VIDEO_REDIRECTS {
                return Err(format!(
                    "Progressive video download '{safe_initial}' exceeded the {MAX_PROGRESSIVE_VIDEO_REDIRECTS}-redirect limit."
                ));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    format!(
                        "Progressive video redirect '{safe_url}' omitted a valid Location header."
                    )
                })?;
            current = current.join(location).map_err(|error| {
                format!("Progressive video redirect from '{safe_url}' is invalid: {error}")
            })?;
            continue;
        }

        if let Some(content_length) = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            && content_length > max_download_bytes as u64
        {
            return Err(format!(
                "Progressive video download '{safe_url}' declares {content_length} bytes, exceeding the {max_download_bytes}-byte download budget."
            ));
        }

        let bytes = response
            .body_mut()
            .with_config()
            // `ureq` checks the limit before its final EOF read, so allow one sentinel byte and
            // enforce the exact public budget below.
            .limit((max_download_bytes as u64).saturating_add(1))
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => format!(
                    "Progressive video download '{safe_url}' exceeds the {max_download_bytes}-byte download budget."
                ),
                ureq::Error::Timeout(kind) => format!(
                    "Progressive video download '{safe_url}' timed out while receiving the body ({kind:?})."
                ),
                other => format!(
                    "Progressive video download '{safe_url}' failed while reading: {other}"
                ),
            })?;
        if bytes.len() > max_download_bytes {
            return Err(format!(
                "Progressive video download '{safe_url}' is {} bytes, exceeding the {max_download_bytes}-byte download budget.",
                bytes.len()
            ));
        }
        if bytes.is_empty() {
            return Err(format!(
                "Progressive video download '{safe_url}' returned an empty response body."
            ));
        }
        return Ok(bytes);
    }
    unreachable!("redirect loop exits through success or an explicit redirect-limit error")
}

/// Stage or update raw container bytes for a progressive video source URL, resource ID, or path.
pub fn stage_progressive_video_bytes(key: impl Into<String>, bytes: Vec<u8>) {
    let key = key.into();
    if key.trim().is_empty() || bytes.is_empty() {
        return;
    }
    if let Ok(mut cache) = staged_progressive_byte_cache().lock() {
        if cache.explicit.len() >= 16
            && !cache.explicit.contains_key(&key)
            && let Some(oldest_key) = cache
                .explicit
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
        {
            cache.explicit.remove(&oldest_key);
        }
        cache.explicit.insert(
            key,
            StagedProgressiveBytes {
                bytes: Arc::from(bytes),
                inserted_at: Instant::now(),
            },
        );
    }
}

/// Cache a successfully downloaded and decoded Progressive response for a short, byte-bounded
/// process lifetime. Local companion files deliberately never enter this URL-keyed cache.
pub fn stage_downloaded_progressive_video_bytes(key: impl Into<String>, bytes: Vec<u8>) {
    let key = key.into();
    if key.trim().is_empty()
        || bytes.is_empty()
        || bytes.len() > MAX_PROGRESSIVE_VIDEO_DOWNLOAD_BYTES
    {
        return;
    }
    if let Ok(mut cache) = staged_progressive_byte_cache().lock() {
        let now = Instant::now();
        cache.downloaded.retain(|_, entry| {
            now.saturating_duration_since(entry.inserted_at) <= DOWNLOADED_PROGRESSIVE_CACHE_TTL
        });
        cache.downloaded.remove(&key);
        loop {
            let retained_bytes = cache
                .downloaded
                .values()
                .map(|entry| entry.bytes.len())
                .sum::<usize>();
            if cache.downloaded.len() < MAX_DOWNLOADED_PROGRESSIVE_CACHE_ENTRIES
                && retained_bytes.saturating_add(bytes.len())
                    <= MAX_DOWNLOADED_PROGRESSIVE_CACHE_BYTES
            {
                break;
            }
            let Some(oldest_key) = cache
                .downloaded
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            cache.downloaded.remove(&oldest_key);
        }
        cache.downloaded.insert(
            key,
            StagedProgressiveBytes {
                bytes: Arc::from(bytes),
                inserted_at: now,
            },
        );
    }
}

/// Remove an invalid or stale staged source so a later resolution may retry its local/remote
/// origin instead of remaining poisoned for the process lifetime.
pub fn discard_staged_progressive_video_bytes(key: &str) {
    if let Ok(mut cache) = staged_progressive_byte_cache().lock() {
        cache.explicit.remove(key);
        cache.downloaded.remove(key);
    }
}

/// Look up staged container bytes for a progressive video source URL, resource ID, or path.
pub fn lookup_staged_progressive_video_bytes(key: &str) -> Option<Arc<[u8]>> {
    if key.trim().is_empty() {
        return None;
    }
    staged_progressive_byte_cache()
        .lock()
        .ok()
        .and_then(|mut cache| {
            if let Some(entry) = cache.explicit.get(key) {
                return Some(entry.bytes.clone());
            }
            let now = Instant::now();
            let expired = cache.downloaded.get(key).is_some_and(|entry| {
                now.saturating_duration_since(entry.inserted_at) > DOWNLOADED_PROGRESSIVE_CACHE_TTL
            });
            if expired {
                cache.downloaded.remove(key);
                return None;
            }
            cache.downloaded.get(key).map(|entry| entry.bytes.clone())
        })
}

/// Resolve a local or staged progressive file path from `url_or_path` / `resource_path` and `base_dir` (`T-274`).
pub fn resolve_progressive_local_path(
    url_or_path: &str,
    resource_path: Option<&str>,
    base_dir: Option<&Path>,
) -> Option<PathBuf> {
    fn extract_path_candidates(raw: &str, out: &mut Vec<String>) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed).trim();
        if let Some(rest) = without_query
            .strip_prefix("file://localhost")
            .or_else(|| without_query.strip_prefix("file://"))
        {
            if !rest.is_empty() {
                out.push(rest.to_owned());
            }
            return;
        }
        if let Some((_, after_scheme)) = without_query.split_once("://") {
            if let Some((_, url_path)) = after_scheme.split_once('/') {
                let clean = url_path.trim_start_matches('/');
                if !clean.is_empty() {
                    out.push(clean.to_owned());
                    if let Some(fname) = Path::new(clean).file_name().and_then(|s| s.to_str()) {
                        out.push(fname.to_owned());
                    }
                }
            }
        } else {
            out.push(without_query.to_owned());
            if let Some(fname) = Path::new(without_query)
                .file_name()
                .and_then(|s| s.to_str())
            {
                out.push(fname.to_owned());
            }
        }
    }

    let mut rel_candidates = Vec::new();
    extract_path_candidates(url_or_path, &mut rel_candidates);
    if let Some(rp) = resource_path {
        extract_path_candidates(rp, &mut rel_candidates);
    }

    for rel in &rel_candidates {
        let p = Path::new(rel);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        if let Some(dir) = base_dir {
            for candidate in [
                dir.join(rel),
                dir.join("media").join(rel),
                dir.join("video").join(rel),
                dir.join("clips").join(rel),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Resolve and decode a timeline video span/source from `project` (`Embedded`, `External`, or `Progressive`, `T-201` / `T-274`).
///
/// For `VideoDeploymentMode::Progressive`, resolves the video byte stream from:
/// 1. Embedded/cached resource bytes if present in `Project::resources` or staged progressive byte cache, OR
/// 2. Local/staged progressive file path resolved against `base_dir` or local path, OR
/// 3. Bounded HTTP(S) download into the process-local cache for remote-only sources.
///
/// Failures are actionable and never substitute unrelated pixels for the authored media.
pub fn resolve_and_decode_project_video(
    project: &Project,
    resource_id: &str,
    source_override: Option<&VideoSourceSpec>,
    base_dir: Option<&Path>,
) -> Result<Arc<DecodedVideoStream>, String> {
    let spec = source_override
        .cloned()
        .or_else(|| resolve_video_resource_source(project, resource_id))
        .ok_or_else(|| format!("Video resource '{resource_id}' does not exist in project."))?;

    let resource = project.resources.iter().find(|r| r.id == resource_id);

    match spec.mode {
        VideoDeploymentMode::Progressive => {
            let safe_source = redacted_progressive_url(&spec.url_or_path);
            // 1. Embedded/cached resource bytes in Project::resources or staged progressive byte cache:
            if let Some(res) = resource
                && !res.data_base64.is_empty()
            {
                let bytes = res
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                let label = if res.path.is_empty() {
                    safe_source.as_str()
                } else {
                    res.path.as_str()
                };
                return decode_video_stream_bytes(label, &spec.mime_type, &bytes).map(Arc::new);
            }
            let mut staged_errors = Vec::new();
            let mut staged_keys = vec![spec.url_or_path.as_str(), resource_id];
            if let Some(path) = resource
                .map(|resource| resource.path.as_str())
                .filter(|path| !path.is_empty())
            {
                staged_keys.push(path);
            }
            staged_keys.sort_unstable();
            staged_keys.dedup();
            for key in staged_keys {
                let Some(staged) = lookup_staged_progressive_video_bytes(key) else {
                    continue;
                };
                match decode_video_stream_bytes(&safe_source, &spec.mime_type, &staged) {
                    Ok(decoded) => return Ok(Arc::new(decoded)),
                    Err(error) => {
                        discard_staged_progressive_video_bytes(key);
                        staged_errors.push(error);
                    }
                }
            }
            if let Some(companion) = project.resources.iter().find(|r| {
                r.is_video()
                    && !r.data_base64.is_empty()
                    && (r.path == spec.url_or_path
                        || resource.is_some_and(|res| !res.path.is_empty() && r.path == res.path))
            }) {
                let bytes = companion
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                return decode_video_stream_bytes(&companion.path, &spec.mime_type, &bytes)
                    .map(Arc::new);
            }

            // 2. Local/staged progressive file path (resolved against project parent directory or staged local path):
            if let Some(local_path) = resolve_progressive_local_path(
                &spec.url_or_path,
                resource.map(|r| r.path.as_str()),
                base_dir,
            ) {
                // Never put project-local companion bytes into the process-global URL cache: a
                // different project may intentionally use the same remote URL with another local
                // companion. The external decoder already owns its bounded local-file backing.
                return decode_external_video_file(&local_path, &safe_source, &spec.mime_type)
                    .map(Arc::new);
            }

            // Validate the remote URL before attempting a network request.
            neoflash_core::validate_progressive_video_url(&spec.url_or_path).map_err(|err| {
                let staged_note = (!staged_errors.is_empty()).then(|| {
                    format!(" Earlier staged bytes were invalid: {}", staged_errors.join("; "))
                });
                format!("Timeline video '{resource_id}' uses Progressive deployment ('{safe_source}') with invalid URL: {err}.{}", staged_note.unwrap_or_default())
            })?;

            // 3. Bounded HTTP(S) download. Decode before caching so a corrupt or unsupported
            // response never poisons later preview/export attempts.
            let downloaded = fetch_progressive_video_bytes(&spec.url_or_path).map_err(|error| {
                let staged_note = (!staged_errors.is_empty()).then(|| {
                    format!(" Earlier staged bytes were invalid: {}", staged_errors.join("; "))
                });
                format!("Timeline video '{resource_id}' uses Progressive deployment ('{safe_source}'), but its media could not be downloaded: {error}.{}", staged_note.unwrap_or_default())
            })?;
            let decoded = decode_video_stream_bytes(&safe_source, &spec.mime_type, &downloaded)
                .map_err(|error| {
                format!(
                    "Timeline video '{resource_id}' downloaded from '{safe_source}' could not be decoded: {error}"
                )
            })?;
            stage_downloaded_progressive_video_bytes(spec.url_or_path.clone(), downloaded);
            Ok(Arc::new(decoded))
        }
        VideoDeploymentMode::External => {
            validate_external_video_path(&spec.url_or_path).map_err(|err| {
                format!(
                    "Video '{resource_id}' has invalid external path '{}': {err}",
                    spec.url_or_path
                )
            })?;
            if let Some(dir) = base_dir {
                let candidate = dir.join(&spec.url_or_path);
                if candidate.exists() {
                    return decode_external_video_file(
                        &candidate,
                        &spec.url_or_path,
                        &spec.mime_type,
                    )
                    .map(Arc::new);
                }
                return Err(format!(
                    "External video file '{}' for resource '{resource_id}' was not found at '{}'. Ensure the relative media file exists beside the project or switch deployment to Embedded.",
                    spec.url_or_path,
                    candidate.display()
                ));
            }
            if let Some(res) = resource
                && !res.data_base64.is_empty()
            {
                let bytes = res
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                return decode_video_stream_bytes(&spec.url_or_path, &spec.mime_type, &bytes)
                    .map(Arc::new);
            }
            Err(format!(
                "External video file '{}' for resource '{resource_id}' was not found (no project base directory available).",
                spec.url_or_path
            ))
        }
        VideoDeploymentMode::Embedded => {
            let res = resource.ok_or_else(|| {
                format!("Embedded video resource '{resource_id}' does not exist in project.")
            })?;
            let bytes = res
                .decoded_bytes()
                .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
            let label = if res.path.is_empty() {
                resource_id
            } else {
                res.path.as_str()
            };
            decode_video_stream_bytes(label, &res.media_type, &bytes).map(Arc::new)
        }
    }
}

fn collect_reachable_video_spans(
    project: &Project,
    root_symbol_id: &str,
) -> Vec<(String, VideoSourceSpec)> {
    let mut visited_symbols = HashSet::new();
    let mut stack = vec![root_symbol_id.to_owned()];
    let mut spans = Vec::new();
    let mut seen_keys = HashSet::new();

    while let Some(sym_id) = stack.pop() {
        if !visited_symbols.insert(sym_id.clone()) {
            continue;
        }
        let Some(symbol) = project.symbol(&sym_id) else {
            continue;
        };
        for layer in &symbol.timeline.layers {
            if !layer_effectively_visible(&symbol.timeline, layer)
                || matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
            {
                continue;
            }
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        stack.push(symbol_id.clone());
                    }
                }
                for element in key
                    .elements
                    .iter()
                    .filter(|element| matches!(element.kind, ElementKind::Video { .. }))
                {
                    if let Some((_, span)) =
                        neoflash_core::effective_video_span_for_element_at_frame(
                            layer,
                            key.frame,
                            &element.id,
                        )
                        && span.sync != VideoSyncMode::Stop
                        && let Some(spec) = resolve_video_source(project, span)
                    {
                        let res_id = if span.resource.is_empty() {
                            spec.url_or_path.clone()
                        } else {
                            span.resource.clone()
                        };
                        let key_tuple = (res_id.clone(), spec.mode, spec.url_or_path.clone());
                        if seen_keys.insert(key_tuple) {
                            spans.push((res_id, spec));
                        }
                    }
                }
            }
        }
    }
    spans
}

fn video_cache_key(resource_id: &str, source: Option<&VideoSourceSpec>) -> String {
    match source {
        Some(source) => format!(
            "{}:{resource_id}|{}|{}:{}|{}:{}",
            resource_id.len(),
            source.mode.as_str(),
            source.url_or_path.len(),
            source.url_or_path,
            source.mime_type.len(),
            source.mime_type,
        ),
        None => resource_id.to_owned(),
    }
}

pub(super) fn placement_video_cache_key(placement: &ResolvedVideoPlacement) -> String {
    video_cache_key(&placement.resource, placement.source.as_ref())
}

fn insert_decoded_video_stream_with_limit(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
    limit: usize,
) -> Result<(), String> {
    insert_decoded_video_stream_with_limits(
        cache,
        key,
        stream,
        limit,
        MAX_DECODED_VIDEO_BATCH_FRAMES,
    )
}

pub(super) fn insert_decoded_video_stream_with_limits(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
    byte_limit: usize,
    frame_limit: usize,
) -> Result<(), String> {
    if cache.contains_key(&key) {
        return Ok(());
    }
    const ARC_ALLOCATION_OVERHEAD: usize = 2 * std::mem::size_of::<usize>();
    const CACHE_BUCKET_OVERHEAD: usize = std::mem::size_of::<usize>();

    let mut seen_streams = HashSet::new();
    let mut seen_images = HashSet::new();
    let mut seen_decoders = HashSet::new();
    let mut total_bytes = 0usize;
    let mut total_frames = 0usize;

    let mut add_bytes = |bytes: usize, context: &str| -> Result<(), String> {
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or_else(|| format!("Decoded timeline-video cache {context} byte size overflow."))?;
        if total_bytes > byte_limit {
            return Err(format!(
                "Decoded timeline videos require at least {total_bytes} retained bytes including frame records and cache structures, exceeding the {} MiB cache budget. Shorten, resize, or use fewer simultaneous videos.",
                byte_limit / (1024 * 1024)
            ));
        }
        Ok(())
    };

    for cache_key in cache.keys().chain(std::iter::once(&key)) {
        add_bytes(
            std::mem::size_of::<(String, Option<Arc<DecodedVideoStream>>)>()
                .checked_add(cache_key.capacity())
                .and_then(|bytes| bytes.checked_add(CACHE_BUCKET_OVERHEAD))
                .ok_or_else(|| {
                    "Decoded timeline-video cache entry byte size overflow.".to_owned()
                })?,
            "entry",
        )?;
    }

    for existing in cache.values().flatten().chain(std::iter::once(&stream)) {
        let stream_ptr = Arc::as_ptr(existing) as usize;
        if !seen_streams.insert(stream_ptr) {
            continue;
        }
        total_frames = total_frames
            .checked_add(existing.logical_frame_count())
            .ok_or_else(|| "Decoded timeline-video frame count overflow.".to_owned())?;
        if total_frames > frame_limit {
            return Err(format!(
                "Decoded timeline videos retain {total_frames} frame records, exceeding the aggregate {frame_limit}-frame cache budget. Shorten the videos or use fewer simultaneous streams."
            ));
        }
        let frame_storage = existing
            .frames
            .capacity()
            .checked_mul(std::mem::size_of::<DecodedVideoFrame>())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<DecodedVideoStream>()))
            .and_then(|bytes| bytes.checked_add(ARC_ALLOCATION_OVERHEAD))
            .ok_or_else(|| "Decoded timeline-video frame-record byte size overflow.".to_owned())?;
        add_bytes(frame_storage, "frame-record")?;

        if let Some(decoder) = &existing.on_demand
            && seen_decoders.insert(Arc::as_ptr(decoder) as usize)
        {
            let ring_budget = decoder.pinned_budget_bytes().ok_or_else(|| {
                "Decoded timeline-video ring decoder byte size overflow.".to_owned()
            })?;
            add_bytes(ring_budget, "ring-decoder")?;
        }

        for frame in &existing.frames {
            let image_ptr = Arc::as_ptr(&frame.image) as usize;
            if !seen_images.insert(image_ptr) {
                continue;
            }
            let image_storage = frame
                .image
                .rgba
                .len()
                .checked_add(std::mem::size_of::<DecodedImage>())
                .and_then(|bytes| bytes.checked_add(ARC_ALLOCATION_OVERHEAD))
                .ok_or_else(|| {
                    "Decoded timeline-video image allocation byte size overflow.".to_owned()
                })?;
            add_bytes(image_storage, "image")?;
        }
    }
    cache.insert(key, Some(stream));
    Ok(())
}

pub(super) fn insert_decoded_video_stream(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
) -> Result<(), String> {
    insert_decoded_video_stream_with_limit(cache, key, stream, MAX_DECODED_VIDEO_BATCH_BYTES)
}

pub(super) fn preflight_and_decode_timeline_videos(
    project: &Project,
    root_symbol_id: &str,
    base_dir: Option<&Path>,
) -> Result<HashMap<String, Option<Arc<DecodedVideoStream>>>, ExportError> {
    let mut cache = HashMap::new();
    for (res_id, spec) in collect_reachable_video_spans(project, root_symbol_id) {
        let stream = resolve_and_decode_project_video(project, &res_id, Some(&spec), base_dir)
            .map_err(ExportError::Options)?;
        insert_decoded_video_stream(&mut cache, video_cache_key(&res_id, Some(&spec)), stream)
            .map_err(ExportError::Options)?;
    }
    Ok(cache)
}
