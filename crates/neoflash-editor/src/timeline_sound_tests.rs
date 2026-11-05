//! Timeline sounds in the editor: Sound button / Properties edits as single Undo steps, Sound
//! layers, import, playback commands for the native preview and Control ▸ Mute sounds.
use crate::app::Editor;
use crate::layer_controls::layer_kind;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::timeline::LayerKind;

/// 16-bit mono PCM WAV of `samples` silent-ish samples at 8 kHz.
fn wav(samples: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..samples)
        .flat_map(|i| (((i % 16) as i16 - 8) * 512).to_le_bytes())
        .collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    for value in [16u32, 1 | (1 << 16), 8000, 16000, 2 | (16 << 16)] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    bytes
}

/// Root with 20 frames; layer 0 "art" holds a rectangle at frame 0, layer 1 "fx" is empty.
fn editor() -> Editor {
    editor_in(&egui::Context::default())
}

fn editor_in(ctx: &egui::Context) -> Editor {
    let mut project = Project::blank();
    project.format_version = 5;
    project
        .resources
        .push(ResourceUnit::from_bytes("beep", "audio/beep.wav", "audio/wav", &wav(4000)).unwrap());
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 20;
    timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "box".into(),
        name: "box".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    }];
    let mut fx = timeline.layers[0].clone();
    fx.id = "fx".into();
    fx.name = "fx".into();
    fx.keyframes.clear();
    timeline.layers.push(fx);
    project.validate().unwrap();
    Editor::from_project(ctx, project)
}

#[test]
fn sound_button_attaches_to_the_span_keyframe_with_one_undo() {
    let mut editor = editor();
    let before = editor.project.clone();
    assert_eq!(
        editor.audio_resources(),
        [("beep".into(), "audio/beep.wav".into())]
    );
    // The empty "fx" layer gets a blank keyframe at the playhead.
    editor.layer = 1;
    editor.frame = 4;
    editor.set_frame_sound(Some("beep".into()));
    let (key, sound) = editor.frame_sound().expect("sound attached");
    assert_eq!((key, sound), (4, FrameSound::new("beep")));
    assert_eq!(
        editor.project.format_version, 6,
        "frame sounds upgrade the format"
    );
    editor.project.validate().unwrap();
    // Properties shows the Sound section: the keyframe is selected.
    assert_eq!(editor.selected_frame_range(), Some((4, 4, 1)));
    let timeline = &editor.symbol().timeline;
    assert_eq!(layer_kind(timeline, &timeline.layers[1]), LayerKind::Sound);
    assert_eq!(layer_kind(timeline, &timeline.layers[0]), LayerKind::Layer);
    // Inside the span the playhead edits the same keyframe.
    editor.frame = 9;
    editor.edit_frame_sound(|sound| {
        sound.sync = SoundSync::Stream;
        sound.loops = 3;
        sound.effect = SoundEffect::FadeOut;
    });
    let (key, sound) = editor.frame_sound().unwrap();
    assert_eq!(key, 4);
    assert_eq!(
        (sound.sync, sound.loops, sound.effect),
        (SoundSync::Stream, 3, SoundEffect::FadeOut)
    );
    // A new keyframe inside the span does not duplicate the sound.
    editor.make_key(false);
    assert_eq!(editor.frame_sound(), None);
    editor.undo();
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, before, "each edit was one Undo step");
}

