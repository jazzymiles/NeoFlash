//! Transient frame selection and pointer gestures. Authored edits live in the core.
use crate::app::{Editor, ScriptTarget};
use crate::timeline_paint::{frame_at_x, timeline_cell_at};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::{
    FrameClipboard, FrameRange, Keyframe, Layer, LayerProperties, LayerRole, TimelineEdit,
    copy_frames, edit_frames,
};

#[derive(Default)]
pub(crate) struct TimelineSelection {
    pub focused: bool,
    pub include_scripts: bool,
    /// Edit multiple frames: the stage shows every keyframe in the onion range and
    /// moves apply to the selected objects on all of them.
    pub edit_multiple: bool,
    /// Control ▸ Play all scenes: editor playback continues into the next scene.
    pub play_all_scenes: bool,
    pub range: Option<FrameRange>,
    symbol: String,
    anchor: Option<(String, u32)>,
    clipboard: Option<FrameClipboard>,
    clipboard_marker: Option<String>,
    gesture: Option<Gesture>,
    viewport: Option<egui::ViewportId>,
}

#[derive(Clone)]
enum Gesture {
    Select {
        anchor_row: usize,
        anchor_frame: u32,
        press_row: usize,
        press_frame: u32,
        moved: bool,
    },
    Move {
        range: FrameRange,
        row: usize,
        frame: u32,
        target_row: usize,
        target_frame: u32,
    },
    Key {
        layer: usize,
        frame: u32,
    },
    End {
        layer: String,
        end: u32,
    },
}

/// Frame-range commands shared by Edit ▸ Timeline, the frame context menu and their shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameCommand {
    Cut,
    Copy,
    Paste,
    Clear,
    Remove,
    SelectAll,
}

impl TimelineSelection {
    pub fn dragging(&self) -> bool {
        self.gesture.is_some()
    }
}

impl Editor {
    pub(crate) fn selected_frame_range(&self) -> Option<(u32, u32, usize)> {
        if !self.timeline_edit.focused {
            return None;
        }
        let range = self.frame_range()?;
        Some((range.start, range.end, range.layers.len()))
    }

    pub(crate) fn frame_range(&self) -> Option<FrameRange> {
        (self.timeline_edit.symbol == self.symbol_id()).then_some(())?;
        let mut range = self.timeline_edit.range.clone()?;
        range
            .layers
            .retain(|id| self.symbol().timeline.layers.iter().any(|l| &l.id == id));
        if range.layers.is_empty() {
            return None;
        }
        range.end = range
            .end
            .min(self.symbol().timeline.frame_count.saturating_sub(1));
        range.start = range.start.min(range.end);
        Some(range)
    }

    pub(crate) fn select_frame_range(&mut self, range: FrameRange) {
        self.timeline_edit.symbol = self.symbol_id().to_owned();
        self.timeline_edit.range = Some(range);
        self.timeline_edit.focused = true;
    }

    fn frame_rectangle(
        &self,
        rows: &[usize],
        a: usize,
        b: usize,
        start: u32,
        end: u32,
    ) -> FrameRange {
        FrameRange {
            layers: rows[a.min(b)..=a.max(b)]
                .iter()
                .filter_map(|&i| {
                    let layer = &self.symbol().timeline.layers[i];
                    (layer.properties.role != LayerRole::Folder).then(|| layer.id.clone())
                })
                .collect(),
            start: start.min(end),
            end: start.max(end),
        }
    }

    pub(crate) fn apply_frame_edit(&mut self, edit: TimelineEdit) -> bool {
        self.finish_edits();
        let mut candidate = self.project.clone();
        match edit_frames(&mut candidate, self.symbol_id(), edit) {
            Ok(()) => {
                self.transact(|editor| editor.project = candidate);
                self.reset_authored_preview();
                self.normalize_selection();
                self.clear_selection();
                true
            }
            Err(error) => {
                self.message(error);
                false
            }
        }
    }

