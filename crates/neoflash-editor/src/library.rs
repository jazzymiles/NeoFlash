//! Authoring library: portable folders live in the document; view preferences do not.
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::{
    app::Editor,
    ui_icons::{Icon, paint_icon},
    workspace::PanelId,
};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::{ElementKind, InstancePlayback, SymbolKind};
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct LibraryPanel {
    pub selected_folder: String,
    search: String,
    sort: LibrarySort,
    descending: bool,
    preview_open: Option<bool>,
    preview_height: f32,
    collapsed: HashSet<String>,
    editing: Option<LibraryEdit>,
    pub(crate) drag: Option<LibraryDrag>,
    drag_press: Option<(egui::ViewportId, u64, Pos2)>,
}

#[derive(Clone)]
pub(crate) enum LibraryDrag {
    Symbol(String),
    Resource(String),
}

/// Keep a source-row press even when native input batches the entire drag in
/// one paint. Stage owns release/cancellation; ordinary row clicks keep their
/// existing selection, double-click and context-menu responses.
pub(crate) fn capture_row_drag(
    state: &mut LibraryPanel,
    ui: &egui::Ui,
    response: &egui::Response,
    payload: impl FnOnce() -> LibraryDrag,
) {
    let press = ui.input(|input| {
        input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } => Some((*pos, input.time.to_bits())),
            _ => None,
        })
    });
    let Some((position, time)) = press.filter(|(position, _)| {
        ui.is_enabled()
            && response.rect.contains(*position)
            && ui.clip_rect().contains(*position)
            && ui.ctx().layer_id_at(*position) == Some(ui.layer_id())
    }) else {
        return;
    };
    let key = (ui.ctx().viewport_id(), time, position);
    if state.drag_press == Some(key) {
        return;
    }
    state.drag_press = Some(key);
    state.drag = Some(payload());
    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum LibrarySort {
    #[default]
    Name,
    Type,
    Linkage,
}
impl LibrarySort {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
            Self::Linkage => "Linkage",
        }
    }
}

struct LibraryEdit {
    target: EditTarget,
    name: String,
    parent: String,
    kind: Option<SymbolKind>,
    focus: bool,
    error: Option<String>,
}
enum EditTarget {
    NewFolder,
    Folder(String),
    Symbol(String),
}

#[derive(Clone)]
struct LibrarySymbol {
    id: String,
    name: String,
    folder: String,
    kind: SymbolKind,
    linkage: String,
    class: String,
    frames: u32,
}
enum LibraryRow {
    Folder { path: String, depth: usize },
    Symbol { symbol: LibrarySymbol, depth: usize },
}
enum Action {
    SelectFolder(String),
    Toggle(String),
    SelectSymbol(String, String),
    EditSymbol(String),
    RenameSymbol(String),
    EditFolder(String),
    NewFolder(String),
}