#[test]
fn removing_a_sound_and_its_resource() {
    let mut editor = editor();
    editor.set_frame_sound(Some("beep".into()));
    assert_eq!(
        editor.frame_sound().unwrap().0,
        0,
        "attached to the art layer's keyframe"
    );
    let timeline = &editor.symbol().timeline;
    assert_eq!(
        layer_kind(timeline, &timeline.layers[0]),
        LayerKind::Layer,
        "a layer with artwork stays a normal layer"
    );
    // A resource in use cannot be dropped silently.
    let mut candidate = editor.project.clone();
    candidate.resources.clear();
    assert!(
        candidate
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
    editor.set_frame_sound(None);
    assert_eq!(editor.frame_sound(), None);
    assert!(editor.project.resources.iter().any(|r| r.id == "beep"));
}

#[test]
fn import_sound_adds_the_resource_and_attaches_it_in_one_step() {
    let mut editor = editor();
    let before = editor.project.clone();
    let directory = std::env::temp_dir().join(format!("neoflash-sound-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("chime.wav");
    std::fs::write(&path, wav(800)).unwrap();
    editor.layer = 1;
    editor.import_frame_sound(&path).unwrap();
    let (_, sound) = editor.frame_sound().unwrap();
    let unit = editor
        .project
        .resources
        .iter()
        .find(|unit| unit.id == sound.resource)
        .unwrap();
    assert_eq!(
        (unit.path.as_str(), unit.media_type.as_str()),
        ("chime.wav", "audio/wav")
    );
    let text = directory.join("notes.txt");
    std::fs::write(&text, "not audio").unwrap();
    assert!(
        editor
            .import_frame_sound(&text)
            .unwrap_err()
            .contains("not a sound file")
    );
    std::fs::remove_dir_all(&directory).unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn editor_playback_feeds_sound_commands_and_mute() {
    let mut editor = editor();
    editor.layer = 1;
    editor.frame = 2;
    editor.set_frame_sound(Some("beep".into()));
    editor.layer = 1;
    editor.frame = 6;
    editor.make_key(true);
    editor.frame = 2;
    editor.edit_frame_sound(|sound| sound.sync = SoundSync::Stream);
    editor.frame = 0;
    editor.set_playing(true);
    editor.pump_timeline_audio();
    assert!(editor.timeline_audio.history.is_empty());
    for _ in 0..2 {
        editor.tick_authored_preview();
        editor.pump_timeline_audio();
    }
    assert!(matches!(
        editor.timeline_audio.history.as_slice(),
        [SoundCommand::StreamStart { resource, offset, .. }] if resource == "beep" && *offset == 0.
    ));
    for _ in 2..6 {
        editor.tick_authored_preview();
        editor.pump_timeline_audio();
    }
    assert!(matches!(
        editor.timeline_audio.history.last(),
        Some(SoundCommand::StreamStop { .. })
    ));
    editor.set_playing(false);
    editor.pump_timeline_audio();
    // Starting inside the span resumes the stream at the playhead's offset.
    editor.frame = 4;
    editor.set_playing(true);
    editor.pump_timeline_audio();
    let fps = f64::from(editor.project.fps);
    assert!(matches!(
        editor.timeline_audio.history.last(),
        Some(SoundCommand::StreamStart { offset, .. }) if (*offset - 2. / fps).abs() < 1e-9
    ));
    editor.set_playing(false);
    editor.toggle_mute_sounds();
    assert!(editor.timeline_audio.muted);
    editor.toggle_mute_sounds();
    assert!(!editor.timeline_audio.muted);
}

#[test]
fn properties_shows_decoded_sound_info() {
    let mut editor = editor();
    editor.set_frame_sound(Some("beep".into()));
    let (_, sound) = editor.frame_sound().unwrap();
    let decoded = editor
        .timeline_audio
        .decoded(&editor.project, &sound.resource)
        .unwrap();
    assert_eq!((decoded.rate(), decoded.channels()), (8000, 1));
    assert!((decoded.duration() - 0.5).abs() < 1e-9);
}

fn render(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920., 1080.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            editor.menu(ctx);
            editor.prepare_docked_panel_focus(ctx);
            editor.dock_grid(ctx);
            editor.stage(ctx);
        },
    )
}

fn labelled(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    let update = output.platform_output.accesskit_update.as_ref()?;
    update.nodes.iter().find_map(|(_, node)| {
        let bounds = node.bounds().filter(|_| node.label() == Some(label))?;
        Some(egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        ))
    })
}

#[test]
fn sound_button_menu_and_properties_section_are_wired() {
    use crate::tests::primary_button;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = editor_in(&ctx);
    editor.layer = 1;
    editor.frame = 3;
    let mut time = 0.;
    let mut step = |editor: &mut Editor, events: Vec<egui::Event>| {
        time += 0.1;
        render(editor, &ctx, time, events)
    };
    let click = |step: &mut dyn FnMut(&mut Editor, Vec<egui::Event>) -> egui::FullOutput,
                 editor: &mut Editor,
                 at: egui::Pos2| {
        step(
            editor,
            vec![egui::Event::PointerMoved(at), primary_button(at, true)],
        );
        step(editor, vec![primary_button(at, false)]);
        step(editor, vec![])
    };
    let out = step(&mut editor, vec![]);
    assert!(
        labelled(&out, "Repeat count").is_none(),
        "no Sound section without a frame sound"
    );
    let button = labelled(&out, "Sound").expect("timeline Sound button");
    let out = click(&mut step, &mut editor, button);
    assert!(labelled(&out, "Import sound…").is_some());
    let item = labelled(&out, "audio/beep.wav").expect("library sound in the menu");
    click(&mut step, &mut editor, item);
    assert_eq!(editor.frame_sound(), Some((3, FrameSound::new("beep"))));
    let mut out = step(&mut editor, vec![]);
    for _ in 0..2 {
        out = step(&mut editor, vec![]);
    }
    for label in ["Envelope…", "Repeat count", "Play", "Sound properties…"] {
        assert!(
            labelled(&out, label).is_some(),
            "Properties ▸ Sound shows {label}"
        );
    }
}

#[test]
fn edit_envelope_dialog_commits_custom_stereo_envelope_and_cancels_cleanly() {
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    let mut editor = editor_in(&ctx);
    editor.layer = 1;
    editor.frame = 2;
    editor.set_frame_sound(Some("beep".into()));
    let before = editor.project.clone();

    // Library menu entry point routes through ConvertedTrigger::EditEnvelope to the real modal dialog.
    Editor::open_planned(&ctx, "Library menu ▸ Edit envelope…");
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920., 1080.),
            )),
            ..Default::default()
        },
        |ctx| editor.draw_dialogs(ctx),
    );
    assert!(editor.has_modal_dialog());
    assert!(editor.edit_envelope.is_some());

    // Mutate draft in Seconds and Frames modes, then Cancel: 0 Undo steps and project unchanged.
    {
        let draft = editor.edit_envelope.as_mut().unwrap();
        draft.effect = SoundEffect::Custom;
        draft.left = vec![(0.0, 0.25), (1.0, 1.0)];
        draft.right = vec![(0.0, 1.0), (0.5, 0.5), (1.0, 0.0)];
        draft.unit = 1;
    }
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920., 1080.),
            )),
            ..Default::default()
        },
        |ctx| editor.draw_dialogs(ctx),
    );
    editor.cancel_edit_envelope();
    assert!(!editor.has_modal_dialog());
    assert_eq!(editor.project, before);

    // Reopen, shape independent L/R curves, and commit on OK: 1 Undo step, format 7, valid.
    editor.open_edit_envelope();
    let mut draft = editor.edit_envelope.take().unwrap();
    draft.effect = SoundEffect::Custom;
    draft.left = vec![(0.0, 0.25), (1.0, 1.0)];
    draft.right = vec![(0.0, 1.0), (0.5, 0.5), (1.0, 0.0)];
    editor.commit_edit_envelope(draft);
    assert_eq!(editor.project.format_version, 7);
    editor.project.validate().unwrap();

    let (_, sound) = editor.frame_sound().unwrap();
    assert_eq!(sound.effect, SoundEffect::Custom);
    let pts = sound.envelope.as_ref().expect("custom envelope stored");
    assert_eq!(pts.len(), 3);
    assert!((pts[1][0] - 0.5).abs() < 1e-6);
    assert!((pts[1][1] - 0.625).abs() < 1e-6, "L interpolated at t=0.5");
    assert!((pts[1][2] - 0.5).abs() < 1e-6, "R key at t=0.5");

    editor.undo();
    assert_eq!(editor.project, before, "OK commits in a single Undo step");
}

