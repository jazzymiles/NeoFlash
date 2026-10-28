//! Transient frame selection and pointer gestures. Authored edits live in the core.
use crate::app::{Editor, ScriptTarget};
use crate::theme::DesignUi as _;
use crate::timeline_paint::{ROW_HEIGHT, frame_at_x, timeline_cell_at};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::{
    FrameClipboard, FrameRange, LayerRole, TimelineEdit, copy_frames, edit_frames,
};

#[derive(Default)]
pub(crate) struct TimelineSelection {
    pub focused: bool,
    pub include_scripts: bool,
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
        row: usize,
        frame: u32,
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

    fn frame_range(&self) -> Option<FrameRange> {
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

    fn copy_frames_to_system_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
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

    fn paste_frames_from_system_clipboard(&mut self, payload: Option<String>) {
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
        if layers.len() != clip.layers.len() {
            self.message("Paste needs one existing destination layer for each copied layer.");
            return;
        }
        let end = range
            .start
            .saturating_add(clip.frame_count.saturating_sub(1));
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
        copy |= ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::C));
        cut |= ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::X));
        paste |= ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::V));
        if cut {
            self.copy_frames_to_system_clipboard(ctx, true);
        } else if copy {
            self.copy_frames_to_system_clipboard(ctx, false);
        }
        if paste {
            self.paste_frames_from_system_clipboard(paste_payload);
        }
        if ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                | i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        }) {
            self.clear_frame_range(false);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            self.insert_frame_range();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
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
    }

    pub(crate) fn frame_range_menu(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.timeline_edit.include_scripts, "Include frame scripts")
            .on_hover_text("Frame scripts belong to this timeline, not to a layer. Include them once in frame copy, move, insert and remove operations. Script collisions are rejected.");
        ui.separator();
        for (label, shortcut, action) in [
            ("Copy Frames", "⌘C", 0),
            ("Cut Frames", "⌘X", 1),
            ("Paste Frames", "⌘V", 2),
            ("Insert Frames", "F5", 3),
            ("Clear Frames", "Delete", 4),
            ("Remove Frames", "", 5),
        ] {
            let enabled = self.frame_range().is_some()
                && (action != 2 || self.timeline_edit.clipboard.is_some());
            if ui
                .add_enabled_semantic(enabled, egui::Button::new(label).shortcut_text(shortcut))
                .clicked()
            {
                match action {
                    0 => self.copy_frames_to_system_clipboard(ui.ctx(), false),
                    1 => self.copy_frames_to_system_clipboard(ui.ctx(), true),
                    2 => self.paste_frames_from_system_clipboard(None),
                    3 => self.insert_frame_range(),
                    4 => self.clear_frame_range(false),
                    _ => self.clear_frame_range(true),
                }
                ui.close();
            }
        }
        ui.separator();
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
            let hit = timeline_cell_at(pos, rect.min, cell, count, rows.len());
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
                let existing = self
                    .frame_range()
                    .filter(|r| r.start <= frame && frame <= r.end && r.layers.contains(&layer_id));
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
                    self.select_frame_range(self.frame_rectangle(
                        rows,
                        anchor_row,
                        row,
                        anchor_frame,
                        frame,
                    ));
                    self.timeline_edit.gesture = Some(Gesture::Select {
                        row: anchor_row,
                        frame: anchor_frame,
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
                    self.select_frame_range(self.frame_rectangle(rows, row, row, frame, frame));
                    self.timeline_edit.anchor = Some((layer_id.clone(), frame));
                    self.timeline_edit.gesture = Some(if key {
                        self.timeline_key_drag =
                            Some((self.symbol_id().to_owned(), layer_id, frame));
                        Gesture::Key {
                            layer: rows[row],
                            frame,
                        }
                    } else {
                        Gesture::Select { row, frame }
                    });
                }
            }
        } else if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((frame, row)) = timeline_cell_at(pos, rect.min, cell, count, rows.len())
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
                self.select_frame_range(self.frame_rectangle(rows, row, row, frame, frame));
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
            let row = (((pos.y - rect.top()) / ROW_HEIGHT).max(0.) as usize)
                .min(rows.len().saturating_sub(1));
            let dragging = ui.input(|i| i.pointer.is_decidedly_dragging()) || response.dragged();
            if dragging || released.is_some() {
                match gesture {
                    Gesture::Select {
                        row: anchor,
                        frame: start,
                    } => {
                        self.select_frame_range(
                            self.frame_rectangle(rows, anchor, row, start, frame),
                        );
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
                        .min(rect.top() + rows.len() as f32 * ROW_HEIGHT)
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
                }) if row != target_row || frame != target_frame => {
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
        let palette = crate::theme::UiPalette::current(ui.ctx());
        if let Some(range) = self.frame_range() {
            for (row, &i) in rows.iter().enumerate() {
                if range.layers.contains(&self.symbol().timeline.layers[i].id) {
                    let selected = Rect::from_min_size(
                        Pos2::new(
                            rect.left() + range.start as f32 * cell,
                            rect.top() + row as f32 * ROW_HEIGHT,
                        ),
                        Vec2::new((range.end - range.start + 1) as f32 * cell, ROW_HEIGHT),
                    );
                    ui.painter().rect_filled(
                        selected,
                        0,
                        palette.selection_fill.linear_multiply(0.45),
                    );
                    ui.painter().rect_stroke(
                        selected,
                        0,
                        egui::Stroke::new(1.5_f32, palette.selection_stroke),
                        egui::StrokeKind::Inside,
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
                        rect.top() + row as f32 * ROW_HEIGHT,
                    ),
                    Vec2::new((range.end - range.start + 1) as f32 * cell, ROW_HEIGHT),
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
            let handle = end_handle(rect, row, cell, end);
            ui.painter()
                .rect_filled(handle.shrink(1.), 1, palette.timeline_key);
            if ui.rect_contains_pointer(handle) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            ui.interact(handle, ui.id().with(("exposure-end", &layer.id)), egui::Sense::hover())
                .on_hover_text(format!("Layer ends after frame {end} · drag to change duration. Shortening removes later keys."));
        }
    }
}

fn end_handle(rect: Rect, row: usize, cell: f32, end: u32) -> Rect {
    Rect::from_center_size(
        Pos2::new(
            rect.left() + end as f32 * cell,
            rect.top() + (row + 1) as f32 * ROW_HEIGHT - 4.,
        ),
        Vec2::new(8., 7.),
    )
}
