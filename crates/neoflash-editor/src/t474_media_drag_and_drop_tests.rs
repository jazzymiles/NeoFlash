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

    // 1a. Drop 3.0s `.wav` onto Timeline even when cursor is at column 12 ->
    //     starts at Frame 1 (`frame = 0`, ZERO leading empty frames!), reuses & renames untouched
    //     sole `Layer 1` -> `"drum_loop"`, and expands 1 -> 72 frames!
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![],
        vec![wav_path.clone()],
        vec![],
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import audio");
    assert_eq!(editor.frame, 0);
    assert_eq!(editor.symbol().timeline.layers.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[0].name, "drum_loop");
    assert_eq!(
        layer_kind(
            &editor.symbol().timeline,
            &editor.symbol().timeline.layers[0]
        ),
        tl::LayerKind::Sound
    );
    assert_eq!(editor.symbol().timeline.frame_count, 72);
    assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0].frame == 0
            && editor.symbol().timeline.layers[0].keyframes[0]
                .sound
                .is_some()
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);

    // 1b. Drop `.mp3` and `.aiff` audio together -> 2 dedicated Sound layers starting at Frame 1 (`frame = 0`)!
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(cell_frame_12),
        vec![],
        vec![mp3_path.clone(), aiff_path.clone()],
        vec![],
    );
    assert_eq!(editor.undo_menu_label(), "Undo Import audio");
    assert_eq!(editor.frame, 0);
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
    for sl in &sound_layers {
        assert_eq!(sl.keyframes.len(), 1);
        assert_eq!(sl.keyframes[0].frame, 0);
        assert!(sl.keyframes[0].sound.is_some());
    }
    assert!(editor.symbol().timeline.frame_count >= 24);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, initial);

    // 1c. Drop `.avi` and `.mp4` individually onto Timeline -> starts at Frame 1 (`frame = 0`), zero empty frames!
    for (v_path, min_frames, expected_stem) in [
        (&avi_path, 48, "intro_video"),
        (&mp4_path, 96, "broll_clip"),
    ] {
        let _ = run_timeline_pass(
            &ctx,
            &mut editor,
            Some(cell_frame_12),
            vec![],
            vec![v_path.clone()],
            vec![],
        );
        assert_eq!(editor.undo_menu_label(), "Undo Import video");
        assert_eq!(editor.frame, 0);
        assert_eq!(editor.symbol().timeline.layers[0].name, expected_stem);
        assert!(editor.symbol().timeline.frame_count >= min_frames);
        assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
        let kf0 = &editor.symbol().timeline.layers[0].keyframes[0];
        assert_eq!(kf0.frame, 0);
        assert!(kf0.video.is_some());
        assert!(
            kf0.elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 1d. Drop `.png` and `.svg` individually onto Timeline -> starts at Frame 1 (`frame = 0`), zero empty frames!
    for (g_path, expected_stem) in [(&png_path, "character"), (&svg_path, "badge")] {
        let _ = run_timeline_pass(
            &ctx,
            &mut editor,
            Some(cell_frame_12),
            vec![],
            vec![g_path.clone()],
            vec![],
        );
        assert_eq!(editor.frame, 0);
        assert_eq!(editor.symbol().timeline.layers[0].name, expected_stem);
        assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
        let kf0 = &editor.symbol().timeline.layers[0].keyframes[0];
        assert_eq!(kf0.frame, 0);
        assert!(
            kf0.elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);
    }

    // 1e. Mixed drop (`drum_loop.wav` + `intro_video.avi` + `character.png` + `badge.svg`) onto
    //     both `(Layer 1, col = 12)` AND empty space below Timeline layers -> dedicated tracks at Frame 1 (`0`), 1 Undo!
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
        assert_eq!(editor.frame, 0);
        assert_eq!(editor.symbol().timeline.frame_count, 72);
        let tl = &editor.symbol().timeline;
        let drum_layer = tl
            .layers
            .iter()
            .find(|l| l.name == "drum_loop")
            .expect("dedicated drum_loop Sound layer");
        assert_eq!(layer_kind(tl, drum_layer), tl::LayerKind::Sound);
        assert_eq!(drum_layer.keyframes.len(), 1);
        assert!(drum_layer.keyframes[0].frame == 0 && drum_layer.keyframes[0].sound.is_some());

        let vid_layer = tl
            .layers
            .iter()
            .find(|l| l.name == "intro_video")
            .expect("dedicated intro_video layer");
        assert_eq!(vid_layer.keyframes.len(), 1);
        assert!(
            vid_layer.keyframes[0].frame == 0
                && vid_layer.keyframes[0].video.is_some()
                && vid_layer.keyframes[0]
                    .elements
                    .iter()
                    .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        );

        for sym_stem in ["character", "badge"] {
            let g_layer = tl
                .layers
                .iter()
                .find(|l| l.name == sym_stem)
                .unwrap_or_else(|| panic!("dedicated {sym_stem} layer"));
            assert_eq!(g_layer.keyframes.len(), 1);
            assert!(
                g_layer.keyframes[0].frame == 0
                    && g_layer.keyframes[0]
                        .elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
            );
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
    assert_eq!(editor.frame, 0);
    assert_eq!(editor.symbol().timeline.frame_count, 3);
    let seq_layer = editor.symbol().timeline.layers.last().unwrap();
    let placed_frames: Vec<u32> = seq_layer
        .keyframes
        .iter()
        .filter(|k| !k.elements.is_empty())
        .map(|k| k.frame)
        .collect();
    assert_eq!(placed_frames, vec![0, 1, 2]);
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
        .expect(
            "drop onto Button timeline starts new layers at Up (0) and keeps 4-frame invariant",
        );
    assert_eq!(btn_ed.symbol().timeline.frame_count, 4);
    assert_eq!(btn_ed.frame, 0);
    btn_ed.project.validate().unwrap();
    btn_ed.undo();
    assert_eq!(btn_ed.project, btn_initial);
}

#[test]
fn window_drop_keeps_document_order_and_recognizes_extensionless_swf() {
    let tmp = TempTestDir::new("ordered-document-drop");
    let script = tmp.path().join("Script.as");
    let movie = tmp.path().join("movie");
    std::fs::write(&script, "trace('ready');").unwrap();
    // Uncompressed SWF, one 100 × 100 frame, with ShowFrame and End tags.
    std::fs::write(
        &movie,
        b"FWS\x08\x18\x00\x00\x00\x60\x00\x1f\x40\x00\x07\xd0\x00\x00\x18\x01\x00\x40\x00\x00\x00",
    )
    .unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    editor.sessions.clear();
    run_full_window_drop(
        &ctx,
        &mut editor,
        Some(Pos2::new(400.0, 300.0)),
        vec![script.clone(), movie.clone()],
    );
    assert_eq!(editor.session_count(), 2);
    assert!(!editor.is_standalone_script_session());
    assert_eq!(
        editor.import_source,
        Some(movie.clone().canonicalize().unwrap())
    );
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.standalone_script_source(), Some("trace('ready');"));

    let mut editor = make_one_frame_editor(&ctx);
    run_full_window_drop(
        &ctx,
        &mut editor,
        Some(Pos2::new(420.0, 10.0)),
        vec![movie.clone()],
    );
    assert_eq!(editor.session_count(), 2);
    assert_eq!(editor.import_source, Some(movie.canonicalize().unwrap()));
}

#[test]
fn mixed_tab_strip_drop_places_media_before_activating_script_tab() {
    let tmp = TempTestDir::new("mixed-tab-strip-drop");
    let script = tmp.path().join("Script.as");
    let image = tmp.path().join("badge.png");
    std::fs::write(&script, "trace('ready');").unwrap();
    std::fs::write(&image, make_png_bytes(20, 20, [200, 100, 50, 255])).unwrap();

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);
    let before = editor.project.clone();
    run_full_window_drop(
        &ctx,
        &mut editor,
        Some(Pos2::new(420.0, 10.0)),
        vec![script, image],
    );
    assert!(editor.is_standalone_script_session());
    assert_eq!(editor.session_count(), 2);
    assert!(editor.switch_session_tab(&ctx, 0));
    assert!(editor.project.symbols.iter().any(|s| s.name == "badge"));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
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
    crate::native_windows::with_test_file_drag_queue(|q| {
        *q = NativeWindowFileDragQueue::default();
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Library — NeoFlash".into()),
            vec![wav_path.clone(), avi_path.clone()],
        );
    });
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
    crate::native_windows::with_test_file_drag_queue(|q| {
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Timeline — NeoFlash".into()),
            vec![wav_path.clone(), png_path.clone()],
        );
    });
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
    crate::native_windows::with_test_file_drag_queue(|q| {
        q.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Color — NeoFlash".into()),
            vec![png_path.clone()],
        );
    });
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

    // 4. Internal `LibraryDrag::Symbol(clip_id)` dropped below rows (hovered_layer == None):
    //    creates a dedicated `"StarClip"` layer starting at Frame 1 (`frame = 0`), ZERO empty frames!
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
    assert_eq!(editor.frame, 0);
    assert_eq!(editor.symbol().timeline.frame_count, 10);
    let new_layer = editor.symbol().timeline.layers.last().unwrap();
    assert_eq!(new_layer.name, "StarClip");
    assert_eq!(new_layer.keyframes.len(), 1);
    assert!(
        new_layer.keyframes[0].frame == 0
            && new_layer.keyframes[0].elements.iter().any(|e| {
                matches!(&e.kind, ElementKind::Instance { symbol_id, .. } if *symbol_id == clip_id)
            })
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);
}

