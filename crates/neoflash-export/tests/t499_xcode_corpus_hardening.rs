//! `[T-499]` Comprehensive Xcode / Swift 6 (`@MainActor`) corpus, `xcodebuild`, runtime pixel & CLI suite.

use neoflash_core::Project;
use neoflash_export::{XcodeExportOptions, export_xcode_project};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static HEAVY_SWIFT_LOCK: Mutex<()> = Mutex::new(());

fn should_skip_xcodebuild() -> bool {
    if std::env::var("NEOFLASH_SKIP_XCODEBUILD")
        .ok()
        .is_some_and(|v| !v.trim().is_empty() && v.trim() != "0")
    {
        return true;
    }
    !Command::new("/usr/bin/xcodebuild")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn sdk_path(sdk_name: &str) -> Option<String> {
    let out = Command::new("/usr/bin/xcrun")
        .args(["--sdk", sdk_name, "--show-sdk-path"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let trimmed = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn ios_simulator_destination_available(xcodeproj_path: &Path, scheme: &str) -> bool {
    let Ok(out) = Command::new("/usr/bin/xcodebuild")
        .arg("-project")
        .arg(xcodeproj_path)
        .arg("-scheme")
        .arg(scheme)
        .arg("-showdestinations")
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("Unable to find a destination") {
        return false;
    }
    let available = stdout
        .split("Ineligible destinations")
        .next()
        .unwrap_or(&stdout);
    available.lines().any(|line| {
        line.contains("platform:iOS Simulator")
            && line.contains("Any iOS Simulator Device")
            && !line.contains("error:")
            && !line.contains("not installed")
    })
}

fn collect_category_example_projects(repo_root: &Path, categories: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk_dir(&p, out);
            } else if p.extension().is_some_and(|ext| ext == "nflash") {
                out.push(p);
            }
        }
    }
    for cat in categories {
        walk_dir(&repo_root.join("examples").join(cat), &mut out);
    }
    out
}

fn collect_tracked_example_projects(repo_root: &Path) -> Vec<PathBuf> {
    collect_category_example_projects(
        repo_root,
        &[
            "games",
            "video-and-audio",
            "animation",
            "as2",
            "as3",
            "graphics-and-filters",
            "ui-and-text",
            "web-and-ads",
        ],
    )
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> (bool, bool, u128, String) {
    let t0 = Instant::now();
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return (false, false, 0, "failed to spawn swiftc".into());
    };
    loop {
        if let Ok(Some(_status)) = child.try_wait() {
            let out = child.wait_with_output().expect("collect swiftc output");
            let elapsed_ms = t0.elapsed().as_millis();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            return (out.status.success(), false, elapsed_ms, stderr);
        }
        if t0.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return (
                false,
                true,
                t0.elapsed().as_millis(),
                format!(
                    "TIMEOUT after {}s (exponential Swift typecheck!)",
                    timeout.as_secs()
                ),
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn warmup_swift_module_cache(
    mod_cache: &Path,
    macos_sdk: Option<&str>,
    ios_sim_sdk: Option<&str>,
    scratch_dir: &Path,
) {
    if let Some(sdk) = macos_sdk {
        let warm_file = scratch_dir.join("warmup_macos.swift");
        fs::write(
            &warm_file,
            "import Foundation\nimport SwiftUI\nimport AppKit\nimport AVFoundation\n",
        )
        .expect("write macOS warmup");
        let _ = Command::new("/usr/bin/xcrun")
            .args([
                "--sdk",
                "macosx",
                "swiftc",
                "-sdk",
                sdk,
                "-typecheck",
                "-swift-version",
                "6",
                "-module-cache-path",
            ])
            .arg(mod_cache)
            .arg(&warm_file)
            .output();
    }
    if let Some(sdk) = ios_sim_sdk {
        let warm_file = scratch_dir.join("warmup_ios.swift");
        fs::write(
            &warm_file,
            "import Foundation\nimport SwiftUI\nimport UIKit\nimport AVFoundation\n",
        )
        .expect("write iOS warmup");
        let _ = Command::new("/usr/bin/xcrun")
            .args([
                "--sdk",
                "iphonesimulator",
                "swiftc",
                "-sdk",
                sdk,
                "-target",
                "arm64-apple-ios17.0-simulator",
                "-typecheck",
                "-swift-version",
                "6",
                "-module-cache-path",
            ])
            .arg(mod_cache)
            .arg(&warm_file)
            .output();
    }
}

#[test]
fn t499_all_games_video_audio_and_animation_compile_swift6_strict_concurrency() {
    if should_skip_xcodebuild() {
        return;
    }
    let _heavy_guard = HEAVY_SWIFT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(macos_sdk) = sdk_path("macosx") else {
        return;
    };
    let ios_sim_sdk = sdk_path("iphonesimulator");

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let targets =
        collect_category_example_projects(&repo_root, &["games", "video-and-audio", "animation"]);
    assert_eq!(
        targets.len(),
        25,
        "expected 11 games + 3 video-and-audio + 11 animation examples"
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    let mod_cache = tmp.path().join("ModuleCache");
    let mac_mod_dir = tmp.path().join("NeoFlashKitMac");
    let ios_mod_dir = tmp.path().join("NeoFlashKitSim");
    fs::create_dir_all(&mod_cache).expect("create ModuleCache");
    fs::create_dir_all(&mac_mod_dir).expect("create NeoFlashKitMac");
    fs::create_dir_all(&ios_mod_dir).expect("create NeoFlashKitSim");
    warmup_swift_module_cache(
        &mod_cache,
        Some(&macos_sdk),
        ios_sim_sdk.as_deref(),
        tmp.path(),
    );

    // Precompile the shared `NeoFlashKit` runtime once per SDK (`macosx` + `iphonesimulator`) so
    // the 25-project corpus sweep does not re-parse 15k lines of runtime 50 times on 3-vCPU CI VMs,
    // while also verifying unedited single-module compilation on `targets[0]`.
    let seed_dir = tmp.path().join("seed_kit");
    let seed_bytes = fs::read(&targets[0]).expect("read seed .nflash");
    let seed_proj = Project::from_nflash_bytes(&seed_bytes)
        .expect("decode seed .nflash")
        .with_resolved_script_language()
        .into_owned();
    let seed_opts = XcodeExportOptions {
        base_dir: targets[0].parent().map(Path::to_path_buf),
        ..XcodeExportOptions::default()
    };
    let seed_report =
        export_xcode_project(&seed_proj, &seed_dir, &seed_opts, None).expect("export seed");
    let kit_files: Vec<PathBuf> = seed_report
        .generated_swift_files
        .iter()
        .filter(|p| {
            p.parent()
                .and_then(|d| d.file_name())
                .is_some_and(|n| n == "NeoFlashKit")
        })
        .cloned()
        .collect();
    assert!(
        !kit_files.is_empty(),
        "expected NeoFlashKit Swift files in seed export"
    );

    let mut build_mac_kit = Command::new("/usr/bin/xcrun");
    build_mac_kit
        .args([
            "--sdk",
            "macosx",
            "swiftc",
            "-sdk",
            &macos_sdk,
            "-parse-as-library",
            "-emit-module",
            "-module-name",
            "NeoFlashKit",
            "-emit-module-path",
        ])
        .arg(mac_mod_dir.join("NeoFlashKit.swiftmodule"))
        .args([
            "-warnings-as-errors",
            "-swift-version",
            "6",
            "-strict-concurrency=complete",
            "-module-cache-path",
        ])
        .arg(&mod_cache);
    for kf in &kit_files {
        build_mac_kit.arg(kf);
    }
    let (mac_kit_ok, _, _, mac_kit_err) = run_with_timeout(build_mac_kit, Duration::from_secs(120));
    assert!(
        mac_kit_ok,
        "NeoFlashKit macosx module build failed:\n{mac_kit_err}"
    );

    if let Some(ios_sdk) = &ios_sim_sdk {
        let mut build_ios_kit = Command::new("/usr/bin/xcrun");
        build_ios_kit
            .args([
                "--sdk",
                "iphonesimulator",
                "swiftc",
                "-sdk",
                ios_sdk,
                "-target",
                "arm64-apple-ios17.0-simulator",
                "-parse-as-library",
                "-emit-module",
                "-module-name",
                "NeoFlashKit",
                "-emit-module-path",
            ])
            .arg(ios_mod_dir.join("NeoFlashKit.swiftmodule"))
            .args([
                "-warnings-as-errors",
                "-swift-version",
                "6",
                "-strict-concurrency=complete",
                "-module-cache-path",
            ])
            .arg(&mod_cache);
        for kf in &kit_files {
            build_ios_kit.arg(kf);
        }
        let (ios_kit_ok, _, _, ios_kit_err) =
            run_with_timeout(build_ios_kit, Duration::from_secs(120));
        assert!(
            ios_kit_ok,
            "NeoFlashKit iphonesimulator module build failed:\n{ios_kit_err}"
        );
    }

    // Verify unedited single-module compilation on `seed_report` (`targets[0]`)
    let mut seed_single_mod = Command::new("/usr/bin/xcrun");
    seed_single_mod
        .args([
            "--sdk",
            "macosx",
            "swiftc",
            "-sdk",
            &macos_sdk,
            "-typecheck",
            "-warnings-as-errors",
            "-swift-version",
            "6",
            "-strict-concurrency=complete",
            "-module-cache-path",
        ])
        .arg(&mod_cache);
    for f in &seed_report.generated_swift_files {
        seed_single_mod.arg(f);
    }
    for f in &seed_report.app_swift_files {
        seed_single_mod.arg(f);
    }
    let (seed_ok, _, _, seed_err) = run_with_timeout(seed_single_mod, Duration::from_secs(120));
    assert!(
        seed_ok,
        "unedited single-module seed typecheck failed:\n{seed_err}"
    );

    let worker_count = (std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        / 2)
    .clamp(1, 4);
    let next_idx = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());

    std::thread::scope(|s| {
        for _ in 0..worker_count {
            s.spawn(|| {
                loop {
                    let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                    if idx >= targets.len() {
                        break;
                    }
                    let path = &targets[idx];
                    let rel = path
                        .strip_prefix(&repo_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    let bytes = fs::read(path).expect("read .nflash");
                    let proj = Project::from_nflash_bytes(&bytes)
                        .expect("decode .nflash")
                        .with_resolved_script_language()
                        .into_owned();
                    let out_dir = tmp.path().join(format!("proj_{idx:02}"));
                    let opts = XcodeExportOptions {
                        base_dir: path.parent().map(Path::to_path_buf),
                        ..XcodeExportOptions::default()
                    };
                    let report = match export_xcode_project(&proj, &out_dir, &opts, None) {
                        Ok(r) => r,
                        Err(err) => {
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("{rel}: export error: {err}"));
                            continue;
                        }
                    };

                    let stage_dir = tmp.path().join(format!("stage_{idx:02}"));
                    fs::create_dir_all(&stage_dir).expect("create stage_dir");
                    let mut staged_scene_files = Vec::new();
                    for (f_idx, f) in report
                        .generated_swift_files
                        .iter()
                        .filter(|p| {
                            !p.parent()
                                .and_then(|d| d.file_name())
                                .is_some_and(|n| n == "NeoFlashKit")
                        })
                        .chain(report.app_swift_files.iter())
                        .enumerate()
                    {
                        let raw = fs::read_to_string(f).expect("read generated/app swift");
                        let fname = f
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| format!("file_{f_idx}.swift"));
                        let dst = stage_dir.join(format!("{f_idx:02}_{fname}"));
                        fs::write(&dst, format!("import NeoFlashKit\n{raw}"))
                            .expect("write staged swift");
                        staged_scene_files.push(dst);
                    }

                    let mut mac_cmd = Command::new("/usr/bin/xcrun");
                    mac_cmd.args([
                        "--sdk",
                        "macosx",
                        "swiftc",
                        "-sdk",
                        &macos_sdk,
                        "-typecheck",
                        "-warnings-as-errors",
                        "-swift-version",
                        "6",
                        "-strict-concurrency=complete",
                        "-I",
                    ]);
                    mac_cmd
                        .arg(&mac_mod_dir)
                        .arg("-module-cache-path")
                        .arg(&mod_cache);
                    for f in &staged_scene_files {
                        mac_cmd.arg(f);
                    }
                    let (mac_ok, _, _, mac_err) =
                        run_with_timeout(mac_cmd, Duration::from_secs(90));
                    if !mac_ok {
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("{rel} [macosx]:\n{mac_err}"));
                    }

                    if let Some(ios_sdk) = &ios_sim_sdk {
                        let mut ios_cmd = Command::new("/usr/bin/xcrun");
                        ios_cmd.args([
                            "--sdk",
                            "iphonesimulator",
                            "swiftc",
                            "-sdk",
                            ios_sdk,
                            "-target",
                            "arm64-apple-ios17.0-simulator",
                            "-typecheck",
                            "-warnings-as-errors",
                            "-swift-version",
                            "6",
                            "-strict-concurrency=complete",
                            "-I",
                        ]);
                        ios_cmd
                            .arg(&ios_mod_dir)
                            .arg("-module-cache-path")
                            .arg(&mod_cache);
                        for f in &staged_scene_files {
                            ios_cmd.arg(f);
                        }
                        let (ios_ok, _, _, ios_err) =
                            run_with_timeout(ios_cmd, Duration::from_secs(90));
                        if !ios_ok {
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("{rel} [iphonesimulator]:\n{ios_err}"));
                        }
                    }
                }
            });
        }
    });

    let errs = failures.into_inner().unwrap();
    assert!(
        errs.is_empty(),
        "expected all 25 games/video-and-audio/animation examples to compile cleanly in Swift 6, got {} failures:\n{}",
        errs.len(),
        errs.join("\n---\n")
    );
}

#[test]
fn t499_flagship_xcodebuild_and_native_macos_pixel_and_runtime_execution() {
    if should_skip_xcodebuild() {
        return;
    }
    let _heavy_guard = HEAVY_SWIFT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(macos_sdk) = sdk_path("macosx") else {
        return;
    };
    let ios_sim_sdk = sdk_path("iphonesimulator");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = tempfile::tempdir().expect("tempdir");

    let export_one = |rel: &str, slug: &str| {
        let src_path = repo_root.join(rel);
        let bytes = fs::read(&src_path).expect("read example");
        let proj = Project::from_nflash_bytes(&bytes)
            .expect("parse example")
            .with_resolved_script_language()
            .into_owned();
        let out_dir = tmp.path().join(slug);
        let opts = XcodeExportOptions {
            bundle_identifier_prefix: format!("cc.redinger.t499.{slug}"),
            base_dir: src_path.parent().map(Path::to_path_buf),
            ..XcodeExportOptions::default()
        };
        let report = export_xcode_project(&proj, &out_dir, &opts, None).expect("export xcode");
        (out_dir, report)
    };

    // 1. Real `/usr/bin/xcodebuild` across AS3 (`xcom-as3`), TypeScript (`simcity-ts`), and AS2 (`doom-as2`)
    let mut exported_flagships = Vec::new();
    for (rel, slug) in [
        ("examples/games/xcom-as3.nflash", "xcom"),
        ("examples/games/simcity-ts.nflash", "simcity"),
        ("examples/games/doom-as2.nflash", "doom"),
    ] {
        let (out_dir, report) = export_one(rel, slug);
        let derived_mac = tmp.path().join(format!("dd_{slug}_mac"));
        let mac_build = Command::new("/usr/bin/xcodebuild")
            .arg("-project")
            .arg(&report.xcodeproj_path)
            .arg("-scheme")
            .arg(&report.scheme_name)
            .args([
                "-configuration",
                "Debug",
                "-sdk",
                "macosx",
                "-arch",
                "arm64",
                "-derivedDataPath",
            ])
            .arg(&derived_mac)
            .args([
                "CODE_SIGNING_ALLOWED=NO",
                "SWIFT_STRICT_CONCURRENCY=complete",
                "build",
            ])
            .output()
            .expect("spawn macOS xcodebuild");
        assert!(
            mac_build.status.success(),
            "macOS xcodebuild failed for {rel}:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&mac_build.stdout),
            String::from_utf8_lossy(&mac_build.stderr)
        );

        if ios_sim_sdk.is_some()
            && ios_simulator_destination_available(&report.xcodeproj_path, &report.scheme_name)
        {
            let derived_ios = tmp.path().join(format!("dd_{slug}_ios"));
            let ios_build = Command::new("/usr/bin/xcodebuild")
                .arg("-project")
                .arg(&report.xcodeproj_path)
                .arg("-scheme")
                .arg(&report.scheme_name)
                .args([
                    "-configuration",
                    "Debug",
                    "-sdk",
                    "iphonesimulator",
                    "-destination",
                    "generic/platform=iOS Simulator",
                    "-derivedDataPath",
                ])
                .arg(&derived_ios)
                .args([
                    "CODE_SIGNING_ALLOWED=NO",
                    "SWIFT_STRICT_CONCURRENCY=complete",
                    "build",
                ])
                .output()
                .expect("spawn iOS Simulator xcodebuild");
            assert!(
                ios_build.status.success(),
                "iOS Simulator xcodebuild failed for {rel}:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&ios_build.stdout),
                String::from_utf8_lossy(&ios_build.stderr)
            );
        }
        exported_flagships.push((slug, out_dir, report));
    }

    // 2. Native macOS `@MainActor` execution + CoreGraphics RGBA8 pixel verification across
    //    `doom-as2`, `xcom-as3`, `simcity-ts`, `fallout-as2`, `saved-game`, and `video-playback`!
    let mod_cache = tmp.path().join("ModuleCache");
    fs::create_dir_all(&mod_cache).expect("create ModuleCache");

    let run_native_verifier = |out_dir: &Path,
                               report: &neoflash_export::XcodeExportReport,
                               slug: &str,
                               extra_swift_checks: &str| {
        let main_swift = tmp.path().join(format!("Verify_{slug}.swift"));
        let bin_path = tmp.path().join(format!("VerifyBin_{slug}"));
        let res_dir = out_dir.join("Generated/Resources");
        let res_dir_lit = format!("{:?}", res_dir.to_string_lossy());
        let harness = format!(
            r#"import Foundation
import CoreGraphics

@main
@MainActor
struct VerifyMain {{
    static func countStagePixels(_ stage: NFStage, width: Int, height: Int) -> (Int, Int) {{
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        var buf = [UInt8](repeating: 0, count: width * height * 4)
        let bg = stage.backgroundColor
        buf.withUnsafeMutableBytes {{ rawPtr in
            guard let base = rawPtr.baseAddress,
                  let ctx = CGContext(
                      data: base,
                      width: width,
                      height: height,
                      bitsPerComponent: 8,
                      bytesPerRow: width * 4,
                      space: colorSpace,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else {{
                fatalError("Failed to create CGContext")
            }}
            ctx.translateBy(x: 0, y: CGFloat(height))
            ctx.scaleBy(x: 1, y: -1)
            NFRenderer.render(
                stage: stage,
                in: ctx,
                viewSize: CGSize(width: width, height: height),
                drawsBackground: true
            )
        }}
        var nonBg = 0
        var buckets = Set<UInt16>()
        for i in stride(from: 0, to: buf.count, by: 4) {{
            let r = buf[i]
            let g = buf[i + 1]
            let b = buf[i + 2]
            if abs(Int(r) - Int(bg.r)) > 4 || abs(Int(g) - Int(bg.g)) > 4 || abs(Int(b) - Int(bg.b)) > 4 {{
                nonBg += 1
            }}
            let q = (UInt16(r >> 4) << 8) | (UInt16(g >> 4) << 4) | UInt16(b >> 4)
            buckets.insert(q)
        }}
        return (nonBg, buckets.count)
    }}

    static func main() {{
        NFResourceCache.shared.resourceDirectory = URL(fileURLWithPath: {res_dir_lit})
        let host = AppSwiftHost()
        let scene = MainScene()
        let stage = NFStage(scene: scene)
        stage.host = host
        let w = max(320, Int(stage.stageWidth.rounded()))
        let h = max(240, Int(stage.stageHeight.rounded()))
        stage.updateViewport(width: Double(w), height: Double(h), safeTop: 0, safeLeft: 0, safeBottom: 0, safeRight: 0)
        {extra_swift_checks}
        print("NATIVE_VERIFY_OK:{slug}")
    }}
}}
"#
        );
        fs::write(&main_swift, harness).expect("write verifier swift");

        let app_host = out_dir.join("App/AppSwiftHost.swift");
        let mut compile = Command::new("/usr/bin/xcrun");
        compile.args([
            "--sdk",
            "macosx",
            "swiftc",
            "-sdk",
            &macos_sdk,
            "-warnings-as-errors",
            "-swift-version",
            "6",
            "-strict-concurrency=complete",
            "-module-cache-path",
        ]);
        compile.arg(&mod_cache);
        for f in &report.generated_swift_files {
            compile.arg(f);
        }
        compile.arg(&app_host);
        compile.arg(&main_swift);
        compile.arg("-o").arg(&bin_path);
        let comp_out = compile.output().expect("compile native verifier");
        assert!(
            comp_out.status.success(),
            "swiftc native verifier compile failed for {slug}:\n{}",
            String::from_utf8_lossy(&comp_out.stderr)
        );

        let run_out = Command::new(&bin_path)
            .output()
            .expect("execute native verifier");
        let stdout = String::from_utf8_lossy(&run_out.stdout);
        let stderr = String::from_utf8_lossy(&run_out.stderr);
        assert!(
            run_out.status.success() && stdout.contains(&format!("NATIVE_VERIFY_OK:{slug}")),
            "native verifier execution failed for {slug}:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    };

    for (slug, out_dir, report) in &exported_flagships {
        let extra = if *slug == "xcom" {
            r#"
        let db = SquadDatabase()
        assert(db.missionTitle == "Sector 7 // OP NIGHTFALL", "Expected E4X missionTitle, got \(db.missionTitle)")
        assert(db.weaponStats["plasma"].asInt == 16, "Expected E4X weaponStats[plasma] == 16, got \(db.weaponStats["plasma"].asInt)")
        for _ in 0..<6 {
            _ = stage.advance(deltaSeconds: 1.0 / 30.0)
        }
        let (nonBg, colors) = countStagePixels(stage, width: w, height: h)
        assert(nonBg >= 5_000, "Expected >= 5000 rendered foreground pixels, got \(nonBg)")
        assert(colors >= 4, "Expected >= 4 distinct color buckets, got \(colors)")
"#
        } else {
            r#"
        for _ in 0..<6 {
            _ = stage.advance(deltaSeconds: 1.0 / 30.0)
        }
        let (nonBg, colors) = countStagePixels(stage, width: w, height: h)
        assert(nonBg >= 5_000, "Expected >= 5000 rendered foreground pixels, got \(nonBg)")
        assert(colors >= 4, "Expected >= 4 distinct color buckets, got \(colors)")
"#
        };
        run_native_verifier(out_dir, report, slug, extra);
    }

    let (fallout_dir, fallout_rep) = export_one("examples/games/fallout-as2.nflash", "fallout");
    run_native_verifier(
        &fallout_dir,
        &fallout_rep,
        "fallout",
        r#"
        for _ in 0..<6 {
            _ = stage.advance(deltaSeconds: 1.0 / 30.0)
        }
        let (nonBg, colors) = countStagePixels(stage, width: w, height: h)
        assert(nonBg >= 5_000, "Expected >= 5000 Fallout foreground pixels, got \(nonBg)")
        assert(colors >= 4, "Expected >= 4 Fallout color buckets, got \(colors)")
"#,
    );

    let (saved_dir, saved_rep) = export_one("examples/games/saved-game.nflash", "savedgame");
    run_native_verifier(
        &saved_dir,
        &saved_rep,
        "savedgame",
        r#"
        assert(scene.restores == true, "Expected cyclic AMF round-trip restores == true")
        assert(scene.malformedPacketRecovered == true, "Expected malformed AMF RangeError recovery")
        scene.collect()
        assert(scene.score() >= 25, "Expected collected score >= 25, got \(scene.score())")
        let (nonBg, _) = countStagePixels(stage, width: w, height: h)
        assert(nonBg >= 1_000, "Expected rendered saved-game UI pixels, got \(nonBg)")
"#,
    );

    let (video_dir, video_rep) = export_one(
        "examples/video-and-audio/video-playback.nflash",
        "videoplay",
    );
    run_native_verifier(
        &video_dir,
        &video_rep,
        "videoplay",
        r#"
        assert(scene.ready == true, "Expected NetConnection.Connect.Success -> scene.ready")
        scene.startVideo()
        assert(scene.plays == 1, "Expected scene.plays == 1")
        assert((scene.stream?.videoWidth ?? 0) > 0, "Expected decoded MP4 videoWidth > 0")
        for _ in 0..<4 {
            _ = stage.advance(deltaSeconds: 1.0 / 24.0)
        }
        let (nonBg, colors) = countStagePixels(stage, width: w, height: h)
        assert(nonBg >= 5_000, "Expected rendered video-playback pixels, got \(nonBg)")
        assert(colors >= 3, "Expected >= 3 color buckets on video-playback stage")
"#,
    );
}

#[test]
fn t499_cli_binary_nflashb1_bundle_loads_and_exports_xcode() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src_bytes =
        fs::read(repo_root.join("examples/games/tetris-as3.nflash")).expect("read tetris-as3");
    let mut proj = Project::from_nflash_bytes(&src_bytes).expect("parse tetris-as3");
    proj.set_large_bundle(true);
    proj.upgrade_format_for_features();
    let bin_bytes = proj
        .to_nflash_bytes()
        .expect("encode NFLASHB1 binary bundle");
    assert!(
        bin_bytes.starts_with(b"NFLASHB1"),
        "expected NFLASHB1 magic header"
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    let bin_input = tmp.path().join("tetris-binary.nflash");
    fs::write(&bin_input, &bin_bytes).expect("write NFLASHB1 file");
    let out_dir = tmp.path().join("TetrisBinaryXcode");

    let cli_out = Command::new(env!("CARGO_BIN_EXE_neoflash-export"))
        .arg("xcode")
        .arg(&bin_input)
        .arg("--output")
        .arg(&out_dir)
        .arg("--bundle-id-prefix")
        .arg("cc.redinger.nflashb1")
        .output()
        .expect("run neoflash-export xcode on NFLASHB1");
    assert!(
        cli_out.status.success(),
        "neoflash-export xcode failed on NFLASHB1:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cli_out.stdout),
        String::from_utf8_lossy(&cli_out.stderr)
    );
    assert!(
        out_dir
            .join("Generated/Scenes/GeneratedScenes.swift")
            .is_file(),
        "expected GeneratedScenes.swift from NFLASHB1 CLI export"
    );

    if Path::new("/usr/bin/plutil").is_file() {
        let pbxproj = fs::read_dir(&out_dir)
            .expect("read out_dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "xcodeproj"))
            .expect("find .xcodeproj")
            .join("project.pbxproj");
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&pbxproj)
            .output()
            .expect("plutil -lint");
        assert!(lint.status.success(), "pbxproj failed plutil -lint");
    }
}

#[test]
fn t499_round1_audit_remediations_destination_safety_loop_closures_and_math() {
    use neoflash_core::{ResourceUnit, ScriptLanguage, SourceUnit};
    use neoflash_export::{pascal_project_name, preflight_xcode_output_dir};

    // 1. `[P0-01]` & `[P2-04]`: Cross-platform destination ownership & Win32 reserved stems
    assert_eq!(pascal_project_name("con"), "NFCon");
    assert_eq!(pascal_project_name("NUL"), "NFNUL");
    assert_eq!(pascal_project_name("com1"), "NFCom1");

    let tmp = tempfile::tempdir().expect("tempdir");
    // Unowned directory containing an unrelated `Generated/` folder MUST be rejected!
    let unowned_dir = tmp.path().join("unrelated_ios_repo");
    fs::create_dir_all(unowned_dir.join("Generated")).expect("mkdir Generated");
    fs::write(unowned_dir.join("Generated/proto.swift"), "// user code").expect("write proto");
    assert!(
        preflight_xcode_output_dir(&unowned_dir).is_err(),
        "must reject unowned directory containing unrelated Generated/"
    );
    assert!(
        export_xcode_project(
            &Project::demo(),
            &unowned_dir,
            &XcodeExportOptions::default(),
            None
        )
        .is_err(),
        "export_xcode_project must refuse unowned directory without deleting Generated/"
    );
    assert!(
        unowned_dir.join("Generated/proto.swift").is_file(),
        "unrelated Generated/proto.swift must remain untouched"
    );

    // Valid NeoFlash export writes `.neoflash-xcode-export.json`, re-exports cleanly, and prefixes Win32 `CON.png` -> `_CON.png`
    let mut demo = Project::demo();
    demo.resources.push(ResourceUnit {
        id: "res-con".into(),
        path: "images/CON.png".into(),
        media_type: "image/png".into(),
        data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".into(),
        bitmap_export: None,
        audio_export: None,
    });
    let owned_dir = tmp.path().join("owned_xcode_out");
    let first_rep = export_xcode_project(&demo, &owned_dir, &XcodeExportOptions::default(), None)
        .expect("first export");
    assert!(
        owned_dir
            .join("Generated/.neoflash-xcode-export.json")
            .is_file()
    );
    assert!(
        first_rep
            .resource_files
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("_CON.png")),
        "expected Win32 reserved stem CON.png -> _CON.png, got {:?}",
        first_rep.resource_files
    );
    preflight_xcode_output_dir(&owned_dir).expect("re-export preflight on NeoFlash-owned dir");

    #[cfg(unix)]
    {
        let sym_target = tmp.path().join("external_app_dir");
        fs::create_dir_all(&sym_target).expect("mkdir external");
        let poisoned = tmp.path().join("poisoned_out");
        export_xcode_project(
            &Project::demo(),
            &poisoned,
            &XcodeExportOptions::default(),
            None,
        )
        .expect("seed poisoned");
        fs::remove_dir_all(poisoned.join("App")).expect("rm App");
        std::os::unix::fs::symlink(&sym_target, poisoned.join("App")).expect("symlink App");
        assert!(
            preflight_xcode_output_dir(&poisoned).is_err(),
            "must reject symlinked App/ directory"
        );
    }

    if should_skip_xcodebuild() {
        return;
    }
    let _heavy_guard = HEAVY_SWIFT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(macos_sdk) = sdk_path("macosx") else {
        return;
    };

    // 2. `[P0-04]`, `[P1-01]`, `[P1-02]`, `[P1-03]`, `[P1-04]`, `[P1-05]`, `[P1-06]`, `[P2-02]`, `[P3-01]`
    //    Compile & execute TypeScript + AS3 edge cases under `-warnings-as-errors -strict-concurrency=complete`!
    let mut ts_proj = Project::demo();
    ts_proj.script_language = Some(ScriptLanguage::TypeScript);
    ts_proj.sources.push(SourceUnit {
        id: "src-audit-ts".into(),
        path: "AuditSuite.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: r#"
export class AuditSuite {
    public selectedLog: string[] = [];
    public imulVal: number = 0;
    public shiftVal: number = 0;
    public swappedA: number = 0;
    public swappedB: number = 0;

    public runChecks(): string {
        const callbacks: Array<() => void> = [];
        const tools = ["residential", "commercial", "industrial", "bulldoze"];
        for (let i = 0; i < tools.length; i++) {
            callbacks.push(() => {
                this.selectedLog.push(tools[i]);
            });
        }
        for (let i = 0; i < callbacks.length; i++) {
            callbacks[i]();
        }
        let branchOut = "";
        if (callbacks.length > 0) {
            const localVal = 42;
            branchOut += String(localVal);
        } else {
            const localVal = "idle";
            branchOut += localVal;
        }
        let a = 10;
        let b = 20;
        [a, b] = [b, a];
        this.swappedA = a;
        this.swappedB = b;
        this.imulVal = Math.imul(0x7fffffff, 0x7fffffff);
        const argb = -15654349; // 0xFF112233 as signed 32-bit int
        this.shiftVal = argb >>> 24;
        return this.selectedLog.join(",") + "|" + branchOut;
    }
}
"#
        .into(),
    });
    let ts_dir = tmp.path().join("ts_audit_proj");
    let ts_rep = export_xcode_project(&ts_proj, &ts_dir, &XcodeExportOptions::default(), None)
        .expect("export ts_audit_proj");

    let mut as3_proj = Project::demo();
    as3_proj.script_language = Some(ScriptLanguage::ActionScript3);
    as3_proj.sources.push(SourceUnit {
        id: "src-audit-as3".into(),
        path: "VecAndMathProbe.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package {
    import flash.display.Sprite;
    import flash.events.Event;
    public class VecAndMathProbe extends Sprite {
        public var visMap:Vector.<int> = new Vector.<int>();
        public var capturedElem:int = -1;

        public function exercise(defer:int = 0):int {
            this.visMap.push(0);
            this.visMap.push(0);
            this.visMap.push(0);
            var vVec:Vector.<int> = this.visMap;
            vVec[1] = 77;
            this.addEventListener(Event.ENTER_FRAME, function(defer:Event):void {
                capturedElem = vVec[1] + int(defer != null ? 1 : 0);
            });
            this.dispatchEvent(new Event(Event.ENTER_FRAME));
            var seed:int = 12345;
            for (var k:int = 0; k < 8; k++) {
                seed = seed * 1664525 + 1013904223;
            }
            var zeroRem:int = seed % 0;
            var u:uint = 0;
            u--;
            var uAsInt:int = int(u);
            var negU:Number = -u;
            var uIdx:uint = 1;
            var uPlusOne:int = u + 1;
            if (u != 0xFFFFFFFF || uAsInt != -1 || negU != -4294967295.0 || uPlusOne != 0 || this.visMap[uIdx] != 77) {
                return -999;
            }
            return capturedElem + zeroRem + defer;
        }
    }
}
"#
        .into(),
    });
    let as3_dir = tmp.path().join("as3_audit_proj");
    let as3_rep = export_xcode_project(&as3_proj, &as3_dir, &XcodeExportOptions::default(), None)
        .expect("export as3_audit_proj");

    let mod_cache = tmp.path().join("AuditModCache");
    fs::create_dir_all(&mod_cache).expect("mkdir AuditModCache");

    // Compile and run TS verifier with `-warnings-as-errors`!
    let ts_main = tmp.path().join("VerifyTsAudit.swift");
    fs::write(
        &ts_main,
        r#"import Foundation
@main
@MainActor
struct VerifyTsAudit {
    static func main() {
        let s = AuditSuite()
        let res = s.runChecks()
        assert(res == "residential,commercial,industrial,bulldoze|42", "loop closure capture mismatch: \(res)")
        assert(s.swappedA == 20 && s.swappedB == 10, "destructuring swap failed: \(s.swappedA), \(s.swappedB)")
        assert(s.imulVal == 1, "Math.imul failed: \(s.imulVal)")
        assert(s.shiftVal == 255, "unsigned >>> 24 failed: \(s.shiftVal)")

        // Cyclic NFValue == & NFJSON.stringify + huge/NaN NFBitmapData bounds (`[P1-05]`, `[P1-06]`)
        let cycA: NFValue = .fromObject([])
        cycA["self"] = cycA
        let cycB: NFValue = .fromObject([])
        cycB["self"] = cycB
        assert((cycA == cycB) == false, "distinct cyclic NFObjectBox == must be false without stack overflow")
        assert((cycA == cycA) == true, "identical cyclic NFObjectBox == must be true")
        let jsonStr = NFJSON.stringify(cycA)
        assert(!jsonStr.isEmpty, "NFJSON.stringify on cyclic object must not crash")

        let bmd = NFBitmapData(Double.nan, Double.infinity, true, 1e30)
        bmd.copyChannel(bmd, NFRectangle(-1e20, -1e20, 1e30, 1e30), NFPoint(1e25, -1e25), 1, 2)
        let hits = bmd.threshold(bmd, NFRectangle(0, 0, 1e30, 1e30), NFPoint(0, 0), "==", 0)
        _ = hits
        assert("abc".charAt(1e30) == "" && "abc".charAt(Double.nan) == "a", "String.charAt clamp failed")
        assert("abc".charCodeAt(1e30).isNaN && "abc".charCodeAt(Double.nan) == 97.0, "String.charCodeAt clamp failed")
        assert("abcdef".substr(1e30, 2) == "" && "abcdef".substring(1e30, -1e30) == "abcdef", "String.substr/substring clamp failed")
        assert("abcdef".slice(1e30, -1e30) == "", "String.slice clamp failed")
        let sp = NFSprite()
        sp.graphics.beginFill(1e30)
        sp.graphics.lineStyle(1.0, 1e30)
        sp.colorTransform = NFColorTransform(1, 1, 1, 1, 1e30, Double.nan, -1e30, 0)
        assert(NFAS2Color(sp).getRGB() == (255 << 16), "NFAS2Color.getRGB clamp failed")
        let tfmt = NFTextFormat(nil, nil, 1e30)
        _ = tfmt
        let snd = NFSound()
        _ = snd.play(Double.nan)
        _ = snd.play(1e30)
        let sparseArr: NFValue = .fromArray([])
        sparseArr[4_279_312_947.0] = .string("hero")
        sparseArr[UInt.max] = .string("umax")
        sparseArr[Int.max] = .string("imax")
        sparseArr["length"] = .int(0x7FFF_FFFF)
        assert(sparseArr[4_279_312_947.0].asString == "hero", "sparse high Double key failed")
        assert(sparseArr[UInt.max].asString == "umax", "UInt.max subscript failed")
        assert(sparseArr[Int.max].asString == "imax", "Int.max subscript failed")
        assert(sparseArr.length == NFArrayBox.maxDenseLength, "NFArrayBox.maxDenseLength clamp failed")
        let stage = NFStage()
        let timer = NFTimer(10.0, 0)
        timer.start()
        assert(!NFTimer.activeTimers.isEmpty)
        stage.teardown()
        assert(NFTimer.activeTimers.isEmpty, "NFStage.teardown() must stop active timers")
        print("TS_AUDIT_VERIFY_OK")
    }
}
"#,
    )
    .expect("write VerifyTsAudit.swift");
    let ts_bin = tmp.path().join("VerifyTsAuditBin");
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.args([
        "--sdk",
        "macosx",
        "swiftc",
        "-sdk",
        &macos_sdk,
        "-warnings-as-errors",
        "-swift-version",
        "6",
        "-strict-concurrency=complete",
        "-module-cache-path",
    ])
    .arg(&mod_cache);
    for f in &ts_rep.generated_swift_files {
        cmd.arg(f);
    }
    cmd.arg(ts_dir.join("App/AppSwiftHost.swift"))
        .arg(&ts_main)
        .arg("-o")
        .arg(&ts_bin);
    let out = cmd.output().expect("compile VerifyTsAudit");
    assert!(
        out.status.success(),
        "VerifyTsAudit compile failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&ts_bin).output().expect("run VerifyTsAudit");
    assert!(
        run.status.success() && String::from_utf8_lossy(&run.stdout).contains("TS_AUDIT_VERIFY_OK"),
        "VerifyTsAudit execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    // Compile and run AS3 verifier (`vec_alias_scopes` in closure + Int overflow + `% 0` + keyword event param) with `-warnings-as-errors`!
    let as3_main = tmp.path().join("VerifyAs3Audit.swift");
    fs::write(
        &as3_main,
        r#"import Foundation
@main
@MainActor
struct VerifyAs3Audit {
    static func main() {
        let p = VecAndMathProbe()
        let r = p.exercise(0)
        assert(p.visMap[1] == 77, "expected visMap[1] == 77")
        assert(r == 78, "expected capturedElem + zeroRem == 78, got \(r)")
        print("AS3_AUDIT_VERIFY_OK")
    }
}
"#,
    )
    .expect("write VerifyAs3Audit.swift");
    let as3_bin = tmp.path().join("VerifyAs3AuditBin");
    let mut cmd2 = Command::new("/usr/bin/xcrun");
    cmd2.args([
        "--sdk",
        "macosx",
        "swiftc",
        "-sdk",
        &macos_sdk,
        "-warnings-as-errors",
        "-swift-version",
        "6",
        "-strict-concurrency=complete",
        "-module-cache-path",
    ])
    .arg(&mod_cache);
    for f in &as3_rep.generated_swift_files {
        cmd2.arg(f);
    }
    cmd2.arg(as3_dir.join("App/AppSwiftHost.swift"))
        .arg(&as3_main)
        .arg("-o")
        .arg(&as3_bin);
    let out2 = cmd2.output().expect("compile VerifyAs3Audit");
    assert!(
        out2.status.success(),
        "VerifyAs3Audit compile failed:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let run2 = Command::new(&as3_bin).output().expect("run VerifyAs3Audit");
    assert!(
        run2.status.success()
            && String::from_utf8_lossy(&run2.stdout).contains("AS3_AUDIT_VERIFY_OK"),
        "VerifyAs3Audit execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run2.stdout),
        String::from_utf8_lossy(&run2.stderr)
    );
}

#[test]
#[ignore = "diagnostic corpus sweep"]
fn t499_diagnostic_sweep_all_examples() {
    if should_skip_xcodebuild() {
        return;
    }
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let examples = collect_tracked_example_projects(&repo_root);
    assert!(!examples.is_empty(), "expected tracked examples");

    let sweep_root = repo_root.join("target/artifacts/t499-sweep");
    let _ = fs::remove_dir_all(&sweep_root);
    let mod_cache = sweep_root.join("ModuleCache");
    fs::create_dir_all(&mod_cache).expect("create ModuleCache");

    let macos_sdk = sdk_path("macosx");
    let ios_sim_sdk = sdk_path("iphonesimulator");
    warmup_swift_module_cache(
        &mod_cache,
        macos_sdk.as_deref(),
        ios_sim_sdk.as_deref(),
        &sweep_root,
    );

    let next_idx = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Value, Vec<String>)>> = Mutex::new(Vec::new());

    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                loop {
                    let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                    if idx >= examples.len() {
                        break;
                    }
                    let path = &examples[idx];
                    let rel = path
                        .strip_prefix(&repo_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    let bytes = fs::read(path).expect("read .nflash");
                    let proj = match Project::from_nflash_bytes(&bytes) {
                        Ok(p) => p.with_resolved_script_language().into_owned(),
                        Err(err) => {
                            results.lock().unwrap().push((
                                idx,
                                json!({
                                    "path": rel,
                                    "status": "load_error",
                                    "error": err.to_string(),
                                }),
                                vec![format!("LOAD_ERR: {err}")],
                            ));
                            continue;
                        }
                    };

                    let out_dir = sweep_root.join(format!("proj_{idx:03}"));
                    let opts = XcodeExportOptions {
                        base_dir: path.parent().map(Path::to_path_buf),
                        ..XcodeExportOptions::default()
                    };
                    let report = match export_xcode_project(&proj, &out_dir, &opts, None) {
                        Ok(r) => r,
                        Err(err) => {
                            results.lock().unwrap().push((
                                idx,
                                json!({
                                    "path": rel,
                                    "status": "export_error",
                                    "error": err.to_string(),
                                }),
                                vec![format!("EXPORT_ERR: {err}")],
                            ));
                            continue;
                        }
                    };

                    let scenes_src =
                        fs::read_to_string(out_dir.join("Generated/Scenes/GeneratedScenes.swift"))
                            .unwrap_or_default();
                    let fixme_count = scenes_src.matches("FIXME [NeoFlash Swift]").count();

                    let mut extracted_errors = Vec::new();
                    let mut mac_ok = false;
                    let mut mac_timed_out = false;
                    let mut mac_ms = 0u128;
                    let mut mac_err = String::new();
                    if let Some(sdk) = &macos_sdk {
                        let mut cmd = Command::new("/usr/bin/xcrun");
                        cmd.args([
                            "--sdk",
                            "macosx",
                            "swiftc",
                            "-sdk",
                            sdk,
                            "-typecheck",
                            "-swift-version",
                            "6",
                            "-strict-concurrency=complete",
                            "-module-cache-path",
                        ]);
                        cmd.arg(&mod_cache);
                        for f in &report.generated_swift_files {
                            cmd.arg(f);
                        }
                        for f in &report.app_swift_files {
                            cmd.arg(f);
                        }
                        let (ok, to, ms, stderr) = run_with_timeout(cmd, Duration::from_secs(45));
                        mac_ok = ok;
                        mac_timed_out = to;
                        mac_ms = ms;
                        if !ok {
                            mac_err = stderr;
                            if to {
                                extracted_errors.push(format!("TIMEOUT_MACOS ({rel})"));
                            } else {
                                for line in mac_err.lines() {
                                    if let Some((_, msg)) = line.split_once(": error: ") {
                                        extracted_errors.push(msg.trim().to_owned());
                                    }
                                }
                            }
                        }
                    }

                    let mut ios_ok = false;
                    let mut ios_timed_out = false;
                    let mut ios_ms = 0u128;
                    let mut ios_err = String::new();
                    if let Some(sdk) = &ios_sim_sdk {
                        let mut cmd = Command::new("/usr/bin/xcrun");
                        cmd.args([
                            "--sdk",
                            "iphonesimulator",
                            "swiftc",
                            "-sdk",
                            sdk,
                            "-target",
                            "arm64-apple-ios17.0-simulator",
                            "-typecheck",
                            "-swift-version",
                            "6",
                            "-strict-concurrency=complete",
                            "-module-cache-path",
                        ]);
                        cmd.arg(&mod_cache);
                        for f in &report.generated_swift_files {
                            cmd.arg(f);
                        }
                        for f in &report.app_swift_files {
                            cmd.arg(f);
                        }
                        let (ok, to, ms, stderr) = run_with_timeout(cmd, Duration::from_secs(45));
                        ios_ok = ok;
                        ios_timed_out = to;
                        ios_ms = ms;
                        if !ok {
                            ios_err = stderr;
                            if to {
                                extracted_errors.push(format!("TIMEOUT_IOS ({rel})"));
                            }
                        }
                    }

                    let _ = fs::remove_dir_all(&out_dir);

                    let preflight_msgs: Vec<String> = report
                        .preflight_items
                        .iter()
                        .map(|p| format!("[{}] {}: {}", p.code, p.location, p.message))
                        .collect();

                    let rec = json!({
                        "path": rel,
                        "script_language": format!("{:?}", proj.script_language),
                        "sources_count": proj.sources.len(),
                        "fixme_count": fixme_count,
                        "preflight_count": report.preflight_items.len(),
                        "preflight_sample": preflight_msgs.into_iter().take(8).collect::<Vec<_>>(),
                        "macos_ok": mac_ok,
                        "macos_timed_out": mac_timed_out,
                        "macos_ms": mac_ms,
                        "ios_ok": ios_ok,
                        "ios_timed_out": ios_timed_out,
                        "ios_ms": ios_ms,
                        "macos_errors": if mac_timed_out { vec![mac_err] } else { mac_err.lines().filter(|l| l.contains(": error: ")).take(12).map(str::to_owned).collect::<Vec<_>>() },
                        "ios_errors": if ios_timed_out { vec![ios_err] } else { ios_err.lines().filter(|l| l.contains(": error: ")).take(6).map(str::to_owned).collect::<Vec<_>>() },
                    });
                    results.lock().unwrap().push((idx, rec, extracted_errors));
                }
            });
        }
    });

    let _ = fs::remove_dir_all(&mod_cache);

    let mut collected = results.into_inner().unwrap();
    collected.sort_by_key(|(idx, _, _)| *idx);

    let mut pass_macos = 0usize;
    let mut pass_ios = 0usize;
    let mut export_failures = 0usize;
    let mut timeouts = Vec::new();
    let mut error_buckets: BTreeMap<String, usize> = BTreeMap::new();
    let mut records = Vec::new();

    for (_, rec, errs) in collected {
        if rec.get("status").is_some() {
            export_failures += 1;
        }
        if rec.get("macos_ok").and_then(Value::as_bool) == Some(true) {
            pass_macos += 1;
        }
        if rec.get("ios_ok").and_then(Value::as_bool) == Some(true) {
            pass_ios += 1;
        }
        if (rec.get("macos_timed_out").and_then(Value::as_bool) == Some(true)
            || rec.get("ios_timed_out").and_then(Value::as_bool) == Some(true))
            && let Some(p) = rec.get("path").and_then(Value::as_str)
        {
            timeouts.push(p.to_owned());
        }
        for e in errs {
            *error_buckets.entry(e).or_insert(0) += 1;
        }
        records.push(rec);
    }

    let mut sorted_buckets: Vec<(String, usize)> = error_buckets.into_iter().collect();
    sorted_buckets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let summary = json!({
        "total_examples": examples.len(),
        "export_failures": export_failures,
        "pass_macos": pass_macos,
        "pass_ios": pass_ios,
        "timeouts": timeouts,
        "top_errors": sorted_buckets,
        "projects": records,
    });
    let report_path = sweep_root.join("initial-corpus-report.json");
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .expect("write report");
    println!(
        "SWEEP SUMMARY: total={} export_fail={} pass_macos={} pass_ios={} timeouts={:?}",
        examples.len(),
        export_failures,
        pass_macos,
        pass_ios,
        timeouts
    );
}
