//! Regression suite for `[T-474]`: After Effects–style Finder & Library drag-and-drop import for
//! audio, video, and images onto Timeline, Stage, and Library (docked + detached floating windows),
//! stale `over_top_chrome` pointer routing, empty/standalone-`.as` session preservation, and
//! `NativeWindowFileDragQueue` (`public.file-url`) bridging.

use crate::app::Editor;
use crate::layer_controls::layer_kind;
use crate::library::LibraryDrag;
use crate::native_windows::{
    MAX_NATIVE_DRAG_FILE_URLS, NativeDragWindowTarget, NativeWindowFileDragQueue,
};
use crate::workspace::{PanelId, PanelPlacement};
use eframe::egui::{self, Pos2, Rect, Vec2};
use image::{ImageEncoder, codecs::png::PngEncoder};
use neoflash_core::*;
use neoflash_ui::components::timeline as tl;
use std::path::{Path, PathBuf};

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t474-{tag}-{}-{}",
            std::process::id(),
            new_id("t474")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 16-bit mono PCM WAV at 8,000 Hz (`24_000` samples = exact `3.0` seconds -> `72` frames @ 24 fps).
fn make_wav_bytes(samples_at_8khz: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..samples_at_8khz)
        .flat_map(|i| (((i % 16) as i16 - 8) * 512).to_le_bytes())
        .collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    for value in [16u32, 1 | (1 << 16), 8_000, 16_000, 2 | (16 << 16)] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    bytes
}

/// Valid CBR 128 kbps 44.1 kHz stereo MPEG-1 Layer III stream (`frames * 1152 / 44100` seconds).
fn make_mp3_bytes(frame_count: usize) -> Vec<u8> {
    let mut frame = vec![0u8; 417];
    frame[0] = 0xFF;
    frame[1] = 0xFB;
    frame[2] = 0x90;
    frame[3] = 0x64;
    let mut out = Vec::with_capacity(417 * frame_count);
    for _ in 0..frame_count {
        out.extend_from_slice(&frame);
    }
    out
}

/// Uncompressed 16-bit big-endian mono `FORM....AIFF` at 44,100 Hz (`0x400E_AC44_0000_0000_0000`).
fn make_aiff_bytes(samples_at_44k1: usize) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples_at_44k1 * 2);
    for i in 0..samples_at_44k1 {
        let s = (((i % 32) as i16 - 16) * 400).to_be_bytes();
        pcm.extend_from_slice(&s);
    }
    let mut comm = Vec::with_capacity(18);
    comm.extend_from_slice(&1i16.to_be_bytes());
    comm.extend_from_slice(&(samples_at_44k1 as u32).to_be_bytes());
    comm.extend_from_slice(&16i16.to_be_bytes());
    comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

    let ssnd_size = (8 + pcm.len()) as u32;
    let form_size = 4 + (8 + comm.len() as u32) + (8 + ssnd_size + (ssnd_size & 1));
    let mut out = Vec::with_capacity((form_size + 8) as usize);
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&form_size.to_be_bytes());
    out.extend_from_slice(b"AIFF");
    out.extend_from_slice(b"COMM");
    out.extend_from_slice(&(comm.len() as u32).to_be_bytes());
    out.extend_from_slice(&comm);
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&ssnd_size.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&pcm);
    if ssnd_size & 1 == 1 {
        out.push(0);
    }
    out
}

fn make_png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        raw.extend_from_slice(&rgba);
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(&raw, w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn make_avi_bytes(w: u16, h: u16, fps: u32, frame_count: usize) -> Vec<u8> {
    let mut jpeg_frame = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        u32::from(w),
        u32::from(h),
        image::Rgb([40, 160, 240]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut jpeg_frame),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    let frames = vec![jpeg_frame; frame_count.max(1)];
    build_minimal_mjpeg_avi(u32::from(w), u32::from(h), fps, &frames, None)
}

fn make_mp4_bytes() -> &'static [u8] {
    b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41"
}

fn make_one_frame_editor(ctx: &egui::Context) -> Editor {
    let mut project = Project::blank();
    project.fps = 24;
    project.symbols[0].timeline.frame_count = 1;
    let mut editor = Editor::from_project(ctx, project);
    editor.history.clear();
    editor
}

