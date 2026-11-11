//! Properties ▸ Video (`PR-09`, `T-217`, `T-253`): non-destructive montage inspector for
//! timeline video spans (`In`/`Out` trim handles and numeric fields, `Stream`/`Event`/`Stop`
//! sync, `Loop`, `Mute`, `Link`/`Unlink` audio partner, `Extract audio` track creation,
//! `Split at playhead`, and cue-point authoring/retiming).

use super::import_video::probe_video_bytes_duration_ms;
use super::planned::caption;
use crate::app::Editor;
use crate::paint_controls::two_column_row;
use eframe::egui;
use neoflash_core::{
    FrameSound, Id, Keyframe, Layer, LayerProperties, LayerRole, ResourceUnit, SoundEffect,
    SoundSync, TimelineVideoSpan, VideoSyncMode, new_id, timeline_video::VideoCuePoint,
    timeline_video::VideoCuePointKind,
};
use neoflash_ui::components::{self as c, MenuItem, timeline as tl};
use neoflash_ui::tokens::space;

fn sync_label(sync: VideoSyncMode) -> &'static str {
    match sync {
        VideoSyncMode::Stream => "Stream",
        VideoSyncMode::Event => "Event",
        VideoSyncMode::Stop => "Stop",
    }
}

enum VideoInspectorAction {
    UpdateSpan(Option<TimelineVideoSpan>),
    LinkPartner(Option<Id>),
    ExtractAudio,
    SplitAtPlayhead,
}