impl Editor {
    pub(crate) fn library_body(&mut self, ui: &mut egui::Ui) {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.library_panel.drag = None;
        }
        // The shell owns horizontal geometry. In particular, a previously
        // resized preview must not enlarge the surrounding dock's min width.
        let content_width = ui
            .available_width()
            .min(ui.clip_rect().right() - ui.cursor().left())
            .max(1.);
        ui.set_max_width(content_width);
        self.panel_title(ui, PanelId::Library, "Library");
        ui.separator();
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 52.).max(24.);
            ui.add_sized(
                [width, crate::theme::CONTROL_HEIGHT],
                egui::TextEdit::singleline(&mut self.library_panel.search)
                    .hint_text("Search library"),
            )
            .on_hover_text("Search symbol names, folders, linkage identifiers and classes");
            if ui
                .add_enabled_semantic(
                    !self.library_panel.search.is_empty(),
                    egui::Button::new("Clear")
                        .min_size(Vec2::new(44., crate::theme::CONTROL_HEIGHT)),
                )
                .clicked()
            {
                self.library_panel.search.clear();
            }
        });
        let height = ui.available_height();
        let selected = self
            .library_selected
            .as_deref()
            .and_then(|id| self.project.symbol(id))
            .map(|symbol| LibrarySymbol {
                id: symbol.id.clone(),
                name: symbol.name.clone(),
                folder: symbol.folder.clone(),
                kind: symbol.kind,
                linkage: symbol.linkage_id.clone().unwrap_or_default(),
                class: symbol.class_name.clone().unwrap_or_default(),
                frames: symbol.timeline.frame_count,
            });
        if let Some(symbol) = selected {
            let spacing = ui.spacing().item_spacing.y;
            let metadata = preview_metadata(&symbol);
            let name_height = ui
                .painter()
                .layout_no_wrap(
                    symbol.name.clone(),
                    FontId::proportional(crate::theme::PANEL_FONT),
                    ui.visuals().text_color(),
                )
                .size()
                .y
                .max(16.);
            let metadata_height = ui
                .painter()
                .layout_no_wrap(
                    metadata.clone(),
                    FontId::proportional(crate::theme::SECONDARY_FONT),
                    ui.visuals().text_color(),
                )
                .size()
                .y
                .max(16.);
            // Header/search have already consumed their measured height. Reserve
            // sort, three list rows, footer and the three separator strips.
            let reserved = crate::theme::CONTROL_HEIGHT
                + crate::theme::ROW_HEIGHT * 3.
                + crate::theme::FOOTER_HEIGHT
                + 3. * 6.
                + 7. * spacing;
            let image_budget = height
                - reserved
                - crate::theme::CONTROL_HEIGHT
                - name_height
                - metadata_height
                - 4. * spacing;
            if image_budget >= 64. {
                let open = *self
                    .library_panel
                    .preview_open
                    .get_or_insert(height >= 330.);
                let result = egui::CollapsingHeader::new("Preview")
                    .id_salt("library-preview")
                    .open(Some(open))
                    .show(ui, |ui| {
                        self.library_preview(ui, &symbol, &metadata, image_budget.min(160.))
                    });
                if result.header_response.clicked() {
                    self.library_panel.preview_open = Some(!open);
                }
            } else {
                let trigger = ui
                    .add_sized(
                        [content_width, crate::theme::CONTROL_HEIGHT],
                        egui::Button::new("Preview").right_text("Open…"),
                    )
                    .on_hover_text("Open symbol preview");
                trigger.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        "Open symbol preview",
                    )
                });
                let popup = egui::Popup::from_toggle_button_response(&trigger)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .width(content_width.clamp(248., 360.));
                let popup_id = popup.get_id();
                let was_open = popup.is_open();
                popup.show(|ui| self.library_preview(ui, &symbol, &metadata, 160.));
                if was_open && !egui::Popup::is_id_open(ui.ctx(), popup_id) {
                    trigger.request_focus();
                }
            }
        }
        ui.separator();
        self.library_browser(ui);
    }

    /// The same first-frame renderer, metadata and preferred height serve both
    /// inline and overflow previews. Temporary panel constraints never overwrite
    /// the user's preferred height or inline disclosure preference.
    fn library_preview(
        &mut self,
        ui: &mut egui::Ui,
        symbol: &LibrarySymbol,
        metadata: &str,
        maximum: f32,
    ) {
        let width = ui
            .available_width()
            .min(ui.clip_rect().right() - ui.cursor().left())
            .max(1.);
        if !(64. ..=160.).contains(&self.library_panel.preview_height) {
            self.library_panel.preview_height = 96.;
        }
        let height = self
            .library_panel
            .preview_height
            .min(maximum)
            .clamp(64., 160.);
        let rect = Rect::from_min_size(ui.cursor().min, Vec2::new(width, height));
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            self.paint_library_preview(ui, &symbol.id)
        });
        let grip = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - 6.),
            rect.right_bottom(),
        );
        let resize = ui
            .interact(grip, ui.id().with("library-preview-height"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text("Resize symbol preview");
        resize.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::DragValue,
                ui.is_enabled(),
                "Resize symbol preview",
            )
        });
        let palette = crate::theme::UiPalette::current(ui.ctx());
        ui.painter().line_segment(
            [
                grip.center() - Vec2::new(12., 0.),
                grip.center() + Vec2::new(12., 0.),
            ],
            Stroke::new(
                if resize.has_focus() { 2.0_f32 } else { 1.0_f32 },
                if resize.has_focus() {
                    palette.focus
                } else {
                    palette.border
                },
            ),
        );
        if resize.dragged()
            && let Some(position) = resize.interact_pointer_pos()
        {
            self.library_panel.preview_height = (position.y - rect.top()).clamp(64., 160.);
        }
        if resize.has_focus() {
            ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    self.library_panel.preview_height =
                        (self.library_panel.preview_height - 8.).max(64.);
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    self.library_panel.preview_height =
                        (self.library_panel.preview_height + 8.).min(160.);
                }
            });
        }
        ui.add_sized(
            [width, 16.],
            egui::Label::new(egui::RichText::new(&symbol.name).strong())
                .truncate()
                .halign(egui::Align::Min),
        )
        .on_hover_text(&symbol.name);
        ui.add_sized(
            [width, 16.],
            egui::Label::new(egui::RichText::new(metadata).size(crate::theme::SECONDARY_FONT))
                .truncate()
                .halign(egui::Align::Min),
        )
        .on_hover_text(metadata);
    }

    fn library_browser(&mut self, ui: &mut egui::Ui) {
        let folders = self.project.library_folder_paths();
        if !self.library_panel.selected_folder.is_empty()
            && !folders.contains(&self.library_panel.selected_folder)
        {
            self.library_panel.selected_folder.clear();
        }
        let symbols: Vec<_> = self
            .project
            .symbols
            .iter()
            .filter(|symbol| symbol.id != self.project.root)
            .map(|symbol| LibrarySymbol {
                id: symbol.id.clone(),
                name: symbol.name.clone(),
                folder: symbol.folder.clone(),
                kind: symbol.kind,
                linkage: symbol.linkage_id.clone().unwrap_or_default(),
                class: symbol.class_name.clone().unwrap_or_default(),
                frames: symbol.timeline.frame_count,
            })
            .collect();
        let selected = self
            .library_selected
            .as_ref()
            .and_then(|id| symbols.iter().find(|symbol| &symbol.id == id))
            .cloned();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("library-sort")
                .width((ui.available_width() - 34. - ui.spacing().item_spacing.x).max(24.))
                .truncate()
                .selected_text(format!("Sort: {}", self.library_panel.sort.label()))
                .show_ui(ui, |ui| {
                    for sort in [LibrarySort::Name, LibrarySort::Type, LibrarySort::Linkage] {
                        ui.selectable_value(&mut self.library_panel.sort, sort, sort.label());
                    }
                });
            if ui
                .add_sized(
                    [34., 24.],
                    egui::Button::new(if self.library_panel.descending {
                        "Z–A"
                    } else {
                        "A–Z"
                    }),
                )
                .on_hover_text("Reverse sort order")
                .clicked()
            {
                self.library_panel.descending = !self.library_panel.descending;
            }
        });
        ui.separator();
        let query = self.library_panel.search.trim().to_lowercase();
        let mut rows = Vec::new();
        build_rows(
            "",
            0,
            &folders,
            &symbols,
            &self.library_panel,
            &query,
            &mut rows,
        );
        let mut action = None;
        let tree_height = (ui.available_height() - crate::theme::FOOTER_HEIGHT - 12.).max(0.);
        egui::ScrollArea::vertical()
            .id_salt("library-tree")
            .max_width(ui.available_width())
            .auto_shrink([false, false])
            .max_height(tree_height)
            .min_scrolled_height(tree_height.min(crate::theme::ROW_HEIGHT))
            .themed()
            .show(ui, |ui| {
                self.library_edit_form(ui, &folders);
                let response = library_row(
                    ui,
                    "Library root",
                    "",
                    0,
                    true,
                    self.library_selected.is_none()
                        && self.library_panel.selected_folder.is_empty(),
                    true,
                );
                if response.clicked() {
                    action = Some(Action::SelectFolder(String::new()));
                }
                response.context_menu(|ui| {
                    crate::ui_icons::menu_style(ui.style_mut());
                    ui.set_width(240.);
                    if crate::ui_icons::menu_item(ui, "New folder", "").clicked() {
                        action = Some(Action::NewFolder(String::new()));
                        ui.close();
                    }
                });
                for row in rows {
                    match row {
                        LibraryRow::Folder { path, depth } => {
                            let expanded =
                                !query.is_empty() || !self.library_panel.collapsed.contains(&path);
                            let response = library_row(
                                ui,
                                basename(&path),
                                "",
                                depth + 1,
                                true,
                                self.library_selected.is_none()
                                    && self.library_panel.selected_folder == path,
                                expanded,
                            );
                            if response.clicked() {
                                let on_disclosure =
                                    response.interact_pointer_pos().is_some_and(|pos| {
                                        pos.x
                                            < response.rect.left() + (depth + 1) as f32 * 12. + 16.
                                    });
                                action = Some(if on_disclosure {
                                    Action::Toggle(path.clone())
                                } else {
                                    Action::SelectFolder(path.clone())
                                });
                            }
                            if response.double_clicked() {
                                action = Some(Action::Toggle(path.clone()));
                            }
                            response.context_menu(|ui| {
                                crate::ui_icons::menu_style(ui.style_mut());
                                ui.set_width(240.);
                                if crate::ui_icons::menu_item(ui, "New folder here", "").clicked() {
                                    action = Some(Action::NewFolder(path.clone()));
                                    ui.close();
                                }
                                if crate::ui_icons::menu_item(ui, "Rename / move folder…", "")
                                    .clicked()
                                {
                                    action = Some(Action::EditFolder(path.clone()));
                                    ui.close();
                                }
                            });
                        }
                        LibraryRow::Symbol { symbol, depth } => {
                            let response = library_row(
                                ui,
                                &symbol.name,
                                &symbol.linkage,
                                depth + 1,
                                false,
                                self.library_selected.as_ref() == Some(&symbol.id),
                                false,
                            )
                            .on_hover_text(format!(
                                "{} · {} frames\n{}",
                                kind_label(symbol.kind),
                                symbol.frames,
                                if symbol.folder.is_empty() {
                                    "Library root"
                                } else {
                                    &symbol.folder
                                }
                            ));
                            capture_row_drag(&mut self.library_panel, ui, &response, || {
                                LibraryDrag::Symbol(symbol.id.clone())
                            });
                            if response.clicked() {
                                action = Some(Action::SelectSymbol(
                                    symbol.id.clone(),
                                    symbol.folder.clone(),
                                ));
                            }
                            if response.double_clicked() {
                                action = Some(Action::EditSymbol(symbol.id.clone()));
                            }
                            response.context_menu(|ui| {
                                crate::ui_icons::menu_style(ui.style_mut());
                                ui.set_width(240.);
                                if crate::ui_icons::menu_item(ui, "Edit symbol", "").clicked() {
                                    action = Some(Action::EditSymbol(symbol.id.clone()));
                                    ui.close();
                                }
                                if crate::ui_icons::menu_item(ui, "Symbol properties…", "")
                                    .clicked()
                                {
                                    action = Some(Action::RenameSymbol(symbol.id.clone()));
                                    ui.close();
                                }
                            });
                        }
                    }
                }
                if symbols.is_empty() && folders.is_empty() {
                    ui.weak("Convert artwork to a symbol with F8.");
                } else if !query.is_empty()
                    && !symbols.iter().any(|symbol| symbol_matches(symbol, &query))
                    && !folders
                        .iter()
                        .any(|folder| folder.to_lowercase().contains(&query))
                {
                    ui.weak("No matching library items.");
                }
                self.resource_library(ui);
            });
        if let Some(action) = action {
            match action {
                Action::SelectFolder(folder) => {
                    self.library_panel.selected_folder = folder;
                    self.library_selected = None;
                }
                Action::Toggle(folder) => {
                    if !self.library_panel.collapsed.remove(&folder) {
                        self.library_panel.collapsed.insert(folder);
                    }
                }
                Action::SelectSymbol(id, folder) => {
                    self.library_selected = Some(id);
                    self.library_panel.selected_folder = folder;
                }
                Action::EditSymbol(id) => self.enter_symbol(id),
                Action::RenameSymbol(id) => self.start_library_edit(EditTarget::Symbol(id), ""),
                Action::EditFolder(folder) => {
                    self.start_library_edit(EditTarget::Folder(folder), "")
                }
                Action::NewFolder(parent) => {
                    self.start_library_edit(EditTarget::NewFolder, &parent)
                }
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.set_min_height(crate::theme::FOOTER_HEIGHT);
            ui.spacing_mut().item_spacing.x = 4.;
            if ui
                .add(crate::ui_icons::IconButton::new(Icon::Symbol, "New symbol"))
                .clicked()
            {
                self.begin_new_symbol();
            }
            if ui
                .add(crate::ui_icons::IconButton::new(Icon::Plus, "New folder"))
                .clicked()
            {
                self.start_library_edit(
                    EditTarget::NewFolder,
                    &self.library_panel.selected_folder.clone(),
                );
            }
            if ui
                .button("Import…")
                .on_hover_text("Import SVG, PNG or JPEG to Stage")
                .clicked()
            {
                self.import_graphics_dialog();
            }
            ui.menu_button("More", |ui| {
                crate::ui_icons::menu_contents(ui, 240., |ui| {
                    if let Some(symbol) = &selected {
                        if crate::ui_icons::menu_item(ui, "Place instance", "").clicked() {
                            if let Err(error) =
                                self.place_library_symbol(ui.ctx(), &symbol.id, None)
                            {
                                self.message(error);
                            }
                            ui.close();
                        }
                        if crate::ui_icons::menu_item(ui, "Edit symbol", "").clicked() {
                            self.enter_symbol(symbol.id.clone());
                            ui.close();
                        }
                        if crate::ui_icons::menu_item(ui, "Symbol properties…", "").clicked() {
                            self.start_library_edit(EditTarget::Symbol(symbol.id.clone()), "");
                            ui.close();
                        }
                    }
                    if !self.library_panel.selected_folder.is_empty()
                        && crate::ui_icons::menu_item(ui, "Folder properties…", "").clicked()
                    {
                        self.start_library_edit(
                            EditTarget::Folder(self.library_panel.selected_folder.clone()),
                            "",
                        );
                        ui.close();
                    }
                    if crate::ui_icons::menu_item_enabled(
                        ui,
                        self.can_convert_selection(),
                        "Convert to Symbol",
                        "F8",
                    )
                    .clicked()
                    {
                        self.convert_to_symbol();
                        ui.close();
                    }
                })
            })
            .response
            .on_hover_text(format!(
                "{} symbols · {} folders",
                symbols.len(),
                folders.len()
            ));
        });
    }

    fn start_library_edit(&mut self, target: EditTarget, parent: &str) {
        let kind = match &target {
            EditTarget::Symbol(id) => self.project.symbol(id).map(|symbol| symbol.kind),
            _ => None,
        };
        let (name, parent) = match &target {
            EditTarget::NewFolder => {
                let paths = self.project.library_folder_paths();
                let mut name = "New folder".to_owned();
                let mut suffix = 2;
                while paths.contains(&join_folder(parent, &name)) {
                    name = format!("New folder {suffix}");
                    suffix += 1;
                }
                (name, parent.to_owned())
            }
            EditTarget::Folder(path) => (basename(path).to_owned(), parent_folder(path).to_owned()),
            EditTarget::Symbol(id) => match self.project.symbol(id) {
                Some(symbol) => (symbol.name.clone(), symbol.folder.clone()),
                None => return,
            },
        };
        self.library_panel.editing = Some(LibraryEdit {
            target,
            name,
            parent,
            kind,
            focus: true,
            error: None,
        });
    }

    fn library_edit_form(&mut self, ui: &mut egui::Ui, folders: &[String]) {
        let Some(mut edit) = self.library_panel.editing.take() else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        ui.group(|ui| {
            ui.strong(match edit.target {
                EditTarget::NewFolder => "New folder",
                EditTarget::Folder(_) => "Organize folder",
                EditTarget::Symbol(_) => "Symbol properties",
            });
            let response = crate::paint_controls::text_row(ui, "Name", &mut edit.name);
            if std::mem::take(&mut edit.focus) {
                response.request_focus();
            }
            crate::paint_controls::property_row(ui, "Folder", |ui| {
                let excluded = match &edit.target {
                    EditTarget::Folder(path) => Some(path.as_str()),
                    _ => None,
                };
                folder_picker(
                    ui,
                    "library-edit-parent",
                    &mut edit.parent,
                    folders,
                    excluded,
                );
            });
            if let Some(kind) = &mut edit.kind {
                crate::paint_controls::property_row(ui, "Type", |ui| {
                    egui::ComboBox::from_id_salt("library-symbol-kind")
                        .width(ui.available_width().min(130.))
                        .truncate()
                        .selected_text(kind_label(*kind))
                        .show_ui(ui, |ui| {
                            for option in [
                                SymbolKind::MovieClip,
                                SymbolKind::Graphic,
                                SymbolKind::Button,
                            ] {
                                ui.selectable_value(kind, option, kind_label(option));
                            }
                        });
                });
                ui.small("Changes this library symbol and all its instances.");
                if *kind == SymbolKind::Button {
                    ui.small("Frames 1–4: Up, Over, Down, Hit. Later frames are preserved.");
                }
                if let EditTarget::Symbol(id) = &edit.target
                    && let Some(symbol) = self.project.symbol(id)
                {
                    for (label, value) in [
                        ("Linkage", &symbol.linkage_id),
                        ("Class", &symbol.class_name),
                    ] {
                        if let Some(value) = value {
                            crate::paint_controls::property_row(ui, label, |ui| {
                                ui.add(egui::Label::new(value).truncate())
                                    .on_hover_text(value);
                            });
                        }
                    }
                }
            }
            if let Some(error) = &edit.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                accept = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                accept = true;
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });
        if cancel {
            return;
        }
        if accept {
            let name = edit.name.trim();
            let mut candidate = self.project.clone();
            let destination = join_folder(&edit.parent, name);
            let result = if name.is_empty()
                || name.chars().any(char::is_control)
                || (!matches!(edit.target, EditTarget::Symbol(_))
                    && (name.contains('/') || name.contains('\\')))
            {
                Err("Enter a name without separators or control characters.".to_owned())
            } else {
                match &edit.target {
                    EditTarget::NewFolder
                        if candidate.library_folder_paths().contains(&destination) =>
                    {
                        Err("That folder already exists.".into())
                    }
                    EditTarget::NewFolder => candidate.add_library_folder(&destination),
                    EditTarget::Folder(from) => {
                        candidate.relocate_library_folder(from, &destination)
                    }
                    EditTarget::Symbol(id) => {
                        candidate.move_library_symbol(id, &edit.parent).and_then(|()| {
                            let symbol = candidate.symbol_mut(id).unwrap();
                            let before = symbol.kind;
                            let kind = edit.kind.unwrap_or(before);
                            if kind != before && symbol.class_name.is_some() {
                                return Err("Type changes for class-linked symbols are not supported yet. The linked class and scripts have been preserved.".into());
                            }
                            if kind == SymbolKind::Graphic && symbol.linkage_id.is_some() {
                                return Err("Graphic symbols cannot keep runtime export linkage. Keep Movie Clip or Button; the export identifier has been preserved.".into());
                            }
                            symbol.name = name.to_owned();
                            symbol.kind = kind;
                            if kind == SymbolKind::Button {
                                symbol.timeline.frame_count = symbol.timeline.frame_count.max(4);
                            }
                            if kind != before {
                                for owner in &mut candidate.symbols {
                                    for element in owner.timeline.layers.iter_mut().flat_map(|layer| &mut layer.keyframes).flat_map(|key| &mut key.elements) {
                                        if let ElementKind::Instance { symbol_id, playback } = &mut element.kind
                                            && symbol_id.as_str() == id.as_str()
                                        {
                                            *playback = match kind {
                                                SymbolKind::MovieClip => InstancePlayback::Independent,
                                                SymbolKind::Button => InstancePlayback::SingleFrame(0),
                                                SymbolKind::Graphic => match *playback {
                                                    InstancePlayback::Independent => InstancePlayback::Loop,
                                                    other => other,
                                                },
                                            };
                                        }
                                    }
                                }
                            }
                            Ok(())
                        })
                    }
                }
                .and_then(|()| candidate.validate().map_err(|error| error.to_string()))
            };
            match result {
                Ok(()) => {
                    self.transact(|editor| editor.project = candidate);
                    self.library_panel.selected_folder =
                        if matches!(edit.target, EditTarget::Symbol(_)) {
                            edit.parent
                        } else {
                            destination
                        };
                    let selected = self.library_panel.selected_folder.clone();
                    self.library_panel.collapsed.retain(|folder| {
                        folder != &selected && !selected.starts_with(&format!("{folder}/"))
                    });
                    return;
                }
                Err(error) => edit.error = Some(error),
            }
        }
        self.library_panel.editing = Some(edit);
    }
}