fn run_timeline_pass(
    ctx: &egui::Context,
    editor: &mut Editor,
    pos: Option<Pos2>,
    hovered: Vec<PathBuf>,
    dropped: Vec<PathBuf>,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut all_events = Vec::new();
    if let Some(p) = pos {
        all_events.push(egui::Event::PointerMoved(p));
    }
    all_events.extend(events);
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 360.0))),
        events: all_events,
        hovered_files: hovered
            .into_iter()
            .map(|path| egui::HoveredFile {
                path: Some(path),
                ..Default::default()
            })
            .collect(),
        dropped_files: dropped
            .into_iter()
            .map(|path| egui::DroppedFile {
                path: Some(path),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    ctx.run(raw, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            editor.timeline_contents(ui);
        });
    })
}

fn run_full_window_drop(
    ctx: &egui::Context,
    editor: &mut Editor,
    pos: Option<Pos2>,
    dropped: Vec<PathBuf>,
) {
    let mut events = Vec::new();
    if let Some(p) = pos {
        events.push(egui::Event::PointerMoved(p));
    }
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 960.0));
    let raw = egui::RawInput {
        screen_rect: Some(screen),
        viewports: std::iter::once((
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                inner_rect: Some(screen),
                outer_rect: Some(screen),
                ..Default::default()
            },
        ))
        .collect(),
        events,
        dropped_files: dropped
            .into_iter()
            .map(|path| egui::DroppedFile {
                path: Some(path),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let _ = ctx.run(raw, |ctx| {
        let _ = editor.handle_window_document_or_as_file_drop(ctx);
        let standalone_as = editor.is_standalone_script_session();
        if editor.has_document() {
            if standalone_as {
                editor.document_bar(ctx);
                editor.standalone_script_editor_central(ctx);
            } else {
                editor.dock_grid(ctx);
                editor.document_bar(ctx);
                editor.stage(ctx);
                editor.detached_panels(ctx);
            }
        } else {
            editor.no_document_view(ctx);
        }
        editor.open_deferred_stage_script_drops(ctx);
    });
}

#[test]
fn t474_timeline_drop_audio_video_images_individually_and_mixed_at_frame_12_and_empty_space() {
    let tmp = TempTestDir::new("tl-drops");
    let wav_path = tmp.path().join("drum_loop.wav");
    let mp3_path = tmp.path().join("voice_line.mp3");
    let aiff_path = tmp.path().join("synth_pad.aiff");
    let avi_path = tmp.path().join("intro_video.avi");
    let mp4_path = tmp.path().join("broll_clip.mp4");
    let png_path = tmp.path().join("character.png");
    let svg_path = tmp.path().join("badge.svg");

    std::fs::write(&wav_path, make_wav_bytes(24_000)).unwrap(); // 3.0s -> 72 frames @ 24fps
    std::fs::write(&mp3_path, make_mp3_bytes(40)).unwrap(); // ~1.04s -> 26 frames @ 24fps
    std::fs::write(&aiff_path, make_aiff_bytes(44_100)).unwrap(); // 1.0s -> 24 frames @ 24fps
    std::fs::write(&avi_path, make_avi_bytes(64, 48, 12, 24)).unwrap(); // 2.0s -> 48 frames @ 24fps
    std::fs::write(&mp4_path, make_mp4_bytes()).unwrap();
    std::fs::write(&png_path, make_png_bytes(48, 36, [240, 80, 40, 255])).unwrap();
    std::fs::write(
        &svg_path,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"32\"><circle cx=\"16\" cy=\"16\" r=\"14\" fill=\"rgb(34,170,136)\"/></svg>",
    )
    .unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    let initial = editor.project.clone();

    // Prime Timeline geometry and verify hover highlight paints without mutating Project.
    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .expect("Timeline drop geometry recorded")
        .clone();
    let cell_frame_12 = Pos2::new(
        zone.grid_origin_x + 12.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    let below_rows_frame_12 = Pos2::new(
        zone.grid_origin_x + 12.5 * zone.cell_width,
        zone.list_origin_y + 4.5 * zone.row_height,
    );

    let hover_out = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![wav_path.clone()],
        vec![],
        vec![],
    );
    assert!(
        hover_out.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.job.text == "Drop audio, video or images onto Timeline"
        )),
        "Timeline hover overlay must render drop prompt"
    );
    assert_eq!(editor.project, initial);

    // 1a. Drop 3.0s `.wav` onto `(Layer 1, frame = 12)` -> reuses empty Layer 1, expands 1 -> 84 frames!
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![],
        vec![wav_path.clone()],
        vec![],
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import audio");
    assert_eq!(editor.frame, 12);
    assert_eq!(editor.symbol().timeline.layers.len(), 1);
    assert_eq!(
        layer_kind(
            &editor.symbol().timeline,
            &editor.symbol().timeline.layers[0]
        ),
        tl::LayerKind::Sound
    );
    assert_eq!(editor.symbol().timeline.frame_count, 12 + 72);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .any(|k| k.frame == 12 && k.sound.is_some())
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);

    // 1b. Drop `.mp3` and `.aiff` audio together -> 2 dedicated Sound layers, neither audio file dropped!
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![],
        vec![mp3_path.clone(), aiff_path.clone()],
        vec![],
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import audio");
    let sound_layers: Vec<&Layer> = editor
        .symbol()
        .timeline
        .layers
        .iter()
        .filter(|l| layer_kind(&editor.symbol().timeline, l) == tl::LayerKind::Sound)
        .collect();
    assert_eq!(
        sound_layers.len(),
        2,
        "both .mp3 and .aiff must receive dedicated Sound layers"
    );
    assert_eq!(sound_layers[0].name, "voice_line");
    assert_eq!(sound_layers[1].name, "synth_pad");
    assert!(editor.symbol().timeline.frame_count >= 12 + 24);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);

    // 1c. Drop `.avi` and `.mp4` individually at frame 12 -> places Video + TimelineVideoSpan at frame 12!
    for (v_path, min_frames) in [(&avi_path, 12 + 48), (&mp4_path, 12 + 96)] {
        let _ = run_timeline_pass(
            &ctx,
            &mut editor,
            Some(cell_frame_12),
            vec![],
            vec![v_path.clone()],
            vec![],
        );
        assert_eq!(editor.undo_menu_label(), "Undo Import video");
        assert_eq!(editor.frame, 12);
        assert!(editor.symbol().timeline.frame_count >= min_frames);
        let kf12 = editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .find(|k| k.frame == 12)
            .expect("video keyframe at frame 12");
        assert!(kf12.video.is_some());
        assert!(
            kf12.elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 1d. Drop `.png` and `.svg` individually at frame 12 -> places Graphic instance at frame 12!
    for g_path in [&png_path, &svg_path] {
        let _ = run_timeline_pass(
            &ctx,
            &mut editor,
            Some(cell_frame_12),
            vec![],
            vec![g_path.clone()],
            vec![],
        );
        assert_eq!(editor.frame, 12);
        let kf12 = editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .find(|k| k.frame == 12)
            .expect("graphic keyframe at frame 12");
        assert!(
            kf12.elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 1e. Mixed drop (`drum_loop.wav` + `intro_video.avi` + `character.png` + `badge.svg`) onto
    //     both `(Layer 1, frame = 12)` AND empty space below Timeline layers -> dedicated tracks, 1 Undo!
    for drop_pos in [cell_frame_12, below_rows_frame_12] {
        let _ = run_timeline_pass(
            &ctx,
            &mut editor,
            Some(drop_pos),
            vec![],
            vec![
                wav_path.clone(),
                avi_path.clone(),
                png_path.clone(),
                svg_path.clone(),
            ],
            vec![],
        );
        assert_eq!(editor.undo_menu_label(), "Undo Import files");
        assert_eq!(editor.frame, 12);
        assert_eq!(editor.symbol().timeline.frame_count, 84);
        let tl = &editor.symbol().timeline;
        let drum_layer = tl
            .layers
            .iter()
            .find(|l| l.name == "drum_loop")
            .expect("dedicated drum_loop Sound layer");
        assert_eq!(layer_kind(tl, drum_layer), tl::LayerKind::Sound);
        assert!(
            drum_layer
                .keyframes
                .iter()
                .any(|k| k.frame == 12 && k.sound.is_some())
        );

        let vid_layer = tl
            .layers
            .iter()
            .find(|l| l.name == "intro_video")
            .expect("dedicated intro_video layer");
        assert!(vid_layer.keyframes.iter().any(|k| {
            k.frame == 12
                && k.video.is_some()
                && k.elements
                    .iter()
                    .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        }));

        for sym_stem in ["character", "badge"] {
            let g_layer = tl
                .layers
                .iter()
                .find(|l| l.name == sym_stem)
                .unwrap_or_else(|| panic!("dedicated {sym_stem} layer"));
            assert!(g_layer.keyframes.iter().any(|k| {
                k.frame == 12
                    && k.elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
            }));
        }
        assert_eq!(
            editor.layer,
            tl.layers.len() - 1,
            "top newly created layer must be selected"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 1f. Atomicity on corrupt file in mixed batch: zero partial mutation!
    let broken_mp4 = tmp.path().join("corrupt.mp4");
    std::fs::write(&broken_mp4, b"NOT_VALID_MP4").unwrap();
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![],
        vec![wav_path, broken_mp4, png_path],
        vec![],
    );
    assert_eq!(editor.project, initial);
    assert!(!editor.history.can_undo());
}

#[test]
fn t474_timeline_drop_numbered_image_sequence_at_frame_5() {
    let tmp = TempTestDir::new("seq-tl");
    let f1 = tmp.path().join("frame_0001.png");
    let f2 = tmp.path().join("frame_0002.png");
    let f3 = tmp.path().join("frame_0003.png");
    std::fs::write(&f1, make_png_bytes(24, 24, [255, 0, 0, 255])).unwrap();
    std::fs::write(&f2, make_png_bytes(24, 24, [0, 255, 0, 255])).unwrap();
    std::fs::write(&f3, make_png_bytes(24, 24, [0, 0, 255, 255])).unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    editor.workspace.sequence_import_options.placement =
        neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer;
    let initial = editor.project.clone();

    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();
    let frame_5_pos = Pos2::new(
        zone.grid_origin_x + 5.5 * zone.cell_width,
        zone.list_origin_y + 3.5 * zone.row_height, // empty space below Layer 1
    );

    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(frame_5_pos),
        vec![],
        vec![f1, f2, f3],
        vec![],
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import Image Sequence");
    assert_eq!(editor.frame, 5);
    assert_eq!(editor.symbol().timeline.frame_count, 8);
    let seq_layer = editor.symbol().timeline.layers.last().unwrap();
    let placed_frames: Vec<u32> = seq_layer
        .keyframes
        .iter()
        .filter(|k| !k.elements.is_empty())
        .map(|k| k.frame)
        .collect();
    assert_eq!(placed_frames, vec![5, 6, 7]);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);
}

#[test]
fn t474_stage_drop_on_locked_or_folder_layer_and_motion_and_button_invariants() {
    let tmp = TempTestDir::new("stage-locked");
    let wav_path = tmp.path().join("sfx.wav");
    let avi_path = tmp.path().join("cutscene.avi");
    let png_path = tmp.path().join("hero.png");
    std::fs::write(&wav_path, make_wav_bytes(16_000)).unwrap(); // 2.0s -> 48 frames
    std::fs::write(&avi_path, make_avi_bytes(48, 36, 12, 12)).unwrap(); // 1.0s -> 24 frames
    std::fs::write(&png_path, make_png_bytes(32, 32, [40, 200, 120, 255])).unwrap();

    let ctx = egui::Context::default();
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let origin = Pos2::new(60.0, 60.0);
    let mapping = crate::stage_mapping::StageMapping::identity();
    let drop_pt = Pos2::new(300.0, 240.0);

    // 1. Locked active layer + Folder active layer on Stage: individual AND mixed drops auto-create Normal layers!
    for is_folder in [false, true] {
        let mut project = Project::blank();
        project.fps = 24;
        project.format_version = 6;
        project.symbols[0].timeline.frame_count = 1;
        if is_folder {
            project.symbols[0].timeline.layers[0].properties.role = LayerRole::Folder;
            project.symbols[0].timeline.layers[0].keyframes.clear();
        } else {
            project.symbols[0].timeline.layers[0].locked = true;
        }
        project.validate().unwrap();
        let mut editor = Editor::from_project(&ctx, project);
        editor.history.clear();
        let initial = editor.project.clone();

        // Single video drop on locked/Folder layer must NOT fail with "Select an unlocked, visible layer..."
        let raw_vid = egui::RawInput {
            screen_rect: Some(area),
            events: vec![egui::Event::PointerMoved(drop_pt)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(avi_path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_vid, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.stage_file_drop(ui, area, origin, mapping);
            });
        });
        assert_eq!(editor.undo_menu_label(), "Undo Import video");
        assert_eq!(editor.symbol().timeline.layers.len(), 2);
        assert!(editor.symbol().timeline.layers[1].visible);
        assert!(!editor.symbol().timeline.layers[1].locked);
        assert_eq!(
            editor.symbol().timeline.layers[1].properties.role,
            LayerRole::Normal
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);

        // Mixed audio + video + image drop on Stage in 1 Undo step!
        let raw_mixed = egui::RawInput {
            screen_rect: Some(area),
            events: vec![egui::Event::PointerMoved(drop_pt)],
            dropped_files: vec![
                egui::DroppedFile {
                    path: Some(wav_path.clone()),
                    ..Default::default()
                },
                egui::DroppedFile {
                    path: Some(avi_path.clone()),
                    ..Default::default()
                },
                egui::DroppedFile {
                    path: Some(png_path.clone()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(raw_mixed, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.stage_file_drop(ui, area, origin, mapping);
            });
        });
        assert_eq!(editor.undo_menu_label(), "Undo Import files");
        assert_eq!(editor.symbol().timeline.layers.len(), 4);
        assert_eq!(editor.symbol().timeline.frame_count, 48);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 2. Dropping video into a Motion Tween span with ◆ property keys normalizes cleanly.
    let mut motion_proj = Project::blank();
    motion_proj.format_version = 10;
    motion_proj.symbols[0].timeline.frame_count = 24;
    let rect_el = Element {
        id: "mover".into(),
        name: "mover".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 40.0,
            height: 40.0,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let mut moved_el = rect_el.clone();
    moved_el.transform.x = 120.0;
    motion_proj.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![rect_el],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Motion,
            ..Keyframe::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![moved_el.clone()],
            keyed_properties: Some(vec![TweenProperty::X]),
            ..Keyframe::default()
        },
        Keyframe {
            frame: 18,
            elements: vec![moved_el],
            keyed_properties: Some(vec![TweenProperty::X]),
            ..Keyframe::default()
        },
    ];
    motion_proj.validate().unwrap();
    let mut motion_ed = Editor::from_project(&ctx, motion_proj);
    motion_ed.history.clear();
    let motion_initial = motion_ed.project.clone();
    motion_ed.frame = 10;
    motion_ed
        .import_video_path_at(&ctx, &avi_path, [200.0, 150.0])
        .expect("video drop on ◆ property keyframe normalizes motion span");
    motion_ed.project.validate().unwrap();
    motion_ed.undo();
    assert_eq!(motion_ed.project, motion_initial);

    // 3. `SymbolKind::Button` 4-frame invariant: dropping long audio/video never expands past 4 frames.
    let mut btn_proj = Project::blank();
    btn_proj.format_version = 6;
    let btn_id = "btn_play".to_owned();
    btn_proj.symbols.push(Symbol {
        id: btn_id.clone(),
        name: "PlayBtn".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "btn_layer".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    btn_proj.validate().unwrap();
    let mut btn_ed = Editor::from_project(&ctx, btn_proj);
    btn_ed.enter_symbol_isolated(btn_id);
    btn_ed.history.clear();
    let btn_initial = btn_ed.project.clone();
    btn_ed
        .import_media_paths(
            &[wav_path, avi_path],
            crate::graphic_import::MediaPlacementMode::Timeline(
                crate::graphic_import::TimelineDropPlacement {
                    frame: 99,
                    hovered_layer: None,
                },
            ),
        )
        .expect("drop onto Button timeline clamps to frame 3 and keeps 4-frame invariant");
    assert_eq!(btn_ed.symbol().timeline.frame_count, 4);
    assert_eq!(btn_ed.frame, 3);
    btn_ed.project.validate().unwrap();
    btn_ed.undo();
    assert_eq!(btn_ed.project, btn_initial);
}

#[test]
fn t474_stale_over_top_chrome_pointer_empty_window_and_dirty_standalone_as_preservation() {
    let tmp = TempTestDir::new("top-chrome-and-sessions");
    let wav_path = tmp.path().join("loop.wav");
    let mp4_path = tmp.path().join("promo.mp4");
    let png_path = tmp.path().join("logo.png");
    let main_as = tmp.path().join("Main.as");
    let caf_path = tmp.path().join("effect.caf");
    let opus_path = tmp.path().join("voice.opus");
    std::fs::write(&wav_path, make_wav_bytes(16_000)).unwrap();
    std::fs::write(&mp4_path, make_mp4_bytes()).unwrap();
    std::fs::write(&png_path, make_png_bytes(20, 20, [200, 100, 50, 255])).unwrap();
    std::fs::write(&main_as, "package {\n\tpublic class Main {}\n}\n").unwrap();
    std::fs::write(&caf_path, b"caff\x00\x01\x00\x00").unwrap();
    std::fs::write(&opus_path, b"OggS\x00\x02opus").unwrap();

    let ctx = egui::Context::default();

    // 1. Stale `latest_pos()` at `y = 10.0` (`over_top_chrome == true`) with `.wav`, `.mp4`, `.png`:
    //    NEVER fails with JSON parse error; imports cleanly onto active authoring document!
    let mut editor = make_one_frame_editor(&ctx);
    run_full_window_drop(
        &ctx,
        &mut editor,
        Some(Pos2::new(420.0, 10.0)),
        vec![wav_path.clone(), mp4_path.clone(), png_path.clone()],
    );
    assert!(
        !editor.status.contains("Invalid project JSON")
            && !editor.status.starts_with("Open failed"),
        "stale y=10.0 pointer must never route media into JSON loader: {}",
        editor.status
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import files");
    assert!(editor.project.resources.iter().any(|r| r.is_audio()));
    assert!(editor.project.resources.iter().any(|r| r.is_video()));
    assert!(editor.project.symbols.iter().any(|s| s.name == "logo"));
    editor.project.validate().unwrap();

    // 2. Empty window (`!editor.has_document()`): dropping media opens an untitled authoring tab and imports!
    editor.sessions.clear();
    assert!(!editor.has_document());
    run_full_window_drop(
        &ctx,
        &mut editor,
        Some(Pos2::new(400.0, 300.0)),
        vec![wav_path.clone(), png_path.clone()],
    );
    assert!(editor.has_document());
    assert!(!editor.is_standalone_script_session());
    assert_eq!(editor.session_count(), 1);
    assert!(editor.project.resources.iter().any(|r| r.is_audio()));
    assert!(editor.project.symbols.iter().any(|s| s.name == "logo"));
    editor.project.validate().unwrap();

    // 3. Dirty standalone `.as` session: dropping media opens a new untitled `.nflash` tab while
    //    preserving the dirty `.as` tab's unsaved source edits 100% intact!
    let mut as_editor = Editor::from_project(&ctx, Project::blank());
    as_editor.sessions.clear();
    assert!(as_editor.open_standalone_script_session(&ctx, main_as));
    let dirty_code = "package {\n\tpublic class Main { public var unsaved:int = 42; }\n}\n";
    as_editor.transact_named("Edit AS", |ed| {
        ed.project.sources[0].source = dirty_code.to_owned();
    });
    assert!(as_editor.dirty());
    assert_eq!(as_editor.session_count(), 1);

    run_full_window_drop(
        &ctx,
        &mut as_editor,
        Some(Pos2::new(500.0, 350.0)),
        vec![wav_path.clone(), mp4_path.clone()],
    );
    assert_eq!(as_editor.session_count(), 2);
    assert!(!as_editor.is_standalone_script_session());
    assert!(as_editor.project.resources.iter().any(|r| r.is_audio()));
    assert!(as_editor.project.resources.iter().any(|r| r.is_video()));
    as_editor.project.validate().unwrap();

    // Switch back to Tab 0 (`Main.as`) and verify unsaved edits + dirty state were preserved!
    assert!(as_editor.switch_session_tab(&ctx, 0));
    assert!(as_editor.is_standalone_script_session());
    assert!(as_editor.dirty());
    assert_eq!(as_editor.standalone_script_source(), Some(dirty_code));

    // 4. Direct `open_path_in_session` with media never hits `read_nflash_project_file` JSON parser!
    assert!(as_editor.open_path_in_session(&ctx, png_path));
    assert!(!as_editor.status.contains("Invalid project JSON"));
    assert!(!as_editor.is_standalone_script_session());
    assert!(as_editor.project.symbols.iter().any(|s| s.name == "logo"));

    // 5. `.caf` and `.opus` without FFmpeg report actionable FFmpeg diagnostic.
    for ext_path in [&caf_path, &opus_path] {
        let err =
            crate::resources::read_audio_resource_with_ffmpeg(ext_path, MAX_RESOURCE_BYTES, None)
                .expect_err("missing FFmpeg must report actionable error");
        assert!(err.contains("FFmpeg"), "unexpected error: {err}");
    }
}

#[test]
fn t474_library_and_detached_floating_timeline_and_library_drops_plus_file_url_queue() {
    let tmp = TempTestDir::new("lib-and-detached");
    let wav_path = tmp.path().join("ambience.wav");
    let avi_path = tmp.path().join("bumper.avi");
    let png_path = tmp.path().join("prop.png");
    std::fs::write(&wav_path, make_wav_bytes(8_000)).unwrap();
    std::fs::write(&avi_path, make_avi_bytes(32, 24, 12, 12)).unwrap();
    std::fs::write(&png_path, make_png_bytes(16, 16, [90, 90, 240, 255])).unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    let initial = editor.project.clone();

    // 1. Docked Library drop imports audio + video + image into Library in 1 Undo step without Stage placement.
    let lib_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 600.0));
    let raw_lib = egui::RawInput {
        screen_rect: Some(lib_rect),
        events: vec![egui::Event::PointerMoved(Pos2::new(160.0, 260.0))],
        dropped_files: vec![
            egui::DroppedFile {
                path: Some(wav_path.clone()),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(avi_path.clone()),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(png_path.clone()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let _ = ctx.run(raw_lib, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            editor.library_contents(ui);
        });
    });
    assert_eq!(editor.undo_menu_label(), "Undo Import files");
    assert!(editor.project.resources.iter().any(|r| r.is_audio()));
    assert!(editor.project.resources.iter().any(|r| r.is_video()));
    assert!(editor.project.symbols.iter().any(|s| s.name == "prop"));
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .sound
            .is_none()
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .video
            .is_none()
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);

    // 2. Detached floating Library, Timeline, and Color viewports via `public.file-url` queue!
    editor.set_panel_placement(&ctx, PanelId::Library, PanelPlacement::Detached);
    editor.set_panel_placement(&ctx, PanelId::Timeline, PanelPlacement::Detached);
    editor.set_panel_placement(&ctx, PanelId::Color, PanelPlacement::Detached);
    editor.history.clear();
    let before_detached = editor.project.clone();

    // Queue a `public.file-url` drop targeting floating `"Library — NeoFlash"`.
    if let Ok(mut q) = crate::native_windows::global_file_drag_queue().lock() {
        *q = NativeWindowFileDragQueue::default();
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Library — NeoFlash".into()),
            vec![wav_path.clone(), avi_path.clone()],
        );
    }
    run_full_window_drop(&ctx, &mut editor, None, vec![]);
    assert!(
        editor.project.resources.iter().any(|r| r.is_audio())
            && editor.project.resources.iter().any(|r| r.is_video()),
        "floating Library must accept dropped audio & video into Library"
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .sound
            .is_none()
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_detached);

    // Queue a `public.file-url` drop targeting floating `"Timeline — NeoFlash"`.
    if let Ok(mut q) = crate::native_windows::global_file_drag_queue().lock() {
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Timeline — NeoFlash".into()),
            vec![wav_path.clone(), png_path.clone()],
        );
    }
    run_full_window_drop(&ctx, &mut editor, None, vec![]);
    assert!(
        editor
            .symbol()
            .timeline
            .layers
            .iter()
            .any(|l| l.name == "ambience"
                && layer_kind(&editor.symbol().timeline, l) == tl::LayerKind::Sound),
        "floating Timeline must accept dropped media onto Timeline tracks"
    );
    assert!(
        editor
            .symbol()
            .timeline
            .layers
            .iter()
            .any(|l| l.name == "prop")
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_detached);

    // Queue a `public.file-url` drop targeting floating `"Color — NeoFlash"` (non-Timeline, non-Library panel):
    // must reject cleanly with the updated guidance message and zero project mutation!
    if let Ok(mut q) = crate::native_windows::global_file_drag_queue().lock() {
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Color — NeoFlash".into()),
            vec![png_path.clone()],
        );
    }
    run_full_window_drop(&ctx, &mut editor, None, vec![]);
    assert_eq!(
        editor.status,
        "Drop media onto the Stage, Timeline, or Library."
    );
    assert_eq!(editor.project, before_detached);

    // Verify `NativeWindowFileDragQueue` zero double-delivery when `RawInput` already has `NSFilenamesPboardType`,
    // and URL count bounding at `MAX_NATIVE_DRAG_FILE_URLS`.
    let mut queue = NativeWindowFileDragQueue::default();
    let many: Vec<PathBuf> = (0..200)
        .map(|i| PathBuf::from(format!("/tmp/f_{i}.png")))
        .collect();
    queue.set_hovered(NativeDragWindowTarget::Root, many.clone());
    let check_ctx = egui::Context::default();
    let _ = check_ctx.run(egui::RawInput::default(), |c| {
        queue.drain_into_context(c, None);
        assert_eq!(
            c.input(|i| i.raw.hovered_files.len()),
            MAX_NATIVE_DRAG_FILE_URLS
        );
    });
    queue.clear_hovered();
    let _ = check_ctx.run(egui::RawInput::default(), |c| {
        queue.drain_into_context(c, None);
        assert!(c.input(|i| i.raw.hovered_files.is_empty()));
    });
    queue.push_dropped(
        NativeDragWindowTarget::Root,
        vec![PathBuf::from("/tmp/fallback.png")],
    );
    let already_has_winit = egui::RawInput {
        dropped_files: vec![egui::DroppedFile {
            path: Some(PathBuf::from("/tmp/from_winit.png")),
            ..Default::default()
        }],
        ..Default::default()
    };
    let _ = check_ctx.run(already_has_winit, |c| {
        queue.drain_into_context(c, None);
        let dropped = c.input(|i| i.raw.dropped_files.clone());
        assert_eq!(
            dropped.len(),
            1,
            "must never duplicate when winit already populated NSFilenamesPboardType"
        );
        assert_eq!(
            dropped[0].path.as_deref(),
            Some(Path::new("/tmp/from_winit.png"))
        );
    });
}

#[test]
fn t474_internal_library_drag_of_audio_video_image_and_symbol_onto_timeline() {
    let tmp = TempTestDir::new("lib-drag-tl");
    let wav_path = tmp.path().join("stinger.wav");
    let avi_path = tmp.path().join("overlay.avi");
    let png_path = tmp.path().join("star.png");
    std::fs::write(&wav_path, make_wav_bytes(16_000)).unwrap(); // 2.0s -> 48 frames
    std::fs::write(&avi_path, make_avi_bytes(40, 30, 12, 18)).unwrap(); // 1.5s -> 36 frames
    std::fs::write(&png_path, make_png_bytes(20, 20, [250, 220, 40, 255])).unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    editor
        .import_graphics_to_library_paths(&[wav_path, avi_path])
        .unwrap();
    let image_unit =
        crate::resources::read_resource(&png_path, MAX_RESOURCE_BYTES).expect("read png resource");
    let image_res_id = image_unit.id.clone();
    editor.add_resource_units(vec![image_unit]).unwrap();
    let clip_id = new_id("symbol");
    editor.transact(|ed| {
        ed.project.symbols.push(Symbol {
            id: clip_id.clone(),
            name: "StarClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 10,
                layers: vec![Layer {
                    id: new_id("layer"),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: new_id("el"),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Ellipse {
                                width: 24.0,
                                height: 24.0,
                                fill: [255, 200, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
        });
    });
    editor.history.clear();
    let baseline = editor.project.clone();

    let audio_res_id = editor
        .project
        .resources
        .iter()
        .find(|r| r.is_audio())
        .unwrap()
        .id
        .clone();
    let video_res_id = editor
        .project
        .resources
        .iter()
        .find(|r| r.is_video())
        .unwrap()
        .id
        .clone();

    // Prime Timeline geometry.
    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();

    // 1. Internal `LibraryDrag::Resource(audio_res_id)` dropped at frame 6 on Timeline!
    let pos_f6 = Pos2::new(
        zone.grid_origin_x + 6.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    editor.library_panel.drag = Some(LibraryDrag::Resource(audio_res_id.clone()));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f6),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f6,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.library_panel.drag.is_none());
    assert_eq!(editor.frame, 6);
    assert_eq!(editor.symbol().timeline.frame_count, 6 + 48);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .any(|k| k.frame == 6 && k.sound.as_ref().is_some_and(|s| s.resource == audio_res_id))
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 2. Internal `LibraryDrag::Resource(video_res_id)` dropped at frame 9 on Timeline!
    let pos_f9 = Pos2::new(
        zone.grid_origin_x + 9.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    editor.library_panel.drag = Some(LibraryDrag::Resource(video_res_id.clone()));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f9),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f9,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.library_panel.drag.is_none());
    assert_eq!(editor.frame, 9);
    assert_eq!(editor.symbol().timeline.frame_count, 9 + 36);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .any(|k| {
                k.frame == 9
                    && k.video.as_ref().is_some_and(|v| v.resource == video_res_id)
                    && k.elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Video { .. }))
            })
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 3. Internal `LibraryDrag::Resource(image_res_id)` dropped at frame 4 on Timeline!
    let pos_f4 = Pos2::new(
        zone.grid_origin_x + 4.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    editor.library_panel.drag = Some(LibraryDrag::Resource(image_res_id));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f4),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f4,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.library_panel.drag.is_none());
    assert_eq!(editor.frame, 4);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .any(|k| {
                k.frame == 4
                    && k.elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
            })
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 4. Internal `LibraryDrag::Symbol(clip_id)` dropped below rows at frame 7 on Timeline!
    let pos_f7_below = Pos2::new(
        zone.grid_origin_x + 7.5 * zone.cell_width,
        zone.list_origin_y + 3.5 * zone.row_height,
    );
    editor.library_panel.drag = Some(LibraryDrag::Symbol(clip_id.clone()));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f7_below),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f7_below,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.library_panel.drag.is_none());
    assert_eq!(editor.frame, 7);
    assert_eq!(editor.symbol().timeline.frame_count, 7 + 10);
    let new_layer = editor.symbol().timeline.layers.last().unwrap();
    assert_eq!(new_layer.name, "StarClip");
    assert!(new_layer.keyframes.iter().any(|k| {
        k.frame == 7
            && k.elements.iter().any(|e| {
                matches!(&e.kind, ElementKind::Instance { symbol_id, .. } if *symbol_id == clip_id)
            })
    }));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);
}