#[test]
fn waveform_peaks_cache_and_frame_sound_trim_playback_window() {
    use crate::timeline_sound::VoiceSource;
    let mut editor = editor();
    editor.layer = 1;
    editor.frame = 0;
    editor.set_frame_sound(Some("beep".into()));
    let with_sound = editor.project.clone();

    let ctx = egui::Context::default();
    assert!(
        editor
            .timeline_audio
            .waveform_peaks(&editor.project, "beep", 16, &ctx)
            .unwrap()
            .is_none(),
        "first request is nonblocking"
    );
    let peaks = wait_for_waveform(&mut editor, &ctx, "beep", 16);
    assert_eq!(peaks.buckets(), 16);
    assert_eq!((peaks.sample_rate, peaks.channels), (8000, 1));
    assert!((peaks.duration - 0.5).abs() < 1e-9);
    assert!(peaks.peak_amplitude(0, 0) > 0.1);
    let cached = editor
        .timeline_audio
        .waveform_peaks(&editor.project, "beep", 16, &ctx)
        .unwrap()
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&peaks, &cached));

    // Trim 0.10 s .. 0.35 s (0.25 s = 2000 frames at 8 kHz): upgrades to format 7 in one Undo step.
    editor.set_frame_sound_trim(Some(0.10), Some(0.35));
    assert_eq!(editor.project.format_version, 7);
    editor.project.validate().unwrap();
    let (_, sound) = editor.frame_sound().unwrap();
    assert_eq!((sound.in_point, sound.out_point), (Some(0.10), Some(0.35)));

    // Verify VoiceSource renders 2 loops of the 2000-frame trimmed segment (4000 stereo frames = 8000 samples).
    let decoded = editor
        .timeline_audio
        .decoded(&editor.project, "beep")
        .unwrap();
    let rendered: Vec<f32> = VoiceSource::new(
        decoded,
        0.0,
        2,
        false,
        (sound.in_point, sound.out_point),
        vec![],
    )
    .unwrap()
    .collect();
    assert_eq!(rendered.len(), 2000 * 2 * 2);

    editor.undo();
    assert_eq!(editor.project, with_sound);
}

