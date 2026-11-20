use std::{env, fs, io::Write, net::IpAddr, process::Command, sync::atomic::Ordering};

use neoflash_core::{
    Element, ElementKind, FrameSound, Keyframe, Layer, Project, ResolvedVideoPlacement,
    ResourceUnit, SoundSync, Symbol, Timeline, Transform, VideoSyncMode,
    timeline_video::{VideoDeploymentMode, VideoSourceSpec},
};

use super::ffprobe_ffmpeg::probe_stream_metadata;
use super::progressive::{
    fetch_progressive_video_bytes_with_limit, is_public_progressive_address,
    redacted_progressive_url,
};
use super::tests::build_four_color_mp4_fixture;
use super::*;

#[test]
fn atlas_video_preflight_uses_exact_later_frame_and_target_symbol() {
    use neoflash_core::{SymbolKind, TimelineVideoSpan};

    let video_element = || Element {
        id: "video".into(),
        name: String::new(),
        transform: Transform::default(),
        kind: ElementKind::Video {
            width: 16.0,
            height: 16.0,
        },
        ..Element::default()
    };
    let corrupt_resource =
        || ResourceUnit::from_bytes("bad", "bad.mp4", "video/mp4", b"not-a-valid-mp4").unwrap();
    let dir = env::temp_dir().join(format!(
        "neoflash-atlas-video-context-{}-{}",
        std::process::id(),
        VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let mut later = Project::blank();
    later.width = 16;
    later.height = 16;
    later.resources.push(corrupt_resource());
    let root_id = later.root.clone();
    let root = later.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 2;
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "safe".into(),
                kind: ElementKind::Rectangle {
                    width: 4.0,
                    height: 4.0,
                    fill: [255, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 1,
            elements: vec![video_element()],
            video: Some(TimelineVideoSpan::embedded("bad")),
            ..Keyframe::default()
        },
    ];
    later.upgrade_format_for_features();
    let later_path = dir.join("later.json");
    let later_error = crate::atlas::export_project_atlas(
        &later,
        &later_path,
        &crate::atlas::AtlasOptions {
            start_frame: 0,
            end_frame: Some(1),
            ..crate::atlas::AtlasOptions::default()
        },
    )
    .expect_err("frame-1 corrupt video must not disappear from atlas preflight");
    assert!(later_error.to_string().contains("bad.mp4"));
    assert!(!later_path.exists());
    assert!(!later_path.with_extension("png").exists());

    let mut nested = Project::blank();
    nested.width = 16;
    nested.height = 16;
    nested.resources.push(corrupt_resource());
    let child_id = "video_child".to_owned();
    nested.symbols.push(Symbol {
        id: child_id.clone(),
        name: "Video child".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "video_layer".into(),
                name: "Video".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![video_element()],
                    video: Some(TimelineVideoSpan::embedded("bad")),
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    nested.upgrade_format_for_features();
    let child_path = dir.join("child.json");
    let child_error = crate::atlas::export_project_atlas(
        &nested,
        &child_path,
        &crate::atlas::AtlasOptions {
            symbol_id: Some(child_id),
            ..crate::atlas::AtlasOptions::default()
        },
    )
    .expect_err("target-symbol corrupt video must not resolve against project root");
    assert!(child_error.to_string().contains("bad.mp4"));
    assert!(!child_path.exists());
    assert!(!child_path.with_extension("png").exists());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn t229_long_form_video_decodes_on_demand_within_pinned_ring_and_matches_seeks_and_trims() {
    use neoflash_core::ResourceUnit;

    // 1. Short clip fast path (<= MAX_EAGER_VIDEO_FRAMES and <= 32 MiB): retains eager frames.
    let short_mp4 = build_four_color_mp4_fixture();
    let short_stream = decode_video_stream_bytes("short.mp4", "video/mp4", &short_mp4).unwrap();
    assert!(short_stream.on_demand.is_none());
    assert_eq!(short_stream.frames.len(), 4);

    // 2. Long-form deterministic nfvf stream (240 frames > MAX_EAGER_VIDEO_FRAMES = 180):
    // switches automatically to OnDemandVideoDecoder with bounded ring eviction.
    let frame_count = 240usize;
    let raw_frames: Vec<Vec<u8>> = (0..frame_count)
        .map(|idx| [idx as u8, ((idx * 3) & 0xFF) as u8, 42, 255].repeat(8 * 8))
        .collect();
    let raw_slices: Vec<&[u8]> = raw_frames.iter().map(Vec::as_slice).collect();
    let long_nfvf = encode_deterministic_mp4_rgba_frames(8, 8, 20, &raw_slices);
    let long_stream = decode_video_stream_bytes("long.mp4", "video/mp4", &long_nfvf).unwrap();
    assert!(long_stream.frames.is_empty());
    let decoder = long_stream
        .on_demand
        .as_ref()
        .expect("expected on-demand windowed decoder");
    assert_eq!(decoder.logical_frame_count(), frame_count);
    assert!(decoder.max_ring_frames() <= MAX_WINDOWED_RING_FRAMES);

    let make_placement = |time_ms: u32,
                          in_point_ms: u32,
                          out_point_ms: Option<u32>,
                          loop_video: bool,
                          sync: VideoSyncMode,
                          stopped: bool| ResolvedVideoPlacement {
        channel: "video".into(),
        target_path: vec!["vid".into()],
        layer_id: "layer".into(),
        resource: "res".into(),
        source: None,
        sync,
        time_ms,
        in_point_ms,
        out_point_ms,
        playing: true,
        loop_video,
        muted: false,
        stopped,
    };

    // Random forward & backward seeks across the 240-frame stream stay pixel-exact and never
    // exceed MAX_WINDOWED_RING_FRAMES.
    for &target_idx in &[0usize, 5, 199, 73, 239, 12, 150, 0] {
        let p = make_placement(
            (target_idx as u32) * 50,
            0,
            None,
            false,
            VideoSyncMode::Stream,
            false,
        );
        let (resolved_idx, frame) = long_stream.frame_at_placement(&p).unwrap();
        assert_eq!(resolved_idx, target_idx);
        assert_eq!(
            &frame.image.rgba[..4],
            &[target_idx as u8, ((target_idx * 3) & 0xFF) as u8, 42, 255]
        );
        assert!(decoder.resident_ring_frames() <= MAX_WINDOWED_RING_FRAMES);
        assert!(decoder.resident_ring_bytes() <= MAX_WINDOWED_RING_STREAM_BYTES);
    }

    // Trim clamps (in_point_ms / out_point_ms), Loop, and Stop semantics match exactly.
    let trimmed = make_placement(5_000, 500, Some(1_500), false, VideoSyncMode::Stream, false);
    let (clamped_idx, _) = long_stream.frame_at_placement(&trimmed).unwrap();
    assert_eq!(
        clamped_idx, 29,
        "non-looped trim must hold final frame before out_point_ms"
    );

    let unstopped = make_placement(0, 0, None, false, VideoSyncMode::Stop, false);
    assert!(long_stream.frame_at_placement(&unstopped).is_none());
    let stopped_hold = make_placement(250, 0, None, false, VideoSyncMode::Stop, true);
    assert_eq!(long_stream.frame_at_placement(&stopped_hold).unwrap().0, 5);

    // 3. Long-form multi-GOP H.264 external MP4 (when ffmpeg + ffprobe are available):
    // verifies real codec seeking across GOP boundaries without loading all frames into RAM.
    if let Some(ffmpeg) = detect_ffmpeg()
        && detect_ffprobe(&ffmpeg).is_some()
    {
        let dir = env::temp_dir().join(format!(
            "neoflash-t229-windowed-h264-{}-{}",
            std::process::id(),
            VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mp4_path = dir.join("long_gop.mp4");
        // 210 frames at 30 fps (7s > MAX_EAGER_VIDEO_FRAMES = 180) with GOP = 30.
        let status = Command::new(&ffmpeg)
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=64x64:r=30:d=7",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "30",
            ])
            .arg(&mp4_path)
            .status()
            .unwrap();
        assert!(status.success());

        let mut project = Project::blank();
        project
            .resources
            .push(ResourceUnit::from_bytes("long_vid", "long_gop.mp4", "video/mp4", &[]).unwrap());
        let spec = VideoSourceSpec {
            mode: VideoDeploymentMode::External,
            url_or_path: "long_gop.mp4".into(),
            mime_type: "video/mp4".into(),
        };
        let ext_stream =
            resolve_and_decode_project_video(&project, "long_vid", Some(&spec), Some(&dir))
                .unwrap();
        assert!(ext_stream.frames.is_empty());
        let ext_dec = ext_stream.on_demand.as_ref().unwrap();
        assert_eq!(ext_dec.logical_frame_count(), 210);

        // Reference full raw decode for exact pixel verification across GOPs:
        let full_raw = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&mp4_path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .output()
            .unwrap();
        assert!(full_raw.status.success());
        let fbytes = 64 * 64 * 4;
        assert_eq!(full_raw.stdout.len(), 210 * fbytes);

        for &seek_frame in &[0usize, 1, 29, 30, 31, 107, 179, 209, 44] {
            let target_ms = ((seek_frame as f64 * 1000.0) / 30.0).round() as u32;
            let p = make_placement(target_ms, 0, None, false, VideoSyncMode::Stream, false);
            let (got_idx, got_frame) = ext_stream.frame_at_placement(&p).unwrap();
            assert_eq!(got_idx, seek_frame);
            let ref_slice = &full_raw.stdout[seek_frame * fbytes..(seek_frame + 1) * fbytes];
            assert_eq!(
                got_frame.image.rgba.as_ref(),
                ref_slice,
                "windowed H.264 seek mismatch at frame {seek_frame}"
            );
            assert!(ext_dec.resident_ring_frames() <= MAX_WINDOWED_RING_FRAMES);
        }

        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn t274_progressive_video_decodes_embedded_staged_and_downloaded_bytes_in_offline_export() {
    use neoflash_core::{ResourceUnit, TimelineVideoSpan};

    let mp4_bytes = build_four_color_mp4_fixture();
    let dir = env::temp_dir().join(format!(
        "neoflash-t274-progressive-export-{}-{}",
        std::process::id(),
        VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("media")).unwrap();
    fs::write(dir.join("media/prog_clip.mp4"), &mp4_bytes).unwrap();

    let make_prog_project = |res: ResourceUnit, url: &str| -> Project {
        let mut p = Project::blank();
        p.width = 64;
        p.height = 64;
        p.fps = 10;
        p.background = [10, 10, 10, 255];
        let res_id = res.id.clone();
        p.resources.push(res);
        let root_id = p.root.clone();
        let root = p.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 4;
        root.timeline.layers[0].keyframes[0].video =
            Some(TimelineVideoSpan::progressive(res_id, url));
        root.timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "v_prog".into(),
            name: String::new(),
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
        p.upgrade_format_for_features();
        p
    };

    let sample_px = |rgba: &[u8], x: u32, y: u32| -> [u8; 4] {
        let idx = ((y * 64 + x) * 4) as usize;
        [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
    };

    // 1. Progressive span with embedded/cached resource bytes in Project::resources:
    let mut emb_prog_proj = make_prog_project(
        ResourceUnit::from_bytes(
            "vid_prog_emb",
            "media/prog_clip.mp4",
            "video/mp4",
            &mp4_bytes,
        )
        .unwrap(),
        "https://cdn.example.com/media/prog_clip.mp4",
    );
    // Also keep an Embedded library symbol referencing vid_prog_emb so project validation allows
    // non-empty data_base64 on the shared resource while the stage span uses Progressive mode.
    emb_prog_proj.symbols.push(Symbol {
        id: "lib_emb_holder".into(),
        name: "LibHolder".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "l".into(),
                name: "L".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    video: Some(TimelineVideoSpan::embedded("vid_prog_emb")),
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        linkage_id: None,
        class_name: None,
    });
    let f0 = render_project_frame_rgba(&emb_prog_proj, None, 0, 64, 64, false, false).unwrap();
    let f1 = render_project_frame_rgba(&emb_prog_proj, None, 1, 64, 64, false, false).unwrap();
    let f2 = render_project_frame_rgba(&emb_prog_proj, None, 2, 64, 64, false, false).unwrap();
    let f3 = render_project_frame_rgba(&emb_prog_proj, None, 3, 64, 64, false, false).unwrap();
    assert_eq!(sample_px(&f0, 16, 16), [255, 0, 0, 255]);
    assert_eq!(sample_px(&f1, 16, 16), [0, 255, 0, 255]);
    assert_eq!(sample_px(&f2, 16, 16), [0, 0, 255, 255]);
    assert_eq!(sample_px(&f3, 16, 16), [255, 255, 0, 255]);

    // 2. Progressive span with empty resource bytes resolved from local/staged file path under base_dir:
    let local_prog_proj = make_prog_project(
        ResourceUnit::from_bytes("vid_prog_local", "media/prog_clip.mp4", "video/mp4", &[])
            .unwrap(),
        "https://cdn.example.com/media/prog_clip.mp4",
    );
    let seq_out = dir.join("prog_seq.png");
    let report = export_project_video(
        &local_prog_proj,
        &seq_out,
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 64,
            height: 64,
            from_frame: 1,
            to_frame: Some(4),
            base_dir: Some(dir.clone()),
            ..VideoOptions::default()
        },
    )
    .expect("progressive video with staged local file exports PNG sequence");
    assert_eq!(report.frame_count, 4);
    let p1 = image::open(dir.join("prog_seq_0001.png"))
        .unwrap()
        .to_rgba8();
    let p2 = image::open(dir.join("prog_seq_0002.png"))
        .unwrap()
        .to_rgba8();
    let p3 = image::open(dir.join("prog_seq_0003.png"))
        .unwrap()
        .to_rgba8();
    let p4 = image::open(dir.join("prog_seq_0004.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(p1.get_pixel(16, 16).0, [255, 0, 0, 255]);
    assert_eq!(p2.get_pixel(16, 16).0, [0, 255, 0, 255]);
    assert_eq!(p3.get_pixel(16, 16).0, [0, 0, 255, 255]);
    assert_eq!(p4.get_pixel(16, 16).0, [255, 255, 0, 255]);

    // 3. Remote-only Progressive spans fetch and decode real bytes. They must never replace
    // unavailable authored media with synthetic pixels.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let served_mp4 = mp4_bytes.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        drain_http_request(&stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            served_mp4.len()
        )
        .unwrap();
        stream.write_all(&served_mp4).unwrap();
        stream.flush().unwrap();
    });
    let remote_url = format!("http://{address}/streams/remote.mp4");
    let remote_prog_proj = make_prog_project(
        ResourceUnit::from_bytes("vid_prog_remote", "streams/remote.mp4", "video/mp4", &[])
            .unwrap(),
        &remote_url,
    );
    let rf0 = render_project_frame_rgba(&remote_prog_proj, None, 0, 64, 64, false, false).unwrap();
    let rf1 = render_project_frame_rgba(&remote_prog_proj, None, 1, 64, 64, false, false).unwrap();
    let rf2 = render_project_frame_rgba(&remote_prog_proj, None, 2, 64, 64, false, false).unwrap();
    server.join().unwrap();
    assert_eq!(sample_px(&rf0, 16, 16), [255, 0, 0, 255]);
    assert_eq!(sample_px(&rf1, 16, 16), [0, 255, 0, 255]);
    assert_eq!(sample_px(&rf2, 16, 16), [0, 0, 255, 255]);

    let _ = fs::remove_dir_all(&dir);
}

fn drain_http_request(stream: &std::net::TcpStream) {
    // Consume complete request headers and any body before answering or dropping so
    // Windows loopback does not emit WSAECONNABORTED on short responses.
    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        assert_ne!(
            std::io::BufRead::read_line(&mut reader, &mut line).unwrap(),
            0
        );
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0u8; content_length];
    std::io::Read::read_exact(&mut reader, &mut body).unwrap();
}

#[test]
fn progressive_download_reports_http_errors_and_enforces_declared_and_streamed_limits() {
    fn serve_once(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            drain_http_request(&stream);
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        });
        (format!("http://{address}/clip.mp4"), handle)
    }

    let (status_url, status_server) = serve_once(
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );
    let status_error = fetch_progressive_video_bytes_with_limit(&status_url, 16).unwrap_err();
    status_server.join().unwrap();
    assert!(status_error.contains("HTTP status 503"), "{status_error}");

    let (declared_url, declared_server) =
        serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n".to_vec());
    let declared_error = fetch_progressive_video_bytes_with_limit(&declared_url, 16).unwrap_err();
    declared_server.join().unwrap();
    assert!(
        declared_error.contains("declares 17 bytes")
            && declared_error.contains("16-byte download budget"),
        "{declared_error}"
    );

    let mut streamed_response = b"HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    streamed_response.extend_from_slice(&[7u8; 17]);
    let (streamed_url, streamed_server) = serve_once(streamed_response);
    let streamed_error = fetch_progressive_video_bytes_with_limit(&streamed_url, 16).unwrap_err();
    streamed_server.join().unwrap();
    assert!(
        streamed_error.contains("16-byte download budget"),
        "{streamed_error}"
    );
}

#[test]
fn progressive_network_policy_blocks_private_ranges_and_redacts_credentials() {
    for address in [
        "127.0.0.1",
        "10.2.3.4",
        "169.254.7.8",
        "192.168.1.2",
        "100.64.1.2",
        "::1",
        "::127.0.0.1",
        "64:ff9b::127.0.0.1",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:7f00:1::1",
        "3fff::1",
    ] {
        let parsed = address.parse::<IpAddr>().unwrap();
        assert!(!is_public_progressive_address(parsed), "{address}");
    }
    for address in [
        "1.1.1.1",
        "8.8.8.8",
        "2001:4860:4860::8888",
        "2606:4700:4700::1111",
    ] {
        let parsed = address.parse::<IpAddr>().unwrap();
        assert!(is_public_progressive_address(parsed), "{address}");
    }

    let safe =
        redacted_progressive_url("https://user:password@example.test/video.mp4?token=secret#frame");
    assert_eq!(safe, "https://example.test/video.mp4");
    assert!(!safe.contains("password"));
    assert!(!safe.contains("secret"));
}

#[test]
fn corrupt_staged_progressive_bytes_are_discarded_before_network_refetch() {
    let mp4_bytes = build_four_color_mp4_fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let served_mp4 = mp4_bytes.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        drain_http_request(&stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            served_mp4.len()
        )
        .unwrap();
        stream.write_all(&served_mp4).unwrap();
        stream.flush().unwrap();
    });
    let url = format!("http://{address}/fresh.mp4?token=do-not-log");
    stage_progressive_video_bytes(url.clone(), b"not a video".to_vec());
    let spec = VideoSourceSpec {
        mode: VideoDeploymentMode::Progressive,
        url_or_path: url.clone(),
        mime_type: "video/mp4".into(),
    };

    let stream =
        resolve_and_decode_project_video(&Project::blank(), "remote-video", Some(&spec), None)
            .expect("invalid staged bytes are removed and the authored URL is fetched");
    server.join().unwrap();
    assert_eq!(stream.logical_frame_count(), 4);
    assert!(lookup_staged_progressive_video_bytes(&url).is_some());
    discard_staged_progressive_video_bytes(&url);
}

#[test]
fn t283_bundled_ffmpeg_and_ffprobe_work_without_path_for_h264_h265_webm_and_audio() {
    use crate::audio::{
        AudioBitRate, AudioChannels, AudioExportOptions, AudioFormat, AudioSampleRate, encode_aac,
        encode_mp3, export_project_audio,
    };

    let (bundled_ffmpeg, bundled_ffprobe) =
        match (detect_bundled_ffmpeg(), detect_bundled_ffprobe()) {
            (Some(ffmpeg), Some(ffprobe)) => (ffmpeg, ffprobe),
            (None, None) => {
                // Fresh cross-platform checkouts do not download a 2+ GiB media bundle.
                // The platform package workflows make the bundle mandatory and run the
                // same empty-PATH codec round trips against the packaged files.
                eprintln!(
                    "Skipping staged-media round trip; no bundled ffmpeg/ffprobe is present."
                );
                return;
            }
            _ => panic!("Staged bundled media must contain both ffmpeg and ffprobe."),
        };

    // 1. Bundled helpers are discovered before PATH and marked as bundled media binaries:
    assert!(is_bundled_media_binary(&bundled_ffmpeg));
    assert!(is_bundled_media_binary(&bundled_ffprobe));
    if env::var("NEOFLASH_FFMPEG")
        .ok()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        assert_eq!(detect_ffmpeg().as_ref(), Some(&bundled_ffmpeg));
    }
    if env::var("NEOFLASH_FFPROBE")
        .ok()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        assert_eq!(
            detect_ffprobe(&bundled_ffmpeg).and_then(|p| p.canonicalize().ok()),
            Some(bundled_ffprobe.clone())
        );
    }

    // 2. On macOS, verify otool -L has zero /opt/homebrew, /usr/local, or @rpath references:
    if cfg!(target_os = "macos") {
        let lib_dir = bundled_ffmpeg
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("lib"))
            .expect("bundled lib dir");
        assert!(
            lib_dir.is_dir(),
            "expected bundled dylib directory at {lib_dir:?}"
        );

        let mut binaries_to_inspect = vec![bundled_ffmpeg.clone(), bundled_ffprobe.clone()];
        for entry in fs::read_dir(&lib_dir).unwrap().flatten() {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("dylib") {
                binaries_to_inspect.push(entry.path());
            }
        }
        for bin_path in &binaries_to_inspect {
            let otool_out = Command::new("/usr/bin/otool")
                .arg("-L")
                .arg(bin_path)
                .output()
                .expect("run /usr/bin/otool -L");
            assert!(otool_out.status.success());
            let text = String::from_utf8_lossy(&otool_out.stdout);
            for line in text.lines().skip(1) {
                let dep = line.trim();
                assert!(
                    !dep.starts_with("/opt/homebrew")
                        && !dep.starts_with("/usr/local")
                        && !dep.starts_with("@rpath"),
                    "Bundled binary {bin_path:?} still references external/unrelocated path: {dep}"
                );
            }
        }

        // Provenance & SBOM files exist alongside the bundle:
        let bundle_root = bundled_ffmpeg.parent().unwrap().parent().unwrap();
        assert!(bundle_root.join("provenance/media-sbom.json").is_file());
        assert!(
            bundle_root
                .join("provenance/ffmpeg-buildconf.txt")
                .is_file()
        );
        assert!(bundle_root.join("provenance/ffmpeg-license.txt").is_file());
        assert!(bundle_root.join("provenance/SHA256SUMS.txt").is_file());
        assert!(
            bundle_root
                .join("licenses/README-CORRESPONDING-SOURCE.txt")
                .is_file()
        );
    }

    // 3. Direct invocation with cleared environment (`env_clear()` + `PATH=""`):
    let ffmpeg_ver = Command::new(&bundled_ffmpeg)
        .env_clear()
        .env("PATH", "")
        .arg("-version")
        .output()
        .expect("launch bundled ffmpeg with empty env");
    assert!(
        ffmpeg_ver.status.success(),
        "bundled ffmpeg failed with empty env: {}",
        String::from_utf8_lossy(&ffmpeg_ver.stderr)
    );
    let ffprobe_ver = Command::new(&bundled_ffprobe)
        .env_clear()
        .env("PATH", "")
        .arg("-version")
        .output()
        .expect("launch bundled ffprobe with empty env");
    assert!(
        ffprobe_ver.status.success(),
        "bundled ffprobe failed with empty env: {}",
        String::from_utf8_lossy(&ffprobe_ver.stderr)
    );

    // 4. Capability detection and hardware/software encoder selection:
    let encoders = probe_ffmpeg_encoders(&bundled_ffmpeg);
    assert!(encoders.contains("libx264"));
    assert!(encoders.contains("libx265"));
    assert!(encoders.contains("libvpx-vp9"));
    assert!(encoders.contains("libopus"));
    assert!(encoders.contains("libmp3lame"));
    assert_eq!(
        select_video_encoder(&bundled_ffmpeg, VideoCodec::H264, false),
        ("libx264", false)
    );
    assert_eq!(
        select_video_encoder(&bundled_ffmpeg, VideoCodec::H265, false),
        ("libx265", false)
    );
    if cfg!(target_os = "macos") && encoders.contains("h264_videotoolbox") {
        assert_eq!(
            select_video_encoder(&bundled_ffmpeg, VideoCodec::H264, true),
            ("h264_videotoolbox", true)
        );
    }
    if cfg!(target_os = "macos") && encoders.contains("hevc_videotoolbox") {
        assert_eq!(
            select_video_encoder(&bundled_ffmpeg, VideoCodec::H265, true),
            ("hevc_videotoolbox", true)
        );
    }

    // 5. End-to-end export + decode round-trip for MP4 (H.264), MP4 (H.265), WebM (VP9+Opus), and MP3/AAC audio:
    let dir = env::temp_dir().join(format!(
        "neoflash-t283-bundled-media-{}-{}",
        std::process::id(),
        VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let mut project = Project::demo();
    let mut samples = Vec::with_capacity(4410);
    for i in 0..4410 {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin()) * 0.4;
        samples.push([v, v]);
    }
    let wav_bytes = crate::audio::encode_wav(
        &samples,
        44_100,
        crate::AudioChannels::Stereo,
        crate::AudioBitDepth::Bit16,
    );
    project.resources.push(
        ResourceUnit::from_bytes("snd_t283", "audio/tone.wav", "audio/wav", &wav_bytes).unwrap(),
    );
    let root_id = project.root.clone();
    let mut snd = FrameSound::new("snd_t283");
    snd.sync = SoundSync::Event;
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound = Some(snd);
    project.upgrade_format_for_features();

    // 5a. MP4 (H.264 + AAC) export & decode:
    let h264_path = dir.join("out_h264.mp4");
    let h264_report = export_project_video(
        &project,
        &h264_path,
        &VideoOptions {
            format: VideoFormat::Mp4,
            codec: VideoCodec::H264,
            width: 128,
            height: 96,
            from_frame: 1,
            to_frame: Some(3),
            include_audio: true,
            ..VideoOptions::default()
        },
    )
    .expect("export H.264 MP4 with bundled ffmpeg");
    assert!(h264_report.audio_included);
    let h264_decoded =
        decode_video_stream_bytes("out_h264.mp4", "video/mp4", &fs::read(&h264_path).unwrap())
            .expect("decode H.264 MP4 with bundled ffmpeg/ffprobe");
    assert_eq!((h264_decoded.width, h264_decoded.height), (128, 96));
    assert!(h264_decoded.logical_frame_count() >= 3);

    // 5b. MP4 (H.265 / HEVC) export & probe:
    let h265_path = dir.join("out_h265.mp4");
    let h265_report = export_project_video(
        &project,
        &h265_path,
        &VideoOptions {
            format: VideoFormat::Mp4,
            codec: VideoCodec::H265,
            width: 128,
            height: 96,
            from_frame: 1,
            to_frame: Some(3),
            include_audio: false,
            ..VideoOptions::default()
        },
    )
    .expect("export H.265 MP4 with bundled ffmpeg");
    assert!(h265_report.bytes > 128);
    let h265_meta = probe_stream_metadata(&bundled_ffmpeg, &h265_path)
        .expect("probe H.265 MP4 with bundled ffprobe")
        .expect("stream metadata present");
    assert_eq!((h265_meta.width, h265_meta.height), (128, 96));

    // 5c. WebM (VP9 + Opus) export & decode:
    let webm_path = dir.join("out_vp9.webm");
    let webm_report = export_project_video(
        &project,
        &webm_path,
        &VideoOptions {
            format: VideoFormat::WebM,
            codec: VideoCodec::Vp9,
            width: 128,
            height: 96,
            from_frame: 1,
            to_frame: Some(3),
            include_audio: true,
            audio_bitrate: VideoAudioBitrate::Opus160,
            ..VideoOptions::default()
        },
    )
    .expect("export VP9 WebM with bundled ffmpeg");
    assert!(webm_report.audio_included);
    let webm_decoded =
        decode_video_stream_bytes("out_vp9.webm", "video/webm", &fs::read(&webm_path).unwrap())
            .expect("decode VP9 WebM with bundled ffmpeg/ffprobe");
    assert_eq!((webm_decoded.width, webm_decoded.height), (128, 96));
    assert!(webm_decoded.logical_frame_count() >= 3);

    // 5d. MP3 & AAC audio export via bundled ffmpeg:
    let mp3_bytes = encode_mp3(
        &samples,
        44_100,
        AudioChannels::Stereo,
        192,
        neoflash_core::SoundExportQuality::Best,
    )
    .expect("encode MP3 via bundled ffmpeg");
    assert!(mp3_bytes.len() > 128);
    let aac_bytes = encode_aac(&samples, 44_100, AudioChannels::Stereo, 192)
        .expect("encode AAC via bundled ffmpeg");
    assert!(aac_bytes.len() > 128);

    let mp3_out_path = dir.join("mix.mp3");
    let mp3_report = export_project_audio(
        &project,
        &mp3_out_path,
        &AudioExportOptions {
            format: AudioFormat::Mp3,
            sample_rate: AudioSampleRate::Hz44100,
            bit_rate: AudioBitRate::Kbps192,
            channels: AudioChannels::Stereo,
            ..AudioExportOptions::default()
        },
    )
    .expect("export project MP3 audio");
    assert!(mp3_report.bytes > 128);
    assert!(mp3_out_path.is_file());

    let _ = fs::remove_dir_all(&dir);
}
