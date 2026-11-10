//! Multi-pass publish stress & matrix test suite (`T-198`).
//!
//! Exercises exhaustive combinations of `PublishOptions`, `HtmlPublishConfig`, `SizeLevel`,
//! `FontExportPolicy`, `AudioExportPolicy`, `HtmlTemplate`, `HtmlScaleMode`, `PlayerChromeConfig`,
//! `VideoDeploymentMode` (`Embedded`, `External`, `Progressive`), and all publish targets
//! (`HTML5`, `DesktopApp`, `ImageSequence`, `SpriteSheet` / `Atlas`, `Svg`, `Lottie`, `VideoMp4`, `SWF`, and `FLA`/`XFL`).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use neoflash_core::{
    BitmapCompression, BitmapExportSettings, Element, ElementKind, FrameScript, Keyframe, Layer,
    LayerProperties, Project, ResourceUnit, Script, ScriptLanguage, TimelineVideoSpan,
    VideoDeploymentMode,
};
use neoflash_export::{
    AtlasFormat, AtlasOptions, AudioExportPolicy, FlaExportOptions, FlaTargetProfile,
    FontExportPolicy, HtmlAcceleration, HtmlPublishConfig, HtmlScaleMode, HtmlTemplate,
    ImageExportFormat, ImageExportOptions, ImageExportRange, LottieOptions, PlayerChromeConfig,
    PublishOptions, SizeLevel, SvgOptions, SwfCompression, SwfPublishOptions, SwfPublishProfile,
    VideoCodec, VideoFormat, VideoOptions, estimate_publish_options_cancellable_with_config,
    estimate_video, export_desktop_app, export_fla_with_profile, export_lottie_json,
    export_project_atlas, export_project_image, export_project_svg, export_project_with_config,
    export_shared_runtime, gzip_len, import_fla_bytes, import_swf_bytes, publish_swf_to_path,
    render_frame_svg,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_dir(prefix: &str) -> PathBuf {
    static ID: AtomicU64 = AtomicU64::new(1);
    let path = std::env::temp_dir().join(format!(
        "neoflash-publish-stress-{prefix}-{}-{}",
        std::process::id(),
        ID.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn create_mock_runtime(path: &Path) {
    fs::create_dir_all(path).expect("create runtime dir");
    fs::write(path.join("neoflash_web.js"), "var NeoFlashWasm;").expect("write neoflash_web.js");
    fs::write(path.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

fn folder_totals(folder: &Path) -> (usize, usize, usize) {
    fn walk(dir: &Path) -> (usize, usize, usize) {
        let mut count = 0usize;
        let mut bytes = 0usize;
        let mut gz = 0usize;
        for entry in fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                let (c, b, g) = walk(&path);
                count += c;
                bytes += b;
                gz += g;
            } else {
                let data = fs::read(&path).expect("read file");
                count += 1;
                bytes += data.len();
                gz += gzip_len(&data);
            }
        }
        (count, bytes, gz)
    }
    walk(folder)
}

fn encode_test_jpeg(quality: u8) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(24 * 24 * 3);
    for y in 0..24u8 {
        for x in 0..24u8 {
            rgb.push(x.wrapping_mul(9).wrapping_add(y));
            rgb.push(y.wrapping_mul(11));
            rgb.push((x ^ y).wrapping_mul(7));
        }
    }
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(&rgb, 24, 24, image::ExtendedColorType::Rgb8)
        .expect("encode test jpeg");
    out
}

fn valid_mp4_header_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    // ftyp box (24 bytes): isom / mp42
    bytes.extend_from_slice(&24u32.to_be_bytes());
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(b"isom");
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(b"isommp42");
    // moov box with avc1 codec marker
    let payload = b"moov-mvhd-trak-mdia-minf-stbl-stsd-avc1-sample";
    let moov_len = (8 + payload.len()) as u32;
    bytes.extend_from_slice(&moov_len.to_be_bytes());
    bytes.extend_from_slice(b"moov");
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn pass1_size_levels_fonts_and_audio_policies_across_real_projects() {
    let dir = temp_dir("pass1");
    let rt = dir.join("runtime");
    create_mock_runtime(&rt);

    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut projects: Vec<(String, Project)> = vec![("demo".into(), Project::demo())];

    for rel in [
        "examples/timeline-tweens.nflash",
        "examples/timeline-sound.nflash",
        "examples/nested-orbit.nflash",
        "examples/midnight-express-4.nflash",
    ] {
        let p = workspace_root.join(rel);
        if p.is_file() {
            let json = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
            let proj = Project::from_json(&json).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
            projects.push((rel.into(), proj));
        }
    }
    for rel in [
        "examples/fla-corpus/authored/nested-playback-as2/nested-playback-as2.fla",
        "examples/fla-corpus/authored/frame-scripts-as3/frame-scripts-as3.fla",
    ] {
        let p = workspace_root.join(rel);
        if p.is_file()
            && let Ok(proj) = neoflash_import::import_project(&p)
        {
            projects.push((rel.into(), proj));
        }
    }
    assert!(
        projects.len() >= 5,
        "expected at least 5 real projects, got {}",
        projects.len()
    );

    let shared_dir = dir.join("shared-runtime");
    export_shared_runtime(&shared_dir, &rt).expect("export shared runtime");
    let shared_id_js =
        fs::read_to_string(shared_dir.join("runtime-id.js")).expect("read runtime-id.js");

    for (p_idx, (name, project)) in projects.iter().enumerate() {
        let font_policies: &[FontExportPolicy] =
            if name == "demo" || name.contains("timeline-tweens") {
                &[
                    FontExportPolicy::Embed,
                    FontExportPolicy::Subset,
                    FontExportPolicy::Device,
                    FontExportPolicy::Outlines,
                ]
            } else {
                &[FontExportPolicy::Embed, FontExportPolicy::Device]
            };
        for level in [
            SizeLevel::Full,
            SizeLevel::Balanced,
            SizeLevel::Compact,
            SizeLevel::Shared("https://cdn.example.com/neoflash-rt/".into()),
        ] {
            for &font_policy in font_policies {
                let options = PublishOptions {
                    fonts: font_policy,
                    size: level.clone(),
                    audio: AudioExportPolicy::Original,
                };
                let cfg = HtmlPublishConfig::clean_publish_default();
                let out = dir.join(format!("p{p_idx}-{}-{font_policy:?}", level.name()));
                export_project_with_config(project, &out, &rt, &options, &cfg).unwrap_or_else(
                    |e| panic!("export {name} ({}, {font_policy:?}): {e}", level.name()),
                );

                let est = estimate_publish_options_cancellable_with_config(
                    project,
                    &rt,
                    &options,
                    &cfg,
                    || false,
                )
                .expect("estimate result")
                .expect("uncancelled estimate");

                assert_eq!(
                    folder_totals(&out),
                    (est.files, est.bytes, est.gzip_bytes),
                    "size estimate must match exact disk totals for {name} ({}, {font_policy:?})",
                    level.name()
                );

                let html = fs::read_to_string(out.join("index.html")).expect("index.html");
                assert!(html.contains("<body data-clean=\"true\">"));
                assert!(html.contains("<footer hidden>"));

                match &level {
                    SizeLevel::Full => {
                        assert!(!out.join("neoflash_web_bg.wasm").exists());
                        let bindings =
                            fs::read_to_string(out.join("neoflash_web.js")).expect("bindings");
                        assert!(!bindings.contains("NeoFlashWasmBase64"));
                    }
                    SizeLevel::Balanced | SizeLevel::Compact => {
                        assert!(out.join("neoflash_web_bg.wasm").is_file());
                        let bindings =
                            fs::read_to_string(out.join("neoflash_web.js")).expect("bindings");
                        assert!(
                            bindings.contains("globalThis.NeoFlashWasmBase64 = \""),
                            "{} must embed NeoFlashWasmBase64 for direct file:// opening",
                            level.name()
                        );
                    }
                    SizeLevel::Shared(_) => {
                        assert!(!out.join("neoflash_web.js").exists());
                        assert!(!out.join("neoflash_web_bg.wasm").exists());
                        let bundle = fs::read_to_string(out.join("bundle.js")).expect("bundle.js");
                        let runtime_id = bundle
                            .split("\"runtime_id\":\"")
                            .nth(1)
                            .expect("runtime_id in bundle")
                            .split('"')
                            .next()
                            .expect("runtime_id end quote");
                        assert!(
                            shared_id_js.contains(runtime_id),
                            "Shared bundle runtime_id must match export_shared_runtime"
                        );
                    }
                }
            }
        }
    }

    // Also test all AudioExportPolicy variants on timeline-sound.nflash
    if let Some((_, sound_proj)) = projects
        .iter()
        .find(|(n, _)| n.contains("timeline-sound.nflash"))
    {
        for audio_policy in AudioExportPolicy::ALL {
            let options = PublishOptions {
                fonts: FontExportPolicy::Embed,
                size: SizeLevel::Compact,
                audio: audio_policy,
            };
            let cfg = HtmlPublishConfig::clean_publish_default();
            let out = dir.join(format!("sound-audio-{audio_policy:?}"));
            export_project_with_config(sound_proj, &out, &rt, &options, &cfg)
                .expect("export sound project with audio policy");
            let est = estimate_publish_options_cancellable_with_config(
                sound_proj,
                &rt,
                &options,
                &cfg,
                || false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(folder_totals(&out), (est.files, est.bytes, est.gzip_bytes));
        }
    }

    fs::remove_dir_all(dir).expect("cleanup pass1");
}

#[test]
fn pass2_templates_scale_modes_accelerations_and_hud_permutations() {
    let dir = temp_dir("pass2");
    let rt = dir.join("runtime");
    create_mock_runtime(&rt);

    let project = Project::demo();
    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    // 1. All 4 HtmlTemplate × 4 HtmlScaleMode × 3 HtmlAcceleration combinations
    for template in HtmlTemplate::ALL {
        for scale_mode in HtmlScaleMode::ALL {
            for acceleration in HtmlAcceleration::ALL {
                let cfg = HtmlPublishConfig {
                    template,
                    scale_mode,
                    acceleration,
                    chrome: PlayerChromeConfig::clean_publish(),
                    ..HtmlPublishConfig::default()
                };
                let out = dir.join(format!("tmpl-{template:?}-{scale_mode:?}-{acceleration:?}"));
                export_project_with_config(&project, &out, &rt, &options, &cfg)
                    .expect("export template/scale/accel");
                let html = fs::read_to_string(out.join("index.html")).expect("read html");
                match template {
                    HtmlTemplate::Responsive => {}
                    HtmlTemplate::FixedStage => {
                        assert!(html.contains("#stage-surface { border: 1px solid #ffffff1a; }"));
                    }
                    HtmlTemplate::Centered => {
                        assert!(
                            html.contains(
                                "#stage-surface { border-radius: 6px; overflow: hidden; }"
                            )
                        );
                    }
                    HtmlTemplate::Fullscreen => {
                        assert!(html.contains("#stage-surface { box-shadow: none; }"));
                    }
                }
            }
        }
    }

    // 2. All 32 permutations of the 5 preview HUD toggles
    for mask in 0..32u8 {
        let chrome = PlayerChromeConfig {
            clean: false,
            show_fps: (mask & 1) != 0,
            show_dimensions: (mask & 2) != 0,
            show_controls: (mask & 4) != 0,
            show_load_summary: (mask & 8) != 0,
            show_trace: (mask & 16) != 0,
        };
        let cfg = HtmlPublishConfig {
            chrome,
            preloader: (mask & 1) != 0,
            loop_timeline: (mask & 2) != 0,
            autoplay: (mask & 4) != 0,
            strict_mode: (mask & 8) != 0,
            include_trace: (mask & 16) != 0,
            ..HtmlPublishConfig::default()
        };
        let out = dir.join(format!("hud-mask-{mask}"));
        export_project_with_config(&project, &out, &rt, &options, &cfg)
            .expect("export hud permutation");
        let html = fs::read_to_string(out.join("index.html")).expect("read html");
        if mask == 0 {
            assert!(html.contains("<body data-clean=\"true\">"));
            assert!(html.contains("<footer hidden>"));
        } else {
            assert!(!html.contains("<body data-clean=\"true\">"));
            assert_eq!(
                html.contains("aria-label=\"Preview controls\" hidden"),
                !chrome.show_controls
            );
            assert_eq!(html.contains("id=\"status\" hidden"), !chrome.show_fps);
            assert_eq!(
                html.contains("id=\"load-info\" hidden"),
                !chrome.show_load_summary
            );
            assert_eq!(
                html.contains(
                    "id=\"trace\" role=\"status\" aria-live=\"polite\" aria-atomic=\"true\" hidden"
                ),
                !chrome.show_trace
            );
            assert_eq!(
                html.contains(&format!("{}×{}", project.width, project.height)),
                chrome.show_dimensions
            );
        }
    }

    fs::remove_dir_all(dir).expect("cleanup pass2");
}

#[test]
fn pass3_jpeg_quality_spritesheets_sourcemaps_minify_and_video_deployment_modes() {
    let dir = temp_dir("pass3");
    let rt = dir.join("runtime");
    create_mock_runtime(&rt);

    // 1. Monotonic JPEG quality re-encoding + sprite_sheets + source_maps + minify
    let mut bitmap_proj = Project::demo();
    let jpeg_98 = encode_test_jpeg(98);
    let mut jpeg_res =
        ResourceUnit::from_bytes("hero-photo", "bitmaps/hero.jpg", "image/jpeg", &jpeg_98)
            .expect("jpeg resource");
    jpeg_res.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::PhotoJpeg,
        use_imported_jpeg_data: true,
        jpeg_quality: 80,
        allow_smoothing: true,
    });
    bitmap_proj.resources.push(jpeg_res);
    bitmap_proj.format_version = bitmap_proj.format_version.max(10);

    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };

    let mut prev_reencoded_len = usize::MAX;
    for q in [95u8, 60, 20] {
        let cfg = HtmlPublishConfig {
            jpeg_quality: q,
            sprite_sheets: true,
            source_maps: true,
            minify: false,
            chrome: PlayerChromeConfig::clean_publish(),
            ..HtmlPublishConfig::default()
        };
        let out = dir.join(format!("jpeg-q{q}"));
        export_project_with_config(&bitmap_proj, &out, &rt, &options, &cfg)
            .expect("export bitmap project");
        assert!(out.join("sprite_sheet.json").is_file());
        assert!(out.join("bundle.js.map").is_file());

        let bundle_raw = fs::read_to_string(out.join("bundle.js")).expect("bundle.js");
        assert!(bundle_raw.ends_with("//# sourceMappingURL=bundle.js.map\n"));
        let json_str = bundle_raw
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n//# sourceMappingURL=bundle.js.map\n")
            .unwrap();
        let val: serde_json::Value = serde_json::from_str(json_str).expect("parse bundle json");
        let b64 = val["project"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "hero-photo")
            .unwrap()["data_base64"]
            .as_str()
            .unwrap();
        let decoded = STANDARD.decode(b64).expect("decode jpeg b64");
        assert!(
            decoded.len() < prev_reencoded_len,
            "JPEG quality {q} should produce fewer bytes than higher quality ({} < {})",
            decoded.len(),
            prev_reencoded_len
        );
        prev_reencoded_len = decoded.len();
    }

    // 2. VideoDeploymentMode: Embedded vs External vs Progressive (T-178)
    let mp4_bytes = valid_mp4_header_bytes();
    let make_video_project = |mode: VideoDeploymentMode| -> Project {
        let mut proj = Project::blank();
        proj.format_version = 10;
        let mut res = ResourceUnit::from_bytes("vid-1", "video/intro.mp4", "video/mp4", &mp4_bytes)
            .expect("video resource");
        if mode == VideoDeploymentMode::Progressive {
            res.data_base64 = "".into();
        }
        proj.resources.push(res);
        let root_id = proj.root.clone();
        let root_sym = proj.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 12;
        root_sym.timeline.layers[0].keyframes[0].video = Some(match mode {
            VideoDeploymentMode::Embedded => TimelineVideoSpan::embedded("vid-1"),
            VideoDeploymentMode::External => {
                TimelineVideoSpan::external("vid-1", "media/intro.mp4")
            }
            VideoDeploymentMode::Progressive => {
                TimelineVideoSpan::progressive("vid-1", "https://cdn.example.com/video/intro.mp4")
            }
        });
        assert!(
            neoflash_core::validate(&proj).is_empty(),
            "video project ({mode:?}) must validate cleanly"
        );
        proj
    };

    let proj_embedded = make_video_project(VideoDeploymentMode::Embedded);
    let proj_external = make_video_project(VideoDeploymentMode::External);
    let proj_progressive = make_video_project(VideoDeploymentMode::Progressive);

    let out_emb = dir.join("vid-embedded");
    let out_ext = dir.join("vid-external");
    let out_prog = dir.join("vid-progressive");
    let cfg = HtmlPublishConfig::clean_publish_default();

    export_project_with_config(&proj_embedded, &out_emb, &rt, &options, &cfg)
        .expect("export embedded video");
    export_project_with_config(&proj_external, &out_ext, &rt, &options, &cfg)
        .expect("export external video");
    export_project_with_config(&proj_progressive, &out_prog, &rt, &options, &cfg)
        .expect("export progressive video");

    // Embedded keeps base64 in bundle.js and writes no external mp4 file
    let bundle_emb = fs::read_to_string(out_emb.join("bundle.js")).unwrap();
    assert!(bundle_emb.contains(&STANDARD.encode(&mp4_bytes)));
    assert!(!out_emb.join("media/intro.mp4").exists());

    // External strips base64 from bundle.js and writes media/intro.mp4 on disk!
    let bundle_ext = fs::read_to_string(out_ext.join("bundle.js")).unwrap();
    assert!(
        !bundle_ext.contains(&STANDARD.encode(&mp4_bytes)),
        "External video must not duplicate base64 bytes in bundle.js"
    );
    assert!(
        out_ext.join("media/intro.mp4").is_file(),
        "External video must write managed relative file media/intro.mp4"
    );
    assert_eq!(
        fs::read(out_ext.join("media/intro.mp4")).unwrap(),
        mp4_bytes
    );

    // Progressive strips base64 from bundle.js, writes no local mp4 file, and records HTTPS URL in video_manifest
    let bundle_prog = fs::read_to_string(out_prog.join("bundle.js")).unwrap();
    assert!(!bundle_prog.contains(&STANDARD.encode(&mp4_bytes)));
    assert!(!out_prog.join("media/intro.mp4").exists());
    assert!(bundle_prog.contains("https://cdn.example.com/video/intro.mp4"));

    // Verify size estimates match exact disk bytes for all 3 video deployment modes!
    for (proj, out_folder) in [
        (&proj_embedded, &out_emb),
        (&proj_external, &out_ext),
        (&proj_progressive, &out_prog),
    ] {
        let est =
            estimate_publish_options_cancellable_with_config(proj, &rt, &options, &cfg, || false)
                .unwrap()
                .unwrap();
        assert_eq!(
            folder_totals(out_folder),
            (est.files, est.bytes, est.gzip_bytes)
        );
    }

    fs::remove_dir_all(dir).expect("cleanup pass3");
}

#[test]
fn pass4_all_publish_and_export_targets_desktop_images_spritesheet_svg_lottie_video_swf_and_fla() {
    let dir = temp_dir("pass4");
    let rt = dir.join("runtime");
    create_mock_runtime(&rt);

    let project = Project::demo();
    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };
    let cfg = HtmlPublishConfig::clean_publish_default();

    // 1. Desktop App (.app + Windows .bat)
    let desktop_dir = dir.join("desktop");
    let app_bundle = export_desktop_app(&project, &desktop_dir, &rt, &options, &cfg)
        .expect("export desktop app");
    assert!(app_bundle.join("Contents/Info.plist").is_file());
    assert!(
        app_bundle
            .join("Contents/Resources/web/index.html")
            .is_file()
    );
    assert!(desktop_dir.join("Launch Ideas in motion.bat").is_file());

    // 2. Image Sequence (PNG frames)
    let seq_dir = dir.join("image-seq");
    let mut seq_opts = ImageExportOptions::for_project(&project, 0);
    seq_opts.format = ImageExportFormat::Png;
    seq_opts.range = ImageExportRange::Frames { start: 0, end: 3 };
    seq_opts.width = 320;
    seq_opts.height = 240;
    seq_opts.transparent_background = true;
    let seq_files = export_project_image(&project, &seq_dir.join("frame.png"), &seq_opts)
        .expect("export PNG image sequence");
    assert_eq!(seq_files.files.len(), 4);
    for f in &seq_files.files {
        assert!(f.is_file());
    }

    // 3. Sprite Sheet / Atlas (PNG + JSON)
    let sheet_png = dir.join("sprites/sheet.png");
    let sheet_opts = AtlasOptions {
        start_frame: 0,
        end_frame: Some(3),
        frame_width: Some(160),
        frame_height: Some(120),
        format: AtlasFormat::JsonHash,
        ..AtlasOptions::default()
    };
    let atlas_report =
        export_project_atlas(&project, &sheet_png, &sheet_opts).expect("export sprite sheet");
    assert!(atlas_report.image_path.is_file());
    assert!(atlas_report.metadata_path.is_file());

    // 4. SVG Animation / Sequence & Single-frame SVG
    let svg_single = render_frame_svg(&project, &SvgOptions::default()).expect("render single SVG");
    assert!(svg_single.0.contains("<svg") && svg_single.0.contains("</svg>"));

    let svg_seq_files = export_project_svg(
        &project,
        dir.join("svg-seq/frame.svg"),
        &SvgOptions {
            frame: 0,
            end_frame: Some(2),
            ..SvgOptions::default()
        },
    )
    .expect("export SVG sequence");
    assert_eq!(svg_seq_files.files.len(), 3);

    // 5. Lottie JSON
    let (lottie, lottie_report) =
        export_lottie_json(&project, &LottieOptions::default()).expect("export Lottie JSON");
    assert!(lottie.contains("\"v\":") && lottie.contains("\"layers\":"));
    assert!(lottie_report.width > 0);

    // 6. Video (MP4) estimate
    let vid_est = estimate_video(
        &project,
        &VideoOptions {
            format: VideoFormat::Mp4,
            codec: VideoCodec::H264,
            width: project.width,
            height: project.height,
            fps: project.fps as f32,
            ..VideoOptions::default()
        },
    )
    .expect("estimate video");
    assert!(vid_est.frame_count > 0 && vid_est.estimated_bytes > 0);

    // 7. SWF compiler across profiles (FlashPlayer8Avm1, FlashPlayer9Avm2, FlashPlayer10Avm2) × compressions (FWS, CWS, ZWS)
    for (profile, compression, lang) in [
        (
            SwfPublishProfile::FlashPlayer8Avm1,
            SwfCompression::UncompressedFws,
            ScriptLanguage::ActionScript2,
        ),
        (
            SwfPublishProfile::FlashPlayer8Avm1,
            SwfCompression::DeflateCws,
            ScriptLanguage::ActionScript2,
        ),
        (
            SwfPublishProfile::FlashPlayer9Avm2,
            SwfCompression::DeflateCws,
            ScriptLanguage::ActionScript3,
        ),
        (
            SwfPublishProfile::FlashPlayer10Avm2,
            SwfCompression::LzmaZws,
            ScriptLanguage::ActionScript3,
        ),
    ] {
        let mut swf_proj = Project::blank();
        let root = swf_proj.root.clone();
        let sym = swf_proj.symbol_mut(&root).unwrap();
        sym.timeline.frame_count = 2;
        sym.timeline.layers = vec![Layer {
            id: "layer-1".into(),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "rect-1".into(),
                    name: "box".into(),
                    kind: ElementKind::Rectangle {
                        width: 100.0,
                        height: 60.0,
                        fill: [40, 120, 220, 255],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 2.0,
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }];
        sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: lang,
                source: "trace(\"swf-ok\");\nstop();".into(),
            },
        });
        let swf_path = dir.join(format!("movie-{profile:?}-{compression:?}.swf"));
        let mut swf_opts = SwfPublishOptions::for_profile(profile);
        swf_opts.compression = compression;
        let report =
            publish_swf_to_path(&swf_proj, &swf_path, &swf_opts).expect("compile and export SWF");
        assert!(report.can_proceed());
        let swf_bytes = fs::read(&swf_path).expect("read swf bytes");
        let reimported = import_swf_bytes(&swf_bytes).expect("reimport swf");
        assert_eq!(reimported.width, swf_proj.width);
    }

    // 8. All 7 FlaTargetProfile targets (Animate2024, FlashCS6, FlashCS55, FlashCS5, FlashCS4, Flash8, FlashMX2004)
    for profile in FlaTargetProfile::ALL {
        let mut fla_proj = Project::blank();
        let root = fla_proj.root.clone();
        fla_proj.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "shape-1".into(),
                name: "card".into(),
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 50.0,
                    fill: [240, 90, 40, 255],
                    stroke: [0, 0, 0, 255],
                    stroke_width: 1.0,
                },
                ..Default::default()
            });
        let fla_path = dir.join(format!("export-{profile:?}.fla"));
        let report =
            export_fla_with_profile(&fla_proj, profile, &FlaExportOptions::default(), &fla_path)
                .expect("export FLA profile");
        assert!(report.can_proceed());
        if fla_path.is_dir() {
            assert!(fla_path.join("DOMDocument.xml").is_file());
        } else {
            let bytes = fs::read(&fla_path).expect("read fla bytes");
            let reimported = import_fla_bytes(&bytes).expect("reimport exported FLA bytes");
            assert_eq!(reimported.width, fla_proj.width);
        }
    }

    fs::remove_dir_all(dir).expect("cleanup pass4");
}

