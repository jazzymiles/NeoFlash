
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
fn temp() -> PathBuf {
    static ID: AtomicU64 = AtomicU64::new(1);
    let path = std::env::temp_dir().join(format!(
        "neoflash-export-{}-{}",
        std::process::id(),
        ID.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
fn runtime(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    fs::write(path.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
}
#[test]
fn atomic_write_uses_distinct_temps_and_cleans_them_after_parallel_replacement() {
    let dir = temp();
    let png = dir.join("sheet.png");
    let json = dir.join("sheet.json");
    let mut workers = Vec::new();
    for index in 0..16u8 {
        let path = if index % 2 == 0 {
            png.clone()
        } else {
            json.clone()
        };
        workers.push(std::thread::spawn(move || {
            let bytes = vec![index; 1024];
            atomic_write(&path, &bytes).expect("parallel atomic write");
        }));
    }
    for worker in workers {
        worker.join().expect("atomic writer thread");
    }
    for path in [&png, &json] {
        let bytes = fs::read(path).expect("final atomic output");
        assert_eq!(bytes.len(), 1024);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
    }
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".neoflash-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary files leaked: {leftovers:?}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn package_paths_reject_generated_name_collisions_before_writing() {
    assert!(reserved_publish_path("index.html"));
    assert!(reserved_publish_path("RUNTIME.JS"));
    assert!(reserved_publish_path("neoflash-export.json"));
    assert!(reserved_publish_path("runtime.js/clip.mp4"));
    assert!(!reserved_publish_path("media/clip.mp4"));

    let duplicate = vec![
        ("media/Clip.mp4".to_owned(), vec![1]),
        ("media/clip.mp4".to_owned(), vec![2]),
    ];
    assert!(validate_package_file_names(&duplicate).is_err());
    let marker = vec![(MARKER.to_owned(), vec![1])];
    assert!(validate_package_file_names(&marker).is_err());
    let file_directory_collision = vec![
        ("runtime.js".to_owned(), vec![1]),
        ("RUNTIME.JS/clip.mp4".to_owned(), vec![2]),
    ];
    assert!(validate_package_file_names(&file_directory_collision).is_err());
    assert!(package_paths_collide("media", "Media/clip.mp4"));

    let dir = temp();
    let sentinel = dir.join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    assert!(write_package(&dir, &file_directory_collision, "collision", "project").is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
    assert!(!dir.join("media").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_external_video_still_rejects_generated_package_path_collision() {
    let dir = temp();
    let runtime_dir = dir.join("runtime");
    runtime(&runtime_dir);
    let output_dir = dir.join("output");

    let mut project = Project::blank();
    project.format_version = 11;
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes("video", "index.html", "video/mp4", &[]).unwrap(),
    );
    project.symbols[0].timeline.layers[0].keyframes[0].video = Some(
        neoflash_core::TimelineVideoSpan::external("video", "index.html"),
    );
    project.validate().unwrap();

    let error = export_project_with_config(
        &project,
        &output_dir,
        &runtime_dir,
        &PublishOptions::default(),
        &HtmlPublishConfig::clean_publish_default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("collides with a generated package file"),
        "got: {error}"
    );
    assert!(
        !output_dir.exists(),
        "failed preflight must not write output"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn package_case_only_asset_rename_never_deletes_the_new_file() {
    let dir = temp();
    let first = vec![
        ("Media/clip.mp4".to_owned(), b"old".to_vec()),
        ("index.html".to_owned(), b"first".to_vec()),
    ];
    write_package(&dir, &first, "case rename", "project").unwrap();

    let second = vec![
        ("media/clip.mp4".to_owned(), b"new".to_vec()),
        ("index.html".to_owned(), b"second".to_vec()),
    ];
    write_package(&dir, &second, "case rename", "project").unwrap();
    assert_eq!(fs::read(dir.join("media/clip.mp4")).unwrap(), b"new");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn export_escapes_title_and_keeps_sources_external() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);
    let mut project = Project::blank();
    project.name = "</title><script>alert('x')</script>".into();
    project.format_version = 3;
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "sample",
            "data/sample.txt",
            "text/plain",
            b"</script>portable data",
        )
        .unwrap(),
    );
    let output = dir.join("output");
    export_project(&project, &output, &rt).unwrap();
    let html = fs::read_to_string(output.join("index.html")).unwrap();
    assert!(html.contains("&lt;/title&gt;&lt;script&gt;"));
    assert!(!html.contains("<script>alert"));
    assert!(
        fs::read_to_string(output.join("bundle.js"))
            .unwrap()
            .contains("AGFzbQEAAAA=")
    );
    assert!(!html.contains("https://"));
    assert!(html.find("src=\"path.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"net.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"external.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"bitmap.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"text.js\"").unwrap() < html.find("src=\"display.js\"").unwrap());
    assert!(html.find("src=\"text-input.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"filters.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(
        html.find("src=\"audio-formats.js\"").unwrap() < html.find("src=\"audio.js\"").unwrap()
    );
    assert!(html.find("src=\"audio.js\"").unwrap() < html.find("src=\"video.js\"").unwrap());
    assert!(html.find("src=\"video.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"xml.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"loader.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"amf.js\"").unwrap() < html.find("src=\"storage.js\"").unwrap());
    assert!(html.find("src=\"storage.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(html.find("src=\"air.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
    assert!(
        html.find("src=\"as3-net-sys.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
    );
    assert!(
        html.find("src=\"as3-media-events.js\"").unwrap()
            < html.find("src=\"runtime.js\"").unwrap()
    );
    assert!(
        fs::read_to_string(output.join("air.js"))
            .unwrap()
            .contains("NeoFlashAIR")
    );
    assert!(
        fs::read_to_string(output.join("as3-media-events.js"))
            .unwrap()
            .contains("NeoFlashAS3MediaEvents")
    );
    let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
    assert!(bundle.contains("PC9zY3JpcHQ+cG9ydGFibGUgZGF0YQ=="));
    assert!(
        !output.join("data/sample.txt").exists(),
        "resource bytes stay in the portable bundle, without filesystem path writes"
    );
    assert!(
        fs::read_to_string(output.join("net.js"))
            .unwrap()
            .contains("NeoFlashNet")
    );
    assert!(
        fs::read_to_string(output.join("external.js"))
            .unwrap()
            .contains("NeoFlashExternal")
    );
    assert!(
        fs::read_to_string(output.join("path.js"))
            .unwrap()
            .contains("isPointInStroke")
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn size_levels_change_delivery_and_leave_out_only_unnamed_families() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);
    let banner = Project::blank();
    let out = dir.join("out");
    let publish = |project: &Project, size: SizeLevel| {
        let options = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size,
            audio: AudioExportPolicy::Original,
        };
        export_project_with_options(project, &out, &rt, &options).unwrap();
        let bundle = fs::read_to_string(out.join("bundle.js")).unwrap();
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        (bundle, html)
    };

    let (bundle, _) = publish(&banner, SizeLevel::Full);
    assert!(bundle.contains("\"wasm\":\"AGFzbQEAAAA=\"") && !out.join(WASM_FILE).exists());

    let (bundle, _) = publish(&banner, SizeLevel::Balanced);
    assert!(
        bundle.contains("\"wasm_url\":\"neoflash_web_bg.wasm\"") && !bundle.contains("\"wasm\":")
    );
    assert!(out.join(WASM_FILE).exists() && out.join("air.js").exists());

    // A banner without scripts drops every script family but keeps sound and video transport.
    let (bundle, html) = publish(&banner, SizeLevel::Compact);
    for gone in [
        "air.js",
        "as3-fte-3d.js",
        "as2-object.js",
        "xml.js",
        "timers.js",
    ] {
        assert!(
            !out.join(gone).exists() && !html.contains(gone),
            "{gone} left out"
        );
    }
    for kept in [
        "net.js",
        "byte-array.js",
        "audio.js",
        "display.js",
        "runtime.js",
        WASM_FILE,
    ] {
        assert!(out.join(kept).exists(), "{kept} kept");
    }
    assert!(
        !bundle.contains("\"program\":"),
        "a project without scripts ships no prelude"
    );
    assert!(bundle.contains("\"level\":\"compact\"") && bundle.contains("AIR desktop APIs"));

    let mut fte = Project::blank();
    let root = fte.root.clone();
    fte.symbol_mut(&root)
        .unwrap()
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source:
                    "import flash.text.engine.ElementFormat;\ntrace(new ElementFormat().fontSize);"
                        .into(),
            },
        });
    publish(&fte, SizeLevel::Compact);
    assert!(out.join("as3-fte-3d.js").exists() && out.join("xml.js").exists());
    assert!(!out.join("air.js").exists() && !out.join("as3-net-sys.js").exists());

    let (bundle, html) = publish(
        &banner,
        SizeLevel::Shared("https://cdn.example.com/nf/".into()),
    );
    let mut files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(
        files,
        ["README.txt", "bundle.js", "index.html", MARKER],
        "stale runtime files are removed"
    );
    assert!(html.contains("src=\"https://cdn.example.com/nf/runtime-id.js\""));
    assert!(
        html.contains("src=\"bundle.js\"")
            && html.contains("src=\"https://cdn.example.com/nf/air.js\"")
    );
    assert!(bundle.contains("\"wasm_url\":\"https://cdn.example.com/nf/neoflash_web_bg.wasm\""));
    assert!(bundle.contains("\"runtime_id\":\""));

    let shared = dir.join("shared");
    export_shared_runtime(&shared, &rt).unwrap();
    let id = fs::read_to_string(shared.join(RUNTIME_ID_FILE)).unwrap();
    let bundle_id = bundle
        .split("\"runtime_id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(
        id.contains(bundle_id) && shared.join(WASM_FILE).exists() && shared.join("air.js").exists()
    );

    let estimates = estimate_publish_sizes(
        &banner,
        &rt,
        FontExportPolicy::Embed,
        "https://cdn.example.com/nf",
    )
    .unwrap();
    let names: Vec<_> = estimates.iter().map(|e| e.level.name()).collect();
    assert_eq!(names, ["full", "balanced", "compact", "shared"]);
    assert!(estimates[2].bytes < estimates[1].bytes && estimates[3].bytes < estimates[2].bytes);
    assert!(estimates[3].shared_runtime_bytes.unwrap() > estimates[1].bytes / 2);
    let totals = |folder: &Path| {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let data = fs::read(entry.unwrap().path()).unwrap();
                (1usize, data.len(), gzip_len(&data))
            })
            .fold((0, 0, 0), |sum, item| {
                (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
            })
    };
    let shared_totals = totals(&shared);
    assert_eq!(estimates[3].shared_runtime_bytes, Some(shared_totals.1));
    assert_eq!(
        estimates[3].shared_runtime_gzip_bytes,
        Some(shared_totals.2)
    );
    for estimate in &estimates {
        publish(&banner, estimate.level.clone());
        assert_eq!(
            totals(&out),
            (estimate.files, estimate.bytes, estimate.gzip_bytes)
        );

        assert_eq!(
            estimate.parts.iter().map(|p| p.bytes).sum::<usize>(),
            estimate.bytes
        );
        assert!(estimate.gzip_bytes < estimate.bytes);
    }
    assert!(estimates[2].omitted_modules.contains(&"air.js".to_owned()));
    assert!(estimates[0].tradeoffs.is_empty() && !estimates[1].tradeoffs.is_empty());
    assert_eq!(
        estimate_publish_options(
            &banner,
            &rt,
            &PublishOptions {
                fonts: FontExportPolicy::Embed,
                size: SizeLevel::Shared(" https://cdn.example.com/nf/ ".into()),
                audio: AudioExportPolicy::Original,
            }
        )
        .unwrap(),
        estimates[3],
        "selected normalized options measure exactly what publication writes"
    );
    let (root_bundle, root_html) = publish(&banner, SizeLevel::Shared("/".into()));
    assert!(root_html.contains("src=\"/runtime-id.js\""));
    assert!(root_bundle.contains("\"wasm_url\":\"/neoflash_web_bg.wasm\""));
    // Rebuilding the runtime must change the identity expected by Shared publications.
    fs::write(rt.join(BINDINGS_FILE), "var NeoFlashWasm; // rebuilt").unwrap();
    let (rebuilt, _) = publish(&banner, SizeLevel::Shared("../runtime".into()));
    assert!(!rebuilt.contains(&format!("\"runtime_id\":\"{bundle_id}\"")));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn estimates_cancel_and_invalid_options_fail_before_runtime_io() {
    let dir = temp();
    let rt = dir.join("runtime");
    let project = Project::blank();
    let invalid = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Shared("https://".into()),
        audio: AudioExportPolicy::Original,
    };
    assert!(matches!(
        export_project_with_options(&project, &dir.join("out"), &rt, &invalid),
        Err(ExportError::Options(_))
    ));
    assert!(matches!(
        estimate_publish_options(&project, &rt, &invalid),
        Err(ExportError::Options(_))
    ));
    assert!(!dir.join("out").exists());
    assert!(
        estimate_publish_levels_cancellable(
            &project,
            &rt,
            FontExportPolicy::Embed,
            &[SizeLevel::Full],
            || true
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        estimate_publish_levels_cancellable(&project, &rt, FontExportPolicy::Embed, &[], || false)
            .unwrap(),
        Some(vec![])
    );
    runtime(&rt);
    let mut checks = 0;
    assert!(
        estimate_publish_levels_cancellable(
            &project,
            &rt,
            FontExportPolicy::Embed,
            &[SizeLevel::Full, SizeLevel::Compact],
            || {
                checks += 1;
                checks >= 6
            }
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        checks, 6,
        "cancellation stops compression without publishing a partial result"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn multiple_scenes_publish_one_main_timeline_with_scene_ranges() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);
    let mut project = Project::blank();
    project.format_version = 6;
    let mut second = project.symbols[0].clone();
    second.id = "scene-two".into();
    second.name = "Scene 2".into();
    second.timeline.frame_count = 4;
    second.timeline.layers[0].id = "scene-two-layer".into();
    second.timeline.scripts.push(neoflash_core::FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "gotoAndStop(1, \"Scene 1\");".into(),
        },
    });
    project.scenes = vec![project.root.clone(), second.id.clone()];
    project.symbols.push(second);
    let output = dir.join("output");
    export_project(&project, &output, &rt).unwrap();
    let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
    let json = bundle
        .strip_prefix("globalThis.NeoFlashBundle = ")
        .and_then(|rest| rest.strip_suffix(";\n"))
        .unwrap();
    let bundle: serde_json::Value = serde_json::from_str(json).unwrap();
    let published: Project = serde_json::from_value(bundle["project"].clone()).unwrap();
    published.validate().unwrap();
    assert!(published.scenes.is_empty());
    assert_eq!(published.symbols.len(), 1);
    let root = published.symbol(&published.root).unwrap();
    assert_eq!(root.timeline.frame_count, 124);
    assert_eq!(root.timeline.scripts[0].frame, 121);
    assert_eq!(
        bundle["scenes"],
        serde_json::json!([
            {"name": "Scene 1", "start": 0, "frames": 120},
            {"name": "Scene 2", "start": 120, "frames": 4},
        ])
    );
    assert!(
        bundle["scripts"][0]["location"]
            .as_str()
            .unwrap()
            .starts_with("Scene 2 / frame 2"),
        "diagnostics keep the authored scene frame"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn export_strips_authoring_guides() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);
    let mut project = Project::blank();
    project.format_version = neoflash_core::Guide::FORMAT_VERSION;
    project.symbols[0]
        .timeline
        .guides
        .push(neoflash_core::Guide {
            axis: neoflash_core::GuideAxis::Vertical,
            position: 123.,
        });
    let output = dir.join("output");
    export_project(&project, &output, &rt).unwrap();
    let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
    assert!(bundle.contains("NeoFlashBundle"));
    assert!(!bundle.contains("\"guides\""), "guides are authoring-only");
    assert_eq!(
        project.symbols[0].timeline.guides.len(),
        1,
        "source untouched"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn refuses_unrelated_output_without_modifying_it() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);
    let out = dir.join("output");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("index.html"), "existing work").unwrap();
    assert!(matches!(
        export_project(&Project::blank(), &out, &rt),
        Err(ExportError::Destination(_))
    ));
    assert_eq!(
        fs::read_to_string(out.join("index.html")).unwrap(),
        "existing work"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn missing_runtime_does_not_create_output() {
    let dir = temp();
    let out = dir.join("out");
    assert!(matches!(
        export_project(&Project::blank(), &out, &dir.join("missing")),
        Err(ExportError::MissingRuntime(_))
    ));
    assert!(!out.exists());
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_maps_handlers_and_groups_identical_authored_origins() {
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 2;
    let source = "// ё\ntrace('mapped');\nthrow new Error('boom');";
    project.symbols[0].timeline.scripts = (0..2)
        .map(|frame| neoflash_core::FrameScript {
            frame,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            },
        })
        .collect();
    let program = neoflash_script::compile_project(&project).unwrap();
    let scripts = compile_scripts(&project, &program).unwrap();
    assert_eq!(scripts.len(), 1);
    let script = &scripts[0];
    assert_eq!(script.source_url, "neoflash-script-0.js");
    assert_eq!(script.location, "Shared script 1");
    assert_eq!(script.locations.len(), 2);
    assert!(script.locations[0].ends_with("frame 1"));
    assert!(script.locations[1].ends_with("frame 2"));
    assert_eq!(script.source_map.sources, ["Shared script 1"]);
    assert!(
        script
            .source_locations
            .iter()
            .any(|location| location.line == 3 && location.source == "Shared script 1")
    );
    // A duplicate invalid script must report errors, never index an absent compiled entry.
    for frame in &mut project.symbols[0].timeline.scripts {
        frame.script.source = "var = ;".into();
    }
    assert!(matches!(
        compile_scripts(&project, &program),
        Err(ExportError::Scripts(_))
    ));
}

#[test]
fn unsupported_script_blocks_export_before_writing() {
    let dir = temp();
    let out = dir.join("out");
    let mut project = Project::blank();
    project.symbols[0]
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "package example { public class Missing {} }".into(),
            },
        });
    assert!(matches!(
        export_project(&project, &out, &dir.join("missing")),
        Err(ExportError::Scripts(_))
    ));
    assert!(!out.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn size_report_breaks_down_scenes_frames_symbols_fonts_resources_and_scripts() {
    let mut project = Project::demo();
    project.format_version = 7;
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "sfx-beep",
            "audio/beep.wav",
            "audio/wav",
            &[0u8; 44],
        )
        .unwrap(),
    );
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "img-dot",
            "images/dot.png",
            "image/png",
            b"\x89PNG\r\n\x1a\n",
        )
        .unwrap(),
    );
    project.sources.push(neoflash_core::SourceUnit {
            id: "src-util".into(),
            path: "AppHelper.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: "package { public class AppHelper { public static function tag():String { return \"ok\"; } } }".into(),
        });
    project.symbols[0]
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(AppHelper.tag());".into(),
            },
        });
    let report = format_size_report(&project).expect("size report succeeds");
    assert!(report.contains("NeoFlash Size Report —"));
    assert!(report.contains("Project Weight:"));
    assert!(report.contains("Scenes & Frames"));
    assert!(report.contains("Frame 1"));
    assert!(report.contains("Symbols"));
    assert!(report.contains("Fonts"));
    assert!(report.contains("Embedded Resources (Audio / Bitmaps / Data)"));
    assert!(report.contains("audio/beep.wav"));
    assert!(report.contains("Audio"));
    assert!(report.contains("images/dot.png"));
    assert!(report.contains("Bitmap"));
    assert!(report.contains("Scripts"));
    assert!(report.contains("AppHelper.as"));
    assert!(report.contains("Scripts Subtotal:"));
    let summary = compute_load_summary(&project).expect("load summary succeeds");
    assert_eq!(summary.scenes_count, 1);
    assert_eq!(summary.resources_count, 2);
    assert_eq!(summary.audio_count, 1);
    assert_eq!(summary.bitmap_count, 1);
    assert!(summary.summary_line().contains("Project weight:"));
    assert!(summary.summary_line().contains("Loads:"));
}

#[test]
fn font_policies_embed_subset_and_convert_static_text_to_outlines() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);

    let mut project = Project::demo();
    project.format_version = 7;
    let ttf_bytes = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "font-neo",
            "fonts/NeoFont.ttf",
            "font/ttf",
            ttf_bytes,
        )
        .unwrap(),
    );
    // Set the demo text element to use NeoFont
    if let Some(elem) = project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .iter_mut()
        .find(|e| matches!(e.kind, ElementKind::Text { .. }))
        && let ElementKind::Text { font_family, .. } = &mut elem.kind
    {
        *font_family = "NeoFont".into();
    }

    // 1. Embed policy injects @font-face CSS and suppresses unembedded font warning
    let warnings = export_warnings(&project);
    assert!(
        !warnings.iter().any(|w| w.contains("NeoFont")),
        "embedded font resource suppresses missing-font warning: {warnings:?}"
    );
    let out_embed = dir.join("out-embed");
    export_project_with_policy(&project, &out_embed, &rt, FontExportPolicy::Embed).unwrap();
    let html_embed = fs::read_to_string(out_embed.join("index.html")).unwrap();
    assert!(html_embed.contains("@font-face"));
    assert!(html_embed.contains("font-family: \"NeoFont\""));
    assert!(html_embed.contains("src=\"dom-layout.js\""));
    assert!(
        fs::read_to_string(out_embed.join("dom-layout.js"))
            .unwrap()
            .contains("NeoFlashDomLayout")
    );

    // 2. Subset policy subsets embedded font resource to used characters
    let out_subset = dir.join("out-subset");
    export_project_with_policy(&project, &out_subset, &rt, FontExportPolicy::Subset).unwrap();
    let bundle_subset: serde_json::Value = serde_json::from_str(
        fs::read_to_string(out_subset.join("bundle.js"))
            .unwrap()
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(bundle_subset["fonts"]["policy"], "subset");
    assert_eq!(
        bundle_subset["fonts"]["subset_stats"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        bundle_subset["fonts"]["subset_stats"][0]["codepoint_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        bundle_subset["fonts"]["subset_stats"][0]["subset_applied"], true,
        "subset policy must emit a real SFNT/TTF subset for TrueType fonts"
    );
    assert!(
        bundle_subset["fonts"]["subset_stats"][0]["subset_bytes"]
            .as_u64()
            .unwrap()
            < bundle_subset["fonts"]["subset_stats"][0]["original_bytes"]
                .as_u64()
                .unwrap()
    );
    assert!(
        !bundle_subset["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("complete browser-loadable font")))
    );

    // 3. Outlines policy converts static text placements into vector Path elements
    let out_outlines = dir.join("out-outlines");
    export_project_with_policy(&project, &out_outlines, &rt, FontExportPolicy::Outlines).unwrap();
    let bundle_outlines: serde_json::Value = serde_json::from_str(
        fs::read_to_string(out_outlines.join("bundle.js"))
            .unwrap()
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(bundle_outlines["fonts"]["policy"], "outlines");
    assert!(
        bundle_outlines["fonts"]["outlined_elements"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let published: Project = serde_json::from_value(bundle_outlines["project"].clone()).unwrap();
    assert!(
        !published.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::Text { .. })),
        "static text was converted to vector path outlines"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn selected_text_embedding_uses_authored_family_style_ranges_and_field_state() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);

    let mut project = Project::blank();
    project.format_version = 7;
    let font_id = "font-book-bold";
    let font_bytes = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            font_id,
            "fonts/Book Antiqua Bold.ttf",
            "font/ttf",
            font_bytes,
        )
        .unwrap(),
    );
    let metadata = serde_json::json!([{
        "id": font_id,
        "name": "Book Antiqua Bold",
        "family": "Book Antiqua",
        "style": "Bold",
        "ranges": [true, false, false, false, false, false, false, false],
        "extra_chars": "Ω",
        "export_for_actionscript": false,
        "class_name": ""
    }]);
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "font-embedding-metadata",
            "fonts/embedding.json",
            "application/json",
            &serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap(),
    );
    let mut format = neoflash_core::TextFormat {
        font: "Book Antiqua".to_owned(),
        size: 18.0,
        bold: true,
        ..neoflash_core::TextFormat::default()
    };
    format.set_field_options(Some(neoflash_core::AuthoredTextFieldOptions {
        embed_fonts: true,
        ..Default::default()
    }));
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(neoflash_core::Element {
            id: "embedded-field".to_owned(),
            name: "Headline".to_owned(),
            kind: ElementKind::TextBox {
                text: "AΩ".to_owned(),
                width: 180.0,
                height: 40.0,
                format,
                word_wrap: false,
                multiline: false,
            },
            ..Default::default()
        });
    project.validate().unwrap();
    assert!(has_embedded_font_face(
        &project,
        "Book Antiqua",
        true,
        false
    ));
    assert!(
        !export_warnings(&project)
            .iter()
            .any(|warning| warning.contains("requires embedded font"))
    );

    let mut subset_project = project.clone();
    let subset = apply_font_policy(&mut subset_project, FontExportPolicy::Subset);
    assert_eq!(subset.subset_stats.len(), 1);
    assert!(subset.subset_stats[0].subset_applied);
    assert!(subset.subset_stats[0].subset_bytes < subset.subset_stats[0].original_bytes);
    let subset_decoded = subset_project.resources[0].decoded_bytes().unwrap();
    assert_eq!(subset_decoded.len(), subset.subset_stats[0].subset_bytes);
    let parsed_subset = ab_glyph::FontRef::try_from_slice(&subset_decoded).unwrap();
    let parsed_orig = ab_glyph::FontRef::try_from_slice(font_bytes).unwrap();
    use ab_glyph::Font as _;
    assert_eq!(parsed_subset.glyph_id('A'), parsed_orig.glyph_id('A'));
    assert_eq!(parsed_subset.glyph_id('Ω'), parsed_orig.glyph_id('Ω'));
    assert_eq!(
        subset.subset_stats[0].codepoint_count, 28,
        "space + A-Z + the explicit custom Omega"
    );

    let mut non_sfnt_project = project.clone();
    non_sfnt_project.resources[0] = neoflash_core::ResourceUnit::from_bytes(
        font_id,
        "fonts/Book Antiqua Bold.woff2",
        "font/woff2",
        b"wOF2-mock-font-payload-that-is-not-sfnt-ttf",
    )
    .unwrap();
    let non_sfnt_subset = apply_font_policy(&mut non_sfnt_project, FontExportPolicy::Subset);
    assert!(!non_sfnt_subset.subset_stats[0].subset_applied);
    assert_eq!(
        non_sfnt_subset.subset_stats[0].original_bytes,
        non_sfnt_subset.subset_stats[0].subset_bytes
    );

    let out = dir.join("out-selected-font");
    export_project_with_policy(&project, &out, &rt, FontExportPolicy::Embed).unwrap();
    let html = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("font-family: \"Book Antiqua\""));
    assert!(html.contains("font-style: normal"));
    assert!(html.contains("font-weight: 700"));
    assert!(!html.contains("font-family: \"Book Antiqua Bold\""));
    let bundle: serde_json::Value = serde_json::from_str(
        fs::read_to_string(out.join("bundle.js"))
            .unwrap()
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap(),
    )
    .unwrap();
    let published: Project = serde_json::from_value(bundle["project"].clone()).unwrap();
    let ElementKind::TextBox { format, .. } = &published.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .find(|element| element.id == "embedded-field")
        .unwrap()
        .kind
    else {
        panic!("published embedded field missing");
    };
    assert!(
        format
            .field_options()
            .is_some_and(|options| options.embed_fonts)
    );

    let ElementKind::TextBox { format, .. } = &mut project.symbols[0].timeline.layers[0].keyframes
        [0]
    .elements
    .iter_mut()
    .find(|element| element.id == "embedded-field")
    .unwrap()
    .kind
    else {
        unreachable!();
    };
    format.italic = true;
    assert!(export_warnings(&project).iter().any(|warning| {
        warning.contains("Book Antiqua")
            && warning.contains("Bold Italic")
            && warning.contains("Properties ▸ Embed font ▸ Embedding…")
    }));

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn executable_publish_code_preserves_token_boundaries_templates_and_comments_when_minified() {
    let source = "const plus = x + +y;\nconst minus = x - -y;\nconst template = `first\n// this line is template data\n${value /* expression comment */}\nlast`;\nconst matcher = /\\/\\//;";
    let config = HtmlPublishConfig {
        minify: true,
        strict_mode: true,
        ..HtmlPublishConfig::clean_publish_default()
    };

    let (published, shift) = apply_js_publish_transforms(source, Some(&config));

    assert_eq!(shift.generated_lines, 1);
    assert_eq!(shift.first_line_columns, 0);
    assert_eq!(published, format!("\"use strict\";\n{source}"));
    assert!(published.contains("x + +y"));
    assert!(published.contains("x - -y"));
    assert!(published.contains("// this line is template data"));
    assert!(published.contains("/* expression comment */"));
}

#[test]
fn executable_publish_source_maps_follow_strict_prefix_and_same_line_removal() {
    let source = "const answer = value + 1;";
    let base_map = neoflash_script::build_source_map("author.js", source, source);
    let strict_config = HtmlPublishConfig {
        minify: true,
        strict_mode: true,
        ..HtmlPublishConfig::clean_publish_default()
    };
    let (strict_code, strict_shift) = apply_js_publish_transforms(source, Some(&strict_config));
    let strict_map = apply_publish_source_map_shift(&base_map, strict_shift);

    assert_eq!(strict_code, format!("\"use strict\";\n{source}"));
    assert!(
        strict_map
            .decode_segments()
            .iter()
            .all(|segment| segment.generated_line >= 1)
    );
    let mapped = strict_map
        .lookup_original_position(2, 1)
        .expect("first authored token remains mapped after strict prefix");
    assert_eq!(mapped.source, "author.js");
    assert_eq!((mapped.line, mapped.column), (1, 1));

    let same_line = "\"use strict\"; const answer = value + 1;";
    let same_line_map = neoflash_script::SourceMap::from_segments(
        None,
        vec!["author.js".into()],
        vec![source.into()],
        Vec::new(),
        vec![neoflash_script::SourceMappingSegment {
            generated_line: 0,
            generated_column: "\"use strict\"; ".encode_utf16().count(),
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        }],
    );
    let non_strict_config = HtmlPublishConfig {
        strict_mode: false,
        ..strict_config
    };
    let (non_strict_code, non_strict_shift) =
        apply_js_publish_transforms(same_line, Some(&non_strict_config));
    let non_strict_map = apply_publish_source_map_shift(&same_line_map, non_strict_shift);
    assert_eq!(non_strict_code, source);
    assert_eq!(non_strict_map.decode_segments()[0].generated_column, 0);
}

#[test]
fn html_publish_config_drives_clean_publish_preview_chrome_templates_scale_jpeg_maps_sprites_and_minify()
 {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);

    let mut project = Project::demo();
    project.format_version = 3;
    let mut rgb = image::RgbImage::new(8, 8);
    for (x, y, px) in rgb.enumerate_pixels_mut() {
        *px = image::Rgb([(x * 28) as u8, (y * 28) as u8, ((x + y) * 16) as u8]);
    }
    let mut jpeg_high = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_high, 98);
    enc.encode_image(&image::DynamicImage::ImageRgb8(rgb))
        .unwrap();
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "photo-jpg",
            "images/photo.jpg",
            "image/jpeg",
            &jpeg_high,
        )
        .unwrap(),
    );

    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    // 1. Clean publish vs Preview chrome + individual HUD toggles
    let out_clean = dir.join("out-clean");
    let clean_cfg = HtmlPublishConfig::clean_publish_default();
    export_project_with_config(&project, &out_clean, &rt, &options, &clean_cfg).unwrap();
    let html_clean = fs::read_to_string(out_clean.join("index.html")).unwrap();
    assert!(html_clean.contains("<body data-clean=\"true\">"));
    assert!(html_clean.contains("<footer hidden>"));

    let out_hud = dir.join("out-hud");
    let hud_cfg = HtmlPublishConfig {
        chrome: PlayerChromeConfig {
            clean: false,
            show_fps: false,
            show_dimensions: true,
            show_controls: false,
            show_load_summary: false,
            show_trace: true,
        },
        template: HtmlTemplate::Centered,
        scale_mode: HtmlScaleMode::NoScale,
        stage_align: HtmlStageAlign::BottomRight,
        allow_fullscreen: true,
        acceleration: HtmlAcceleration::GpuWebGl,
        loop_timeline: false,
        autoplay: false,
        preloader: false,
        source_maps: true,
        sprite_sheets: true,
        include_trace: false,
        minify: false,
        jpeg_quality: 30,
        strict_mode: true,
        assets_folder: false,
        include_html: true,
        sequence_policy: SequencePublishPolicy::KeepFrames,
    };
    export_project_with_config(&project, &out_hud, &rt, &options, &hud_cfg).unwrap();
    let html_hud = fs::read_to_string(out_hud.join("index.html")).unwrap();
    assert!(!html_hud.contains("<body data-clean=\"true\">"));
    assert!(html_hud.contains("border-radius: 6px; overflow: hidden;"));
    assert!(html_hud.contains("Ideas in motion · 960×600"));
    assert!(html_hud.contains("id=\"fullscreen-toggle\""));
    assert!(html_hud.contains("<span id=\"status\" hidden></span>"));
    assert!(html_hud.contains("<div class=\"transport\" aria-label=\"Preview controls\" hidden>"));
    assert!(html_hud.contains("<span id=\"load-info\" hidden></span>"));
    assert!(
        !html_hud.contains(
            "id=\"trace\" role=\"status\" aria-live=\"polite\" aria-atomic=\"true\" hidden"
        )
    );
    assert!(html_hud.contains("id=\"loading\" role=\"status\" hidden style=\"display:none\""));

    let bundle_hud_raw = fs::read_to_string(out_hud.join("bundle.js")).unwrap();
    assert!(bundle_hud_raw.contains("\"stage_align\": \"bottom_right\""));
    assert!(bundle_hud_raw.contains("\"allow_fullscreen\": true"));
    assert!(bundle_hud_raw.ends_with(";\n"));
    assert!(
        bundle_hud_raw.lines().count() > 20,
        "minify: false pretty-prints bundle.js"
    );
    assert!(out_hud.join("bundle.js.map").is_file());
    assert!(out_hud.join("sprite_sheet.json").is_file());

    let bundle_json = bundle_hud_raw
        .strip_prefix("globalThis.NeoFlashBundle = ")
        .unwrap()
        .strip_suffix(";\n")
        .unwrap();
    let bundle_val: serde_json::Value = serde_json::from_str(bundle_json).unwrap();
    assert!(
        bundle_val["program"]["code"]
            .as_str()
            .unwrap()
            .ends_with("//# sourceMappingURL=bundle.js.map"),
        "the map belongs to the dynamically evaluated neoflash-project.js program"
    );
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(out_hud.join("bundle.js.map")).unwrap()).unwrap();
    assert_eq!(map["file"], "neoflash-project.js");
    assert_eq!(bundle_val["player_config"]["template"], "centered");
    assert_eq!(bundle_val["player_config"]["scale_mode"], "no_scale");
    assert_eq!(bundle_val["player_config"]["acceleration"], "gpu_web_gl");
    assert_eq!(bundle_val["player_config"]["loop_timeline"], false);
    assert_eq!(bundle_val["player_config"]["autoplay"], false);
    assert_eq!(bundle_val["player_config"]["preloader"], false);
    assert_eq!(bundle_val["player_config"]["include_trace"], false);
    assert_eq!(bundle_val["player_config"]["strict_mode"], true);
    assert_eq!(bundle_val["player_config"]["chrome"]["show_fps"], false);
    assert_eq!(
        bundle_val["player_config"]["chrome"]["show_dimensions"],
        true
    );

    let reencoded_b64 = bundle_val["project"]["resources"][0]["data_base64"]
        .as_str()
        .unwrap();
    let reencoded_bytes = STANDARD.decode(reencoded_b64).unwrap();
    assert!(
        reencoded_bytes.len() < jpeg_high.len(),
        "jpeg_quality=30 re-encodes JPEG to fewer bytes than quality=98 ({} < {})",
        reencoded_bytes.len(),
        jpeg_high.len()
    );

    // Verify background size estimate matches exact on-disk bytes for custom HtmlPublishConfig
    let est_hud =
        estimate_publish_options_cancellable_with_config(&project, &rt, &options, &hud_cfg, || {
            false
        })
        .unwrap()
        .unwrap();
    let totals = |folder: &Path| {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let data = fs::read(entry.unwrap().path()).unwrap();
                (1usize, data.len(), gzip_len(&data))
            })
            .fold((0, 0, 0), |sum, item| {
                (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
            })
    };
    assert_eq!(
        totals(&out_hud),
        (est_hud.files, est_hud.bytes, est_hud.gzip_bytes)
    );

    // 2. include_html: false omits index.html and matches size estimate
    let mut no_html_cfg = clean_cfg.clone();
    no_html_cfg.include_html = false;
    let out_no_html = dir.join("out-no-html");
    export_project_with_config(&project, &out_no_html, &rt, &options, &no_html_cfg).unwrap();
    assert!(!out_no_html.join("index.html").exists());
    assert!(out_no_html.join("bundle.js").is_file());
    let est_no_html = estimate_publish_options_cancellable_with_config(
        &project,
        &rt,
        &options,
        &no_html_cfg,
        || false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        totals(&out_no_html),
        (est_no_html.files, est_no_html.bytes, est_no_html.gzip_bytes)
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compact_and_balanced_packages_embed_file_protocol_wasm_fallback_for_authored_and_classic_fla_scenes()
 {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.join("../..");
    let mut scenes: Vec<(String, Project)> = vec![("demo".into(), Project::demo())];
    for rel in [
        "examples/fla-corpus/authored/nested-playback-as2/nested-playback-as2.fla",
        "examples/fla-corpus/authored/frame-scripts-as3/frame-scripts-as3.fla",
        "examples/Macromedia Flash MX ActionScript_program files/Superclass.fla",
    ] {
        let path = workspace_root.join(rel);
        if path.is_file()
            && let Ok(proj) = neoflash_import::import_project(&path)
        {
            scenes.push((rel.to_owned(), proj));
        }
    }
    assert!(
        scenes.len() >= 3,
        "expected demo plus at least two imported FLA scenes"
    );

    let totals = |folder: &Path| {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let data = fs::read(entry.unwrap().path()).unwrap();
                (1usize, data.len(), gzip_len(&data))
            })
            .fold((0, 0, 0), |sum, item| {
                (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
            })
    };

    for (idx, (label, scene)) in scenes.iter().enumerate() {
        for level in [SizeLevel::Full, SizeLevel::Balanced, SizeLevel::Compact] {
            let out = dir.join(format!("scene-{idx}-{}", level.name()));
            let options = PublishOptions {
                fonts: FontExportPolicy::Embed,
                size: level.clone(),
                audio: AudioExportPolicy::Original,
            };
            let cfg = HtmlPublishConfig::clean_publish_default();
            export_project_with_config(scene, &out, &rt, &options, &cfg)
                .unwrap_or_else(|e| panic!("failed exporting {label} at {}: {e}", level.name()));
            let bindings = fs::read_to_string(out.join(BINDINGS_FILE)).unwrap();
            if level.wasm_inline() {
                assert!(
                    !bindings.contains("NeoFlashWasmBase64"),
                    "Full level inlines wasm in bundle.js, not neoflash_web.js ({label})"
                );
            } else {
                let expected_b64 =
                    STANDARD.encode(fs::read(out.join(WASM_FILE)).expect("wasm file"));
                assert!(
                    bindings.contains(&format!(
                        "globalThis.NeoFlashWasmBase64 = \"{expected_b64}\";"
                    )),
                    "{} level must embed NeoFlashWasmBase64 fallback in neoflash_web.js for direct file:// opening ({label})",
                    level.name()
                );
            }
            let est = estimate_publish_options_cancellable_with_config(
                scene,
                &rt,
                &options,
                &cfg,
                || false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                totals(&out),
                (est.files, est.bytes, est.gzip_bytes),
                "size estimate matches published disk bytes for {label} ({})",
                level.name()
            );
        }
    }

    let shared_dir = dir.join("shared-runtime");
    export_shared_runtime(&shared_dir, &rt).unwrap();
    let shared_bindings = fs::read_to_string(shared_dir.join(BINDINGS_FILE)).unwrap();
    assert!(
        shared_bindings.contains("globalThis.NeoFlashWasmBase64 = \"AGFzbQEAAAA=\";"),
        "shared runtime also embeds NeoFlashWasmBase64 for local relative-path file:// setups"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn export_desktop_app_creates_macos_app_bundle_and_windows_launcher() {
    let dir = temp();
    let rt = dir.join("runtime");
    runtime(&rt);

    let project = Project::demo();
    let out_desktop = dir.join("desktop-out");
    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };
    let app_bundle = export_desktop_app(
        &project,
        &out_desktop,
        &rt,
        &options,
        &HtmlPublishConfig::clean_publish_default(),
    )
    .unwrap();
    assert!(app_bundle.ends_with("Ideas in motion.app"));
    assert!(app_bundle.join("Contents/Info.plist").is_file());
    assert!(app_bundle.join("Contents/MacOS/Ideas in motion").is_file());
    assert!(
        app_bundle
            .join("Contents/Resources/web/index.html")
            .is_file()
    );
    assert!(out_desktop.join("Launch Ideas in motion.bat").is_file());

    let html = fs::read_to_string(app_bundle.join("Contents/Resources/web/index.html")).unwrap();
    assert!(html.contains("<body data-clean=\"true\">"));

    fs::remove_dir_all(dir).unwrap();
}