/// Wait only in tests; the live UI receives a repaint from the worker.
fn wait_for_waveform(
    editor: &mut Editor,
    ctx: &egui::Context,
    resource: &str,
    buckets: usize,
) -> std::sync::Arc<WaveformPeaks> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(peaks) = editor
            .timeline_audio
            .waveform_peaks(&editor.project, resource, buckets, ctx)
            .unwrap()
        {
            return peaks;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "audio worker did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn waveform_worker_reuses_decode_across_zoom_and_invalidates_replaced_bytes() {
    let mut editor = editor();
    let ctx = egui::Context::default();
    let first = wait_for_waveform(&mut editor, &ctx, "beep", 16);
    let decoded = editor
        .timeline_audio
        .decoded_if_ready(&editor.project, "beep")
        .unwrap()
        .unwrap();
    let zoom = wait_for_waveform(&mut editor, &ctx, "beep", 32);
    assert_eq!(zoom.buckets(), 32);
    let reused = editor
        .timeline_audio
        .decoded_if_ready(&editor.project, "beep")
        .unwrap()
        .unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&decoded, &reused),
        "zoom reuses decoded PCM"
    );

    // A pending request for old bytes must not overwrite a replacement or an Undo source.
    editor
        .timeline_audio
        .waveform_peaks(&editor.project, "beep", 64, &ctx)
        .unwrap();
    let old_unit = editor.project.resources[0].clone();
    editor.project.resources[0] =
        ResourceUnit::from_bytes("beep", "audio/beep.wav", "audio/wav", &wav(800)).unwrap();
    assert!(
        editor
            .timeline_audio
            .decoded_if_ready(&editor.project, "beep")
            .is_none()
    );
    assert!(
        editor
            .timeline_audio
            .waveform_peaks(&editor.project, "beep", 16, &ctx)
            .unwrap()
            .is_none()
    );
    let replacement = wait_for_waveform(&mut editor, &ctx, "beep", 16);
    assert!((replacement.duration - 0.1).abs() < 1e-9);
    assert!(!std::sync::Arc::ptr_eq(&first, &replacement));
    editor.project.resources[0] = old_unit;
    let restored = wait_for_waveform(&mut editor, &ctx, "beep", 16);
    assert!((restored.duration - 0.5).abs() < 1e-9);

    editor.project.resources[0] =
        ResourceUnit::from_bytes("beep", "audio/bad.wav", "audio/wav", b"not a wave").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match editor
            .timeline_audio
            .waveform_peaks(&editor.project, "beep", 16, &ctx)
        {
            Err(error) => {
                assert!(error.contains("bad.wav"));
                break;
            }
            Ok(None) => {}
            Ok(Some(_)) => panic!("stale waveform for invalid replacement"),
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        editor
            .timeline_audio
            .waveform_peaks(&editor.project, "beep", 16, &ctx)
            .is_err(),
        "decode failures stay cached"
    );
}