#[test]
fn t474_a01_media_drop_on_filtered_and_repeater_exposure_frame_preserves_authored_pose() {
    let tmp = TempTestDir::new("a01-authored-pose");
    let png_path = tmp.path().join("badge.png");
    let avi_path = tmp.path().join("cut.avi");
    std::fs::write(&png_path, make_png_bytes(16, 16, [80, 160, 240, 255])).unwrap();
    std::fs::write(&avi_path, make_avi_bytes(32, 24, 12, 12)).unwrap();

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.format_version = 12;
    project.fps = 24;
    project.symbols[0].timeline.frame_count = 24;
    project.symbols[0].timeline.layers[0].properties.filters = vec![FilterSpec::Blur {
        blur_x: 6.0,
        blur_y: 6.0,
        quality: 1,
    }];
    let base_el_id: Id = "authored-rect".into();
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![Element {
        id: base_el_id.clone(),
        name: "BaseRect".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 30.0,
            height: 20.0,
            fill: [220, 80, 60, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    }];
    project.symbols[0].timeline.layers[0].keyframes[0].appearances = vec![ElementAppearance {
        element_id: base_el_id.clone(),
        modifiers: vec![VectorModifier::Repeater {
            copies: 4,
            offset_x: 15.0,
            offset_y: 0.0,
            scale_step: 1.0,
            rotation_step: 0.0,
        }],
        ..ElementAppearance::default()
    }];
    let clip_id = new_id("symbol");
    project.symbols.push(Symbol {
        id: clip_id.clone(),
        name: "DotClip".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
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
                            width: 12.0,
                            height: 12.0,
                            fill: [255, 255, 255, 255],
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
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    editor.history.clear();
    let baseline = editor.project.clone();

    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();

    // 1. Stage drop of `.png` onto exposure frame 6 of filtered + Repeater Layer 1:
    //    must NOT bake layer filters onto `base_el_id` and must NOT flatten/serialize `#rep` copies!
    editor.layer = 0;
    editor.frame = 6;
    editor
        .import_graphics_paths(std::slice::from_ref(&png_path), Some([140.0, 100.0]))
        .unwrap();
    let kf6 = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 6)
        .expect("keyframe at frame 6");
    assert!(
        !kf6.elements.iter().any(|e| e.id.contains("#rep")),
        "derived Repeater copies must never be baked into Keyframe.elements"
    );
    let preserved_rect = kf6
        .elements
        .iter()
        .find(|e| e.id == base_el_id)
        .expect("authored rectangle preserved at frame 6");
    assert!(matches!(preserved_rect.kind, ElementKind::Rectangle { .. }));
    assert!(
        preserved_rect.filters.is_empty(),
        "derived layer filters must never be baked onto element.filters"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);
    editor.frame = 0;

    // 1b. Finder drop of `.png` onto Timeline over occupied Layer 1 at column 6:
    //     creates dedicated `"badge"` layer at Frame 1 (`frame = 0`) without altering Layer 1!
    let pos_f6 = Pos2::new(
        zone.grid_origin_x + 6.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f6),
        vec![],
        vec![png_path],
        vec![],
    );
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    assert_eq!(editor.symbol().timeline.layers[1].name, "badge");
    assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes[0].frame, 0);
    assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 2. Drop `LibraryDrag::Symbol` onto exposure frame 10 of Layer 1.
    let pos_f10 = Pos2::new(
        zone.grid_origin_x + 10.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    editor.library_panel.drag = Some(LibraryDrag::Symbol(clip_id));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f10),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f10,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let kf10 = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 10)
        .expect("keyframe at frame 10");
    assert!(!kf10.elements.iter().any(|e| e.id.contains("#rep")));
    let preserved_rect_10 = kf10.elements.iter().find(|e| e.id == base_el_id).unwrap();
    assert!(matches!(
        preserved_rect_10.kind,
        ElementKind::Rectangle { .. }
    ));
    assert!(preserved_rect_10.filters.is_empty());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 3. Video import onto exposure frame 14 of Layer 1 (`import_video_path_at`).
    editor.layer = 0;
    editor.frame = 14;
    editor
        .import_video_path_at(&ctx, &avi_path, [160.0, 120.0])
        .unwrap();
    let kf14 = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 14)
        .expect("keyframe at frame 14");
    assert!(!kf14.elements.iter().any(|e| e.id.contains("#rep")));
    let preserved_rect_14 = kf14.elements.iter().find(|e| e.id == base_el_id).unwrap();
    assert!(matches!(
        preserved_rect_14.kind,
        ElementKind::Rectangle { .. }
    ));
    assert!(preserved_rect_14.filters.is_empty());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);
}

