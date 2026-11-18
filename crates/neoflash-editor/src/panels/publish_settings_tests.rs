//! Exercise the real modal and NF2 controls without compiling/compressing the installed runtime.
use super::*;
use crate::theme::ThemeMode;
use eframe::egui::{Event, Pos2, Rect, Vec2};
use neoflash_core::{Element, ElementKind, Project, ResourceUnit};

struct Driver {
    ctx: egui::Context,
    editor: Editor,
    output: egui::FullOutput,
    time: f64,
}

impl Driver {
    fn new(theme: ThemeMode) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project
            .resources
            .push(ResourceUnit::from_bytes("notice", "notice.txt", "text/plain", b"test").unwrap());
        project.format_version = project.format_version.max(3);
        project.validate().expect("valid publish settings fixture");
        let mut editor = Editor::from_project(&ctx, project);
        editor.set_theme(&ctx, theme);
        // This supported warning policy makes estimates fail synchronously. The controller's
        // own family separately verifies workers, exact exports, cancellation and stale results.
        editor.workspace.publish.warnings_as_errors = true;
        assert!(!neoflash_export::export_warnings(&editor.project).is_empty());
        Self {
            ctx,
            editor,
            output: Default::default(),
            time: 0.0,
        }
    }

    fn step(&mut self, events: Vec<Event>) {
        self.time += 0.1;
        self.output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                self.editor.shortcuts(ctx);
                self.editor.publish_settings_dialog(ctx);
            },
        );
    }

    fn settle(&mut self) {
        self.step(vec![]);
        self.step(vec![]);
    }

    fn open(&mut self) {
        self.editor.open_publish_settings();
        assert!(self.editor.has_modal_dialog());
        assert!(self.editor.has_modal_dialog_in(&self.ctx));
        self.settle();
    }

    fn node(&self, label: &str) -> &egui::accesskit::Node {
        self.output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| (node.label() == Some(label)).then_some(node))
            .unwrap_or_else(|| panic!("missing control {label:?}"))
    }

    fn click(&mut self, label: &str) {
        let bounds = self.node(label).bounds().unwrap();
        let at = Pos2::new(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![Event::PointerMoved(at), button(true)]);
        self.step(vec![button(false)]);
        self.settle();
    }

    fn key(&mut self, key: egui::Key) {
        self.step(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        self.step(vec![Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        self.settle();
    }

    fn set_url(&mut self, value: &str) {
        self.click("Runtime URL");
        self.step(vec![Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]);
        self.step(vec![Event::Paste(value.into())]);
        self.settle();
    }
}

fn painted_text(output: &egui::FullOutput) -> Vec<String> {
    fn visit(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| visit(shape, texts)),
            _ => {}
        }
    }
    let mut texts = vec![];
    for shape in &output.shapes {
        visit(&shape.shape, &mut texts);
    }
    texts
}

#[test]
fn publish_modal_draft_validation_cancel_and_save_in_both_themes() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let mut d = Driver::new(theme);
        // Give background commands something real to change: F6 would split this span,
        // Delete would remove the selected shape, and B would switch the drawing tool.
        let root = d.editor.project.root.clone();
        let timeline = &mut d.editor.project.symbol_mut(&root).unwrap().timeline;
        timeline.frame_count = 3;
        timeline.layers[0].keyframes[0].elements.push(Element {
            id: "publish-modal-shape".into(),
            kind: ElementKind::Rectangle {
                width: 20.,
                height: 12.,
                fill: [20, 90, 180, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.,
            },
            ..Default::default()
        });
        d.editor.project.validate().unwrap();
        d.editor.frame = 1;
        d.editor.select_single_id("publish-modal-shape".into());
        let saved = d.editor.workspace.publish.clone();
        let project = d.editor.project.clone();
        let revision = d.editor.history.revision();
        let selection = d.editor.selection.clone();
        d.open();
        for key in [
            egui::Key::F6,
            egui::Key::Backspace,
            egui::Key::Delete,
            egui::Key::B,
        ] {
            d.key(key);
            assert_eq!(
                d.editor.project, project,
                "background {key:?} must not edit the document"
            );
            assert_eq!(d.editor.selection, selection);
            assert_eq!(d.editor.history.revision(), revision);
            assert!(d.editor.tool == crate::app::Tool::Select);
        }
        assert_eq!(
            d.node("Full").toggled(),
            Some(egui::accesskit::Toggled::True)
        );
        for label in ["Balanced", "Compact"] {
            d.click(label);
            assert_eq!(
                d.node(label).toggled(),
                Some(egui::accesskit::Toggled::True)
            );
            assert_eq!(
                d.editor.workspace.publish, saved,
                "selection is only a draft"
            );
            assert_eq!(
                level_label(
                    &d.editor
                        .publish_settings
                        .as_ref()
                        .unwrap()
                        .settings
                        .options
                        .size
                ),
                label
            );
        }
        d.click("Shared");
        for label in ["Publish", "OK"] {
            assert!(
                d.node(label).is_disabled(),
                "empty Shared URL disables {label}"
            );
        }
        assert!(painted_text(&d.output).iter().any(|s| s == "Size unknown"));
        d.set_url("../runtime");
        for label in ["Publish", "OK"] {
            assert!(!d.node(label).is_disabled(), "valid URL enables {label}");
        }
        d.click("Full");
        d.click("Shared");
        assert_eq!(
            d.editor
                .publish_settings
                .as_ref()
                .unwrap()
                .settings
                .options
                .size,
            SizeLevel::Shared("../runtime".into()),
            "trying another level preserves the draft URL"
        );
        d.set_url("javascript:alert(1)");
        let texts = painted_text(&d.output);
        assert!(texts.iter().any(|s| s.contains("Shared runtime URL must")));
        assert!(texts.iter().any(|s| s == "Size unknown"));
        assert_eq!(texts.iter().filter(|s| s.as_str() == "—").count(), 3);
        for label in ["Publish", "OK"] {
            assert!(d.node(label).is_disabled());
            d.click(label);
            assert!(d.editor.publish_settings.is_some());
        }
        d.key(egui::Key::Enter);
        assert!(
            d.editor.publish_settings.is_some(),
            "Enter cannot save an invalid URL"
        );
        d.key(egui::Key::Escape);
        assert!(d.editor.publish_settings.is_none());
        assert_eq!(d.editor.workspace.publish, saved);

        d.open();
        d.click("Compact");
        d.click("Cancel");
        assert!(d.editor.publish_settings.is_none());
        assert_eq!(d.editor.workspace.publish, saved);
        d.open();
        d.click("Balanced");
        d.click("OK");
        assert!(d.editor.publish_settings.is_none());
        let mut expected = saved;
        expected.options.size = SizeLevel::Balanced;
        assert_eq!(d.editor.workspace.publish, expected);
        assert_eq!(d.editor.project, project);
        assert_eq!(
            d.editor.selection, selection,
            "modal URL editing owns its clipboard and selection commands"
        );
        assert_eq!(
            d.editor.history.revision(),
            revision,
            "settings add no document Undo"
        );
    }
}