#[test]
fn waveform_visible_columns_follow_stereo_trim_loops_scroll_and_silence() {
    use crate::timeline_sound::visible_waveform_columns;
    let peaks = sample_waveform_peaks(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], 2, 4, 4).unwrap();
    let mut sound = FrameSound::new("sound");
    sound.in_point = Some(0.25);
    sound.out_point = Some(0.75);
    sound.loops = 2;
    let full = visible_waveform_columns(&peaks, &sound, 0.25, 0., 6);
    assert_eq!(
        full[0],
        [
            (0., 0.3),
            (0., 0.5),
            (0., 0.3),
            (0., 0.5),
            (0., 0.),
            (0., 0.)
        ]
    );
    assert_eq!(
        full[1],
        [
            (0., 0.4),
            (0., 0.6),
            (0., 0.4),
            (0., 0.6),
            (0., 0.),
            (0., 0.)
        ]
    );
    let scrolled = visible_waveform_columns(&peaks, &sound, 0.25, 0.5, 4);
    assert_eq!(scrolled[0], full[0][2..]);
    assert_eq!(scrolled[1], full[1][2..]);
    sound.loop_forever = true;
    assert_eq!(
        visible_waveform_columns(&peaks, &sound, 0.25, 20., 2)[0],
        [(0., 0.3), (0., 0.5)]
    );
    assert_eq!(
        visible_waveform_columns(&peaks, &sound, 1., 0.375, 1)[1],
        [(0., 0.6)],
        "wide columns aggregate every repeated peak"
    );
    sound.sync = SoundSync::Stop;
    assert_eq!(
        visible_waveform_columns(&peaks, &sound, 0.25, 0., 2)[0],
        [(0., 0.), (0., 0.)]
    );
}

fn collect_shape_texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                collect_shape_texts(s, out);
            }
        }
        _ => {}
    }
}

