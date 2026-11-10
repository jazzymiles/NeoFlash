//! Authored timeline video scheduling, deployment modes, cue-point evaluation and container/codec diagnostics (`T-118`, `T-178`).
//!
//! Following NeoFlash's media architecture (`timeline_sound.rs`), Rust owns all timeline
//! synchronization, frame-to-time mapping, loop wrapping, cue-point crossing, deployment source
//! resolution (`Embedded`, `External`, `Progressive`) and container/CORS diagnostics.
//! Hosts (native wgpu/egui and HTML5 Canvas/WebGL) only decode and present frames.

use crate::evaluate::{DrawItem, Node};
use crate::{ElementKind, Id, Keyframe, Layer, LayerRole, Project};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};

/// How an authored video resource or span is deployed at publish/runtime (`T-178`). Embedded
/// timeline video is format 9; External and Progressive deployment are format 11.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoDeploymentMode {
    /// Embedded video bytes stored in the project resource (`data_base64`), synced to timeline frames.
    #[default]
    Embedded,
    /// Managed relative publish asset (e.g. `assets/video/intro.mp4`) without absolute paths.
    External,
    /// HTTP/HTTPS progressive download or streaming URL (`http://` or `https://`) with empty `data_base64`.
    Progressive,
}

impl VideoDeploymentMode {
    pub const ALL: [Self; 3] = [Self::Embedded, Self::External, Self::Progressive];

    pub fn is_embedded(&self) -> bool {
        matches!(self, Self::Embedded)
    }

    pub fn is_external(&self) -> bool {
        matches!(self, Self::External)
    }

    pub fn is_progressive(&self) -> bool {
        matches!(self, Self::Progressive)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::External => "external",
            Self::Progressive => "progressive",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Embedded => "Embed video in timeline",
            Self::External => "Load external video with playback component",
            Self::Progressive => "Progressive download from a web server",
        }
    }
}

/// Resolved deployment source specification for a timeline video span or resource (`T-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoSourceSpec {
    pub mode: VideoDeploymentMode,
    pub url_or_path: String,
    pub mime_type: String,
}

/// External video asset to copy/stage alongside a published bundle (`T-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalVideoAsset {
    pub resource_id: Id,
    pub relative_path: String,
    pub mime_type: String,
    /// Decoded media bytes if still present in the authoring project, or empty if external-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bytes: Vec<u8>,
}

/// Package manifest entry describing how a project video is deployed (`T-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoManifestEntry {
    pub resource_id: Id,
    pub source: VideoSourceSpec,
    /// True when the video bytes are embedded inside the project bundle (`Embedded` mode).
    pub embedded_in_bundle: bool,
    /// True when the publish pipeline must stage an external relative asset file (`External` mode).
    pub requires_external_file: bool,
    /// True when the video streams progressively over HTTP(S) with CORS (`Progressive` mode).
    pub requires_network_cors: bool,
    /// Decoded payload size in bytes inside the project bundle (`0` for `External` and `Progressive`).
    pub bundled_byte_len: usize,
}

/// How a timeline video span synchronizes with the timeline playhead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSyncMode {
    /// Locked to the timeline playhead frame; seeks deterministically when scrubbing or stepping.
    #[default]
    Stream,
    /// Triggered when its keyframe is entered; advances on its own wall-clock until stopped.
    Event,
    /// Stops active video playback for the referenced video resource.
    Stop,
}

/// Kind of authored cue point embedded in or attached to a timeline video span.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCuePointKind {
    Navigation,
    #[default]
    Event,
    ActionScript,
}

/// A named timestamp marker on a timeline video span (`T-118`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoCuePoint {
    pub name: String,
    pub time_ms: u32,
    #[serde(default)]
    pub kind: VideoCuePointKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<(String, String)>,
}

/// Authored timeline video settings attached to a keyframe or video layer span (`T-118`, `T-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineVideoSpan {
    #[serde(default)]
    pub resource: Id,
    #[serde(default)]
    pub sync: VideoSyncMode,
    #[serde(default)]
    pub start_offset_ms: u32,
    #[serde(default)]
    pub loop_video: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cue_points: Vec<VideoCuePoint>,
    /// Deployment mode (`Embedded` default; `External` or `Progressive` in format 11, `T-178`).
    #[serde(default, skip_serializing_if = "VideoDeploymentMode::is_embedded")]
    pub deployment: VideoDeploymentMode,
    /// Validated relative publish asset path when `deployment == External` (e.g. `assets/video/intro.mp4`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "url_or_path"
    )]
    pub external_path: Option<String>,
    /// Validated `http://` or `https://` progressive stream URL when `deployment == Progressive`.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "url")]
    pub stream_url: Option<String>,
}

impl Default for TimelineVideoSpan {
    fn default() -> Self {
        Self {
            resource: String::new(),
            sync: VideoSyncMode::Stream,
            start_offset_ms: 0,
            loop_video: false,
            muted: false,
            cue_points: Vec::new(),
            deployment: VideoDeploymentMode::Embedded,
            external_path: None,
            stream_url: None,
        }
    }
}

impl TimelineVideoSpan {
    pub fn embedded(resource: impl Into<Id>) -> Self {
        Self {
            resource: resource.into(),
            deployment: VideoDeploymentMode::Embedded,
            ..Self::default()
        }
    }

    pub fn external(resource: impl Into<Id>, relative_path: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            deployment: VideoDeploymentMode::External,
            external_path: Some(relative_path.into()),
            ..Self::default()
        }
    }

    pub fn progressive(resource: impl Into<Id>, stream_url: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            deployment: VideoDeploymentMode::Progressive,
            stream_url: Some(stream_url.into()),
            ..Self::default()
        }
    }

    pub fn uses_non_embedded_deployment(&self) -> bool {
        !self.deployment.is_embedded() || self.external_path.is_some() || self.stream_url.is_some()
    }

    pub fn source_spec(&self, project: &Project) -> Option<VideoSourceSpec> {
        resolve_video_source(project, self)
    }
}

fn serialize_static_code<S: Serializer>(
    code: &&'static str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(code)
}

fn deserialize_static_code<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<&'static str, D::Error> {
    let s = String::deserialize(deserializer)?;
    Ok(match s.as_str() {
        "video-cors-remote" => "video-cors-remote",
        "video-codec-hevc" => "video-codec-hevc",
        "video-empty-bytes" => "video-empty-bytes",
        "video-unknown-container" => "video-unknown-container",
        "video-unsupported-mime" => "video-unsupported-mime",
        "video-invalid-external-path" => "video-invalid-external-path",
        "video-invalid-progressive-url" => "video-invalid-progressive-url",
        "video-progressive-duplicate-bytes" => "video-progressive-duplicate-bytes",
        _ => "video-diagnostic",
    })
}

/// Container, codec or CORS diagnostic produced when inspecting a video resource (`T-118`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoCodecDiagnostic {
    #[serde(
        serialize_with = "serialize_static_code",
        deserialize_with = "deserialize_static_code"
    )]
    pub code: &'static str,
    pub message: String,
    pub is_error: bool,
}

