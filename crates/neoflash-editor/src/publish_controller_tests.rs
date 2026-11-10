use super::*;
use crate::workspace::WorkspacePrefs;
use neoflash_export::{FontExportPolicy, SizeLevel};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-publish-controller-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("runtime")).unwrap();
        std::fs::write(
            path.join("runtime/neoflash_web.js"),
            "var NeoFlashWasm = {};\n",
        )
        .unwrap();
        std::fs::write(
            path.join("runtime/neoflash_web_bg.wasm"),
            b"\0asm\x01\0\0\0",
        )
        .unwrap();
        Self(path)
    }
    fn runtime(&self) -> PathBuf {
        self.0.join("runtime")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_ready(
    controller: &mut PublishController,
    project: &Project,
    prefs: &PublishPrefs,
    runtime: &Path,
    ctx: &egui::Context,
) -> SizeEstimate {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match controller.refresh(project, prefs, runtime, ctx) {
            EstimateState::Ready(estimate) => return estimate.clone(),
            EstimateState::Failed(error) => panic!("{error}"),
            _ => assert!(Instant::now() < deadline, "estimate did not finish"),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn persisted_options_drive_publish_save_and_exact_background_size() {
    let fixture = Fixture::new();
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let original = editor.project.clone();
    let history = editor.history.revision();
    let legacy = WorkspacePrefs::from_json(r#"{"version":1,"publish":{"publish_on_save":true}}"#);
    assert_eq!(legacy.publish.options, PublishOptions::default());
    assert!(legacy.publish.publish_on_save);
    let output = fixture.0.join("published");
    for (index, size) in SizeLevel::all(" ../shared%20runtime/ ")
        .into_iter()
        .enumerate()
    {
        editor.workspace.publish.options = PublishOptions {
            size,
            // A non-default policy proves that the editor does not silently hardcode Embed.
            fonts: FontExportPolicy::Device,
            audio: neoflash_export::AudioExportPolicy::Original,
        };
        let json = serde_json::to_string(&editor.workspace).unwrap();
        let restored = WorkspacePrefs::from_json(&json);
        assert_eq!(restored.publish.options, editor.workspace.publish.options);
        editor.workspace = restored;
        let estimate = wait_ready(
            &mut editor.publish_controller,
            &editor.project,
            &editor.workspace.publish,
            &fixture.runtime(),
            &ctx,
        );
        let publish_settings = editor.workspace.publish.clone();
        assert!(editor.publish_html5_with_settings(&output, &fixture.runtime(), &publish_settings));
        let bytes: Vec<Vec<u8>> = std::fs::read_dir(&output)
            .unwrap()
            .map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
            .collect();
        assert_eq!(estimate.files, bytes.len());
        assert_eq!(estimate.bytes, bytes.iter().map(Vec::len).sum::<usize>());
        assert_eq!(
            estimate.gzip_bytes,
            bytes
                .iter()
                .map(|b| neoflash_export::gzip_len(b))
                .sum::<usize>()
        );
        assert_eq!(
            estimate.parts.iter().map(|part| part.bytes).sum::<usize>(),
            estimate.bytes
        );
        let bundle = std::fs::read_to_string(output.join("bundle.js")).unwrap();
        assert!(bundle.contains("\"policy\":\"device\""));
        match index {
            0 => assert!(
                bundle.contains("\"wasm\":") && !output.join("neoflash_web_bg.wasm").exists()
            ),
            1 => assert!(
                output.join("neoflash_web_bg.wasm").exists() && output.join("air.js").exists()
            ),
            2 => assert!(!output.join("air.js").exists() && output.join("audio.js").exists()),
            3 => {
                assert!(!output.join("runtime.js").exists());
                assert!(bundle.contains("\"runtime_id\":"));
                assert!(estimate.shared_runtime_bytes.is_some());
            }
            _ => unreachable!(),
        }
    }
    let mut draft = editor.workspace.publish.clone();
    draft.options.size = SizeLevel::Balanced;
    editor
        .publish_controller
        .refresh(&editor.project, &draft, &fixture.runtime(), &ctx);
    editor.update_publish_estimate(&ctx);
    assert_eq!(
        editor.publish_controller.current.as_ref().unwrap().options,
        draft.options,
        "frame polling preserves the draft even when saved preferences differ"
    );
    let draft_estimate = wait_ready(
        &mut editor.publish_controller,
        &editor.project,
        &draft,
        &fixture.runtime(),
        &ctx,
    );
    assert!(editor.publish_html5_with_settings(&output, &fixture.runtime(), &draft));
    assert_eq!(draft_estimate.level, SizeLevel::Balanced);
    assert!(output.join("neoflash_web_bg.wasm").exists());
    assert!(
        matches!(editor.workspace.publish.options.size, SizeLevel::Shared(_)),
        "publishing a draft does not save its settings"
    );
    let previous_root = editor.project.root.clone();
    editor.project.root = "different-document".into();
    editor.update_publish_estimate(&ctx);
    assert_eq!(editor.publish_controller.state, EstimateState::Idle);
    editor.project.root = previous_root;
    assert_eq!(
        editor.project, original,
        "publish preferences never mutate the document"
    );
    assert_eq!(
        editor.history.revision(),
        history,
        "publication adds no Undo entry"
    );

    editor.workspace.publish.options.size = SizeLevel::Balanced;
    editor.workspace.publish.publish_on_save = true;
    editor.workspace.publish.output_folder = Some(output.to_string_lossy().into_owned());
    assert!(editor.save_to_path_with_runtime(fixture.0.join("movie.nflash"), &fixture.runtime()));
    assert!(
        output.join("neoflash_web_bg.wasm").is_file(),
        "publish-on-save uses selected options too"
    );
    let before = std::fs::read(output.join("bundle.js")).unwrap();
    editor.project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "resource",
            "data.txt",
            "text/plain",
            b"portable content",
        )
        .unwrap(),
    );
    editor.workspace.publish.warnings_as_errors = true;
    assert!(
        matches!(editor.publish_controller.refresh(&editor.project, &editor.workspace.publish, &fixture.runtime(), &ctx),
        EstimateState::Failed(error) if error.contains("warning(s) treated as errors"))
    );
    let publish_settings = editor.workspace.publish.clone();
    assert!(!editor.publish_html5_with_settings(&output, &fixture.runtime(), &publish_settings));
    assert_eq!(std::fs::read(output.join("bundle.js")).unwrap(), before);
    editor.workspace.publish.warnings_as_errors = false;
    editor.workspace.publish.options.size = SizeLevel::Shared("javascript:alert(1)".into());
    assert!(matches!(
        editor.publish_controller.refresh(
            &editor.project,
            &editor.workspace.publish,
            &fixture.runtime(),
            &ctx
        ),
        EstimateState::Failed(_)
    ));
    let publish_settings = editor.workspace.publish.clone();
    assert!(!editor.publish_html5_with_settings(&output, &fixture.runtime(), &publish_settings));
    assert_eq!(
        std::fs::read(output.join("bundle.js")).unwrap(),
        before,
        "invalid options preserve an existing package"
    );
}

#[test]
fn publish_on_save_routes_every_enabled_target_and_never_uses_disabled_default_video() {
    let fixture = Fixture::new();
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let desktop_dir = fixture.0.join("save-desktop");
    let image_dir = fixture.0.join("save-images");
    let svg_file = fixture.0.join("save-svg/animation.svg");
    let disabled_video = fixture.0.join("must-not-exist.mp4");

    let settings = &mut editor.workspace.publish;
    settings.publish_on_save = true;
    settings.default_target = crate::workspace::PublishTarget::VideoMp4;
    settings.html5_enabled = false;
    settings.desktop_app_enabled = true;
    settings.desktop_app_folder = Some(desktop_dir.to_string_lossy().into_owned());
    settings.video_enabled = false;
    settings.video_file = Some(disabled_video.to_string_lossy().into_owned());
    settings.image_sequence_enabled = true;
    settings.image_sequence_folder = Some(image_dir.to_string_lossy().into_owned());
    settings.svg_enabled = true;
    settings.svg_file = Some(svg_file.to_string_lossy().into_owned());
    settings.output_folder = None;

    assert!(editor.save_to_path_with_runtime(
        fixture.0.join("all-enabled-targets.nflash"),
        &fixture.runtime(),
    ));
    assert!(
        desktop_dir
            .join(format!("{}.app/Contents/Info.plist", editor.project.name))
            .is_file()
    );
    assert!(
        std::fs::read_dir(&image_dir).unwrap().next().is_some(),
        "enabled image publishing must write at least one frame"
    );
    assert!(svg_file.is_file());
    assert!(
        !disabled_video.exists(),
        "default_target must never override a disabled video checkbox"
    );
}

#[test]
fn pending_edits_replacement_cancel_and_runtime_changes_discard_old_results() {
    let fixture = Fixture::new();
    let runtime = fixture.runtime();
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let mut prefs = PublishPrefs::default();
    let mut controller = PublishController::default();
    let initial = wait_ready(&mut controller, &project, &prefs, &runtime, &ctx);

    // A deliberately held worker makes the late-result race deterministic. No sleep/race with
    // compression is needed to prove coalescing and rejection of even a successful stale result.
    let old = controller.current.clone().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, result) = mpsc::channel();
    controller.running = Some(Running {
        request: old,
        cancel: cancel.clone(),
        result,
    });
    project.name = "Uncommitted field edit".into();
    assert!(matches!(
        controller.refresh(&project, &prefs, &runtime, &ctx),
        EstimateState::Computing
    ));
    assert!(cancel.load(Ordering::Relaxed));
    prefs.options.size = SizeLevel::Compact;
    controller.refresh(&project, &prefs, &runtime, &ctx);
    project = Project::blank(); // replacing the active document while the old estimate is running
    project.name = "Replacement document".into();
    prefs.options.size = SizeLevel::Shared("../runtime".into());
    controller.refresh(&project, &prefs, &runtime, &ctx);
    assert_eq!(
        controller.pending.as_ref().unwrap().project.name,
        "Replacement document"
    );
    assert_eq!(
        controller.pending.as_ref().unwrap().options.size,
        prefs.options.size
    );
    sender.send(Ok(Some(initial))).unwrap();
    let current = wait_ready(&mut controller, &project, &prefs, &runtime, &ctx);
    assert_eq!(current.level, prefs.options.size);

    // Runtime installation/rebuild at the same location and changing warning policy invalidate.
    std::fs::write(
        runtime.join("neoflash_web.js"),
        "var NeoFlashWasm = {}; // replacement build\n",
    )
    .unwrap();
    assert!(matches!(
        controller.refresh(&project, &prefs, &runtime, &ctx),
        EstimateState::Computing
    ));
    prefs.warnings_as_errors = true;
    controller.refresh(&project, &prefs, &runtime, &ctx);
    assert!(controller.current.as_ref().unwrap().warnings_as_errors);
    controller.cancel();
    assert_eq!(controller.state, EstimateState::Idle);
    assert!(
        controller.current.is_none() && controller.pending.is_none() && controller.demand.is_none()
    );
    assert!(
        controller
            .running
            .as_ref()
            .unwrap()
            .cancel
            .load(Ordering::Relaxed)
    );
    // Reopening after cancellation produces a fresh result, including under the changed runtime.
    let reopened = wait_ready(&mut controller, &project, &prefs, &runtime, &ctx);
    assert_ne!(current.shared_runtime_bytes, reopened.shared_runtime_bytes);
    std::fs::remove_file(runtime.join("neoflash_web.js")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while !matches!(
        controller.refresh(&project, &prefs, &runtime, &ctx),
        EstimateState::Failed(_)
    ) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