pub(crate) fn folder_picker(
    ui: &mut egui::Ui,
    salt: &str,
    value: &mut String,
    folders: &[String],
    excluded: Option<&str>,
) {
    egui::ComboBox::from_id_salt(salt)
        .truncate()
        .selected_text(if value.is_empty() {
            "Library root"
        } else {
            value.as_str()
        })
        .width(ui.available_width().min(270.))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), "Library root");
            for folder in folders {
                if excluded
                    .is_some_and(|path| folder == path || folder.starts_with(&format!("{path}/")))
                {
                    continue;
                }
                ui.selectable_value(value, folder.clone(), folder);
            }
        });
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn parent_folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn join_folder(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}
fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::MovieClip => "Movie Clip",
        SymbolKind::Graphic => "Graphic",
        SymbolKind::Button => "Button",
    }
}
fn symbol_matches(symbol: &LibrarySymbol, query: &str) -> bool {
    [&symbol.name, &symbol.folder, &symbol.linkage, &symbol.class]
        .iter()
        .any(|text| text.to_lowercase().contains(query))
}
fn build_rows(
    parent: &str,
    depth: usize,
    folders: &[String],
    symbols: &[LibrarySymbol],
    state: &LibraryPanel,
    query: &str,
    rows: &mut Vec<LibraryRow>,
) {
    let mut child_folders: Vec<_> = folders
        .iter()
        .filter(|path| parent_folder(path) == parent)
        .collect();
    child_folders.sort_by_key(|path| path.to_lowercase());
    if state.descending {
        child_folders.reverse();
    }
    for folder in child_folders {
        let prefix = format!("{folder}/");
        let matches = query.is_empty()
            || folder.to_lowercase().contains(query)
            || folders
                .iter()
                .any(|child| child.starts_with(&prefix) && child.to_lowercase().contains(query))
            || symbols.iter().any(|symbol| {
                (symbol.folder == *folder || symbol.folder.starts_with(&prefix))
                    && symbol_matches(symbol, query)
            });
        if !matches {
            continue;
        }
        rows.push(LibraryRow::Folder {
            path: folder.clone(),
            depth,
        });
        if !query.is_empty() || !state.collapsed.contains(folder) {
            build_rows(folder, depth + 1, folders, symbols, state, query, rows);
        }
    }
    let mut items: Vec<_> = symbols
        .iter()
        .filter(|symbol| {
            symbol.folder == parent && (query.is_empty() || symbol_matches(symbol, query))
        })
        .collect();
    items.sort_by_key(|symbol| {
        (
            match state.sort {
                LibrarySort::Name => symbol.name.to_lowercase(),
                LibrarySort::Type => kind_label(symbol.kind).to_owned(),
                LibrarySort::Linkage => symbol.linkage.to_lowercase(),
            },
            symbol.name.to_lowercase(),
            symbol.id.clone(),
        )
    });
    if state.descending {
        items.reverse();
    }
    rows.extend(items.into_iter().map(|symbol| LibraryRow::Symbol {
        symbol: symbol.clone(),
        depth,
    }));
}
fn preview_metadata(symbol: &LibrarySymbol) -> String {
    let mut text = format!(
        "{} · {} frame{}",
        kind_label(symbol.kind),
        symbol.frames,
        if symbol.frames == 1 { "" } else { "s" }
    );
    if !symbol.linkage.is_empty() {
        text.push_str(&format!(" · Linkage: {}", symbol.linkage));
    }
    if !symbol.class.is_empty() {
        text.push_str(&format!(" · Class: {}", symbol.class));
    }
    text
}