/// Host instruction emitted by Rust's timeline video scheduler (`T-118`, `T-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum VideoCommand {
    /// Seek a timeline-locked stream video to `target_time_ms` and present its decoded frame.
    SeekAndPresent {
        channel: String,
        target_path: Vec<Id>,
        layer_id: Id,
        resource: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<VideoSourceSpec>,
        target_time_ms: u32,
        playing: bool,
        muted: bool,
        loop_video: bool,
    },
    /// Trigger an independent event video on keyframe entry.
    StartEvent {
        channel: String,
        target_path: Vec<Id>,
        layer_id: Id,
        resource: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<VideoSourceSpec>,
        start_offset_ms: u32,
        loop_video: bool,
        muted: bool,
    },
    /// Stop playback of `resource`.
    Stop { resource: Id },
    /// Stop one timeline-owned stream while leaving its last decoded frame available.
    StopChannel { channel: String },
    /// Pause timeline-owned streams without affecting scripted NetStreams.
    PauseStreams,
    /// Stop all active video instances.
    StopAll,
    /// Notify the host/script runtime that `cue_point` was crossed at `frame`.
    CuePointTriggered {
        layer_id: Id,
        resource: Id,
        cue_point: VideoCuePoint,
        frame: u32,
    },
}

#[derive(Clone, Debug)]
struct ActiveVideo {
    target_time_ms: u32,
}

/// Stateful authored-video scheduler. Like timeline sound, this is owned by `Player`: Rust
/// decides which span is active and the browser only decodes, seeks and paints native frames.
#[derive(Default)]
pub(crate) struct TimelineVideoState {
    commands: Vec<VideoCommand>,
    streams: HashMap<String, ActiveVideo>,
    entries: HashMap<String, u32>,
    started: bool,
}

struct ActiveSpan<'a> {
    owner: &'a Node,
    layer: &'a Layer,
    key: &'a Keyframe,
    span: &'a TimelineVideoSpan,
}

fn active_span_for_item<'a>(
    project: &'a Project,
    nodes: &'a [Node],
    item: &DrawItem,
) -> Option<ActiveSpan<'a>> {
    let (owner, layer) = nodes
        .iter()
        .filter(|node| item.instance_path.starts_with(&node.path))
        .filter_map(|node| {
            let symbol = node.symbol_id.as_ref().and_then(|id| project.symbol(id))?;
            let layer = symbol
                .timeline
                .layers
                .iter()
                .find(|layer| layer.id == item.layer_id)?;
            Some((node, layer))
        })
        .max_by_key(|(node, _)| node.path.len())?;
    let key = layer.key_at(owner.frame)?;
    let span = key.video.as_ref()?;
    Some(ActiveSpan {
        owner,
        layer,
        key,
        span,
    })
}

fn video_channel(target_path: &[Id], layer_id: &str, key_frame: u32) -> String {
    serde_json::to_string(&(target_path, layer_id, key_frame)).unwrap_or_default()
}

impl TimelineVideoState {
    fn push(&mut self, command: VideoCommand) {
        self.started = true;
        self.commands.push(command);
    }

    /// Reconcile every visible authored Video placement with the span on its owning layer.
    /// A placement is intentionally required: importing media without "Place instance on Stage"
    /// keeps it in the document without inventing an invisible decoder surface.
    pub(crate) fn sync(
        &mut self,
        project: &Project,
        nodes: &[Node],
        items: &[DrawItem],
        running: impl Fn(&[Id]) -> bool,
    ) {
        let mut video_items: Vec<_> = items
            .iter()
            .filter(|item| matches!(item.kind, ElementKind::Video { .. }))
            .collect();
        video_items.sort_by(|a, b| {
            a.instance_path
                .cmp(&b.instance_path)
                .then_with(|| a.layer_id.cmp(&b.layer_id))
        });

        let mut alive_streams = HashSet::new();
        let mut alive_entries = HashSet::new();
        for item in video_items {
            let Some(active) = active_span_for_item(project, nodes, item) else {
                continue;
            };
            let channel = video_channel(&item.instance_path, &active.layer.id, active.key.frame);
            let source = resolve_video_source(project, active.span);
            let effective_resource = if active.span.resource.is_empty() {
                source
                    .as_ref()
                    .map(|s| s.url_or_path.clone())
                    .unwrap_or_default()
            } else {
                active.span.resource.clone()
            };
            match active.span.sync {
                VideoSyncMode::Stream => {
                    let target_time_ms = compute_frame_video_time_ms(
                        active.key.frame,
                        active.owner.frame,
                        project.fps as f32,
                        active.span.start_offset_ms,
                        None,
                        active.span.loop_video,
                    );
                    if let Some(previous) = self.streams.get(&channel) {
                        for cue_point in collect_crossed_cue_points(
                            &active.span.cue_points,
                            previous.target_time_ms,
                            target_time_ms,
                        ) {
                            self.push(VideoCommand::CuePointTriggered {
                                layer_id: active.layer.id.clone(),
                                resource: effective_resource.clone(),
                                cue_point,
                                frame: active.owner.frame,
                            });
                        }
                    }
                    self.push(VideoCommand::SeekAndPresent {
                        channel: channel.clone(),
                        target_path: item.instance_path.clone(),
                        layer_id: active.layer.id.clone(),
                        resource: effective_resource,
                        source,
                        target_time_ms,
                        playing: running(&active.owner.path),
                        muted: active.span.muted,
                        loop_video: active.span.loop_video,
                    });
                    self.streams
                        .insert(channel.clone(), ActiveVideo { target_time_ms });
                    alive_streams.insert(channel);
                }
                VideoSyncMode::Event => {
                    let previous = self.entries.get(&channel).copied();
                    if active.owner.frame == active.key.frame
                        && previous != Some(active.owner.frame)
                    {
                        self.push(VideoCommand::StartEvent {
                            channel: channel.clone(),
                            target_path: item.instance_path.clone(),
                            layer_id: active.layer.id.clone(),
                            resource: effective_resource,
                            source,
                            start_offset_ms: active.span.start_offset_ms,
                            loop_video: active.span.loop_video,
                            muted: active.span.muted,
                        });
                    }
                    self.entries.insert(channel.clone(), active.owner.frame);
                    alive_entries.insert(channel);
                }
                VideoSyncMode::Stop => {
                    // Stop spans do not need a visible decoder surface and are reconciled from
                    // their owning timeline below. Avoid coupling them to this DrawItem.
                }
            }
        }

        // Stop sync is an instruction, not a display placement. It must fire even when its
        // keyframe intentionally has no Video element (the usual authoring shape for a stop).
        let mut timeline_nodes: Vec<_> = nodes
            .iter()
            .filter(|node| node.symbol_id.is_some())
            .collect();
        timeline_nodes.sort_by(|a, b| a.path.cmp(&b.path));
        for owner in timeline_nodes {
            let Some(symbol) = owner.symbol_id.as_ref().and_then(|id| project.symbol(id)) else {
                continue;
            };
            for layer in &symbol.timeline.layers {
                if matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder) {
                    continue;
                }
                let Some(key) = layer.key_at(owner.frame) else {
                    continue;
                };
                let Some(span) = key
                    .video
                    .as_ref()
                    .filter(|span| span.sync == VideoSyncMode::Stop)
                else {
                    continue;
                };
                let channel = video_channel(&owner.path, &layer.id, key.frame);
                let previous = self.entries.get(&channel).copied();
                if owner.frame == key.frame && previous != Some(owner.frame) {
                    let resource = if span.resource.is_empty() {
                        resolve_video_source(project, span)
                            .map(|s| s.url_or_path)
                            .unwrap_or_default()
                    } else {
                        span.resource.clone()
                    };
                    self.push(VideoCommand::Stop { resource });
                }
                self.entries.insert(channel.clone(), owner.frame);
                alive_entries.insert(channel);
            }
        }

