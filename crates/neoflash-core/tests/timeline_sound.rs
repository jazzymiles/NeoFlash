//! Timeline (frame) sounds: model validation and the Player's sound commands for each
//! Flash sync mode, including loops, goto/seek, stop and Restart.
use neoflash_core::{
    AuthoredPlayback, FrameSound, Keyframe, Layer, LayerProperties, PlaybackCommand, Player,
    Project, ResourceUnit, SoundCommand, SoundEffect, SoundSync, Timeline,
};

fn layer(id: &str, keyframes: Vec<Keyframe>) -> Layer {
    Layer {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes,
        properties: LayerProperties::default(),
    }
}

fn key(frame: u32, sound: Option<(&str, SoundSync)>) -> Keyframe {
    Keyframe {
        frame,
        sound: sound.map(|(resource, sync)| FrameSound {
            sync,
            ..FrameSound::new(resource)
        }),
        ..Default::default()
    }
}

fn project(frame_count: u32, layers: Vec<Layer>) -> Project {
    let mut project = Project::blank();
    project.fps = 10;
    project.symbols[0].timeline = Timeline {
        frame_count,
        layers,
        scripts: vec![],
        guides: vec![],
    };
    project.resources = vec![
        ResourceUnit::from_bytes("beep", "audio/beep.wav", "audio/wav", b"RIFF....WAVE").unwrap(),
        ResourceUnit::from_bytes("song", "audio/song.mp3", "audio/mpeg", b"ID3").unwrap(),
        ResourceUnit::from_bytes("pic", "images/pic.png", "image/png", b"PNG").unwrap(),
    ];
    project.format_version = 6;
    project.validate().unwrap();
    project
}

fn ops(commands: &[SoundCommand]) -> Vec<String> {
    commands
        .iter()
        .map(|command| match command {
            SoundCommand::Play { resource, sync, .. } => format!("play {resource} {sync:?}"),
            SoundCommand::Stop { resource } => format!("stop {resource}"),
            SoundCommand::StreamStart {
                resource, offset, ..
            } => format!("stream {resource} @{offset:.1}"),
            SoundCommand::StreamStop { .. } => "stream-stop".into(),
            SoundCommand::StopAll => "stop-all".into(),
        })
        .collect()
}

fn tick(player: &mut Player, project: &Project) -> Vec<String> {
    player.tick(project);
    ops(&player.drain_sound_commands())
}

#[test]
fn event_start_and_stop_fire_once_per_keyframe_entry() {
    let project = project(
        6,
        vec![
            layer(
                "fx",
                vec![
                    key(0, None),
                    key(2, Some(("beep", SoundSync::Event))),
                    key(3, Some(("song", SoundSync::Start))),
                    key(5, Some(("song", SoundSync::Stop))),
                ],
            ),
            layer("art", vec![key(0, None)]),
        ],
    );
    let mut player = Player::new(&project);
    assert!(player.drain_sound_commands().is_empty());
    assert!(tick(&mut player, &project).is_empty());
    assert_eq!(tick(&mut player, &project), ["play beep Event"]);
    assert_eq!(tick(&mut player, &project), ["play song Start"]);
    // Staying inside the span does not re-trigger.
    assert!(tick(&mut player, &project).is_empty());
    assert_eq!(tick(&mut player, &project), ["stop song"]);
    // Looping back re-enters frame 2 and plays the Event sound again (overlapping).
    assert!(tick(&mut player, &project).is_empty());
    assert!(tick(&mut player, &project).is_empty());
    assert_eq!(tick(&mut player, &project), ["play beep Event"]);
    // gotoAndPlay re-enters a keyframe; a stopped timeline triggers nothing more.
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(3)));
    assert_eq!(ops(&player.drain_sound_commands()), ["play song Start"]);
    assert!(tick(&mut player, &project).is_empty());
}

#[test]
fn event_sound_carries_repeat_loop_and_effect_envelope() {
    let mut sound_key = key(0, Some(("beep", SoundSync::Event)));
    let sound = sound_key.sound.as_mut().unwrap();
    sound.loops = 3;
    sound.effect = SoundEffect::FadeIn;
    let project = project(2, vec![layer("fx", vec![sound_key])]);
    let mut player = Player::new(&project);
    let commands = player.drain_sound_commands();
    assert_eq!(
        commands,
        [SoundCommand::Play {
            resource: "beep".into(),
            sync: SoundSync::Event,
            loops: 3,
            loop_forever: false,
            envelope: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        }]
    );
    let json = serde_json::to_value(&commands[0]).unwrap();
    assert_eq!(json["op"], "play");
    assert_eq!(json["sync"], "event");
    assert_eq!(SoundEffect::FadeLeftToRight.gains_at(0.25), (0.75, 0.25));
}

