//! Integration tests for loadable HTML5 publication modules (`T-016`):
//! `module.mjs` ES module entry generation, manifest metadata, size estimation accounting,
//! overwrite ownership cleanup, `Compact` WASM reachability, and `NeoFlashWasmFactory` wrapping.

use neoflash_core::{
    Element, ElementKind, FrameScript, HtmlRenderMode, Project, ResourceUnit, Script,
    ScriptLanguage,
};
use neoflash_export::{
    AudioExportPolicy, BannerOptions, FontExportPolicy, HtmlPublishConfig, PublishOptions,
    SizeLevel, estimate_publish_options_cancellable_with_config, estimate_publish_sizes,
    export_native_banner_html, export_project_with_config, export_shared_runtime, gzip_len,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_dir(prefix: &str) -> PathBuf {
    static ID: AtomicU64 = AtomicU64::new(1);
    let path = std::env::temp_dir().join(format!(
        "neoflash-html5-modules-{prefix}-{}-{}",
        std::process::id(),
        ID.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn create_mock_bindgen_runtime(path: &Path) {
    fs::create_dir_all(path).expect("create runtime dir");
    let mock_bindings = concat!(
        "let NeoFlashWasm = (function(exports) {\n",
        "  exports.WebPlayer = class WebPlayer {};\n",
        "  return exports;\n",
        "})({ __proto__: null });\n",
    );
    fs::write(path.join("neoflash_web.js"), mock_bindings).expect("write neoflash_web.js");
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

fn parse_module_manifest(module_source: &str) -> serde_json::Value {
    let prefix = "export const manifest = Object.freeze(";
    let start = module_source
        .find(prefix)
        .expect("module.mjs should declare `export const manifest = Object.freeze(...)`")
        + prefix.len();
    let end = module_source[start..]
        .find(");\n")
        .expect("manifest Object.freeze closing");
    serde_json::from_str(&module_source[start..start + end]).expect("valid manifest JSON")
}

fn encode_test_png() -> Vec<u8> {
    let rgba = [240u8, 90, 50, 255].repeat(16 * 16);
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(enc, &rgba, 16, 16, image::ExtendedColorType::Rgba8)
        .expect("encode test png");
    out
}

#[test]
fn standard_html5_exports_emit_module_mjs_across_levels_and_render_modes() {
    let root = temp_dir("matrix");
    let rt = root.join("runtime");
    create_mock_bindgen_runtime(&rt);

    let mut project = Project::demo();
    project.name = "Modular Widget".into();
    let png_bytes = encode_test_png();
    let png_res =
        ResourceUnit::from_bytes("badge-res", "images/badge.png", "image/png", &png_bytes)
            .expect("png resource");
    let res_id = png_res.id.clone();
    project.resources.push(png_res);
    let wav_bytes = b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00";
    let wav_res = ResourceUnit::from_bytes("chime-res", "audio/chime.wav", "audio/wav", wav_bytes)
        .expect("wav resource");
    project.resources.push(wav_res);
    project.format_version = project.format_version.max(10);
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "badge-el".into(),
            name: "badge".into(),
            kind: ElementKind::Bitmap {
                resource_id: res_id,
                width: 16,
                height: 16,
                smoothing: true,
            },
            ..Element::default()
        });

    let levels = [
        ("full", SizeLevel::Full),
        ("balanced", SizeLevel::Balanced),
        ("compact", SizeLevel::Compact),
        (
            "shared",
            SizeLevel::Shared("https://cdn.example/neoflash-rt/".into()),
        ),
    ];

    for (level_name, level) in &levels {
        for render_mode in [HtmlRenderMode::Canvas, HtmlRenderMode::Dom] {
            let mode_name = match render_mode {
                HtmlRenderMode::Canvas => "canvas",
                HtmlRenderMode::Dom => "dom",
            };
            let out_dir = root.join(format!("{level_name}-{mode_name}"));
            let options = PublishOptions {
                fonts: FontExportPolicy::Embed,
                size: level.clone(),
                audio: AudioExportPolicy::Original,
            };
            let cfg = HtmlPublishConfig {
                render_mode,
                assets_folder: render_mode == HtmlRenderMode::Dom,
                sprite_sheets: render_mode == HtmlRenderMode::Canvas && *level_name == "balanced",
                ..HtmlPublishConfig::default()
            };

            let entry_path = export_project_with_config(&project, &out_dir, &rt, &options, &cfg)
                .expect("export standard HTML5 package");
            assert_eq!(entry_path, out_dir.join("index.html"));

            let module_path = out_dir.join("module.mjs");
            assert!(
                module_path.is_file(),
                "expected module.mjs in {level_name}/{mode_name}"
            );
            let module_src = fs::read_to_string(&module_path).expect("read module.mjs");
            assert!(module_src.contains("export async function loadBundle("));
            assert!(module_src.contains("export async function ensureRuntime("));
            assert!(module_src.contains("export async function mount(container, options = {})"));
            assert!(module_src.contains("export default mount;"));
            assert!(module_src.contains("globalThis.__neoflashRuntimeLoader"));
            assert!(module_src.contains("globalThis.NeoFlash.mount(container,"));

            let manifest = parse_module_manifest(&module_src);
            assert_eq!(manifest["name"], "Modular Widget");
            assert_eq!(manifest["width"], project.width);
            assert_eq!(manifest["height"], project.height);
            assert_eq!(manifest["fps"], project.fps);
            assert_eq!(manifest["renderMode"], mode_name);
            assert_eq!(manifest["runtimeLevel"], *level_name);
            assert_eq!(manifest["bundle"], "bundle.js");

            match level {
                SizeLevel::Full => {
                    assert!(manifest["runtimeUrl"].is_null());
                    assert!(manifest["wasmUrl"].is_null());
                }
                SizeLevel::Balanced | SizeLevel::Compact => {
                    assert!(manifest["runtimeUrl"].is_null());
                    assert_eq!(manifest["wasmUrl"], "neoflash_web_bg.wasm");
                }
                SizeLevel::Shared(_) => {
                    assert_eq!(manifest["runtimeUrl"], "https://cdn.example/neoflash-rt");
                    assert_eq!(
                        manifest["wasmUrl"],
                        "https://cdn.example/neoflash-rt/neoflash_web_bg.wasm"
                    );
                }
            }

            let assets = manifest["assets"]
                .as_array()
                .expect("manifest.assets array");
            if cfg.assets_folder {
                assert!(
                    assets
                        .iter()
                        .any(|a| a.as_str().is_some_and(|s| s.starts_with("assets/"))),
                    "expected assets/ path in manifest.assets, got {assets:?}"
                );
            }
            if cfg.sprite_sheets {
                assert!(
                    assets.iter().any(|a| a == "sprite_sheet.png"),
                    "expected sprite_sheet.png in manifest.assets, got {assets:?}"
                );
            }

            let marker_raw =
                fs::read_to_string(out_dir.join("neoflash-export.json")).expect("read marker");
            let marker: serde_json::Value =
                serde_json::from_str(&marker_raw).expect("parse marker");
            assert_eq!(marker["module"], "module.mjs");
            assert!(
                marker["files"]
                    .as_array()
                    .expect("marker files")
                    .iter()
                    .any(|f| f == "module.mjs")
            );
        }
    }

    // `include_html = false` still emits `module.mjs` and returns `bundle.js`.
    let no_html_dir = root.join("no-html");
    let no_html_cfg = HtmlPublishConfig {
        include_html: false,
        ..HtmlPublishConfig::default()
    };
    let returned = export_project_with_config(
        &project,
        &no_html_dir,
        &rt,
        &PublishOptions::default(),
        &no_html_cfg,
    )
    .expect("export with include_html = false");
    assert_eq!(returned, no_html_dir.join("bundle.js"));
    assert!(!no_html_dir.join("index.html").exists());
    assert!(no_html_dir.join("module.mjs").is_file());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn native_banner_excludes_module_mjs_and_package_overwrite_cleans_stale_module() {
    let root = temp_dir("banner-overwrite");
    let rt = root.join("runtime");
    create_mock_bindgen_runtime(&rt);

    // 1. Native Banner export never emits module.mjs.
    let mut banner_project = Project::blank();
    banner_project.name = "Ad Banner".into();
    banner_project.width = 300;
    banner_project.height = 250;
    banner_project.fps = 24;
    banner_project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "banner-box".into(),
            name: "box".into(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 60.0,
                fill: [255, 90, 40, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let banner_dir = root.join("native-banner");
    export_native_banner_html(
        &banner_project,
        banner_dir.join("index.html"),
        &BannerOptions::default(),
    )
    .expect("export native banner");
    assert!(banner_dir.join("index.html").is_file());
    assert!(
        !banner_dir.join("module.mjs").exists(),
        "Native Banner must not emit module.mjs"
    );

    // 2. Overwriting a standard HTML5 export directory with a non-module package removes stale module.mjs,
    //    and re-exporting standard HTML5 restores it cleanly.
    let pkg_dir = root.join("switched-package");
    export_project_with_config(
        &Project::demo(),
        &pkg_dir,
        &rt,
        &PublishOptions::default(),
        &HtmlPublishConfig::default(),
    )
    .expect("initial HTML5 export");
    assert!(pkg_dir.join("module.mjs").is_file());

    export_shared_runtime(&pkg_dir, &rt).expect("overwrite with shared-runtime package");
    assert!(
        !pkg_dir.join("module.mjs").exists(),
        "non-module package overwrite must remove stale module.mjs"
    );
    let shared_marker: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(pkg_dir.join("neoflash-export.json")).expect("read shared marker"),
    )
    .expect("parse shared marker");
    assert!(shared_marker.get("module").is_none());

    export_project_with_config(
        &Project::demo(),
        &pkg_dir,
        &rt,
        &PublishOptions::default(),
        &HtmlPublishConfig::default(),
    )
    .expect("re-export HTML5 package");
    assert!(pkg_dir.join("module.mjs").is_file());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn publish_size_estimates_account_for_module_mjs_exactly() {
    let root = temp_dir("estimates");
    let rt = root.join("runtime");
    create_mock_bindgen_runtime(&rt);

    let project = Project::demo();
    let shared_url = "https://cdn.example/runtime";
    let all_estimates = estimate_publish_sizes(&project, &rt, FontExportPolicy::Embed, shared_url)
        .expect("estimate_publish_sizes across all 4 levels");
    assert_eq!(all_estimates.len(), 4);

    for est in &all_estimates {
        let opts = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size: est.level.clone(),
            audio: AudioExportPolicy::Original,
        };
        let sub_default = root.join(format!("{}-default", est.level.name()));
        neoflash_export::export_project_with_options(&project, &sub_default, &rt, &opts)
            .expect("export default");
        assert_eq!(
            (est.files, est.bytes, est.gzip_bytes),
            folder_totals(&sub_default),
            "estimate_publish_sizes mismatch for {:?}",
            est.level
        );

        let sub = root.join(est.level.name());
        let cfg = HtmlPublishConfig::default();
        let single =
            estimate_publish_options_cancellable_with_config(&project, &rt, &opts, &cfg, || false)
                .expect("single estimate result")
                .expect("uncancelled single estimate");

        export_project_with_config(&project, &sub, &rt, &opts, &cfg)
            .expect("export for size check");
        let (disk_files, disk_bytes, disk_gz) = folder_totals(&sub);
        assert_eq!(
            single.files, disk_files,
            "file count mismatch for {:?}: est={} disk={}",
            est.level, single.files, disk_files
        );
        assert_eq!(
            single.bytes, disk_bytes,
            "byte total mismatch for {:?}: est={} disk={}",
            est.level, single.bytes, disk_bytes
        );
        assert_eq!(
            single.gzip_bytes, disk_gz,
            "gzip byte total mismatch for {:?}: est={} disk={}",
            est.level, single.gzip_bytes, disk_gz
        );
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compact_wasm_reachability_and_reusable_wasm_factory_preserved() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let real_rt = workspace_root.join("target/web-runtime");
    let root = temp_dir("compact-wasm");
    let rt = if real_rt.join("neoflash_web_bg.wasm").is_file() {
        real_rt
    } else {
        let fallback = root.join("runtime");
        create_mock_bindgen_runtime(&fallback);
        fallback
    };

    let mut project = Project::blank();
    project.name = "Compact Module".into();
    project.width = 320;
    project.height = 240;
    project.fps = 24;
    project.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "trace('compact module ready'); stop();".into(),
        },
    });

    let compact_dir = root.join("compact");
    export_project_with_config(
        &project,
        &compact_dir,
        &rt,
        &PublishOptions {
            size: SizeLevel::Compact,
            ..PublishOptions::default()
        },
        &HtmlPublishConfig::default(),
    )
    .expect("export compact module");

    let bindings_js =
        fs::read_to_string(compact_dir.join("neoflash_web.js")).expect("read neoflash_web.js");
    assert!(
        bindings_js.contains("globalThis.NeoFlashWasmFactory ="),
        "expected wasm-bindgen output to expose NeoFlashWasmFactory for multi-instance isolation"
    );
    assert!(
        bindings_js.contains("var NeoFlashWasm = globalThis.NeoFlashWasm"),
        "expected var NeoFlashWasm backward-compatible global binding"
    );

    let module_src =
        fs::read_to_string(compact_dir.join("module.mjs")).expect("read compact module.mjs");
    assert!(
        !module_src.contains("\"air.js\"") && !module_src.contains("\"fte3d.js\""),
        "Compact module.mjs RUNTIME_FILES must omit unused optional host families"
    );
    assert!(!compact_dir.join("air.js").exists());
    assert!(!compact_dir.join("fte3d.js").exists());

    if rt != root.join("runtime") {
        let input_wasm_len = fs::metadata(rt.join("neoflash_web_bg.wasm"))
            .expect("input wasm metadata")
            .len();
        let compact_wasm_len = fs::metadata(compact_dir.join("neoflash_web_bg.wasm"))
            .expect("compact wasm metadata")
            .len();
        assert!(
            compact_wasm_len < input_wasm_len,
            "Compact WASM reachability pass must shrink neoflash_web_bg.wasm ({compact_wasm_len} >= {input_wasm_len})"
        );
    }

    let shared_rt_dir = root.join("shared-rt");
    export_shared_runtime(&shared_rt_dir, &rt).expect("export shared runtime");
    let shared_bindings = fs::read_to_string(shared_rt_dir.join("neoflash_web.js"))
        .expect("read shared neoflash_web.js");
    assert!(shared_bindings.contains("globalThis.NeoFlashWasmFactory ="));

    let _ = fs::remove_dir_all(&root);
}