        let mut ended: Vec<_> = self
            .streams
            .keys()
            .filter(|channel| !alive_streams.contains(*channel))
            .cloned()
            .collect();
        ended.sort();
        for channel in ended {
            self.streams.remove(&channel);
            self.push(VideoCommand::StopChannel { channel });
        }
        self.entries
            .retain(|channel, _| alive_entries.contains(channel));
    }

    pub(crate) fn pause_streams(&mut self) {
        if !self.streams.is_empty() {
            self.push(VideoCommand::PauseStreams);
        }
    }

    pub(crate) fn stop_streams(&mut self) {
        let mut channels: Vec<_> = self.streams.drain().map(|(channel, _)| channel).collect();
        channels.sort();
        for channel in channels {
            self.push(VideoCommand::StopChannel { channel });
        }
    }

    pub(crate) fn reset(&mut self) {
        let started = self.started;
        *self = Self::default();
        if started {
            self.push(VideoCommand::StopAll);
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<VideoCommand> {
        std::mem::take(&mut self.commands)
    }
}

/// Pure deterministic frame-to-media-time mapping in milliseconds (`T-118`).
///
/// Computes the target video timestamp for `current_frame` relative to `keyframe_start_frame` at `fps`,
/// applying `start_offset_ms` and clamping or wrapping by `duration_ms` when `loop_video` is set.
pub fn compute_frame_video_time_ms(
    keyframe_start_frame: u32,
    current_frame: u32,
    fps: f32,
    start_offset_ms: u32,
    duration_ms: Option<u32>,
    loop_video: bool,
) -> u32 {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        f64::from(fps)
    } else {
        24.0
    };
    let elapsed_frames = current_frame.saturating_sub(keyframe_start_frame);
    let elapsed_ms = ((f64::from(elapsed_frames) * 1000.0) / safe_fps).round() as u64;
    let total_ms = u64::from(start_offset_ms).saturating_add(elapsed_ms);

    if let Some(dur) = duration_ms.filter(|&d| d > 0) {
        let dur_u64 = u64::from(dur);
        if loop_video {
            (total_ms % dur_u64) as u32
        } else {
            total_ms.min(dur_u64) as u32
        }
    } else {
        total_ms.min(u64::from(u32::MAX)) as u32
    }
}

/// Collect cue points crossed between `prev_time_ms` and `curr_time_ms` in deterministic chronological order.
///
/// - On forward playback or scrub (`curr_time_ms > prev_time_ms`), matches cue points in `(prev_time_ms, curr_time_ms]`
///   (and includes `0` when starting from `prev_time_ms == 0`).
/// - On backward scrub or loop wrap (`curr_time_ms < prev_time_ms`), matches cue points in `[curr_time_ms, prev_time_ms)`.
/// - When `curr_time_ms == prev_time_ms`, matches cue points at that exact timestamp only if `curr_time_ms == 0`.
pub fn collect_crossed_cue_points(
    cue_points: &[VideoCuePoint],
    prev_time_ms: u32,
    curr_time_ms: u32,
) -> Vec<VideoCuePoint> {
    let mut crossed: Vec<VideoCuePoint> = cue_points
        .iter()
        .filter(|cp| {
            if curr_time_ms > prev_time_ms {
                if prev_time_ms == 0 {
                    cp.time_ms <= curr_time_ms
                } else {
                    cp.time_ms > prev_time_ms && cp.time_ms <= curr_time_ms
                }
            } else if curr_time_ms < prev_time_ms {
                cp.time_ms >= curr_time_ms && cp.time_ms < prev_time_ms
            } else {
                prev_time_ms == 0 && cp.time_ms == 0
            }
        })
        .cloned()
        .collect();
    crossed.sort_by(|a, b| a.time_ms.cmp(&b.time_ms).then_with(|| a.name.cmp(&b.name)));
    crossed
}

fn contains_fourcc(haystack: &[u8], fourcc: &[u8; 4]) -> bool {
    haystack.windows(4).any(|w| w == fourcc)
}

/// Infer a canonical browser video MIME type from a filename or URL extension (`T-178`).
pub fn infer_video_mime_type(path_or_url: &str) -> &'static str {
    let clean = path_or_url
        .split(['?', '#'])
        .next()
        .unwrap_or(path_or_url)
        .trim()
        .to_ascii_lowercase();
    if clean.ends_with(".webm") {
        "video/webm"
    } else if clean.ends_with(".ogv") || clean.ends_with(".ogg") {
        "video/ogg"
    } else {
        "video/mp4"
    }
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn contains_encoded_dot_dot(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    lower == "%2e%2e"
        || lower == ".%2e"
        || lower == "%2e."
        || lower.contains("%2f")
        || lower.contains("%5c")
}

/// Strictly validate a managed relative publish asset path for `VideoDeploymentMode::External` (`T-178`).
///
/// Rejects empty paths, leading/trailing whitespace, control characters (`\0`, `\r`, `\n`),
/// backslashes, absolute paths (`/` or Windows drive letters), `file:`/`http:`/`https:`/`data:` URLs,
/// URL credentials (`user:pass@`), and `.` or `..` traversal segments.
pub fn validate_external_video_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.trim().is_empty() {
        return Err("external video path must be a non-empty relative path".into());
    }
    if path.len() > 1024 {
        return Err("external video path exceeds 1024 bytes".into());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("external video path must not contain control characters".into());
    }
    if path != path.trim() {
        return Err("external video path must not have leading or trailing whitespace".into());
    }
    if path.contains('\\') {
        return Err("external video path must use forward slashes and not backslashes".into());
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return Err("external video path must be a relative asset path, not a file: URL".into());
    }
    if path.contains('@') {
        return Err("external video path must not contain URL credentials".into());
    }
    if path.starts_with('/') || path.starts_with('~') || has_windows_drive_prefix(path) {
        return Err(
            "external video path must be relative and not an absolute filesystem path".into(),
        );
    }
    if path.contains(':') {
        return Err(
            "external video path must be a relative path without a URL scheme or drive letter"
                .into(),
        );
    }
    if path.contains(['?', '#']) {
        return Err("external video path must not contain query or fragment components".into());
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || contains_encoded_dot_dot(segment)
        {
            return Err(
                "external video path must not contain empty, '.' or '..' parent traversal segments"
                    .into(),
            );
        }
    }
    Ok(())
}