impl Editor {
    /// Active keyframe frame and `TimelineVideoSpan` on the selected layer at `self.frame`.
    pub(crate) fn frame_video(&self) -> Option<(u32, TimelineVideoSpan)> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        Some((key.frame, key.video.clone()?))
    }

    pub(crate) fn video_resources(&self) -> Vec<(Id, String)> {
        self.project
            .resources
            .iter()
            .filter(|r| r.is_video())
            .map(|r| (r.id.clone(), r.path.clone()))
            .collect()
    }

    fn probed_resource_duration_ms(&self, resource_id: &str) -> Option<u32> {
        let res = self.project.resource(resource_id)?;
        let bytes = res.decoded_bytes().ok()?;
        probe_video_bytes_duration_ms(&bytes)
    }

    /// Extract embedded video audio into a dedicated `audio/*` resource, create a linked audio
    /// layer immediately below the video layer with matching `SoundSync` and `in_point`/`out_point`,
    /// and mute the video span so audio plays once — committed as one Undo step.
    pub fn extract_active_video_audio_track(&mut self) -> Result<Id, String> {
        self.finish_edits();
        if !self.layer_editable() {
            return Err("Select an unlocked, visible video layer before extracting audio.".into());
        }
        let Some((key_frame, span)) = self.frame_video() else {
            return Err("No timeline video span is active at the current frame.".into());
        };
        let Some(video_res) = self.project.resource(&span.resource) else {
            return Err("Referenced video resource was not found in the Library.".into());
        };
        let raw_bytes = video_res
            .decoded_bytes()
            .map_err(|e| format!("Could not decode video bytes: {e}"))?;
        if raw_bytes.is_empty() {
            return Err(
                "External or progressive URL videos do not embed local audio bytes to extract."
                    .into(),
            );
        }

        let (audio_bytes, audio_mime, ext) = if neoflash_core::is_avi_bytes(&raw_bytes)
            && let Ok(decoded) = neoflash_core::decode_avi_bytes(&raw_bytes)
            && let (Some(bytes), Some(mime)) = (decoded.audio_bytes, decoded.audio_mime)
            && !bytes.is_empty()
        {
            let ext = if mime.contains("mpeg") || mime.contains("mp3") {
                "mp3"
            } else {
                "wav"
            };
            (bytes, mime.to_owned(), ext)
        } else {
            extract_wav_with_ffmpeg(&raw_bytes)?
        };

        let symbol_id = self.symbol_id().to_owned();
        let video_layer_idx = self.layer;
        let video_layer_id = self.symbol().timeline.layers[video_layer_idx].id.clone();
        let video_layer_name = self.symbol().timeline.layers[video_layer_idx].name.clone();
        let parent_folder = self.symbol().timeline.layers[video_layer_idx]
            .properties
            .parent
            .clone();

        let mut candidate = self.project.clone();
        let stem = std::path::Path::new(&video_res.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("video_audio");
        let mut audio_path = format!("audio/{stem}.{ext}");
        let mut seq = 1u32;
        while candidate
            .resources
            .iter()
            .any(|r| r.path.eq_ignore_ascii_case(&audio_path))
        {
            audio_path = format!("audio/{stem}-{seq}.{ext}");
            seq += 1;
        }
        let audio_res_id = new_id("res");
        let audio_res = ResourceUnit::from_bytes(
            audio_res_id.clone(),
            audio_path.clone(),
            &audio_mime,
            &audio_bytes,
        )
        .map_err(|e| e.to_string())?;
        candidate.resources.push(audio_res);

        let audio_layer_id = new_id("layer");
        let sound_sync = match span.sync {
            VideoSyncMode::Stream => SoundSync::Stream,
            VideoSyncMode::Event => SoundSync::Event,
            VideoSyncMode::Stop => SoundSync::Stop,
        };
        let in_point = (span.start_offset_ms > 0).then(|| f64::from(span.start_offset_ms) / 1000.0);
        let out_point = span.end_offset_ms.map(|ms| f64::from(ms) / 1000.0);
        let sound_key = Keyframe {
            frame: key_frame,
            sound: Some(FrameSound {
                resource: audio_res_id.clone(),
                sync: sound_sync,
                loops: 1,
                loop_forever: span.loop_video,
                effect: SoundEffect::None,
                envelope: None,
                in_point,
                out_point,
            }),
            ..Keyframe::default()
        };
        let mut keyframes = Vec::new();
        if key_frame > 0 {
            keyframes.push(Keyframe::default());
        }
        keyframes.push(sound_key);

        let symbol = candidate
            .symbol_mut(&symbol_id)
            .ok_or_else(|| "Active symbol not found.".to_owned())?;
        if let Some(v_layer) = symbol
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == video_layer_id)
        {
            v_layer.properties.linked_layer = Some(audio_layer_id.clone());
            if let Some(vk) = v_layer.keyframes.iter_mut().find(|k| k.frame == key_frame)
                && let Some(vspan) = &mut vk.video
            {
                vspan.muted = true;
            }
        }
        let audio_layer = Layer {
            id: audio_layer_id.clone(),
            name: format!("{video_layer_name} Audio"),
            visible: true,
            locked: false,
            keyframes,
            properties: LayerProperties {
                parent: parent_folder,
                linked_layer: Some(video_layer_id),
                ..LayerProperties::default()
            },
        };
        let insert_idx = video_layer_idx.min(symbol.timeline.layers.len());
        symbol.timeline.layers.insert(insert_idx, audio_layer);

        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;

        self.transact_named("Extract video audio", |editor| {
            editor.project = candidate;
            editor.layer = video_layer_idx + 1;
        });
        self.message(format!(
            "Extracted audio to {audio_path} and linked tracks."
        ));
        Ok(audio_res_id)
    }

    /// Video section in `Properties` when the active layer has a `TimelineVideoSpan` at `self.frame`.
    pub(crate) fn frame_video_section(&mut self, ui: &mut egui::Ui) {
        let Some((key_frame, span)) = self.frame_video() else {
            return;
        };
        let editable = self.layer_editable();
        let videos = self.video_resources();
        let res_name = videos
            .iter()
            .find(|(id, _)| *id == span.resource)
            .map_or("Missing video", |(_, path)| path.as_str())
            .to_owned();
        let probed_ms = self.probed_resource_duration_ms(&span.resource);
        let strip_max_ms = probed_ms
            .or(span.end_offset_ms)
            .unwrap_or_else(|| span.start_offset_ms.saturating_add(4_000))
            .max(span.start_offset_ms.saturating_add(200));

        let active_layer_id = self.symbol().timeline.layers[self.layer].id.clone();
        let linked_partner = self.symbol().timeline.layers[self.layer]
            .properties
            .linked_layer
            .clone();
        let partner_candidates: Vec<(Id, String)> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .filter(|l| {
                l.id != active_layer_id && l.properties.role != LayerRole::Folder && !l.locked
            })
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();

        let mut action: Option<VideoInspectorAction> = None;
        let mut next_span = span.clone();
        let w = ui.available_width();

        c::SectionHeader::new("Video")
            .id_salt("properties-video")
            .show(ui, |ui| {
                ui.add_enabled_ui(editable, |ui| {
                    c::Select::new("frame-video-resource", &res_name)
                        .width(w)
                        .show(ui, |ui| {
                            if ui.add(MenuItem::new("None")).clicked() {
                                action = Some(VideoInspectorAction::UpdateSpan(None));
                                ui.close();
                            }
                            for (id, path) in &videos {
                                if ui
                                    .add(MenuItem::new(path).checked(*id == span.resource))
                                    .clicked()
                                {
                                    next_span.resource = id.clone();
                                    action = Some(VideoInspectorAction::UpdateSpan(Some(
                                        next_span.clone(),
                                    )));
                                    ui.close();
                                }
                            }
                        });

                    ui.horizontal(|ui| {
                        let sync_lbl = format!("Sync: {}", sync_label(next_span.sync));
                        c::Select::new("frame-video-sync", &sync_lbl)
                            .width(132.0)
                            .show(ui, |ui| {
                                for mode in [
                                    VideoSyncMode::Stream,
                                    VideoSyncMode::Event,
                                    VideoSyncMode::Stop,
                                ] {
                                    if ui
                                        .add(
                                            MenuItem::new(sync_label(mode))
                                                .checked(mode == next_span.sync),
                                        )
                                        .clicked()
                                    {
                                        next_span.sync = mode;
                                        ui.close();
                                    }
                                }
                            });
                        ui.add(c::Checkbox::new(&mut next_span.loop_video, "Loop"));
                        ui.add(c::Checkbox::new(&mut next_span.muted, "Mute"));
                    });

                    // Interactive NF2 video trim strip + handles.
                    let cur_in_ms = next_span
                        .start_offset_ms
                        .min(strip_max_ms.saturating_sub(50));
                    let cur_out_ms = next_span.end_offset_ms.unwrap_or(strip_max_ms).clamp(
                        cur_in_ms.saturating_add(50),
                        strip_max_ms.max(cur_in_ms + 50),
                    );
                    let (strip_rect, strip_resp) =
                        ui.allocate_exact_size(egui::vec2(w, 27.0), egui::Sense::click_and_drag());
                    tl::paint_span(
                        ui.painter(),
                        ui.ctx(),
                        strip_rect,
                        tl::SpanType::Video,
                        false,
                    );
                    let in_frac = (cur_in_ms as f32 / strip_max_ms.max(1) as f32).clamp(0.0, 1.0);
                    let out_frac =
                        (cur_out_ms as f32 / strip_max_ms.max(1) as f32).clamp(in_frac, 1.0);
                    let x_in = egui::lerp(strip_rect.left()..=strip_rect.right(), in_frac);
                    let x_out = egui::lerp(strip_rect.left()..=strip_rect.right(), out_frac)
                        .max(x_in + 4.0);
                    let active_rect = egui::Rect::from_min_max(
                        egui::pos2(x_in, strip_rect.top()),
                        egui::pos2(x_out.min(strip_rect.right()), strip_rect.bottom()),
                    );
                    tl::paint_span(
                        ui.painter(),
                        ui.ctx(),
                        active_rect,
                        tl::SpanType::Video,
                        true,
                    );
                    let pal = neoflash_ui::palette(ui.ctx());
                    for hx in [x_in, x_out.min(strip_rect.right())] {
                        let hrect = egui::Rect::from_center_size(
                            egui::pos2(
                                hx.clamp(strip_rect.left() + 2.0, strip_rect.right() - 2.0),
                                strip_rect.center().y,
                            ),
                            egui::vec2(4.0, strip_rect.height() - 4.0),
                        );
                        ui.painter().rect_filled(hrect, 1.5, pal.accent_default);
                    }
                    let drag_id = ui.id().with(("props-video-trim", key_frame));
                    if strip_resp.hovered() || strip_resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if (strip_resp.drag_started() || strip_resp.clicked())
                        && let Some(pos) = strip_resp.interact_pointer_pos()
                    {
                        let pick_in = (pos.x - x_in).abs() <= (pos.x - x_out).abs();
                        ui.ctx().data_mut(|d| d.insert_temp(drag_id, pick_in));
                    }
                    if (strip_resp.dragged() || strip_resp.clicked())
                        && let Some(pos) = strip_resp.interact_pointer_pos()
                    {
                        let pick_in = ui
                            .ctx()
                            .data(|d| d.get_temp::<bool>(drag_id))
                            .unwrap_or(true);
                        let frac = ((pos.x - strip_rect.left()) / strip_rect.width().max(1.0))
                            .clamp(0.0, 1.0);
                        let target_ms = (f64::from(frac) * f64::from(strip_max_ms)).round() as u32;
                        if pick_in {
                            next_span.start_offset_ms =
                                target_ms.min(cur_out_ms.saturating_sub(50));
                        } else {
                            next_span.end_offset_ms =
                                Some(target_ms.max(cur_in_ms.saturating_add(50)));
                        }
                    }
                    if strip_resp.drag_stopped() {
                        ui.ctx().data_mut(|d| d.remove::<bool>(drag_id));
                    }

                    let mut in_ms_f = f64::from(next_span.start_offset_ms);
                    let mut out_ms_f = f64::from(next_span.end_offset_ms.unwrap_or(strip_max_ms));
                    let (in_resp, out_resp) = two_column_row(
                        ui,
                        |ui, col_w| {
                            ui.add(
                                c::Field::new(&mut in_ms_f, "In")
                                    .prefix("In")
                                    .unit("ms")
                                    .range(0.0..=600_000.0)
                                    .decimals(0)
                                    .stepper()
                                    .width(col_w),
                            )
                        },
                        |ui, col_w| {
                            ui.add(
                                c::Field::new(&mut out_ms_f, "Out")
                                    .prefix("Out")
                                    .unit("ms")
                                    .range(1.0..=600_000.0)
                                    .decimals(0)
                                    .stepper()
                                    .width(col_w),
                            )
                        },
                    );
                    if in_resp.changed() {
                        let new_in = in_ms_f.round().max(0.0) as u32;
                        next_span.start_offset_ms = new_in;
                        if let Some(end_ms) = next_span.end_offset_ms
                            && end_ms <= new_in
                        {
                            next_span.end_offset_ms = Some(new_in + 50);
                        }
                    }
                    if out_resp.changed() {
                        let new_out = out_ms_f
                            .round()
                            .max(f64::from(next_span.start_offset_ms + 1))
                            as u32;
                        next_span.end_offset_ms = Some(new_out);
                    }

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        if ui
                            .add_enabled(
                                next_span.end_offset_ms.is_some(),
                                c::Button::ghost("Clear out"),
                            )
                            .clicked()
                        {
                            next_span.end_offset_ms = None;
                        }
                        if ui
                            .add_enabled(
                                self.frame > key_frame,
                                c::Button::ghost("Split at playhead"),
                            )
                            .clicked()
                        {
                            action = Some(VideoInspectorAction::SplitAtPlayhead);
                        }
                        if ui
                            .add_enabled(
                                next_span.deployment.is_embedded(),
                                c::Button::ghost("Extract audio"),
                            )
                            .clicked()
                        {
                            action = Some(VideoInspectorAction::ExtractAudio);
                        }
                    });

                    // Linked audio/video track selector.
                    let partner_lbl = match linked_partner.as_deref().and_then(|pid| {
                        partner_candidates
                            .iter()
                            .find(|(id, _)| id == pid)
                            .map(|(_, name)| name.as_str())
                    }) {
                        Some(name) => format!("Linked track: {name}"),
                        None => "Linked track: None".to_owned(),
                    };
                    c::Select::new("frame-video-linked-track", &partner_lbl)
                        .width(w)
                        .show(ui, |ui| {
                            if ui
                                .add(MenuItem::new("None").checked(linked_partner.is_none()))
                                .clicked()
                            {
                                action = Some(VideoInspectorAction::LinkPartner(None));
                                ui.close();
                            }
                            for (pid, pname) in &partner_candidates {
                                if ui
                                    .add(
                                        MenuItem::new(pname).checked(
                                            linked_partner.as_deref() == Some(pid.as_str()),
                                        ),
                                    )
                                    .clicked()
                                {
                                    action =
                                        Some(VideoInspectorAction::LinkPartner(Some(pid.clone())));
                                    ui.close();
                                }
                            }
                        });

                    // Authored cue points + retiming.
                    ui.horizontal(|ui| {
                        caption(
                            ui,
                            &format!(
                                "{} · Key F{} · {} cue{}",
                                next_span.deployment.label(),
                                key_frame + 1,
                                next_span.cue_points.len(),
                                if next_span.cue_points.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ),
                        );
                        if ui.add(c::Button::ghost("+ Cue")).clicked() {
                            let elapsed_ms = neoflash_core::frame_delta_ms(
                                self.frame.saturating_sub(key_frame),
                                f64::from(self.project.fps.max(1)),
                            );
                            let cue_time = next_span.start_offset_ms.saturating_add(elapsed_ms);
                            let idx = next_span.cue_points.len() + 1;
                            next_span.cue_points.push(VideoCuePoint {
                                name: format!("cue_{idx}"),
                                time_ms: cue_time,
                                kind: VideoCuePointKind::Event,
                                parameters: Vec::new(),
                            });
                        }
                    });
                    let mut remove_cue_idx = None;
                    for (idx, cue) in next_span.cue_points.iter_mut().enumerate() {
                        ui.push_id(("video-cue", idx), |ui| {
                            let mut t_ms = f64::from(cue.time_ms);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S2;
                                ui.add(c::TextField::new(&mut cue.name, "Cue").width(108.0));
                                let r = ui.add(
                                    c::Field::new(&mut t_ms, "Cue time")
                                        .unit("ms")
                                        .range(0.0..=600_000.0)
                                        .decimals(0)
                                        .width(96.0),
                                );
                                if r.changed() {
                                    cue.time_ms = t_ms.round().max(0.0) as u32;
                                }
                                if ui.add(c::Button::ghost("×")).clicked() {
                                    remove_cue_idx = Some(idx);
                                }
                            });
                        });
                    }
                    if let Some(idx) = remove_cue_idx {
                        next_span.cue_points.remove(idx);
                    }
                });
            });

        if action.is_none() && next_span != span {
            action = Some(VideoInspectorAction::UpdateSpan(Some(next_span)));
        }

        match action {
            Some(VideoInspectorAction::UpdateSpan(updated)) => {
                let _ = self.set_keyframe_video_span(updated);
            }
            Some(VideoInspectorAction::LinkPartner(Some(partner_id))) => {
                let _ = self.link_layers(&active_layer_id, &partner_id);
            }
            Some(VideoInspectorAction::LinkPartner(None)) => {
                let _ = self.unlink_layer(&active_layer_id);
            }
            Some(VideoInspectorAction::ExtractAudio) => {
                if let Err(err) = self.extract_active_video_audio_track() {
                    self.message(err);
                }
            }
            Some(VideoInspectorAction::SplitAtPlayhead) => {
                let _ = self.split_clip_at_playhead();
            }
            None => {}
        }
    }
}