    pub(crate) fn copy_frame_range(&mut self, cut: bool) -> bool {
        let Some(range) = self.frame_range() else {
            return false;
        };
        let include_scripts = self.timeline_edit.include_scripts;
        match copy_frames(&self.project, self.symbol_id(), &range, include_scripts) {
            Ok(clip) => {
                if !cut
                    || self.apply_frame_edit(TimelineEdit::Clear {
                        range,
                        include_scripts,
                    })
                {
                    self.message(format!(
                        "{} {} frames on {} layers",
                        if cut { "Cut" } else { "Copied" },
                        clip.frame_count,
                        clip.layers.len()
                    ));
                    self.timeline_edit.clipboard = Some(clip);
                    self.timeline_edit.clipboard_marker = None;
                    true
                } else {
                    false
                }
            }
            Err(error) => {
                self.message(error);
                false
            }
        }
    }

    pub(crate) fn copy_frames_to_system_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        if self.copy_frame_range(cut) {
            // Native Paste emits a payload only when the system clipboard has
            // text. The actual frame data remains scoped to this live document.
            let marker = format!(
                "NeoFlash Timeline frames: {}:{}",
                self.clipboard.session,
                neoflash_core::new_id("frames")
            );
            self.timeline_edit.clipboard_marker = Some(marker.clone());
            ctx.copy_text(marker);
        }
    }

    pub(crate) fn paste_frames_from_system_clipboard(&mut self, payload: Option<String>) {
        let text = match payload.map(Ok).unwrap_or_else(|| {
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|error| error.to_string())
        }) {
            Ok(text) => text,
            Err(error) => {
                self.message(format!("Cannot read clipboard: {error}"));
                return;
            }
        };
        if self.timeline_edit.clipboard_marker.as_deref() != Some(text.as_str()) {
            self.timeline_edit.clipboard = None;
            self.timeline_edit.clipboard_marker = None;
            self.message("The clipboard no longer contains frames from this document. Copy frames in the Timeline again.");
            return;
        }
        self.paste_frame_range();
    }

    pub(crate) fn paste_frame_range(&mut self) {
        let Some(clip) = self.timeline_edit.clipboard.clone() else {
            self.message("Copy frames in the Timeline before pasting them.");
            return;
        };
        let Some(range) = self.frame_range() else {
            return;
        };
        let timeline = &self.symbol().timeline;
        let rows = crate::layer_controls::visible_layer_indices(timeline);
        let Some(first) = rows
            .iter()
            .position(|&index| range.layers.first() == Some(&timeline.layers[index].id))
        else {
            return;
        };
        let layers: Vec<_> = rows[first..]
            .iter()
            .map(|&index| &timeline.layers[index])
            .filter(|l| l.properties.role != LayerRole::Folder)
            .take(clip.layers.len())
            .map(|l| l.id.clone())
            .collect();
        let mut layers = layers;
        let missing = clip.layers.len().saturating_sub(layers.len());
        let end = range
            .start
            .saturating_add(clip.frame_count.saturating_sub(1));
        if missing > 0 {
            let symbol_id = self.symbol_id().to_owned();
            let mut candidate = self.project.clone();
            {
                let Some(symbol) = candidate.symbol_mut(&symbol_id) else {
                    return;
                };
                let timeline = &mut symbol.timeline;
                let parent = range
                    .layers
                    .first()
                    .and_then(|first_id| timeline.layers.iter().find(|l| &l.id == first_id))
                    .and_then(|l| l.properties.parent.clone());
                for _ in 0..missing {
                    let mut idx = timeline.layers.len() + 1;
                    let id = loop {
                        let cand_id = format!("layer-{idx}");
                        if !timeline.layers.iter().any(|l| l.id == cand_id) {
                            break cand_id;
                        }
                        idx += 1;
                    };
                    let name = format!("Layer {idx}");
                    let new_layer = Layer {
                        id: id.clone(),
                        name,
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties {
                            parent: parent.clone(),
                            ..Default::default()
                        },
                    };
                    // Display order goes top-to-bottom (`layers.iter().enumerate().rev()`), so
                    // inserting at index 0 places the new destination layer below existing rows.
                    timeline.layers.insert(0, new_layer);
                    layers.push(id);
                }
            }
            if let Err(err) = edit_frames(
                &mut candidate,
                &symbol_id,
                TimelineEdit::Paste {
                    clip,
                    layers: layers.clone(),
                    at: range.start,
                    include_scripts: self.timeline_edit.include_scripts,
                },
            ) {
                self.message(format!("Timeline edit failed: {err}"));
                return;
            }
            if let Err(err) = candidate.validate() {
                self.message(format!("Timeline edit failed: {err}"));
                return;
            }
            self.finish_edits();
            self.playing = false;
            let selected_layers = layers.clone();
            self.transact(|editor| {
                editor.project = candidate;
                editor.layer = editor
                    .layer
                    .min(editor.symbol().timeline.layers.len().saturating_sub(1));
                editor.frame = editor
                    .frame
                    .min(editor.symbol().timeline.frame_count.saturating_sub(1));
                editor.normalize_selection();
            });
            self.select_frame_range(FrameRange {
                layers: selected_layers,
                start: range.start,
                end,
            });
            return;
        }
        if self.apply_frame_edit(TimelineEdit::Paste {
            clip,
            layers: layers.clone(),
            at: range.start,
            include_scripts: self.timeline_edit.include_scripts,
        }) {
            self.select_frame_range(FrameRange {
                layers,
                start: range.start,
                end,
            });
        }
    }

    pub(crate) fn clear_frame_range(&mut self, remove: bool) {
        if let Some(range) = self.frame_range() {
            let include_scripts = self.timeline_edit.include_scripts;
            self.apply_frame_edit(if remove {
                TimelineEdit::Delete {
                    range,
                    include_scripts,
                }
            } else {
                TimelineEdit::Clear {
                    range,
                    include_scripts,
                }
            });
        }
    }

    pub(crate) fn insert_frame_range(&mut self) {
        if let Some(range) = self.frame_range() {
            self.apply_frame_edit(TimelineEdit::Insert {
                layers: range.layers,
                at: range.start,
                count: range.end - range.start + 1,
                include_scripts: self.timeline_edit.include_scripts,
            });
        }
    }

    pub(crate) fn set_layer_duration(&mut self, end_frame: u32) -> bool {
        let layers = if self.timeline_edit.focused {
            self.frame_range().map(|r| r.layers)
        } else {
            None
        }
        .unwrap_or_else(|| vec![self.symbol().timeline.layers[self.layer].id.clone()]);
        self.apply_frame_edit(TimelineEdit::SetEnd { layers, end_frame })
    }

    pub(crate) fn timeline_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::commands::CommandId;
        if !self.timeline_edit.focused
            || self.frame_range().is_none()
            || ctx.wants_keyboard_input()
            || self.confirm.is_some()
            || self
                .timeline_edit
                .viewport
                .is_some_and(|id| id != ctx.viewport_id())
        {
            return;
        }
        let mut copy = false;
        let mut cut = false;
        let mut paste = false;
        let mut paste_payload = None;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Copy => {
                    copy = true;
                    false
                }
                egui::Event::Cut => {
                    cut = true;
                    false
                }
                egui::Event::Paste(text) => {
                    paste = true;
                    paste_payload = Some(text.clone());
                    false
                }
                _ => true,
            })
        });
        copy |= self.take_command_shortcut(ctx, CommandId::EditCopy);
        cut |= self.take_command_shortcut(ctx, CommandId::EditCut);
        paste |= self.take_command_shortcut(ctx, CommandId::EditPaste);
        if cut {
            self.copy_frames_to_system_clipboard(ctx, true);
        } else if copy {
            self.copy_frames_to_system_clipboard(ctx, false);
        }
        if paste {
            self.paste_frames_from_system_clipboard(paste_payload);
        }
        if self.take_command_shortcut(ctx, CommandId::EditClear) {
            self.clear_frame_range(false);
        }
        if self.take_command_shortcut(ctx, CommandId::InsertFrame) {
            self.insert_frame_range();
        }
        if self.take_command_shortcut(ctx, CommandId::EditSelectAll) {
            self.select_all_frames();
        }
    }

    pub(crate) fn frame_command_enabled(&self, command: FrameCommand) -> bool {
        command == FrameCommand::SelectAll
            || (self.frame_range().is_some()
                && (command != FrameCommand::Paste || self.timeline_edit.clipboard.is_some()))
    }

    pub(crate) fn run_frame_command(&mut self, ctx: &egui::Context, command: FrameCommand) {
        if !self.frame_command_enabled(command) {
            return;
        }
        match command {
            FrameCommand::Cut => self.copy_frames_to_system_clipboard(ctx, true),
            FrameCommand::Copy => self.copy_frames_to_system_clipboard(ctx, false),
            FrameCommand::Paste => self.paste_frames_from_system_clipboard(None),
            FrameCommand::Clear => self.clear_frame_range(false),
            FrameCommand::Remove => self.clear_frame_range(true),
            FrameCommand::SelectAll => self.select_all_frames(),
        }
    }

    /// Every frame of every non-folder layer in the edited timeline.
    pub(crate) fn select_all_frames(&mut self) {
        let layers = self
            .symbol()
            .timeline
            .layers
            .iter()
            .filter(|l| l.properties.role != LayerRole::Folder)
            .map(|l| l.id.clone())
            .collect();
        self.select_frame_range(FrameRange {
            layers,
            start: 0,
            end: self.symbol().timeline.frame_count.saturating_sub(1),
        });
    }

    /// Inclusive `(start, end)` frame span containing `frame` on `layer_index`.
    /// ◆ property keys inside a motion tween belong to the enclosing motion span; frames outside
    /// the layer's exposure or before its first keyframe select only `(frame, frame)`.
    pub(crate) fn layer_span_range(
        &self,
        layer_index: usize,
        frame: u32,
        count: u32,
    ) -> (u32, u32) {
        let Some(layer) = self.symbol().timeline.layers.get(layer_index) else {
            return (frame, frame);
        };
        let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
        if frame >= layer_end || layer.properties.role == LayerRole::Folder {
            return (frame, frame);
        }
        if let Some((start, _, end)) = neoflash_core::motion_span(layer, frame) {
            let exclusive_end = end.unwrap_or(layer_end).min(layer_end);
            return if exclusive_end > start.frame {
                (start.frame, exclusive_end - 1)
            } else {
                (frame, frame)
            };
        }
        let Some(start) = layer
            .keyframes
            .iter()
            .filter(|k| k.frame <= frame && k.keyed_properties.is_none())
            .map(|k| k.frame)
            .max()
        else {
            return (frame, frame);
        };
        let next = layer
            .keyframes
            .iter()
            .filter(|k| k.frame > start && k.keyed_properties.is_none())
            .map(|k| k.frame)
            .min();
        let exclusive_end = next.unwrap_or(layer_end).min(layer_end);
        if exclusive_end > start {
            (start, exclusive_end - 1)
        } else {
            (frame, frame)
        }
    }

    // Shared viewport geometry is passed directly by the timeline painter.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn timeline_grid_interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        clip: Rect,
        cell: f32,
        count: u32,
        rows: &[usize],
        blocked: bool,
    ) {
        let row_height = self.workspace.timeline.row_height();
        if rows.is_empty() {
            return;
        }
        if self.timeline_edit.symbol != self.symbol_id() {
            self.timeline_edit.range = None;
            self.timeline_edit.anchor = None;
            self.timeline_edit.gesture = None;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused) {
            self.timeline_edit.gesture = None;
            self.timeline_key_drag = None;
            return;
        }
        let (pressed, released) = ui.input(|input| {
            let mut down = None;
            let mut up = None;
            for event in &input.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                } = event
                {
                    if *pressed {
                        down = Some((*pos, *modifiers));
                        up = None;
                    } else {
                        up = Some((*pos, *modifiers));
                    }
                }
            }
            (down, up)
        });
        if let Some((pos, modifiers)) = pressed.filter(|(pos, _)| {
            !blocked
                && ui.is_enabled()
                && clip.contains(*pos)
                && !response.context_menu_opened()
                && ui.ctx().layer_id_at(*pos) == Some(ui.layer_id())
        }) {
            let hit = timeline_cell_at(pos, rect.min, cell, count, rows.len(), row_height);
            let end_hit = rows
                .iter()
                .enumerate()
                .find(|(row, index)| {
                    let layer = &self.symbol().timeline.layers[**index];
                    layer.properties.role != LayerRole::Folder
                        && end_handle(
                            rect,
                            *row,
                            cell,
                            layer.properties.end_frame.unwrap_or(count),
                            row_height,
                        )
                        .contains(pos)
                })
                .map(|(_, &i)| {
                    (
                        self.symbol().timeline.layers[i].id.clone(),
                        self.symbol().timeline.layers[i]
                            .properties
                            .end_frame
                            .unwrap_or(count),
                    )
                });
            if hit.is_some() || end_hit.is_some() {
                self.finish_edits();
                self.set_playing(false);
                self.reset_authored_preview();
                self.clear_selection();
                self.timeline_edit.focused = true;
                self.timeline_edit.symbol = self.symbol_id().to_owned();
                self.timeline_edit.viewport = Some(ui.ctx().viewport_id());
                self.script_target = ScriptTarget::Frame;
            }
            if let Some((layer, end)) = end_hit {
                self.timeline_edit.gesture = Some(Gesture::End { layer, end });
            } else if let Some((frame, row)) = hit {
                let layer_id = self.symbol().timeline.layers[rows[row]].id.clone();
                let span_mode = self.workspace.timeline_prefs.span_selection && !modifiers.command;
                let existing = (!modifiers.command)
                    .then(|| {
                        self.frame_range().filter(|r| {
                            r.start <= frame && frame <= r.end && r.layers.contains(&layer_id)
                        })
                    })
                    .flatten();
                self.frame = frame;
                self.layer = rows[row];
                if modifiers.shift {
                    let (anchor_row, anchor_frame) = self
                        .timeline_edit
                        .anchor
                        .as_ref()
                        .and_then(|(id, frame)| {
                            rows.iter()
                                .position(|i| &self.symbol().timeline.layers[*i].id == id)
                                .map(|row| (row, *frame))
                        })
                        .unwrap_or((row, frame));
                    let (sel_start, sel_end) = if span_mode {
                        let (a0, a1) = self.layer_span_range(rows[anchor_row], anchor_frame, count);
                        let (b0, b1) = self.layer_span_range(rows[row], frame, count);
                        (a0.min(b0), a1.max(b1))
                    } else {
                        (anchor_frame, frame)
                    };
                    self.select_frame_range(
                        self.frame_rectangle(rows, anchor_row, row, sel_start, sel_end),
                    );
                    self.timeline_edit.gesture = Some(Gesture::Select {
                        anchor_row,
                        anchor_frame,
                        press_row: row,
                        press_frame: frame,
                        moved: false,
                    });
                } else if let Some(range) = existing {
                    self.timeline_edit.gesture = Some(Gesture::Move {
                        range,
                        row,
                        frame,
                        target_row: row,
                        target_frame: frame,
                    });
                } else {
                    let key = self.symbol().timeline.layers[rows[row]]
                        .keyframes
                        .iter()
                        .any(|key| key.frame == frame);
                    let (sel_start, sel_end) = if span_mode {
                        self.layer_span_range(rows[row], frame, count)
                    } else {
                        (frame, frame)
                    };
                    self.select_frame_range(
                        self.frame_rectangle(rows, row, row, sel_start, sel_end),
                    );
                    self.timeline_edit.anchor = Some((layer_id.clone(), frame));
                    self.timeline_edit.gesture = Some(if key {
                        self.timeline_key_drag =
                            Some((self.symbol_id().to_owned(), layer_id, frame));
                        Gesture::Key {
                            layer: rows[row],
                            frame,
                        }
                    } else {
                        Gesture::Select {
                            anchor_row: row,
                            anchor_frame: frame,
                            press_row: row,
                            press_frame: frame,
                            moved: false,
                        }
                    });
                }
            }
        } else if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((frame, row)) =
                timeline_cell_at(pos, rect.min, cell, count, rows.len(), row_height)
        {
            self.finish_edits();
            self.set_playing(false);
            self.reset_authored_preview();
            self.clear_selection();
            let layer_id = &self.symbol().timeline.layers[rows[row]].id;
            if !self
                .frame_range()
                .is_some_and(|r| r.start <= frame && frame <= r.end && r.layers.contains(layer_id))
            {
                let (sel_start, sel_end) = if self.workspace.timeline_prefs.span_selection {
                    self.layer_span_range(rows[row], frame, count)
                } else {
                    (frame, frame)
                };
                self.select_frame_range(self.frame_rectangle(rows, row, row, sel_start, sel_end));
            }
            self.timeline_edit.focused = true;
            self.timeline_edit.viewport = Some(ui.ctx().viewport_id());
            self.frame = frame;
            self.layer = rows[row];
            self.script_target = ScriptTarget::Frame;
        }
        let position = released
            .map(|p| p.0)
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        if let Some(pos) = position
            && let Some(gesture) = self.timeline_edit.gesture.clone()
        {
            let frame = frame_at_x(pos.x, rect.left(), cell, count);
            let row = (((pos.y - rect.top()) / row_height).max(0.) as usize)
                .min(rows.len().saturating_sub(1));
            let dragging = ui.input(|i| i.pointer.is_decidedly_dragging()) || response.dragged();
            if dragging || released.is_some() {
                match gesture {
                    Gesture::Select {
                        anchor_row,
                        anchor_frame,
                        press_row,
                        press_frame,
                        moved,
                    } => {
                        let moved = moved || row != press_row || frame != press_frame;
                        self.timeline_edit.gesture = Some(Gesture::Select {
                            anchor_row,
                            anchor_frame,
                            press_row,
                            press_frame,
                            moved,
                        });
                        if moved {
                            self.select_frame_range(self.frame_rectangle(
                                rows,
                                anchor_row,
                                row,
                                anchor_frame,
                                frame,
                            ));
                        }
                        self.frame = frame;
                        self.layer = rows[row];
                    }
                    Gesture::Move {
                        range,
                        row: start_row,
                        frame: start,
                        ..
                    } => {
                        self.timeline_edit.gesture = Some(Gesture::Move {
                            range,
                            row: start_row,
                            frame: start,
                            target_row: row,
                            target_frame: frame,
                        })
                    }
                    Gesture::Key { layer, .. } => {
                        self.frame = frame;
                        self.layer = layer;
                    }
                    Gesture::End { layer, .. } => {
                        let end =
                            (((pos.x - rect.left()) / cell).round().max(0.) as u32).min(1_000_000);
                        self.timeline_edit.gesture = Some(Gesture::End { layer, end });
                    }
                }
            }
        }
        if let Some((pos, modifiers)) = released {
            let gesture = self.timeline_edit.gesture.take();
            self.timeline_key_drag = None;
            if pos.y < clip.top()
                || pos.y
                    >= clip
                        .bottom()
                        .min(rect.top() + rows.len() as f32 * row_height)
            {
                return;
            }
            match gesture {
                Some(Gesture::Key { layer, frame }) if frame != self.frame => {
                    self.move_key(layer, frame, self.frame, modifiers.alt);
                    self.select_frame_range(FrameRange {
                        layers: vec![self.symbol().timeline.layers[layer].id.clone()],
                        start: self.frame,
                        end: self.frame,
                    });
                }
                Some(Gesture::Move {
                    range,
                    row,
                    frame,
                    target_row,
                    target_frame,
                }) => {
                    if row != target_row || frame != target_frame {
                        let delta = target_frame as i64 - frame as i64;
                        let start = (range.start as i64 + delta).max(0) as u32;
                        let first = rows
                            .iter()
                            .position(|i| {
                                Some(&self.symbol().timeline.layers[*i].id) == range.layers.first()
                            })
                            .unwrap_or(row);
                        let destination =
                            (first as isize + target_row as isize - row as isize).max(0) as usize;
                        let layers: Vec<_> = rows
                            .iter()
                            .skip(destination)
                            .map(|&i| &self.symbol().timeline.layers[i])
                            .filter(|l| l.properties.role != LayerRole::Folder)
                            .take(range.layers.len())
                            .map(|l| l.id.clone())
                            .collect();
                        let end = start + range.end - range.start;
                        if self.apply_frame_edit(TimelineEdit::Move {
                            range,
                            layers: layers.clone(),
                            at: start,
                            copy: modifiers.alt,
                            include_scripts: self.timeline_edit.include_scripts,
                        }) {
                            self.select_frame_range(FrameRange { layers, start, end });
                            self.frame = start;
                        }
                    } else if let Some(&layer_index) = rows.get(row) {
                        let layer_id = self.symbol().timeline.layers[layer_index].id.clone();
                        let (sel_start, sel_end) =
                            if self.workspace.timeline_prefs.span_selection && !modifiers.command {
                                self.layer_span_range(layer_index, frame, count)
                            } else {
                                (frame, frame)
                            };
                        self.select_frame_range(
                            self.frame_rectangle(rows, row, row, sel_start, sel_end),
                        );
                        self.timeline_edit.anchor = Some((layer_id, frame));
                    }
                }
                Some(Gesture::End { layer, end }) => {
                    self.apply_frame_edit(TimelineEdit::SetEnd {
                        layers: vec![layer],
                        end_frame: end,
                    });
                }
                _ => {}
            }
        }
    }

    pub(crate) fn paint_frame_selection(
        &self,
        ui: &egui::Ui,
        rect: Rect,
        cell: f32,
        rows: &[usize],
    ) {
        let row_height = self.workspace.timeline.row_height();
        // Selected frames are painted per row as `NF2/TL Span` Type=Selected (panels/timeline.rs)
        // so markers on top can switch to Tone=On Selection. Here: cursor feedback only.
        let palette = crate::theme::UiPalette::current(ui.ctx());
        if let Some(range) = self.frame_range() {
            for (row, &i) in rows.iter().enumerate() {
                if range.layers.contains(&self.symbol().timeline.layers[i].id) {
                    let selected = Rect::from_min_size(
                        Pos2::new(
                            rect.left() + range.start as f32 * cell,
                            rect.top() + row as f32 * row_height,
                        ),
                        Vec2::new((range.end - range.start + 1) as f32 * cell, row_height),
                    );
                    if ui.rect_contains_pointer(selected) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                }
            }
        }
        if let Some(Gesture::Move {
            range,
            row,
            frame,
            target_row,
            target_frame,
        }) = &self.timeline_edit.gesture
        {
            let first = rows
                .iter()
                .position(|i| Some(&self.symbol().timeline.layers[*i].id) == range.layers.first())
                .unwrap_or(*row);
            let destination =
                (first as isize + *target_row as isize - *row as isize).max(0) as usize;
            let start = (range.start as i64 + *target_frame as i64 - *frame as i64).max(0) as u32;
            for (row, _) in rows
                .iter()
                .enumerate()
                .skip(destination)
                .filter(|(_, i)| {
                    self.symbol().timeline.layers[**i].properties.role != LayerRole::Folder
                })
                .take(range.layers.len())
            {
                let preview = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + start as f32 * cell,
                        rect.top() + row as f32 * row_height,
                    ),
                    Vec2::new((range.end - range.start + 1) as f32 * cell, row_height),
                );
                ui.painter().rect_stroke(
                    preview,
                    0,
                    egui::Stroke::new(2.0_f32, palette.accent),
                    egui::StrokeKind::Inside,
                );
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        for (row, &i) in rows.iter().enumerate() {
            let layer = &self.symbol().timeline.layers[i];
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            let end = match &self.timeline_edit.gesture {
                Some(Gesture::End { layer: id, end }) if id == &layer.id => *end,
                _ => layer
                    .properties
                    .end_frame
                    .unwrap_or(self.symbol().timeline.frame_count),
            };
            let handle = end_handle(rect, row, cell, end, row_height);
            // The span end ▯ marks the layer end; the drag handle shows as an accent edge
            // while hovered or dragged (drop-indicator role).
            let dragging = matches!(&self.timeline_edit.gesture, Some(Gesture::End { layer: id, .. }) if id == &layer.id);
            if ui.rect_contains_pointer(handle) || dragging {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                let x = rect.left() + end as f32 * cell;
                let top = rect.top() + row as f32 * row_height;
                ui.painter().vline(
                    x,
                    top..=top + row_height,
                    egui::Stroke::new(2.0_f32, palette.accent),
                );
            }
            ui.interact(handle, ui.id().with(("exposure-end", &layer.id)), egui::Sense::hover())
                .on_hover_text(format!("Layer ends after frame {end} · drag to change duration. Shortening removes later keys."));
        }
    }
}

fn end_handle(rect: Rect, row: usize, cell: f32, end: u32, row_height: f32) -> Rect {
    Rect::from_center_size(
        Pos2::new(
            rect.left() + end as f32 * cell,
            rect.top() + (row + 1) as f32 * row_height - 4.,
        ),
        Vec2::new(8., 7.),
    )
}