/// Strictly validate a progressive download / streaming URL for `VideoDeploymentMode::Progressive` (`T-178`).
///
/// Requires a valid `http://` or `https://` URL with a non-empty host; rejects `file:`, `javascript:`,
/// `data:`, embedded credentials (`user:pass@host`), `..` path traversal, backslashes, and control characters.
pub fn validate_progressive_video_url(url: &str) -> Result<(), String> {
    if url.is_empty() || url.trim().is_empty() {
        return Err("progressive video URL must be a non-empty http:// or https:// URL".into());
    }
    if url.len() > 2048 {
        return Err("progressive video URL exceeds 2048 bytes".into());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(
            "progressive video URL must not contain control characters or whitespace".into(),
        );
    }
    if url.contains('\\') {
        return Err("progressive video URL must not contain backslashes".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("file:")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
    {
        return Err(
            "progressive video URL must use http:// or https:// (file:, javascript: and data: are forbidden)"
                .into(),
        );
    }
    let rest = if let Some(r) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        r
    } else if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return Err("progressive video URL must start with http:// or https://".into());
    };

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let tail = &rest[authority_end..];

    if authority.is_empty() {
        return Err("progressive video URL requires a non-empty host".into());
    }
    if authority.contains('@') || lower.contains("%40") {
        return Err(
            "progressive video URL must not contain embedded credentials (user:pass@host)".into(),
        );
    }

    let host = if let Some(after_bracket) = authority.strip_prefix('[') {
        let Some(close) = after_bracket.find(']') else {
            return Err("progressive video URL has an unclosed IPv6 host literal".into());
        };
        let ipv6 = &after_bracket[..close];
        let remainder = &after_bracket[close + 1..];
        if let Some(port_str) = remainder.strip_prefix(':') {
            let Ok(port) = port_str.parse::<u16>() else {
                return Err("progressive video URL has an invalid port".into());
            };
            if port == 0 {
                return Err("progressive video URL port must be in 1..=65535".into());
            }
        } else if !remainder.is_empty() {
            return Err("progressive video URL has invalid characters after IPv6 host".into());
        }
        ipv6
    } else if let Some((h, port_str)) = authority.rsplit_once(':') {
        let Ok(port) = port_str.parse::<u16>() else {
            return Err("progressive video URL has an invalid port".into());
        };
        if port == 0 {
            return Err("progressive video URL port must be in 1..=65535".into());
        }
        h
    } else {
        authority
    };

    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'))
    {
        return Err("progressive video URL requires a valid non-empty host".into());
    }

    let path_part = tail.split(['?', '#']).next().unwrap_or("");
    let lower_path = path_part.to_ascii_lowercase();
    if lower_path.contains("%5c") || lower_path.contains("%00") {
        return Err("progressive video URL path contains forbidden encoded characters".into());
    }
    for segment in path_part.split('/') {
        if segment == "." || segment == ".." || contains_encoded_dot_dot(segment) {
            return Err(
                "progressive video URL must not contain '.' or '..' path traversal segments".into(),
            );
        }
    }
    Ok(())
}

/// Resolve the runtime/publish `VideoSourceSpec` for a `TimelineVideoSpan` (`T-178`).
pub fn resolve_video_source(
    project: &Project,
    span: &TimelineVideoSpan,
) -> Option<VideoSourceSpec> {
    let resource = if span.resource.is_empty() {
        None
    } else {
        project.resources.iter().find(|r| r.id == span.resource)
    };
    let mode = if !span.deployment.is_embedded() {
        span.deployment
    } else if span.stream_url.is_some() {
        VideoDeploymentMode::Progressive
    } else if span.external_path.is_some() {
        VideoDeploymentMode::External
    } else if let Some(res) = resource
        && res.data_base64.is_empty()
    {
        let lower = res.path.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            VideoDeploymentMode::Progressive
        } else {
            VideoDeploymentMode::External
        }
    } else {
        VideoDeploymentMode::Embedded
    };

    let url_or_path = match mode {
        VideoDeploymentMode::Embedded => resource.map(|r| r.path.clone()).unwrap_or_default(),
        VideoDeploymentMode::External => span
            .external_path
            .clone()
            .or_else(|| span.stream_url.clone())
            .or_else(|| resource.map(|r| r.path.clone()))
            .unwrap_or_default(),
        VideoDeploymentMode::Progressive => span
            .stream_url
            .clone()
            .or_else(|| span.external_path.clone())
            .or_else(|| resource.map(|r| r.path.clone()))
            .unwrap_or_default(),
    };

    if url_or_path.is_empty() && resource.is_none() {
        return None;
    }

    let mime_type = resource
        .map(|r| r.media_type.trim().to_ascii_lowercase())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| infer_video_mime_type(&url_or_path).to_owned());

    Some(VideoSourceSpec {
        mode,
        url_or_path,
        mime_type,
    })
}

/// Resolve the `VideoSourceSpec` for a video resource ID across `project` (`T-178`).
pub fn resolve_video_resource_source(
    project: &Project,
    resource_id: &str,
) -> Option<VideoSourceSpec> {
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                if let Some(span) = &key.video
                    && (span.resource == resource_id
                        || (span.resource.is_empty()
                            && (span.external_path.as_deref() == Some(resource_id)
                                || span.stream_url.as_deref() == Some(resource_id))))
                {
                    return resolve_video_source(project, span);
                }
            }
        }
    }
    let resource = project
        .resources
        .iter()
        .find(|r| r.id == resource_id && r.is_video())?;
    let mode = if resource.data_base64.is_empty() {
        let lower = resource.path.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            VideoDeploymentMode::Progressive
        } else {
            VideoDeploymentMode::External
        }
    } else {
        VideoDeploymentMode::Embedded
    };
    Some(VideoSourceSpec {
        mode,
        url_or_path: resource.path.clone(),
        mime_type: resource.media_type.trim().to_ascii_lowercase(),
    })
}

/// Collect all resolved timeline video sources `(resource_id, VideoSourceSpec)` in deterministic order (`T-178`).
pub fn timeline_video_sources(project: &Project) -> Vec<(Id, VideoSourceSpec)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                if let Some(span) = &key.video
                    && let Some(spec) = resolve_video_source(project, span)
                {
                    let resource_id = if span.resource.is_empty() {
                        spec.url_or_path.clone()
                    } else {
                        span.resource.clone()
                    };
                    let source_key = (
                        resource_id.clone(),
                        spec.mode,
                        spec.url_or_path.clone(),
                        spec.mime_type.clone(),
                    );
                    if seen.insert(source_key) {
                        out.push((resource_id, spec));
                    }
                }
            }
        }
    }
    for res in &project.resources {
        if res.is_video()
            && let Some(spec) = resolve_video_resource_source(project, &res.id)
        {
            let source_key = (
                res.id.clone(),
                spec.mode,
                spec.url_or_path.clone(),
                spec.mime_type.clone(),
            );
            if seen.insert(source_key) {
                out.push((res.id.clone(), spec));
            }
        }
    }
    out
}

/// Collect external video assets (`VideoDeploymentMode::External`) for the export/publish pipeline (`T-178`).
pub fn collect_external_video_assets(project: &Project) -> Vec<ExternalVideoAsset> {
    let mut assets = Vec::new();
    let mut seen = HashSet::new();
    for (resource_id, spec) in timeline_video_sources(project) {
        if spec.mode != VideoDeploymentMode::External {
            continue;
        }
        if !seen.insert((resource_id.clone(), spec.url_or_path.clone())) {
            continue;
        }
        let bytes = project
            .resources
            .iter()
            .find(|r| r.id == resource_id)
            .and_then(|r| {
                if r.data_base64.is_empty() {
                    None
                } else {
                    r.decoded_bytes().ok()
                }
            })
            .unwrap_or_default();
        assets.push(ExternalVideoAsset {
            resource_id,
            relative_path: spec.url_or_path,
            mime_type: spec.mime_type,
            bytes,
        });
    }
    assets
}

/// Produce package manifest entries for all videos in `project` (`Embedded`, `External`, `Progressive`, `T-178`).
pub fn video_manifest_entries(project: &Project) -> Vec<VideoManifestEntry> {
    let mut entries = Vec::new();
    for (resource_id, source) in timeline_video_sources(project) {
        let bundled_byte_len = match source.mode {
            VideoDeploymentMode::Embedded => project
                .resources
                .iter()
                .find(|r| r.id == resource_id)
                .and_then(|r| r.decoded_bytes().ok())
                .map_or(0, |b| b.len()),
            VideoDeploymentMode::External | VideoDeploymentMode::Progressive => 0,
        };
        entries.push(VideoManifestEntry {
            resource_id,
            embedded_in_bundle: source.mode == VideoDeploymentMode::Embedded,
            requires_external_file: source.mode == VideoDeploymentMode::External,
            requires_network_cors: source.mode == VideoDeploymentMode::Progressive,
            bundled_byte_len,
            source,
        });
    }
    entries
}

