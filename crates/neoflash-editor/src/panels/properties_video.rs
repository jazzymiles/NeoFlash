//! Properties ▸ Video (`PR-09`, `T-217`, `T-253`): non-destructive montage inspector for
//! timeline video spans (`In`/`Out` trim handles and numeric fields, `Stream`/`Event`/`Stop`
//! sync, `Loop`, `Mute`, `Link`/`Unlink` audio partner, `Extract audio` track creation,
//! `Split at playhead`, and cue-point authoring/retiming).

use super::import_video::probe_video_bytes_duration_ms;
use crate::app::Editor;
use crate::paint_controls::two_column_row;
use eframe::egui;
use neoflash_core::{
    FrameSound, Id, Keyframe, Layer, LayerProperties, ResourceUnit, SoundEffect, SoundSync,
    TimelineVideoSpan, VideoSyncMode, new_id, timeline_video::VideoCuePoint,
    timeline_video::VideoCuePointKind,
};
use neoflash_ui::components::{self as c, MenuItem};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

fn sync_label(sync: VideoSyncMode) -> &'static str {
    match sync {
        VideoSyncMode::Stream => "Stream",
        VideoSyncMode::Event => "Event",
        VideoSyncMode::Stop => "Stop",
    }
}

enum VideoInspectorAction {
    UpdateSpan(Option<TimelineVideoSpan>),
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

