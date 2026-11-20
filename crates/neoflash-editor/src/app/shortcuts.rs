use super::*;

impl Editor {
    /// Whether any `egui::Popup` or editor popover (View settings, Tools flyout/popover, Color,
    /// layer outline colour, Symbols & scenes, Help search) is open in `ctx`.
    pub(crate) fn has_open_popup(&self, ctx: &egui::Context) -> bool {
        self.stage_view.settings_open
            || crate::panels::dialog_helpers::is_any_editor_popup_open(self, ctx)
    }

    /// True while widget keyboard focus (text input or Tab-focused control), a modal dialog, or
    /// an open popup/menu owns unmodified and single-key keyboard input.
    pub(crate) fn blocks_unmodified_shortcuts(&self, ctx: &egui::Context) -> bool {
        ctx.wants_keyboard_input() || self.has_modal_dialog_in(ctx) || self.has_open_popup(ctx)
    }

    /// Latches `Space`-pan suppression whenever `Space` is down while focus, a modal, or a popup
    /// owns input, preventing a Tab-focused button from bleeding into Stage pan if its click
    /// surrenders focus or closes the control mid-frame.
    pub(crate) fn update_stage_space_pan_suppression(&self, ctx: &egui::Context) {
        let suppression_id = egui::Id::new("nf2-stage-space-pan-suppressed");
        let space_down = ctx.input(|i| i.key_down(egui::Key::Space));
        if !space_down {
            ctx.data_mut(|d| d.remove::<bool>(suppression_id));
            return;
        }
        if self.blocks_unmodified_shortcuts(ctx) {
            ctx.data_mut(|d| d.insert_temp(suppression_id, true));
        }
    }