#[test]
fn custom_reset_is_drafted_and_same_root_document_switch_closes_publish() {
    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.workspace.publish.options.size = SizeLevel::Compact;
    d.editor.workspace.publish.options.fonts = FontExportPolicy::Outlines;
    let saved = d.editor.workspace.publish.clone();
    d.open();
    for (_, label) in LEVELS {
        assert_eq!(
            d.node(label).toggled(),
            Some(egui::accesskit::Toggled::False)
        );
    }
    d.click("Reset to Compact");
    assert_eq!(
        d.editor
            .publish_settings
            .as_ref()
            .unwrap()
            .settings
            .options
            .fonts,
        FontExportPolicy::Embed
    );
    assert_eq!(d.editor.workspace.publish, saved);
    d.click("Cancel");
    assert_eq!(d.editor.workspace.publish, saved);
    d.open();
    d.click("Reset to Compact");
    d.click("OK");
    assert_eq!(
        d.editor.workspace.publish.options.fonts,
        FontExportPolicy::Embed
    );
    assert_eq!(d.editor.workspace.publish.options.size, SizeLevel::Compact);

    d.open();
    d.click("Shared");
    let original_root = d.editor.project.root.clone();
    let mut cloned = d.editor.project.clone();
    cloned.name = "Independent same-root document".into();
    cloned
        .validate()
        .expect("same-root document must be accepted");
    let old_count = d.editor.session_count();
    d.editor.open_new_session_tab(&d.ctx, cloned, None);
    assert_eq!(d.editor.session_count(), old_count + 1);
    assert_eq!(d.editor.project.name, "Independent same-root document");
    assert_eq!(d.editor.project.root, original_root);
    assert!(d.editor.publish_settings.is_none());
    d.editor.update_publish_estimate(&d.ctx);
    d.settle();
    assert!(
        d.editor.publish_settings.is_none(),
        "polling cannot reopen a closed draft"
    );
    assert_eq!(d.editor.workspace.publish.options.size, SizeLevel::Compact);
}

#[test]
fn weight_states_use_on_disk_parts_exact_total_gzip_and_clear_old_numbers() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let d = Driver::new(theme);
        let draft = PublishSettingsDraft::new(PublishPrefs::default());
        let estimate = SizeEstimate {
            level: SizeLevel::Full,
            files: 7,
            bytes: 10_000,
            gzip_bytes: 1_234,
            parts: vec![
                neoflash_export::SizePart {
                    kind: "wasm",
                    bytes: 9_000,
                    gzip_bytes: 1,
                },
                neoflash_export::SizePart {
                    kind: "runtime_js",
                    bytes: 1_000,
                    gzip_bytes: 99_999,
                },
            ],
            shared_runtime_bytes: None,
            shared_runtime_gzip_bytes: None,
            tradeoffs: vec![],
            omitted_modules: vec![],
        };
        let draw = |state: &EstimateState| {
            d.ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700., 400.))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(566.);
                        weight(ui, state, &draft);
                        caption(ui, &footer_label(&draft, state));
                    });
                },
            )
        };
        let ready = draw(&EstimateState::Ready(estimate));
        let texts = painted_text(&ready);
        assert!(texts.iter().any(|s| s == "9.0 KB"));
        assert!(texts.iter().any(|s| s == "1.0 KB"));
        assert!(texts.iter().any(|s| s == "1.2 KB"));
        assert!(
            texts
                .iter()
                .any(|s| s == "Full · 10.0 KB · 1.2 KB gzip · 7 files")
        );
        assert!(
            !texts.iter().any(|s| s == "100.0 KB"),
            "part gzip must not be summed"
        );
        let palette = neoflash_ui::palette(&d.ctx);
        let bar_width = |color| {
            ready
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == color && (rect.rect.height() - 8.).abs() < 0.01 =>
                    {
                        Some(rect.rect.width())
                    }
                    _ => None,
                })
                .fold(0.0_f32, f32::max)
        };
        assert!(
            bar_width(palette.accent_default) > 8. * bar_width(palette.syntax_keyword),
            "bar proportions use disk bytes, not inverted gzip attribution"
        );
        for state in [
            EstimateState::Computing,
            EstimateState::Failed("Runtime missing".into()),
        ] {
            let output = draw(&state);
            let texts = painted_text(&output);
            assert!(
                !texts.iter().any(|s| s.contains("KB")),
                "stale sizes must disappear"
            );
            assert_eq!(texts.iter().filter(|s| s.as_str() == "—").count(), 3);
            match state {
                EstimateState::Computing => assert!(texts.iter().any(|s| s == "Estimating Full…")),
                EstimateState::Failed(_) => {
                    assert!(texts.iter().any(|s| s == "Runtime missing"));
                    assert!(texts.iter().any(|s| s == "Retry"));
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn publish_settings_and_preferences_audio_policy_update_custom_badge_and_persist() {
    use crate::panels::preferences::PAGE_PUBLISH;
    use neoflash_export::AudioExportPolicy;

    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.open_publish_settings();
    d.settle();

    {
        let draft = d
            .editor
            .publish_settings
            .as_mut()
            .expect("publish settings open");
        assert_eq!(draft.settings.options.audio, AudioExportPolicy::Original);
        assert!(!draft.custom());
        draft.settings.options.audio = AudioExportPolicy::Mp3_128;
        assert!(
            draft.custom(),
            "non-original audio policy marks preset as Custom"
        );
        draft.select_level(1);
        assert_eq!(
            draft.settings.options.audio,
            AudioExportPolicy::Original,
            "selecting a SizeLevel resets audio policy to Original"
        );
        draft.settings.options.audio = AudioExportPolicy::Mp3_160;
    }
    d.settle();
    assert!(
        painted_text(&d.output).iter().any(|t| t == "MP3 160 kbps"),
        "Publish settings shows selected audio policy"
    );
    d.click("OK");
    assert!(d.editor.publish_settings.is_none());
    assert_eq!(
        d.editor.workspace.publish.options.audio,
        AudioExportPolicy::Mp3_160
    );

    // Preferences ▸ Publish shows the same audio policy under Compression and supports Reset + Save.
    d.editor.open_preferences(PAGE_PUBLISH);
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = d.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                ..Default::default()
            },
            |ctx| {
                d.editor.preferences_dialog(ctx);
            },
        );
    }
    let pref_texts = painted_text(&out);
    assert!(
        pref_texts.iter().any(|t| t == "Audio"),
        "Preferences ▸ Publish renders Audio row"
    );
    assert!(
        pref_texts.iter().any(|t| t == "MP3 160 kbps"),
        "Preferences ▸ Publish displays current audio policy: {pref_texts:?}"
    );
}

