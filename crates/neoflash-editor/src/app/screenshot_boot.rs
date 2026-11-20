use super::*;

impl Editor {
    pub(super) fn apply_screenshot_boot(&mut self, egui_ctx: &egui::Context, is_screenshot: bool) {
        let s = self;
        // Screenshot aid: `NEOFLASH_SCREENSHOT_ROWS=short|compact` captures the Frame View row heights.
        if is_screenshot {
            // `NEOFLASH_SCREENSHOT_DENSITY=compact|classic` captures a density profile.
            match std::env::var("NEOFLASH_SCREENSHOT_DENSITY").as_deref() {
                Ok("compact") => s
                    .workspace
                    .set_density(crate::workspace::DensityPref::Compact),
                Ok("classic") => s
                    .workspace
                    .set_density(crate::workspace::DensityPref::ClassicFlash),
                _ => {}
            }
            // `NEOFLASH_SCREENSHOT_STAGEBAR=collapsed|editbar` captures a Stage toolbar state.
            match std::env::var("NEOFLASH_SCREENSHOT_STAGEBAR").as_deref() {
                Ok("collapsed") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::Collapsed
                }
                // The collapsed pill expanded in place, as while the pointer is over it.
                Ok("hover") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::Collapsed;
                    egui_ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("stage-toolbar-linger"), f64::MAX)
                    });
                }
                Ok("editbar") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::EditBar
                }
                _ => {}
            }
            // `NEOFLASH_SCREENSHOT_PRIORITY=library` captures Library priority (R-054).
            if std::env::var("NEOFLASH_SCREENSHOT_PRIORITY").as_deref() == Ok("library") {
                s.workspace.dock.give_priority(PanelId::Library);
            }
            // `NEOFLASH_SCREENSHOT_COLOR=expanded` shows the full Color picker (R-055);
            // `NEOFLASH_SCREENSHOT_RECENT=1` fills the recent colours from the document palette.
            if std::env::var("NEOFLASH_SCREENSHOT_COLOR").as_deref() == Ok("expanded") {
                s.workspace.color_picker_expanded = true;
            }
            if std::env::var("NEOFLASH_SCREENSHOT_RECENT").as_deref() == Ok("1") {
                s.workspace.recent_colors = vec![
                    [0x0C, 0x10, 0x24, 255],
                    [0xF0, 0xBE, 0x91, 255],
                    [0xE5, 0x39, 0x35, 255],
                    [0x2E, 0xA8, 0x4F, 255],
                    [0x1E, 0x63, 0xD6, 255],
                    [0xFF, 0xD4, 0x00, 255],
                    [0xB0, 0x17, 0x7A, 255],
                    [0xFF, 0xFF, 0xFF, 255],
                ];
            }
            match std::env::var("NEOFLASH_SCREENSHOT_ROWS").as_deref() {
                Ok("short") => s.workspace.timeline.short_rows = true,
                Ok("compact") => s.workspace.timeline.compact_rows = true,
                _ => {}
            }
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_FRAME=<layer row>,<frame>` selects that frame
        // (zero-based), so captures show frame Properties (e.g. Properties ▸ Sound).
        if let Some((layer, frame)) =
            std::env::var("NEOFLASH_SCREENSHOT_FRAME")
                .ok()
                .and_then(|value| {
                    let (layer, frame) = value.split_once(',')?;
                    Some((
                        layer.trim().parse::<usize>().ok()?,
                        frame.trim().parse::<u32>().ok()?,
                    ))
                })
            && let Some(id) = s.symbol().timeline.layers.get(layer).map(|l| l.id.clone())
        {
            s.layer = layer;
            s.frame = frame;
            s.normalize_selection();
            s.select_frame_range(FrameRange {
                layers: vec![id],
                start: s.frame,
                end: s.frame,
            });
        }
        match std::env::var("NEOFLASH_SCREENSHOT_ONION").as_deref() {
            Ok("outlines") => {
                s.onion = true;
                s.onion_outlines = true;
            }
            Ok("solid" | "1" | "true") => {
                s.onion = true;
                s.onion_outlines = false;
            }
            _ => {}
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_SELECT=select|transform|subselect|brush|pencil`
        // selects every editable object on the current frame with that tool, so captures show the
        // NF2 selection boxes or the transform box.
        if let Ok(tool) = std::env::var("NEOFLASH_SCREENSHOT_SELECT") {
            let tool = match tool.as_str() {
                "transform" => Some(Tool::Transform),
                "subselect" => Some(Tool::Subselect),
                "select" => Some(Tool::Select),
                "brush" => Some(Tool::Brush),
                "pencil" => Some(Tool::Pencil),
                "razor" => Some(Tool::Razor),
                _ => None,
            };
            if let Some(tool) = tool {
                s.set_tool(tool);
                if !matches!(tool, Tool::Brush | Tool::Pencil | Tool::Razor) {
                    s.select_all();
                }
            }
        }
        // `NEOFLASH_SCREENSHOT_SELECT=motion` selects one motion object; pair it with
        // `NEOFLASH_SCREENSHOT_ELEMENT=<stable id>` to prove a specific same-layer track.
        if std::env::var("NEOFLASH_SCREENSHOT_SELECT").as_deref() == Ok("motion") {
            s.set_tool(Tool::Select);
            if let Some(layer) = s.symbol().timeline.layers.get(s.layer) {
                let layer_id = layer.id.clone();
                let sampled = sample_layer_in_at_fps(
                    &s.symbol().timeline,
                    layer,
                    s.frame,
                    f64::from(s.project.fps),
                );
                let requested = std::env::var("NEOFLASH_SCREENSHOT_ELEMENT").ok();
                let element = requested
                    .as_deref()
                    .and_then(|id| sampled.iter().find(|element| element.id == id))
                    .or_else(|| requested.is_none().then(|| sampled.first()).flatten());
                if let Some(element) = element {
                    s.set_selection(vec![crate::selection::SelectionTarget {
                        layer: layer_id,
                        element: element.id.clone(),
                    }]);
                }
            }
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_PANELS=swatches,info,history,project` reveals real panels.
        if let Ok(list) = std::env::var("NEOFLASH_SCREENSHOT_PANELS") {
            for raw in list.split(',') {
                let panel = match raw.trim().to_ascii_lowercase().as_str() {
                    "history" => Some(PanelId::History),
                    "brushes" => Some(PanelId::Brushes),
                    "swatches" => Some(PanelId::Swatches),
                    "color" => Some(PanelId::Color),
                    "transform" => Some(PanelId::Transform),
                    "align" => Some(PanelId::Align),
                    "info" => Some(PanelId::Info),
                    "project" => Some(PanelId::Project),
                    "ease" => Some(PanelId::Ease),
                    "scene" => Some(PanelId::Scene),
                    "motion_editor" => Some(PanelId::MotionEditor),
                    "filters" => Some(PanelId::Filters),
                    "debugger" => Some(PanelId::Debugger),
                    "compiler_errors" => Some(PanelId::CompilerErrors),
                    "output" => Some(PanelId::Output),
                    "code_snippets" => Some(PanelId::CodeSnippets),
                    "movie_explorer" => Some(PanelId::MovieExplorer),
                    "library" => Some(PanelId::Library),
                    "accessibility" => Some(PanelId::Accessibility),
                    _ => None,
                };
                if let Some(panel) = panel {
                    s.show_panel(panel);
                }
            }
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_TOOLS=rail` docks the one-column Tools rail; `menu`
        // and `rail-menu` open the palette's "⋯" menu; `float` floats the palette inside the main
        // window (embedded viewport) so the capture shows it.
        let tools_capture = std::env::var("NEOFLASH_SCREENSHOT_TOOLS");
        if matches!(tools_capture.as_deref(), Ok("menu" | "rail-menu")) {
            s.open_tools_menu(egui_ctx);
        }
        match tools_capture.as_deref() {
            Ok("rail" | "rail-menu") => {
                s.workspace.dock.left.size = neoflash_ui::metrics::tools_rail() + 6.
            }
            Ok("float") => {
                egui_ctx.set_embed_viewports(true);
                s.set_panel_placement(
                    egui_ctx,
                    PanelId::Tools,
                    crate::workspace::PanelPlacement::Detached,
                );
            }
            _ => {}
        }
        match std::env::var("NEOFLASH_SCREENSHOT_DIALOG").as_deref() {
            Ok("document_settings") => s.open_document_settings(),
            Ok("trace_bitmap") => {
                s.select_all();
                // Trace needs exactly one bitmap: keep the first one of a larger document.
                if let Some((target, _)) = s.sampled_targets().into_iter().find(|(_, element)| {
                    matches!(element.kind, neoflash_core::ElementKind::Bitmap { .. })
                }) {
                    s.set_selection(vec![target]);
                }
                s.open_trace_bitmap(egui_ctx);
            }
            Ok("customize_tools") => s.open_customize_tools(egui_ctx),
            Ok("workspace_save") => {
                s.workspace_dialog = Some(crate::workspace_dialogs::WorkspaceDialog::Save {
                    name: "My workspace".into(),
                    focus: false,
                })
            }
            Ok("publish_settings") => {
                s.open_publish_settings();
                // `NEOFLASH_SCREENSHOT_PUBLISH_TARGET=html5|desktop|video|images|svg` picks the page.
                if let (Some(draft), Ok(target)) = (
                    s.publish_settings.as_mut(),
                    std::env::var("NEOFLASH_SCREENSHOT_PUBLISH_TARGET"),
                ) {
                    draft.select_target_slug(&target);
                }
            }
            Ok("export_video") => s.open_export_video(None),
            Ok("export_audio") => s.open_export_audio(egui_ctx, None),
            Ok("export_image") => {
                s.open_export_image(egui_ctx);
                // `NEOFLASH_SCREENSHOT_IMAGE_FORMAT=png|svg|psd|ai|pdf` picks the format.
                if let Ok(format) = std::env::var("NEOFLASH_SCREENSHOT_IMAGE_FORMAT") {
                    crate::panels::export_image::select_export_image_format_slug(egui_ctx, &format);
                }
            }
            Ok("export_lottie") => s.open_export_lottie(None),
            Ok("figma_import") => s.open_figma_import_screenshot_sample(egui_ctx),
            Ok("import_video") => s.open_import_video(egui_ctx),
            Ok(dialog) if dialog.starts_with("ai_") && s.open_sample_ai_dialog(dialog) => {}
            Ok("batch_rename") => s.open_batch_rename_library_modal(egui_ctx),
            Ok("batch_linkage") => s.open_batch_linkage_modal(egui_ctx),
            Ok("batch_bitmaps") => s.open_batch_bitmap_compression_modal(egui_ctx),
            Ok("instance_namer") => s.open_auto_instance_namer_modal(egui_ctx),
            Ok("randomize_selection") => s.open_randomize_selection_modal(egui_ctx),
            Ok("sound_properties") => s.open_sound_properties(egui_ctx, None),
            Ok("edit_envelope") => s.open_edit_envelope(),
            Ok("find_replace") => s.open_find_replace(),
            Ok("layer_properties") => s.open_layer_properties(egui_ctx, 0),
            Ok("stroke_style") => s.open_stroke_style(egui_ctx),
            Ok("font_embedding") => s.open_font_embedding(egui_ctx),
            Ok("font_embedding_face") => s.open_font_embedding_with_face(egui_ctx),
            Ok("convert_symbol") => {
                s.select_all();
                s.begin_symbol_conversion();
            }
            Ok("new_symbol") => s.begin_new_symbol(),
            Ok("bitmap_properties") => s.open_bitmap_properties(egui_ctx, None),
            Ok("magic_wand") => s.open_magic_wand_settings(),
            Ok("polystar") => s.open_polystar_settings(),
            Ok("scale_rotate") => {
                s.select_all();
                s.open_scale_and_rotate_dialog();
            }
            Ok("about") => s.show_about = true,
            Ok("revert") => s.confirm_revert = true,
            Ok("workspace_delete") => {
                s.workspace_dialog = Some(crate::workspace_dialogs::WorkspaceDialog::Delete {
                    name: "My workspace".into(),
                })
            }
            Ok("beta_feedback") => s.open_beta_feedback(),
            Ok("help") => s.open_help_manual(egui_ctx),
            Ok(dialog @ ("scripting_reference" | "scripting_guides")) => {
                let guides = dialog == "scripting_guides";
                crate::panels::help_manual::open_reference_for_screenshot(egui_ctx, guides);
            }
            Ok("ui_studio") => crate::panels::ui_studio::set_studio_open(egui_ctx, true),
            Ok("paste_motion_special") => {
                s.copy_motion();
                s.open_paste_motion_special();
            }
            _ => {}
        }
        match std::env::var("NEOFLASH_SCREENSHOT_DOCUMENTS").as_deref() {
            Ok("empty") => s.request_close_documents(egui_ctx, false),
            Ok("tabs") => {
                s.project.name = "Website".into();
                s.new_document_session(egui_ctx);
                s.project.name = "Animation".into();
                s.new_document_session(egui_ctx);
                s.project.name = "Game".into();
                s.switch_session_tab(egui_ctx, 1);
            }
            _ => {}
        }
        if std::env::var_os("NEOFLASH_SCREENSHOT_TOAST").is_some() {
            s.notify(neoflash_ui::components::ToastType::Warning,
                "Partial stroke erasing on masked artwork is not available yet; no artwork was changed.");
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_PREFERENCES=<page index>` opens Preferences on that page.
        if let Some(page) = std::env::var("NEOFLASH_SCREENSHOT_PREFERENCES")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            s.open_preferences(page);
        }
    }
}