fn extract_wav_with_ffmpeg(video_bytes: &[u8]) -> Result<(Vec<u8>, String, &'static str), String> {
    let dir = std::env::temp_dir().join(format!("neoflash-vid-audio-{}", new_id("tmp")));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Temporary folder failed: {e}"))?;
    let in_path = dir.join("input_video.bin");
    let out_path = dir.join("extracted.wav");
    let write_res = std::fs::write(&in_path, video_bytes);
    if let Err(err) = write_res {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("Temporary video write failed: {err}"));
    }
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            in_path.to_str().unwrap_or_default(),
            "-vn",
            "-acodec",
            "pcm_s16le",
            "-ar",
            "44100",
            "-ac",
            "2",
            out_path.to_str().unwrap_or_default(),
        ])
        .output();
    let result = match status {
        Ok(out) if out.status.success() => {
            let wav = std::fs::read(&out_path)
                .map_err(|e| format!("Could not read extracted WAV audio: {e}"))?;
            if wav.is_empty() {
                Err("The video clip does not contain an extractable audio stream.".into())
            } else {
                Ok((wav, "audio/wav".to_owned(), "wav"))
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = stderr.trim();
            if detail.is_empty() {
                Err("The video clip does not contain an extractable audio stream.".into())
            } else {
                Err(format!("Audio extraction failed: {detail}"))
            }
        }
        Err(_) => Err(
            "Audio extraction for MP4/WebM requires `ffmpeg` on PATH (AVI PCM/MP3 extracts natively)."
                .into(),
        ),
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}
