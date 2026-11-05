//! R-035 document strip, list, empty state and keyboard navigation.
use crate::app::{Editor, Pending};
use crate::theme::DesignUi as _;
use eframe::egui;
use neoflash_ui::{components as nf, icons::i16, tokens::space};

struct TabInfo {
    name: String,
    path: String,
    folder: String,
    dirty: bool,
}

impl Editor {
    fn document_tabs(&self) -> Vec<TabInfo> {
        (0..self.session_count())
            .map(|index| {
                let (project, path, dirty) = if index == self.active_session {
                    (&self.project, &self.path, self.dirty())
                } else {
                    let session = self.sessions[index].as_ref().expect("inactive session");
                    (&session.project, &session.path, session.dirty())
                };
                TabInfo {
                    name: path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| project.name.clone()),
                    path: path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| project.name.clone()),
                    folder: path
                        .as_ref()
                        .and_then(|p| p.parent())
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    dirty,
                }
            })
            .collect()
    }

    pub(crate) fn document_strip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let width = ui.available_width();
        if width <= 0. {
            return;
        }
        let active_changed = ctx.data_mut(|data| {
            let id = egui::Id::new("document-strip-active-session");
            let changed = data.get_temp::<String>(id).as_ref() != Some(&self.clipboard.session);
            data.insert_temp(id, self.clipboard.session.clone());
            changed
        });
        let mut activate = None;
        let mut close = None;
        ui.allocate_ui_with_layout(
            egui::vec2(width, nf::headers::DOCUMENT_TAB_HEIGHT),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let list = ui.add(nf::IconButton::new(i16::CHEVDOWN, "Open documents"));
                nf::popup_below(&list, 280., |ui| self.document_list(ui, &ctx));
                let tabs = self.document_tabs();
                egui::ScrollArea::horizontal()
                    .id_salt("document-strip")
                    .auto_shrink([false, true])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::S1;
                            for (index, tab) in tabs.iter().enumerate() {
                                ui.push_id(index, |ui| {
                                    let mut widget = nf::DocumentTab::new(&tab.name)
                                        .active(index == self.active_session)
                                        .dirty(tab.dirty)
                                        .path(&tab.path);
                                    if tabs.iter().filter(|other| other.name == tab.name).count()
                                        > 1
                                    {
                                        widget = widget.folder(&tab.folder);
                                    }
                                    let response = widget.show(ui);
                                    if active_changed && index == self.active_session {
                                        response.response.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    if response.close_clicked() {
                                        close = Some(index);
                                    } else if response.response.clicked() {
                                        activate = Some(index);
                                    }
                                });
                            }
                        });
                    });
            },
        );
        if let Some(index) = close {
            self.close_session_tab(&ctx, index);
        } else if let Some(index) = activate {
            self.switch_session_tab(&ctx, index);
        }
    }

    pub(crate) fn document_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        nf::menu_section(ui, "Open documents");
        for (index, tab) in self.document_tabs().iter().enumerate() {
            let label = format!("{}{}", tab.name, if tab.dirty { " — unsaved" } else { "" });
            if ui
                .add(nf::MenuItem::new(&label).checked(index == self.active_session))
                .on_hover_text(&tab.path)
                .clicked()
            {
                self.switch_session_tab(ctx, index);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        ui.add_enabled_ui_semantic(self.session_count() > 1, |ui| {
            if ui
                .add(nf::MenuItem::new("Next document").shortcut("⌃⇥"))
                .clicked()
            {
                self.cycle_document(ctx, false);
                ui.close();
            }
            if ui
                .add(nf::MenuItem::new("Previous document").shortcut("⌃⇧⇥"))
                .clicked()
            {
                self.cycle_document(ctx, true);
                ui.close();
            }
        });
        ui.add_enabled_ui_semantic(self.has_document(), |ui| {
            if ui.add(nf::MenuItem::new("Close").shortcut("⌘W")).clicked() {
                self.close_session_tab(ctx, self.active_session);
                ui.close();
            }
            ui.add_enabled_ui_semantic(self.session_count() > 1, |ui| {
                if ui.add(nf::MenuItem::new("Close other documents")).clicked() {
                    self.request_close_documents(ctx, true);
                    ui.close();
                }
            });
            if ui
                .add(nf::MenuItem::new("Close all").shortcut("⌥⌘W"))
                .clicked()
            {
                self.request_close_documents(ctx, false);
                ui.close();
            }
        });
    }

    pub(crate) fn document_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        let previous = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let next = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let close_all = egui::Modifiers {
            command: true,
            mac_cmd: true,
            alt: true,
            ..Default::default()
        };
        if ctx.input_mut(|i| i.consume_key(previous, egui::Key::Tab)) {
            self.cycle_document(ctx, true);
            return true;
        }
        if ctx.input_mut(|i| i.consume_key(next, egui::Key::Tab)) {
            self.cycle_document(ctx, false);
            return true;
        }
        if ctx.input_mut(|i| i.consume_key(close_all, egui::Key::W)) {
            self.request_close_documents(ctx, false);
            return true;
        }
        false
    }

    pub(crate) fn no_document_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(nf::Surface::App.frame(ctx))
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space((ui.available_height() / 2. - 80.).max(0.));
                    nf::EmptyState {
                        icon: i16::FILE_CODE,
                        title: "No document open",
                        body: "",
                        action: None,
                    }
                    .show(ui);
                    if ui.add(nf::Button::primary("New document")).clicked() {
                        self.request(Pending::New, ctx);
                    }
                    if ui.add(nf::Button::ghost("Open…")).clicked() {
                        self.request(Pending::Open, ctx);
                    }
                });
            });
    }
}
