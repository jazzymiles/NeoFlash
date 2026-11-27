use neoflash_core::{Player, Project, ScriptLanguage};
use neoflash_export::{
    AudioExportPolicy, FlaExportOptions, FlaTargetProfile, FontExportPolicy, HtmlPublishConfig,
    PublishOptions, SizeLevel, SwfPublishOptions, SwfPublishProfile, export_fla_bytes_with_profile,
    export_project_with_config, publish_swf_bytes,
};
use neoflash_script::convert::{ConversionTarget, convert_project_scripts};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn write_mock_wasm_runtime(runtime_dir: &Path) {
    fs::create_dir_all(runtime_dir).expect("create mock runtime dir");
    fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;\n")
        .expect("write neoflash_web.js");
    fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

#[test]
fn t496d_xcom_as3_flagship_validates_exports_roundtrips_and_boots() {
    let root = workspace_root();
    let project_path = root.join("examples/xcom-as3.nflash");
    let raw = fs::read_to_string(&project_path).expect("read examples/xcom-as3.nflash");
    assert!(
        raw.len() < 350_000,
        "examples/xcom-as3.nflash must stay compact (< 350 KB), got {} B",
        raw.len()
    );

    let project = Project::from_json(&raw).expect("parse examples/xcom-as3.nflash");
    assert_eq!(project.width, 800);
    assert_eq!(project.height, 520);
    assert_eq!(project.fps, 30);
    assert_eq!(project.script_language, Some(ScriptLanguage::ActionScript3));

    // 1. Validate document invariants + multi-package AS3 architecture + 0 script errors/warnings
    project
        .validate()
        .expect("xcom-as3.nflash project.validate()");
    assert!(
        project.sources.len() >= 5,
        "expected >= 5 multi-package AS3 SourceUnits, got {}",
        project.sources.len()
    );
    for expected_path in [
        "xcom/sim/ITacticalActor.as",
        "xcom/events/TacticalEvent.as",
        "xcom/model/SquadDatabase.as",
        "xcom/sim/BattlescapeSim.as",
        "xcom/render/TacticalHud.as",
    ] {
        assert!(
            project.sources.iter().any(|u| u.path == expected_path),
            "missing expected AS3 SourceUnit '{expected_path}'"
        );
    }

    let program = neoflash_script::check_project(&project).unwrap_or_else(|diags| {
        let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
        panic!("xcom-as3.nflash failed check_project:\n{}", msgs.join("\n"));
    });
    assert!(
        !program.code.is_empty() && program.class_names.len() >= 5,
        "compiled ProjectProgram must include >= 5 AS3 classes"
    );
    let warnings = neoflash_script::check_project_warnings(&project);
    assert!(
        warnings.is_empty(),
        "xcom-as3.nflash must compile with 0 warnings, got: {warnings:?}"
    );

    // 2. HTML5 export: verify standalone `as-intrinsics.js` + lean `bundle.js` (< 180 KB)
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let mut clean_cfg = HtmlPublishConfig::clean_publish_default();
    clean_cfg.assets_folder = true;
    clean_cfg.sprite_sheets = true;
    clean_cfg.omit_file_fallback = true;
    let compact_opts = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };
    let html_out = temp_dir.path().join("xcom-html5");
    export_project_with_config(&project, &html_out, &runtime_dir, &compact_opts, &clean_cfg)
        .expect("HTML5 export of xcom-as3.nflash");

    assert!(
        html_out.join("as-intrinsics.js").is_file(),
        "HTML5 export must emit standalone as-intrinsics.js"
    );
    let bundle_js = fs::read_to_string(html_out.join("bundle.js")).expect("read bundle.js");
    assert!(
        !bundle_js.contains("\"as_prelude\":"),
        "bundle.js must externalize as_prelude to as-intrinsics.js"
    );
    assert!(
        bundle_js.len() < 180_000,
        "xcom-as3 bundle.js must be < 180 KB, got {} B",
        bundle_js.len()
    );

    // 3. FP10 AVM2 SWF export (`blocking_count == 0`), multi-frame SWF execution, and SWF re-import round-trip
    let (swf_bytes, swf_rep) = publish_swf_bytes(
        &project,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            ..SwfPublishOptions::default()
        },
    )
    .expect("FP10 AVM2 SWF export of xcom-as3.nflash");
    assert!(
        !swf_rep.has_blocking(),
        "FP10 AVM2 SWF preflight must have 0 blocking items, got: {:?}",
        swf_rep.items
    );

    assert!(
        swf_bytes.len() > 64
            && (&swf_bytes[..3] == b"CWS" || &swf_bytes[..3] == b"FWS")
            && swf_bytes[3] >= 10,
        "expected valid FP10+ SWF header"
    );
    let _swf_play = neoflash_import::play_swf_bytes(&swf_bytes, 4).expect("play xcom-fp10.swf");

    let swf_path = temp_dir.path().join("xcom-fp10.swf");
    fs::write(&swf_path, &swf_bytes).expect("write xcom-fp10.swf");
    let reimported = neoflash_import::import_project(&swf_path).expect("re-import xcom-fp10.swf");
    reimported
        .validate()
        .expect("re-imported xcom-fp10.swf validate()");
    neoflash_script::check_project(&reimported).unwrap_or_else(|errs| {
        let msgs: Vec<String> = errs.iter().map(ToString::to_string).collect();
        panic!(
            "re-imported xcom-fp10.swf failed strict check_project:\n{}",
            msgs.join("\n")
        );
    });

    // 4. `.fla` / XFL export: verify `<DOMSoundItem>` clean linkage identifiers (`FO-05`)
    let (fla_bytes, _fla_rep) = export_fla_bytes_with_profile(
        &project,
        FlaTargetProfile::AnimateCC,
        &FlaExportOptions::default(),
    )
    .expect("export .fla");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(fla_bytes)).expect("open .fla zip");
    let mut dom_xml = String::new();
    zip.by_name("DOMDocument.xml")
        .expect("DOMDocument.xml in .fla")
        .read_to_string(&mut dom_xml)
        .expect("read DOMDocument.xml");
    for sfx_linkage in [
        "SfxPlasma",
        "SfxAlien",
        "SfxStep",
        "SfxRadar",
        "SfxBoom",
        "SfxMedkit",
    ] {
        assert!(
            dom_xml.contains(&format!("linkageIdentifier=\"{sfx_linkage}\"")),
            "DOMDocument.xml missing <DOMSoundItem> linkageIdentifier={sfx_linkage}"
        );
    }

    // 5. Deterministic AS3 -> JS script conversion (`FO-11`)
    let (js_project, conv_rep) = convert_project_scripts(&project, ConversionTarget::JavaScript)
        .expect("deterministic AS3 -> JS conversion");
    js_project
        .validate()
        .expect("converted JS project validate()");
    assert!(
        conv_rep.converted_source_units >= 5 && conv_rep.converted_frame_scripts >= 1,
        "expected >= 5 converted source units and >= 1 frame script, got {conv_rep:?}"
    );
    neoflash_script::check_project(&js_project).expect("converted JS project compiles cleanly");

    // 6. Core `Player` multi-frame boot without overflow or runtime errors
    let mut player = Player::new(&project);
    for _ in 0..8 {
        player.tick(&project);
        assert!(
            !player.script_overflowed(),
            "Player must not overflow script budget during multi-frame boot"
        );
        let _ = player.drain_sound_commands();
    }
}
