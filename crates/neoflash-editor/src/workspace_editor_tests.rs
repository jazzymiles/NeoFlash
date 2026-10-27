use crate::{
    app::{BottomTab, Editor, Pending, ScriptTarget, Tool},
    theme::ThemeMode,
    workspace::{PREFS_KEY, PanelId},
};
use eframe::{App, egui};
use neoflash_core::{ObjectEvent, Project};
use std::collections::BTreeMap;

#[test]
fn workspace_changes_preserve_document_selection_history_and_widget_draft() {
    let ctx = egui::Context::default();
    let mut initial = Project::demo();
    initial.format_version = 3;
    initial.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "settings-resource",
            "settings.txt",
            "text/plain",
            b"theme=dark",
        )
        .unwrap(),
    );
    let mut editor = Editor::from_project(&ctx, initial);
    editor.select_all();
    editor.transact(|e| e.project.name = "Unsaved authoring work".into());
    let project = editor.project.clone();
    let selection = editor.selection.clone();
    editor.frame = 7;
    editor.tool = Tool::Pencil;
    editor.script_target = ScriptTarget::Object;
    editor.script_event = ObjectEvent::EnterFrame;
    editor.zoom = 1.25;
    let draft_id = egui::Id::new("in-progress-widget-draft");
    ctx.data_mut(|d| d.insert_temp(draft_id, "unfinished text".to_owned()));
    for panel in PanelId::ALL {
        editor.set_panel_visible(panel, false);
    }
    editor.set_theme(&ctx, ThemeMode::Dark);
    editor.show_panel(PanelId::Actions);
    editor.restore_workspace(&ctx);
    assert_eq!(editor.project, project);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.frame, 7);
    assert!(editor.tool == Tool::Pencil);
    assert!(editor.script_target == ScriptTarget::Object);
    assert_eq!(editor.script_event, ObjectEvent::EnterFrame);
    assert_eq!(editor.zoom, 1.25);
    assert_eq!(editor.workspace.theme, ThemeMode::Dark);
    assert!(editor.dirty());
    assert!(editor.history.can_undo());
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(draft_id)).as_deref(),
        Some("unfinished text")
    );
    editor.undo();
    assert_ne!(editor.project.name, project.name);
    assert!(!editor.dirty());
    let unit = neoflash_core::ResourceUnit::from_bytes(
        "imported-resource",
        "data.txt",
        "text/plain",
        b"portable data",
    )
    .unwrap();
    editor.add_resource_units(vec![unit.clone()]).unwrap();
    assert!(editor.dirty());
    let imported = editor.project.clone();
    assert!(editor.add_resource_units(vec![unit]).is_err());
    assert_eq!(
        editor.project, imported,
        "conflicting resource import is atomic"
    );
    editor.undo();
    assert_eq!(editor.project.resources.len(), 1);
    assert!(!editor.dirty(), "Undo restores the saved resource snapshot");
    editor.redo();
    assert_eq!(editor.project.resources.len(), 2);
    assert!(editor.dirty());

    // Reuse the resource workflow for native bitmap paint/cache and undo ownership.
    let image_project =
        Project::from_json(include_str!("../../../examples/bitmap-pixels.nflash")).unwrap();
    let mut image_editor = Editor::from_project(&ctx, image_project);
    let first = crate::tests::stage_input_frame(&mut image_editor, &ctx, 0., vec![]);
    let uploaded: Vec<_> = first
        .textures_delta
        .set
        .iter()
        .filter(|(_, delta)| delta.image.size() == [16, 16])
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(uploaded.len(), 1, "the authored PNG uploads once");
    let second = crate::tests::stage_input_frame(&mut image_editor, &ctx, 0.1, vec![]);
    assert!(
        !second
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| uploaded.contains(id)),
        "unchanged images reuse their texture"
    );
    let before = image_editor.project.clone();
    image_editor.add_element(
        neoflash_core::ElementKind::Bitmap {
            resource_id: before.resources[0].id.clone(),
            width: 16,
            height: 16,
            smoothing: true,
        },
        80.,
        80.,
    );
    image_editor.project.validate().unwrap();
    let third = crate::tests::stage_input_frame(&mut image_editor, &ctx, 0.2, vec![]);
    assert_eq!(
        third
            .textures_delta
            .set
            .iter()
            .filter(|(_, delta)| delta.image.size() == [16, 16])
            .count(),
        1,
        "nearest and smoothed instances have independent sampler textures"
    );
    image_editor.undo();
    assert_eq!(
        image_editor.project, before,
        "Undo removes only the new placement"
    );

    // The same authoring workflow covers styled layout, cache reuse and rotated clipping.
    let (mut text_editor, text_ctx, page) = crate::tests::rectangle_stage();
    let original = text_editor.project.clone();
    let kind = neoflash_core::ElementKind::TextBox {
        text: "A shared text layout wraps this paragraph inside a narrow field.".into(),
        width: 80.,
        height: 28.,
        multiline: true,
        word_wrap: true,
        format: neoflash_core::TextFormat {
            font: "Arial".into(),
            size: 18.,
            bold: true,
            ..Default::default()
        },
    };
    let transform = neoflash_core::Transform {
        x: 120.,
        y: 120.,
        rotation: 27.,
        ..Default::default()
    };
    text_editor.add_element_transformed(kind.clone(), transform);
    text_editor.load_project_fonts(&text_ctx);
    let output = crate::tests::stage_input_frame(&mut text_editor, &text_ctx, 0.1, vec![]);
    let mut vertices = 0;
    for shape in output.shapes {
        if let egui::Shape::Mesh(mesh) = shape.shape {
            for vertex in &mesh.vertices {
                let world = (vertex.pos - page.min).to_pos2();
                let local =
                    crate::stage::inverse(neoflash_core::transform_matrix(transform), world)
                        .unwrap();
                assert!(
                    (-0.02..=80.02).contains(&local.x) && (-0.02..=28.02).contains(&local.y),
                    "text ink clips before rotation: {local:?}"
                );
                vertices += 1;
            }
        }
    }
    assert!(vertices > 0, "the field paints visible glyphs");
    let _ = text_ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let first = crate::text_boxes::layout(&painter, &kind).unwrap();
        let second = crate::text_boxes::layout(&painter, &kind).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert!(first.lines.len() > 1);
        let mut wider = kind.clone();
        if let neoflash_core::ElementKind::TextBox { width, .. } = &mut wider {
            *width = 300.;
        }
        assert!(
            crate::text_boxes::layout(&painter, &wider)
                .unwrap()
                .lines
                .len()
                < first.lines.len()
        );
    });
    text_editor.undo();
    assert_eq!(text_editor.project, original);
}