#[test]
fn t474_a02_second_video_drop_on_exposure_frame_of_video_layer_creates_dedicated_layer() {
    let tmp = TempTestDir::new("a02-second-video");
    let intro_path = tmp.path().join("intro.avi");
    let broll_path = tmp.path().join("broll.avi");
    std::fs::write(&intro_path, make_avi_bytes(64, 48, 12, 24)).unwrap(); // 2.0s -> 48 frames
    std::fs::write(&broll_path, make_avi_bytes(40, 30, 12, 12)).unwrap(); // 1.0s -> 24 frames

    let ctx = egui::Context::default();
    let mut editor = make_one_frame_editor(&ctx);

    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();

    // Place first video `intro.avi` on `(Layer 1, frame = 0)`.
    let pos_f0 = Pos2::new(
        zone.grid_origin_x + 0.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f0),
        vec![],
        vec![intro_path],
        vec![],
    );
    assert_eq!(editor.symbol().timeline.layers.len(), 1);
    assert_eq!(editor.symbol().timeline.frame_count, 48);
    editor.history.clear();
    let one_video_baseline = editor.project.clone();

    // 1. Timeline file drop of `broll.avi` onto `(Layer 1, frame = 20)` (an exposure frame of `intro.avi`):
    //    must create a dedicated `broll` layer starting at Frame 1 (`frame = 0`, zero leading empty frames)
    //    instead of cloning `intro.avi`'s `ElementKind::Video`!
    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();
    let pos_f20 = Pos2::new(
        zone.grid_origin_x + 20.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f20),
        vec![],
        vec![broll_path.clone()],
        vec![],
    );
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    assert_eq!(editor.symbol().timeline.layers[1].name, "broll");
    assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
    let broll_kf = &editor.symbol().timeline.layers[1].keyframes[0];
    assert_eq!(broll_kf.frame, 0);
    assert_eq!(
        broll_kf
            .elements
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::Video { .. }))
            .count(),
        1,
        "second video keyframe must never carry two ElementKind::Video elements"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, one_video_baseline);

    // 2. Stage `import_video_path_at` at exposure frame 20 on `Layer 1` -> creates dedicated layer at Frame 1 (`0`)!
    editor.layer = 0;
    editor.frame = 20;
    editor
        .import_video_path_at(&ctx, &broll_path, [180.0, 140.0])
        .unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    assert_eq!(editor.symbol().timeline.layers[1].name, "broll");
    assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes[0].frame, 0);
    assert_eq!(editor.frame, 0);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, one_video_baseline);

    // 3. Internal `LibraryDrag::Resource(video_id)` onto `(Layer 1, frame = 20)` -> dedicated layer at Frame 1 (`0`)!
    editor
        .import_graphics_to_library_paths(std::slice::from_ref(&broll_path))
        .unwrap();
    let broll_res_id = editor
        .project
        .resources
        .iter()
        .find(|r| r.path.contains("broll"))
        .unwrap()
        .id
        .clone();
    editor.history.clear();
    let with_lib_broll = editor.project.clone();
    editor.library_panel.drag = Some(LibraryDrag::Resource(broll_res_id));
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(pos_f20),
        vec![],
        vec![],
        vec![egui::Event::PointerButton {
            pos: pos_f20,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    assert_eq!(editor.symbol().timeline.layers[1].name, "broll");
    assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes[0].frame, 0);
    assert_eq!(editor.frame, 0);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, with_lib_broll);
}

#[test]
fn t474_a03_media_and_library_drops_clear_stale_timeline_frame_selection() {
    let tmp = TempTestDir::new("a03-range-clear");
    let wav_path = tmp.path().join("hit.wav");
    let png_path = tmp.path().join("icon.png");
    std::fs::write(&wav_path, make_wav_bytes(8_000)).unwrap(); // 1.0s -> 24 frames
    std::fs::write(&png_path, make_png_bytes(16, 16, [255, 120, 40, 255])).unwrap();

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.fps = 24;
    project.symbols[0].timeline.frame_count = 12;
    let mut editor = Editor::from_project(&ctx, project);
    editor.history.clear();

    let layer0_id = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![layer0_id.clone()],
        start: 0,
        end: 3,
    });
    assert!(editor.timeline_edit.range.is_some());
    assert!(editor.timeline_edit.focused);

    // 1. Library-only drop onto Library panel preserves active Timeline frame selection.
    let lib_area = Rect::from_min_size(Pos2::ZERO, Vec2::new(360.0, 520.0));
    let raw_lib = egui::RawInput {
        screen_rect: Some(lib_area),
        events: vec![egui::Event::PointerMoved(Pos2::new(120.0, 180.0))],
        dropped_files: vec![egui::DroppedFile {
            path: Some(png_path.clone()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_lib, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            editor.library_contents(ui);
        });
    });
    assert!(
        editor.timeline_edit.range.is_some(),
        "LibraryOnly import must not clear Timeline frame selection"
    );

    // 2. Dropping `.png` onto Timeline creating a track at Frame 1 (`0`) MUST clear `timeline_edit.range`
    //    so subsequent `convert_to_keyframes` (`F6`) targets the active track rather than `Layer 1 @ 0..=3`!
    //    First give `Layer 1` an element so `icon.png` creates `Layer 2` above it.
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "bg".into(),
            name: "bg".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [40, 40, 40, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();
    let below_f8 = Pos2::new(
        zone.grid_origin_x + 8.5 * zone.cell_width,
        zone.list_origin_y + 3.5 * zone.row_height,
    );
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(below_f8),
        vec![],
        vec![png_path],
        vec![],
    );
    assert_eq!(editor.layer, 1);
    assert_eq!(editor.frame, 0);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[1].keyframes[0].frame, 0);
    assert!(
        editor.timeline_edit.range.is_none() && !editor.timeline_edit.focused,
        "Timeline graphic drop must clear stale timeline_edit.range"
    );
    let layer0_kfs_before = editor.symbol().timeline.layers[0].keyframes.clone();
    editor.frame = 9;
    editor.convert_to_keyframes(false);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes,
        layer0_kfs_before,
        "F6 after Timeline drop must target the newly active layer, not stale Layer 1 range"
    );
    assert!(
        editor.symbol().timeline.layers[1]
            .keyframes
            .iter()
            .any(|k| k.frame == 9)
    );

    // 3. Re-select stale `Layer 1 @ 0..=3`, then drop `.wav` audio onto Timeline at col 10:
    //    new Sound track starts at Frame 1 (`frame = 0`) and replaces stale `Layer 1` range!
    editor.select_frame_range(FrameRange {
        layers: vec![layer0_id],
        start: 0,
        end: 3,
    });
    let _ = run_timeline_pass(&ctx, &mut editor, None, vec![], vec![], vec![]);
    let zone = editor
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();
    let below_f10 = Pos2::new(
        zone.grid_origin_x + 10.5 * zone.cell_width,
        zone.list_origin_y + 4.5 * zone.row_height,
    );
    let _ = run_timeline_pass(
        &ctx,
        &mut editor,
        Some(below_f10),
        vec![],
        vec![wav_path],
        vec![],
    );
    assert_eq!(editor.layer, 2);
    assert_eq!(editor.frame, 0);
    assert_eq!(editor.symbol().timeline.layers[2].keyframes.len(), 1);
    assert_eq!(editor.symbol().timeline.layers[2].keyframes[0].frame, 0);
    let new_sound_layer_id = editor.symbol().timeline.layers[2].id.clone();
    assert_eq!(
        editor.timeline_edit.range,
        Some(FrameRange {
            layers: vec![new_sound_layer_id],
            start: 0,
            end: 0,
        }),
        "Audio drop must focus Frame 1 (`0`) on the new Sound track and clear stale Layer 1 @ 0..=3"
    );
    editor.project.validate().unwrap();
}