#[test]
fn pass5_sequential_overwrite_and_stale_file_cleanup_stress() {
    let dir = temp_dir("pass5");
    let rt = dir.join("runtime");
    create_mock_runtime(&rt);

    let out = dir.join("reused-publish-folder");
    let mp4_bytes = valid_mp4_header_bytes();

    // Step 1: Publish Balanced + source_maps + sprite_sheets + External video
    let mut proj = Project::demo();
    proj.format_version = 10;
    let jpeg_bytes = encode_test_jpeg(85);
    proj.resources
        .push(ResourceUnit::from_bytes("bmp-1", "img/pic.jpg", "image/jpeg", &jpeg_bytes).unwrap());
    proj.resources.push(
        ResourceUnit::from_bytes("vid-1", "media/clip.mp4", "video/mp4", &mp4_bytes).unwrap(),
    );
    let root = proj.root.clone();
    proj.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0].video =
        Some(TimelineVideoSpan::external("vid-1", "media/clip.mp4"));

    let opts_balanced = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Balanced,
        audio: AudioExportPolicy::Original,
    };
    let cfg_rich = HtmlPublishConfig {
        source_maps: true,
        sprite_sheets: true,
        include_html: true,
        chrome: PlayerChromeConfig::clean_publish(),
        ..HtmlPublishConfig::default()
    };
    export_project_with_config(&proj, &out, &rt, &opts_balanced, &cfg_rich).expect("step 1 export");
    assert!(out.join("neoflash_web_bg.wasm").is_file());
    assert!(out.join("bundle.js.map").is_file());
    assert!(out.join("sprite_sheet.json").is_file());
    assert!(out.join("media/clip.mp4").is_file());
    assert!(out.join("index.html").is_file());

    // Step 2: Re-publish into the SAME folder with Full + no source_maps + no sprite_sheets + no External video + include_html: false
    let proj_step2 = Project::demo();
    let opts_full = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };
    let cfg_minimal = HtmlPublishConfig {
        source_maps: false,
        sprite_sheets: false,
        include_html: false,
        chrome: PlayerChromeConfig::clean_publish(),
        ..HtmlPublishConfig::default()
    };
    export_project_with_config(&proj_step2, &out, &rt, &opts_full, &cfg_minimal)
        .expect("step 2 overwrite export");

    // Verify all stale files from Step 1 were cleaned up!
    assert!(
        !out.join("neoflash_web_bg.wasm").exists(),
        "stale .wasm must be removed when switching Balanced -> Full"
    );
    assert!(
        !out.join("bundle.js.map").exists(),
        "stale bundle.js.map must be removed when source_maps is turned off"
    );
    assert!(
        !out.join("sprite_sheet.json").exists(),
        "stale sprite_sheet.json must be removed when sprite_sheets is turned off"
    );
    assert!(
        !out.join("media/clip.mp4").exists(),
        "stale external video must be removed when no longer present"
    );
    assert!(
        !out.join("index.html").exists(),
        "stale index.html must be removed when include_html is false"
    );

    // Step 3: Re-publish into the SAME folder with Shared runtime + include_html: true
    let opts_shared = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Shared("https://cdn.example.com/rt".into()),
        audio: AudioExportPolicy::Original,
    };
    let cfg_clean = HtmlPublishConfig::clean_publish_default();
    export_project_with_config(&proj_step2, &out, &rt, &opts_shared, &cfg_clean)
        .expect("step 3 shared overwrite export");
    assert!(out.join("index.html").is_file());
    assert!(!out.join("runtime.js").exists());
    assert!(!out.join("neoflash_web.js").exists());

    // Step 4: Re-publish into the SAME folder with Compact + Clean
    let opts_compact = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };
    export_project_with_config(&proj_step2, &out, &rt, &opts_compact, &cfg_clean)
        .expect("step 4 compact overwrite export");
    assert!(out.join("index.html").is_file());
    assert!(out.join("runtime.js").is_file());
    assert!(out.join("neoflash_web.js").is_file());
    assert!(out.join("neoflash_web_bg.wasm").is_file());

    // Verify no temporary .neoflash-*.tmp files leaked anywhere
    let leaked_tmps: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".neoflash-"))
        .collect();
    assert!(leaked_tmps.is_empty(), "leaked temp files: {leaked_tmps:?}");

    fs::remove_dir_all(dir).expect("cleanup pass5");
}