#[test]
fn f9_reveals_hidden_actions_even_while_a_text_field_has_focus() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let original = editor.project.clone();
    editor.set_panel_visible(PanelId::Actions, false);
    let mut text = "do not replace this draft".to_owned();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id_salt("typing-test"))
                .request_focus();
        });
    });
    assert!(ctx.wants_keyboard_input());
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::F9,
                physical_key: Some(egui::Key::F9),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| editor.shortcuts(ctx),
    );
    assert!(editor.workspace.is_panel_visible(PanelId::Actions));
    assert_eq!(editor.bottom, BottomTab::Actions);
    assert_eq!(editor.project, original);
    assert_eq!(text, "do not replace this draft");
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn new_and_open_keep_workspace_preferences_and_save_only_preferences() {
    #[derive(Default)]
    struct Storage(BTreeMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_theme(&ctx, ThemeMode::Dark);
    editor.set_panel_visible(PanelId::Library, false);
    editor.show_panel(PanelId::Actions);
    let preferences = editor.workspace.clone();
    editor.perform(Pending::New, &ctx);
    assert_eq!(editor.workspace, preferences);
    editor.open_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/script-recovery.nflash"),
        &ctx,
    );
    assert_eq!(editor.workspace, preferences);
    assert!(ctx.style().visuals.dark_mode);
    assert!(!editor.dirty());
    let mut storage = Storage::default();
    App::save(&mut editor, &mut storage);
    assert_eq!(storage.0.len(), 1);
    assert!(storage.0.contains_key(PREFS_KEY));
    assert!(!storage.0[PREFS_KEY].contains("scripts"));
    assert!(!editor.persist_egui_memory());
}