#[test]
fn t474_a04_failed_media_drop_on_welcome_or_dirty_standalone_as_leaves_no_spurious_tab() {
    let tmp = TempTestDir::new("a04-rollback-temp-session");
    let corrupt_mp4 = tmp.path().join("corrupt.mp4");
    let unsupported_mkv = tmp.path().join("clip.mkv");
    let broken_png = tmp.path().join("broken.png");
    let empty_dir = tmp.path().join("not_an_xfl_folder");
    let as_path = tmp.path().join("Main.as");

    std::fs::write(&corrupt_mp4, b"not a valid mp4 container").unwrap();
    std::fs::write(&unsupported_mkv, b"\x1A\x45\xDF\xA3mkv").unwrap();
    std::fs::write(&broken_png, b"not a valid png").unwrap();
    std::fs::create_dir_all(&empty_dir).unwrap();
    std::fs::write(&as_path, "package {\n    public class Main {}\n}\n").unwrap();

    let ctx = egui::Context::default();

    // 1. Empty editor window (`!has_document()`, 0 sessions): failed drops must leave 0 sessions!
    let mut empty_ed = Editor::from_project(&ctx, Project::blank());
    empty_ed.close_session_tab(&ctx, 0);
    assert!(!empty_ed.has_document());
    assert_eq!(empty_ed.session_count(), 0);

    for bad_path in [&corrupt_mp4, &unsupported_mkv, &broken_png, &empty_dir] {
        empty_ed.status.clear();
        run_full_window_drop(
            &ctx,
            &mut empty_ed,
            Some(Pos2::new(400.0, 300.0)),
            vec![bad_path.clone()],
        );
        assert!(
            !empty_ed.has_document() && empty_ed.session_count() == 0,
            "failed drop of {:?} on Welcome screen must not leave a blank Untitled session",
            bad_path.file_name()
        );
        assert!(
            !empty_ed.status.is_empty() && empty_ed.status != "Document closed.",
            "failure status must be preserved after rolling back temporary session (got {:?})",
            empty_ed.status
        );
    }

    // 2. Dirty standalone `.as` session: failed drops via window drop OR `open_path_in_session`
    //    must keep `Main.as` active, dirty, and sole session (`session_count() == 1`)!
    let mut as_ed = Editor::from_project(&ctx, Project::blank());
    as_ed.close_session_tab(&ctx, 0);
    assert!(as_ed.open_path_in_session(&ctx, as_path));
    let dirty_code = "package {\n    public class Main { public var unsaved:int = 99; }\n}\n";
    as_ed.transact_named("Edit AS", |ed| {
        ed.project.sources[0].source = dirty_code.to_owned();
    });
    assert!(as_ed.is_standalone_script_session());
    assert!(as_ed.dirty());
    assert_eq!(as_ed.session_count(), 1);

    for bad_path in [&corrupt_mp4, &unsupported_mkv, &broken_png, &empty_dir] {
        as_ed.status.clear();
        run_full_window_drop(
            &ctx,
            &mut as_ed,
            Some(Pos2::new(400.0, 300.0)),
            vec![bad_path.clone()],
        );
        assert_eq!(
            as_ed.session_count(),
            1,
            "failed drop of {:?} must not leave a blank Untitled tab next to Main.as",
            bad_path.file_name()
        );
        assert_eq!(as_ed.active_session, 0);
        assert!(as_ed.is_standalone_script_session());
        assert!(as_ed.dirty());
        assert_eq!(as_ed.project.sources[0].source, dirty_code);
        assert!(!as_ed.status.is_empty() && as_ed.status != "Document closed.");

        assert!(!as_ed.open_path_in_session(&ctx, bad_path.clone()));
        assert_eq!(as_ed.session_count(), 1);
        assert!(as_ed.is_standalone_script_session());
        assert!(as_ed.dirty());
        assert_eq!(as_ed.project.sources[0].source, dirty_code);
    }
}

