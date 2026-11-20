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
    Editor::open_entry_point(&ctx, "Library menu ▸ Edit envelope…");
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
    // v3: `Result` is the row label, the caption beside it is the value.
    assert!(
        texts.iter().any(|t| t == "Result") && texts.iter().any(|t| t.starts_with("160 kbps mono")),
        "missing live export result row in {texts:?}"
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

fn stereo_wav_bytes(frames: usize, rate: u32, left_amp: f32, right_amp: f32) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let sign = if i % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
        let l = (sign * left_amp * 32_767.0)
            .round()
            .clamp(-32_768.0, 32_767.0) as i16;
        let r = (sign * right_amp * 32_767.0)
            .round()
            .clamp(-32_768.0, 32_767.0) as i16;
        pcm.extend_from_slice(&l.to_le_bytes());
        pcm.extend_from_slice(&r.to_le_bytes());
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&pcm);
    bytes
}

#[test]
fn edit_envelope_renders_decoded_stereo_waveform_behind_curve_and_commits_trim_in_one_undo() {
    let ctx = egui::Context::default();
    let mut editor = editor_in(&ctx);
    let stereo_bytes = stereo_wav_bytes(1000, 1000, 0.9, 0.0); // 1.0 s stereo: loud L, silent R
    editor.project.resources.push(
        ResourceUnit::from_bytes("stereo", "audio/stereo.wav", "audio/wav", &stereo_bytes).unwrap(),
    );
    editor.layer = 1;
    editor.frame = 0;
    editor.set_frame_sound(Some("stereo".into()));
    editor.edit_frame_sound(|s| s.effect = SoundEffect::FadeIn);
    editor.clear_history();
    let before = editor.project.clone();
    assert_eq!(before.format_version, 6);

    editor.open_edit_envelope();
    assert!(editor.edit_envelope.is_some());

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let mut out = egui::FullOutput::default();
    for pass in 0..2 {
        out = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(pass as f64),
                ..Default::default()
            },
            |ctx| editor.edit_envelope_dialog(ctx),
        );
    }

    fn flatten_shapes<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
        match shape {
            egui::Shape::Vec(children) => {
                for child in children {
                    flatten_shapes(child, out);
                }
            }
            other => out.push(other),
        }
    }
    fn collect_flat(output: &egui::FullOutput) -> Vec<&egui::Shape> {
        let mut flat = Vec::new();
        for clipped in &output.shapes {
            flatten_shapes(&clipped.shape, &mut flat);
        }
        flat
    }
    let flat_shapes = collect_flat(&out);

    // Find the ruler bounds from the "0s" and "1s" text labels to split L vs R and drag trim handles!
    let mut ruler_left_x = None;
    let mut ruler_right_x = None;
    let mut ruler_center_y = None;
    for shape in &flat_shapes {
        if let egui::Shape::Text(t) = shape {
            if t.galley.text() == "0.0s" || t.galley.text() == "0s" {
                ruler_left_x = Some(t.pos.x);
                ruler_center_y = Some(t.pos.y + t.galley.size().y * 0.5);
            } else if t.galley.text() == "1.0s" || t.galley.text() == "1s" {
                ruler_right_x = Some(t.pos.x + t.galley.size().x);
            }
        }
    }
    let left_x = ruler_left_x.expect("0s label on ruler");
    let right_x = ruler_right_x.expect("1s label on ruler");
    let ry = ruler_center_y.expect("ruler y");
    let width = right_x - left_x;
    assert!(width > 100.0);

    let nf = neoflash_ui::palette(&ctx);
    // Collect `timeline_static` row background rects, envelope curves, and `success_default`
    // vertical waveform bars to verify z-order AND that synthetic PRNG bars are stripped!
    let mut l_bg_indices: Vec<usize> = Vec::new();
    let mut r_bg_indices: Vec<usize> = Vec::new();
    let mut curve_indices_and_y: Vec<(usize, f32)> = Vec::new();
    let mut wave_bars: Vec<(usize, f32, f32)> = Vec::new(); // (shape_idx, center_y, height)
    for (idx, shape) in flat_shapes.iter().enumerate() {
        match shape {
            egui::Shape::Rect(r) if r.fill == nf.timeline_static && r.rect.width() > 100.0 => {
                if r.rect.center().y < ry {
                    l_bg_indices.push(idx);
                } else {
                    r_bg_indices.push(idx);
                }
            }
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == nf.success_default
                    && (points[0].x - points[1].x).abs() < 1e-3 =>
            {
                let cy = (points[0].y + points[1].y) * 0.5;
                let h = (points[1].y - points[0].y).abs();
                wave_bars.push((idx, cy, h));
            }
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == nf.accent_default
                    && (points[1].x - points[0].x).abs() > 50.0 =>
            {
                curve_indices_and_y.push((idx, points[0].y.min(points[1].y)));
            }
            egui::Shape::Path(path) if !path.closed && (path.stroke.width - 1.5).abs() < 1e-3 => {
                if let Some(first) = path.points.first() {
                    curve_indices_and_y.push((idx, first.y));
                }
            }
            _ => {}
        }
    }

    assert_eq!(
        l_bg_indices.len(),
        2,
        "expected 2 timeline_static bg rects on L lane"
    );
    assert_eq!(
        r_bg_indices.len(),
        2,
        "expected 2 timeline_static bg rects on R lane"
    );
    assert_eq!(
        curve_indices_and_y.len(),
        2,
        "expected L and R envelope curves"
    );
    assert!(
        wave_bars.len() >= 100,
        "expected decoded stereo waveform peak bars, got {}",
        wave_bars.len()
    );
    let last_l_bg_idx = *l_bg_indices.last().unwrap();
    let last_r_bg_idx = *r_bg_indices.last().unwrap();
    let (curve_l_idx, _) = curve_indices_and_y[0];
    let (curve_r_idx, _) = curve_indices_and_y[1];
    let mut l_bars = Vec::new();
    let mut r_bars = Vec::new();
    for &(idx, cy, h) in &wave_bars {
        if cy < ry {
            assert!(
                idx > last_l_bg_idx && idx < curve_l_idx,
                "L waveform peak bar ({idx}) must sit above both L timeline_static bgs ({last_l_bg_idx}) and behind L curve ({curve_l_idx})"
            );
            l_bars.push(h);
        } else {
            assert!(
                idx > last_r_bg_idx && idx < curve_r_idx,
                "R waveform peak bar ({idx}) must sit above both R timeline_static bgs ({last_r_bg_idx}) and behind R curve ({curve_r_idx})"
            );
            r_bars.push(h);
        }
    }
    assert!(!l_bars.is_empty() && !r_bars.is_empty());
    let max_l = l_bars.iter().copied().fold(0.0_f32, f32::max);
    let max_r = r_bars.iter().copied().fold(0.0_f32, f32::max);
    assert!(
        max_l > 20.0,
        "expected loud decoded L peak > 20.0 px, got {max_l}"
    );
    assert!(
        max_r <= 2.0,
        "expected silent decoded R channel to strip synthetic PRNG bars (max_r <= 2.0 px), got {max_r}"
    );

    // Drag start trim handle from left_x + 1 to 25% (0.25s) and end trim handle from right_x - 1 to 75% (0.75s).
    let mut sim_time = 2.0_f64;
    let mut drag_handle = |editor: &mut Editor, from_x: f32, to_x: f32| {
        let start_pos = egui::pos2(from_x, ry);
        let end_pos = egui::pos2(to_x, ry);
        sim_time += 0.1;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(sim_time),
                events: vec![
                    egui::Event::PointerMoved(start_pos),
                    egui::Event::PointerButton {
                        pos: start_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| editor.edit_envelope_dialog(ctx),
        );
        sim_time += 0.1;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(sim_time),
                events: vec![egui::Event::PointerMoved(end_pos)],
                ..Default::default()
            },
            |ctx| editor.edit_envelope_dialog(ctx),
        );
        sim_time += 0.1;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(sim_time),
                events: vec![egui::Event::PointerButton {
                    pos: end_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| editor.edit_envelope_dialog(ctx),
        );
    };

    drag_handle(&mut editor, left_x + 1.0, left_x + width * 0.25);
    drag_handle(&mut editor, right_x - 1.0, left_x + width * 0.75);

    let draft = editor.edit_envelope.clone().expect("draft still open");
    let inp = draft.in_point.expect("in_point set by ruler drag");
    let outp = draft.out_point.expect("out_point set by ruler drag");
    assert!((inp - 0.25).abs() < 0.03, "expected ~0.25s, got {inp}");
    assert!((outp - 0.75).abs() < 0.03, "expected ~0.75s, got {outp}");

    // Verify trim-shade overlay rects also sit strictly above both `timeline_static` backgrounds
    // and strictly behind each lane's envelope curve!
    let trimmed_out = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(sim_time + 1.0),
            ..Default::default()
        },
        |ctx| editor.edit_envelope_dialog(ctx),
    );
    let trimmed_flat = collect_flat(&trimmed_out);
    let mut trim_l_bgs = Vec::new();
    let mut trim_r_bgs = Vec::new();
    let mut trim_l_shades = Vec::new();
    let mut trim_r_shades = Vec::new();
    let mut trim_curves = Vec::new();
    for (idx, shape) in trimmed_flat.iter().enumerate() {
        match shape {
            egui::Shape::Rect(r) if r.fill == nf.timeline_static && r.rect.width() > 100.0 => {
                if r.rect.center().y < ry {
                    trim_l_bgs.push(idx);
                } else {
                    trim_r_bgs.push(idx);
                }
            }
            // Exclude the full-screen modal backdrop (`r.rect.width() < 1000.0`).
            egui::Shape::Rect(r)
                if r.fill == nf.overlay && r.rect.width() > 5.0 && r.rect.width() < 1000.0 =>
            {
                if r.rect.center().y < ry {
                    trim_l_shades.push(idx);
                } else {
                    trim_r_shades.push(idx);
                }
            }
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == nf.accent_default
                    && (points[1].x - points[0].x).abs() > 50.0 =>
            {
                trim_curves.push(idx);
            }
            egui::Shape::Path(path) if !path.closed && (path.stroke.width - 1.5).abs() < 1e-3 => {
                trim_curves.push(idx);
            }
            _ => {}
        }
    }
    assert_eq!(
        trim_l_shades.len(),
        2,
        "expected in/out trim shades on L lane"
    );
    assert_eq!(
        trim_r_shades.len(),
        2,
        "expected in/out trim shades on R lane"
    );
    assert_eq!(trim_curves.len(), 2);
    for idx in trim_l_shades {
        assert!(
            idx > *trim_l_bgs.last().unwrap() && idx < trim_curves[0],
            "L trim shade ({idx}) must sit between L backgrounds and L curve"
        );
    }
    for idx in trim_r_shades {
        assert!(
            idx > *trim_r_bgs.last().unwrap() && idx < trim_curves[1],
            "R trim shade ({idx}) must sit between R backgrounds and R curve"
        );
    }

    // Preview audition respects the draft trim window, and VoiceSource yields only the trimmed segment!
    let preview = draft.preview_sound(editor.frame_sound().map(|(_, s)| s).as_ref());
    editor.timeline_audio.audition(&editor.project, &preview);
    assert!(editor.timeline_audio.is_auditioning());
    assert_eq!(
        editor.timeline_audio.last_audition_trim,
        Some((Some(inp), Some(outp)))
    );
    let decoded = editor
        .timeline_audio
        .decoded(&editor.project, "stereo")
        .unwrap();
    let trimmed_source = crate::timeline_sound::VoiceSource::new(
        decoded,
        0.0,
        1,
        false,
        (Some(inp), Some(outp)),
        vec![],
    )
    .expect("trimmed voice source");
    let sample_count = trimmed_source.count();
    let expected_samples = ((outp - inp) * 1000.0).round() as usize * 2;
    assert!(
        sample_count.abs_diff(expected_samples) <= 4,
        "expected ~{expected_samples} stereo samples, got {sample_count}"
    );

    // Confirm with Enter: commits in a single Undo step and upgrades format_version to 7!
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| editor.edit_envelope_dialog(ctx),
    );
    assert!(editor.edit_envelope.is_none());
    let (_, committed) = editor.frame_sound().unwrap();
    assert_eq!(committed.in_point, Some(inp));
    assert_eq!(committed.out_point, Some(outp));
    assert_eq!(editor.project.format_version, 7);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(
        editor.project, before,
        "single Undo restores pre-dialog document"
    );
}