fn output_texts(out: &egui::FullOutput) -> Vec<String> {
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_shape_texts(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn export_audio_modal_renders_both_themes_and_exports_wav_mp3_ogg_and_aac() {
    use crate::panels::export_audio::{
        export_audio_draft, has_export_audio_dialog, update_export_audio_draft,
    };
    use crate::theme::ThemeMode;
    use neoflash_export::{
        AudioBitDepth, AudioBitRate, AudioChannels, AudioExportRange, AudioExportSource,
        AudioFormat, AudioSampleRate,
    };

    let ctx = egui::Context::default();
    let mut editor = editor_in(&ctx);
    editor.layer = 1;
    editor.frame = 0;
    editor.set_frame_sound(Some("beep".into()));

    editor.open_export_audio(&ctx, None);
    assert!(has_export_audio_dialog(&ctx));
    let draft = export_audio_draft(&ctx).expect("draft opened");
    assert_eq!(draft.options.format, AudioFormat::Mp3);
    assert_eq!(draft.options.source, AudioExportSource::TimelineMix);
    assert!(draft.cached_duration > 0.0);

    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        editor.set_theme(&ctx, theme);
        let mut out = egui::FullOutput::default();
        for _ in 0..2 {
            out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1920.0, 1080.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    editor.export_audio_dialog(ctx);
                },
            );
        }
        let texts = output_texts(&out);
        for expected in [
            "Export audio",
            "WAV",
            "MP3",
            "OGG",
            "AAC",
            "SOURCE",
            "FORMAT",
            "PROCESSING",
            "Normalize to −1 dB",
            "Trim silence",
            "Export",
        ] {
            assert!(
                texts.iter().any(|t| t == expected),
                "missing '{expected}' in {theme:?}: {texts:?}"
            );
        }
    }

    let temp_dir =
        std::env::temp_dir().join(format!("nf2-export-audio-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    for (fmt, ext, source) in [
        (AudioFormat::Wav, "wav", AudioExportSource::TimelineMix),
        (
            AudioFormat::Mp3,
            "mp3",
            AudioExportSource::Layer("fx".into()),
        ),
        (
            AudioFormat::Ogg,
            "ogg",
            AudioExportSource::Resource("beep".into()),
        ),
        (AudioFormat::Aac, "m4a", AudioExportSource::TimelineMix),
    ] {
        editor.open_export_audio(&ctx, Some(source.clone()));
        update_export_audio_draft(&ctx, |d| {
            d.options.format = fmt;
            d.options.sample_rate = AudioSampleRate::Hz22050;
            d.options.bit_depth = AudioBitDepth::Bit16;
            d.options.channels = AudioChannels::Mono;
            d.options.bit_rate = AudioBitRate::Kbps128;
            d.options.range = AudioExportRange::WholeMovie;
            d.options.normalize = true;
            d.options.fade_out_seconds = 0.2;
            d.options.trim_silence = true;
        });
        let path = temp_dir.join(format!("out.{ext}"));
        let report = editor
            .confirm_export_audio_to_path(&ctx, &path)
            .expect("audio export succeeds");
        assert!(!has_export_audio_dialog(&ctx), "dialog closes after export");
        assert_eq!(report.format, fmt);
        assert!(report.bytes > 0);
        assert!(path.exists());
    }

    editor.open_export_audio(&ctx, None);
    assert!(has_export_audio_dialog(&ctx));
    editor.cancel_export_audio(&ctx);
    assert!(!has_export_audio_dialog(&ctx));
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn sound_properties_export_settings_commit_with_one_undo_and_upgrade_to_format_9() {
    use crate::panels::sound_properties::{
        has_sound_properties_dialog, update_sound_properties_draft_export,
    };

    let ctx = egui::Context::default();
    let mut editor = editor_in(&ctx);
    let before = editor.project.clone();
    assert_eq!(before.format_version, 5);
    assert!(before.resources[0].audio_export.is_none());

    editor.open_sound_properties(&ctx, Some("beep".into()));
    assert!(has_sound_properties_dialog(&ctx));
    update_sound_properties_draft_export(&ctx, |exp| {
        exp.compression = SoundCompression::Mp3;
        exp.bit_rate_kbps = 160;
        exp.quality = SoundExportQuality::Medium;
        exp.convert_to_mono = true;
    });

    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1080.0),
                )),
                ..Default::default()
            },
            |ctx| {
                editor.sound_properties_dialog(ctx);
            },
        );
    }
    let texts = output_texts(&out);
    assert!(
        texts.iter().any(|t| t.starts_with("Result: 160 kbps mono")),
        "missing live export result caption in {texts:?}"
    );

    editor.confirm_sound_properties(&ctx).unwrap();
    assert!(!has_sound_properties_dialog(&ctx));
    assert_eq!(editor.project.format_version, 9);
    assert_eq!(
        editor.project.resources[0].audio_export,
        Some(SoundExportSettings {
            compression: SoundCompression::Mp3,
            bit_rate_kbps: 160,
            quality: SoundExportQuality::Medium,
            convert_to_mono: true,
        })
    );
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);
}
