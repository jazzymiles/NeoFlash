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
};

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

fn probe_video_duration_seconds(raw_path: &str, deployment: usize) -> Option<f64> {
    if deployment == 2 || raw_path.trim().is_empty() {
        return None;
    }
    let path = expand_import_path(raw_path);
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

        let probed_duration_sec = probe_video_duration_seconds(raw_input, draft.deployment);
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
                preview_box(ui, 300.0, 169.0, "Video preview");
                let strip_max_sec =
                    probe_video_duration_seconds(&draft.file_path, draft.deployment)
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