#[test]
fn t476_frame_script_never_migrates_to_new_layer_and_new_layers_start_at_frame_one() {
    let tmp = TempTestDir::new("t476-script-and-frame1");
    let wav_path = tmp.path().join("theme_music.wav");
    let avi_path = tmp.path().join("intro_reel.avi");
    let png_path = tmp.path().join("hero_sprite.png");
    let seq1 = tmp.path().join("walk_0001.png");
    let seq2 = tmp.path().join("walk_0002.png");
    std::fs::write(&wav_path, make_wav_bytes(16_000)).unwrap(); // 2.0s -> 48 frames @ 24fps
    std::fs::write(&avi_path, make_avi_bytes(48, 36, 12, 12)).unwrap(); // 1.0s -> 24 frames @ 24fps
    std::fs::write(&png_path, make_png_bytes(32, 32, [220, 90, 40, 255])).unwrap();
    std::fs::write(&seq1, make_png_bytes(16, 16, [255, 0, 0, 255])).unwrap();
    std::fs::write(&seq2, make_png_bytes(16, 16, [0, 255, 0, 255])).unwrap();

    let ctx = egui::Context::default();

    // 1. `Project::demo()` has a layerless `FrameScript { layer: None, frame: 0 }` on `Orbit instances` (layer 2).
    //    Verify that `+ Add Layer`, Timeline drops (at x = col 18!), and Stage drops (at frame 15!)
    //    NEVER migrate the ActionScript `a` marker onto the newly created layer, AND every newly
    //    created layer starts at Frame 1 (`frame = 0`) with ZERO leading empty frames!
    let mut demo_ed = Editor::from_project(&ctx, Project::demo());
    demo_ed.history.clear();
    let demo_baseline = demo_ed.project.clone();
    let orbit_id = demo_ed.symbol().timeline.layers[2].id.clone();
    assert_eq!(demo_ed.symbol().timeline.scripts[0].layer, None);
    assert!(demo_ed.symbol().timeline.key_carries_frame_script(2, 0));

    // 1a. Clicking `+ Add Layer` (`add_new_layer_at_top`) pins the script to `Orbit instances` (layer 2)
    //     and leaves `Layer 4` (layer 3) completely script-free!
    demo_ed.add_new_layer_at_top();
    assert_eq!(demo_ed.symbol().timeline.layers.len(), 4);
    assert_eq!(
        demo_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&orbit_id)
    );
    assert!(demo_ed.symbol().timeline.key_carries_frame_script(2, 0));
    assert!(
        !demo_ed.symbol().timeline.key_carries_frame_script(3, 0),
        "newly added top layer must NEVER steal frame 0 ActionScript from Orbit instances"
    );
    demo_ed.project.validate().unwrap();
    demo_ed.undo();
    assert_eq!(demo_ed.project, demo_baseline);

    // 1b. Finder drop of audio, video, single image, sequence, and mixed batch onto `Project::demo()` Timeline
    //     at horizontal column 18 (`x = grid_origin_x + 18.5 * cell_width`):
    //     - `Orbit instances` (layer 2) keeps its `FrameScript` (`key_carries_frame_script(2, 0) == true`)
    //     - Zero new layers carry a frame script
    //     - Every newly created layer starts immediately at Frame 1 (`keyframes[0].frame == 0`, non-empty)!
    let _ = run_timeline_pass(&ctx, &mut demo_ed, None, vec![], vec![], vec![]);
    let zone = demo_ed
        .timeline_drop_zones
        .get(&egui::ViewportId::ROOT)
        .unwrap()
        .clone();
    let drop_col_18_over_orbit = Pos2::new(
        zone.grid_origin_x + 18.5 * zone.cell_width,
        zone.list_origin_y + 0.5 * zone.row_height,
    );
    let drop_col_18_below_rows = Pos2::new(
        zone.grid_origin_x + 18.5 * zone.cell_width,
        zone.list_origin_y + 4.5 * zone.row_height,
    );

    demo_ed.workspace.sequence_import_options.placement =
        neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer;

    for pos in [drop_col_18_over_orbit, drop_col_18_below_rows] {
        for batch in [
            vec![wav_path.clone()],
            vec![avi_path.clone()],
            vec![png_path.clone()],
            vec![seq1.clone(), seq2.clone()],
            vec![wav_path.clone(), avi_path.clone(), png_path.clone()],
        ] {
            let _ = run_timeline_pass(&ctx, &mut demo_ed, Some(pos), vec![], batch.clone(), vec![]);
            assert_eq!(demo_ed.frame, 0);
            let tl = &demo_ed.symbol().timeline;
            assert!(
                tl.layers.len() > 3,
                "Timeline drop on occupied demo must create dedicated track(s)"
            );
            assert!(
                tl.key_carries_frame_script(2, 0),
                "Orbit instances must retain Frame 1 ActionScript after dropping {batch:?}"
            );
            for new_idx in 3..tl.layers.len() {
                assert!(
                    !tl.key_carries_frame_script(new_idx, 0),
                    "new layer {} ('{}') must NEVER steal Frame 1 ActionScript",
                    new_idx,
                    tl.layers[new_idx].name
                );
                let new_layer = &tl.layers[new_idx];
                assert!(
                    !new_layer.keyframes.is_empty() && new_layer.keyframes[0].frame == 0,
                    "new layer '{}' must start at Frame 1 (0), got {:?}",
                    new_layer.name,
                    new_layer
                        .keyframes
                        .iter()
                        .map(|k| k.frame)
                        .collect::<Vec<_>>()
                );
                let kf0 = &new_layer.keyframes[0];
                assert!(
                    !kf0.elements.is_empty() || kf0.sound.is_some() || kf0.video.is_some(),
                    "Frame 1 (0) of new layer '{}' must hold the imported media, never an empty placeholder keyframe!",
                    new_layer.name
                );
            }
            demo_ed.project.validate().unwrap();
            demo_ed.undo();
            assert_eq!(demo_ed.project, demo_baseline);
        }
    }

    // 2. Classic Flash dedicated `"Actions"` layer (`elements: []`, `FrameScript { layer: None, frame: 0 }`)
    //    at `editor.frame = 14`: dropping audio, video, or image onto STAGE or TIMELINE must:
    //    - NEVER pollute the `"Actions"` layer with audio, video, or graphics!
    //    - NEVER move `stop();` off `"Actions"` onto the newly created media layer!
    //    - Start the newly created media layer at Frame 1 (`frame = 0`), even though `editor.frame` was 14!
    let mut actions_proj = Project::blank();
    actions_proj.fps = 24;
    actions_proj.symbols[0].timeline.frame_count = 24;
    actions_proj.symbols[0].timeline.layers[0].name = "Actions".into();
    let actions_layer_id = actions_proj.symbols[0].timeline.layers[0].id.clone();
    actions_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "stop();".into(),
        },
    });
    actions_proj.validate().unwrap();

    let mut actions_ed = Editor::from_project(&ctx, actions_proj);
    actions_ed.history.clear();
    let actions_baseline = actions_ed.project.clone();
    let stage_area = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let stage_origin = Pos2::new(60.0, 60.0);
    let mapping = crate::stage_mapping::StageMapping::identity();

    for single_file in [&wav_path, &avi_path, &png_path] {
        actions_ed.layer = 0;
        actions_ed.frame = 14;
        let raw_stage = egui::RawInput {
            screen_rect: Some(stage_area),
            events: vec![egui::Event::PointerMoved(Pos2::new(320.0, 220.0))],
            dropped_files: vec![egui::DroppedFile {
                path: Some(single_file.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_stage, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                actions_ed.stage_file_drop(ui, stage_area, stage_origin, mapping);
            });
        });
        let tl = &actions_ed.symbol().timeline;
        assert_eq!(
            tl.layers.len(),
            2,
            "dropping {:?} when Actions layer is active must create a separate media layer",
            single_file.file_name()
        );
        assert!(
            tl.layers[0]
                .keyframes
                .iter()
                .all(|k| k.elements.is_empty() && k.sound.is_none() && k.video.is_none()),
            "Actions layer must stay completely free of media/elements"
        );
        assert_eq!(tl.scripts[0].layer.as_ref(), Some(&actions_layer_id));
        assert!(tl.key_carries_frame_script(0, 0));
        assert!(!tl.key_carries_frame_script(1, 0));
        assert_eq!(tl.layers[1].keyframes.len(), 1);
        assert_eq!(tl.layers[1].keyframes[0].frame, 0);
        assert!(
            !tl.layers[1].keyframes[0].elements.is_empty()
                || tl.layers[1].keyframes[0].sound.is_some()
                || tl.layers[1].keyframes[0].video.is_some()
        );
        actions_ed.project.validate().unwrap();
        actions_ed.undo();
        assert_eq!(actions_ed.project, actions_baseline);
    }

    // 3. `F6` (`ConvertToKeyframes` in `edit_frames`) on a higher layer that previously had NO keyframe
    //    at `script.frame` must ALSO pin the lower layer's `layer: None` script instead of stealing it!
    let mut f6_proj = Project::blank();
    f6_proj.symbols[0].timeline.frame_count = 10;
    let bottom_id = f6_proj.symbols[0].timeline.layers[0].id.clone();
    f6_proj.symbols[0].timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 5,
            ..Keyframe::default()
        });
    f6_proj.symbols[0].timeline.layers.push(Layer {
        id: new_id("layer"),
        name: "Top Artwork".into(),
        visible: true,
        locked: false,
        properties: Default::default(),
        keyframes: vec![Keyframe::default()],
    });
    f6_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 5,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "trace('frame 6 on bottom');".into(),
        },
    });
    f6_proj.validate().unwrap();
    assert_eq!(
        f6_proj.symbols[0].timeline.action_layer_for_frame(5),
        Some(0)
    );

    let mut f6_ed = Editor::from_project(&ctx, f6_proj);
    f6_ed.history.clear();
    let f6_baseline = f6_ed.project.clone();
    f6_ed.layer = 1;
    f6_ed.frame = 5;
    f6_ed.convert_to_keyframes(false);
    assert_eq!(
        f6_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&bottom_id)
    );
    assert!(f6_ed.symbol().timeline.key_carries_frame_script(0, 5));
    assert!(!f6_ed.symbol().timeline.key_carries_frame_script(1, 5));
    f6_ed.project.validate().unwrap();
    f6_ed.undo();
    assert_eq!(f6_ed.project, f6_baseline);
}

