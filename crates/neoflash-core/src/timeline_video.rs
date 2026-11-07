//! Authored timeline video scheduling, cue-point evaluation and container/codec diagnostics (`T-118`).
//!
//! Following NeoFlash's media architecture (`timeline_sound.rs`), Rust owns all timeline
//! synchronization, frame-to-time mapping, loop wrapping, cue-point crossing and container/CORS
//! diagnostics. Hosts (native wgpu/egui and HTML5 Canvas/WebGL) only decode and present frames.

use crate::Id;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

fn serialize_static_code<S: Serializer>(code: &&'static str, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(code)
}

fn deserialize_static_code<'de, D: Deserializer<'de>>(deserializer: D) -> Result<&'static str, D::Error> {
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
        layer_id: Id,
        resource: Id,
        target_time_ms: u32,
        playing: bool,
        muted: bool,
    },
    /// Trigger an independent event video on keyframe entry.
    StartEvent {
        layer_id: Id,
        resource: Id,
        start_offset_ms: u32,
        loop_video: bool,
        muted: bool,
    },
    /// Stop playback of `resource`.
    Stop { resource: Id },
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
        assert!(diags_ok.is_empty(), "expected no errors for H.264 MP4, got {diags_ok:?}");

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
}