fn library_row(
    ui: &mut egui::Ui,
    name: &str,
    linkage: &str,
    depth: usize,
    folder: bool,
    selected: bool,
    expanded: bool,
) -> egui::Response {
    let height = crate::theme::ROW_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        if folder {
            Sense::click()
        } else {
            Sense::click_and_drag()
        },
    );
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    if selected || response.hovered() {
        painter.rect_filled(rect, 3., visuals.weak_bg_fill);
    }
    if selected {
        painter.line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(2.0_f32, visuals.text_color()),
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect.shrink(1.),
            3.,
            Stroke::new(2.0_f32, crate::theme::UiPalette::current(ui.ctx()).focus),
            egui::StrokeKind::Inside,
        );
    }
    let x = rect.left() + depth as f32 * 12. + 3.;
    let center = Pos2::new(x + 19., rect.center().y);
    if folder {
        let points = if expanded {
            vec![
                Pos2::new(x, center.y - 2.),
                Pos2::new(x + 7., center.y - 2.),
                Pos2::new(x + 3.5, center.y + 3.),
            ]
        } else {
            vec![
                Pos2::new(x + 1., center.y - 4.),
                Pos2::new(x + 1., center.y + 4.),
                Pos2::new(x + 6., center.y),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            visuals.text_color(),
            Stroke::NONE,
        ));
        let icon = Rect::from_center_size(center, Vec2::new(13., 10.));
        painter.rect_stroke(
            icon,
            1.,
            Stroke::new(1.0_f32, visuals.text_color()),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                icon.left_top() + Vec2::new(0., -2.),
                icon.left_top() + Vec2::new(5., -2.),
            ],
            Stroke::new(1.0_f32, visuals.text_color()),
        );
    } else {
        paint_icon(
            &painter,
            Rect::from_center_size(center, Vec2::splat(15.)),
            Icon::Symbol,
            Stroke::new(1.0_f32, visuals.text_color()),
        );
    }
    let show_linkage = !linkage.is_empty() && rect.width() - depth as f32 * 12. >= 220.;
    let name_right = if !show_linkage {
        rect.right()
    } else {
        rect.right() - (rect.width() * 0.3).min(100.)
    };
    let name_painter = painter.with_clip_rect(Rect::from_min_max(
        rect.min,
        Pos2::new(name_right - 4., rect.bottom()),
    ));
    name_painter.text(
        Pos2::new(x + 31., rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(crate::theme::PANEL_FONT),
        visuals.text_color(),
    );
    if show_linkage {
        let linkage_painter = painter.with_clip_rect(Rect::from_min_max(
            Pos2::new(name_right, rect.top()),
            rect.max,
        ));
        linkage_painter.text(
            Pos2::new(name_right, rect.center().y),
            Align2::LEFT_CENTER,
            linkage,
            FontId::proportional(crate::theme::SECONDARY_FONT),
            visuals.text_color(),
        );
    }
    let response = response.on_hover_text(if linkage.is_empty() {
        name.to_owned()
    } else {
        format!("{name}\nLinkage: {linkage}")
    });
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    response
}