        let (audio_bytes, audio_mime) =
            neoflash_export::video_audio::embedded_soundtrack(&raw_bytes)?
                .ok_or("This video clip does not contain an audio stream to extract.")?;
        let ext = if audio_mime == "audio/mpeg" {
            "mp3"
        } else {
            "wav"
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
            audio_mime,
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

        let mut action: Option<VideoInspectorAction> = None;
        let mut next_span = span.clone();
        let w = ui.available_width();
        let p = neoflash_ui::palette(ui.ctx());

        let sel_id = ui.id().with(("video-cue-sel", key_frame));
        let edit_id = ui.id().with(("video-cue-edit", key_frame));
        let mut selected_cue: Option<usize> = ui.ctx().data(|d| d.get_temp(sel_id));
        let mut editing_cue: Option<usize> = ui.ctx().data(|d| d.get_temp(edit_id));

        c::SectionHeader::new("Video")
            .id_salt("properties-video")
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::s2();
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
                        ui.spacing_mut().item_spacing.x = space::s2();
                        let sync_lbl = sync_label(next_span.sync);
                        // The select takes what the two checkboxes leave.
                        let toggles_w = crate::panels::properties::checkbox_width(ui, "Loop")
                            + crate::panels::properties::checkbox_width(ui, "Mute")
                            + 2.0 * space::s2();
                        let sync_w = (ui.available_width() - toggles_w).max(0.0);
                        c::Select::new("frame-video-sync", sync_lbl)
                            .prefix("Sync")
                            .width(sync_w)
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

                    ui.label(rich("Trim", TextStyle::Overline).color(p.text_tertiary));
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
                        ui.spacing_mut().item_spacing.x = space::s2();
                        if ui
                            .add_enabled(
                                self.frame > key_frame,
                                c::Button::new("Split at playhead"),
                            )
                            .clicked()
                        {
                            action = Some(VideoInspectorAction::SplitAtPlayhead);
                        }
                        if ui
                            .add_enabled(
                                next_span.deployment.is_embedded(),
                                c::Button::new("Extract audio"),
                            )
                            .clicked()
                        {
                            action = Some(VideoInspectorAction::ExtractAudio);
                        }
                    });
                    if ui
                        .add_enabled(
                            next_span.start_offset_ms > 0 || next_span.end_offset_ms.is_some(),
                            c::Button::ghost("Reset trim"),
                        )
                        .clicked()
                    {
                        next_span.start_offset_ms = 0;
                        next_span.end_offset_ms = None;
                    }

                    ui.label(rich("Cue points", TextStyle::Overline).color(p.text_tertiary));
                    let columns = [0.0, 0.56];
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(c::TableRow {
                            cells: &["Name", "Time"],
                            kind: c::TableRowKind::Header,
                            columns: &columns,
                        });
                        for (idx, cue) in next_span.cue_points.iter_mut().enumerate() {
                            let is_sel = selected_cue == Some(idx);
                            let is_edit = editing_cue == Some(idx);
                            if is_edit {
                                let mut t_ms = f64::from(cue.time_ms);
                                ui.push_id(("video-cue-edit-row", idx), |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = space::s1();
                                        let name_w = (w * columns[1] - space::s1()).max(0.0);
                                        let time_w = (w - name_w - space::s1()).max(0.0);
                                        let mut name_resp = ui.add(
                                            c::TextField::new(&mut cue.name, "Cue name")
                                                .width(name_w),
                                        );
                                        name_resp.id = c::TextField::edit_id(name_resp.id);
                                        let mut time_resp = ui.add(
                                            c::Field::new(&mut t_ms, "Cue time")
                                                .unit("ms")
                                                .range(0.0..=600_000.0)
                                                .decimals(0)
                                                .width(time_w),
                                        );
                                        time_resp.id = time_resp.id.with("edit");
                                        if time_resp.changed() {
                                            cue.time_ms = t_ms.round().max(0.0) as u32;
                                        }
                                        let blurred_row = (name_resp.lost_focus()
                                            && !time_resp.has_focus())
                                            || (time_resp.lost_focus() && !name_resp.has_focus());
                                        let close_key = ui.input_mut(|i| {
                                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                                || i.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Escape,
                                                )
                                        });
                                        if close_key || blurred_row {
                                            editing_cue = None;
                                        } else {
                                            super::dialog_helpers::mark_inline_edit_active(
                                                ui.ctx(),
                                            );
                                        }
                                    });
                                });
                            } else {
                                let time_lbl = format!("{} ms", cue.time_ms);
                                let resp = ui.add(c::TableRow {
                                    cells: &[&cue.name, &time_lbl],
                                    kind: if is_sel {
                                        c::TableRowKind::Selected
                                    } else {
                                        c::TableRowKind::Row
                                    },
                                    columns: &columns,
                                });
                                if resp.double_clicked() {
                                    selected_cue = Some(idx);
                                    editing_cue = Some(idx);
                                } else if resp.clicked() {
                                    if is_sel {
                                        editing_cue = Some(idx);
                                    } else {
                                        selected_cue = Some(idx);
                                    }
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s1();
                        if ui
                            .add(c::IconButton::new(i16::PLUS, "Add cue point"))
                            .clicked()
                        {
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
                            selected_cue = Some(next_span.cue_points.len() - 1);
                            editing_cue = selected_cue;
                        }
                        let can_remove =
                            selected_cue.is_some_and(|i| i < next_span.cue_points.len());
                        if ui
                            .add_enabled(
                                can_remove,
                                c::IconButton::new(i16::TRASH, "Remove cue point"),
                            )
                            .clicked()
                            && let Some(idx) = selected_cue
                            && idx < next_span.cue_points.len()
                        {
                            next_span.cue_points.remove(idx);
                            editing_cue = None;
                            selected_cue = if next_span.cue_points.is_empty() {
                                None
                            } else {
                                Some(idx.min(next_span.cue_points.len() - 1))
                            };
                        }
                    });
                });
            });

        if let Some(sel) = selected_cue.filter(|&i| i < next_span.cue_points.len()) {
            ui.ctx().data_mut(|d| d.insert_temp(sel_id, sel));
        } else {
            ui.ctx().data_mut(|d| d.remove::<usize>(sel_id));
        }
        if let Some(edit) = editing_cue.filter(|&i| i < next_span.cue_points.len()) {
            ui.ctx().data_mut(|d| d.insert_temp(edit_id, edit));
        } else {
            ui.ctx().data_mut(|d| d.remove::<usize>(edit_id));
        }

        if action.is_none() && next_span != span {
            action = Some(VideoInspectorAction::UpdateSpan(Some(next_span)));
        }

        match action {
            Some(VideoInspectorAction::UpdateSpan(updated)) => {
                let _ = self.set_keyframe_video_span(updated);
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

#[cfg(all(test, unix))]
mod portable_media_tests {
    use neoflash_export::video_audio::extract_wav_with_resolved_ffmpeg;
    use std::os::unix::fs::PermissionsExt;

    static FIXTURE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    struct Fixture(std::path::PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn t289_extract_audio_uses_unicode_bundled_helper_with_an_empty_path() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-t289-медиа-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let fixture = Fixture(root);
        let helper = fixture
            .0
            .join("комплект media")
            .join("bundled-media")
            .join("bin")
            .join("ffmpeg");
        std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
        std::fs::write(
            &helper,
            br#"#!/bin/sh
input=
previous=
output=
for argument do
    if [ "$previous" = "-i" ]; then input=$argument; fi
    previous=$argument
    output=$argument
done
[ -n "$input" ] && [ -f "$input" ] || exit 61
[ "${PATH+x}" = x ] && [ -z "$PATH" ] || exit 62
printf 'RIFF\014\000\000\000WAVE' > "$output"
printf '%s\n%s\n' "$input" "$output" > "${0%/*}/capture.txt"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();

        let wav = extract_wav_with_resolved_ffmpeg(b"\x1aE\xdf\xa3fake-webm", &helper)
            .unwrap()
            .unwrap();

        assert_eq!(wav, b"RIFF\x0c\0\0\0WAVE");
        let capture = std::fs::read_to_string(helper.with_file_name("capture.txt")).unwrap();
        let mut paths = capture.lines().map(std::path::Path::new);
        let input = paths.next().unwrap();
        let output = paths.next().unwrap();
        assert!(input.is_absolute());
        assert_eq!(input.file_name().unwrap(), "input_video.bin");
        assert!(output.is_absolute());
        assert_eq!(output.file_name().unwrap(), "extracted.wav");
    }
}

#[cfg(test)]
mod frame_video_ui_tests {
    use crate::app::Editor;
    use eframe::egui;
    use neoflash_core::timeline_video::{VideoCuePoint, VideoCuePointKind, VideoSyncMode};
    use neoflash_core::{Project, ResourceUnit, TimelineVideoSpan, build_minimal_mjpeg_avi};

    fn collected_text(output: &egui::FullOutput) -> Vec<String> {
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        texts
    }

    fn accesskit_bounds(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        output
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|b| {
                        egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        )
                    })
            })
    }

    fn click_point(
        ctx: &egui::Context,
        pos: egui::Pos2,
        draw: &mut dyn FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let mut run = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320.0, 720.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::right("props")
                        .exact_width(280.0)
                        .show(ctx, |ui| {
                            draw(ui);
                        });
                },
            )
        };
        let _ = run(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        run(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ])
    }

    #[test]
    fn frame_video_section_matches_r093_layout_controls_and_cue_table() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut proj = Project::blank();
        proj.fps = 10;
        let avi = build_minimal_mjpeg_avi(4, 4, 10, &vec![vec![0xFF, 0xD8, 0xFF, 0xD9]; 6], None);
        let res = ResourceUnit::from_bytes("vid-1", "videos/clip.avi", "video/avi", &avi).unwrap();
        proj.resources.push(res);

        let mut editor = Editor::from_project(&ctx, proj);
        editor.layer = 0;
        editor.frame = 0;
        assert!(editor.set_keyframe_video_span(Some(TimelineVideoSpan {
            resource: "vid-1".to_owned(),
            sync: VideoSyncMode::Stream,
            start_offset_ms: 100,
            end_offset_ms: Some(500),
            loop_video: false,
            muted: false,
            cue_points: vec![VideoCuePoint {
                name: "intro".to_owned(),
                time_ms: 150,
                kind: VideoCuePointKind::Event,
                parameters: vec![],
            }],
            ..Default::default()
        })));

        let run_panel = |ed: &mut Editor| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320.0, 720.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::right("props")
                        .exact_width(280.0)
                        .show(ctx, |ui| {
                            ed.frame_video_section(ui);
                        });
                },
            )
        };

        let out = run_panel(&mut editor);
        let texts = collected_text(&out);

        for expected in [
            "VIDEO",
            "Sync",
            "Stream",
            "Loop",
            "Mute",
            "TRIM",
            "In",
            "Out",
            "Split at playhead",
            "Extract audio",
            "Reset trim",
            "CUE POINTS",
            "NAME",
            "TIME",
            "intro",
            "150 ms",
        ] {
            assert!(
                texts.iter().any(|t| t == expected),
                "expected R-093 label {expected:?} in Properties ▸ Video; got {texts:?}"
            );
        }
        for obsolete in ["+ Cue", "×", "Clear out", "Linked track"] {
            assert!(
                !texts.iter().any(|t| t == obsolete),
                "obsolete pre-R-093 label {obsolete:?} must not appear: {texts:?}"
            );
        }

        let add_btn = accesskit_bounds(&out, "Add cue point")
            .expect("Add cue point icon button must expose accessible label");
        let remove_btn = accesskit_bounds(&out, "Remove cue point")
            .expect("Remove cue point icon button must expose accessible label");
        assert!(
            add_btn.left() < remove_btn.left()
                && (add_btn.center().y - remove_btn.center().y).abs() <= 2.0,
            "Add and Remove cue point icon buttons should share one row"
        );

        let _ = click_point(&ctx, add_btn.center(), &mut |ui| {
            editor.frame_video_section(ui);
        });
        let after_add = run_panel(&mut editor);
        assert_eq!(
            editor.frame_video().unwrap().1.cue_points.len(),
            2,
            "clicking Add cue point must insert a cue point row"
        );

        // Select the new second cue row and click Remove cue point.
        assert!(
            collected_text(&after_add)
                .iter()
                .any(|t| t.starts_with("cue_")),
            "newly added cue_ row present in table; got {:?}",
            collected_text(&after_add)
        );
        // Confirm inline edit with Enter so the row returns to TableRow display, then select & remove it.
        let after_enter = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 720.0),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::right("props")
                    .exact_width(280.0)
                    .show(ctx, |ui| {
                        editor.frame_video_section(ui);
                    });
            },
        );
        let remove_pos = accesskit_bounds(&after_enter, "Remove cue point")
            .expect("Remove cue point button present")
            .center();
        let mut row2_pos = None;
        fn find_cue_pos(shape: &egui::Shape, out_pos: &mut Option<egui::Pos2>) {
            if out_pos.is_some() {
                return;
            }
            match shape {
                egui::Shape::Text(t) if t.galley.text().starts_with("cue_") => {
                    *out_pos = Some(egui::pos2(t.pos.x + 8.0, t.pos.y + 4.0));
                }
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        find_cue_pos(s, out_pos);
                    }
                }
                _ => {}
            }
        }
        for s in &after_enter.shapes {
            find_cue_pos(&s.shape, &mut row2_pos);
        }
        let row2_pos = row2_pos.expect("confirmed cue_ row rendered as TableRow");
        let _ = click_point(&ctx, row2_pos, &mut |ui| {
            editor.frame_video_section(ui);
        });
        let _ = click_point(&ctx, remove_pos, &mut |ui| {
            editor.frame_video_section(ui);
        });
        let remaining = &editor.frame_video().unwrap().1.cue_points;
        assert_eq!(
            remaining.len(),
            1,
            "clicking Remove cue point must delete the selected cue point"
        );
        assert_eq!(remaining[0].name, "intro");
        editor.project.validate().expect("project remains valid");
    }
}
