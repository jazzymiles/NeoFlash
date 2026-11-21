use eframe::egui::{self, Align, Align2, CursorIcon, Pos2, Rect, Sense, Ui, Vec2, vec2};
use neoflash_ui::{metrics, tokens::space};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ReaderWindow {
    normal: Option<Rect>,
    maximized: bool,
    drag_start: Option<(Rect, Pos2)>,
}

impl ReaderWindow {
    pub(super) fn bounds(self, viewport: Rect) -> Rect {
        if self.maximized {
            return viewport;
        }
        let size = vec2(metrics::reference_width(), metrics::dialog_xl_height())
            .min((viewport.size() - Vec2::splat(2.0 * space::s5())).max(Vec2::ZERO));
        let default =
            Align2::CENTER_TOP.anchor_size(viewport.center_top() + vec2(0.0, space::s5()), size);
        let normal = self.normal.unwrap_or(default);
        let size = normal.size().min(viewport.size());
        let min = normal.min.clamp(viewport.min, viewport.max - size);
        Rect::from_min_size(min, size)
    }

    pub(super) fn toggle_maximized(&mut self) {
        self.maximized = !self.maximized;
        self.drag_start = None;
    }

    pub(super) fn resize_edges(&mut self, ui: &mut Ui, bounds: Rect, viewport: Rect) {
        if self.maximized {
            return;
        }
        let minimum = vec2(metrics::dialog_l(), metrics::dialog_l_height()).min(bounds.size());
        let interaction = &ui.style().interaction;
        let side = interaction.resize_grab_radius_side;
        let corner = interaction.resize_grab_radius_corner;
        // Reserve each corner exclusively for diagonal resizing.
        for (edge, cursor) in [
            (Align2::LEFT_CENTER, CursorIcon::ResizeHorizontal),
            (Align2::RIGHT_CENTER, CursorIcon::ResizeHorizontal),
            (Align2::CENTER_TOP, CursorIcon::ResizeVertical),
            (Align2::CENTER_BOTTOM, CursorIcon::ResizeVertical),
            (Align2::LEFT_TOP, CursorIcon::ResizeNwSe),
            (Align2::RIGHT_BOTTOM, CursorIcon::ResizeNwSe),
            (Align2::RIGHT_TOP, CursorIcon::ResizeNeSw),
            (Align2::LEFT_BOTTOM, CursorIcon::ResizeNeSw),
        ] {
            let handle_size = match (edge.x(), edge.y()) {
                (Align::Center, _) => vec2((bounds.width() - 2.0 * corner).max(0.0), side),
                (_, Align::Center) => vec2(side, (bounds.height() - 2.0 * corner).max(0.0)),
                _ => Vec2::splat(corner),
            };
            let handle = edge.align_size_within_rect(handle_size, bounds);
            let response = ui
                .interact(handle, ui.id().with(("resize", edge)), Sense::drag())
                .on_hover_cursor(cursor);
            if response.drag_started_by(egui::PointerButton::Primary) {
                self.drag_start = ui.input(|i| i.pointer.press_origin().map(|p| (bounds, p)));
            }
            if (response.dragged_by(egui::PointerButton::Primary)
                || response.drag_stopped_by(egui::PointerButton::Primary))
                && let Some((start, pointer)) = self.drag_start
                && let Some(pos) = response.interact_pointer_pos()
            {
                let delta = pos - pointer;
                let mut next = bounds;
                for (axis, align) in [edge.x(), edge.y()].into_iter().enumerate() {
                    match align {
                        Align::Min => {
                            next.min[axis] = (start.min[axis] + delta[axis])
                                .clamp(viewport.min[axis], next.max[axis] - minimum[axis])
                        }
                        Align::Max => {
                            next.max[axis] = (start.max[axis] + delta[axis])
                                .clamp(next.min[axis] + minimum[axis], viewport.max[axis])
                        }
                        Align::Center => {}
                    }
                }
                self.normal = Some(next);
                ui.ctx().set_cursor_icon(cursor);
                ui.ctx().request_repaint();
            }
            if response.drag_stopped() {
                self.drag_start = None;
            }
        }
    }
}