#[test]
fn publish_settings_and_preferences_video_target_switch_and_mp4_publish() {
    use crate::panels::preferences::PAGE_PUBLISH;
    use crate::workspace::PublishTarget;

    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.open_preferences(PAGE_PUBLISH);
    {
        let pref = d.editor.preferences.as_mut().expect("prefs open");
        assert_eq!(pref.publish.default_target, PublishTarget::Html5);
        pref.publish.default_target = PublishTarget::VideoMp4;
        pref.publish.video_enabled = true;
    }
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = d.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                ..Default::default()
            },
            |ctx| {
                d.editor.preferences_dialog(ctx);
            },
        );
    }
    let pref_texts = painted_text(&out);
    assert!(
        pref_texts.iter().any(|t| t == "Video (MP4)"),
        "Preferences ▸ Publish shows Video (MP4) default target: {pref_texts:?}"
    );
    let draft_copy = d.editor.preferences.clone().unwrap();
    d.editor.save_preferences(&d.ctx, &draft_copy);
    d.editor.preferences = None;
    assert_eq!(
        d.editor.workspace.publish.default_target,
        PublishTarget::VideoMp4
    );

    d.open();
    assert_eq!(
        d.editor.publish_settings.as_ref().unwrap().selected_target,
        PublishTarget::VideoMp4
    );
    let video_texts = painted_text(&d.output);
    assert!(
        video_texts.iter().any(|t| t.contains("MP4 (H.264)")),
        "Publish settings Video tab renders video footer summary: {video_texts:?}"
    );
    assert!(
        video_texts
            .iter()
            .any(|t| t == "Render nested movie clip animation and scripts"),
        "Publish settings Video tab renders Content options"
    );
    d.click("OK");
    assert!(d.editor.publish_settings.is_none());

    if neoflash_export::detect_ffmpeg().is_some() {
        let dir =
            std::env::temp_dir().join(format!("neoflash-publish-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mp4_path = dir.join("published.mp4");
        let mut settings = d.editor.workspace.publish.clone();
        settings.video_file = Some(mp4_path.to_string_lossy().into_owned());
        settings.video_options.width = 160;
        settings.video_options.height = 120;
        settings.video_options.to_frame = Some(2);
        assert!(d.editor.publish_video_with_settings(None, &settings));
        assert!(mp4_path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn default_target_and_empty_target_guard_route_to_publish_settings_instead_of_noop() {
    use crate::workspace::{PublishPrefs, PublishTarget, WorkspacePrefs};

    let def = PublishPrefs::default();
    assert_eq!(def.default_target, PublishTarget::Html5);
    assert!(def.html5_enabled);
    assert!(def.has_any_enabled_target());

    // Deserializing a legacy prefs JSON with all target flags false auto-enables default_target.
    let mut legacy: WorkspacePrefs = serde_json::from_str(
        r#"{"publish":{"default_target":"html5","html5_enabled":false,"desktop_app_enabled":false,"video_enabled":false,"image_sequence_enabled":false,"svg_enabled":false}}"#,
    )
    .expect("parse legacy prefs");
    assert!(!legacy.publish.has_any_enabled_target());
    legacy.sanitize_preference_groups();
    assert!(
        legacy.publish.html5_enabled,
        "sanitize_preference_groups restores default HTML5 target when none was enabled"
    );

    // If all targets are manually unchecked in memory, clicking Publish opens Publish settings
    // instead of opening a folder picker that does nothing.
    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.workspace.publish.html5_enabled = false;
    d.editor.workspace.publish.desktop_app_enabled = false;
    d.editor.workspace.publish.video_enabled = false;
    d.editor.workspace.publish.image_sequence_enabled = false;
    d.editor.workspace.publish.svg_enabled = false;
    assert!(d.editor.publish_settings.is_none());

    d.editor.export(false);
    assert!(
        d.editor.publish_settings.is_some(),
        "Publish with no enabled targets must open Publish settings modal"
    );
    assert!(
        d.editor.status.contains("Publish settings"),
        "informative status set: {}",
        d.editor.status
    );
}

#[test]
fn publish_button_in_publish_settings_persists_all_edited_settings_without_resetting() {
    use crate::workspace::{PublishProfilePreset, PublishTarget};
    use neoflash_export::{HtmlScaleMode, HtmlTemplate, SizeLevel};

    let temp_dir =
        std::env::temp_dir().join(format!("neoflash-publish-persist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let publish_dir = temp_dir.join("published-clean-compact");
    let has_wasm = Editor::runtime_dir().join("neoflash_web_bg.wasm").is_file();
    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.workspace.publish.warnings_as_errors = false;
    d.open();

    {
        let draft = d
            .editor
            .publish_settings
            .as_mut()
            .expect("publish settings open");
        // Simulate unchecking html5_enabled while HTML5 tab is selected, and customizing options:
        draft.settings.html5_enabled = false;
        draft.selected_target = PublishTarget::Html5;
        if !has_wasm {
            // In unbuilt worktrees lacking `neoflash_web_bg.wasm`, fall back to zero-WASM Native Banner
            // so `Publish` still completes cleanly (`ok == true`) and verifies setting persistence.
            draft.settings.profile = PublishProfilePreset::WebBannerNative;
        }
        draft.settings.output_folder = Some(publish_dir.to_string_lossy().into_owned());
        draft.settings.options.size = SizeLevel::Compact;
        draft.settings.template = HtmlTemplate::Centered;
        draft.settings.scale_mode = HtmlScaleMode::ExactFit;
        draft.settings.loop_timeline = false;
        draft.settings.autoplay = false;
        draft.settings.preloader = false;
        draft.settings.source_maps = true;
        draft.settings.sprite_sheets = true;
        draft.settings.include_trace = false;
        draft.settings.minify = false;
        draft.settings.jpeg_quality = 65;
        draft.settings.strict = true;
    }
    d.settle();
    d.click("Publish");

    assert!(
        d.editor.publish_settings.is_none(),
        "clicking Publish closes the modal after publishing"
    );
    assert!(
        publish_dir.join("index.html").is_file(),
        "HTML5 package published to configured folder"
    );
    if has_wasm {
        let html = std::fs::read_to_string(publish_dir.join("index.html")).unwrap();
        assert!(
            html.contains("<body data-clean=\"true\">"),
            "Publish produces clean stage output without technical preview bar"
        );
        let bindings = std::fs::read_to_string(publish_dir.join("neoflash_web.js")).unwrap();
        assert!(
            bindings.contains("globalThis.NeoFlashWasmBase64 ="),
            "Compact publish includes direct file:// WASM fallback"
        );
        assert!(publish_dir.join("bundle.js.map").is_file());
    }

    // Verify settings did NOT reset after clicking Publish!
    let saved = &d.editor.workspace.publish;
    assert!(
        saved.html5_enabled,
        "clicking Publish auto-enabled the active target and persisted it"
    );
    assert_eq!(saved.options.size, SizeLevel::Compact);
    assert_eq!(saved.template, HtmlTemplate::Centered);
    assert_eq!(saved.scale_mode, HtmlScaleMode::ExactFit);
    assert!(!saved.loop_timeline);
    assert!(!saved.autoplay);
    assert!(!saved.preloader);
    assert!(saved.source_maps);
    assert!(saved.sprite_sheets);
    assert!(!saved.include_trace);
    assert!(!saved.minify);
    assert_eq!(saved.jpeg_quality, 65);
    assert!(saved.strict);

    // Reopen Publish settings and verify the draft initializes with the persisted settings
    d.open();
    let reopened = d.editor.publish_settings.as_ref().unwrap();
    assert!(reopened.settings.html5_enabled);
    assert_eq!(reopened.settings.options.size, SizeLevel::Compact);
    assert_eq!(reopened.settings.jpeg_quality, 65);
    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Area rect of the Publish settings modal as laid out in the last frame.
fn publish_dialog_rect(ctx: &egui::Context) -> Rect {
    ctx.memory(|m| m.area_rect(egui::Id::new("publish-settings")))
        .expect("Publish settings modal area")
}

#[test]
fn publish_settings_v3_layout_fixed_size_collapsed_simulator_and_hints() {
    use neoflash_ui::metrics;
    use neoflash_ui::tokens::space;

    let mut d = Driver::new(ThemeMode::Dark);
    d.open();

    // A 1024x720 window still fits the density size; the dialog keeps a margin and never follows
    // its content.
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = d.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 720.0))),
                ..Default::default()
            },
            |ctx| {
                d.editor.publish_settings_dialog(ctx);
            },
        );
    }
    let dialog = publish_dialog_rect(&d.ctx);
    let margin = 2.0 * space::s5();
    let expected = Vec2::new(
        metrics::dialog_xl().min(1024.0 - margin),
        metrics::dialog_l_height().min(720.0 - margin),
    );
    // The modal frame adds its 1 pt border on every side.
    assert!(
        (dialog.width() - expected.x).abs() <= 2.0 && (dialog.height() - expected.y).abs() <= 2.0,
        "Publish settings must be {expected:?} at 1024x720, got {:?}",
        dialog.size()
    );
    assert!(dialog.top() >= 0.0 && dialog.bottom() <= 720.0);

    fn collect_rects(shape: &egui::Shape, out_rects: &mut Vec<(egui::Color32, Rect)>) {
        match shape {
            egui::Shape::Rect(r) => out_rects.push((r.fill, r.rect)),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_rects(s, out_rects)),
            _ => {}
        }
    }
    let mut all_rects = Vec::new();
    for s in &out.shapes {
        collect_rects(&s.shape, &mut all_rects);
    }

    let texts = painted_text(&out);
    // R-089 #3: Network startup simulator starts collapsed by default.
    assert!(
        texts.iter().any(|t| t == "NETWORK STARTUP SIMULATOR"),
        "expected Network startup simulator section header: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Bootstrap"),
        "collapsed Network startup simulator must hide readings until expanded: {texts:?}"
    );
    // R-089 #6: empty HTML5 output field shows "Choose a file".
    assert!(
        texts.iter().any(|t| t == "Choose a file"),
        "empty output field must show 'Choose a file' hint: {texts:?}"
    );
    // Every target has the same Output row and footer: one "Browse…" and the three actions.
    assert_eq!(texts.iter().filter(|t| t.as_str() == "Browse…").count(), 1);

    // R-089 #5: the active target row has the bg/selected fill across the sidebar.
    let bg_selected = neoflash_ui::palette(&d.ctx).bg_selected;
    let row_width = metrics::sidebar_wide() - 2.0 * space::s1() - 1.0;
    assert!(
        all_rects.iter().any(|(fill, rect)| *fill == bg_selected
            && (rect.width() - row_width).abs() <= 1.0
            && (rect.height() - metrics::row()).abs() <= 0.5),
        "expected bg/selected fill on active target row"
    );

    // Form grid: the Playback checkboxes share one row above the Stage selects, and Sequences is
    // its own row under the Bitmaps checkboxes.
    fn find_text_in_shape(shape: &egui::Shape, needle: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.text().contains(needle) => {
                Some(Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find_text_in_shape(s, needle)),
            _ => None,
        }
    }
    let find_text_rect = |needle: &str| -> Option<Rect> {
        out.shapes
            .iter()
            .find_map(|s| find_text_in_shape(&s.shape, needle))
    };
    let loop_r = find_text_rect("Loop").expect("Loop checkbox");
    let fullscreen_r = find_text_rect("Fullscreen").expect("Fullscreen checkbox");
    let scale_r = find_text_rect("Scale").expect("Scale select");
    let align_r = find_text_rect("Align").expect("Align select");
    assert!((loop_r.center().y - fullscreen_r.center().y).abs() <= 6.0);
    assert!(fullscreen_r.bottom() <= scale_r.top() + 4.0);
    assert!((scale_r.center().y - align_r.center().y).abs() <= 6.0);

    let sprite_r = find_text_rect("Sprite sheets").expect("Sprite sheets checkbox");
    let seq_r = find_text_rect("Sequences").expect("Sequences row");
    assert!(
        sprite_r.bottom() <= seq_r.top() + 4.0,
        "Sequences select must be on its own row under asset checkboxes"
    );
    // One grid: row labels share the left edge of the page, controls share the control axis.
    let label_x = find_text_rect("JPEG quality")
        .expect("JPEG quality label")
        .left();
    assert!((seq_r.left() - label_x).abs() <= 0.5);
    assert!((loop_r.left() - sprite_r.left()).abs() <= 0.5);
    assert!(
        (loop_r.left()
            - label_x
            - metrics::dialog_label()
            - space::s2()
            - metrics::check_box()
            - space::s2())
        .abs()
            <= 1.0,
        "controls start one label column after the labels"
    );
}

