//! Fast integration regression test for `examples/simcity-ts.nflash` (`T-496E`):
//! - Validates document invariants (`project.validate()`) and strict TypeScript module + handler checking (`0` errors, `0` warnings).
//! - Exports HTML5 in both `HtmlRenderMode::Dom` and `HtmlRenderMode::Canvas`, verifying `FO-10` pure-TS cleanliness (`as-intrinsics.js` is NOT emitted and `bundle.js` omits ActionScript intrinsics).
//! - Boots `neoflash_core::Player` across multiple frames cleanly.

use neoflash_core::{HtmlRenderMode, Player, Project};
use neoflash_export::{
    AudioExportPolicy, FontExportPolicy, HtmlPublishConfig, PublishOptions, SizeLevel,
    export_project_with_config,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn temp_test_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "neoflash-t496e-simcity-{}-{}-{}",
        label,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

fn write_mock_wasm_runtime(runtime_dir: &Path) {
    fs::create_dir_all(runtime_dir).expect("create mock runtime dir");
    fs::write(
        runtime_dir.join("neoflash_web.js"),
        "let NeoFlashWasm = (function(exports) { exports.WebPlayer = class WebPlayer {}; return exports; })({ __proto__: null });\n",
    )
    .expect("write neoflash_web.js");
    fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

#[test]
fn simcity_ts_validates_typechecks_exports_dom_and_canvas_without_as_intrinsics_and_boots_player() {
    let root = workspace_root();
    let project_path = root.join("examples/simcity-ts.nflash");
    let raw = fs::read_to_string(&project_path).expect("read examples/simcity-ts.nflash");
    let project: Project =
        serde_json::from_str(&raw).expect("parse examples/simcity-ts.nflash as Project");

    assert_eq!(project.width, 800);
    assert_eq!(project.height, 520);
    assert_eq!(project.fps, 30);
    assert!(
        project.sources.len() >= 3,
        "expected multi-module TypeScript architecture (>=3 SourceUnits), got {}",
        project.sources.len()
    );

    // 1. Validate document model invariants.
    project
        .validate()
        .expect("examples/simcity-ts.nflash must pass project.validate()");

    // 2. Compile and strictly check TypeScript modules + frame controller (0 errors, 0 warnings).
    let program = neoflash_script::check_project(&project).unwrap_or_else(|diags| {
        let formatted: Vec<String> = diags.iter().map(ToString::to_string).collect();
        panic!(
            "examples/simcity-ts.nflash failed neoflash_script::check_project:\n{}",
            formatted.join("\n")
        );
    });
    assert!(
        program.module_names.len() >= 3,
        "expected >=3 compiled TS modules in topological graph, got {:?}",
        program.module_names
    );

    let warnings = neoflash_script::check_project_warnings(&project);
    assert!(
        warnings.is_empty(),
        "examples/simcity-ts.nflash must compile with 0 warnings, got: {warnings:?}"
    );

    // 3. Multi-frame native Player boot check.
    let mut player = Player::new(&project);
    for _ in 0..5 {
        player.tick(&project);
    }
    let snapshot = player.runtime_snapshot(&project);
    assert!(
        !snapshot.draws.is_empty(),
        "expected authored UI backdrop draws in initial runtime snapshot"
    );

    // 4. Export HTML5 in both Dom and Canvas modes (compact size level) and assert FO-10 pure-TS cleanliness.
    let temp_dir = temp_test_dir("dom-canvas");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let options = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };

    for (mode_label, render_mode) in [
        ("dom", HtmlRenderMode::Dom),
        ("canvas", HtmlRenderMode::Canvas),
    ] {
        let out_dir = temp_dir.join(mode_label);
        let cfg = HtmlPublishConfig {
            render_mode,
            ..HtmlPublishConfig::default()
        };
        let index_path =
            export_project_with_config(&project, &out_dir, &runtime_dir, &options, &cfg)
                .unwrap_or_else(|e| panic!("HTML5 {mode_label} export failed: {e}"));

        assert!(index_path.is_file(), "{mode_label}: missing index.html");
        assert!(
            out_dir.join("bundle.js").is_file(),
            "{mode_label}: missing bundle.js"
        );
        assert!(
            out_dir.join("runtime.js").is_file(),
            "{mode_label}: missing runtime.js"
        );
        assert!(
            out_dir.join("module.mjs").is_file(),
            "{mode_label}: missing module.mjs"
        );
        assert!(
            !out_dir.join("as-intrinsics.js").exists(),
            "FO-10 regression: {mode_label} export must NOT emit as-intrinsics.js for pure TypeScript"
        );

        let index_html = fs::read_to_string(&index_path).expect("read index.html");
        assert!(
            !index_html.contains("as-intrinsics.js"),
            "FO-10 regression: {mode_label} index.html must not reference as-intrinsics.js"
        );
        if render_mode == HtmlRenderMode::Dom {
            assert!(
                out_dir.join("dom-layout.js").is_file(),
                "DOM export must include dom-layout.js"
            );
        }

        let bundle_js = fs::read_to_string(out_dir.join("bundle.js")).expect("read bundle.js");
        assert!(
            bundle_js.len() < 160 * 1024,
            "{mode_label}: expected ultra-compact pure-TS bundle.js (<160 KiB), got {} bytes",
            bundle_js.len()
        );
        assert!(
            bundle_js.starts_with("globalThis.NeoFlashBundle = "),
            "{mode_label}: bundle.js must assign globalThis.NeoFlashBundle"
        );
        assert!(
            !bundle_js.contains("__nfDefaultNativeTypes"),
            "FO-10 regression: {mode_label} bundle.js must not inline ActionScript intrinsics"
        );
        let json_payload = bundle_js
            .trim_start_matches("globalThis.NeoFlashBundle = ")
            .trim_end()
            .trim_end_matches(';');
        let parsed_bundle: serde_json::Value =
            serde_json::from_str(json_payload).expect("bundle.js payload must be valid JSON");
        assert!(
            parsed_bundle.get("as_prelude").is_none() || parsed_bundle["as_prelude"].is_null(),
            "{mode_label}: pure TypeScript bundle must not carry as_prelude"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
