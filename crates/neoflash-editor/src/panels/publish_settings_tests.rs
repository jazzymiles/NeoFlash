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
        d.click("Shared runtime");
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
        d.click("Shared runtime");
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
    d.click("Shared runtime");
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
        painted_text(&d.output)
            .iter()
            .any(|t| t == "Audio: MP3 160 kbps"),
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
