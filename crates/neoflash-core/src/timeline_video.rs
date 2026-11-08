//! Authored timeline video scheduling, cue-point evaluation and container/codec diagnostics (`T-118`).
//!
//! Following NeoFlash's media architecture (`timeline_sound.rs`), Rust owns all timeline
//! synchronization, frame-to-time mapping, loop wrapping, cue-point crossing and container/CORS
//! diagnostics. Hosts (native wgpu/egui and HTML5 Canvas/WebGL) only decode and present frames.

use crate::evaluate::{DrawItem, Node};
use crate::{ElementKind, Id, Keyframe, Layer, LayerRole, Project};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};

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

/// Authored timeline video settings attached to a keyframe or video layer span (`T-118`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineVideoSpan {
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
        }
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

/// Host instruction emitted by Rust's timeline video scheduler (`T-118`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum VideoCommand {
    /// Seek a timeline-locked stream video to `target_time_ms` and present its decoded frame.
    SeekAndPresent {
        channel: String,
        target_path: Vec<Id>,
        layer_id: Id,
        resource: Id,
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
                                resource: active.span.resource.clone(),
                                cue_point,
                                frame: active.owner.frame,
                            });
                        }
                    }
                    self.push(VideoCommand::SeekAndPresent {
                        channel: channel.clone(),
                        target_path: item.instance_path.clone(),
                        layer_id: active.layer.id.clone(),
                        resource: active.span.resource.clone(),
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
                            resource: active.span.resource.clone(),
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
                    self.push(VideoCommand::Stop {
                        resource: span.resource.clone(),
                    });
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
}
