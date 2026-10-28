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

    // One file-drop batch covers editable SVG, rich-paint fallback and atomic Undo.
    let graphics_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/graphic-import");
    std::fs::create_dir_all(&graphics_dir).unwrap();
    let vector_file = graphics_dir.join("solid.svg");
    let png_file = graphics_dir.join("picture.png");
    let pixels =
        Project::from_json(include_str!("../../../examples/bitmap-pixels.nflash")).unwrap();
    std::fs::write(&png_file, pixels.resources[0].decoded_bytes().unwrap()).unwrap();
    let rich_file = graphics_dir.join("gradient.svg");
    std::fs::write(&vector_file,r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><path fill="#e64b41" d="M10 10H90V90H10Z"/></svg>"##).unwrap();
    std::fs::write(&rich_file,r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/></svg>"##).unwrap();
    let graphics_ctx = egui::Context::default();
    let mut graphic_editor = Editor::from_project(&graphics_ctx, Project::blank());
    graphic_editor.library_panel.selected_folder = "Artwork/Imported".into();
    // Import into a selected folder still creates visible artwork, in one Undo.
    graphic_editor
        .project
        .symbol_mut(&graphic_editor.project.root.clone())
        .unwrap()
        .timeline
        .layers[0]
        .properties
        .role = neoflash_core::LayerRole::Folder;
    graphic_editor
        .project
        .symbol_mut(&graphic_editor.project.root.clone())
        .unwrap()
        .timeline
        .layers[0]
        .keyframes
        .clear();
    graphic_editor.project.format_version = 4;
    let before = graphic_editor.project.clone();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1024., 768.),
        )),
        events: vec![egui::Event::PointerMoved(egui::pos2(500., 380.))],
        dropped_files: vec![
            egui::DroppedFile {
                path: Some(vector_file.clone()),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(rich_file),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(png_file),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let _ = graphics_ctx.run(raw, |ctx| graphic_editor.stage(ctx));
    graphic_editor.project.validate().unwrap();
    assert_eq!(graphic_editor.selection.len(), 3);
    assert_eq!(graphic_editor.symbol().timeline.layers.len(), 2);
    assert_eq!(graphic_editor.layer, 1);
    assert_eq!(
        neoflash_core::evaluate(&graphic_editor.project, graphic_editor.symbol_id(), 0).len(),
        3
    );
    assert_eq!(
        graphic_editor.project.symbols.len(),
        before.symbols.len() + 3
    );
    assert!(
        graphic_editor
            .project
            .symbols
            .iter()
            .filter(|s| s.id != graphic_editor.project.root)
            .all(|s| s.folder == "Artwork/Imported")
    );
    let kinds: Vec<_> = graphic_editor
        .project
        .symbols
        .iter()
        .filter(|s| s.id != graphic_editor.project.root)
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .map(|e| &e.kind)
        .collect();
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, neoflash_core::ElementKind::Path { .. }))
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, neoflash_core::ElementKind::Bitmap { .. }))
    );
    let imported = graphic_editor.project.clone();
    let invalid_file = graphics_dir.join("not-a-graphic.txt");
    std::fs::write(&invalid_file, "not a graphic").unwrap();
    assert!(
        graphic_editor
            .import_graphics_paths(&[vector_file, invalid_file], None)
            .is_err()
    );
    assert_eq!(
        graphic_editor.project, imported,
        "a failed file batch changes nothing"
    );
    graphic_editor.undo();
    assert_eq!(
        graphic_editor.project, before,
        "one Undo removes the complete drop batch"
    );

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

    // Resizing a paired property into stacked rows preserves its live draft,
    // not just its authored value. Enter remains the only commit here.
    let fields_ctx = egui::Context::default();
    crate::theme::initialize_editor_style(&fields_ctx);
    let mut values = [3., 4.];
    let mut render_fields = |width: f32, events| {
        let mut responses = None;
        let _ = fields_ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 300.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let [ref mut x, ref mut y] = values;
                    responses = Some(crate::paint_controls::numeric_pair(
                        ui,
                        ["X", "Y"],
                        [x, y],
                        -1000. ..=1000.,
                        1.,
                    ));
                });
            },
        );
        (responses.unwrap(), values)
    };
    let (initial_fields, _) = render_fields(280., vec![]);
    let field = initial_fields[0].id;
    fields_ctx.memory_mut(|memory| memory.request_focus(field));
    render_fields(280., vec![]);
    let (wrapped, uncommitted) = render_fields(
        160.,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    mac_cmd: true,
                    ..Default::default()
                },
            },
            egui::Event::Text("123".into()),
        ],
    );
    assert_eq!(wrapped[0].id, field);
    assert!(wrapped[0].has_focus());
    assert_eq!(uncommitted[0], 3.);
    assert_eq!(
        fields_ctx
            .data(|data| data.get_temp::<String>(field))
            .as_deref(),
        Some("123")
    );
    let (unwrapped, committed) = render_fields(
        280.,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(unwrapped[0].id, field);
    assert_eq!(committed[0], 123.);
}

#[test]
fn f9_reveals_hidden_actions_even_while_a_text_field_has_focus() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 8;
    project.symbols[0]
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            frame: 4,
            script: neoflash_core::Script {
                language: neoflash_core::ScriptLanguage::JavaScript,
                source: "this.stop();".into(),
            },
        });
    let mut editor = Editor::from_project(&ctx, project);
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

    // A timeline invocation replaces a stale code-tab target, while retaining
    // source-file selection and document contents. Revealing the dock focuses
    // the requested frame's editor rather than the widget used before F9.
    editor.script_target = ScriptTarget::Source;
    editor.selected_source = Some("remembered-source".into());
    editor.frame = 4;
    editor.set_playing(true);
    editor.open_frame_actions();
    assert!(editor.script_target == ScriptTarget::Frame);
    assert_eq!(editor.frame, 4);
    assert!(!editor.playing);
    assert_eq!(editor.selected_source.as_deref(), Some("remembered-source"));
    let frame_editor = editor.actions_editor_id();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.prepare_docked_panel_focus(ctx);
        egui::CentralPanel::default().show(ctx, |ui| editor.actions_contents(ui));
    });
    assert!(ctx.memory(|memory| memory.has_focus(frame_editor)));
    editor.frame = 3;
    assert_ne!(frame_editor, editor.actions_editor_id());
    assert_eq!(editor.project, original);
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
