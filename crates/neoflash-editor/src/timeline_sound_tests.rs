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