#[test]
fn stream_follows_the_playhead_through_its_span() {
    let project = project(
        10,
        vec![layer(
            "music",
            vec![
                key(0, None),
                key(2, Some(("song", SoundSync::Stream))),
                key(6, None),
            ],
        )],
    );
    let mut player = Player::new(&project);
    assert!(player.drain_sound_commands().is_empty());
    assert!(tick(&mut player, &project).is_empty());
    assert_eq!(tick(&mut player, &project), ["stream song @0.0"]);
    // Continuous playback keeps the one stream running.
    for _ in 3..6 {
        assert!(tick(&mut player, &project).is_empty());
    }
    // Leaving the span (next keyframe) stops it.
    assert_eq!(tick(&mut player, &project), ["stream-stop"]);

    // A goto into the middle of the span starts at the frame offset (frame 4 = 0.2 s at 10 fps).
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndPlay(4)));
    assert_eq!(ops(&player.drain_sound_commands()), ["stream song @0.2"]);
    assert!(tick(&mut player, &project).is_empty());
    // A jump inside the span restarts the stream at the new offset.
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndPlay(3)));
    assert_eq!(
        ops(&player.drain_sound_commands()),
        ["stream-stop", "stream song @0.1"]
    );
    // stop() on the timeline stops the stream; play() resumes it at the current offset.
    assert!(player.control(&project, &[], PlaybackCommand::Stop));
    assert_eq!(ops(&player.drain_sound_commands()), ["stream-stop"]);
    assert!(tick(&mut player, &project).is_empty());
    assert!(player.control(&project, &[], PlaybackCommand::Play));
    assert_eq!(ops(&player.drain_sound_commands()), ["stream song @0.1"]);
    assert!(tick(&mut player, &project).is_empty());
    // A transport pause stops streams; resuming restarts at the offset of the next frame.
    player.set_playing(false);
    assert_eq!(ops(&player.drain_sound_commands()), ["stream-stop"]);
    player.set_playing(true);
    assert_eq!(tick(&mut player, &project), ["stream song @0.3"]);
    // gotoAndStop inside the span is silent.
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(4)));
    assert_eq!(ops(&player.drain_sound_commands()), ["stream-stop"]);
}

#[test]
fn stream_restarts_when_the_timeline_loops_and_seek_resyncs() {
    let project = project(
        4,
        vec![layer(
            "music",
            vec![key(0, Some(("song", SoundSync::Stream)))],
        )],
    );
    let mut player = Player::new(&project);
    assert_eq!(ops(&player.drain_sound_commands()), ["stream song @0.0"]);
    for _ in 1..4 {
        assert!(tick(&mut player, &project).is_empty());
    }
    assert_eq!(
        tick(&mut player, &project),
        ["stream-stop", "stream song @0.0"]
    );
    player.seek(&project, 3);
    assert_eq!(
        ops(&player.drain_sound_commands()),
        ["stream-stop", "stream song @0.3"]
    );
    // The editor preview starting mid-span hears only the stream at its offset.
    let mut preview = AuthoredPlayback::new(&project, &project.root, 2);
    assert_eq!(
        ops(&preview.drain_sound_commands()),
        ["stream-stop", "stream song @0.2"]
    );
    preview.tick(&project);
    assert!(preview.drain_sound_commands().is_empty());
}

#[test]
fn stop_all_and_restart_silence_everything() {
    let project = project(
        3,
        vec![layer(
            "music",
            vec![key(0, Some(("song", SoundSync::Stream)))],
        )],
    );
    let mut player = Player::new(&project);
    player.drain_sound_commands();
    player.stop_all_sounds();
    assert_eq!(ops(&player.drain_sound_commands()), ["stop-all"]);
    // The stream resumes at its offset on the next frame, as Flash streams do.
    assert_eq!(tick(&mut player, &project), ["stream song @0.1"]);
    player.reset(&project);
    assert_eq!(
        ops(&player.drain_sound_commands()),
        ["stop-all", "stream song @0.0"]
    );
}

