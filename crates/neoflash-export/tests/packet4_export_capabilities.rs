use image::{ImageFormat, Rgba, RgbaImage};
use neoflash_core::{
    BitmapCompression, BitmapExportSettings, Element, ElementKind, FrameScript, FrameSound,
    InstancePlayback, Player, Project, ResourceUnit, Script, ScriptLanguage, SymbolKind, Transform,
    new_id,
};
use neoflash_export::{
    AtlasFormat, AtlasImageFormat, AtlasOptions, AudioExportPolicy, BannerOptions,
    FontExportPolicy, HtmlAcceleration, HtmlPublishConfig, ImageExportFormat, ImageExportOptions,
    ImageExportRange, PublishOptions, SequencePublishPolicy, SizeLevel, estimate_native_banner,
    estimate_publish_options_cancellable_with_config, export_native_banner_html,
    export_project_atlas, export_project_image, export_project_with_config, gzip_len,
};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn make_rgba_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x, y, Rgba(color));
        }
    }
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)
        .expect("encode png");
    buf.into_inner()
}

fn make_silent_wav(samples: usize) -> Vec<u8> {
    let sample_rate = 22050u32;
    let data_bytes = (samples * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..samples {
        let s = ((i as f32 * 0.1).sin() * 4000.0) as i16;
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "neoflash-p4-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn dummy_runtime(dir: &std::path::Path) -> PathBuf {
    let rt = dir.join("runtime");
    fs::create_dir_all(&rt).unwrap();
    fs::write(rt.join("neoflash_web.js"), "var NeoFlashWasm = {};\n").unwrap();
    fs::write(rt.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    rt
}

#[test]
fn per_bitmap_settings_dedup_sprite_sheets_and_assets_folder_round_trip() {
    let dir = temp_dir("bitmap-sheets");
    let rt = dummy_runtime(&dir);

    let mut project = Project::blank();
    project.width = 200;
    project.height = 120;

    // Two identical PNGs with WebP override (should deduplicate encoded payload) + one PhotoJpeg + audio WAV
    let red_png = make_rgba_png(24, 24, [230, 40, 50, 255]);
    let blue_png = make_rgba_png(24, 24, [30, 120, 240, 180]);

    let mut res_webp_a =
        ResourceUnit::from_bytes("bmp_webp_a", "images/a.png", "image/png", &red_png).unwrap();
    res_webp_a.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::Webp,
        use_imported_jpeg_data: true,
        jpeg_quality: 80,
        allow_smoothing: false,
    });
    let mut res_webp_b =
        ResourceUnit::from_bytes("bmp_webp_b", "images/b.png", "image/png", &red_png).unwrap();
    res_webp_b.bitmap_export = res_webp_a.bitmap_export;

    let mut res_jpeg =
        ResourceUnit::from_bytes("bmp_jpeg", "images/c.png", "image/png", &blue_png).unwrap();
    res_jpeg.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::PhotoJpeg,
        use_imported_jpeg_data: false,
        jpeg_quality: 65,
        allow_smoothing: true,
    });

    let wav_bytes = make_silent_wav(1100);
    let res_audio =
        ResourceUnit::from_bytes("snd_chime", "audio/chime.wav", "audio/wav", &wav_bytes).unwrap();

    project
        .resources
        .extend([res_webp_a, res_webp_b, res_jpeg, res_audio]);
    project.symbols[0].timeline.layers[0].keyframes[0].sound = Some(FrameSound::new("snd_chime"));
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: new_id("el1"),
            name: "A".into(),
            transform: Transform {
                x: 10.0,
                y: 10.0,
                ..Transform::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "bmp_webp_a".into(),
                width: 24,
                height: 24,
                smoothing: true,
            },
            ..Element::default()
        },
        Element {
            id: new_id("el2"),
            name: "B".into(),
            transform: Transform {
                x: 50.0,
                y: 10.0,
                ..Transform::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "bmp_webp_b".into(),
                width: 24,
                height: 24,
                smoothing: true,
            },
            ..Element::default()
        },
        Element {
            id: new_id("el3"),
            name: "C".into(),
            transform: Transform {
                x: 90.0,
                y: 10.0,
                ..Transform::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "bmp_jpeg".into(),
                width: 24,
                height: 24,
                smoothing: false,
            },
            ..Element::default()
        },
    ];
    project.upgrade_format_for_features();

    // 1. Publish without sprite_sheets first to verify per-bitmap WebP + JPEG overrides and allow_smoothing
    let out_direct = dir.join("out-direct");
    let opts = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Balanced,
        audio: AudioExportPolicy::Original,
    };
    let mut cfg = HtmlPublishConfig::clean_publish_default();
    cfg.jpeg_quality = 85;
    export_project_with_config(&project, &out_direct, &rt, &opts, &cfg).expect("direct publish");
    let bundle_direct = fs::read_to_string(out_direct.join("bundle.js")).unwrap();
    assert!(
        bundle_direct.contains("image/webp"),
        "personal WebP override must re-encode PNG resource to image/webp"
    );
    assert!(
        bundle_direct.contains("image/jpeg"),
        "PhotoJpeg override must re-encode PNG resource to image/jpeg"
    );

    // 2. Publish with sprite_sheets = true and assets_folder = true
    let out_sheet = dir.join("out-sheet");
    cfg.sprite_sheets = true;
    cfg.assets_folder = true;
    cfg.acceleration = HtmlAcceleration::Canvas2d;
    export_project_with_config(&project, &out_sheet, &rt, &opts, &cfg).expect("sheet publish");

    assert!(out_sheet.join("assets/sprite_sheet.png").is_file());
    assert!(out_sheet.join("assets/sprite_sheet.json").is_file());

    // Exact disk byte accounting check
    let est =
        estimate_publish_options_cancellable_with_config(&project, &rt, &opts, &cfg, || false)
            .unwrap()
            .unwrap();
    let mut disk_files = 0usize;
    let mut disk_bytes = 0usize;
    let mut disk_gzip = 0usize;
    fn walk_files(dir: &std::path::Path, files: &mut usize, bytes: &mut usize, gzip: &mut usize) {
        for entry in fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk_files(&p, files, bytes, gzip);
            } else {
                let data = fs::read(&p).unwrap();
                *files += 1;
                *bytes += data.len();
                *gzip += gzip_len(&data);
            }
        }
    }
    walk_files(&out_sheet, &mut disk_files, &mut disk_bytes, &mut disk_gzip);
    assert_eq!(est.files, disk_files);
    assert_eq!(est.bytes, disk_bytes);
    assert_eq!(est.gzip_bytes, disk_gzip);

    let html = fs::read_to_string(out_sheet.join("index.html")).unwrap();
    assert!(html.contains("data-acceleration=\"canvas2d\""));

    let bundle_sheet = fs::read_to_string(out_sheet.join("bundle.js")).unwrap();
    assert!(bundle_sheet.contains("\"sprite_sheet\":{\"image_url\":\"assets/sprite_sheet.png\""));
    assert!(bundle_sheet.contains("\"asset_urls\":{\"audio/chime.wav\":\"assets/chime-"));

    // Extract project JSON from bundle.js and verify Project::from_published_bundle_json + Player::hydrate_sprite_sheet
    let prefix = "globalThis.NeoFlashBundle = ";
    let end = bundle_sheet.rfind(';').unwrap();
    let bundle_val: serde_json::Value =
        serde_json::from_str(&bundle_sheet[prefix.len()..end]).expect("parse bundle json");
    let pub_proj_json = serde_json::to_string(&bundle_val["project"]).unwrap();
    let pub_proj =
        Project::from_published_bundle_json(&pub_proj_json).expect("published project loads");
    let mut player = Player::new(&pub_proj);
    let sheet_png = fs::read(out_sheet.join("assets/sprite_sheet.png")).unwrap();
    let sheet_json = fs::read_to_string(out_sheet.join("assets/sprite_sheet.json")).unwrap();
    let hydrated = player
        .hydrate_sprite_sheet(&sheet_png, &sheet_json)
        .expect("hydrate sprite sheet");
    assert_eq!(hydrated, 3);
    let px_webp = player
        .as2_bitmap_load(&pub_proj, "bmp_webp_a")
        .expect("load hydrated webp bitmap")
        .expect("pixel ref a");
    let px_jpeg = player
        .as2_bitmap_load(&pub_proj, "bmp_jpeg")
        .expect("load hydrated jpeg bitmap")
        .expect("pixel ref c");
    assert_ne!(px_webp, px_jpeg);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn texture_atlas_formats_extrude_and_multipage_work() {
    let dir = temp_dir("atlas");
    let mut project = Project::demo();
    project.symbols[0].timeline.frame_count = 4;

    for fmt in [
        AtlasFormat::JsonHash,
        AtlasFormat::JsonArray,
        AtlasFormat::StarlingXml,
        AtlasFormat::EaselJs,
        AtlasFormat::Css,
    ] {
        let opts = AtlasOptions {
            format: fmt,
            image_format: AtlasImageFormat::Png,
            extrude: 2,
            trim: true,
            max_width: 2048,
            max_height: 2048,
            ..AtlasOptions::default()
        };
        let meta_path = dir.join(format!("sheet_{fmt:?}.{}", fmt.extension()));
        let report = export_project_atlas(&project, &meta_path, &opts).expect("export atlas");
        assert!(report.image_path.is_file());
        assert!(report.metadata_path.is_file());
        let text = fs::read_to_string(&report.metadata_path).unwrap();
        match fmt {
            AtlasFormat::EaselJs => {
                assert!(text.contains("\"animations\"") && text.contains("\"frames\""))
            }
            AtlasFormat::Css => {
                assert!(text.contains("background-image:") && text.contains("background-position:"))
            }
            AtlasFormat::StarlingXml => {
                assert!(text.contains("<TextureAtlas") && text.contains("<SubTexture"))
            }
            AtlasFormat::JsonHash | AtlasFormat::JsonArray => {
                assert!(text.contains("\"frames\"") && text.contains("\"meta\""))
            }
        }
    }

    // Multi-page overflow packing + WebP sheet encoding
    let multi_opts = AtlasOptions {
        format: AtlasFormat::JsonHash,
        image_format: AtlasImageFormat::Webp,
        trim: false,
        max_width: project.width + 16,
        max_height: project.height + 16,
        multi_page: true,
        deduplicate: false,
        ..AtlasOptions::default()
    };
    let multi_report = export_project_atlas(&project, dir.join("multi.json"), &multi_opts)
        .expect("multi-page atlas");
    assert!(
        multi_report.page_paths.len() >= 2,
        "tight max dimensions with deduplicate=false must spill onto multiple atlas pages"
    );
    for p in &multi_report.page_paths {
        assert!(p.is_file());
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn sequence_export_covers_ai_svg_png_jpg_webp_gif_bmp_on_scripted_timeline() {
    let dir = temp_dir("sequences");
    let mut project = Project::blank();
    project.width = 160;
    project.height = 100;
    project.fps = 24;
    project.symbols[0].timeline.frame_count = 3;
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();\nthis.onEnterFrame = function() { box._x = box._x + 30; };".into(),
        },
    });
    // Create a child MovieClip symbol `box` so `box._x` moves via live script even while root is stop()-halted!
    let mut box_sym = Project::blank().symbols.remove(0);
    box_sym.id = "sym_box".into();
    box_sym.name = "Box".into();
    box_sym.kind = SymbolKind::MovieClip;
    box_sym.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: new_id("rect"),
            name: "Shape".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 30.0,
                height: 30.0,
                fill: [250, 90, 30, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    project.symbols.push(box_sym);
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: new_id("inst"),
            name: "box".into(),
            transform: Transform {
                x: 10.0,
                y: 30.0,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym_box".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });

    for fmt in ImageExportFormat::SEQUENCE_FORMATS {
        let seq_dir = dir.join(fmt.extension());
        fs::create_dir_all(&seq_dir).unwrap();
        let mut opts = ImageExportOptions::for_project(&project, 0);
        opts.format = fmt;
        opts.range = ImageExportRange::WholeMovie;
        opts.symbol_id = None;
        opts.runtime_capture = true;
        opts.jpeg_quality = 90;
        let out_file = seq_dir.join(format!("frame.{}", fmt.extension()));
        let report = export_project_image(&project, &out_file, &opts)
            .unwrap_or_else(|e| panic!("sequence export failed for {fmt:?}: {e}"));
        assert_eq!(report.files.len(), 3);
        let f0 = fs::read(&report.files[0]).unwrap();
        let f2 = fs::read(&report.files[2]).unwrap();
        assert!(!f0.is_empty() && !f2.is_empty());
        assert_ne!(
            f0, f2,
            "scripted box._x += 30 must produce distinct frame 1 vs frame 3 output for {fmt:?}"
        );
        if fmt == ImageExportFormat::Ai {
            assert!(
                f0.starts_with(b"%PDF-") || f0.windows(12).any(|w| w == b"%!PS-Adobe-3"),
                "Adobe Illustrator sequence frame must carry valid PDF/AI vector header"
            );
        }
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn sequence_import_placements_and_publish_policies_work() {
    let dir = temp_dir("seq-policy");
    let rt = dummy_runtime(&dir);

    let frames_dir = dir.join("frames");
    fs::create_dir_all(&frames_dir).unwrap();
    let colors = [[240, 40, 40, 255], [40, 220, 80, 255], [40, 90, 240, 255]];
    let mut paths = Vec::new();
    for (idx, c) in colors.iter().enumerate() {
        let p = frames_dir.join(format!("walk_{:03}.png", idx + 1));
        fs::write(&p, make_rgba_png(32, 32, *c)).unwrap();
        paths.push(p);
    }

    // Verify all 4 sequence import placements
    for placement in [
        neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer,
        neoflash_import::ImageSequencePlacement::ConvertToGraphicSymbol,
        neoflash_import::ImageSequencePlacement::ConvertToMovieClip,
        neoflash_import::ImageSequencePlacement::SeparateLayers,
    ] {
        let seq_opts = neoflash_import::ImageSequenceImportOptions {
            placement,
            frame_step: 2,
            ..neoflash_import::ImageSequenceImportOptions::default()
        };
        let imported = neoflash_import::import_numbered_image_sequence_paths_with_options(
            &paths, false, &seq_opts,
        )
        .expect("sequence import ok")
        .expect("detected sequence");
        imported
            .project
            .validate()
            .expect("valid imported sequence project");
        match placement {
            neoflash_import::ImageSequencePlacement::ConvertToMovieClip => {
                assert!(imported.project.symbols.iter().any(|s| {
                    s.id != imported.project.root && s.kind == neoflash_core::SymbolKind::MovieClip
                }));
            }
            neoflash_import::ImageSequencePlacement::SeparateLayers => {
                assert_eq!(imported.project.symbols[0].timeline.layers.len(), 3);
            }
            _ => {}
        }
    }

    // Build a single-layer sequence project and test PackAtlas and TranscodeVideo publish policies
    let seq_proj = neoflash_import::import_numbered_image_sequence_paths_with_options(
        &paths,
        false,
        &neoflash_import::ImageSequenceImportOptions {
            placement: neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer,
            frame_step: 1,
            ..neoflash_import::ImageSequenceImportOptions::default()
        },
    )
    .unwrap()
    .unwrap()
    .project;

    let pub_opts = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Balanced,
        audio: AudioExportPolicy::Original,
    };

    let mut cfg_atlas = HtmlPublishConfig::clean_publish_default();
    cfg_atlas.sequence_policy = SequencePublishPolicy::PackAtlas;
    let out_atlas = dir.join("pub-atlas");
    export_project_with_config(&seq_proj, &out_atlas, &rt, &pub_opts, &cfg_atlas)
        .expect("PackAtlas publish");
    assert!(
        out_atlas.join("sprite_sheet.png").is_file(),
        "SequencePublishPolicy::PackAtlas must pack sequence frames into sprite_sheet.png"
    );

    let mut cfg_vid = HtmlPublishConfig::clean_publish_default();
    cfg_vid.sequence_policy = SequencePublishPolicy::TranscodeVideo;
    let out_vid = dir.join("pub-vid");
    export_project_with_config(&seq_proj, &out_vid, &rt, &pub_opts, &cfg_vid)
        .expect("TranscodeVideo publish");
    assert!(out_vid.join("index.html").is_file());

    // Also verify BannerOptions budget enforcement
    let banner_proj = Project::blank();
    let tight_banner = BannerOptions {
        max_gzip_bytes: Some(10),
        ..BannerOptions::default()
    };
    let est = estimate_native_banner(&banner_proj, &tight_banner).unwrap();
    assert!(!est.within_budget);
    assert!(export_native_banner_html(&banner_proj, dir.join("fail.html"), &tight_banner).is_err());

    let _ = fs::remove_dir_all(dir);
}
