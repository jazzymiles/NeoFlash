mod packet7_support;

use neoflash_core::{Project, flatten_scenes_with_ranges};
use neoflash_export::{
    AepOptions, AtlasFormat, AtlasImageFormat, AtlasOptions, AudioExportPolicy, FontExportPolicy,
    HtmlPublishConfig, ImageExportFormat, ImageExportOptions, ImageExportRange, PublishOptions,
    SizeLevel, SwfCompression, SwfPublishOptions, SwfPublishProfile, VideoOptions,
    export_project_aep, export_project_atlas, export_project_image, export_project_video,
    export_project_with_config, export_project_with_options, publish_swf_bytes,
};
use packet7_support::{as2_suite, as3_suite, express5, express6};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const AS2_REL_PATHS: [&str; 9] = [
    "examples/as2/00-child-module.nflash",
    "examples/as2/01-math-geometry.nflash",
    "examples/as2/02-oop-xml-collections.nflash",
    "examples/as2/03-movieclips-nested-timelines.nflash",
    "examples/as2/04-events-stage-align-drag.nflash",
    "examples/as2/05-drawing-api-filters-bitmapdata.nflash",
    "examples/as2/06-dynamic-input-html-text.nflash",
    "examples/as2/07-sound-video-loadpublish.nflash",
    "examples/as2/08-interactive-arcade-miniapp.nflash",
];

const AS3_REL_PATHS: [&str; 9] = [
    "examples/as3/00-child-module.nflash",
    "examples/as3/01-math-geometry.nflash",
    "examples/as3/02-oop-xml-collections.nflash",
    "examples/as3/03-movieclips-nested-timelines.nflash",
    "examples/as3/04-events-stage-align-drag.nflash",
    "examples/as3/05-drawing-api-filters-bitmapdata.nflash",
    "examples/as3/06-dynamic-input-html-text.nflash",
    "examples/as3/07-sound-video-loadpublish.nflash",
    "examples/as3/08-interactive-arcade-miniapp.nflash",
];