/// Extract external video assets and clear `data_base64` on all non-Embedded (`External` and `Progressive`)
/// video resources so the published project JSON does not duplicate raw video bytes (`T-178`).
pub fn strip_non_embedded_video_bytes(project: &mut Project) -> Vec<ExternalVideoAsset> {
    let external_assets = collect_external_video_assets(project);
    let sources = timeline_video_sources(project);
    let embedded_ids: HashSet<&str> = sources
        .iter()
        .filter(|(_, spec)| spec.mode.is_embedded())
        .map(|(id, _)| id.as_str())
        .collect();
    let non_embedded_ids: HashSet<&str> = sources
        .iter()
        .filter(|(_, spec)| !spec.mode.is_embedded())
        .map(|(id, _)| id.as_str())
        .collect();
    for res in &mut project.resources {
        if res.is_video()
            && non_embedded_ids.contains(res.id.as_str())
            && !embedded_ids.contains(res.id.as_str())
        {
            res.data_base64 = "".into();
        }
    }
    external_assets
}

/// Inspect a video resource or span across all three deployment modes (`Embedded`, `External`, `Progressive`, `T-178`).
pub fn inspect_video_deployment(
    name: &str,
    mime: &str,
    bytes: &[u8],
    mode: VideoDeploymentMode,
    url_or_path: Option<&str>,
) -> Vec<VideoCodecDiagnostic> {
    match mode {
        VideoDeploymentMode::Embedded => inspect_video_resource(name, mime, bytes, None),
        VideoDeploymentMode::External => {
            let mut diags = Vec::new();
            let path = url_or_path.unwrap_or("").trim();
            if let Err(err) = validate_external_video_path(path) {
                diags.push(VideoCodecDiagnostic {
                    code: "video-invalid-external-path",
                    message: format!("Video '{name}' has invalid external path '{path}': {err}"),
                    is_error: true,
                });
            }
            if !bytes.is_empty() {
                diags.extend(inspect_video_resource(name, mime, bytes, None));
            } else {
                let mime_trimmed = mime.trim().to_ascii_lowercase();
                if !mime_trimmed.is_empty()
                    && !mime_trimmed.starts_with("video/")
                    && mime_trimmed != "application/octet-stream"
                {
                    diags.push(VideoCodecDiagnostic {
                        code: "video-unsupported-mime",
                        message: format!(
                            "Video '{name}' has non-video MIME type '{mime}'; expected video/mp4, video/webm or video/ogg."
                        ),
                        is_error: true,
                    });
                }
            }
            diags
        }
        VideoDeploymentMode::Progressive => {
            let mut diags = Vec::new();
            let url = url_or_path.unwrap_or("").trim();
            if let Err(err) = validate_progressive_video_url(url) {
                diags.push(VideoCodecDiagnostic {
                    code: "video-invalid-progressive-url",
                    message: format!("Video '{name}' has invalid progressive URL '{url}': {err}"),
                    is_error: true,
                });
            } else {
                diags.extend(inspect_video_resource(name, mime, &[], Some(url)));
            }
            if !bytes.is_empty() {
                diags.push(VideoCodecDiagnostic {
                    code: "video-progressive-duplicate-bytes",
                    message: format!(
                        "Video '{name}' is in progressive mode and must not embed duplicate raw video bytes."
                    ),
                    is_error: true,
                });
            }
            diags
        }
    }
}

