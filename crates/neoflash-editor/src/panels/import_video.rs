//! Import video modal dialog and live floating body (NF2 `Dialog / Import video`, `DL-07`,
//! `W3.5`; reference `dialog-import-video.png`), opened from `File ▸ Import ▸ Import video…`
//! and `Window ▸ Panels ▸ Planned panels ▸ Import video`.

use super::dialog_helpers::{group, preview_box};
use super::planned::{FooterClick, caption, footer_clicked};
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_core::{
    Element, ElementKind, Id, Keyframe, ResourceUnit, SymbolKind, TimelineVideoSpan, Transform,
    VideoDeploymentMode, VideoSyncMode, new_id, validate_external_video_path,
    validate_progressive_video_url,
};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::tokens::space;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

enum ProgressivePreviewFetch {
    Queued {
        mime_type: String,
    },
    Loading(mpsc::Receiver<Result<(), String>>),
    Failed {
        message: String,
        reported: bool,
        retry_at: Instant,
    },
}

const MAX_PROGRESSIVE_PREVIEW_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROGRESSIVE_PREVIEW_FETCHES: usize = 4;
const MAX_PROGRESSIVE_PREVIEW_STATES: usize = 32;
const PROGRESSIVE_PREVIEW_RETRY_DELAY: Duration = Duration::from_secs(5);

fn failed_progressive_preview(message: String) -> ProgressivePreviewFetch {
    ProgressivePreviewFetch::Failed {
        message,
        reported: false,
        retry_at: Instant::now() + PROGRESSIVE_PREVIEW_RETRY_DELAY,
    }
}

fn read_progressive_preview_file(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_PROGRESSIVE_PREVIEW_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROGRESSIVE_PREVIEW_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_PROGRESSIVE_PREVIEW_BYTES).then_some(bytes)
}

fn progressive_preview_fetches()
-> &'static Mutex<std::collections::HashMap<String, ProgressivePreviewFetch>> {
    static FETCHES: OnceLock<Mutex<std::collections::HashMap<String, ProgressivePreviewFetch>>> =
        OnceLock::new();
    FETCHES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn fetch_progressive_preview_bytes(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(test)]
    {
        neoflash_export::fetch_progressive_video_bytes_allowing_private_for_tests(url)
    }
    #[cfg(not(test))]
    {
        neoflash_export::fetch_progressive_video_bytes(url)
    }
}

fn start_progressive_preview_fetch(
    url: &str,
    mime_type: &str,
) -> Result<mpsc::Receiver<Result<(), String>>, String> {
    let (sender, receiver) = mpsc::channel();
    let worker_url = url.to_owned();
    let worker_mime = mime_type.to_owned();
    std::thread::Builder::new()
        .name("neoflash-progressive-preview".into())
        .spawn(move || {
            let result = fetch_progressive_preview_bytes(&worker_url).and_then(|bytes| {
                neoflash_export::decode_video_stream_bytes(
                    "downloaded Progressive preview",
                    &worker_mime,
                    &bytes,
                )
                .map(|_| {
                    neoflash_export::stage_downloaded_progressive_video_bytes(
                        worker_url.clone(),
                        bytes,
                    );
                })
                .map_err(|error| {
                    format!("Downloaded Progressive preview could not be decoded: {error}")
                })
            });
            let _ = sender.send(result);
        })
        .map_err(|error| format!("Could not start Progressive preview download: {error}"))?;
    Ok(receiver)
}

fn poll_or_start_progressive_preview_fetch(url: &str, mime_type: &str) -> bool {
    if neoflash_export::lookup_staged_progressive_video_bytes(url).is_some() {
        if let Ok(mut fetches) = progressive_preview_fetches().lock() {
            fetches.remove(url);
        }
        return false;
    }
    let Ok(mut fetches) = progressive_preview_fetches().lock() else {
        return false;
    };

    // Reap every completed worker before enforcing the process-wide concurrency limit. A dialog
    // can close or switch URLs while its worker is running; without this sweep those completed,
    // now-offscreen jobs would permanently consume all four download slots.
    let completed = fetches
        .iter_mut()
        .filter_map(|(key, state)| {
            let ProgressivePreviewFetch::Loading(receiver) = state else {
                return None;
            };
            match receiver.try_recv() {
                Ok(result) => Some((key.clone(), result)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    key.clone(),
                    Err("Progressive preview worker stopped before returning a result.".to_owned()),
                )),
            }
        })
        .collect::<Vec<_>>();
    for (key, result) in completed {
        match result {
            Ok(()) => {
                fetches.remove(&key);
            }
            Err(message) => {
                fetches.insert(key, failed_progressive_preview(message));
            }
        }
    }
    // A worker may have staged its validated bytes after the lock-free check above but before the
    // completion sweep. Recheck here so that successful completion cannot start a duplicate GET.
    if neoflash_export::lookup_staged_progressive_video_bytes(url).is_some() {
        fetches.remove(url);
        return false;
    }
    let active_fetches = fetches
        .values()
        .filter(|state| matches!(state, ProgressivePreviewFetch::Loading(_)))
        .count();
    if matches!(
        fetches.get(url),
        Some(ProgressivePreviewFetch::Queued { .. })
    ) {
        if active_fetches >= MAX_PROGRESSIVE_PREVIEW_FETCHES {
            return true;
        }
        let queued_mime = match fetches.remove(url) {
            Some(ProgressivePreviewFetch::Queued { mime_type }) => mime_type,
            _ => mime_type.to_owned(),
        };
        match start_progressive_preview_fetch(url, &queued_mime) {
            Ok(receiver) => {
                fetches.insert(url.to_owned(), ProgressivePreviewFetch::Loading(receiver));
                return true;
            }
            Err(message) => {
                fetches.insert(url.to_owned(), failed_progressive_preview(message));
                return false;
            }
        }
    }
    let retry_failed = fetches.get(url).is_some_and(|state| {
        matches!(
            state,
            ProgressivePreviewFetch::Failed {
                reported: true,
                retry_at,
                ..
            } if Instant::now() >= *retry_at
        )
    });
    if retry_failed {
        // Keep a surfaced transient error from poisoning this URL for the rest of the process.
        // The delay prevents a fast failing endpoint from spawning a worker every UI frame.
        fetches.remove(url);
    }
    if let Some(state) = fetches.get_mut(url) {
        match state {
            ProgressivePreviewFetch::Loading(receiver) => match receiver.try_recv() {
                Ok(Ok(())) => {
                    fetches.remove(url);
                    return false;
                }
                Ok(Err(message)) => {
                    *state = failed_progressive_preview(message);
                    return false;
                }
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    *state = failed_progressive_preview(
                        "Progressive preview worker stopped before returning a result.".into(),
                    );
                    return false;
                }
            },
            ProgressivePreviewFetch::Queued { .. } => unreachable!("queued state handled above"),
            ProgressivePreviewFetch::Failed { .. } => return false,
        }
    }

    // Bound retained states while never discarding an active transfer. Queued entries preserve
    // eventual loading without spawning an unbounded thread per URL pasted into the dialog.
    if fetches.len() >= MAX_PROGRESSIVE_PREVIEW_STATES
        && let Some(evict) = fetches
            .iter()
            .find(|(_, state)| !matches!(state, ProgressivePreviewFetch::Loading(_)))
            .map(|(key, _)| key.clone())
    {
        fetches.remove(&evict);
    }
    let url_owned = url.to_owned();
    if active_fetches >= MAX_PROGRESSIVE_PREVIEW_FETCHES {
        fetches.insert(
            url_owned,
            ProgressivePreviewFetch::Queued {
                mime_type: mime_type.to_owned(),
            },
        );
        return true;
    }
    match start_progressive_preview_fetch(url, mime_type) {
        Ok(receiver) => {
            fetches.insert(url_owned, ProgressivePreviewFetch::Loading(receiver));
            true
        }
        Err(message) => {
            fetches.insert(url_owned, failed_progressive_preview(message));
            false
        }
    }
}

