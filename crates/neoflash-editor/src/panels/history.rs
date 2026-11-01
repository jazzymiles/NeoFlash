//! History panel (NF2 `Panel / History`, PP-09 / W1.1; reference `panels-more.png`): chronological
//! undo/redo step list (`NF2/Menu Item` rows, current state checked, undone steps dimmed) and the
//! bottom Undo · Redo · Clear history icon row.
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;

impl Editor {
    pub(crate) fn history_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::History, "History");
        let undo_labels: Vec<String> = self.history.undo_labels().map(str::to_owned).collect();
        let redo_labels: Vec<String> = self.history.redo_labels().map(str::to_owned).collect();
        let undo_len = undo_labels.len();
        let footer_h = crate::theme::CONTROL_HEIGHT + space::S2;
        let list_h = (ui.available_height() - footer_h).max(crate::theme::CONTROL_HEIGHT);
        let mut target_step = None;

        egui::ScrollArea::vertical()
            .id_salt("history-steps")
            .auto_shrink([false, false])
            .max_height(list_h)
            .themed()
            .show(ui, |ui| {
                c::menu::menu_style(ui.style_mut());
                if undo_labels.is_empty() && redo_labels.is_empty() {
                    ui.add_enabled(false, c::MenuItem::new("Open").checked(true));
                    return;
                }
                let open_row = ui.add(c::MenuItem::new("Open").checked(undo_len == 0));
                if open_row.clicked() && undo_len > 0 {
                    target_step = Some(0);
                }
                for (i, label) in undo_labels.iter().enumerate() {
                    let is_current = i + 1 == undo_len;
                    let response = ui.add(c::MenuItem::new(label).checked(is_current));
                    if response.clicked() && !is_current {
                        target_step = Some(i + 1);
                    }
                }
                for (offset, label) in redo_labels.iter().enumerate() {
                    let step = undo_len + offset + 1;
                    // Undone steps render in NF2 disabled state (`panels-more.png`) while still
                    // letting the user click any row to step forward without discarding redo.
                    let shown = ui.add_enabled(false, c::MenuItem::new(label));
                    let click = ui.interact(
                        shown.rect,
                        ui.id().with(("history-redo-step", step)),
                        egui::Sense::click(),
                    );
                    if ui.is_enabled() && click.clicked() {
                        target_step = Some(step);
                    }
                }
            });

        if let Some(step) = target_step {
            self.step_history_to(step);
        }

        ui.add_space(space::S2);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.history.can_undo() || self.vector.draft.is_some(),
                    c::IconButton::new(i16::UNDO, "Undo").shortcut("⌘Z"),
                )
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(
                    self.history.can_redo(),
                    c::IconButton::new(i16::REDO, "Redo").shortcut("⇧⌘Z"),
                )
                .clicked()
            {
                self.redo();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.history.is_empty(),
                        c::IconButton::new(i16::TRASH, "Clear history"),
                    )
                    .clicked()
                {
                    self.clear_history();
                }
            });
        });
    }
}