/// v3 "nothing jumps": the dialog rect and the footer are the same for every target and option,
/// in every density.
#[test]
fn publish_settings_dialog_rect_is_identical_for_every_target_in_each_density() {
    use crate::workspace::{DensityPref, PublishTarget};
    use neoflash_ui::metrics;

    for density in DensityPref::ALL {
        let mut d = Driver::new(ThemeMode::Light);
        d.editor.workspace.density = density;
        d.editor.apply_density(&d.ctx);
        d.open();
        let expected = Vec2::new(metrics::dialog_xl(), metrics::dialog_l_height());
        let mut reference: Option<(Rect, egui::accesskit::Rect)> = None;
        let mut check = |d: &Driver, what: &str| {
            let rect = publish_dialog_rect(&d.ctx);
            assert!(
                (rect.width() - expected.x).abs() <= 2.0
                    && (rect.height() - expected.y).abs() <= 2.0,
                "{density:?} · {what}: dialog is {:?}, expected {expected:?}",
                rect.size()
            );
            let publish = d.node("Publish").bounds().unwrap();
            match &reference {
                None => reference = Some((rect, publish)),
                Some((first, first_publish)) => {
                    assert_eq!(rect, *first, "{density:?} · {what}: dialog rect changed");
                    assert_eq!(
                        publish, *first_publish,
                        "{density:?} · {what}: footer moved"
                    );
                }
            }
        };
        for target in [
            PublishTarget::Html5,
            PublishTarget::DesktopApp,
            PublishTarget::VideoMp4,
            PublishTarget::ImageSequence,
            PublishTarget::SvgAnimation,
        ] {
            d.editor.publish_settings.as_mut().unwrap().selected_target = target;
            d.settle();
            check(&d, &format!("{target:?}"));
        }
        // Options that add or remove rows keep the size as well.
        {
            let draft = d.editor.publish_settings.as_mut().unwrap();
            draft.selected_target = PublishTarget::Html5;
            draft.settings.options.size = SizeLevel::Shared("../runtime".into());
            draft.settings.options.fonts = FontExportPolicy::Outlines;
        }
        d.settle();
        check(&d, "HTML5, shared runtime, custom");
        {
            let draft = d.editor.publish_settings.as_mut().unwrap();
            draft.selected_target = PublishTarget::ImageSequence;
            draft.settings.image_sequence_sprite_sheet = true;
        }
        d.settle();
        check(&d, "sprite sheet");
        d.editor.publish_settings = None;
    }
    neoflash_ui::metrics::set_density(Default::default());
}