fn take_progressive_preview_error(url: &str) -> Option<String> {
    let mut fetches = progressive_preview_fetches().lock().ok()?;
    let ProgressivePreviewFetch::Failed {
        message, reported, ..
    } = fetches.get_mut(url)?
    else {
        return None;
    };
    if *reported {
        None
    } else {
        *reported = true;
        Some(message.clone())
    }
}

fn progressive_preview_mime(path: &str, codec: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".webm") || codec.contains("VP9") {
        "video/webm"
    } else if lower.ends_with(".ogv") || lower.ends_with(".ogg") {
        "video/ogg"
    } else {
        "video/mp4"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImportVideoDraft {
    pub(crate) file_path: String,
    pub(crate) in_time: String,
    pub(crate) out_time: String,
    /// 0 = Embed in timeline, 1 = Load external video, 2 = Progressive download
    pub(crate) deployment: usize,
    pub(crate) symbol_kind: SymbolKind,
    pub(crate) codec: String,
    pub(crate) place_on_stage: bool,
    pub(crate) expand_timeline: bool,
    pub(crate) include_audio: bool,
    pub(crate) match_fps: bool,
    pub(crate) source_fps: u32,
}

impl Default for ImportVideoDraft {
    fn default() -> Self {
        Self {
            file_path: "~/Movies/intro.mp4".to_owned(),
            in_time: "00:00.00".to_owned(),
            out_time: "00:04.00".to_owned(),
            deployment: 0,
            symbol_kind: SymbolKind::MovieClip,
            codec: "H.264".to_owned(),
            place_on_stage: true,
            expand_timeline: true,
            include_audio: true,
            match_fps: false,
            source_fps: 30,
        }
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-import-video-draft")
}

fn inline_draft_id() -> egui::Id {
    egui::Id::new("nf2-import-video-inline-draft")
}

pub(crate) fn has_import_video_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<ImportVideoDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn import_video_draft(ctx: &egui::Context) -> Option<ImportVideoDraft> {
    ctx.data(|d| d.get_temp::<ImportVideoDraft>(draft_id()))
        .or_else(|| ctx.data(|d| d.get_temp::<ImportVideoDraft>(inline_draft_id())))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn update_import_video_draft(
    ctx: &egui::Context,
    mut f: impl FnMut(&mut ImportVideoDraft),
) {
    for id in [draft_id(), inline_draft_id()] {
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<ImportVideoDraft>(id)) {
            f(&mut draft);
            ctx.data_mut(|d| d.insert_temp(id, draft));
        }
    }
}

#[cfg(test)]
pub(crate) fn set_inline_import_video_draft(ctx: &egui::Context, draft: ImportVideoDraft) {
    ctx.data_mut(|data| data.insert_temp(inline_draft_id(), draft));
}

pub(crate) fn parse_timecode_seconds(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if let Some((mins, secs)) = trimmed.split_once(':') {
        let m: f64 = mins.trim().parse().ok()?;
        let s: f64 = secs.trim().parse().ok()?;
        if m >= 0.0 && s >= 0.0 {
            return Some(m * 60.0 + s);
        }
    }
    trimmed.parse::<f64>().ok().filter(|v| *v >= 0.0)
}

pub(crate) fn format_timecode_seconds(seconds: f64) -> String {
    let clamped = seconds.max(0.0);
    let mins = (clamped / 60.0).floor() as u32;
    let rem = clamped - f64::from(mins) * 60.0;
    format!("{mins:02}:{rem:05.2}")
}

pub(crate) fn probe_video_bytes_duration_ms(bytes: &[u8]) -> Option<u32> {
    neoflash_core::probe_video_bytes_duration_ms(bytes)
}

fn probe_video_duration_seconds_with_base(
    raw_path: &str,
    deployment: usize,
    base_dir: Option<&Path>,
) -> Option<f64> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if deployment == 2 {
        if let Some(staged) = neoflash_export::lookup_staged_progressive_video_bytes(trimmed) {
            return probe_video_bytes_duration_ms(&staged).map(|ms| f64::from(ms) / 1000.0);
        }
        if let Some(local_path) =
            neoflash_export::resolve_progressive_local_path(trimmed, None, base_dir)
            && let Ok(meta) = std::fs::metadata(&local_path)
            && meta.len() > 0
            && meta.len() <= neoflash_core::MAX_RESOURCE_BYTES as u64
            && let Ok(bytes) = std::fs::read(&local_path)
        {
            return probe_video_bytes_duration_ms(&bytes).map(|ms| f64::from(ms) / 1000.0);
        }
        return None;
    }
    let path = expand_import_path(trimmed);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 || meta.len() > neoflash_core::MAX_RESOURCE_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    probe_video_bytes_duration_ms(&bytes).map(|ms| f64::from(ms) / 1000.0)
}

fn expand_import_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(trimmed)
}

