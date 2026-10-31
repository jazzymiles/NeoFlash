//! `NF2/Icon Group` with per-item state: each item may be disabled (`State=Disabled`) and carry the
//! shortcut shown in its tooltip (timeline frame tools F5 / F6 / F7, tween kinds the engine lacks).
//! Same geometry and colours as [`IconGroup::show`].
use super::IconGroup;
use crate::theme::{cr, palette};
use crate::tokens::radius;
use eframe::egui::{self, Sense, Stroke, StrokeKind, Ui};

impl IconGroup<'_> {
    /// Like [`IconGroup::show`], with `enabled[i]` (default `true`) and `shortcuts[i]` (default none)
    /// per item. Disabled items keep their tooltip so the reason stays discoverable.
    pub fn show_with(
        self,
        ui: &mut Ui,
        enabled: &[bool],
        shortcuts: &[Option<&str>],
    ) -> Option<usize> {
        let p = palette(ui.ctx());
        let w = self.items.len() as f32 * 28.0 + 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 34.0), Sense::hover());
        ui.painter().rect_filled(rect, cr(radius::LG), p.bg_field);
        ui.painter().rect_stroke(
            rect,
            cr(radius::LG),
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        let mut clicked = None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(3.0))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (icon, name)) in self.items.iter().enumerate() {
                    let mut button = super::IconButton::new(*icon, name)
                        .side(28.0)
                        .selected(self.selected == Some(i));
                    if let Some(Some(key)) = shortcuts.get(i) {
                        button = button.shortcut(key);
                    }
                    if ui
                        .add_enabled(enabled.get(i).copied().unwrap_or(true), button)
                        .clicked()
                    {
                        clicked = Some(i);
                    }
                }
            },
        );
        clicked
    }
}