#[test]
fn publish_profiles_and_all_five_targets_desktop_image_sequence_svg_and_html5() {
    use crate::workspace::{PublishProfilePreset, PublishTarget};
    use neoflash_export::SizeLevel;

    let temp_dir =
        std::env::temp_dir().join(format!("neoflash-publish-targets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let rt_dir = temp_dir.join("runtime");
    std::fs::create_dir_all(&rt_dir).unwrap();
    std::fs::write(rt_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    std::fs::write(rt_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();

    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.workspace.publish.warnings_as_errors = false;

    // Verify all profile presets apply their expected configurations
    for preset in PublishProfilePreset::ALL {
        assert!(!preset.label().is_empty());
        d.editor.workspace.publish.apply_profile(preset);
        assert!(d.editor.workspace.publish.has_any_enabled_target());
    }
    d.editor
        .workspace
        .publish
        .apply_profile(PublishProfilePreset::WebCompact);
    assert_eq!(d.editor.workspace.publish.options.size, SizeLevel::Compact);
    d.editor
        .workspace
        .publish
        .apply_profile(PublishProfilePreset::DebugPreview);
    assert!(!d.editor.workspace.publish.publish_clean);
    assert!(d.editor.workspace.publish.source_maps);
    d.editor
        .workspace
        .publish
        .apply_profile(PublishProfilePreset::Default);
    assert!(d.editor.workspace.publish.publish_clean);

    // Verify Desktop app, Image sequence, Sprite sheet, and SVG animation targets publish cleanly
    let desktop_dir = temp_dir.join("desktop-out");
    let seq_dir = temp_dir.join("seq-out");
    let sheet_file = temp_dir.join("sheet-out/sprites.json");
    let svg_file = temp_dir.join("svg-out/scene.svg");

    let mut settings = d.editor.workspace.publish.clone();
    settings.desktop_app_folder = Some(desktop_dir.to_string_lossy().into_owned());
    assert!(
        d.editor
            .publish_desktop_app_with_settings(None, &rt_dir, &settings)
    );
    assert!(
        desktop_dir
            .join(format!("{}.app/Contents/Info.plist", d.editor.project.name))
            .is_file()
    );

    settings.image_sequence_sprite_sheet = false;
    settings.image_sequence_folder = Some(seq_dir.to_string_lossy().into_owned());
    assert!(
        d.editor
            .publish_image_sequence_with_settings(None, &settings)
    );

    settings.image_sequence_sprite_sheet = true;
    settings.image_sequence_folder = Some(sheet_file.to_string_lossy().into_owned());
    assert!(
        d.editor
            .publish_image_sequence_with_settings(None, &settings)
    );
    assert!(sheet_file.is_file());

    settings.svg_file = Some(svg_file.to_string_lossy().into_owned());
    assert!(d.editor.publish_svg_with_settings(None, &settings));
    assert!(svg_file.is_file());

    // Verify switching between all 5 target tabs in Publish settings UI
    d.open();
    for target in [
        PublishTarget::Html5,
        PublishTarget::DesktopApp,
        PublishTarget::VideoMp4,
        PublishTarget::ImageSequence,
        PublishTarget::SvgAnimation,
    ] {
        d.editor.publish_settings.as_mut().unwrap().selected_target = target;
        d.settle();
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn preferences_publish_preview_vs_clean_hud_checkboxes_and_test_movie_distinction() {
    use crate::panels::preferences::PAGE_PUBLISH;

    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.open_preferences(PAGE_PUBLISH);
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = d.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                ..Default::default()
            },
            |ctx| {
                d.editor.preferences_dialog(ctx);
            },
        );
    }
    let texts = painted_text(&out);
    for expected in [
        "PREVIEW & PUBLISH",
        "Preview mode",
        "Technical bar",
        "Clean stage",
        "Clean publish output",
        "FPS & frames",
        "Stage size (W×H)",
        "Playback controls",
        "Weight & size report",
        "Trace & diagnostics",
        "Images",
        "JPEG 80",
        "Acceleration",
        "Strict mode",
    ] {
        assert!(
            texts.iter().any(|t| t == expected),
            "Preferences ▸ Publish missing '{expected}' in {texts:?}"
        );
    }

    {
        let pref = d.editor.preferences.as_mut().expect("prefs open");
        pref.publish.preview_chrome.clean = false;
        pref.publish.preview_chrome.show_fps = false;
        pref.publish.preview_chrome.show_dimensions = true;
        pref.publish.preview_chrome.show_controls = false;
        pref.publish.preview_chrome.show_load_summary = false;
        pref.publish.preview_chrome.show_trace = true;
        pref.publish.publish_clean = true;
    }
    let draft_copy = d.editor.preferences.clone().unwrap();
    d.editor.save_preferences(&d.ctx, &draft_copy);
    d.editor.preferences = None;

    let preview_cfg = d.editor.workspace.publish.preview_html_config();
    let publish_cfg = d.editor.workspace.publish.publish_html_config();
    assert!(
        !preview_cfg.chrome.effective_clean(),
        "Test Movie / Preview uses technical bar according to Preferences ▸ Publish"
    );
    assert!(!preview_cfg.chrome.show_fps);
    assert!(preview_cfg.chrome.show_dimensions);
    assert!(!preview_cfg.chrome.show_controls);
    assert!(!preview_cfg.chrome.show_load_summary);
    assert!(preview_cfg.chrome.show_trace);
    assert!(
        publish_cfg.chrome.effective_clean(),
        "Publish uses clean stage output without technical HUD"
    );
}

#[test]
fn save_as_fla_supports_all_seven_version_profiles_logs_compatibility_and_preserves_dirty_state() {
    use neoflash_export::FlaTargetProfile;

    let temp_dir =
        std::env::temp_dir().join(format!("neoflash-save-as-fla-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let mut d = Driver::new(ThemeMode::Dark);
    let native_path = temp_dir.join("canonical.nflash");
    assert!(d.editor.save_to_path(native_path.clone()));
    assert_eq!(d.editor.path.as_deref(), Some(native_path.as_path()));
    assert!(!d.editor.dirty());

    // Make the canonical project dirty and set FPS > 120 so FLA compatibility report emits a non-Exact item
    d.editor.project.fps = 144;
    assert!(d.editor.dirty());

    // Export all 7 FlaTargetProfile versions through Save As and verify round-trip + Output logging + dirty preservation
    for (idx, profile) in FlaTargetProfile::ALL.into_iter().enumerate() {
        d.editor.workspace.publish.fla_target_profile = profile;
        d.editor.output.clear();
        let target_path = if profile.is_directory() {
            temp_dir.join(format!("export_{idx}.xfl"))
        } else {
            temp_dir.join(format!("export_{idx}.fla"))
        };
        assert_eq!(
            d.editor.resolve_fla_target_profile_for_path(&target_path),
            Some(profile)
        );
        assert!(
            d.editor.save_to_path(target_path.clone()),
            "Save As FLA failed for {:?}",
            profile
        );
        assert!(
            d.editor.dirty(),
            "Save As FLA must keep native .nflash dirty state intact for {:?}",
            profile
        );
        assert_eq!(
            d.editor.path.as_deref(),
            Some(native_path.as_path()),
            "Save As FLA must not overwrite canonical .nflash path"
        );
        assert!(
            d.editor
                .output
                .iter()
                .any(|line| line.contains("frame_rate_clamped")),
            "Save As FLA must log compatibility report items to Output for {:?}: {:?}",
            profile,
            d.editor.output
        );
        assert!(
            d.editor
                .output
                .iter()
                .any(|line| line.contains(profile.label())),
            "Save As FLA must log target profile to Output for {:?}",
            profile
        );
        let reimported = neoflash_import::import_project(&target_path)
            .unwrap_or_else(|e| panic!("reimport {:?} failed: {e}", profile));
        assert_eq!(reimported.fps, 120);
    }

    // Verify suffix-based profile resolution overrides default profile when user types e.g. scene.cs6.fla or scene.flash8.fla
    d.editor.workspace.publish.fla_target_profile = FlaTargetProfile::AnimateCC;
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.cs6.fla")),
        Some(FlaTargetProfile::FlashCS6)
    );
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.cs5_5.fla")),
        Some(FlaTargetProfile::FlashCS5_5)
    );
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.cs5.fla")),
        Some(FlaTargetProfile::FlashCS5)
    );
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.cs4.fla")),
        Some(FlaTargetProfile::LegacyFlashCS4)
    );
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.flash8.fla")),
        Some(FlaTargetProfile::LegacyFlash8)
    );
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.xfl")),
        Some(FlaTargetProfile::UncompressedXfl)
    );
    d.editor.workspace.publish.fla_target_profile = FlaTargetProfile::UncompressedXfl;
    assert_eq!(
        d.editor
            .resolve_fla_target_profile_for_path(Path::new("scene.fla")),
        Some(FlaTargetProfile::AnimateCC),
        "a .fla archive must never be written as an XFL directory"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn web_app_dom_profile_persists_across_nflash_save_reopen_and_drives_publish_and_test_movie() {
    use crate::workspace::PublishProfilePreset;
    use neoflash_core::HtmlRenderMode;

    fn collect_tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, cur: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(cur).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    let rel = path
                        .strip_prefix(base)
                        .unwrap()
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        let mut map = std::collections::BTreeMap::new();
        walk(root, root, &mut map);
        map
    }

    let temp_dir =
        std::env::temp_dir().join(format!("neoflash-web-app-dom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let rt_dir = temp_dir.join("runtime");
    std::fs::create_dir_all(&rt_dir).unwrap();
    std::fs::write(rt_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    std::fs::write(rt_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();

    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.workspace.publish.warnings_as_errors = false;

    // 1. Capture baseline Default (Canvas) publish output before touching WebAppDom.
    let baseline_canvas_dir = temp_dir.join("baseline-canvas");
    assert!(d.editor.publish_html5_with_settings(
        &baseline_canvas_dir,
        &rt_dir,
        &d.editor.workspace.publish.clone()
    ));
    let baseline_canvas_files = collect_tree(&baseline_canvas_dir);

    // 2. Open File ▸ Publish Settings…, select Profile: Web App · DOM in the NF2 Profile dropdown, and apply OK.
    d.open();
    d.click("Profile, Default");
    d.click("Web App · DOM");
    assert_eq!(
        d.editor.publish_settings.as_ref().unwrap().settings.profile,
        PublishProfilePreset::WebAppDom
    );
    assert_eq!(
        d.editor
            .publish_settings
            .as_ref()
            .unwrap()
            .settings
            .render_mode,
        HtmlRenderMode::Dom
    );
    d.click("OK");

    assert_eq!(
        d.editor.workspace.publish.profile,
        PublishProfilePreset::WebAppDom
    );
    assert_eq!(d.editor.workspace.publish.render_mode, HtmlRenderMode::Dom);
    assert_eq!(d.editor.project.html_render_mode(), HtmlRenderMode::Dom);
    assert_eq!(
        d.editor.workspace.publish.publish_html_config().render_mode,
        HtmlRenderMode::Dom
    );
    assert_eq!(
        d.editor.workspace.publish.preview_html_config().render_mode,
        HtmlRenderMode::Dom,
        "Test Movie (⌘↵) preview config must also honor Web App · DOM"
    );

    // 3. Save .nflash to disk and reopen in a brand-new Editor instance with default Canvas workspace prefs.
    let nflash_path = temp_dir.join("web-app-dom.nflash");
    assert!(d.editor.save_to_path(nflash_path.clone()));
    assert!(!d.editor.dirty());

    let fresh_ctx = egui::Context::default();
    let mut reopened = Editor::from_project(&fresh_ctx, Project::blank());
    assert_eq!(
        reopened.workspace.publish.render_mode,
        HtmlRenderMode::Canvas
    );
    assert!(reopened.open_path_in_session(&fresh_ctx, nflash_path.clone()));
    assert_eq!(reopened.project.html_render_mode(), HtmlRenderMode::Dom);
    assert_eq!(reopened.workspace.publish.render_mode, HtmlRenderMode::Dom);
    assert_eq!(
        reopened.workspace.publish.profile,
        PublishProfilePreset::WebAppDom
    );
    reopened.open_publish_settings();
    assert_eq!(
        reopened
            .publish_settings
            .as_ref()
            .unwrap()
            .active_profile_label(),
        "Web App · DOM"
    );
    reopened.publish_settings = None;

    // 4. Publish (⇧F12) and Test Movie (⌘↵) from the reopened editor both produce live DOM bundles.
    let dom_publish_dir = temp_dir.join("dom-publish");
    reopened.workspace.publish.warnings_as_errors = false;
    assert!(reopened.publish_html5_with_settings(
        &dom_publish_dir,
        &rt_dir,
        &reopened.workspace.publish.clone()
    ));
    let dom_html = std::fs::read_to_string(dom_publish_dir.join("index.html")).unwrap();
    let dom_bundle = std::fs::read_to_string(dom_publish_dir.join("bundle.js")).unwrap();
    assert!(dom_html.contains("data-render-mode=\"dom\""));
    assert!(dom_html.contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\""));
    assert!(dom_bundle.contains("\"renderMode\":\"dom\""));

    let test_movie_dir = temp_dir.join("dom-test-movie");
    neoflash_export::export_project_with_config(
        &reopened.project,
        &test_movie_dir,
        &rt_dir,
        &reopened
            .workspace
            .publish
            .export_options(&reopened.project)
            .unwrap(),
        &reopened.workspace.publish.preview_html_config(),
    )
    .unwrap();
    let preview_html = std::fs::read_to_string(test_movie_dir.join("index.html")).unwrap();
    assert!(preview_html.contains("data-render-mode=\"dom\""));
    assert!(
        preview_html.contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\"")
    );

    // 5. Switching back to Profile: Default restores byte-identical Canvas output.
    d.open();
    d.click("Profile, Web App · DOM");
    d.click("Default");
    d.click("OK");
    assert_eq!(d.editor.project.html_render_mode(), HtmlRenderMode::Canvas);
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        HtmlRenderMode::Canvas
    );
    d.editor.workspace.publish.warnings_as_errors = false;

    let restored_canvas_dir = temp_dir.join("restored-canvas");
    assert!(d.editor.publish_html5_with_settings(
        &restored_canvas_dir,
        &rt_dir,
        &d.editor.workspace.publish.clone()
    ));
    assert_eq!(
        collect_tree(&restored_canvas_dir),
        baseline_canvas_files,
        "switching back to Profile: Default must produce byte-identical Canvas bundle"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn t311_publish_settings_single_undo_unchanged_clean_and_session_tab_profile_sync() {
    let mut d = Driver::new(ThemeMode::Dark);
    d.editor.history.clear();
    d.editor.saved_project = Some(d.editor.project.clone());
    assert!(!d.editor.dirty());
    assert!(!d.editor.history.can_undo());

    // 1. Confirming Publish settings with no document-stored changes pushes 0 Undo steps.
    d.open();
    d.click("OK");
    assert!(!d.editor.dirty());
    assert!(!d.editor.history.can_undo());

    // 2. Switching profile to Web App · DOM commits ONE "Publish settings" Undo step.
    d.open();
    d.click("Profile, Default");
    d.click("Web App · DOM");
    d.click("OK");
    assert!(d.editor.dirty());
    assert_eq!(
        d.editor.project.html_render_mode(),
        neoflash_core::HtmlRenderMode::Dom
    );
    assert_eq!(d.editor.undo_menu_label(), "Undo Publish settings");
    d.editor.project.validate().unwrap();

    d.editor.undo();
    assert_eq!(
        d.editor.project.html_render_mode(),
        neoflash_core::HtmlRenderMode::Canvas
    );
    assert!(!d.editor.dirty());
    assert!(
        !d.editor.history.can_undo(),
        "Publish settings confirmation must record at most 1 Undo step"
    );

    // 3. Switching between Canvas and DOM document tabs keeps workspace.publish.profile synchronized.
    d.editor.workspace.publish.profile = PublishProfilePreset::WebBalanced;
    d.editor.workspace.publish.render_mode = neoflash_core::HtmlRenderMode::Canvas;

    let mut dom_project = Project::blank();
    dom_project.set_html_render_mode(neoflash_core::HtmlRenderMode::Dom);
    let dom_tab = d.editor.open_new_session_tab(&d.ctx, dom_project, None);
    assert_eq!(dom_tab, 1);
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        neoflash_core::HtmlRenderMode::Dom
    );
    assert_eq!(
        d.editor.workspace.publish.profile,
        PublishProfilePreset::WebAppDom,
        "DOM tab must switch active publish profile preset to WebAppDom"
    );

    assert!(d.editor.switch_session_tab(&d.ctx, 0));
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        neoflash_core::HtmlRenderMode::Canvas
    );
    assert_eq!(
        d.editor.workspace.publish.profile,
        PublishProfilePreset::Default,
        "Canvas tab must clear stale WebAppDom preset on tab switch"
    );
}

#[test]
fn t320_export_modals_two_step_escape_extension_sync_and_zero_frame_safety() {
    use crate::panels::{export_audio, export_image, export_lottie, export_video};
    use neoflash_export::{AudioFormat, ImageExportFormat, LottieFormat, VideoFormat};

    // 1. Two-step Escape in Publish settings: opening a Select popup + first Escape closes ONLY the popup;
    //    second Escape closes the modal!
    let mut d = Driver::new(ThemeMode::Dark);
    d.open();
    d.click("Profile, Default");
    assert!(
        egui::Popup::is_any_open(&d.ctx),
        "clicking Profile Select must open popup"
    );
    d.key(egui::Key::Escape);
    assert!(
        !egui::Popup::is_any_open(&d.ctx),
        "first Escape must close the open Select popup"
    );
    assert!(
        d.editor.publish_settings.is_some(),
        "first Escape while Select popup was open must keep Publish settings modal open"
    );
    d.key(egui::Key::Escape);
    assert!(
        d.editor.publish_settings.is_none(),
        "second Escape with no open popup must close Publish settings modal"
    );

    // 2. Two-step Escape across Export video, Export audio, Export image, Export Lottie (+ Notes modal),
    //    plus 0-frame / 0x0 bounds safety!
    let mut zero_proj = Project::blank();
    zero_proj.width = 0;
    zero_proj.height = 0;
    zero_proj.symbols[0].timeline.frame_count = 0;

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, zero_proj);

    let run_dialog_frame =
        |ctx: &egui::Context, events: Vec<Event>, f: &mut dyn FnMut(&egui::Context)| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                    events,
                    ..Default::default()
                },
                |ctx| f(ctx),
            );
        };
    let press_escape = |ctx: &egui::Context, f: &mut dyn FnMut(&egui::Context)| {
        run_dialog_frame(
            ctx,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            f,
        );
        run_dialog_frame(
            ctx,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            f,
        );
        run_dialog_frame(ctx, vec![], f);
    };
    let fake_popup_id = egui::Id::new("t320-test-select-popup");

    // 2a. Export video on 0-frame / 0x0 project + two-step Escape + output extension sync
    editor.open_export_video(None);
    run_dialog_frame(&ctx, vec![], &mut |c| editor.export_video_modal(c));
    assert!(editor.export_video.is_some());
    {
        let draft = editor.export_video.as_ref().unwrap();
        assert!(draft.width.is_finite() && draft.width >= 2.0);
        assert!(draft.height.is_finite() && draft.height >= 2.0);
    }
    egui::Popup::open_id(&ctx, fake_popup_id);
    press_escape(&ctx, &mut |c| editor.export_video_modal(c));
    assert!(
        editor.export_video.is_some(),
        "Export video must remain open on first Escape when popup was open"
    );
    press_escape(&ctx, &mut |c| editor.export_video_modal(c));
    assert!(
        editor.export_video.is_none(),
        "Export video must close on second Escape"
    );

    let mut video_path = PathBuf::from("/tmp/movie.mp4");
    for (fmt, expected) in [
        (VideoFormat::WebM, "/tmp/movie.webm"),
        (VideoFormat::Mov, "/tmp/movie.mov"),
        (VideoFormat::Gif, "/tmp/movie.gif"),
        (VideoFormat::PngSequence, "/tmp/movie_0001.png"),
        (VideoFormat::Mp4, "/tmp/movie.mp4"),
    ] {
        export_video::sync_video_output_path_extension(&mut video_path, fmt);
        assert_eq!(video_path, PathBuf::from(expected));
    }

    // 2b. Export audio on 0-frame project + two-step Escape + output extension sync
    editor.open_export_audio(&ctx, None);
    run_dialog_frame(&ctx, vec![], &mut |c| editor.export_audio_dialog(c));
    assert!(export_audio::has_export_audio_dialog(&ctx));
    egui::Popup::open_id(&ctx, fake_popup_id);
    press_escape(&ctx, &mut |c| editor.export_audio_dialog(c));
    assert!(export_audio::has_export_audio_dialog(&ctx));
    press_escape(&ctx, &mut |c| editor.export_audio_dialog(c));
    assert!(!export_audio::has_export_audio_dialog(&ctx));

    let mut audio_path = PathBuf::from("/tmp/soundtrack.flac");
    for (fmt, expected) in [
        (AudioFormat::Wav, "/tmp/soundtrack.wav"),
        (AudioFormat::Ogg, "/tmp/soundtrack.ogg"),
        (AudioFormat::Aac, "/tmp/soundtrack.m4a"),
        (AudioFormat::Mp3, "/tmp/soundtrack.mp3"),
    ] {
        export_audio::sync_audio_output_path_extension(&mut audio_path, fmt);
        assert_eq!(audio_path, PathBuf::from(expected));
    }

    // 2c. Export image on 0-frame / 0x0 project + two-step Escape + output extension sync
    editor.open_export_image(&ctx);
    run_dialog_frame(&ctx, vec![], &mut |c| editor.export_image_dialog(c));
    assert!(export_image::has_export_image_dialog(&ctx));
    egui::Popup::open_id(&ctx, fake_popup_id);
    press_escape(&ctx, &mut |c| editor.export_image_dialog(c));
    assert!(export_image::has_export_image_dialog(&ctx));
    press_escape(&ctx, &mut |c| editor.export_image_dialog(c));
    assert!(!export_image::has_export_image_dialog(&ctx));

    let mut image_path = PathBuf::from("/tmp/frame.png");
    for (fmt, expected) in [
        (ImageExportFormat::Jpeg, "/tmp/frame.jpg"),
        (ImageExportFormat::Webp, "/tmp/frame.webp"),
        (ImageExportFormat::Svg, "/tmp/frame.svg"),
        (ImageExportFormat::Png, "/tmp/frame.png"),
    ] {
        export_image::sync_image_output_path_extension(&mut image_path, fmt);
        assert_eq!(image_path, PathBuf::from(expected));
    }

    // 2d. Export Lottie + Lottie Notes modal on 0-frame / 0x0 project + two-step Escape + extension sync
    editor.open_export_lottie(None);
    run_dialog_frame(&ctx, vec![], &mut |c| editor.export_lottie_modal(c));
    assert!(editor.export_lottie.is_some());
    editor.export_lottie.as_mut().unwrap().show_notes_modal = true;
    run_dialog_frame(&ctx, vec![], &mut |c| editor.export_lottie_modal(c));
    egui::Popup::open_id(&ctx, fake_popup_id);
    press_escape(&ctx, &mut |c| editor.export_lottie_modal(c));
    assert!(
        editor.export_lottie.as_ref().unwrap().show_notes_modal,
        "Lottie notes modal stays open on first Escape while popup is open"
    );
    press_escape(&ctx, &mut |c| editor.export_lottie_modal(c));
    assert!(
        !editor.export_lottie.as_ref().unwrap().show_notes_modal,
        "second Escape closes Lottie notes sub-modal while keeping main Lottie modal open"
    );
    assert!(editor.export_lottie.is_some());
    egui::Popup::open_id(&ctx, fake_popup_id);
    press_escape(&ctx, &mut |c| editor.export_lottie_modal(c));
    assert!(editor.export_lottie.is_some());
    press_escape(&ctx, &mut |c| editor.export_lottie_modal(c));
    assert!(editor.export_lottie.is_none());

    let mut lottie_path = PathBuf::from("/tmp/anim.json");
    export_lottie::sync_lottie_output_path_extension(&mut lottie_path, LottieFormat::DotLottie);
    assert_eq!(lottie_path, PathBuf::from("/tmp/anim.lottie"));
    export_lottie::sync_lottie_output_path_extension(&mut lottie_path, LottieFormat::Json);
    assert_eq!(lottie_path, PathBuf::from("/tmp/anim.json"));

    // 2e. Publish settings sprite-sheet / image-sequence path extension sync
    let known_seq_exts = &["json", "xml", "css", "png", "jpg", "jpeg", "webp"];
    let mut seq_path = Some("/tmp/out/spritesheet.json".to_owned());
    sync_publish_sequence_path_extension(&mut seq_path, "xml", known_seq_exts);
    assert_eq!(seq_path.as_deref(), Some("/tmp/out/spritesheet.xml"));
    sync_publish_sequence_path_extension(&mut seq_path, "css", known_seq_exts);
    assert_eq!(seq_path.as_deref(), Some("/tmp/out/spritesheet.css"));
    let mut img_seq_file = Some("/tmp/out/frame.png".to_owned());
    sync_publish_sequence_path_extension(&mut img_seq_file, "webp", known_seq_exts);
    assert_eq!(img_seq_file.as_deref(), Some("/tmp/out/frame.webp"));
}