/// Inspect a video resource's container header, codec brands and external URL for browser/native
/// playback compatibility (`T-118`).
pub fn inspect_video_resource(
    name: &str,
    mime: &str,
    bytes: &[u8],
    external_url: Option<&str>,
) -> Vec<VideoCodecDiagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(url) = external_url.map(str::trim).filter(|u| !u.is_empty())
        && (url.starts_with("http://") || url.starts_with("https://"))
    {
        diagnostics.push(VideoCodecDiagnostic {
            code: "video-cors-remote",
            message: format!(
                "Video '{name}' uses cross-origin URL '{url}'; HTML5 WebGL/Canvas export requires CORS headers (Access-Control-Allow-Origin)."
            ),
            is_error: false,
        });
    }

    let mime_trimmed = mime.trim().to_ascii_lowercase();
    if !mime_trimmed.is_empty()
        && !mime_trimmed.starts_with("video/")
        && mime_trimmed != "application/octet-stream"
    {
        diagnostics.push(VideoCodecDiagnostic {
            code: "video-unsupported-mime",
            message: format!(
                "Video '{name}' has non-video MIME type '{mime}'; expected video/mp4, video/webm or video/ogg."
            ),
            is_error: true,
        });
    }

    if bytes.is_empty() {
        if external_url.map(str::trim).unwrap_or("").is_empty() {
            diagnostics.push(VideoCodecDiagnostic {
                code: "video-empty-bytes",
                message: format!(
                    "Video '{name}' has no embedded media bytes and no external stream URL."
                ),
                is_error: true,
            });
        }
        return diagnostics;
    }

    let is_mp4 = bytes.len() >= 12 && &bytes[4..8] == b"ftyp";
    let is_webm = bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]);
    let is_ogg = bytes.starts_with(b"OggS");

    if !is_mp4 && !is_webm && !is_ogg {
        diagnostics.push(VideoCodecDiagnostic {
            code: "video-unknown-container",
            message: format!(
                "Video '{name}' is missing a valid MP4 (ftyp), WebM (EBML) or Ogg (OggS) container header."
            ),
            is_error: true,
        });
        return diagnostics;
    }

    // Scan bounded prefix (up to 64 KiB) for HEVC fourccs (`hev1`, `hvc1`) in MP4 containers.
    let scan_len = bytes.len().min(65_536);
    let header_slice = &bytes[..scan_len];
    if is_mp4 && (contains_fourcc(header_slice, b"hev1") || contains_fourcc(header_slice, b"hvc1"))
    {
        diagnostics.push(VideoCodecDiagnostic {
            code: "video-codec-hevc",
            message: format!(
                "Video '{name}' uses HEVC (hev1/hvc1); transcode to H.264 (avc1) MP4 or VP9 WebM for cross-browser and native timeline sync."
            ),
            is_error: true,
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Element, InstancePlayback, LayerProperties, Player, ResourceUnit, Symbol, SymbolKind,
        Timeline, Transform,
    };

    fn video_element(id: &str) -> Element {
        Element {
            id: id.into(),
            name: id.into(),
            transform: Transform::default(),
            kind: ElementKind::Video {
                width: 320.0,
                height: 180.0,
            },
            ..Element::default()
        }
    }

    fn video_layer(id: &str, sync: VideoSyncMode) -> Layer {
        Layer {
            id: id.into(),
            name: id.into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![video_element("video")],
                video: Some(TimelineVideoSpan {
                    resource: "movie".into(),
                    sync,
                    ..TimelineVideoSpan::default()
                }),
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        }
    }

    fn runtime_video_project(sync: VideoSyncMode) -> Project {
        let mut project = Project::blank();
        project.format_version = 9;
        project.fps = 10;
        project.symbols[0].timeline = Timeline {
            frame_count: 4,
            layers: vec![video_layer("video-layer", sync)],
            scripts: vec![],
            guides: vec![],
        };
        project.resources.push(
            ResourceUnit::from_bytes(
                "movie",
                "media/movie.mp4",
                "video/mp4",
                b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2avc1mp41",
            )
            .unwrap(),
        );
        project.validate().unwrap();
        project
    }

    #[test]
    fn frame_to_video_time_sync_clamps_and_loops_deterministically() {
        // 24 fps: 12 frames = 500 ms. With 200 ms start offset -> 700 ms.
        assert_eq!(
            compute_frame_video_time_ms(10, 22, 24.0, 200, Some(2000), false),
            700
        );

        // Non-looping span clamps to duration_ms (1000 ms).
        assert_eq!(
            compute_frame_video_time_ms(0, 48, 24.0, 0, Some(1000), false),
            1000
        );

        // Looping span wraps modulo duration_ms (2000 ms elapsed % 1200 ms = 800 ms).
        assert_eq!(
            compute_frame_video_time_ms(0, 48, 24.0, 0, Some(1200), true),
            800
        );
    }

    #[test]
    fn cue_points_trigger_on_forward_play_and_scrub() {
        let cues = vec![
            VideoCuePoint {
                name: "intro".into(),
                time_ms: 0,
                kind: VideoCuePointKind::Navigation,
                parameters: vec![("scene".into(), "Scene 1".into())],
            },
            VideoCuePoint {
                name: "beat_1".into(),
                time_ms: 250,
                kind: VideoCuePointKind::Event,
                parameters: vec![],
            },
            VideoCuePoint {
                name: "script_hook".into(),
                time_ms: 500,
                kind: VideoCuePointKind::ActionScript,
                parameters: vec![("action".into(), "syncSubtitles".into())],
            },
        ];

        // Initial step 0 -> 300 ms crosses both "intro" (0 ms) and "beat_1" (250 ms).
        let crossed_initial = collect_crossed_cue_points(&cues, 0, 300);
        assert_eq!(crossed_initial.len(), 2);
        assert_eq!(crossed_initial[0].name, "intro");
        assert_eq!(crossed_initial[1].name, "beat_1");

        // Next step 300 -> 500 ms crosses only "script_hook" (500 ms).
        let crossed_next = collect_crossed_cue_points(&cues, 300, 500);
        assert_eq!(crossed_next.len(), 1);
        assert_eq!(crossed_next[0].name, "script_hook");

        // Backward scrub 500 -> 200 ms crosses "beat_1" (250 ms).
        let crossed_back = collect_crossed_cue_points(&cues, 500, 200);
        assert_eq!(crossed_back.len(), 1);
        assert_eq!(crossed_back[0].name, "beat_1");
    }

    #[test]
    fn inspect_video_resource_detects_mp4_webm_hevc_missing_header_and_cors() {
        // Valid H.264 MP4 header (`ftypisom...avc1`)
        let mp4_avc1 = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41";
        let diags_ok = inspect_video_resource("clip.mp4", "video/mp4", mp4_avc1, None);
        assert!(
            diags_ok.is_empty(),
            "expected no errors for H.264 MP4, got {diags_ok:?}"
        );

        // Valid WebM EBML header
        let webm_bytes = b"\x1A\x45\xDF\xA3\x93\x42\x82\x88webm";
        assert!(inspect_video_resource("clip.webm", "video/webm", webm_bytes, None).is_empty());

        //Valid Ogg header
        let ogg_bytes = b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(inspect_video_resource("clip.ogv", "video/ogg", ogg_bytes, None).is_empty());

        // Unsupported HEVC (`hvc1`) inside MP4 container
        let mp4_hevc = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isomhvc1";
        let hevc_diags = inspect_video_resource("hevc.mp4", "video/mp4", mp4_hevc, None);
        assert_eq!(hevc_diags.len(), 1);
        assert_eq!(hevc_diags[0].code, "video-codec-hevc");
        assert!(hevc_diags[0].is_error);

        // Cross-origin URL warning
        let cors_diags = inspect_video_resource(
            "remote.mp4",
            "video/mp4",
            mp4_avc1,
            Some("https://cdn.example.com/remote.mp4"),
        );
        assert_eq!(cors_diags.len(), 1);
        assert_eq!(cors_diags[0].code, "video-cors-remote");
        assert!(!cors_diags[0].is_error);

        // Missing/corrupt container header
        let bad_diags = inspect_video_resource("corrupt.mp4", "video/mp4", b"NOT_A_VIDEO", None);
        assert_eq!(bad_diags.len(), 1);
        assert_eq!(bad_diags[0].code, "video-unknown-container");
        assert!(bad_diags[0].is_error);
    }

    #[test]
    fn project_validation_requires_a_video_resource_for_timeline_spans() {
        let mut project = Project::blank();
        project.format_version = 9;
        project.symbols[0].timeline.layers[0].keyframes[0].video = Some(TimelineVideoSpan {
            resource: "clip".into(),
            cue_points: vec![VideoCuePoint {
                name: "intro".into(),
                time_ms: 0,
                kind: VideoCuePointKind::Event,
                parameters: Vec::new(),
            }],
            ..TimelineVideoSpan::default()
        });

        assert!(
            project
                .validate()
                .unwrap_err()
                .to_string()
                .contains("video resource 'clip' does not exist")
        );
        project.resources.push(
            ResourceUnit::from_bytes("clip", "video/clip.bin", "application/octet-stream", b"x")
                .unwrap(),
        );
        assert!(
            project
                .validate()
                .unwrap_err()
                .to_string()
                .contains("is not video (video/*)")
        );

        project.resources[0].media_type = "VIDEO/MP4".into();
        project.validate().unwrap();
    }

    #[test]
    fn player_schedules_timeline_video_across_play_pause_seek_and_restart() {
        let project = runtime_video_project(VideoSyncMode::Stream);
        let mut player = Player::new(&project);

        let startup = player.drain_video_commands();
        assert_eq!(startup.len(), 1);
        let channel = match &startup[0] {
            VideoCommand::SeekAndPresent {
                channel,
                target_path,
                resource,
                target_time_ms,
                playing,
                ..
            } => {
                assert_eq!(target_path, &[String::from("video")]);
                assert_eq!(resource, "movie");
                assert_eq!(*target_time_ms, 0);
                assert!(*playing);
                channel.clone()
            }
            other => panic!("unexpected startup command: {other:?}"),
        };

        player.tick(&project);
        assert!(matches!(
            player.drain_video_commands().as_slice(),
            [VideoCommand::SeekAndPresent {
                target_time_ms: 100,
                playing: true,
                ..
            }]
        ));

        player.set_playing(false);
        assert_eq!(player.drain_video_commands(), [VideoCommand::PauseStreams]);
        player.set_playing(true);
        player.sync_timeline_videos(&project);
        assert!(matches!(
            player.drain_video_commands().as_slice(),
            [VideoCommand::SeekAndPresent {
                target_time_ms: 100,
                playing: true,
                ..
            }]
        ));

        player.seek(&project, 3);
        let seek = player.drain_video_commands();
        assert!(matches!(
            seek.as_slice(),
            [VideoCommand::StopChannel { channel: stopped }, VideoCommand::SeekAndPresent {
                channel: resumed,
                target_time_ms: 300,
                ..
            }] if stopped == &channel && resumed == &channel
        ));

        player.reset(&project);
        assert!(matches!(
            player.drain_video_commands().as_slice(),
            [
                VideoCommand::StopAll,
                VideoCommand::SeekAndPresent {
                    target_time_ms: 0,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn duplicate_nested_video_instances_have_distinct_full_placement_targets() {
        let mut project = runtime_video_project(VideoSyncMode::Stream);
        let clip = Symbol {
            id: "clip".into(),
            name: "Clip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![video_layer("clip-video-layer", VideoSyncMode::Stream)],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: None,
        };
        let instance = |id: &str| Element {
            id: id.into(),
            name: id.into(),
            kind: ElementKind::Instance {
                symbol_id: "clip".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        };
        project.symbols[0].timeline = Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "root-layer".into(),
                name: "Root".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![instance("left"), instance("right")],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: vec![],
            guides: vec![],
        };
        project.symbols.push(clip);
        project.validate().unwrap();

        let mut player = Player::new(&project);
        let commands = player.drain_video_commands();
        assert_eq!(commands.len(), 2);
        let mut targets = commands
            .into_iter()
            .map(|command| match command {
                VideoCommand::SeekAndPresent {
                    channel,
                    target_path,
                    ..
                } => (channel, target_path),
                other => panic!("unexpected command: {other:?}"),
            })
            .collect::<Vec<_>>();
        targets.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            targets[0].1,
            vec![String::from("left"), String::from("video")]
        );
        assert_eq!(
            targets[1].1,
            vec![String::from("right"), String::from("video")]
        );
        assert_ne!(targets[0].0, targets[1].0);
    }

    #[test]
    fn stop_sync_fires_without_a_visible_video_placement() {
        let mut project = runtime_video_project(VideoSyncMode::Stop);
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .clear();
        project.validate().unwrap();

        let mut player = Player::new(&project);
        assert_eq!(
            player.drain_video_commands(),
            [VideoCommand::Stop {
                resource: "movie".into()
            }]
        );
        player.sync_timeline_videos(&project);
        assert!(player.drain_video_commands().is_empty());
    }

    #[test]
    fn embedded_video_requires_format_9_while_non_embedded_deployment_requires_format_11() {
        // Timeline video was introduced in format 9, and the omitted deployment field defaults to
        // Embedded so existing format-9 documents remain loadable.
        let mut embedded = runtime_video_project(VideoSyncMode::Stream);
        embedded.format_version = 8;
        assert!(!embedded.uses_format_7());
        assert!(embedded.uses_format_9());
        assert!(!embedded.uses_format_11());
        let err = embedded.validate().unwrap_err().to_string();
        assert!(err.contains("require document version 9"), "got: {err}");
        embedded.upgrade_format_for_features();
        assert_eq!(embedded.format_version, 9);
        embedded.validate().unwrap();

        let mut historically_under_versioned = embedded.clone();
        historically_under_versioned.format_version = 7;
        let raw = serde_json::to_string(&historically_under_versioned).unwrap();
        let migrated = Project::from_json(&raw).expect("under-versioned video document migrates");
        assert_eq!(migrated.format_version, 9);
        migrated.validate().unwrap();

        // Omitted `deployment` in JSON defaults to `Embedded`.
        let json = embedded.to_json().unwrap();
        assert!(!json.contains("\"deployment\""));
        let round_trip = Project::from_json(&json).unwrap();
        assert_eq!(
            round_trip.symbols[0].timeline.layers[0].keyframes[0]
                .video
                .as_ref()
                .unwrap()
                .deployment,
            VideoDeploymentMode::Embedded
        );

        // External deployment mode requires format >= 11 and upgrades via feature detection.
        let mut external = runtime_video_project(VideoSyncMode::Stream);
        external.format_version = 9;
        external.resources[0].path = "assets/video/intro.mp4".into();
        external.resources[0].data_base64 = "".into();
        external.symbols[0].timeline.layers[0].keyframes[0].video = Some(
            TimelineVideoSpan::external("movie", "assets/video/intro.mp4"),
        );
        assert!(external.uses_format_9());
        assert!(external.uses_format_11());
        let err = external.validate().unwrap_err().to_string();
        assert!(err.contains("require document version 11"), "got: {err}");
        external.upgrade_format_for_features();
        assert_eq!(external.format_version, 11);
        external
            .validate()
            .expect("upgraded format-11 external video should validate");
        let mut historically_under_versioned = external.clone();
        historically_under_versioned.format_version = 7;
        let raw = serde_json::to_string(&historically_under_versioned).unwrap();
        let migrated = Project::from_json(&raw).expect("under-versioned external video migrates");
        assert_eq!(migrated.format_version, 11);
        migrated.validate().unwrap();

        // Progressive deployment mode requires format >= 11 and empty data_base64.
        let mut progressive = runtime_video_project(VideoSyncMode::Event);
        progressive.format_version = 9;
        progressive.resources[0].path = "https://cdn.example.com/media/intro.mp4".into();
        progressive.resources[0].data_base64 = "".into();
        progressive.symbols[0].timeline.layers[0].keyframes[0].video = Some(
            TimelineVideoSpan::progressive("movie", "https://cdn.example.com/media/intro.mp4"),
        );
        assert!(progressive.uses_format_11());
        progressive.upgrade_format_for_features();
        assert_eq!(progressive.format_version, 11);
        progressive
            .validate()
            .expect("upgraded format-11 progressive video should validate");
    }

    #[test]
    fn external_video_path_validation_rejects_unsafe_paths() {
        assert!(validate_external_video_path("assets/video/intro.mp4").is_ok());
        assert!(validate_external_video_path("clips/scene_01.webm").is_ok());

        for invalid in [
            "",
            "   ",
            "/Users/miles/Movies/intro.mp4",
            "C:\\Videos\\intro.mp4",
            "C:/Videos/intro.mp4",
            "../secret/intro.mp4",
            "assets/../../intro.mp4",
            "assets\\video\\intro.mp4",
            "file:///Users/miles/intro.mp4",
            "http://example.com/intro.mp4",
            "https://user:pass@example.com/intro.mp4",
            "user:pass@assets/intro.mp4",
            "assets/intro\0.mp4",
            "assets/intro\n.mp4",
        ] {
            assert!(
                validate_external_video_path(invalid).is_err(),
                "expected external path {invalid:?} to be rejected"
            );
        }

        let mut project = runtime_video_project(VideoSyncMode::Stream);
        project.format_version = 11;
        project.resources[0].data_base64 = "".into();
        project.symbols[0].timeline.layers[0].keyframes[0].video = Some(
            TimelineVideoSpan::external("movie", "/Users/miles/Desktop/clip.mp4"),
        );
        let err = project.validate().unwrap_err().to_string();
        assert!(err.contains("video.external_path"), "got: {err}");
    }

    #[test]
    fn progressive_video_url_validation_rejects_unsafe_urls_and_duplicate_bytes() {
        assert!(validate_progressive_video_url("https://cdn.example.com/video/intro.mp4").is_ok());
        assert!(
            validate_progressive_video_url("http://localhost:8080/streams/clip.webm?token=1")
                .is_ok()
        );

        for invalid in [
            "",
            "assets/video/intro.mp4",
            "file:///Users/miles/intro.mp4",
            "javascript:alert(1)",
            "data:video/mp4;base64,AAAA",
            "https://",
            "https:///path-only.mp4",
            "https://user:pass@cdn.example.com/intro.mp4",
            "https://cdn.example.com/video/../secret.mp4",
            "https://cdn.example.com/video\\intro.mp4",
            "https://cdn.example.com/video/intro\r\n.mp4",
        ] {
            assert!(
                validate_progressive_video_url(invalid).is_err(),
                "expected progressive URL {invalid:?} to be rejected"
            );
        }

        // Progressive mode must reject duplicate embedded bytes in data_base64.
        let mut project = runtime_video_project(VideoSyncMode::Event);
        project.format_version = 11;
        project.resources[0].path = "https://cdn.example.com/video/intro.mp4".into();
        // Leave non-empty data_base64 from runtime_video_project -> should fail validation!
        project.symbols[0].timeline.layers[0].keyframes[0].video = Some(
            TimelineVideoSpan::progressive("movie", "https://cdn.example.com/video/intro.mp4"),
        );
        let err = project.validate().unwrap_err().to_string();
        assert!(
            err.contains("empty data_base64"),
            "expected duplicate embedded bytes rejection, got: {err}"
        );
    }

    #[test]
    fn video_manifest_and_external_asset_helpers_distinguish_all_three_modes() {
        let mut project = runtime_video_project(VideoSyncMode::Stream);
        project.format_version = 11;
        project.resources.push(
            ResourceUnit::from_bytes(
                "ext_vid",
                "assets/video/cutscene.mp4",
                "video/mp4",
                b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2avc1mp41",
            )
            .unwrap(),
        );
        let mut prog_res =
            ResourceUnit::from_bytes("prog_vid", "video/stream.webm", "video/webm", b"x").unwrap();
        prog_res.path = "https://cdn.example.com/live/stream.webm".into();
        prog_res.data_base64 = "".into();
        project.resources.push(prog_res);

        project.symbols[0].timeline.layers.push(Layer {
            id: "ext-layer".into(),
            name: "External".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![video_element("video-ext")],
                video: Some(TimelineVideoSpan::external(
                    "ext_vid",
                    "assets/video/cutscene.mp4",
                )),
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        });
        project.symbols[0].timeline.layers.push(Layer {
            id: "prog-layer".into(),
            name: "Progressive".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![video_element("video-prog")],
                video: Some(TimelineVideoSpan::progressive(
                    "prog_vid",
                    "https://cdn.example.com/live/stream.webm",
                )),
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        });

        let extracted = project.strip_non_embedded_video_bytes();
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].resource_id, "ext_vid");
        assert_eq!(extracted[0].relative_path, "assets/video/cutscene.mp4");
        assert!(!extracted[0].bytes.is_empty());
        assert!(project.resource("ext_vid").unwrap().data_base64.is_empty());
        assert!(!project.resource("movie").unwrap().data_base64.is_empty());

        project.validate().unwrap();

        let manifest = project.video_manifest_entries();
        assert_eq!(manifest.len(), 3);
        let by_id: std::collections::HashMap<_, _> = manifest
            .into_iter()
            .map(|e| (e.resource_id.clone(), e))
            .collect();
        assert_eq!(by_id["movie"].source.mode, VideoDeploymentMode::Embedded);
        assert!(by_id["movie"].embedded_in_bundle);
        assert!(by_id["movie"].bundled_byte_len > 0);
        assert_eq!(by_id["ext_vid"].source.mode, VideoDeploymentMode::External);
        assert_eq!(
            by_id["ext_vid"].source.url_or_path,
            "assets/video/cutscene.mp4"
        );
        assert!(by_id["ext_vid"].requires_external_file);
        assert_eq!(by_id["ext_vid"].bundled_byte_len, 0);
        assert_eq!(
            by_id["prog_vid"].source.mode,
            VideoDeploymentMode::Progressive
        );
        assert_eq!(
            by_id["prog_vid"].source.url_or_path,
            "https://cdn.example.com/live/stream.webm"
        );
        assert!(by_id["prog_vid"].requires_network_cors);
        assert_eq!(by_id["prog_vid"].bundled_byte_len, 0);

        let mut player = Player::new(&project);
        let commands = player.drain_video_commands();
        assert_eq!(commands.len(), 3);
        let sources: Vec<_> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                VideoCommand::SeekAndPresent { source, .. }
                | VideoCommand::StartEvent { source, .. } => source.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(sources.len(), 3);
        assert!(
            sources
                .iter()
                .any(|s| s.mode == VideoDeploymentMode::Embedded)
        );
        assert!(sources.iter().any(|s| {
            s.mode == VideoDeploymentMode::External && s.url_or_path == "assets/video/cutscene.mp4"
        }));
        assert!(sources.iter().any(|s| {
            s.mode == VideoDeploymentMode::Progressive
                && s.url_or_path == "https://cdn.example.com/live/stream.webm"
        }));
    }

    #[test]
    fn mixed_deployment_of_one_resource_is_lossless_in_either_layer_order() {
        let make_project = |reverse_layers: bool| {
            let mut project = runtime_video_project(VideoSyncMode::Stream);
            project.format_version = 11;
            project.symbols[0].timeline.layers.push(Layer {
                id: "external-layer".into(),
                name: "External".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![video_element("external-video")],
                    video: Some(TimelineVideoSpan::external(
                        "movie",
                        "assets/video/movie.mp4",
                    )),
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            });
            project.symbols[0].timeline.layers.push(Layer {
                id: "progressive-layer".into(),
                name: "Progressive".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![video_element("progressive-video")],
                    video: Some(TimelineVideoSpan::progressive(
                        "movie",
                        "https://cdn.example.com/video/movie.mp4",
                    )),
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            });
            if reverse_layers {
                project.symbols[0].timeline.layers.reverse();
            }
            project.validate().unwrap();
            project
        };

        let canonical_sources = |project: &Project| {
            let mut sources = timeline_video_sources(project)
                .into_iter()
                .map(|(id, source)| {
                    (
                        id,
                        source.mode.as_str().to_owned(),
                        source.url_or_path,
                        source.mime_type,
                    )
                })
                .collect::<Vec<_>>();
            sources.sort();
            sources
        };

        let forward = make_project(false);
        let reverse = make_project(true);
        let expected = canonical_sources(&forward);
        assert_eq!(expected, canonical_sources(&reverse));
        assert_eq!(expected.len(), 3);

        for mut project in [forward, reverse] {
            let manifest = video_manifest_entries(&project);
            assert_eq!(manifest.len(), 3);
            assert!(manifest.iter().any(|entry| entry.embedded_in_bundle));
            assert!(manifest.iter().any(|entry| entry.requires_external_file));
            assert!(manifest.iter().any(|entry| entry.requires_network_cors));

            let external_assets = strip_non_embedded_video_bytes(&mut project);
            assert_eq!(external_assets.len(), 1);
            assert_eq!(external_assets[0].relative_path, "assets/video/movie.mp4");
            assert!(!external_assets[0].bytes.is_empty());
            assert!(
                !project.resource("movie").unwrap().data_base64.is_empty(),
                "Embedded use must retain the shared resource bytes"
            );
            project.validate().unwrap();
        }

        let mut external_and_progressive_only = make_project(false);
        external_and_progressive_only.symbols[0]
            .timeline
            .layers
            .retain(|layer| layer.id != "video-layer");
        external_and_progressive_only.validate().expect(
            "External use requires the shared bytes even when Progressive also uses the resource",
        );
        let external_assets = strip_non_embedded_video_bytes(&mut external_and_progressive_only);
        assert_eq!(external_assets.len(), 1);
        assert!(!external_assets[0].bytes.is_empty());
        assert!(
            external_and_progressive_only
                .resource("movie")
                .unwrap()
                .data_base64
                .is_empty(),
            "published project JSON must strip bytes after capturing the External asset"
        );
        external_and_progressive_only.validate().unwrap();
    }
}
