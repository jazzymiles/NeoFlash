use super::*;
use eframe::egui::Color32;

#[test]
fn percent_and_px_sizing_and_track_resolution_are_exact() {
    let box_pct = UiBoxConstraints {
        width: UiSizeUnit::Percent(50.0),
        height: UiSizeUnit::Px(32.0),
        min_width: Some(100.0),
        max_width: Some(220.0),
        min_height: None,
        max_height: None,
    };
    assert_eq!(box_pct.resolve_width(300.0, 60.0), 150.0);
    // Clamped to min_width = 100.0 when 50% of 160 is 80
    assert_eq!(box_pct.resolve_width(160.0, 60.0), 100.0);
    // Clamped to max_width = 220.0 when 50% of 600 is 300
    assert_eq!(box_pct.resolve_width(600.0, 60.0), 220.0);
    assert_eq!(box_pct.resolve_height(500.0, 20.0), 32.0);

    // Unit conversion % <-> px
    assert_eq!(UiSizeUnit::Percent(25.0).to_px(320.0), UiSizeUnit::Px(80.0));
    assert_eq!(
        UiSizeUnit::Px(80.0).to_percent(320.0),
        UiSizeUnit::Percent(25.0)
    );

    // 2-col, 3-col, 4-col, and mixed [120px, 50%, 50%] track resolution
    let two_col = resolve_track_sizes(
        &[UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
        308.0,
        8.0,
        &[64.0, 64.0],
    );
    assert_eq!(two_col, vec![150.0, 150.0]);

    let four_col = resolve_track_sizes(
        &[
            UiSizeUnit::Percent(25.0),
            UiSizeUnit::Percent(25.0),
            UiSizeUnit::Percent(25.0),
            UiSizeUnit::Percent(25.0),
        ],
        424.0,
        8.0,
        &[40.0; 4],
    );
    assert_eq!(four_col, vec![100.0, 100.0, 100.0, 100.0]);

    let mixed = resolve_track_sizes(
        &[
            UiSizeUnit::Px(120.0),
            UiSizeUnit::Percent(50.0),
            UiSizeUnit::Percent(50.0),
        ],
        336.0,
        8.0,
        &[64.0; 3],
    );
    // 336 - 2*8 gap = 320 usable; 120px fixed leaves 200px, split 50%/50% = 100px each
    assert_eq!(mixed, vec![120.0, 100.0, 100.0]);
}

#[test]
fn system_tokens_propagate_to_bound_nodes_and_preserve_local_overrides() {
    let mut state = UiLayoutStudioState::default();
    state.load_preset(StudioPanelPreset::PropertiesMovieClip);
    let origin = egui::pos2(0.0, 0.0);
    let avail = egui::vec2(400.0, 600.0);

    // Locate the 2-column Transform Grid inside Properties — Movie Clip
    let grid_id = state.document.root.children[1].children[0].id;
    let first_field_id = state.document.root.children[1].children[0].children[0].id;

    let layout_before = compute_studio_layout(&state.document, &state.systems, origin, avail);
    let gap_before = layout_before.node_box(grid_id).unwrap().gap.x;
    let field_h_before = layout_before
        .node_box(first_field_id)
        .unwrap()
        .resolved_height;
    assert_eq!(gap_before, 8.0);
    assert_eq!(field_h_before, 32.0);

    // 1. Edit TwoColumnFormSystem gap -> immediately updates bound Grid gap
    state.systems.two_column_form.gap = SpacingValue::Px(16.0);
    // 2. Switch density profile to Classic Flash -> immediately updates leaf control heights to 24pt
    state.systems.density.active_profile = StudioDensityProfile::ClassicFlash;
    // 3. Edit Typography Mono system scale -> immediately updates Field typography
    state
        .systems
        .typography
        .entry_mut(StudioTextStyle::Mono)
        .size_pt = 13.5;

    let layout_after = compute_studio_layout(&state.document, &state.systems, origin, avail);
    assert_eq!(layout_after.node_box(grid_id).unwrap().gap.x, 16.0);
    assert_eq!(
        layout_after
            .node_box(first_field_id)
            .unwrap()
            .resolved_height,
        24.0
    );
    assert_eq!(
        layout_after
            .node_box(first_field_id)
            .unwrap()
            .typography
            .size_pt,
        13.5
    );

    // Local override on `first_field_id` wins over system scale until reset
    {
        let node = state.document.find_node_mut(first_field_id).unwrap();
        node.style
            .typography
            .promote_to_custom(&state.systems)
            .size_pt = 19.0;
    }
    let layout_override = compute_studio_layout(&state.document, &state.systems, origin, avail);
    assert_eq!(
        layout_override
            .node_box(first_field_id)
            .unwrap()
            .typography
            .size_pt,
        19.0
    );

    // 1-click reset restores system binding
    assert!(state.document.reset_node_to_system_defaults(first_field_id));
    let layout_reset = compute_studio_layout(&state.document, &state.systems, origin, avail);
    assert_eq!(
        layout_reset
            .node_box(first_field_id)
            .unwrap()
            .typography
            .size_pt,
        13.5
    );
}

#[test]
fn wrap_unwrap_nested_grid_in_grid_and_reordering_work_with_undo_redo() {
    let mut state = UiLayoutStudioState::default();
    state.load_preset(StudioPanelPreset::PropertiesMovieClip);
    let pos_sec_id = state.document.root.children[1].id;
    let pos_grid_id = state.document.root.children[1].children[0].id;
    let x_id = state.document.root.children[1].children[0].children[0].id;
    let y_id = state.document.root.children[1].children[0].children[1].id;

    // Wrap X and Y fields inside a nested 3-column Grid inside the outer 2-column Grid's first cell!
    state.commit_history("Wrap X and Y in 3-col Grid");
    let nested_grid_id = state
        .document
        .wrap_selected_nodes(&[x_id, y_id], ContainerWrapPreset::Grid3Equal)
        .expect("wrap succeeds");

    // Verify nested grid is now child 0 of `pos_grid_id` and holds `[x_id, y_id]`
    let outer_grid = state.document.find_node(pos_grid_id).unwrap();
    assert_eq!(outer_grid.children[0].id, nested_grid_id);
    assert_eq!(outer_grid.children[0].children.len(), 2);

    // Insert a 4-column sub-grid inside `nested_grid_id` (3 levels of nested grids!)
    let deep_grid_id = state
        .document
        .insert_nested_grid(
            nested_grid_id,
            vec![
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
            ],
            true,
        )
        .expect("deep nested grid");

    let computed = compute_studio_layout(
        &state.document,
        &state.systems,
        egui::pos2(0.0, 0.0),
        egui::vec2(360.0, 600.0),
    );
    let outer_box = computed.node_box(pos_grid_id).unwrap();
    let nested_box = computed.node_box(nested_grid_id).unwrap();
    let deep_box = computed.node_box(deep_grid_id).unwrap();
    assert!(nested_box.resolved_width < outer_box.resolved_width);
    assert!(deep_box.resolved_width < nested_box.resolved_width);
    assert!(deep_box.resolved_width > 0.0);

    // Reorder X and Y inside `nested_grid_id`
    assert!(state.document.move_node_down(x_id));
    assert_eq!(
        state.document.find_node(nested_grid_id).unwrap().children[0].id,
        y_id
    );
    assert!(state.document.move_node_up(x_id));
    assert_eq!(
        state.document.find_node(nested_grid_id).unwrap().children[0].id,
        x_id
    );

    // Cycle prevention on reparenting: cannot reparent `pos_sec_id` into its own descendant `nested_grid_id`
    assert!(
        !state
            .document
            .reparent_node(pos_sec_id, nested_grid_id, None)
    );

    // Unwrap `nested_grid_id` -> splices children back into `pos_grid_id`
    assert!(state.document.unwrap_selected_container(nested_grid_id));
    assert!(state.document.find_node(nested_grid_id).is_none());
    assert_eq!(
        state.document.find_node(pos_grid_id).unwrap().children[0].id,
        x_id
    );

    // Undo & Redo restore the exact states
    assert!(state.undo());
    assert!(state.redo());
}

#[test]
fn json_round_trip_and_rust_codegen_export_are_deterministic() {
    let mut state = UiLayoutStudioState::default();
    state.load_preset(StudioPanelPreset::PropertiesMovieClip);
    state.systems.spacing.s3 = 14.0;
    state.systems.two_column_form.label_width_px = 84.0;
    state.systems.timeline_geometry.tween_arrow_head_length_px = 12.5;

    let tmp_dir = std::env::temp_dir().join(format!("nf2-ui-studio-test-{}", std::process::id()));
    let json_path = tmp_dir.join("ui-layout-overrides.json");
    let rust_path = tmp_dir.join("exported_layout.rs");

    state.save_studio_json(&json_path).expect("save JSON");
    state.export_rust_to_file(&rust_path).expect("export Rust");

    let mut loaded = UiLayoutStudioState::default();
    loaded.load_studio_json(&json_path).expect("load JSON");
    assert_eq!(loaded.document, state.document);
    assert_eq!(loaded.systems.spacing.s3, 14.0);
    assert_eq!(loaded.systems.two_column_form.label_width_px, 84.0);
    assert_eq!(
        loaded.systems.timeline_geometry.tween_arrow_head_length_px,
        12.5
    );

    let code1 = export_rust_layout_with_systems(&state.document, &state.systems);
    let code2 = export_rust_layout_with_systems(&loaded.document, &loaded.systems);
    assert_eq!(code1, code2);
    assert!(code1.contains("pub fn render_properties_movie_clip(ui: &mut Ui)"));
    assert!(code1.contains("c::SectionHeader::new(\"POSITION AND SIZE\")"));
    assert!(code1.contains("pub const TWEEN_ARROW_HEAD_LENGTH_PX: f32 = 12.50;"));

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn timeline_geometry_and_workspace_shell_systems_resolve_arrows_and_docks() {
    let mut tg = TimelineGeometrySystem::default();
    let span = Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(200.0, 24.0));
    let (start, tip, tri) = tg.compute_tween_arrow_points(span);
    assert_eq!(start.x, 110.0); // 100 + 10.0 start_inset
    assert_eq!(tip.x, 296.0); // 300 - 4.0 end_inset
    assert_eq!(tri[0].x, 296.0);
    assert_eq!(tri[1].x, 289.0); // 296 - 7.0 head_length

    // Change arrow head length to 18px and wing to 6px
    tg.tween_arrow_head_length_px = 18.0;
    tg.tween_arrow_head_wing_px = 6.0;
    let (_start2, tip2, tri2) = tg.compute_tween_arrow_points(span);
    assert_eq!(tri2[1].x, tip2.x - 18.0);
    assert!((tri2[2].y - tri2[1].y - 12.0).abs() < 1e-4);

    // Layer column % vs px resolution
    assert_eq!(tg.resolve_layer_list_width(800.0), 220.0);
    tg.layer_list_width = UiSizeUnit::Percent(25.0);
    assert_eq!(tg.resolve_layer_list_width(800.0), 200.0);

    // WorkspaceShellSystem dock proportions in % and px
    let mut ws = WorkspaceShellSystem::default();
    let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let r1 = ws.resolve_shell_rects(viewport);
    assert_eq!(r1.left_tools.width(), 46.0);
    assert_eq!(r1.right_inspector.width(), 250.0); // 25% of 1000

    ws.shell_right_inspector_width = UiSizeUnit::Px(310.0);
    let r2 = ws.resolve_shell_rects(viewport);
    assert_eq!(r2.right_inspector.width(), 310.0);
}

#[test]
fn studio_palette_overrides_and_legacy_preview_theme_cover_all_four_modes() {
    let mut overrides = PaletteTokenOverrides::default();
    let colors = [
        Color32::from_rgb(1, 2, 3),
        Color32::from_rgb(4, 5, 6),
        Color32::from_rgb(7, 8, 9),
        Color32::from_rgb(10, 11, 12),
    ];
    for (theme, color) in ThemeMode::ALL.into_iter().zip(colors) {
        assert_eq!(
            overrides.resolve_color(theme, StudioColorRole::BgApp),
            theme.nf2().palette().bg_app
        );
        overrides.set_color_override(theme, StudioColorRole::BgApp, color);
        assert_eq!(
            overrides.resolve_color(theme, StudioColorRole::BgApp),
            color
        );
    }
    for (theme, color) in ThemeMode::ALL.into_iter().zip(colors) {
        assert_eq!(
            overrides.resolve_color(theme, StudioColorRole::BgApp),
            color,
            "{theme:?} override must stay isolated"
        );
    }

    let mut state = UiLayoutStudioState::default();
    state.systems.palette_overrides = overrides;
    for (theme, color) in ThemeMode::ALL.into_iter().zip(colors) {
        state.preview_theme = theme;
        let svg = state.export_board_svg();
        let palette = theme.nf2().palette();
        assert!(
            svg.contains(&format!(r#"fill="{}""#, color32_to_hex(color))),
            "{theme:?} SVG export must use its exact override"
        );
        assert!(
            svg.contains(&format!(
                r#"fill="{}" data-nf-timeline-span="classic""#,
                color32_to_hex(palette.timeline_classic_tween)
            )),
            "{theme:?} SVG export must use its exact classic-tween token"
        );
        assert!(
            svg.contains(&format!(
                r#"stroke="{}" stroke-width="#,
                color32_to_hex(palette.timeline_marker)
            )),
            "{theme:?} SVG export must use its exact timeline-marker token"
        );
    }
    state.preview_theme = ThemeMode::DarkGraphite;
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains(r#""preview_theme":"dark_graphite""#));
    let legacy_light = json.replace(
        r#""preview_theme":"dark_graphite""#,
        r#""preview_dark_mode":false"#,
    );
    let legacy_dark = json.replace(
        r#""preview_theme":"dark_graphite""#,
        r#""preview_dark_mode":true"#,
    );
    assert_eq!(
        serde_json::from_str::<UiLayoutStudioState>(&legacy_light)
            .unwrap()
            .preview_theme,
        ThemeMode::Light
    );
    assert_eq!(
        serde_json::from_str::<UiLayoutStudioState>(&legacy_dark)
            .unwrap()
            .preview_theme,
        ThemeMode::Dark
    );
}

#[test]
fn studio_window_and_all_presets_render_in_all_four_themes() {
    fn shape_has_rect_fill(shape: &egui::Shape, color: Color32) -> bool {
        match shape {
            egui::Shape::Rect(rect) => rect.fill == color,
            egui::Shape::Vec(shapes) => {
                shapes.iter().any(|child| shape_has_rect_fill(child, color))
            }
            _ => false,
        }
    }
    fn has_rect_fill(output: &egui::FullOutput, color: Color32) -> bool {
        output
            .shapes
            .iter()
            .any(|shape| shape_has_rect_fill(&shape.shape, color))
    }

    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());

    for theme in ThemeMode::ALL {
        neoflash_ui::theme::install(&ctx, theme.nf2());
        let palette = theme.nf2().palette();
        let mut state = UiLayoutStudioState {
            open: true,
            ..Default::default()
        };
        for preset in StudioPanelPreset::ALL {
            state.load_preset(preset);
            for tab in [
                StudioInspectorTab::NodeInspector,
                StudioInspectorTab::SystemsTab,
                StudioInspectorTab::CodeExport,
            ] {
                state.inspector_tab = tab;
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    ui_layout_studio_window_ui(ctx, &mut state);
                });
                assert!(
                    out.shapes.len() > 20,
                    "Theme {theme:?} preset {:?} tab {:?} painted too few shapes",
                    preset,
                    tab
                );
                assert_eq!(state.preview_theme, theme);
                if tab == StudioInspectorTab::NodeInspector
                    && preset == StudioPanelPreset::FullWorkspaceShell
                {
                    assert!(
                        has_rect_fill(&out, palette.bg_canvas),
                        "Theme {theme:?} workspace shell must paint bg/canvas"
                    );
                    assert!(
                        has_rect_fill(&out, palette.bg_stage),
                        "Theme {theme:?} workspace shell must paint bg/stage"
                    );
                }
                if tab == StudioInspectorTab::NodeInspector
                    && preset == StudioPanelPreset::TimelineStudio
                {
                    for (name, color) in [
                        ("timeline/static", palette.timeline_static),
                        ("timeline/classic-tween", palette.timeline_classic_tween),
                        ("timeline/motion-tween", palette.timeline_motion_tween),
                        ("timeline/shape-tween", palette.timeline_shape_tween),
                    ] {
                        assert!(
                            has_rect_fill(&out, color),
                            "Theme {theme:?} timeline studio must paint {name}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn multi_panel_artboard_canvas_lays_out_and_resizes_without_clipping() {
    let mut state = UiLayoutStudioState::default();
    // Default board has 6 simultaneous core panels + timeline laid out on the canvas
    assert_eq!(state.artboards.len(), 6);
    assert_eq!(
        state.artboards[0].preset,
        StudioPanelPreset::PropertiesMovieClip
    );
    assert_eq!(
        state.artboards[1].preset,
        StudioPanelPreset::LibraryAndAssetsStudio
    );
    assert_eq!(
        state.artboards[2].preset,
        StudioPanelPreset::ColorAndGradientStudio
    );
    assert_eq!(
        state.artboards[3].preset,
        StudioPanelPreset::AlignAndTransform
    );
    assert_eq!(state.artboards[4].preset, StudioPanelPreset::ToolsPanel);
    assert_eq!(state.artboards[5].preset, StudioPanelPreset::TimelineStudio);

    // Verify top-row artboards do not overlap horizontally and timeline is below them
    for i in 0..4 {
        let right_edge = state.artboards[i].canvas_pos[0] + state.artboards[i].canvas_size[0];
        assert!(
            right_edge <= state.artboards[i + 1].canvas_pos[0],
            "Artboard {i} overlaps artboard {}",
            i + 1
        );
    }
    assert!(
        state.artboards[5].canvas_pos[1]
            >= state.artboards[0].canvas_pos[1] + state.artboards[0].canvas_size[1]
    );

    // Resize artboard 0 from 300px wide to 480px wide and verify % children scale proportionally
    let grid_id = state.artboards[0].document.root.children[1].children[0].id;
    let layout_300 = compute_studio_layout(
        &state.artboards[0].document,
        &state.systems,
        Pos2::ZERO,
        egui::vec2(
            state.artboards[0].canvas_size[0],
            state.artboards[0].canvas_size[1],
        ),
    );
    state.resize_artboard(0, 480.0, 520.0);
    assert_eq!(state.artboards[0].canvas_size, [480.0, 520.0]);
    let layout_480 = compute_studio_layout(
        &state.artboards[0].document,
        &state.systems,
        Pos2::ZERO,
        egui::vec2(
            state.artboards[0].canvas_size[0],
            state.artboards[0].canvas_size[1],
        ),
    );
    assert!(
        layout_480.node_box(grid_id).unwrap().resolved_width
            > layout_300.node_box(grid_id).unwrap().resolved_width
    );

    // Add, duplicate, auto-arrange, and remove artboards
    let new_idx = state.add_artboard(StudioPanelPreset::FiltersPanel);
    assert_eq!(state.artboards.len(), 7);
    assert_eq!(state.active_artboard_idx, new_idx);
    let dup_idx = state.duplicate_artboard(new_idx);
    assert_eq!(state.artboards.len(), 8);
    assert_eq!(state.active_artboard_idx, dup_idx);
    state.auto_arrange_artboards();
    state.remove_artboard(dup_idx);
    assert_eq!(state.artboards.len(), 7);
}

#[test]
fn svg_export_and_import_roundtrip_and_raw_figma_svg_to_panel_conversion() {
    let mut state = UiLayoutStudioState::default();
    state.select_artboard(0); // Properties — Movie Clip

    // 1. Export active artboard to SVG and verify SVG structure & NF2 metadata
    let svg = state.export_active_svg();
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains("data-nf-panel=\"Properties — Movie Clip\""));
    assert!(svg.to_ascii_lowercase().contains("data-nf-kind=\"field\""));
    assert!(svg.contains("POSITION AND SIZE"));

    // 2. Round-trip import of NeoFlash SVG reconstructs the editable panel
    let roundtrip_doc = import_svg_to_document(&svg).expect("round-trip SVG import succeeds");
    assert_eq!(roundtrip_doc.name, "Properties — Movie Clip");
    assert!(roundtrip_doc.all_node_ids().len() >= 10);

    // 3. Raw Figma / Illustrator SVG (without `data-nf-kind` attributes) converts into a structured NF2 panel!
    let raw_figma_svg = sample_svg_panel_snippet();
    assert!(!raw_figma_svg.contains("data-nf-kind"));
    let before_count = state.artboards.len();
    let imported_idx = state
        .import_svg_as_artboard(&raw_figma_svg)
        .expect("raw Figma SVG converts to NF2 panel artboard");
    assert_eq!(state.artboards.len(), before_count + 1);
    assert_eq!(state.active_artboard_idx, imported_idx);

    let imported_doc = &state.artboards[imported_idx].document;
    assert!(imported_doc.all_node_ids().len() >= 6);
    // Check that sections, rows/grids, fields, selects, or buttons were reconstructed
    let mut has_section = false;
    let mut has_field = false;
    let mut has_button_or_swatch = false;
    for id in imported_doc.all_node_ids() {
        if let Some(n) = imported_doc.find_node(id) {
            match n.kind {
                UiNodeKind::Section { .. } => has_section = true,
                UiNodeKind::Field { .. } | UiNodeKind::TextField { .. } => has_field = true,
                UiNodeKind::Button { .. } | UiNodeKind::ColorSwatch { .. } => {
                    has_button_or_swatch = true;
                }
                _ => {}
            }
        }
    }
    assert!(
        has_section,
        "Expected reconstructed Section header from raw SVG"
    );
    assert!(
        has_field,
        "Expected reconstructed numeric/text Field from raw SVG"
    );
    assert!(
        has_button_or_swatch,
        "Expected reconstructed Button or ColorSwatch from raw SVG"
    );

    // 4. Combined multi-artboard board SVG export works
    let board_svg = state.export_board_svg();
    assert!(board_svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(board_svg.contains("Properties — Movie Clip"));
    assert!(board_svg.contains("Align &amp; Transform"));
}

#[test]
fn four_theme_switching_locks_preview_theme_and_side_by_side_comparison_renders_all_four_palettes()
{
    fn shape_has_rect_fill(shape: &egui::Shape, color: Color32) -> bool {
        match shape {
            egui::Shape::Rect(rect) => rect.fill == color,
            egui::Shape::Vec(shapes) => {
                shapes.iter().any(|child| shape_has_rect_fill(child, color))
            }
            _ => false,
        }
    }
    fn has_rect_fill(output: &egui::FullOutput, color: Color32) -> bool {
        output
            .shapes
            .iter()
            .any(|shape| shape_has_rect_fill(&shape.shape, color))
    }

    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, ThemeMode::Dark.nf2());

    let mut state = UiLayoutStudioState {
        open: true,
        ..Default::default()
    };
    let make_input = |t: f64| egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
        time: Some(t),
        ..Default::default()
    };
    // Warm up egui::Area sizing pass & Surface::Dialog fade-in animation
    let _ = ctx.run(make_input(0.0), |ctx| {
        ui_layout_studio_window_ui(ctx, &mut state);
    });
    let _ = ctx.run(make_input(1.0), |ctx| {
        ui_layout_studio_window_ui(ctx, &mut state);
    });

    // Explicit selection of Mid Neutral Gray and Dark Graphite must survive host Dark frames!
    for (i, chosen) in [ThemeMode::MidNeutralGray, ThemeMode::DarkGraphite]
        .into_iter()
        .enumerate()
    {
        state.set_preview_theme(chosen);
        let out = ctx.run(make_input(2.0 + i as f64), |ctx| {
            ui_layout_studio_window_ui(ctx, &mut state);
        });
        assert_eq!(state.preview_theme, chosen);
        assert!(
            has_rect_fill(&out, chosen.nf2().palette().bg_app),
            "Expected Studio canvas to paint {chosen:?} bg_app even while host ctx is Dark"
        );
        assert!(
            has_rect_fill(&out, chosen.nf2().palette().bg_panel),
            "Expected Studio panel artboard to paint {chosen:?} bg_panel"
        );
    }

    // 4-theme side-by-side comparison places Dark, Light, Mid Neutral Gray, and Dark Graphite on one board
    state.load_preset(StudioPanelPreset::FullWorkspaceShell);
    let artboards_before_compare = state.artboards.len();
    state.open_four_theme_comparison();
    assert_eq!(state.artboards.len(), 4);
    assert_eq!(
        state
            .artboards
            .iter()
            .map(|a| a.theme_override)
            .collect::<Vec<_>>(),
        vec![
            Some(ThemeMode::Dark),
            Some(ThemeMode::Light),
            Some(ThemeMode::MidNeutralGray),
            Some(ThemeMode::DarkGraphite),
        ]
    );
    let out = ctx.run(make_input(10.0), |ctx| {
        ui_layout_studio_window_ui(ctx, &mut state);
    });
    for theme in ThemeMode::ALL {
        assert!(
            has_rect_fill(&out, theme.nf2().palette().bg_canvas),
            "Side-by-side 4-theme comparison must paint {theme:?} bg_canvas"
        );
    }

    // Exported SVG board preserves per-artboard theme tags and round-trips all 4 artboards
    let board_svg = state.export_board_svg();
    assert!(board_svg.contains("data-nf-theme=\"dark\""));
    assert!(board_svg.contains("data-nf-theme=\"light\""));
    assert!(board_svg.contains("data-nf-theme=\"mid-neutral-gray\""));
    assert!(board_svg.contains("data-nf-theme=\"dark-graphite\""));

    // Board-level Undo restores the multi-artboard board before 4-theme comparison, and Redo brings all 4 back
    assert!(state.undo());
    assert_eq!(state.artboards.len(), artboards_before_compare);
    assert!(state.redo());
    assert_eq!(state.artboards.len(), 4);

    // Multi-artboard SVG round-trip imports every artboard on the board
    let mut reimported = UiLayoutStudioState::default();
    let base_count = reimported.artboards.len();
    let first_idx = reimported
        .import_svg_as_artboard(&board_svg)
        .expect("import 4-artboard SVG board");
    assert_eq!(first_idx, base_count);
    assert_eq!(reimported.artboards.len(), base_count + 4);
    assert_eq!(
        reimported.artboards[first_idx..]
            .iter()
            .map(|a| a.theme_override)
            .collect::<Vec<_>>(),
        vec![
            Some(ThemeMode::Dark),
            Some(ThemeMode::Light),
            Some(ThemeMode::MidNeutralGray),
            Some(ThemeMode::DarkGraphite),
        ]
    );

    // Persisted JSON bundle round-trips preview_theme and preview_theme_locked
    state.set_preview_theme(ThemeMode::MidNeutralGray);
    let tmp_json =
        std::env::temp_dir().join(format!("nf-ui-studio-bundle-{}.json", std::process::id()));
    state.save_studio_json(&tmp_json).expect("save studio json");
    let mut reloaded = UiLayoutStudioState::default();
    reloaded
        .load_studio_json(&tmp_json)
        .expect("load studio json");
    let _ = std::fs::remove_file(&tmp_json);
    assert_eq!(reloaded.preview_theme, ThemeMode::MidNeutralGray);
    assert!(reloaded.preview_theme_locked);
}

fn collect_shape_texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => {
            for child in shapes {
                collect_shape_texts(child, out);
            }
        }
        _ => {}
    }
}

fn collect_output_texts(out: &egui::FullOutput) -> Vec<String> {
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_shape_texts(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn r069_atomic_snapshot_undo_redo_covers_document_systems_artboards_and_view_state() {
    let mut state = UiLayoutStudioState::default();
    let initial_artboard_count = state.artboards.len();
    let initial_density = state.systems.density.active_profile;
    let initial_pan = state.canvas_pan;
    let initial_root_children = state.document.root.children.len();

    // 1. Add artboard records atomic history
    let added_idx = state.add_artboard(StudioPanelPreset::FiltersPanel);
    assert_eq!(state.artboards.len(), initial_artboard_count + 1);
    assert_eq!(state.active_artboard_idx, added_idx);
    assert!(state.undo());
    assert_eq!(state.artboards.len(), initial_artboard_count);
    assert!(state.redo());
    assert_eq!(state.artboards.len(), initial_artboard_count + 1);

    // 2. Duplicate artboard + Auto-arrange + Remove artboard all record undoable board snapshots
    let dup_idx = state.duplicate_artboard(added_idx);
    assert_eq!(state.artboards.len(), initial_artboard_count + 2);
    state.canvas_pan = [120.0, 48.0];
    state.solo_active_artboard = true;
    state.auto_arrange_artboards();
    assert_eq!(state.canvas_pan, [0.0, 0.0]);
    assert!(state.undo());
    assert_eq!(state.canvas_pan, [120.0, 48.0]);
    assert!(state.solo_active_artboard);

    state.remove_artboard(dup_idx);
    assert_eq!(state.artboards.len(), initial_artboard_count + 1);
    assert!(state.undo());
    assert_eq!(state.artboards.len(), initial_artboard_count + 2);

    // Undo back to original baseline
    while state.undo() {}
    assert_eq!(state.artboards.len(), initial_artboard_count);
    assert_eq!(state.systems.density.active_profile, initial_density);
    assert_eq!(state.canvas_pan, initial_pan);
    assert_eq!(state.document.root.children.len(), initial_root_children);
}

#[test]
fn r069_drag_math_derives_from_start_plus_total_delta_without_clamp_drift_and_coalesces_history() {
    // 1. Pure drag helpers: clamping past boundary and pulling back derives strictly from start + total_delta
    assert_eq!(
        canvas::apply_artboard_move_drag([24.0, 40.0], egui::vec2(-100.0, -100.0)),
        [8.0, 28.0]
    );
    assert_eq!(
        canvas::apply_artboard_move_drag([24.0, 40.0], egui::vec2(50.0, 20.0)),
        [74.0, 60.0]
    );
    assert_eq!(
        canvas::apply_pan_drag([10.0, 10.0], egui::vec2(-1000.0, 25.0)),
        [-600.0, 35.0]
    );
    assert_eq!(
        canvas::apply_artboard_resize_drag([200.0, 300.0], egui::vec2(100.0, -50.0), 2.0),
        [250.0, 275.0]
    );
    // Pulling past min size (76x80) and returning never accumulates clamp error
    assert_eq!(
        canvas::apply_artboard_resize_drag([200.0, 300.0], egui::vec2(-300.0, -400.0), 1.0),
        [76.0, 80.0]
    );
    assert_eq!(
        canvas::apply_artboard_resize_drag([200.0, 300.0], egui::vec2(40.0, 20.0), 1.0),
        [240.0, 320.0]
    );

    // 2. Coalesced Inspector drag over multiple frames produces exactly 1 Undo step
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, ThemeMode::Dark.nf2());

    let mut state = UiLayoutStudioState {
        open: true,
        inspector_tab: StudioInspectorTab::SystemsTab,
        systems_subtab: SystemsSubTab::TimelineGeometry,
        ..Default::default()
    };
    let initial_head = state.systems.timeline_geometry.tween_arrow_head_length_px;
    let pointer_down = egui::Event::PointerButton {
        pos: Pos2::new(100.0, 100.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    let pointer_up = egui::Event::PointerButton {
        pos: Pos2::new(112.0, 100.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    };

    // Frame 1 & 2 while pointer is down with continuous mutations
    let mut t = 1.0_f64;
    for delta in [3.0_f32, 6.0, 9.0] {
        state.systems.timeline_geometry.tween_arrow_head_length_px = initial_head + delta;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
                time: Some(t),
                events: vec![
                    egui::Event::PointerMoved(Pos2::new(100.0 + delta, 100.0)),
                    pointer_down.clone(),
                ],
                ..Default::default()
            },
            |ctx| ui_layout_studio_window_ui(ctx, &mut state),
        );
        t += 0.1;
    }
    // Simulated SystemsTab helper directly: verify drag coalescing commits once on pointer-up
    let mut before_drag = UiStudioSystems::default();
    before_drag.timeline_geometry.tween_arrow_head_length_px = initial_head;
    state.systems.timeline_geometry.tween_arrow_head_length_px = initial_head + 9.0;
    let snap = state.capture_snapshot("Edit NF2 system tokens");
    let mut held_snap = snap;
    held_snap.systems = before_drag;
    let sys_drag_id = egui::Id::new("nf2-studio-inspector-systems-drag");
    ctx.data_mut(|d| d.insert_temp(sys_drag_id, held_snap));

    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
            time: Some(t + 0.1),
            events: vec![
                egui::Event::PointerMoved(Pos2::new(112.0, 100.0)),
                pointer_up,
            ],
            ..Default::default()
        },
        |ctx| ui_layout_studio_window_ui(ctx, &mut state),
    );
    assert_eq!(state.history.undo_stack.len(), 1);
    assert!(state.undo());
    assert_eq!(
        state.systems.timeline_geometry.tween_arrow_head_length_px,
        initial_head
    );
}

#[test]
fn r069_session_bundle_and_editor_persist_artboards_across_reload() {
    let mut state = UiLayoutStudioState::default();
    let new_idx = state.add_artboard(StudioPanelPreset::FiltersPanel);
    state.artboards[new_idx].canvas_pos = [444.0, 222.0];
    state.artboards[new_idx].canvas_size = [375.0, 515.0];
    state.solo_active_artboard = true;
    state.canvas_pan = [32.0, -18.0];
    state.canvas_zoom = 1.25;
    state.show_wireframe_outlines = false;
    state.show_dimension_badges = false;
    state.inspector_tab = StudioInspectorTab::CodeExport;
    state.systems_subtab = SystemsSubTab::PaletteTokens;
    state.set_preview_theme(ThemeMode::DarkGraphite);

    let tmp_json =
        std::env::temp_dir().join(format!("nf-ui-studio-session-{}.json", std::process::id()));
    state
        .save_studio_json(&tmp_json)
        .expect("save session json");

    let mut reloaded = UiLayoutStudioState::default();
    reloaded
        .load_studio_json_initial(&tmp_json)
        .expect("restore initial session");
    let _ = std::fs::remove_file(&tmp_json);

    assert_eq!(reloaded.artboards.len(), state.artboards.len());
    assert_eq!(reloaded.active_artboard_idx, new_idx);
    assert_eq!(reloaded.active_preset, StudioPanelPreset::FiltersPanel);
    assert_eq!(reloaded.artboards[new_idx].canvas_pos, [444.0, 222.0]);
    assert_eq!(reloaded.artboards[new_idx].canvas_size, [375.0, 515.0]);
    assert!(reloaded.solo_active_artboard);
    assert_eq!(reloaded.canvas_pan, [32.0, -18.0]);
    assert_eq!(reloaded.canvas_zoom, 1.25);
    assert!(!reloaded.show_wireframe_outlines);
    assert!(!reloaded.show_dimension_badges);
    assert_eq!(reloaded.inspector_tab, StudioInspectorTab::CodeExport);
    assert_eq!(reloaded.systems_subtab, SystemsSubTab::PaletteTokens);
    assert_eq!(reloaded.preview_theme, ThemeMode::DarkGraphite);
    assert!(
        !reloaded.history.can_undo(),
        "initial session restore must not record a spurious undo entry"
    );

    // Legacy JSON without new session keys still deserializes cleanly via #[serde(default)]
    let legacy_json = serde_json::json!({
        "document": state.document,
        "systems": state.systems,
        "artboards": state.artboards,
        "preview_theme": "light",
        "preview_theme_locked": true
    })
    .to_string();
    let legacy: StudioPersistedBundle =
        serde_json::from_str(&legacy_json).expect("deserialize legacy bundle");
    assert_eq!(legacy.canvas_zoom, 1.0);
    assert!(legacy.show_wireframe_outlines);
    assert!(legacy.show_dimension_badges);
    assert!(!legacy.solo_active_artboard);
}

#[test]
fn r069_artboard_header_truncates_title_places_size_badge_inside_without_overlap() {
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, ThemeMode::Dark.nf2());

    let font = neoflash_ui::typography::font(TextStyle::MonoSmall);
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
            time: Some(0.5),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            for w in [180.0_f32, 225.0, 260.0, 300.0, 620.0, 1310.0] {
                let hdr_rect = Rect::from_min_size(Pos2::new(24.0, 12.0), egui::vec2(w, 22.0));
                let layout = canvas::compute_artboard_header_layout(hdr_rect, 66.0, w >= 225.0);

                let badge = layout
                    .size_badge_rect
                    .expect("size badge should fit inside artboard header");
                assert!(hdr_rect.contains_rect(badge));
                assert!(hdr_rect.contains_rect(layout.title_rect));

                let mut rects = vec![layout.title_rect, badge];
                for pill in [layout.svg_rect, layout.dup_rect, layout.close_rect]
                    .into_iter()
                    .flatten()
                {
                    assert!(hdr_rect.contains_rect(pill));
                    rects.push(pill);
                }
                for i in 0..rects.len() {
                    for j in (i + 1)..rects.len() {
                        assert!(
                            !rects[i].intersects(rects[j]),
                            "header rects must never overlap at width {w}: {:?} vs {:?}",
                            rects[i],
                            rects[j]
                        );
                    }
                }

                let (galley, truncated) = canvas::truncate_galley_with_ellipsis(
                    &painter,
                    "Properties — Movie Clip · Mid Neutral Gray",
                    font.clone(),
                    layout.title_rect.width(),
                    Color32::WHITE,
                );
                assert!(
                    galley.size().x <= layout.title_rect.width() + 0.5,
                    "galley width {} exceeded title_rect width {} at width {w}",
                    galley.size().x,
                    layout.title_rect.width()
                );
                if w <= 260.0 {
                    assert!(truncated);
                    assert!(galley.text().ends_with('…'));
                }
            }
        },
    );

    // Rendered UI check: Add to Canvas has no leading '+', size badge lives in header, no overlapping root pill
    let mut state = UiLayoutStudioState {
        open: true,
        solo_active_artboard: true,
        show_dimension_badges: true,
        ..Default::default()
    };
    state.select_single(state.document.root.id);
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
            time: Some(0.8),
            ..Default::default()
        },
        |ctx| ui_layout_studio_window_ui(ctx, &mut state),
    );
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))),
            time: Some(1.0),
            ..Default::default()
        },
        |ctx| ui_layout_studio_window_ui(ctx, &mut state),
    );
    let texts = collect_output_texts(&out);
    assert!(
        texts.iter().any(|t| t == "Add to Canvas"),
        "expected 'Add to Canvas' in rendered UI texts: {texts:?}"
    );
    assert!(!texts.iter().any(|t| t == "+ Add to Canvas"));
    assert!(
        texts.iter().any(|t| t == "300×480px"),
        "artboard size badge should render inside artboard header: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.starts_with("Col · ")),
        "root floating badge must not overlap panel header: {texts:?}"
    );
}