#[test]
fn t320_failed_publish_in_publish_settings_keeps_modal_open_and_preserves_workspace_and_render_mode()
 {
    use crate::workspace::{PublishProfilePreset, PublishTarget};

    let temp_dir = std::env::temp_dir().join(format!(
        "neoflash-publish-fail-preserve-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let foreign_dir = temp_dir.join("foreign-non-empty");
    std::fs::create_dir_all(&foreign_dir).unwrap();
    std::fs::write(foreign_dir.join("keep-me.txt"), b"DO_NOT_OVERWRITE").unwrap();

    let mut d = Driver::new(ThemeMode::Dark);
    assert_eq!(
        d.editor.project.html_render_mode(),
        neoflash_core::HtmlRenderMode::Canvas
    );
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        neoflash_core::HtmlRenderMode::Canvas
    );
    assert_eq!(
        d.editor.workspace.publish.profile,
        PublishProfilePreset::Default
    );
    let initial_jpeg_quality = d.editor.workspace.publish.jpeg_quality;

    // 1. `needs_shared_output == true` (`Some(output)`) branch: switch draft to `Web App · DOM`
    //    and attempt `Publish` into a foreign non-empty folder -> must fail!
    d.open();
    {
        let draft = d.editor.publish_settings.as_mut().unwrap();
        draft
            .settings
            .apply_profile(PublishProfilePreset::WebAppDom);
        draft.settings.jpeg_quality = 42;
        draft.settings.output_folder = Some(foreign_dir.to_string_lossy().into_owned());
        draft.selected_target = PublishTarget::Html5;
    }
    d.settle();
    d.click("Publish");

    assert!(
        d.editor.publish_settings.is_some(),
        "failed Publish must keep Publish settings modal open"
    );
    assert!(
        d.editor
            .publish_settings
            .as_ref()
            .and_then(|dr| dr.error.as_deref())
            .is_some_and(|e| !e.is_empty()),
        "failed Publish must populate draft.error for InlineMessage::error banner"
    );
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        neoflash_core::HtmlRenderMode::Canvas,
        "failed Publish must NOT overwrite workspace.publish.render_mode with unconfirmed draft"
    );
    assert_eq!(
        d.editor.workspace.publish.profile,
        PublishProfilePreset::Default,
        "failed Publish must NOT leak draft profile into workspace.publish"
    );
    assert_eq!(
        d.editor.workspace.publish.jpeg_quality, initial_jpeg_quality,
        "failed Publish must leave workspace.publish settings unchanged"
    );
    assert_eq!(
        d.editor.project.html_render_mode(),
        d.editor.workspace.publish.render_mode,
        "project.html_render_mode() and workspace.publish.render_mode must remain synchronized"
    );
    assert_eq!(
        std::fs::read(foreign_dir.join("keep-me.txt")).unwrap(),
        b"DO_NOT_OVERWRITE"
    );

    // 2. `!needs_shared_output` branch: disable HTML5, enable SVG pointed at an existing directory
    //    (guaranteed write error) while draft is switched to `Web App · DOM` -> must also keep modal
    //    open with `draft.error.is_some()` and leave `workspace.publish` & `project.html_render_mode()` untouched!
    let dir_masquerading_as_svg = temp_dir.join("already-a-dir.svg");
    std::fs::create_dir_all(&dir_masquerading_as_svg).unwrap();
    {
        let draft = d.editor.publish_settings.as_mut().unwrap();
        draft.error = None;
        draft
            .settings
            .apply_profile(PublishProfilePreset::WebAppDom);
        draft.settings.html5_enabled = false;
        draft.settings.desktop_app_enabled = false;
        draft.settings.video_enabled = false;
        draft.settings.image_sequence_enabled = false;
        draft.settings.svg_enabled = true;
        draft.selected_target = PublishTarget::SvgAnimation;
        draft.settings.svg_file = Some(dir_masquerading_as_svg.to_string_lossy().into_owned());
    }
    d.settle();
    d.click("Publish");

    assert!(
        d.editor.publish_settings.is_some(),
        "failed !needs_shared_output Publish must keep Publish settings modal open"
    );
    assert!(
        d.editor
            .publish_settings
            .as_ref()
            .and_then(|dr| dr.error.as_deref())
            .is_some_and(|e| !e.is_empty()),
        "failed !needs_shared_output Publish must populate draft.error"
    );
    assert_eq!(
        d.editor.workspace.publish.render_mode,
        neoflash_core::HtmlRenderMode::Canvas
    );
    assert_eq!(
        d.editor.project.html_render_mode(),
        d.editor.workspace.publish.render_mode
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn t331_publish_image_sequence_prunes_stale_tail_frames_preserves_sprite_sheets_and_refuses_unowned_files()
 {
    let temp_dir =
        std::env::temp_dir().join(format!("neoflash-t331-publish-seq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let mut d = Driver::new(ThemeMode::Dark);
    let root_id = d.editor.project.root.clone();
    d.editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .frame_count = 6;

    // 1. Publish sprite sheet (`spritesheet.json` + `spritesheet.png`) into `pub_dir`.
    let pub_dir = temp_dir.join("published");
    let mut sheet_prefs = d.editor.workspace.publish.clone();
    sheet_prefs.image_sequence_enabled = true;
    sheet_prefs.image_sequence_sprite_sheet = true;
    sheet_prefs.image_sequence_folder = Some(pub_dir.to_string_lossy().into_owned());
    assert!(
        d.editor
            .publish_image_sequence_with_settings(None, &sheet_prefs)
    );
    let sheet_json = std::fs::read(pub_dir.join("spritesheet.json")).unwrap();
    let sheet_png = std::fs::read(pub_dir.join("spritesheet.png")).unwrap();

    // 2. Publish 6-frame sequence (`frame_0001..0006.png`) into the same `pub_dir`.
    let mut seq_prefs = sheet_prefs.clone();
    seq_prefs.image_sequence_sprite_sheet = false;
    seq_prefs.image_sequence_format = neoflash_export::ImageExportFormat::Png;
    assert!(
        d.editor
            .publish_image_sequence_with_settings(None, &seq_prefs)
    );
    for idx in 1..=6 {
        assert!(pub_dir.join(format!("frame_{idx:04}.png")).is_file());
    }

    // 3. Shorten timeline to 3 frames and re-publish: `frame_0004..0006.png` removed, sprite sheet untouched.
    d.editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .frame_count = 3;
    assert!(
        d.editor
            .publish_image_sequence_with_settings(None, &seq_prefs)
    );
    for idx in 1..=3 {
        assert!(pub_dir.join(format!("frame_{idx:04}.png")).is_file());
    }
    for stale in 4..=6 {
        assert!(!pub_dir.join(format!("frame_{stale:04}.png")).exists());
    }
    assert_eq!(
        std::fs::read(pub_dir.join("spritesheet.json")).unwrap(),
        sheet_json
    );
    assert_eq!(
        std::fs::read(pub_dir.join("spritesheet.png")).unwrap(),
        sheet_png
    );

    // 4. Unowned colliding file in a fresh folder refuses Publish cleanly without clobbering.
    let unowned_dir = temp_dir.join("unowned");
    std::fs::create_dir_all(&unowned_dir).unwrap();
    std::fs::write(unowned_dir.join("frame_0002.png"), b"unowned artist frame").unwrap();
    seq_prefs.image_sequence_folder = Some(unowned_dir.to_string_lossy().into_owned());
    assert!(
        !d.editor
            .publish_image_sequence_with_settings(None, &seq_prefs),
        "unowned frame_0002.png must block Publish image sequence"
    );
    assert_eq!(
        std::fs::read(unowned_dir.join("frame_0002.png")).unwrap(),
        b"unowned artist frame"
    );
    assert!(!unowned_dir.join("frame_0001.png").exists());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