    /// True when Stage temporary `Space` pan must NOT activate.
    pub(crate) fn blocks_stage_space_pan(&self, ctx: &egui::Context) -> bool {
        self.update_stage_space_pan_suppression(ctx);
        let suppression_id = egui::Id::new("nf2-stage-space-pan-suppressed");
        self.blocks_unmodified_shortcuts(ctx)
            || ctx
                .data(|d| d.get_temp::<bool>(suppression_id))
                .unwrap_or(false)
    }

    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        use crate::clipboard::ClipboardCommand;
        use crate::commands::{CommandId as C, modifiers_match};
        self.update_stage_space_pan_suppression(ctx);
        if self.has_modal_dialog_in(ctx)
            || crate::panels::symbols_popover::is_symbols_popover_open(ctx)
            || crate::panels::function_search::is_function_search_open(ctx)
        {
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        if self.document_shortcuts(ctx) {
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        if !self.has_document() {
            for &cmd in C::ALL {
                if self.command_menu_state(Some(ctx), cmd).enabled
                    && self.take_command_shortcut(ctx, cmd)
                {
                    let _ = self.execute_menu_command(ctx, cmd);
                    if self.has_document() {
                        break;
                    }
                }
            }
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        if self.is_standalone_script_session() {
            let clipboard_generation = crate::clipboard::paste_generation(ctx);
            self.clipboard_events_with_generation(ctx, || clipboard_generation);
            let typing = self.blocks_unmodified_shortcuts(ctx);
            for &cmd in C::ALL {
                if typing
                    && matches!(
                        cmd,
                        C::EditUndo
                            | C::EditRedo
                            | C::EditCut
                            | C::EditCopy
                            | C::EditPaste
                            | C::EditSelectAll
                    )
                {
                    continue;
                }
                if self.command_menu_state(Some(ctx), cmd).enabled
                    && self.take_command_shortcut(ctx, cmd)
                {
                    let _ = self.execute_menu_command(ctx, cmd);
                    if !self.is_standalone_script_session() {
                        break;
                    }
                }
            }
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        let clipboard_generation = crate::clipboard::paste_generation(ctx);
        self.handle_stage_text_edit_keys(ctx);
        self.menu_shortcuts(ctx);
        self.timeline_shortcuts(ctx);
        self.view_shortcuts(ctx);
        self.clipboard_events_with_generation(ctx, || clipboard_generation);

        // Global commands allowed even while a text field has focus (when bound to a non-typing chord).
        if self.take_command_shortcut(ctx, C::FileSaveAs) {
            self.save(true);
        }
        if self.take_command_shortcut(ctx, C::FileSave) {
            self.save(false);
        }
        if self.take_command_shortcut(ctx, C::FileOpen) {
            self.request(Pending::Open, ctx);
        }
        if self.take_command_shortcut(ctx, C::FileNew) {
            self.request(Pending::New, ctx);
        }
        if self.take_command_shortcut(ctx, C::ControlTestScene) {
            self.test_scene();
        }
        if self.take_command_shortcut(ctx, C::ControlTestMovie) {
            self.export(true);
        }
        if self.take_command_shortcut(ctx, C::WindowActions) {
            self.open_actions_for_current_context();
        }

        let typing = self.blocks_unmodified_shortcuts(ctx);
        if !typing {
            if self.take_command_shortcut(ctx, C::EditRedo) {
                if self.history.can_redo() {
                    self.redo();
                } else if !self.selection.is_empty()
                    && !crate::commands::has_override(
                        &self.workspace.shortcuts,
                        C::ModifyRemoveTransform,
                    )
                {
                    self.reset_selection_transform(ctx);
                }
            }
            if self.take_command_shortcut(ctx, C::EditUndo) {
                self.undo();
            }
            if self.take_command_shortcut(ctx, C::EditCut) {
                self.request_clipboard(ctx, ClipboardCommand::Cut);
            }
            if self.take_command_shortcut(ctx, C::EditCopy) {
                self.request_clipboard(ctx, ClipboardCommand::Copy);
            }
            if self.take_command_shortcut(ctx, C::EditPasteInPlace) {
                self.request_clipboard(ctx, ClipboardCommand::PasteInPlace);
            }
            if self.take_command_shortcut(ctx, C::EditPaste) {
                self.request_clipboard(ctx, ClipboardCommand::Paste);
            }
            if self.take_command_shortcut(ctx, C::EditSelectAll) {
                self.select_all();
            }
            if self.take_command_shortcut(ctx, C::EditDuplicate) {
                self.duplicate();
            }
            if self.take_command_shortcut(ctx, C::InsertKeyframe) {
                self.make_key(false);
            }
            if self.take_command_shortcut(ctx, C::InsertBlankKeyframe) {
                self.make_key(true);
            }
            if self.take_command_shortcut(ctx, C::InsertFrame) {
                self.insert_frame();
            }
            if self.take_command_shortcut(ctx, C::InsertNewSymbol) {
                self.begin_new_symbol();
            }
            if self.take_command_shortcut(ctx, C::ModifyConvertToSymbol) {
                self.convert_to_symbol();
            }
            if self.take_command_shortcut(ctx, C::EditClear) {
                self.delete_selected();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape))
                && !crate::panels::dialog_helpers::escape_dismissed_focus_or_popup(ctx)
            {
                let finish_open_pen = self
                    .vector
                    .draft
                    .as_ref()
                    .is_some_and(|draft| !draft.pencil && draft.anchors.len() >= 2);
                let captured = self.object_gesture.is_some()
                    || self.scale9_guide_drag.is_some()
                    || self.timeline_motion.drag.is_some()
                    || self.timeline_motion.curve_drag.is_some()
                    || self.timeline_shape_hints.drag.is_some()
                    || self.authoring_tools.is_drawing()
                    || self.shape_draft.is_some()
                    || self.vector.point_drag.is_some()
                    || self.vector.draft.is_some();
                if finish_open_pen {
                    // Escape ends the current open contour and leaves Pen active, so the next
                    // click begins a second path instead of silently losing the first one.
                    self.finish_path(false);
                } else {
                    self.cancel_edits();
                }
                if !captured {
                    self.clear_selection();
                }
            }
            let finish_draft = self.vector.draft.is_some()
                && ctx.input_mut(|i| {
                    let before = i.events.len();
                    let mut found = false;
                    i.events.retain(|event| {
                        let hit = !found
                            && matches!(
                                event,
                                egui::Event::Key {
                                    key: Key::Enter,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } if modifiers_match(*modifiers, Modifiers::NONE)
                            );
                        found |= hit;
                        !hit
                    });
                    i.events.len() != before
                });
            if finish_draft {
                self.finish_path(false);
            } else if self.take_command_shortcut(ctx, C::ControlPlay) {
                self.set_playing(!self.playing);
            }
            for tool in [
                Tool::Select,
                Tool::Transform,
                Tool::Rectangle,
                Tool::Ellipse,
                Tool::PolyStar,
                Tool::Text,
                Tool::Pen,
                Tool::Pencil,
                Tool::Subselect,
                Tool::Line,
                Tool::Brush,
                Tool::Bucket,
                Tool::Ink,
                Tool::Eyedropper,
                Tool::Eraser,
                Tool::Lasso,
                Tool::Hand,
                Tool::Zoom,
                Tool::Gradient,
                Tool::Razor,
                Tool::Deco,
                Tool::SprayBrush,
                Tool::Bone,
                Tool::Bind,
                Tool::AssetWarp,
                Tool::Translate3D,
                Tool::Rotate3D,
            ] {
                if self.take_command_shortcut(ctx, C::for_tool(tool)) {
                    if !matches!(
                        tool,
                        Tool::Select
                            | Tool::Subselect
                            | Tool::Transform
                            | Tool::Gradient
                            | Tool::Bone
                            | Tool::Bind
                            | Tool::AssetWarp
                            | Tool::Translate3D
                            | Tool::Rotate3D
                            | Tool::Razor
                    ) {
                        self.clear_selection();
                    }
                    self.set_tool(tool);
                }
            }
            if self.take_command_shortcut(ctx, C::ToolObjectDrawing) {
                self.workspace.drawing.object_drawing = !self.workspace.drawing.object_drawing;
            }
            if !self.timeline_edit.focused
                && !self.selection.is_empty()
                && self.object_gesture.is_none()
                && !self.authoring_tools.is_drawing()
                && self.shape_draft.is_none()
                && self.vector.draft.is_none()
            {
                let mut nudge = [0.0, 0.0];
                ctx.input_mut(|i| {
                    if !i.modifiers.command && !i.modifiers.alt && !i.modifiers.ctrl {
                        let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                        let m = if i.modifiers.shift {
                            Modifiers::SHIFT
                        } else {
                            Modifiers::NONE
                        };
                        if i.consume_key(m, Key::ArrowLeft) {
                            nudge[0] -= step;
                        }
                        if i.consume_key(m, Key::ArrowRight) {
                            nudge[0] += step;
                        }
                        if i.consume_key(m, Key::ArrowUp) {
                            nudge[1] -= step;
                        }
                        if i.consume_key(m, Key::ArrowDown) {
                            nudge[1] += step;
                        }
                    }
                });
                if nudge != [0.0, 0.0] && !self.nudge_selected_vector_points(nudge[0], nudge[1]) {
                    self.nudge_selection(nudge[0], nudge[1]);
                }
            }
            let last = self.symbol().timeline.frame_count.saturating_sub(1);
            if self.take_command_shortcut(ctx, C::ControlStepBack) {
                self.seek_frame(self.frame.saturating_sub(1));
            }
            if self.take_command_shortcut(ctx, C::ControlStepForward) {
                self.seek_frame((self.frame + 1).min(last));
            }
        }
        crate::commands::clear_queued_commands(ctx);
    }

    /// Design §8 menu shortcuts routed through [`crate::commands`]. Modifiers match exactly, so ⇧F5
    /// never runs F5 and ⌥⇧R never selects the Rectangle tool. Shortcuts of unavailable menu items are
    /// consumed at the end if unbound to an active command.
    fn menu_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::authoring_modify::Arrange;
        use crate::commands::{CommandId as C, has_override, modifiers_match};
        use crate::timeline_edit::FrameCommand as F;
        // Text creation and text-object editing deliberately focus the Properties text field.
        // Classic Flash object shortcuts remain object commands while a Stage selection exists,
        // so handle them before the general typing guard. Their Command-modified chords are safe
        // from ordinary character input and each command still validates the current selection.
        let typing = self.blocks_unmodified_shortcuts(ctx);
        let break_apart_safe = !typing
            || crate::commands::effective_shortcuts(&self.workspace.shortcuts, C::ModifyBreakApart)
                .iter()
                .all(|sc| sc.modifiers.command || sc.modifiers.ctrl || sc.modifiers.alt);
        let ungroup_safe = !typing
            || crate::commands::effective_shortcuts(&self.workspace.shortcuts, C::ModifyUngroup)
                .iter()
                .all(|sc| sc.modifiers.command || sc.modifiers.ctrl || sc.modifiers.alt);
        let group_safe = !typing
            || crate::commands::effective_shortcuts(&self.workspace.shortcuts, C::ModifyGroup)
                .iter()
                .all(|sc| sc.modifiers.command || sc.modifiers.ctrl || sc.modifiers.alt);
        if !self.selection.is_empty()
            && break_apart_safe
            && self.take_command_shortcut(ctx, C::ModifyBreakApart)
        {
            self.break_apart_selection();
        }
        if self.can_ungroup_selection()
            && ungroup_safe
            && self.take_command_shortcut(ctx, C::ModifyUngroup)
        {
            self.ungroup_selection();
        }
        if !self.selection.is_empty()
            && group_safe
            && self.take_command_shortcut(ctx, C::ModifyGroup)
        {
            self.group_selection();
        }
        if self.blocks_unmodified_shortcuts(ctx) {
            return;
        }
        // ⌥⌘X / ⌥⌘C / ⌥⌘V arrive as Cut / Copy / Paste events on macOS when un-overridden.
        let default_frame_clip = !has_override(&self.workspace.shortcuts, C::EditTimelineCutFrames)
            && !has_override(&self.workspace.shortcuts, C::EditTimelineCopyFrames)
            && !has_override(&self.workspace.shortcuts, C::EditTimelinePasteFrames);
        if default_frame_clip {
            let frame_clipboard = ctx.input_mut(|i| {
                if !(i.modifiers.alt && i.modifiers.command) {
                    return None;
                }
                let mut command = None;
                i.events.retain(|event| {
                    let found = match event {
                        egui::Event::Cut => Some(F::Cut),
                        egui::Event::Copy => Some(F::Copy),
                        egui::Event::Paste(_) => Some(F::Paste),
                        _ => None,
                    };
                    command = command.or(found);
                    found.is_none()
                });
                command
            });
            if let Some(command) = frame_clipboard {
                self.run_frame_command(ctx, command);
            }
        }
        for command in [F::Cut, F::Copy, F::Paste, F::Clear, F::Remove, F::SelectAll] {
            if self.take_command_shortcut(ctx, C::for_frame_command(command)) {
                self.run_frame_command(ctx, command);
            }
        }
        if self.take_command_shortcut(ctx, C::EditTimelineReverseFrames)
            && self.can_reverse_frames()
        {
            self.reverse_frames();
        }
        if self.take_command_shortcut(ctx, C::EditTimelineSyncSymbols) && self.can_sync_symbols() {
            self.sync_symbols();
        }
        if self.take_command_shortcut(ctx, C::EditTimelineCopyMotion) && self.can_copy_motion() {
            self.copy_motion();
        }
        if self.take_command_shortcut(ctx, C::EditTimelinePasteMotion) && self.can_paste_motion() {
            self.paste_motion();
        }

        // File.
        if self.take_command_shortcut(ctx, C::FileClose) {
            if ctx.viewport_id() != egui::ViewportId::ROOT
                && self.close_floating_viewport(ctx.viewport_id())
            {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                self.request(Pending::CloseDocument, ctx);
            }
        }
        if self.take_command_shortcut(ctx, C::FileRevert) && self.can_revert() {
            self.request_revert();
        }
        if self.take_command_shortcut(ctx, C::FileImportGraphics) {
            self.import_graphics_dialog();
        }
        if self.take_command_shortcut(ctx, C::FileImportLegacyFlash) {
            self.request(Pending::Import, ctx);
        }
        if self.take_command_shortcut(ctx, C::FileExportVideo) {
            self.open_export_video(None);
        }
        if self.take_command_shortcut(ctx, C::FileExportAudio) {
            self.open_export_audio(ctx, None);
        }
        if self.take_command_shortcut(ctx, C::FileExportLottie) {
            self.export_lottie_dialog(None);
        }
        if self.take_command_shortcut(ctx, C::FileExportAfterEffects) {
            self.export_aep_dialog(None, None);
        }
        if self.take_command_shortcut(ctx, C::FileExportTextureAtlas) {
            self.export_texture_atlas_dialog(None);
        }
        if self.take_command_shortcut(ctx, C::FileExportSwf) {
            self.export_swf_dialog(None);
        }
        if self.take_command_shortcut(ctx, C::FilePublishSettings) {
            self.open_publish_settings();
        }
        if self.take_command_shortcut(ctx, C::FilePublish) {
            self.export(false);
        }

        // Edit.
        if self.take_command_shortcut(ctx, C::EditDeselect) {
            self.finish_edits();
            self.clear_selection();
        }
        if self.take_command_shortcut(ctx, C::EditFindAndReplace) {
            self.open_find_replace();
        }
        if self.take_command_shortcut(ctx, C::EditSymbols) {
            self.toggle_edit_symbols();
        }
        if self.take_command_shortcut(ctx, C::EditSendToAi) && self.has_ready_ai_connection() {
            let _ = self.open_send_to_ai();
        }
        if self.take_command_shortcut(ctx, C::EditPreferences) {
            self.open_preferences(crate::panels::preferences::PAGE_GENERAL);
        }
        if self.take_command_shortcut(ctx, C::EditKeyboardShortcuts) {
            self.open_preferences(crate::panels::preferences::PAGE_SHORTCUTS);
        }

        // Insert.
        if self.take_command_shortcut(ctx, C::InsertLayer) {
            self.insert_timeline_layer();
        }
        if self.take_command_shortcut(ctx, C::InsertLayerFolder) {
            self.insert_layer_folder(self.layer);
        }
        if self.take_command_shortcut(ctx, C::InsertMotionGuide)
            && self.can_add_motion_guide_to(self.layer)
        {
            self.add_motion_guide();
        }
        for (cmd, kind) in [
            (C::InsertClassicTween, neoflash_core::TweenKind::Classic),
            (C::InsertShapeTween, neoflash_core::TweenKind::Shape),
            (C::InsertMotionTween, neoflash_core::TweenKind::Motion),
        ] {
            if self.take_command_shortcut(ctx, cmd)
                && self.can_create_tween()
                && self.current_tween() != Some(kind)
            {
                self.create_tween(kind);
            }
        }
        if self.take_command_shortcut(ctx, C::InsertScene) {
            self.new_scene();
        }

        // Modify.
        if self.take_command_shortcut(ctx, C::ModifyDocument) {
            self.open_document_settings();
        }
        if self.take_command_shortcut(ctx, C::ModifyDistributeToLayers)
            && self.can_distribute_to_layers()
        {
            self.distribute_to_layers();
        }
        if self.take_command_shortcut(ctx, C::ModifyTraceBitmap) && self.can_trace_bitmap() {
            self.open_trace_bitmap(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineLayerProperties) {
            self.open_layer_properties(ctx, self.layer);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineClearKeyframe) {
            self.clear_keyframe();
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineConvertToKeyframes)
            && self.layer_editable()
        {
            self.convert_to_keyframes(false);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineConvertToBlankKeyframes)
            && self.layer_editable()
        {
            self.convert_to_keyframes(true);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineSplitClip) && self.layer_editable() {
            self.split_clip_at_playhead();
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineLinkTracks) && self.can_link_av_layers()
        {
            self.link_selected_or_adjacent_av_layers();
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineUnlinkTracks)
            && self.can_unlink_av_layers()
        {
            self.unlink_active_layers();
        }
        if self.take_command_shortcut(ctx, C::ModifyFreeTransform) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::FreeTransform);
        }
        if self.take_command_shortcut(ctx, C::ModifyDistort)
            && self.can_distort_or_envelope_selection()
        {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Distort);
        }
        if self.take_command_shortcut(ctx, C::ModifyEnvelope)
            && self.can_distort_or_envelope_selection()
        {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Envelope);
        }
        if self.take_command_shortcut(ctx, C::ModifyScale) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Scale);
        }
        if self.take_command_shortcut(ctx, C::ModifyRotateAndSkew) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::RotateAndSkew);
        }
        if self.take_command_shortcut(ctx, C::ModifyScaleAndRotate) && !self.selection.is_empty() {
            self.open_scale_and_rotate_dialog();
        }
        if self.take_command_shortcut(ctx, C::ModifyRotate90Cw) && !self.selection.is_empty() {
            self.rotate_90_cw_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyRotate90Ccw) && !self.selection.is_empty() {
            self.rotate_90_ccw_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyFlipVertical) && !self.selection.is_empty() {
            self.flip_vertical_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyFlipHorizontal) && !self.selection.is_empty() {
            self.flip_horizontal_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyRemoveTransform) && !self.selection.is_empty() {
            self.reset_selection_transform(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyDeleteEnvelope)
            && self.can_delete_envelope_selection()
        {
            self.delete_envelope_selection();
        }
        for (cmd, dir) in [
            (C::ModifyArrangeFront, Arrange::Front),
            (C::ModifyArrangeForward, Arrange::Forward),
            (C::ModifyArrangeBackward, Arrange::Backward),
            (C::ModifyArrangeBack, Arrange::Back),
        ] {
            if self.take_command_shortcut(ctx, cmd) && !self.selection.is_empty() {
                self.arrange_selection(dir);
            }
        }
        // Control.
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        if self.take_command_shortcut(ctx, C::ControlRewind) {
            self.seek_frame(0);
        }
        if self.take_command_shortcut(ctx, C::ControlGoToEnd) {
            self.seek_frame(last);
        }
        if self.take_command_shortcut(ctx, C::ControlLoopPlayback) {
            self.workspace.timeline.loop_playback = !self.workspace.timeline.loop_playback;
        }
        if self.take_command_shortcut(ctx, C::ControlPlayAllScenes) {
            self.timeline_edit.play_all_scenes = !self.timeline_edit.play_all_scenes;
        }
        if self.take_command_shortcut(ctx, C::ControlMuteSounds) {
            self.toggle_mute_sounds();
        }
        if self.take_command_shortcut(ctx, C::ControlEnableSimpleButtons) {
            self.set_enable_simple_buttons(!self.enable_simple_buttons);
        }

        // Window.
        for panel in PanelId::DETACHABLE {
            if panel == PanelId::Actions {
                continue;
            }
            if self.take_command_shortcut(ctx, C::for_panel(panel)) {
                let visible = !self.panels_hidden && self.workspace.is_panel_active(panel);
                self.set_panel_visible(panel, !visible);
            }
        }
        if self.take_command_shortcut(ctx, C::WindowHidePanels) {
            self.toggle_hide_panels();
        }
        if self.take_command_shortcut(ctx, C::WindowInspectorPriority) {
            self.give_dock_priority(PanelId::Properties);
        }
        if self.take_command_shortcut(ctx, C::WindowLibraryPriority) {
            self.give_dock_priority(PanelId::Library);
        }

        if self.take_command_shortcut(ctx, C::ViewShowShapeHints) {
            self.toggle_shape_hints();
        }
        if self.take_command_shortcut(ctx, C::CaptureStageA) {
            let label = format!("A (F{})", self.frame + 1);
            let _ = self.capture_stage_snapshot_a(label);
        }
        if self.take_command_shortcut(ctx, C::CaptureStageB) {
            let label = format!("B (F{})", self.frame + 1);
            let _ = self.capture_stage_snapshot_b(label);
        }
        if self.take_command_shortcut(ctx, C::ToggleStageAbSplit) {
            let _ = self.toggle_stage_ab_split();
        }
        if self.take_command_shortcut(ctx, C::ApplyMotionWiggler) {
            let cfg = self.timeline_motion.wiggler.clone();
            let _ = self.apply_motion_wiggler(cfg, None);
        }
        if self.take_command_shortcut(ctx, C::ToggleMotionSketch) {
            self.set_motion_sketch_armed(!self.is_motion_sketch_armed());
        }
        if self.take_command_shortcut(ctx, C::ExpandShapeAppearance) {
            let _ = self.expand_selected_shape_appearance();
        }
        if self.take_command_shortcut(ctx, C::SetWorkAreaIn) {
            let _ = self.set_work_area_in_at_playhead();
        }
        if self.take_command_shortcut(ctx, C::SetWorkAreaOut) {
            let _ = self.set_work_area_out_at_playhead();
        }
        if self.take_command_shortcut(ctx, C::ClearWorkArea) {
            let _ = self.clear_timeline_work_area();
        }
        if self.take_command_shortcut(ctx, C::AddTimelineMarker) {
            let f = self.frame;
            let count = self.active_timeline_markers().len() + 1;
            let _ = self.upsert_timeline_marker(neoflash_core::TimelineMarker {
                id: format!("marker-{f}-{count}"),
                frame: f,
                duration_frames: 1,
                name: format!("Marker {count}"),
                comment: String::new(),
                color_tag: Some("#4296FA".into()),
                cue_point_parameters: std::collections::BTreeMap::new(),
            });
        }
        if self.take_command_shortcut(ctx, C::NextTimelineMarker) {
            let _ = self.navigate_timeline_marker(true);
        }
        if self.take_command_shortcut(ctx, C::PreviousTimelineMarker) {
            let _ = self.navigate_timeline_marker(false);
        }
        if self.take_command_shortcut(ctx, C::ModifyAddShapeHint) {
            self.add_shape_hint();
        }
        if self.take_command_shortcut(ctx, C::ModifyRemoveAllShapeHints)
            && self.can_remove_shape_hints()
        {
            self.remove_all_shape_hints();
        }

        for cmd in [
            C::FileExportSwfFp8,
            C::FileExportSwfFp9,
            C::FileExportSwfFp10,
            C::FileExportSwfFp10Scriptless,
            C::HelpSearchFunctions,
            C::HelpManual,
            C::HelpScriptingReference,
            C::HelpOpenExample,
            C::HelpCheckForUpdates,
            C::HelpSendBetaFeedback,
            C::HelpAbout,
        ] {
            if self.command_menu_state(Some(ctx), cmd).enabled
                && self.take_command_shortcut(ctx, cmd)
            {
                let _ = self.execute_menu_command(ctx, cmd);
            }
        }

        // Unavailable items: consume only after active registry commands have had a chance to run,
        // so a user can rebind an active command onto F4 / ⌥⌘K / etc. if desired.
        let (cmd, shift, alt) = (Modifiers::COMMAND, Modifiers::SHIFT, Modifiers::ALT);
        for (m, key) in [(cmd, Key::P), (cmd | shift, Key::W), (alt | cmd, Key::K)] {
            ctx.input_mut(|i| {
                let held = i.modifiers;
                if modifiers_match(held, m) {
                    let _ = i.consume_key(m, key);
                }
            });
        }
    }

    /// Queue and dispatch a [`crate::commands::CommandId`] through the editor command router (`T-264`).
    #[allow(dead_code)]
    pub fn execute_command(&mut self, ctx: &egui::Context, cmd: crate::commands::CommandId) {
        crate::commands::queue_command(ctx, cmd);
        self.shortcuts(ctx);
    }
}