fn unique_video_resource_path(resources: &[ResourceUnit], preferred: &str, bytes: &[u8]) -> String {
    if !resources
        .iter()
        .any(|resource| resource.path.eq_ignore_ascii_case(preferred))
    {
        return preferred.to_owned();
    }
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let (stem, extension) = preferred
        .rsplit_once('.')
        .filter(|(stem, _)| stem.rsplit('/').next().is_some_and(|name| !name.is_empty()))
        .map_or((preferred, ""), |(stem, extension)| (stem, extension));
    for sequence in 0_u32.. {
        let suffix = if sequence == 0 {
            format!("-{hash:016x}")
        } else {
            format!("-{hash:016x}-{sequence}")
        };
        let candidate = if extension.is_empty() {
            format!("{stem}{suffix}")
        } else {
            format!("{stem}{suffix}.{extension}")
        };
        if !resources
            .iter()
            .any(|resource| resource.path.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    unreachable!("the finite resource list always leaves a unique path")
}

impl Editor {
    /// Open the `Import video` modal dialog (`DL-07`).
    pub(crate) fn open_import_video(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        ctx.data_mut(|d| d.insert_temp(draft_id(), ImportVideoDraft::default()));
    }

    pub(crate) fn cancel_import_video(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.remove::<ImportVideoDraft>(draft_id());
            data.remove::<ImportVideoDraft>(inline_draft_id());
        });
    }

    /// Execute video import from the active `Import video` draft in one `"Import video"` Undo step.
    pub(crate) fn confirm_import_video(&mut self, ctx: &egui::Context) -> Result<Id, String> {
        let draft = ctx
            .data(|d| d.get_temp::<ImportVideoDraft>(draft_id()))
            .or_else(|| ctx.data(|d| d.get_temp::<ImportVideoDraft>(inline_draft_id())))
            .unwrap_or_default();
        self.import_video_draft(ctx, draft, None)
    }

    /// Import a Finder-dropped video immediately at the authored pointer coordinate. Native drops
    /// use the same validated resource/timeline path as the dialog, but do not require a second
    /// modal confirmation after the user has already dropped the file on the Stage.
    pub(crate) fn import_video_path_at(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        point: [f64; 2],
    ) -> Result<Id, String> {
        let draft = ImportVideoDraft {
            file_path: path.display().to_string(),
            ..ImportVideoDraft::default()
        };
        self.import_video_draft(ctx, draft, Some(point))
    }

    fn import_video_draft(
        &mut self,
        ctx: &egui::Context,
        draft: ImportVideoDraft,
        placement: Option<[f64; 2]>,
    ) -> Result<Id, String> {
        if !self.layer_editable() {
            return Err("Select an unlocked, visible layer before importing video.".into());
        }

        let raw_input = draft.file_path.trim();
        if raw_input.is_empty() {
            return Err("Choose a video file or URL to import.".into());
        }

        let base_dir = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let probed_duration_sec = probe_video_duration_seconds_with_base(
            raw_input,
            draft.deployment,
            base_dir.as_deref(),
        );
        let default_out_sec = probed_duration_sec.unwrap_or(4.0).max(0.1);
        let in_sec = parse_timecode_seconds(&draft.in_time)
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
            .clamp(0.0, 36_000.0);
        let raw_out_sec = if draft.out_time.trim() == "00:04.00" {
            probed_duration_sec.unwrap_or(4.0)
        } else {
            parse_timecode_seconds(&draft.out_time)
                .filter(|v| v.is_finite())
                .unwrap_or(default_out_sec)
        };
        let out_sec = raw_out_sec.clamp(in_sec + 0.1, in_sec + 600.0);
        let duration_sec = (out_sec - in_sec).clamp(0.1, 600.0);
        let start_offset_ms = (in_sec * 1000.0).round().max(0.0) as u32;
        let explicit_trim = start_offset_ms > 0
            || draft.out_time.trim() != "00:04.00"
            || probed_duration_sec.is_some_and(|src| out_sec + 0.005 < src);
        let end_offset_ms = explicit_trim.then(|| {
            (out_sec * 1000.0)
                .round()
                .max(f64::from(start_offset_ms + 1)) as u32
        });

        let mut candidate = self.project.clone();
        let (res_id, rel_path, file_name, resource, video_span) = if draft.deployment == 2 {
            validate_progressive_video_url(raw_input)?;
            let url_no_query = raw_input.split(['?', '#']).next().unwrap_or(raw_input);
            let url_file_name = url_no_query
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty() && name.contains('.'))
                .unwrap_or("stream.mp4");
            let ext = Path::new(url_file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4")
                .to_ascii_lowercase();
            let media_type = if ext == "webm" || draft.codec.contains("VP9") {
                "video/webm"
            } else if ext == "ogv" || ext == "ogg" {
                "video/ogg"
            } else {
                "video/mp4"
            };
            let preferred_path = format!("video/{url_file_name}");
            let rel_path = unique_video_resource_path(
                &candidate.resources,
                &preferred_path,
                raw_input.as_bytes(),
            );
            let res_id = new_id("res");
            let resource = ResourceUnit {
                id: res_id.clone(),
                path: rel_path.clone(),
                media_type: media_type.to_owned(),
                data_base64: String::new().into(),
                bitmap_export: None,
                audio_export: None,
            };
            let span = TimelineVideoSpan {
                resource: res_id.clone(),
                sync: VideoSyncMode::Event,
                deployment: VideoDeploymentMode::Progressive,
                external_path: None,
                stream_url: Some(raw_input.to_owned()),
                start_offset_ms,
                end_offset_ms,
                loop_video: false,
                muted: !draft.include_audio,
                cue_points: Vec::new(),
            };
            (
                res_id,
                rel_path,
                url_file_name.to_owned(),
                Some(resource),
                span,
            )
        } else {
            let expanded_path = expand_import_path(raw_input);
            let path_obj = expanded_path.as_path();
            let file = std::fs::File::open(path_obj).map_err(|error| {
                format!("Could not read video '{}': {error}", path_obj.display())
            })?;
            let byte_limit = neoflash_core::MAX_RESOURCE_BYTES;
            let file_len = file
                .metadata()
                .map_err(|error| {
                    format!("Could not inspect video '{}': {error}", path_obj.display())
                })?
                .len();
            if file_len > byte_limit as u64 {
                return Err(format!(
                    "Video '{}' exceeds the {} MiB resource limit.",
                    path_obj.display(),
                    byte_limit / (1024 * 1024)
                ));
            }
            let mut raw_bytes = Vec::with_capacity(file_len.min(byte_limit as u64) as usize);
            file.take(byte_limit as u64 + 1)
                .read_to_end(&mut raw_bytes)
                .map_err(|error| {
                    format!("Could not read video '{}': {error}", path_obj.display())
                })?;
            if raw_bytes.len() > byte_limit {
                return Err(format!(
                    "Video '{}' exceeds the {} MiB resource limit.",
                    path_obj.display(),
                    byte_limit / (1024 * 1024)
                ));
            }
            let ext = path_obj
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4")
                .to_ascii_lowercase();
            let media_type = if raw_bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
                "video/webm"
            } else if raw_bytes.starts_with(b"OggS") {
                "video/ogg"
            } else if neoflash_core::is_avi_bytes(&raw_bytes) {
                "video/x-msvideo"
            } else if raw_bytes.len() >= 12 && &raw_bytes[4..8] == b"ftyp" {
                "video/mp4"
            } else if ext == "webm" {
                "video/webm"
            } else if ext == "ogv" || ext == "ogg" {
                "video/ogg"
            } else if ext == "avi" {
                "video/x-msvideo"
            } else {
                "video/mp4"
            };
            let file_name = path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("intro.mp4")
                .to_owned();
            let diagnostics =
                neoflash_core::inspect_video_resource(&file_name, media_type, &raw_bytes, None);
            let errors: Vec<_> = diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.is_error)
                .map(|diagnostic| diagnostic.message.as_str())
                .collect();
            if !errors.is_empty() {
                return Err(errors.join(" "));
            }
            let preferred_path = format!("video/{file_name}");
            let identical = candidate.resources.iter().find(|resource| {
                resource.is_video()
                    && resource.media_type.eq_ignore_ascii_case(media_type)
                    && resource
                        .decoded_bytes()
                        .is_ok_and(|existing| existing.as_slice() == raw_bytes.as_slice())
            });
            let (res_id, rel_path, resource) = if let Some(existing) = identical {
                (existing.id.clone(), existing.path.clone(), None)
            } else {
                let rel_path =
                    unique_video_resource_path(&candidate.resources, &preferred_path, &raw_bytes);
                let res_id = new_id("res");
                let resource = ResourceUnit::from_bytes(
                    res_id.clone(),
                    rel_path.clone(),
                    media_type,
                    &raw_bytes,
                )
                .map_err(|e| e.to_string())?;
                (res_id, rel_path, Some(resource))
            };
            let (deployment, sync, external_path) = if draft.deployment == 1 {
                validate_external_video_path(&rel_path)?;
                (
                    VideoDeploymentMode::External,
                    VideoSyncMode::Event,
                    Some(rel_path.clone()),
                )
            } else {
                (VideoDeploymentMode::Embedded, VideoSyncMode::Stream, None)
            };
            let span = TimelineVideoSpan {
                resource: res_id.clone(),
                sync,
                deployment,
                external_path,
                stream_url: None,
                start_offset_ms,
                end_offset_ms,
                loop_video: false,
                muted: !draft.include_audio,
                cue_points: Vec::new(),
            };
            (res_id, rel_path, file_name, resource, span)
        };

        let layer_idx = self.layer;
        let frame = self.frame;
        let symbol_id = self.symbol_id().to_owned();
        if let Some(resource) = resource {
            candidate.resources.push(resource);
        }
        candidate.format_version = candidate.format_version.max(3);
        if draft.match_fps {
            candidate.fps = draft.source_fps.clamp(1, 120);
        }
        let fps = candidate.fps.max(1);
        let needed_frames =
            frame.saturating_add(((duration_sec * f64::from(fps)).ceil() as u32).max(1));
        let symbol = candidate
            .symbol_mut(&symbol_id)
            .ok_or_else(|| "Active symbol not found.".to_string())?;
        if draft.expand_timeline && symbol.timeline.frame_count < needed_frames {
            symbol.timeline.frame_count = needed_frames;
        }
        let max_tl_frames = symbol.timeline.frame_count;
        let layer = symbol
            .timeline
            .layers
            .get_mut(layer_idx)
            .ok_or_else(|| "Selected layer was not found.".to_string())?;
        if draft.expand_timeline
            && let Some(end) = &mut layer.properties.end_frame
            && *end < needed_frames.min(max_tl_frames)
        {
            *end = needed_frames.min(max_tl_frames);
        }
        if !layer.keyframes.iter().any(|key| key.frame == frame) {
            let previous_elements = layer
                .key_at(frame)
                .map(|key| key.elements.clone())
                .unwrap_or_default();
            layer.keyframes.push(Keyframe {
                frame,
                elements: previous_elements,
                ..Keyframe::default()
            });
            layer.keyframes.sort_by_key(|key| key.frame);
        }
        let key = layer
            .keyframes
            .iter_mut()
            .find(|key| key.frame == frame)
            .ok_or_else(|| "Could not create the destination video keyframe.".to_string())?;
        // A motion-span property keyframe (◆) cannot carry a timeline video span; convert it to an
        // ordinary keyframe so validation and motion normalization stay consistent.
        key.keyed_properties = None;
        key.video = Some(video_span);
        if draft.place_on_stage {
            let center = placement.unwrap_or([240.0, 180.0]);
            key.elements.push(Element {
                id: new_id("video"),
                name: file_name.to_owned(),
                transform: Transform {
                    x: center[0] - 160.0,
                    y: center[1] - 120.0,
                    ..Transform::default()
                },
                kind: ElementKind::Video {
                    width: 320.0,
                    height: 240.0,
                },
                ..Element::default()
            });
        }
        layer.normalize_motion_keys();
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|error| error.to_string())?;

        self.finish_edits();
        self.transact_named("Import video", |editor| {
            editor.project = candidate;
        });

        ctx.data_mut(|data| {
            data.remove::<ImportVideoDraft>(draft_id());
            data.remove::<ImportVideoDraft>(inline_draft_id());
        });
        self.message(format!("Imported video {rel_path}."));
        Ok(res_id)
    }

    /// Decode the active `Import video` preview frame at `scrub_time_ms` (or `draft.in_time` when
    /// `None`) for `Embedded`, `External`, and `Progressive` video sources (`T-274`).
    pub(crate) fn decode_import_video_preview_frame(
        &mut self,
        draft: &ImportVideoDraft,
        scrub_time_ms: Option<u32>,
    ) -> Option<neoflash_export::DecodedVideoFrame> {
        let raw_input = draft.file_path.trim();
        if raw_input.is_empty() {
            return None;
        }
        let base_dir = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let mime_type = progressive_preview_mime(raw_input, &draft.codec);

        let stream = if draft.deployment == 2 {
            if let Some(staged) = neoflash_export::lookup_staged_progressive_video_bytes(raw_input)
            {
                match neoflash_export::decode_video_stream_bytes(
                    "staged Progressive preview",
                    mime_type,
                    &staged,
                ) {
                    Ok(stream) => stream,
                    Err(error) => {
                        neoflash_export::discard_staged_progressive_video_bytes(raw_input);
                        self.message(format!(
                            "Staged Progressive preview could not be decoded and was discarded: {error}"
                        ));
                        return None;
                    }
                }
            } else if let Some(local_path) = neoflash_export::resolve_progressive_local_path(
                raw_input,
                None,
                base_dir.as_deref(),
            ) {
                let bytes = read_progressive_preview_file(&local_path)?;
                match neoflash_export::decode_video_stream_bytes(
                    &local_path.to_string_lossy(),
                    mime_type,
                    &bytes,
                ) {
                    Ok(stream) => stream,
                    Err(error) => {
                        self.message(format!(
                            "Progressive preview file '{}' could not be decoded: {error}",
                            local_path.display()
                        ));
                        return None;
                    }
                }
            } else if validate_progressive_video_url(raw_input).is_ok() {
                poll_or_start_progressive_preview_fetch(raw_input, mime_type);
                return None;
            } else {
                return None;
            }
        } else {
            let expanded = expand_import_path(raw_input);
            let candidate = if expanded.is_file() {
                Some(expanded)
            } else {
                base_dir
                    .as_deref()
                    .map(|dir| dir.join(raw_input))
                    .filter(|p| p.is_file())
            };
            if let Some(path) = candidate {
                let bytes = std::fs::read(&path).ok()?;
                neoflash_export::decode_video_stream_bytes(raw_input, mime_type, &bytes).ok()?
            } else {
                let staged = neoflash_export::lookup_staged_progressive_video_bytes(raw_input)?;
                neoflash_export::decode_video_stream_bytes(raw_input, mime_type, &staged).ok()?
            }
        };

        let in_ms = (parse_timecode_seconds(&draft.in_time).unwrap_or(0.0) * 1000.0)
            .round()
            .max(0.0) as u32;
        let out_ms = parse_timecode_seconds(&draft.out_time)
            .map(|s| (s * 1000.0).round().max(f64::from(in_ms + 1)) as u32);
        let time_ms = scrub_time_ms.unwrap_or(in_ms);
        let placement = neoflash_core::ResolvedVideoPlacement {
            channel: "import-preview".into(),
            target_path: vec!["preview".into()],
            layer_id: "preview".into(),
            resource: "preview".into(),
            source: None,
            sync: VideoSyncMode::Stream,
            time_ms,
            in_point_ms: in_ms,
            out_point_ms: out_ms,
            playing: false,
            loop_video: false,
            muted: !draft.include_audio,
            stopped: false,
        };
        stream.frame_at_placement(&placement).map(|(_, f)| f)
    }

    pub(crate) fn import_video_body(
        &mut self,
        ui: &mut Ui,
        draft: &mut ImportVideoDraft,
    ) -> (bool, bool) {
        let mut close_cancel = false;
        let mut do_import = false;

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                if let Some(decoded_frame) = self.decode_import_video_preview_frame(draft, None) {
                    let p = neoflash_ui::palette(ui.ctx());
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(300.0, 169.0), egui::Sense::hover());
                    ui.painter().rect(
                        rect,
                        neoflash_ui::tokens::radius::SM,
                        p.bg_canvas,
                        egui::Stroke::new(1.0_f32, p.border_subtle),
                        egui::StrokeKind::Inside,
                    );
                    let w = decoded_frame.image.width.max(1) as usize;
                    let h = decoded_frame.image.height.max(1) as usize;
                    if decoded_frame.image.rgba.len() >= w * h * 4 {
                        let color_img = egui::ColorImage::from_rgba_unmultiplied(
                            [w, h],
                            &decoded_frame.image.rgba[..w * h * 4],
                        );
                        let tex = ui.ctx().load_texture(
                            "import-video-live-preview",
                            color_img,
                            egui::TextureOptions::LINEAR,
                        );
                        let inner = rect.shrink(2.0);
                        let scale = (inner.width() / w as f32)
                            .min(inner.height() / h as f32)
                            .max(0.01);
                        let draw_size = egui::vec2(w as f32 * scale, h as f32 * scale);
                        let draw_rect = egui::Rect::from_center_size(inner.center(), draw_size);
                        ui.painter().image(
                            tex.id(),
                            draw_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                } else {
                    preview_box(ui, 300.0, 169.0, "Video preview");
                    if draft.deployment == 2
                        && validate_progressive_video_url(draft.file_path.trim()).is_ok()
                    {
                        let preview_mime =
                            progressive_preview_mime(draft.file_path.trim(), &draft.codec);
                        if poll_or_start_progressive_preview_fetch(
                            draft.file_path.trim(),
                            preview_mime,
                        ) {
                            ui.ctx().request_repaint_after(Duration::from_millis(50));
                        }
                        if let Some(error) = take_progressive_preview_error(draft.file_path.trim())
                        {
                            self.message(error);
                        }
                    }
                }
                let base_dir = self
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf);
                let strip_max_sec = probe_video_duration_seconds_with_base(
                    &draft.file_path,
                    draft.deployment,
                    base_dir.as_deref(),
                )
                .unwrap_or_else(|| {
                    parse_timecode_seconds(&draft.out_time)
                        .unwrap_or(4.0)
                        .max(4.0)
                })
                .max(0.2);
                let cur_in = parse_timecode_seconds(&draft.in_time)
                    .unwrap_or(0.0)
                    .clamp(0.0, (strip_max_sec - 0.1).max(0.0));
                let cur_out = parse_timecode_seconds(&draft.out_time)
                    .unwrap_or(strip_max_sec)
                    .clamp((cur_in + 0.1).min(strip_max_sec), strip_max_sec);
                let (span, strip_resp) =
                    ui.allocate_exact_size(egui::vec2(300.0, 27.0), egui::Sense::click_and_drag());
                tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Video, false);
                let in_frac = (cur_in / strip_max_sec).clamp(0.0, 1.0) as f32;
                let out_frac = (cur_out / strip_max_sec).clamp(f64::from(in_frac), 1.0) as f32;
                let x_in = egui::lerp(span.left()..=span.right(), in_frac);
                let x_out = egui::lerp(span.left()..=span.right(), out_frac).max(x_in + 4.0);
                let active_span = egui::Rect::from_min_max(
                    egui::pos2(x_in, span.top()),
                    egui::pos2(x_out.min(span.right()), span.bottom()),
                );
                tl::paint_span(
                    ui.painter(),
                    ui.ctx(),
                    active_span,
                    tl::SpanType::Video,
                    true,
                );
                let pal = neoflash_ui::palette(ui.ctx());
                for hx in [x_in, x_out.min(span.right())] {
                    let hrect = egui::Rect::from_center_size(
                        egui::pos2(
                            hx.clamp(span.left() + 2.0, span.right() - 2.0),
                            span.center().y,
                        ),
                        egui::vec2(4.0, span.height() - 4.0),
                    );
                    ui.painter().rect_filled(hrect, 1.5, pal.accent_default);
                }
                let trim_drag_id = ui.id().with("import-video-trim-drag");
                if strip_resp.hovered() || strip_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if (strip_resp.drag_started() || strip_resp.clicked())
                    && let Some(pos) = strip_resp.interact_pointer_pos()
                {
                    let pick_in = (pos.x - x_in).abs() <= (pos.x - x_out).abs();
                    ui.ctx().data_mut(|d| d.insert_temp(trim_drag_id, pick_in));
                }
                if (strip_resp.dragged() || strip_resp.clicked())
                    && let Some(pos) = strip_resp.interact_pointer_pos()
                {
                    let pick_in = ui
                        .ctx()
                        .data(|d| d.get_temp::<bool>(trim_drag_id))
                        .unwrap_or(true);
                    let frac = ((pos.x - span.left()) / span.width().max(1.0)).clamp(0.0, 1.0);
                    let sec = f64::from(frac) * strip_max_sec;
                    if pick_in {
                        let next_in = sec.clamp(0.0, (cur_out - 0.1).max(0.0));
                        draft.in_time = format_timecode_seconds(next_in);
                    } else {
                        let next_out = sec.clamp((cur_in + 0.1).min(strip_max_sec), strip_max_sec);
                        draft.out_time = format_timecode_seconds(next_out);
                    }
                }
                if strip_resp.drag_stopped() {
                    ui.ctx().data_mut(|d| d.remove::<bool>(trim_drag_id));
                }
                ui.horizontal(|ui| {
                    ui.add(
                        c::TextField::new(&mut draft.in_time, "In")
                            .prefix("In")
                            .width(146.0),
                    );
                    ui.add(
                        c::TextField::new(&mut draft.out_time, "Out")
                            .prefix("Out")
                            .width(146.0),
                    );
                });
                caption(ui, "Drag the in / out handles on the strip to trim.");
            });
            ui.add_space(space::S5);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                ui.horizontal(|ui| {
                    ui.add(
                        c::TextField::new(&mut draft.file_path, "File")
                            .prefix("File")
                            .width(260.0),
                    );
                    if ui.add(c::Button::new("Browse…")).clicked()
                        && let Some(picked) = rfd::FileDialog::new()
                            .set_title("Import video")
                            .add_filter("Video", &["mp4", "webm", "mov", "m4v", "avi", "ogv"])
                            .pick_file()
                    {
                        draft.file_path = picked.display().to_string();
                    }
                });
                group(ui, "Deployment");
                for (idx, label, hint) in [
                    (
                        0usize,
                        "Embed in timeline",
                        "Frames sync with the timeline; best for short clips",
                    ),
                    (
                        1,
                        "Load external video",
                        "Streams with a player component; keeps .nfl small",
                    ),
                    (
                        2,
                        "Progressive download",
                        "Plays while downloading from a URL",
                    ),
                ] {
                    if ui
                        .add(c::Radio::new(draft.deployment == idx, label))
                        .clicked()
                    {
                        draft.deployment = idx;
                    }
                    caption(ui, hint);
                }
                group(ui, "Embedding");
                ui.horizontal(|ui| {
                    let sym_lbl = match draft.symbol_kind {
                        SymbolKind::Graphic => "Symbol: Graphic",
                        _ => "Symbol: Movie clip",
                    };
                    c::Select::new(ui.id().with("import-video-symbol"), sym_lbl)
                        .width(172.0)
                        .show(ui, |ui| {
                            for (kind, lbl) in [
                                (SymbolKind::MovieClip, "Symbol: Movie clip"),
                                (SymbolKind::Graphic, "Symbol: Graphic"),
                            ] {
                                if ui
                                    .add(c::MenuItem::new(lbl).checked(draft.symbol_kind == kind))
                                    .clicked()
                                {
                                    draft.symbol_kind = kind;
                                    ui.close();
                                }
                            }
                        });
                    let codec_lbl = format!("Codec: {}", draft.codec);
                    c::Select::new(ui.id().with("import-video-codec"), &codec_lbl)
                        .width(172.0)
                        .show(ui, |ui| {
                            for codec in ["H.264", "WebM VP9"] {
                                if ui
                                    .add(
                                        c::MenuItem::new(&format!("Codec: {codec}"))
                                            .checked(draft.codec == codec),
                                    )
                                    .clicked()
                                {
                                    draft.codec = codec.to_owned();
                                    ui.close();
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.add(c::Checkbox::new(
                        &mut draft.place_on_stage,
                        "Place on stage",
                    ));
                    ui.add(c::Checkbox::new(
                        &mut draft.expand_timeline,
                        "Expand timeline if needed",
                    ));
                });
                ui.horizontal(|ui| {
                    ui.add(c::Checkbox::new(&mut draft.include_audio, "Include audio"));
                    ui.add(c::Checkbox::new(
                        &mut draft.match_fps,
                        "Match document frame rate",
                    ));
                });
            });
        });

        let in_sec = parse_timecode_seconds(&draft.in_time)
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
            .clamp(0.0, 36_000.0);
        let out_sec = parse_timecode_seconds(&draft.out_time)
            .filter(|v| v.is_finite())
            .unwrap_or(24.0)
            .clamp(in_sec + 0.1, in_sec + 600.0);
        let dur = out_sec - in_sec;
        let summary = format!(
            "1280×720 · {} fps → {} fps · {:.1} s · {}",
            draft.source_fps, self.project.fps, dur, draft.codec
        );
        match footer_clicked(ui, &summary, None, &["Cancel", "Import"]) {
            Some(FooterClick::Button(0)) => close_cancel = true,
            Some(FooterClick::Button(1)) => do_import = true,
            _ => {}
        }
        (close_cancel, do_import)
    }

    pub(crate) fn import_video_inline_panel(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut draft = ctx
            .data(|d| d.get_temp::<ImportVideoDraft>(inline_draft_id()))
            .unwrap_or_default();
        let (close_cancel, do_import) = self.import_video_body(ui, &mut draft);
        ctx.data_mut(|d| d.insert_temp(inline_draft_id(), draft));
        if do_import {
            return self.confirm_import_video(&ctx).is_ok();
        }
        close_cancel
    }

    pub(crate) fn import_video_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<ImportVideoDraft>(draft_id())) else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_import = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);

        let modal = egui::Modal::new(egui::Id::new("import-video"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(720.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Import video",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 720.0,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(720.0 - 2.0 * space::S5);
                        let (c, imp) = self.import_video_body(ui, &mut draft);
                        close_cancel |= c;
                        do_import |= imp;
                    });
            });

        if !close_cancel && !do_import {
            match kb.resolve(ctx, &modal, !draft.file_path.trim().is_empty()) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_import = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));

        if close_cancel {
            self.cancel_import_video(ctx);
            return;
        }
        if do_import {
            let _ = self.confirm_import_video(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::Project;
    use neoflash_export::encode_deterministic_mp4_rgba_frames;
    use std::io::Write as _;
    use std::net::TcpListener;

    #[test]
    fn failed_progressive_preview_retries_after_error_is_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/transient-preview.mp4",
            listener.local_addr().unwrap()
        );
        progressive_preview_fetches().lock().unwrap().insert(
            url.clone(),
            ProgressivePreviewFetch::Failed {
                message: "first transient failure".into(),
                reported: false,
                retry_at: Instant::now() - Duration::from_millis(1),
            },
        );

        assert_eq!(
            take_progressive_preview_error(&url).as_deref(),
            Some("first transient failure")
        );
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request);
            socket
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 9\r\nConnection: close\r\n\r\ntry later",
                )
                .unwrap();
        });
        assert!(
            poll_or_start_progressive_preview_fetch(&url, "video/mp4"),
            "a surfaced transient failure whose backoff elapsed must start a retry"
        );
        server.join().unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let retry_error = loop {
            poll_or_start_progressive_preview_fetch(&url, "video/mp4");
            if let Some(error) = take_progressive_preview_error(&url) {
                break error;
            }
            assert!(
                Instant::now() < deadline,
                "retried Progressive preview did not report its result"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(retry_error.contains("503"), "{retry_error}");
        progressive_preview_fetches().lock().unwrap().remove(&url);
    }

    #[test]
    fn t274_import_video_progressive_preview_decodes_and_scrubs_frames_and_imports_span() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-import-video-t274-prog-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("media")).unwrap();

        let f0 = [255u8, 0, 0, 255].repeat(16);
        let f1 = [0u8, 255, 0, 255].repeat(16);
        let f2 = [0u8, 0, 255, 255].repeat(16);
        let f3 = [255u8, 255, 0, 255].repeat(16);
        let mp4_bytes = encode_deterministic_mp4_rgba_frames(4, 4, 10, &[&f0, &f1, &f2, &f3]);
        std::fs::write(dir.join("media/prog_preview.mp4"), &mp4_bytes).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.path = Some(dir.join("scene.nflash"));

        let mut draft = ImportVideoDraft {
            file_path: "https://cdn.example.com/media/prog_preview.mp4".into(),
            deployment: 2, // Progressive download
            in_time: "00:00.00".into(),
            out_time: "00:00.40".into(),
            ..ImportVideoDraft::default()
        };

        // 1. Progressive preview resolves local/staged file under project base_dir and scrubs frames
        let frame_0 = editor
            .decode_import_video_preview_frame(&draft, Some(0))
            .expect("decode progressive preview at 0ms");
        let frame_1 = editor
            .decode_import_video_preview_frame(&draft, Some(100))
            .expect("decode progressive preview at 100ms");
        let frame_2 = editor
            .decode_import_video_preview_frame(&draft, Some(200))
            .expect("decode progressive preview at 200ms");
        let frame_3 = editor
            .decode_import_video_preview_frame(&draft, Some(300))
            .expect("decode progressive preview at 300ms");
        assert_eq!(&frame_0.image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&frame_1.image.rgba[..4], &[0, 255, 0, 255]);
        assert_eq!(&frame_2.image.rgba[..4], &[0, 0, 255, 255]);
        assert_eq!(&frame_3.image.rgba[..4], &[255, 255, 0, 255]);

        // Changing draft.in_time scrubs the default preview frame to the trimmed in-point
        draft.in_time = "00:00.20".into();
        let trimmed_preview = editor
            .decode_import_video_preview_frame(&draft, None)
            .expect("decode progressive preview at in_time 200ms");
        assert_eq!(&trimmed_preview.image.rgba[..4], &[0, 0, 255, 255]);

        // 2. Remote-only progressive preview downloads real bounded bytes off the UI thread.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote_addr = listener.local_addr().unwrap();
        let remote_bytes = mp4_bytes.clone();
        let remote_server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request);
            let response_header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                remote_bytes.len()
            );
            socket.write_all(response_header.as_bytes()).unwrap();
            socket.write_all(&remote_bytes).unwrap();
        });
        let remote_draft = ImportVideoDraft {
            file_path: format!("http://{remote_addr}/streams/remote_only.mp4"),
            deployment: 2,
            in_time: "00:00.00".into(),
            out_time: "00:01.00".into(),
            ..ImportVideoDraft::default()
        };
        assert!(
            editor
                .decode_import_video_preview_frame(&remote_draft, Some(0))
                .is_none(),
            "the first call must schedule rather than block on network I/O"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let remote_0 = loop {
            if let Some(frame) = editor.decode_import_video_preview_frame(&remote_draft, Some(0)) {
                break frame;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "real Progressive preview did not finish within the test deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let remote_2 = editor
            .decode_import_video_preview_frame(&remote_draft, Some(200))
            .expect("cached Progressive preview frame at 200ms");
        remote_server.join().unwrap();
        assert_eq!(&remote_0.image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&remote_2.image.rgba[..4], &[0, 0, 255, 255]);

        // 3. Confirming progressive import creates a valid Progressive video span that decodes on Stage
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        let res_id = editor
            .confirm_import_video(&ctx)
            .expect("confirm progressive video import");
        editor
            .project
            .validate()
            .expect("project validates cleanly");

        let scene =
            neoflash_core::evaluate_scene(&editor.project, &editor.project.root, 0, false, true);
        let items: Vec<_> = scene.draws.iter().map(|d| d.item.clone()).collect();
        let placements = neoflash_core::resolve_scene_video_placements(
            &editor.project,
            &editor.project.root,
            0,
            &std::collections::HashMap::new(),
            &items,
        );
        assert_eq!(placements.len(), 1);
        let placement = placements.values().next().unwrap();
        assert_eq!(placement.resource, res_id);
        assert_eq!(
            placement.source.as_ref().map(|s| s.mode),
            Some(VideoDeploymentMode::Progressive)
        );
        let (_, stage_frame) = editor
            .video_textures
            .frame_for_placement(&editor.project, Some(&dir), placement)
            .expect("native stage decodes imported progressive span");
        assert_eq!(&stage_frame.image.rgba[..4], &[0, 0, 255, 255]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
