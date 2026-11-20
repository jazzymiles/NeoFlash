use std::{
    collections::HashMap,
    env, fs,
    io::{BufReader, Cursor},
    sync::{Arc, atomic::Ordering},
};

use neoflash_core::{
    AuthoredBitmapFill, DecodedImage, InstancePlayback, LayerRole, Project, VideoSyncMode,
};

use super::decoder::{
    StagedMediaFile, checked_decoded_video_duration_ms, checked_decoded_video_payload_len,
    finish_pam_rgba_frames, parse_ffmpeg_fps_from_stderr, parse_pam_rgba_frames,
    read_bounded_process_diagnostics, read_bounded_video_file_with_limit,
};
use super::encode::encode_rgba_png_frame;
use super::ffprobe_ffmpeg::parse_ffprobe_metadata;
use super::progressive::insert_decoded_video_stream_with_limits;
use super::*;

use neoflash_core::{
    Element, FrameSound, Keyframe, Layer, ResourceUnit, SoundSync, Symbol, Timeline, Transform,
};

#[test]
fn estimate_and_render_demo_project_frames() {
    let project = Project::demo();
    let options = VideoOptions {
        width: 320,
        height: 200,
        fps: 24.0,
        ..VideoOptions::default()
    };
    let estimate = estimate_video(&project, &options).expect("estimate");
    assert!(estimate.frame_count >= 1);
    assert_eq!(estimate.width, 320);
    assert_eq!(estimate.height, 200);
    assert!(estimate.estimated_bytes > 0);

    let rgba = render_project_frame_rgba(&project, None, 0, 320, 200, false, true)
        .expect("render frame 0");
    assert_eq!(rgba.len(), 320 * 200 * 4);
    assert!(rgba.as_chunks::<4>().0.iter().any(|px| px[3] == 255));
}