#[test]
fn t476_audit_edge_cases_coalesce_multi_scripts_collapsed_folders_figma_video_paste_and_batch() {
    let tmp = TempTestDir::new("t476-audit-remediations");
    let wav_path = tmp.path().join("cue.wav");
    let avi_path = tmp.path().join("clip.avi");
    let png_path = tmp.path().join("badge.png");
    let png_bytes = make_png_bytes(16, 16, [40, 180, 90, 255]);
    std::fs::write(&wav_path, make_wav_bytes(8_000)).unwrap();
    std::fs::write(&avi_path, make_avi_bytes(16, 16, 24, 6)).unwrap();
    std::fs::write(&png_path, &png_bytes).unwrap();

    let ctx = egui::Context::default();

    // 1. Finding #1: Multiple `FrameScript { layer: None, frame: 0 }` entries on the same frame
    //    (e.g., imported legacy `.fla` multi-script frame) coalesce cleanly without violating
    //    `(layer_id, frame)` uniqueness in `validation.rs`!
    let mut multi_proj = Project::blank();
    multi_proj.format_version = 12;
    let legacy_layer_id = multi_proj.symbols[0].timeline.layers[0].id.clone();
    multi_proj.symbols[0].timeline.layers[0].name = "Legacy Scripts".into();
    multi_proj.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var a:int = 1;".into(),
            },
        },
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(a);".into(),
            },
        },
    ];
    multi_proj.validate().unwrap();

    let mut multi_ed = Editor::from_project(&ctx, multi_proj);
    multi_ed.history.clear();
    let multi_baseline = multi_ed.project.clone();
    multi_ed
        .import_media_paths(
            std::slice::from_ref(&png_path),
            crate::graphic_import::MediaPlacementMode::Timeline(
                crate::graphic_import::TimelineDropPlacement {
                    frame: 12,
                    hovered_layer: Some(0),
                },
            ),
        )
        .expect("multi-script legacy frame coalesces cleanly on Timeline drop");
    multi_ed.project.validate().unwrap();
    let multi_tl = &multi_ed.symbol().timeline;
    assert_eq!(multi_tl.layers.len(), 2);
    assert_eq!(multi_tl.scripts.len(), 1);
    assert_eq!(multi_tl.scripts[0].layer.as_ref(), Some(&legacy_layer_id));
    assert_eq!(
        multi_tl.scripts[0].script.source,
        "var a:int = 1;\n\ntrace(a);"
    );
    assert!(multi_tl.key_carries_frame_script(0, 0));
    assert!(!multi_tl.key_carries_frame_script(1, 0));
    assert_eq!(multi_tl.layers[1].keyframes[0].frame, 0);
    assert!(!multi_tl.layers[1].keyframes[0].elements.is_empty());
    multi_ed.undo();
    assert_eq!(multi_ed.project, multi_baseline);

    // Mixed-language `layer: None` entries route the 2nd language to another valid layer instead of failing validation.
    let mut mixed_lang_proj = Project::blank();
    mixed_lang_proj.format_version = 12;
    let bot_id = mixed_lang_proj.symbols[0].timeline.layers[0].id.clone();
    let top_id = new_id("layer");
    mixed_lang_proj.symbols[0].timeline.layers.push(Layer {
        id: top_id.clone(),
        name: "Top Script".into(),
        visible: true,
        locked: false,
        properties: Default::default(),
        keyframes: vec![Keyframe::default()],
    });
    mixed_lang_proj.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "stop();".into(),
            },
        },
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "console.log('ok');".into(),
            },
        },
    ];
    mixed_lang_proj.validate().unwrap();
    let mut mixed_ed = Editor::from_project(&ctx, mixed_lang_proj);
    mixed_ed
        .import_media_paths(
            std::slice::from_ref(&png_path),
            crate::graphic_import::MediaPlacementMode::Timeline(
                crate::graphic_import::TimelineDropPlacement {
                    frame: 0,
                    hovered_layer: None,
                },
            ),
        )
        .expect(
            "mixed-language layer:None scripts route without duplicate (layer_id, frame) error",
        );
    mixed_ed.project.validate().unwrap();
    assert_eq!(
        mixed_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&top_id)
    );
    assert_eq!(
        mixed_ed.symbol().timeline.scripts[1].layer.as_ref(),
        Some(&bot_id)
    );

    // 2. Finding #5: `layer: None` script inside a collapsed `LayerRole::Folder` stays pinned to the
    //    child layer inside the folder instead of migrating onto a newly dropped top-level layer!
    let mut folder_proj = Project::blank();
    folder_proj.format_version = 12;
    let child_id = folder_proj.symbols[0].timeline.layers[0].id.clone();
    let folder_id = new_id("folder");
    folder_proj.symbols[0].timeline.layers[0].name = "Nested Actions".into();
    folder_proj.symbols[0].timeline.layers[0].properties.parent = Some(folder_id.clone());
    folder_proj.symbols[0].timeline.layers.insert(
        0,
        Layer {
            id: folder_id,
            name: "Group".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                role: LayerRole::Folder,
                collapsed: true,
                ..Default::default()
            },
            keyframes: Vec::new(),
        },
    );
    folder_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "stop();".into(),
        },
    });
    folder_proj.validate().unwrap();
    assert_eq!(
        folder_proj.symbols[0].timeline.action_layer_for_frame(0),
        None
    );

    let mut folder_ed = Editor::from_project(&ctx, folder_proj);
    folder_ed.history.clear();
    let folder_baseline = folder_ed.project.clone();
    folder_ed
        .import_media_paths(
            std::slice::from_ref(&wav_path),
            crate::graphic_import::MediaPlacementMode::Timeline(
                crate::graphic_import::TimelineDropPlacement {
                    frame: 8,
                    hovered_layer: None,
                },
            ),
        )
        .unwrap();
    folder_ed.project.validate().unwrap();
    let folder_tl = &folder_ed.symbol().timeline;
    assert_eq!(folder_tl.layers.len(), 3);
    assert_eq!(folder_tl.scripts[0].layer.as_ref(), Some(&child_id));
    assert!(folder_tl.key_carries_frame_script(1, 0));
    assert!(!folder_tl.key_carries_frame_script(2, 0));
    assert_eq!(folder_tl.layers[2].keyframes[0].frame, 0);
    folder_ed.undo();
    assert_eq!(folder_ed.project, folder_baseline);

    // 3. Finding #2 & #6.2: `active_timeline_is_single_blank_layer`, `commit_figma_pages_directly_as_layers`,
    //    and `sole_untouched_blank_layer` respect `"Actions"` layers and non-default layer/keyframe props!
    let mut fig_target = Project::blank();
    fig_target.symbols[0].timeline.frame_count = 20;
    fig_target.symbols[0].timeline.layers[0].name = "Actions".into();
    let fig_actions_id = fig_target.symbols[0].timeline.layers[0].id.clone();
    fig_target.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "stop();".into(),
        },
    });
    assert!(
        !crate::panels::figma_import::active_timeline_is_single_blank_layer(
            &fig_target.symbols[0].timeline,
            0
        )
    );

    let mut donor_figma = Project::blank();
    let fig_folder_id = new_id("fig_folder");
    donor_figma.symbols[0].timeline.layers[0].name = "Figma Hero".into();
    donor_figma.symbols[0].timeline.layers[0].properties.parent = Some(fig_folder_id.clone());
    donor_figma.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "hero_rect".into(),
            name: "hero".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 80.0,
                height: 40.0,
                fill: [20, 120, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    donor_figma.symbols[0].timeline.layers.insert(
        0,
        Layer {
            id: fig_folder_id,
            name: "Figma Group".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                role: LayerRole::Folder,
                ..Default::default()
            },
            keyframes: Vec::new(),
        },
    );
    let mut fig_ed = Editor::from_project(&ctx, fig_target);
    fig_ed.frame = 14;
    fig_ed.history.clear();
    let fig_baseline = fig_ed.project.clone();
    let fig_report = neoflash_import::ImportReport {
        source: "hero.fig".into(),
        format: neoflash_import::ImportFormat::Figma,
        version: None,
        can_import: true,
        statistics: neoflash_import::Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };
    fig_ed
        .commit_figma_pages_directly_as_layers(
            Path::new("hero.fig"),
            donor_figma,
            &fig_report,
            None,
        )
        .expect("Figma pages import onto Actions timeline preserves Actions and starts at Frame 1");
    fig_ed.project.validate().unwrap();
    assert_eq!(fig_ed.frame, 0);
    assert_eq!(fig_ed.symbol().timeline.layers.len(), 3);
    assert_eq!(fig_ed.symbol().timeline.layers[0].name, "Actions");
    assert_eq!(
        fig_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&fig_actions_id)
    );
    assert!(fig_ed.symbol().timeline.key_carries_frame_script(0, 0));
    assert!(!fig_ed.symbol().timeline.key_carries_frame_script(1, 0));
    assert!(!fig_ed.symbol().timeline.key_carries_frame_script(2, 0));
    assert_eq!(
        fig_ed.symbol().timeline.layers[1].properties.role,
        LayerRole::Folder
    );
    assert!(
        fig_ed.symbol().timeline.layers[1].keyframes.is_empty(),
        "LayerRole::Folder imported from Figma must keep empty keyframes"
    );
    assert_eq!(fig_ed.symbol().timeline.layers[2].keyframes.len(), 1);
    assert_eq!(fig_ed.symbol().timeline.layers[2].keyframes[0].frame, 0);
    assert_eq!(
        fig_ed.symbol().timeline.layers[2].keyframes[0]
            .elements
            .len(),
        1
    );
    fig_ed.undo();
    assert_eq!(fig_ed.project, fig_baseline);

    // Non-default `LayerProperties` (`blend_mode: Multiply`) on sole `Layer 1` is NOT overwritten by Timeline drop.
    let mut blend_proj = Project::blank();
    blend_proj.format_version = 10;
    blend_proj.symbols[0].timeline.layers[0]
        .properties
        .blend_mode = BlendMode::Multiply;
    blend_proj.validate().unwrap();
    let mut blend_ed = Editor::from_project(&ctx, blend_proj);
    blend_ed
        .import_media_paths(
            std::slice::from_ref(&png_path),
            crate::graphic_import::MediaPlacementMode::Timeline(
                crate::graphic_import::TimelineDropPlacement {
                    frame: 0,
                    hovered_layer: None,
                },
            ),
        )
        .unwrap();
    assert_eq!(blend_ed.symbol().timeline.layers.len(), 2);
    assert_eq!(
        blend_ed.symbol().timeline.layers[0].properties.blend_mode,
        BlendMode::Multiply
    );
    blend_ed.project.validate().unwrap();

    // 4. Finding #3: `import_video_draft_with_options` with `expand_timeline = false` and `end_frame = Some(2)`
    //    at `editor.frame = 5` extends `end_frame` so `validation.rs` never fails!
    let mut vid_proj = Project::blank();
    vid_proj.format_version = 6;
    vid_proj.symbols[0].timeline.frame_count = 10;
    vid_proj.symbols[0].timeline.layers[0].properties.end_frame = Some(2);
    vid_proj.validate().unwrap();
    let mut vid_ed = Editor::from_project(&ctx, vid_proj);
    vid_ed.frame = 5;
    let draft = crate::panels::import_video::ImportVideoDraft {
        file_path: avi_path.display().to_string(),
        expand_timeline: false,
        ..Default::default()
    };
    vid_ed
        .import_video_draft_with_options(&ctx, draft, Some([100.0, 100.0]), false)
        .expect("import_video with expand_timeline=false extends end_frame past placed keyframe");
    vid_ed.project.validate().unwrap();
    assert_eq!(vid_ed.symbol().timeline.frame_count, 10);
    assert!(
        vid_ed.symbol().timeline.layers[0]
            .properties
            .end_frame
            .unwrap()
            >= 6
    );

    // 5. Finding #4: `add_actions_and_stop_layer` pins existing `layer: None` artwork scripts to their
    //    original layer instead of clobbering them with `"stop();\n"`!
    let mut demo_stop = Editor::from_project(&ctx, Project::demo());
    let orbit_id = demo_stop.symbol().timeline.layers[2].id.clone();
    let original_orbit_src = demo_stop.symbol().timeline.scripts[0].script.source.clone();
    demo_stop.history.clear();
    let demo_stop_baseline = demo_stop.project.clone();
    demo_stop.add_actions_and_stop_layer();
    demo_stop.project.validate().unwrap();
    let stop_tl = &demo_stop.symbol().timeline;
    assert_eq!(stop_tl.layers.len(), 4);
    assert_eq!(stop_tl.layers[3].name, "actions");
    let actions_new_id = stop_tl.layers[3].id.clone();
    assert_eq!(stop_tl.scripts.len(), 2);
    assert!(
        stop_tl
            .scripts
            .iter()
            .any(|s| s.layer.as_ref() == Some(&orbit_id) && s.script.source == original_orbit_src),
        "Orbit instances must retain its original script untouched!"
    );
    assert!(
        stop_tl
            .scripts
            .iter()
            .any(|s| s.layer.as_ref() == Some(&actions_new_id)
                && s.script.source.trim() == "stop();"),
        "New Actions layer must receive its own stop() script!"
    );
    demo_stop.undo();
    assert_eq!(demo_stop.project, demo_stop_baseline);

    // Also test existing `"actions"` layer below an artwork layer that owns `layer: None` script:
    let mut existing_act_proj = Project::blank();
    existing_act_proj.symbols[0].timeline.layers[0].name = "actions".into();
    let existing_act_id = existing_act_proj.symbols[0].timeline.layers[0].id.clone();
    let art_top_id = new_id("art");
    existing_act_proj.symbols[0].timeline.layers.push(Layer {
        id: art_top_id.clone(),
        name: "Artwork".into(),
        visible: true,
        locked: false,
        properties: Default::default(),
        keyframes: vec![Keyframe::default()],
    });
    existing_act_proj.symbols[0]
        .timeline
        .scripts
        .push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace('art');".into(),
            },
        });
    let mut existing_act_ed = Editor::from_project(&ctx, existing_act_proj);
    existing_act_ed.add_actions_and_stop_layer();
    existing_act_ed.project.validate().unwrap();
    assert_eq!(existing_act_ed.symbol().timeline.scripts.len(), 2);
    assert!(
        existing_act_ed
            .symbol()
            .timeline
            .scripts
            .iter()
            .any(|s| s.layer.as_ref() == Some(&art_top_id) && s.script.source == "trace('art');")
    );
    assert!(
        existing_act_ed
            .symbol()
            .timeline
            .scripts
            .iter()
            .any(|s| s.layer.as_ref() == Some(&existing_act_id)
                && s.script.source.trim() == "stop();")
    );

    // 6. Clipboard Paste (`paste_svg` & `paste_image_bytes`) onto an `"Actions"` layer creates `"Pasted graphics"`
    //    and pins `layer: None` script onto `"Actions"`!
    let mut paste_proj = Project::blank();
    paste_proj.symbols[0].timeline.layers[0].name = "Actions".into();
    let paste_actions_id = paste_proj.symbols[0].timeline.layers[0].id.clone();
    paste_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "stop();".into(),
        },
    });
    let mut paste_ed = Editor::from_project(&ctx, paste_proj);
    paste_ed.history.clear();
    let paste_baseline = paste_ed.project.clone();
    assert!(paste_ed.paste_svg(
        &ctx,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\"><rect width=\"24\" height=\"24\" fill=\"#00aaff\"/></svg>",
        false
    ));
    paste_ed.project.validate().unwrap();
    assert_eq!(paste_ed.symbol().timeline.layers.len(), 2);
    assert!(
        paste_ed.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert_eq!(
        paste_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&paste_actions_id)
    );
    assert!(
        !paste_ed.symbol().timeline.layers[1].keyframes[0]
            .elements
            .is_empty()
    );
    paste_ed.undo();
    assert_eq!(paste_ed.project, paste_baseline);

    assert!(paste_ed.paste_image_bytes(&ctx, &png_bytes, "image/png", false));
    paste_ed.project.validate().unwrap();
    assert_eq!(paste_ed.symbol().timeline.layers.len(), 2);
    assert!(
        paste_ed.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert_eq!(
        paste_ed.symbol().timeline.scripts[0].layer.as_ref(),
        Some(&paste_actions_id)
    );
    assert!(
        !paste_ed.symbol().timeline.layers[1].keyframes[0]
            .elements
            .is_empty()
    );
    paste_ed.undo();
    assert_eq!(paste_ed.project, paste_baseline);

    // 7. Finding #6.1: Headless `batch` `add_layer` and `add_keyframe` pin displaced `layer: None` scripts!
    let batch_proj_path = tmp.path().join("batch_pin.nflash");
    let mut batch_proj = Project::blank();
    batch_proj.symbols[0].timeline.frame_count = 6;
    let batch_bottom_id = batch_proj.symbols[0].timeline.layers[0].id.clone();
    batch_proj.symbols[0].timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 3,
            ..Keyframe::default()
        });
    batch_proj.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace('f0');".into(),
            },
        },
        FrameScript {
            layer: None,
            frame: 3,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace('f3');".into(),
            },
        },
    ];
    std::fs::write(
        &batch_proj_path,
        serde_json::to_string_pretty(&batch_proj).unwrap(),
    )
    .unwrap();
    let ops_json = serde_json::json!([
        { "op": "add_layer", "name": "Top Batch Layer" },
        { "op": "add_keyframe", "layer": "Top Batch Layer", "frame": 3 }
    ])
    .to_string();
    let args = vec![
        "--headless".to_owned(),
        "batch".to_owned(),
        "--project".to_owned(),
        batch_proj_path.display().to_string(),
        "--in-place".to_owned(),
        "--stdin".to_owned(),
    ];
    let mut stdin = std::io::Cursor::new(ops_json.into_bytes());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = crate::headless::run_cli_with_io(&args, true, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(
        code,
        0,
        "headless batch failed: stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let saved_batch: Project =
        serde_json::from_str(&std::fs::read_to_string(&batch_proj_path).unwrap()).unwrap();
    saved_batch.validate().unwrap();
    assert_eq!(saved_batch.symbols[0].timeline.layers.len(), 2);
    assert!(
        saved_batch.symbols[0]
            .timeline
            .scripts
            .iter()
            .all(|s| s.layer.as_ref() == Some(&batch_bottom_id)),
        "both frame 0 and frame 3 layer:None scripts must remain pinned to bottom layer after batch add_layer + add_keyframe!"
    );
}