#[test]
fn sounds_in_nested_movie_clips_and_guide_layers() {
    use neoflash_core::{Element, ElementKind, InstancePlayback, LayerRole, Symbol, SymbolKind};
    let mut project = project(4, vec![]);
    project.symbols.push(Symbol {
        id: "clip".into(),
        name: "clip".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![layer(
                "clip-fx",
                vec![key(1, Some(("beep", SoundSync::Event)))],
            )],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    });
    let instance = Element {
        id: "inst".into(),
        name: "inst".into(),
        transform: Default::default(),
        kind: ElementKind::Instance {
            symbol_id: "clip".into(),
            playback: InstancePlayback::Independent,
        },
        scripts: vec![],
        accessibility: None,
    };
    let mut guide = layer("guide", vec![key(0, Some(("song", SoundSync::Event)))]);
    guide.properties.role = LayerRole::Guide;
    project.symbols[0].timeline.layers = vec![
        guide,
        layer(
            "art",
            vec![Keyframe {
                frame: 0,
                elements: vec![instance],
                ..Default::default()
            }],
        ),
    ];
    project.validate().unwrap();
    let mut player = Player::new(&project);
    assert!(
        player.drain_sound_commands().is_empty(),
        "guide layers are not published"
    );
    assert_eq!(tick(&mut player, &project), ["play beep Event"]);
}

#[test]
fn validation_requires_an_audio_resource_and_format_6() {
    let mut project = project(
        2,
        vec![layer("fx", vec![key(0, Some(("beep", SoundSync::Event)))])],
    );
    let json = project.to_json().unwrap();
    assert!(json.contains("\"sound\""));
    assert!(
        !json.contains("\"sync\""),
        "Event sync and defaults are not serialized"
    );
    assert_eq!(Project::from_json(&json).unwrap(), project);
    // Older documents without sounds still parse with the field absent.
    let legacy = r#"{"frame":0,"elements":[],"tween":null,"label":""}"#;
    assert_eq!(
        serde_json::from_str::<Keyframe>(legacy).unwrap().sound,
        None
    );
    let parsed: FrameSound = serde_json::from_str(r#"{"resource":"beep"}"#).unwrap();
    assert_eq!(parsed, FrameSound::new("beep"));

    let sound = |p: &mut Project| {
        p.symbols[0].timeline.layers[0].keyframes[0]
            .sound
            .as_mut()
            .unwrap()
            .resource = "pic".into();
    };
    sound(&mut project);
    assert!(
        project
            .validate()
            .unwrap_err()
            .to_string()
            .contains("not audio")
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .sound
        .as_mut()
        .unwrap()
        .resource = "missing".into();
    assert!(
        project
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .sound
        .as_mut()
        .unwrap()
        .resource = "beep".into();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .sound
        .as_mut()
        .unwrap()
        .loops = 0;
    assert!(
        project
            .validate()
            .unwrap_err()
            .to_string()
            .contains("repeat count")
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .sound
        .as_mut()
        .unwrap()
        .loops = 1;
    project.format_version = 5;
    assert!(project.uses_format_6());
    assert!(
        project
            .validate()
            .unwrap_err()
            .to_string()
            .contains("frame sounds")
    );
    project.upgrade_format_for_features();
    assert_eq!(project.format_version, 6);
}

#[test]
fn custom_stereo_sound_envelope_emits_points_and_validates_bounds() {
    use neoflash_core::SoundEffect;
    let pts = vec![[0.0, 0.2, 0.9], [0.5, 1.0, 0.3], [1.0, 0.0, 0.8]];
    let mut proj = project(
        4,
        vec![layer("fx", vec![key(0, Some(("beep", SoundSync::Event)))])],
    );
    {
        let sound = proj.symbols[0].timeline.layers[0].keyframes[0]
            .sound
            .as_mut()
            .unwrap();
        sound.effect = SoundEffect::Custom;
        sound.envelope = Some(pts.clone());
        let (gl, gr) = sound.gains_at(0.25);
        assert!((gl - 0.6).abs() < 1e-9 && (gr - 0.6).abs() < 1e-9);
    }
    assert!(proj.uses_format_7());
    assert!(
        proj.validate().is_err(),
        "format 6 rejects custom envelopes"
    );
    proj.upgrade_format_for_features();
    assert_eq!(proj.format_version, 7);
    proj.validate().unwrap();

    let mut player = Player::new(&proj);
    let cmds = player.drain_sound_commands();
    assert!(matches!(
        cmds.as_slice(),
        [SoundCommand::Play { envelope, .. }] if *envelope == pts
    ));

    let round_trip = Project::from_json(&proj.to_json().unwrap()).unwrap();
    assert_eq!(round_trip, proj);

    // Reject empty, >32 points, unsorted timestamps, and out-of-range gains.
    for bad in [
        vec![],
        (0..33).map(|i| [i as f64 / 32.0, 1.0, 1.0]).collect(),
        vec![[0.6, 1.0, 1.0], [0.2, 1.0, 1.0]],
        vec![[0.0, 1.5, 0.5]],
    ] {
        let mut invalid = proj.clone();
        invalid.symbols[0].timeline.layers[0].keyframes[0]
            .sound
            .as_mut()
            .unwrap()
            .envelope = Some(bad);
        assert!(invalid.validate().is_err());
    }
}