#[test]
fn raster_output_samples_authored_bitmap_inside_transformed_shape() {
    let mut project = Project::blank();
    project.width = 20;
    project.height = 16;
    project.format_version = 9;
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let png = encode_rgba_png_frame(&[red, blue].concat(), 2, 1).unwrap();
    project
        .resources
        .push(ResourceUnit::from_bytes("stripes", "stripes.png", "image/png", &png).unwrap());
    let shape = Element {
        id: "oval".into(),
        name: "Striped oval".into(),
        transform: Transform {
            x: 2.,
            y: 2.,
            ..Default::default()
        },
        kind: ElementKind::Ellipse {
            width: 16.,
            height: 12.,
            fill: [255; 4],
            stroke: [0, 255, 0, 255],
            stroke_width: 2.,
        },
        ..Default::default()
    };
    let frame = &mut project.symbols[0].timeline.layers[0].keyframes[0];
    frame.elements.push(shape);
    frame.set_bitmap_fill(AuthoredBitmapFill {
        element_id: "oval".into(),
        resource_id: "stripes".into(),
        matrix: [2., 0., 0., 2., 2., 0.],
        repeat: true,
        smoothing: false,
    });

    for (repeat, expected) in [(true, red), (false, blue)] {
        project.symbols[0].timeline.layers[0].keyframes[0].bitmap_fills[0].repeat = repeat;
        let rgba = render_project_frame_rgba(&project, None, 0, 20, 16, true, false).unwrap();
        let at = |x: usize, y: usize| rgba.as_chunks::<4>().0[y * 20 + x];
        assert_eq!(at(4, 8), red);
        assert_eq!(at(6, 8), blue);
        assert_eq!(at(12, 8), expected);
        assert_eq!(at(2, 8), [0, 255, 0, 255]);
        assert_eq!(at(2, 2), [0; 4]);

        // Interchange precomps keep the original contour and its independently editable stroke.
        let mut masked = project.clone();
        assert_eq!(
            crate::image_fill_layers::prepare_image_fill_layers(&mut masked).unwrap(),
            1
        );
        masked.validate().unwrap();
        let reconstructed =
            render_project_frame_rgba(&masked, None, 0, 20, 16, true, false).unwrap();
        for (x, y) in [(2, 2), (4, 8), (6, 8), (12, 8), (2, 8)] {
            assert_eq!(reconstructed.as_chunks::<4>().0[y * 20 + x], at(x, y));
        }
        let (svg, _, defs) = crate::render_frame_svg(&project, &Default::default()).unwrap();
        assert!(defs >= 2 && svg.contains("<pattern") && svg.contains("data:image/png;base64,"));
        let (json, _) = crate::export_lottie_json(&project, &Default::default()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(json["assets"].as_array().unwrap().iter().any(|asset| {
            asset["layers"].as_array().is_some_and(|layers| {
                layers
                    .iter()
                    .any(|image| image["ty"] == 2 && image["masksProperties"].is_array())
            })
        }));
    }

    let frame = &mut project.symbols[0].timeline.layers[0].keyframes[0];
    frame.bitmap_fills[0].matrix = [0.; 6];
    frame.elements[0].transform.alpha = 0.5;
    let rgba = render_project_frame_rgba(&project, None, 0, 20, 16, true, false).unwrap();
    let at = |x: usize, y: usize| rgba.as_chunks::<4>().0[y * 20 + x];
    assert_eq!(at(12, 8), [255, 0, 0, 128]);
}

#[test]
fn exports_gif_and_png_sequence_and_honors_stop_after() {
    let project = Project::demo();
    let dir = std::env::temp_dir().join(format!("neoflash-video-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let gif_path = dir.join("demo.gif");
    let gif_report = export_project_video(
        &project,
        &gif_path,
        &VideoOptions {
            format: VideoFormat::Gif,
            width: 160,
            height: 100,
            fps: 10.0,
            stop_mode: VideoStopMode::AfterSeconds(0.3),
            ..VideoOptions::default()
        },
    )
    .expect("export gif");
    assert_eq!(gif_report.frame_count, 3);
    assert!(gif_path.is_file());
    assert!(gif_report.bytes > 64);

    let png_path = dir.join("seq.png");
    let png_report = export_project_video(
        &project,
        &png_path,
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 120,
            height: 80,
            from_frame: 1,
            to_frame: Some(2),
            ..VideoOptions::default()
        },
    )
    .expect("export png sequence");
    assert_eq!(png_report.frame_count, 2);
    assert!(dir.join("seq_0001.png").is_file());
    assert!(dir.join("seq_0002.png").is_file());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn runtime_capture_toggle_controls_nested_movie_clip_animation() {
    let mut project = Project::blank();
    project.width = 100;
    project.height = 100;
    let child_id = "mc_child".to_owned();
    project.symbols.push(Symbol {
        id: child_id.clone(),
        name: "ChildMC".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![Layer {
                id: "cl".into(),
                name: "L1".into(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: String::new(),
                        elements: vec![Element {
                            id: "r0".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 100.0,
                                height: 100.0,
                                fill: [255, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        tween: None,
                        tween_kind: Default::default(),
                        keyed_properties: None,
                        orient_to_path: false,
                        property_eases: Vec::new(),
                        motion_path: None,
                        sync_symbols: false,
                        snap_to_guide: false,
                        sound: None,
                        shape_hints: Vec::new(),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 1,
                        label: String::new(),
                        elements: vec![Element {
                            id: "r1".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 100.0,
                                height: 100.0,
                                fill: [0, 255, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        tween: None,
                        tween_kind: Default::default(),
                        keyed_properties: None,
                        orient_to_path: false,
                        property_eases: Vec::new(),
                        motion_path: None,
                        sync_symbols: false,
                        snap_to_guide: false,
                        sound: None,
                        shape_hints: Vec::new(),
                        ..Keyframe::default()
                    },
                ],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        linkage_id: None,
        class_name: None,
    });
    let root_sym = project.symbol_mut(&project.root.clone()).unwrap();
    root_sym.timeline.frame_count = 2;
    root_sym.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "inst".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: child_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });

    let captured_f1 = render_project_frame_rgba(&project, None, 1, 32, 32, false, true).unwrap();
    let static_f1 = render_project_frame_rgba(&project, None, 1, 32, 32, false, false).unwrap();
    // Frame 1 with runtime_capture renders green (child frame 1); without runtime_capture renders red (child frame 0).
    let center_idx = (16 * 32 + 16) * 4;
    assert_eq!(&captured_f1[center_idx..center_idx + 3], &[0, 255, 0]);
    assert_eq!(&static_f1[center_idx..center_idx + 3], &[255, 0, 0]);
}

#[test]
fn mixes_timeline_audio_and_exports_mp4_when_ffmpeg_present() {
    let mut project = Project::demo();
    // Attach a small 44.1 kHz sine wave WAV resource to frame 0.
    let mut samples = Vec::with_capacity(4410);
    for i in 0..4410 {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin()) * 0.5;
        samples.push([v, v]);
    }
    let wav_bytes = crate::audio::encode_wav(
        &samples,
        44_100,
        crate::AudioChannels::Stereo,
        crate::AudioBitDepth::Bit16,
    );
    project.resources.push(
        ResourceUnit::from_bytes("snd_1", "audio/tone.wav", "audio/wav", &wav_bytes)
            .expect("resource unit"),
    );
    let root_id = project.root.clone();
    let mut snd = FrameSound::new("snd_1");
    snd.sync = SoundSync::Event;
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound = Some(snd);
    project.format_version = 6;

    if detect_ffmpeg().is_some() {
        let dir = std::env::temp_dir().join(format!("neoflash-mp4-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mp4_path = dir.join("out.mp4");
        let report = export_project_video(
            &project,
            &mp4_path,
            &VideoOptions {
                format: VideoFormat::Mp4,
                width: 160,
                height: 100,
                from_frame: 1,
                to_frame: Some(3),
                include_audio: true,
                ..VideoOptions::default()
            },
        )
        .expect("export mp4");
        assert!(mp4_path.is_file());
        assert!(report.bytes > 256);
        assert!(report.audio_included);
        let _ = fs::remove_dir_all(&dir);
    }
}

pub(super) fn build_four_color_mp4_fixture() -> Vec<u8> {
    let w = 16u32;
    let h = 16u32;
    let colors: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ];
    let frames: Vec<Vec<u8>> = colors
        .into_iter()
        .map(|c| {
            let mut buf = Vec::with_capacity((w * h * 4) as usize);
            for _ in 0..(w * h) {
                buf.extend_from_slice(&c);
            }
            buf
        })
        .collect();
    let frame_refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
    encode_deterministic_mp4_rgba_frames(w, h, 10, &frame_refs)
}

fn build_solid_mp4_fixture(color: [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..(16 * 16) {
        rgba.extend_from_slice(&color);
    }
    encode_deterministic_mp4_rgba_frames(16, 16, 10, &[&rgba])
}

#[test]
fn pam_video_pipe_is_parsed_incrementally_without_a_whole_stream_buffer() {
    let mut bytes = Vec::new();
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        bytes.extend_from_slice(
            b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
        );
        bytes.extend_from_slice(&color);
    }
    let mut reader = BufReader::new(Cursor::new(bytes));
    let raw = parse_pam_rgba_frames(&mut reader).unwrap();
    assert_eq!((raw.width, raw.height, raw.images.len()), (1, 1, 2));
    let stream = finish_pam_rgba_frames(raw, 10.0).unwrap();
    assert_eq!(stream.duration_ms, 200);
    assert_eq!(&stream.frames[0].image.rgba[..4], &[255, 0, 0, 255]);
    assert_eq!(&stream.frames[1].image.rgba[..4], &[0, 0, 255, 255]);
}

#[test]
fn video_decode_rejects_oversized_pam_before_allocating_payload() {
    let header = format!(
        "P7\nWIDTH {}\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nENDHDR\n",
        MAX_DECODED_VIDEO_WIDTH + 1
    );
    let mut reader = BufReader::new(Cursor::new(header.into_bytes()));
    let error = parse_pam_rgba_frames(&mut reader).unwrap_err();
    assert!(error.contains("dimensions"), "got: {error}");

    let error = checked_decoded_video_payload_len(4096, 4096, 9).unwrap_err();
    assert!(error.contains("retained-frame budget"), "got: {error}");
    let error = checked_decoded_video_duration_ms(1, 0.0001).unwrap_err();
    assert!(error.contains("duration"), "got: {error}");
}

#[test]
fn staged_video_and_wav_files_are_exclusive_and_removed_by_drop_guard() {
    for (name, extension) in [("fixture", "mp4"), ("timeline audio", "wav")] {
        let staged = StagedMediaFile::create(name, extension, b"safe bytes").unwrap();
        let path = staged.path().to_path_buf();
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some(extension)
        );
        assert_eq!(fs::read(&path).unwrap(), b"safe bytes");
        drop(staged);
        assert!(!path.exists());
    }
}

#[test]
fn external_video_read_is_bounded_before_allocation() {
    let path = env::temp_dir().join(format!(
        "neoflash-bounded-external-video-{}-{}",
        std::process::id(),
        VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let file = fs::File::create(&path).unwrap();
    file.set_len(17).unwrap();
    drop(file);
    let error = read_bounded_video_file_with_limit(&path, "large.mp4", 16).unwrap_err();
    let _ = fs::remove_file(path);
    assert!(error.contains("input budget"), "got: {error}");
}

#[test]
fn video_cache_budget_counts_tiny_frame_records_and_shared_aliases() {
    let make_stream = |color: [u8; 4]| {
        Arc::new(DecodedVideoStream {
            width: 1,
            height: 1,
            fps: 24.0,
            duration_ms: 42,
            frames: vec![DecodedVideoFrame {
                timestamp_ms: 0,
                duration_ms: 42,
                image: Arc::new(DecodedImage {
                    width: 1,
                    height: 1,
                    rgba: color.to_vec().into(),
                }),
            }],
            on_demand: None,
        })
    };
    let mut cache = HashMap::new();
    let first = make_stream([1, 2, 3, 255]);
    insert_decoded_video_stream_with_limits(
        &mut cache,
        "first".into(),
        first.clone(),
        usize::MAX,
        2,
    )
    .unwrap();
    // A second key for the same allocation must not double-count resident bytes.
    insert_decoded_video_stream_with_limits(&mut cache, "alias".into(), first, usize::MAX, 2)
        .unwrap();
    insert_decoded_video_stream_with_limits(
        &mut cache,
        "second".into(),
        make_stream([4, 5, 6, 255]),
        usize::MAX,
        2,
    )
    .unwrap();
    let error = insert_decoded_video_stream_with_limits(
        &mut cache,
        "third".into(),
        make_stream([7, 8, 9, 255]),
        usize::MAX,
        2,
    )
    .unwrap_err();
    assert!(error.contains("aggregate 2-frame"), "got: {error}");
    assert!(!cache.contains_key("third"));

    let mut structural_cache = HashMap::new();
    let error = insert_decoded_video_stream_with_limits(
        &mut structural_cache,
        "tiny".into(),
        make_stream([0, 0, 0, 255]),
        4,
        100,
    )
    .unwrap_err();
    assert!(
        error.contains("frame records and cache structures"),
        "got: {error}"
    );
}

#[test]
fn bounded_ffmpeg_diagnostics_keep_early_fps_and_final_error() {
    let mut bytes = b"Input #0\n  Stream #0: Video: h264, yuv420p, 29.97 fps, 30 tbr\n".to_vec();
    bytes.resize(MAX_FFMPEG_DIAGNOSTIC_BYTES + 20_000, b'x');
    bytes.extend_from_slice(b"\nfinal codec error\n");
    let capture = read_bounded_process_diagnostics(Cursor::new(bytes)).unwrap();
    let head = String::from_utf8_lossy(&capture.head);
    let tail = String::from_utf8_lossy(&capture.tail);
    assert_eq!(parse_ffmpeg_fps_from_stderr(&head), Some(29.97));
    assert!(tail.contains("final codec error"));
}

#[test]
fn ffprobe_metadata_rejects_decoder_bombs_and_preserves_fractional_fps() {
    let metadata = parse_ffprobe_metadata(
        "width=1920\nheight=1080\nr_frame_rate=30/1\navg_frame_rate=30000/1001\nduration=1.001\nnb_frames=30\n",
    )
    .unwrap();
    assert_eq!((metadata.width, metadata.height), (1920, 1080));
    assert!((metadata.fps - 29.970_03).abs() < 0.001);

    let error = parse_ffprobe_metadata(
        "width=8192\nheight=8192\nr_frame_rate=24/1\navg_frame_rate=24/1\nduration=1\nnb_frames=24\n",
    )
    .unwrap_err();
    assert!(error.contains("dimensions"), "got: {error}");
    let error = parse_ffprobe_metadata(
        "width=1920\nheight=1080\nr_frame_rate=60/1\navg_frame_rate=60/1\nduration=10\nnb_frames=600\n",
    )
    .unwrap_err();
    assert!(error.contains("retained-frame budget"), "got: {error}");
}

#[test]
fn video_decode_cache_distinguishes_source_overrides_for_one_resource() {
    use neoflash_core::{LayerProperties, TimelineVideoSpan};
    let dir = std::env::temp_dir().join(format!(
        "neoflash-video-source-cache-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("media")).unwrap();
    fs::write(
        dir.join("media/red.mp4"),
        build_solid_mp4_fixture([255, 0, 0, 255]),
    )
    .unwrap();
    fs::write(
        dir.join("media/blue.mp4"),
        build_solid_mp4_fixture([0, 0, 255, 255]),
    )
    .unwrap();

    let mut project = Project::blank();
    project.width = 32;
    project.height = 16;
    project
        .resources
        .push(ResourceUnit::from_bytes("shared", "media/red.mp4", "video/mp4", &[]).unwrap());
    let make_layer = |id: &str, element_id: &str, x: f64, path: &str| Layer {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: element_id.into(),
                transform: Transform {
                    x,
                    ..Transform::default()
                },
                kind: ElementKind::Video {
                    width: 16.0,
                    height: 16.0,
                },
                ..Element::default()
            }],
            video: Some(TimelineVideoSpan::external("shared", path)),
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    };
    project.symbols[0].timeline.layers = vec![
        make_layer("red-layer", "red-video", 0.0, "media/red.mp4"),
        make_layer("blue-layer", "blue-video", 16.0, "media/blue.mp4"),
    ];
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let rgba = render_project_frame_rgba_with_base_dir(
        &project,
        None,
        0,
        32,
        16,
        false,
        false,
        Some(&dir),
    )
    .unwrap();
    let pixel = |x: usize, y: usize| &rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
    assert_eq!(pixel(8, 8), &[255, 0, 0, 255]);
    assert_eq!(pixel(24, 8), &[0, 0, 255, 255]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn t201_offline_video_export_composites_decoded_embedded_and_external_frames_sync_trims_nested_and_transforms()
 {
    use neoflash_core::{LayerProperties, TimelineVideoSpan};
    let mp4_bytes = build_four_color_mp4_fixture();
    let mut project = Project::blank();
    project.width = 64;
    project.height = 64;
    project.fps = 10;
    project.background = [10, 10, 10, 255];
    project.resources.push(
        ResourceUnit::from_bytes("vid_1", "clips/four.mp4", "video/mp4", &mp4_bytes)
            .expect("video resource"),
    );

    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 4;
        let kf = &mut root.timeline.layers[0].keyframes[0];
        kf.frame = 0;
        kf.video = Some(TimelineVideoSpan::embedded("vid_1"));
        kf.elements = vec![Element {
            id: "v_el".into(),
            name: "video_inst".into(),
            transform: Transform {
                x: 8.0,
                y: 8.0,
                scale_x: 2.0,
                scale_y: 2.0,
                ..Transform::default()
            },
            kind: ElementKind::Video {
                width: 16.0,
                height: 16.0,
            },
            ..Element::default()
        }];
    }
    project.upgrade_format_for_features();
    project.validate().expect("valid video project");

    // 1. Stream sync across frames 0..4 produces Red, Green, Blue, Yellow at (16, 16).
    let sample_px = |rgba: &[u8], x: u32, y: u32| -> [u8; 4] {
        let idx = ((y * 64 + x) * 4) as usize;
        [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
    };
    let f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, false).unwrap();
    let f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, false).unwrap();
    let f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
    let f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, false).unwrap();
    assert_eq!(sample_px(&f0, 16, 16), [255, 0, 0, 255]);
    assert_eq!(sample_px(&f1, 16, 16), [0, 255, 0, 255]);
    assert_eq!(sample_px(&f2, 16, 16), [0, 0, 255, 255]);
    assert_eq!(sample_px(&f3, 16, 16), [255, 255, 0, 255]);
    // Outside the placed 32x32 scaled video quad (at 4, 4), background remains [10, 10, 10, 255].
    assert_eq!(sample_px(&f0, 4, 4), [10, 10, 10, 255]);

    // 2. Trim [in_point_ms = 100, out_point_ms = 300] (Green & Blue only):
    {
        let span = project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .video
            .as_mut()
            .unwrap();
        span.start_offset_ms = 100;
        span.end_offset_ms = Some(300);
        span.loop_video = false;
    }
    project.upgrade_format_for_features();
    let clamp_f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, false).unwrap();
    let clamp_f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, false).unwrap();
    let clamp_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
    assert_eq!(sample_px(&clamp_f0, 16, 16), [0, 255, 0, 255]);
    assert_eq!(sample_px(&clamp_f1, 16, 16), [0, 0, 255, 255]);
    assert_eq!(
        sample_px(&clamp_f2, 16, 16),
        [0, 0, 255, 255],
        "clamped at out_point_ms when loop_video is false"
    );

    {
        let span = project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .video
            .as_mut()
            .unwrap();
        span.loop_video = true;
    }
    let loop_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
    let loop_f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, false).unwrap();
    assert_eq!(
        sample_px(&loop_f2, 16, 16),
        [0, 255, 0, 255],
        "wrapped back to in_point_ms (100ms = Green) when loop_video is true"
    );
    assert_eq!(sample_px(&loop_f3, 16, 16), [0, 0, 255, 255]);

    // 3. VideoSyncMode::Event + VideoSyncMode::Stop retains the paused frame at the Stop keyframe:
    {
        let root = project.symbol_mut(&root_id).unwrap();
        let el = root.timeline.layers[0].keyframes[0].elements[0].clone();
        let mut start_span = TimelineVideoSpan::embedded("vid_1");
        start_span.sync = VideoSyncMode::Event;
        root.timeline.layers[0].keyframes[0].video = Some(start_span);
        let mut stop_span = TimelineVideoSpan::embedded("vid_1");
        stop_span.sync = VideoSyncMode::Stop;
        root.timeline.layers[0].keyframes.push(Keyframe {
            frame: 2,
            elements: vec![el],
            video: Some(stop_span),
            ..Keyframe::default()
        });
    }
    let ev_f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, true).unwrap();
    let ev_f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, true).unwrap();
    let ev_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, true).unwrap();
    let ev_f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, true).unwrap();
    assert_eq!(sample_px(&ev_f0, 16, 16), [255, 0, 0, 255]);
    assert_eq!(sample_px(&ev_f1, 16, 16), [0, 255, 0, 255]);
    assert_eq!(
        sample_px(&ev_f2, 16, 16),
        [0, 255, 0, 255],
        "Stop keyframe at frame 2 retains the last presented frame (Green)"
    );
    assert_eq!(sample_px(&ev_f3, 16, 16), [0, 255, 0, 255]);

    // 4. External deployment relative to base_dir + nested MovieClip + layer mask + PNG sequence & MP4 export:
    let dir = std::env::temp_dir().join(format!("neoflash-t201-video-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("media")).unwrap();
    let external_rel = "media/clip.mp4";
    fs::write(dir.join(external_rel), &mp4_bytes).unwrap();

    let mut ext_project = Project::blank();
    ext_project.width = 64;
    ext_project.height = 64;
    ext_project.fps = 10;
    ext_project.background = [0, 0, 0, 255];
    ext_project
        .resources
        .push(ResourceUnit::from_bytes("vid_ext", external_rel, "video/mp4", &[]).unwrap());
    let child_id = "nested_video_mc".to_owned();
    let ext_span = TimelineVideoSpan::external("vid_ext", external_rel);
    ext_project.symbols.push(Symbol {
        id: child_id.clone(),
        name: "NestedVideo".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![
                Layer {
                    id: "masked_video_layer".into(),
                    name: "Video".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Normal,
                        mask: Some("mask_layer".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        video: Some(ext_span),
                        elements: vec![Element {
                            id: "nested_v".into(),
                            name: String::new(),
                            transform: Transform {
                                x: 0.0,
                                y: 0.0,
                                scale_x: 4.0,
                                scale_y: 4.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Video {
                                width: 16.0,
                                height: 16.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "mask_layer".into(),
                    name: "Mask".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "mask_rect".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 32.0,
                                height: 64.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        linkage_id: None,
        class_name: None,
    });
    let ext_root = ext_project.root.clone();
    {
        let root = ext_project.symbol_mut(&ext_root).unwrap();
        root.timeline.frame_count = 4;
        root.timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "child_inst".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: child_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        }];
    }
    ext_project.upgrade_format_for_features();
    ext_project
        .validate()
        .expect("valid external nested project");

    let seq_path = dir.join("out_seq.png");
    let seq_report = export_project_video(
        &ext_project,
        &seq_path,
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 64,
            height: 64,
            from_frame: 1,
            to_frame: Some(3),
            runtime_capture: true,
            base_dir: Some(dir.clone()),
            ..VideoOptions::default()
        },
    )
    .expect("export png sequence with external nested video");
    assert_eq!(seq_report.frame_count, 3);
    let png1 = image::open(dir.join("out_seq_0001.png"))
        .unwrap()
        .to_rgba8();
    let png2 = image::open(dir.join("out_seq_0002.png"))
        .unwrap()
        .to_rgba8();
    let png3 = image::open(dir.join("out_seq_0003.png"))
        .unwrap()
        .to_rgba8();
    // Inside left-half mask (x = 16, y = 16): Red, Green, Blue across frames 1..=3.
    assert_eq!(png1.get_pixel(16, 16).0, [255, 0, 0, 255]);
    assert_eq!(png2.get_pixel(16, 16).0, [0, 255, 0, 255]);
    assert_eq!(png3.get_pixel(16, 16).0, [0, 0, 255, 255]);
    // Outside left-half mask (x = 48, y = 16): clipped to black background [0, 0, 0, 255].
    assert_eq!(png1.get_pixel(48, 16).0, [0, 0, 0, 255]);

    // When ffmpeg is available, also verify real H.264 MP4 export + decoding that real H.264 MP4 back!
    if detect_ffmpeg().is_some() {
        let real_mp4_path = dir.join("media/encoded_h264.mp4");
        export_project_video(
            &project,
            &real_mp4_path,
            &VideoOptions {
                format: VideoFormat::Mp4,
                width: 64,
                height: 64,
                from_frame: 1,
                to_frame: Some(2),
                include_audio: false,
                ..VideoOptions::default()
            },
        )
        .expect("export real H.264 mp4");
        let real_mp4_bytes = fs::read(&real_mp4_path).unwrap();
        let decoded_real =
            decode_video_stream_bytes("encoded_h264.mp4", "video/mp4", &real_mp4_bytes)
                .expect("decode real H.264 mp4 via ffmpeg");
        assert!(decoded_real.frames.len() >= 2);
        assert_eq!(decoded_real.width, 64);
        assert_eq!(decoded_real.height, 64);
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn t201_offline_video_export_fails_actionably_on_missing_external_corrupt_or_progressive_without_partial_output()
 {
    use neoflash_core::TimelineVideoSpan;
    let dir = std::env::temp_dir().join(format!("neoflash-t201-neg-video-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let make_project_with_span = |res: ResourceUnit, span: TimelineVideoSpan| -> Project {
        let mut p = Project::blank();
        p.width = 32;
        p.height = 32;
        p.resources.push(res);
        let root_id = p.root.clone();
        let root = p.symbol_mut(&root_id).unwrap();
        root.timeline.layers[0].keyframes[0].video = Some(span);
        root.timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "v".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Video {
                width: 32.0,
                height: 32.0,
            },
            ..Element::default()
        }];
        p.upgrade_format_for_features();
        p
    };

    // 1. Progressive remote URL in offline export fails actionably without creating output.
    let prog_span =
        TimelineVideoSpan::progressive("vid_prog", "https://cdn.example.com/remote.mp4");
    let prog_proj = make_project_with_span(
        ResourceUnit::from_bytes("vid_prog", "clips/remote.mp4", "video/mp4", &[]).unwrap(),
        prog_span,
    );
    let prog_out = dir.join("prog.gif");
    let prog_err = export_project_video(
        &prog_proj,
        &prog_out,
        &VideoOptions {
            format: VideoFormat::Gif,
            width: 32,
            height: 32,
            ..VideoOptions::default()
        },
    )
    .expect_err("progressive remote URL must fail offline export");
    assert!(prog_err.to_string().contains("Progressive"));
    assert!(!prog_out.exists(), "no partial output file may remain");

    // 2. Missing external file fails actionably without creating output.
    let miss_span = TimelineVideoSpan::external("vid_miss", "clips/does_not_exist.mp4");
    let miss_proj = make_project_with_span(
        ResourceUnit::from_bytes("vid_miss", "clips/does_not_exist.mp4", "video/mp4", &[]).unwrap(),
        miss_span,
    );
    let miss_out = dir.join("miss.gif");
    let miss_err = export_project_video(
        &miss_proj,
        &miss_out,
        &VideoOptions {
            format: VideoFormat::Gif,
            width: 32,
            height: 32,
            base_dir: Some(dir.clone()),
            ..VideoOptions::default()
        },
    )
    .expect_err("missing external file must fail offline export");
    assert!(miss_err.to_string().contains("was not found"));
    assert!(!miss_out.exists(), "no partial output file may remain");

    // 3. Corrupt / unsupported codec fails actionably without creating output.
    let corrupt_proj = make_project_with_span(
        ResourceUnit::from_bytes("vid_bad", "corrupt.mp4", "video/mp4", b"not-a-valid-mp4")
            .unwrap(),
        TimelineVideoSpan::embedded("vid_bad"),
    );
    let corrupt_out = dir.join("corrupt.gif");
    let corrupt_err = export_project_video(
        &corrupt_proj,
        &corrupt_out,
        &VideoOptions {
            format: VideoFormat::Gif,
            width: 32,
            height: 32,
            ..VideoOptions::default()
        },
    )
    .expect_err("corrupt video container must fail offline export");
    assert!(
        corrupt_err.to_string().contains("corrupt.mp4")
            || corrupt_err.to_string().contains("Unsupported")
    );
    assert!(!corrupt_out.exists(), "no partial output file may remain");

    // The shared authored-scene rasterizer must propagate the same decoder failure to every
    // export family instead of silently omitting the video layer.
    let atlas_out = dir.join("corrupt_atlas.json");
    let atlas_err = crate::atlas::export_project_atlas(
        &corrupt_proj,
        &atlas_out,
        &crate::atlas::AtlasOptions::default(),
    )
    .expect_err("atlas export must propagate corrupt video decode");
    assert!(
        atlas_err.to_string().contains("corrupt.mp4")
            || atlas_err.to_string().contains("Unsupported"),
        "got: {atlas_err}"
    );
    assert!(!atlas_out.exists());
    assert!(!atlas_out.with_extension("png").exists());

    for (format, filename) in [
        (crate::image_export::ImageExportFormat::Psd, "corrupt.psd"),
        (crate::image_export::ImageExportFormat::Ai, "corrupt.ai"),
    ] {
        let output = dir.join(filename);
        let mut image_options =
            crate::image_export::ImageExportOptions::for_project(&corrupt_proj, 0);
        image_options.format = format;
        let error =
            crate::image_export::export_project_image(&corrupt_proj, &output, &image_options)
                .expect_err("raster image export must propagate corrupt video decode");
        assert!(
            error.to_string().contains("corrupt.mp4") || error.to_string().contains("Unsupported"),
            "got: {error}"
        );
        assert!(!output.exists(), "no partial {format:?} output may remain");
    }

    let _ = fs::remove_dir_all(&dir);
}