const EXPRESS5_REL_PATH: &str = "examples/midnight-express-5.nflash";
const EXPRESS6_REL_PATH: &str = "examples/midnight-express-6.nflash";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn temp_dir(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "neoflash-p7-{label}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

fn mock_wasm_runtime(dir: &Path) -> PathBuf {
    let rt = dir.join("mock-runtime");
    fs::create_dir_all(&rt).expect("create mock runtime");
    fs::write(rt.join("neoflash_web.js"), "var NeoFlashWasm = {};\n").expect("write mock js");
    fs::write(rt.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").expect("write mock wasm");
    rt
}

fn write_project_pretty(path: &Path, project: &Project) {
    project
        .validate()
        .unwrap_or_else(|e| panic!("generated {} failed validate(): {e}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create example parent dir");
    }
    let mut json = serde_json::to_string_pretty(project).expect("serialize project JSON");
    json.push('\n');
    fs::write(path, json).expect("write example .nflash");
}

fn generate_and_write_suites(root: &Path, runtime_dir: &Path, scratch: &Path) {
    let publish_opts = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    // 1. Build & compile AS2 00-child-module so 07 embeds real compiled bundle.js & index.html
    let as2_00 = as2_suite::build_as2_00_child_module();
    let as2_00_out = scratch.join("as2-00-build");
    export_project_with_options(&as2_00, &as2_00_out, runtime_dir, &publish_opts)
        .expect("compile as2/00-child-module");
    let as2_child_bundle = fs::read(as2_00_out.join("bundle.js")).expect("read as2 child bundle");
    let as2_child_html = fs::read(as2_00_out.join("index.html")).expect("read as2 child html");

    let as2_projects = [
        as2_00,
        as2_suite::build_as2_01_math_geometry(),
        as2_suite::build_as2_02_oop_xml_collections(),
        as2_suite::build_as2_03_movieclips_nested_timelines(),
        as2_suite::build_as2_04_events_stage_align_drag(),
        as2_suite::build_as2_05_drawing_api_filters_bitmapdata(),
        as2_suite::build_as2_06_dynamic_input_html_text(),
        as2_suite::build_as2_07_sound_video_loadpublish(&as2_child_bundle, &as2_child_html),
        as2_suite::build_as2_08_interactive_arcade_miniapp(),
    ];
    for (rel, proj) in AS2_REL_PATHS.iter().zip(as2_projects.iter()) {
        write_project_pretty(&root.join(rel), proj);
    }

    // 2. Build & compile AS3 00-child-module so 07 embeds real compiled bundle.js & index.html
    let as3_00 = as3_suite::build_as3_00_child_module();
    let as3_00_out = scratch.join("as3-00-build");
    export_project_with_options(&as3_00, &as3_00_out, runtime_dir, &publish_opts)
        .expect("compile as3/00-child-module");
    let as3_child_bundle = fs::read(as3_00_out.join("bundle.js")).expect("read as3 child bundle");
    let as3_child_html = fs::read(as3_00_out.join("index.html")).expect("read as3 child html");

    let as3_projects = [
        as3_00,
        as3_suite::build_as3_01_math_geometry(),
        as3_suite::build_as3_02_oop_xml_collections(),
        as3_suite::build_as3_03_movieclips_nested_timelines(),
        as3_suite::build_as3_04_events_stage_align_drag(),
        as3_suite::build_as3_05_drawing_api_filters_bitmapdata(),
        as3_suite::build_as3_06_dynamic_input_html_text(),
        as3_suite::build_as3_07_sound_video_loadpublish(&as3_child_bundle, &as3_child_html),
        as3_suite::build_as3_08_interactive_arcade_miniapp(),
    ];
    for (rel, proj) in AS3_REL_PATHS.iter().zip(as3_projects.iter()) {
        write_project_pretty(&root.join(rel), proj);
    }
}

#[test]
fn as2_and_as3_example_suites_validate_compile_and_publish_cleanly() {
    let root = workspace_root();
    let scratch = temp_dir("as-suites");
    let real_rt = root.join("target/web-runtime");
    let use_real_rt = std::env::var_os("NEOFLASH_VERIFY_PUBLISH_DIR").is_some()
        && real_rt.join("neoflash_web_bg.wasm").exists();
    let runtime_dir = if use_real_rt {
        real_rt
    } else {
        mock_wasm_runtime(&scratch)
    };

    if std::env::var("NEOFLASH_WRITE_EXAMPLES").as_deref() == Ok("1") {
        generate_and_write_suites(&root, &runtime_dir, &scratch);
    }

    let publish_root = std::env::var("NEOFLASH_VERIFY_PUBLISH_DIR")
        .map(|p| {
            let pb = PathBuf::from(p);
            if pb.is_absolute() { pb } else { root.join(pb) }
        })
        .unwrap_or_else(|_| scratch.join("published"));

    let publish_opts = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };
    let config = HtmlPublishConfig::default();

    for (suite_tag, paths) in [("as2", &AS2_REL_PATHS[..]), ("as3", &AS3_REL_PATHS[..])] {
        for rel_path in paths {
            let full_path = root.join(rel_path);
            let raw = fs::read_to_string(&full_path)
                .unwrap_or_else(|e| panic!("missing {rel_path}: {e}"));
            let project: Project = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("failed to parse {rel_path}: {e}"));

            project
                .validate()
                .unwrap_or_else(|e| panic!("{rel_path} failed validate(): {e}"));

            let program = neoflash_script::check_project(&project).unwrap_or_else(|diags| {
                let joined: Vec<String> = diags.iter().map(ToString::to_string).collect();
                panic!("{rel_path} failed check_project:\n{}", joined.join("\n"));
            });
            assert!(
                !program.code.is_empty() || !program.class_names.is_empty(),
                "{rel_path} produced empty script program"
            );

            let warnings = neoflash_script::check_project_warnings(&project);
            let unsupported: Vec<_> = warnings
                .iter()
                .filter(|w| w.code == "unsupported-feature")
                .collect();
            assert!(
                unsupported.is_empty(),
                "{rel_path} emitted unsupported-feature warnings: {unsupported:?}"
            );

            let stem = Path::new(rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("stem");
            let out_dir = publish_root.join(suite_tag).join(stem);
            let _ = fs::remove_dir_all(&out_dir);
            let index_html = export_project_with_config(
                &project,
                &out_dir,
                &runtime_dir,
                &publish_opts,
                &config,
            )
            .unwrap_or_else(|e| panic!("{rel_path} failed HTML5 publish: {e}"));

            assert!(index_html.exists(), "{rel_path} missing index.html");
            assert!(
                out_dir.join("bundle.js").exists(),
                "{rel_path} missing bundle.js"
            );
            assert!(
                out_dir.join("runtime.js").exists(),
                "{rel_path} missing runtime.js"
            );
        }
    }

    if std::env::var_os("NEOFLASH_VERIFY_PUBLISH_DIR").is_none() {
        let _ = fs::remove_dir_all(&scratch);
    }
}

fn verify_midnight_express_episode(
    episode_num: u32,
    rel_path: &str,
    slug: &str,
    locomotive_symbol_id: &str,
    full_media_env: &str,
    builder: fn() -> Project,
) {
    let root = workspace_root();
    let scratch = temp_dir(&format!("express{episode_num}"));
    let express_path = root.join(rel_path);

    if std::env::var("NEOFLASH_WRITE_EXAMPLES").as_deref() == Ok("1") {
        let generated = builder();
        write_project_pretty(&express_path, &generated);
    }

    let raw =
        fs::read_to_string(&express_path).unwrap_or_else(|e| panic!("missing {rel_path}: {e}"));
    let project: Project =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {rel_path}: {e}"));

    project
        .validate()
        .unwrap_or_else(|e| panic!("Midnight Express {episode_num} project.validate(): {e}"));
    assert_eq!(
        project.scenes.len(),
        3,
        "Episode {episode_num} must have 3 scenes"
    );
    assert_eq!(project.width, 960);
    assert_eq!(project.height, 540);
    assert_eq!(project.fps, 24);

    let (flattened, ranges) = flatten_scenes_with_ranges(&project);
    flattened
        .validate()
        .unwrap_or_else(|e| panic!("flattened Episode {episode_num} validate(): {e}"));
    assert_eq!(ranges.len(), 3);
    let root_sym = flattened.symbol(&flattened.root).expect("flattened root");
    assert_eq!(root_sym.timeline.frame_count, 180);

    // 1. HTML5 export
    let real_rt = root.join("target/web-runtime");
    let use_real_rt = std::env::var_os("NEOFLASH_VERIFY_PUBLISH_DIR").is_some()
        && real_rt.join("neoflash_web_bg.wasm").exists();
    let runtime_dir = if use_real_rt {
        real_rt
    } else {
        mock_wasm_runtime(&scratch)
    };
    let html_dir = std::env::var("NEOFLASH_VERIFY_PUBLISH_DIR")
        .map(|p| {
            let pb = PathBuf::from(p);
            (if pb.is_absolute() { pb } else { root.join(pb) }).join(slug)
        })
        .unwrap_or_else(|_| scratch.join("html5"));
    let _ = fs::remove_dir_all(&html_dir);
    let html_index = export_project_with_config(
        &project,
        &html_dir,
        &runtime_dir,
        &PublishOptions::default(),
        &HtmlPublishConfig::default(),
    )
    .unwrap_or_else(|e| panic!("export Midnight Express {episode_num} HTML5: {e}"));
    assert!(html_index.exists());

    // 2. Multi-format frame sequence exports (.svg, .ai, .png across Scene 2 guided viaduct)
    for (fmt, ext) in [
        (ImageExportFormat::Svg, "svg"),
        (ImageExportFormat::Ai, "ai"),
        (ImageExportFormat::Png, "png"),
    ] {
        let seq_dir = scratch.join(format!("seq-{ext}"));
        fs::create_dir_all(&seq_dir).unwrap();
        let mut img_opts = ImageExportOptions::for_project(&flattened, 75);
        img_opts.format = fmt;
        img_opts.range = ImageExportRange::Frames { start: 74, end: 76 };
        let report = export_project_image(
            &flattened,
            &seq_dir.join(format!("express{episode_num}.{ext}")),
            &img_opts,
        )
        .unwrap_or_else(|e| panic!("sequence export {ext} failed: {e}"));
        assert_eq!(
            report.files.len(),
            3,
            "{ext} sequence should write 3 frames"
        );
        for f in &report.files {
            assert!(fs::metadata(f).unwrap().len() > 128);
        }
    }

    // 3. Texture Atlas export for locomotive symbol
    let atlas_dir = scratch.join("atlas");
    fs::create_dir_all(&atlas_dir).unwrap();
    let atlas_opts = AtlasOptions {
        symbol_id: Some(locomotive_symbol_id.into()),
        format: AtlasFormat::StarlingXml,
        image_format: AtlasImageFormat::Png,
        ..AtlasOptions::default()
    };
    let atlas_report =
        export_project_atlas(&project, atlas_dir.join("locomotive.xml"), &atlas_opts)
            .expect("export locomotive Starling XML texture atlas");
    assert!(atlas_report.image_path.exists());
    assert!(atlas_report.metadata_path.exists());

    // 4. SWF publish (baking Motion/Shape/Guided tweens + outlined vector text)
    let swf_opts = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer10Avm2,
        compression: SwfCompression::DeflateCws,
        bake_unsupported_tweens: true,
        expand_variable_width_strokes: true,
        outline_text: true,
        scriptless_mode: false,
    };
    let (swf_bytes, swf_report) =
        publish_swf_bytes(&project, &swf_opts).expect("publish Midnight Express SWF");
    assert!(swf_report.can_proceed());
    assert_eq!(&swf_bytes[0..3], b"CWS");
    assert!(swf_bytes.len() > 512);

    // 5. Adobe After Effects (.aep + .aepx + .jsx + Footage) export
    let aep_dir = scratch.join("aep");
    fs::create_dir_all(&aep_dir).unwrap();
    let aep_opts = AepOptions {
        shape_morph_step: 4,
        ..AepOptions::default()
    };
    let aep_report = export_project_aep(&project, aep_dir.join(format!("{slug}.aep")), &aep_opts)
        .expect("export Midnight Express AEP");
    assert!(aep_report.aep_path.exists());
    assert!(aep_report.jsx_path.as_ref().is_some_and(|p| p.exists()));
    assert_eq!(
        aep_report.footage_files.len(),
        4,
        "1 crest PNG + 3 stereo WAVs"
    );

    // 6. MP4 video export (fast 6-frame sample in default suite; full 180-frame render on opt-in)
    let full_media = std::env::var(full_media_env).as_deref() == Ok("1");
    let mp4_path = if full_media {
        html_dir
            .parent()
            .unwrap_or(&scratch)
            .join(format!("{slug}.mp4"))
    } else {
        scratch.join(format!("{slug}-sample.mp4"))
    };
    let vid_opts = VideoOptions {
        from_frame: if full_media { 1 } else { 72 },
        to_frame: if full_media { None } else { Some(77) },
        width: if full_media { 960 } else { 480 },
        height: if full_media { 540 } else { 270 },
        include_audio: true,
        runtime_capture: false,
        ..VideoOptions::default()
    };
    let vid_report =
        export_project_video(&project, &mp4_path, &vid_opts).expect("export Midnight Express MP4");
    assert!(vid_report.output_path.exists());
    assert!(fs::metadata(&vid_report.output_path).unwrap().len() > 1024);

    if std::env::var_os("NEOFLASH_VERIFY_PUBLISH_DIR").is_none() {
        let _ = fs::remove_dir_all(&scratch);
    }
}

#[test]
fn midnight_express_5_validates_evaluates_and_exports_across_all_formats() {
    verify_midnight_express_episode(
        5,
        EXPRESS5_REL_PATH,
        "midnight-express-5",
        "sym-locomotive",
        "NEOFLASH_FULL_EXPRESS5_MEDIA",
        express5::build_midnight_express_5,
    );
}

#[test]
fn midnight_express_6_validates_evaluates_and_exports_across_all_formats() {
    verify_midnight_express_episode(
        6,
        EXPRESS6_REL_PATH,
        "midnight-express-6",
        "sym-aurora-locomotive",
        "NEOFLASH_FULL_EXPRESS6_MEDIA",
        express6::build_midnight_express_6,
    );
}
