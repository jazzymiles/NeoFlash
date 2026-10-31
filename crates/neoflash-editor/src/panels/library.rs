//! Authoring library: portable folders live in the document; view preferences do not.
//!
//! NF2 Library (`panels-more.png`): preview of the selected item, `NF2/Search`, item count with the
//! ⋯ menu (sort, item commands), `NF2/Tree Item` rows with type icons and linkage, `NF2/Empty State`
//! and icon-button footer.
use crate::theme::ScrollTheme as _;
use crate::{app::Editor, workspace::PanelId};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::{ElementKind, InstancePlayback, SymbolKind};
use neoflash_ui::components::{self as nf, TreeItemType};
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
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
        ui.spacing_mut().item_spacing = Vec2::splat(space::S2);
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
        let mut overflow = None;
        if let Some(symbol) = selected {
            let spacing = ui.spacing().item_spacing.y;
            let metadata = preview_metadata(&symbol);
            let text_height = |text: &str, style: TextStyle| {
                ui.painter()
                    .layout_no_wrap(
                        text.to_owned(),
                        neoflash_ui::typography::font(style),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .y
                    .max(16.)
            };
            let name_height = text_height(&symbol.name, TextStyle::BodyStrong);
            let metadata_height = text_height(&metadata, TextStyle::Small);
            // Reserve search, the item-count row, three list rows and the footer.
            let reserved = metrics::CONTROL * 2.
                + metrics::ROW * 3.
                + crate::theme::FOOTER_HEIGHT
                + 1.
                + 7. * spacing;
            let image_budget = height
                - reserved
                - metrics::SECTION_HEADER
                - name_height
                - metadata_height
                - 4. * spacing;
            if image_budget >= 64. {
                let open = *self
                    .library_panel
                    .preview_open
                    .get_or_insert(height >= 330.);
                let shown = nf::SectionHeader::new("Preview")
                    .id_salt("library-preview")
                    .default_open(open)
                    .show(ui, |ui| {
                        self.library_preview(ui, &symbol, &metadata, image_budget.min(160.))
                    });
                self.library_panel.preview_open = Some(shown.is_some());
            } else {
                overflow = Some((symbol, metadata));
            }
        }
        let search = ui.add(
            nf::Search::new(&mut self.library_panel.search, "Search library").width(content_width),
        );
        nf::tooltip(
            search,
            "Search symbol names, folders, linkage identifiers and classes",
            None,
        );
        self.library_browser(ui, overflow);
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
        let p = neoflash_ui::palette(ui.ctx());
        ui.add_sized(
            [width, 16.],
            egui::Label::new(rich(&symbol.name, TextStyle::BodyStrong).color(p.text_primary))
                .truncate()
                .halign(egui::Align::Min),
        )
        .on_hover_text(&symbol.name);
        ui.add_sized(
            [width, 16.],
            egui::Label::new(rich(metadata, TextStyle::Small).color(p.text_tertiary))
                .truncate()
                .halign(egui::Align::Min),
        )
        .on_hover_text(metadata);
    }

    fn library_browser(&mut self, ui: &mut egui::Ui, overflow: Option<(LibrarySymbol, String)>) {
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
        let query = self.library_panel.search.trim().to_lowercase();
        let matching = if query.is_empty() {
            symbols.len()
        } else {
            symbols
                .iter()
                .filter(|symbol| symbol_matches(symbol, &query))
                .count()
        };
        self.library_count_row(
            ui,
            &symbols,
            folders.len(),
            matching,
            &query,
            &selected,
            overflow,
        );
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
        let footer = crate::theme::FOOTER_HEIGHT + 1. + 2. * ui.spacing().item_spacing.y;
        let tree_height = (ui.available_height() - footer).max(0.);
        egui::ScrollArea::vertical()
            .id_salt("library-tree")
            .max_width(ui.available_width())
            .auto_shrink([false, false])
            .max_height(tree_height)
            .min_scrolled_height(tree_height.min(metrics::ROW))
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                let editing = self.library_panel.editing.is_some();
                self.library_edit_form(ui, &folders);
                if symbols.is_empty() && folders.is_empty() && query.is_empty() && !editing {
                    let import = nf::EmptyState {
                        icon: i20::SYMBOL,
                        title: "Library is empty",
                        body: "Convert artwork to a symbol (F8) or import files to see them here.",
                        action: Some("Import to library…"),
                    }
                    .show(ui);
                    if import {
                        self.import_graphics_dialog();
                    }
                    self.resource_library(ui);
                    return;
                }
                let root = tree_row(
                    ui,
                    "Library root",
                    TreeItemType::FolderOpen,
                    self.library_selected.is_none()
                        && self.library_panel.selected_folder.is_empty(),
                    0,
                    "",
                );
                if root.row.clicked() {
                    action = Some(Action::SelectFolder(String::new()));
                }
                nf::context_menu(&root.row, |ui| {
                    if ui.add(nf::MenuItem::new("New folder")).clicked() {
                        action = Some(Action::NewFolder(String::new()));
                    }
                });
                for row in rows {
                    match row {
                        LibraryRow::Folder { path, depth } => {
                            let expanded =
                                !query.is_empty() || !self.library_panel.collapsed.contains(&path);
                            let response = tree_row(
                                ui,
                                basename(&path),
                                if expanded {
                                    TreeItemType::FolderOpen
                                } else {
                                    TreeItemType::FolderClosed
                                },
                                self.library_selected.is_none()
                                    && self.library_panel.selected_folder == path,
                                depth + 1,
                                "",
                            );
                            if response.toggle || response.row.double_clicked() {
                                action = Some(Action::Toggle(path.clone()));
                            } else if response.row.clicked() {
                                action = Some(Action::SelectFolder(path.clone()));
                            }
                            nf::tooltip(response.row.clone(), &path, None);
                            nf::context_menu(&response.row, |ui| {
                                if ui.add(nf::MenuItem::new("New folder here")).clicked() {
                                    action = Some(Action::NewFolder(path.clone()));
                                }
                                if ui.add(nf::MenuItem::new("Rename / move folder…")).clicked() {
                                    action = Some(Action::EditFolder(path.clone()));
                                }
                            });
                        }
                        LibraryRow::Symbol { symbol, depth } => {
                            let response = tree_row(
                                ui,
                                &symbol.name,
                                TreeItemType::Symbol(kind_icon(symbol.kind)),
                                self.library_selected.as_ref() == Some(&symbol.id),
                                depth + 1,
                                &symbol.linkage,
                            )
                            .row;
                            let place = if symbol.folder.is_empty() {
                                "Library root"
                            } else {
                                &symbol.folder
                            };
                            let mut hint = format!(
                                "{} · {} frames · {place}",
                                kind_label(symbol.kind),
                                symbol.frames
                            );
                            if !symbol.linkage.is_empty() {
                                hint.push_str(&format!(" · Linkage: {}", symbol.linkage));
                            }
                            let response = nf::tooltip(response, &hint, None);
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
                            nf::context_menu(&response, |ui| {
                                if ui.add(nf::MenuItem::new("Edit symbol")).clicked() {
                                    action = Some(Action::EditSymbol(symbol.id.clone()));
                                }
                                if ui.add(nf::MenuItem::new("Symbol properties…")).clicked() {
                                    action = Some(Action::RenameSymbol(symbol.id.clone()));
                                }
                            });
                        }
                    }
                }
                if !query.is_empty()
                    && matching == 0
                    && !folders
                        .iter()
                        .any(|folder| folder.to_lowercase().contains(&query))
                {
                    nf::EmptyState {
                        icon: i20::SEARCH,
                        title: "No matches",
                        body: "No library items match this search.",
                        action: None,
                    }
                    .show(ui);
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
        nf::divider(ui, false);
        self.library_footer(ui, &selected);
    }

    /// "N items", the overflow preview and the ⋯ menu (sort order and item commands).
    #[allow(clippy::too_many_arguments)]
    fn library_count_row(
        &mut self,
        ui: &mut egui::Ui,
        symbols: &[LibrarySymbol],
        folders: usize,
        matching: usize,
        query: &str,
        selected: &Option<LibrarySymbol>,
        overflow: Option<(LibrarySymbol, String)>,
    ) {
        let p = neoflash_ui::palette(ui.ctx());
        let total = symbols.len() + self.project.resources.len();
        let count = if query.is_empty() {
            format!("{total} item{}", if total == 1 { "" } else { "s" })
        } else {
            format!("{matching} of {} symbols", symbols.len())
        };
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(width, metrics::CONTROL),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                let label = ui.label(rich(&count, TextStyle::Small).color(p.text_secondary));
                nf::tooltip(
                    label,
                    &format!(
                        "{} symbols · {folders} folders · {} resource files",
                        symbols.len(),
                        self.project.resources.len()
                    ),
                    None,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let menu = ui
                        .push_id("library-options", |ui| {
                            ui.add(nf::IconButton::new(i16::MORE, "Library options"))
                        })
                        .inner;
                    nf::popup_below(&menu, 240., |ui| self.library_menu(ui, selected));
                    if let Some((symbol, metadata)) = overflow {
                        self.library_preview_overflow(ui, &symbol, &metadata, width);
                    }
                });
            },
        );
    }

    fn library_menu(&mut self, ui: &mut egui::Ui, selected: &Option<LibrarySymbol>) {
        nf::menu_section(ui, "Sort by");
        for sort in [LibrarySort::Name, LibrarySort::Type, LibrarySort::Linkage] {
            if ui
                .add(nf::MenuItem::new(sort.label()).checked(self.library_panel.sort == sort))
                .clicked()
            {
                self.library_panel.sort = sort;
            }
        }
        nf::menu_separator(ui);
        for (descending, label) in [(false, "Ascending (A–Z)"), (true, "Descending (Z–A)")] {
            if ui
                .add(nf::MenuItem::new(label).checked(self.library_panel.descending == descending))
                .clicked()
            {
                self.library_panel.descending = descending;
            }
        }
        nf::menu_separator(ui);
        if let Some(symbol) = selected {
            if ui.add(nf::MenuItem::new("Place instance")).clicked()
                && let Err(error) = self.place_library_symbol(ui.ctx(), &symbol.id, None)
            {
                self.message(error);
            }
            if ui.add(nf::MenuItem::new("Edit symbol")).clicked() {
                self.enter_symbol(symbol.id.clone());
            }
            if ui.add(nf::MenuItem::new("Symbol properties…")).clicked() {
                self.start_library_edit(EditTarget::Symbol(symbol.id.clone()), "");
            }
        }
        if !self.library_panel.selected_folder.is_empty()
            && ui.add(nf::MenuItem::new("Folder properties…")).clicked()
        {
            self.start_library_edit(
                EditTarget::Folder(self.library_panel.selected_folder.clone()),
                "",
            );
        }
        if ui
            .add_enabled(
                self.can_convert_selection(),
                nf::MenuItem::new("Convert to Symbol").shortcut("F8"),
            )
            .clicked()
        {
            self.convert_to_symbol();
        }
        // Sound and font library items (§9) open their planned dialogs.
        nf::menu_separator(ui);
        for path in [
            "Library menu ▸ Sound properties…",
            "Library menu ▸ Edit envelope…",
            "Library menu ▸ Font embedding…",
        ] {
            let label = path.rsplit(" ▸ ").next().unwrap_or(path);
            if ui.add(nf::MenuItem::new(label)).clicked() {
                Self::open_planned(ui.ctx(), path);
                ui.close();
            }
        }
    }

    /// Short panels: the preview opens in a popover instead of squeezing the list.
    fn library_preview_overflow(
        &mut self,
        ui: &mut egui::Ui,
        symbol: &LibrarySymbol,
        metadata: &str,
        width: f32,
    ) {
        // Stable id: the popup's open state is keyed by the trigger.
        let trigger = ui
            .push_id("library-preview-trigger", |ui| {
                ui.add(nf::IconButton::new(i16::EYE, "Open symbol preview"))
            })
            .inner;
        let popup = egui::Popup::from_toggle_button_response(&trigger)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(nf::Surface::Popover.frame(ui.ctx()))
            .width(width.clamp(248., 360.));
        let popup_id = popup.get_id();
        let was_open = popup.is_open();
        popup.show(|ui| self.library_preview(ui, symbol, metadata, 160.));
        if was_open && !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            trigger.request_focus();
        }
    }

    /// Footer: new symbol, new folder, properties (icon buttons) and Import….
    fn library_footer(&mut self, ui: &mut egui::Ui, selected: &Option<LibrarySymbol>) {
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(width, crate::theme::FOOTER_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                if ui
                    .add(nf::IconButton::new(i16::PLUS, "New symbol"))
                    .clicked()
                {
                    self.begin_new_symbol();
                }
                if ui
                    .add(nf::IconButton::new(i16::FOLDERPLUS, "New folder"))
                    .clicked()
                {
                    self.start_library_edit(
                        EditTarget::NewFolder,
                        &self.library_panel.selected_folder.clone(),
                    );
                }
                let target = match selected {
                    Some(symbol) => Some(EditTarget::Symbol(symbol.id.clone())),
                    None if !self.library_panel.selected_folder.is_empty() => Some(
                        EditTarget::Folder(self.library_panel.selected_folder.clone()),
                    ),
                    None => None,
                };
                let properties = ui.add_enabled(
                    target.is_some(),
                    nf::IconButton::new(i16::INFO, "Item properties"),
                );
                if properties.clicked()
                    && let Some(target) = target
                {
                    self.start_library_edit(target, "");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let import = ui.add(nf::Button::ghost("Import…"));
                    if nf::tooltip(import, "Import SVG, PNG or JPEG to Stage", None).clicked() {
                        self.import_graphics_dialog();
                    }
                });
            },
        );
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
        let p = neoflash_ui::palette(ui.ctx());
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            nf::menu_section(
                ui,
                match edit.target {
                    EditTarget::NewFolder => "New folder",
                    EditTarget::Folder(_) => "Organize folder",
                    EditTarget::Symbol(_) => "Symbol properties",
                },
            );
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
            let note = |ui: &mut egui::Ui, text: &str| {
                ui.label(rich(text, TextStyle::Small).color(p.text_tertiary));
            };
            if let Some(kind) = &mut edit.kind {
                crate::paint_controls::property_row(ui, "Type", |ui| {
                    let width = ui.available_width().min(130.);
                    nf::Select::options(
                        ui,
                        "library-symbol-kind",
                        kind,
                        &[
                            (SymbolKind::MovieClip, kind_label(SymbolKind::MovieClip)),
                            (SymbolKind::Graphic, kind_label(SymbolKind::Graphic)),
                            (SymbolKind::Button, kind_label(SymbolKind::Button)),
                        ],
                        width,
                    );
                });
                note(ui, "Changes this library symbol and all its instances.");
                if *kind == SymbolKind::Button {
                    note(
                        ui,
                        "Frames 1–4: Up, Over, Down, Hit. Later frames are preserved.",
                    );
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
                                ui.add(
                                    egui::Label::new(
                                        rich(value, TextStyle::Body).color(p.text_primary),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(value);
                            });
                        }
                    }
                }
            }
            if let Some(error) = &edit.error {
                ui.label(rich(error, TextStyle::Small).color(p.danger_default));
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                accept = ui.add(nf::Button::primary("Save")).clicked();
                cancel = ui.add(nf::Button::new("Cancel")).clicked();
            });
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                accept = true;
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            nf::divider(ui, false);
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
    let label = if value.is_empty() {
        "Library root".to_owned()
    } else {
        value.clone()
    };
    let width = ui.available_width().min(270.);
    nf::Select::new(salt, &label)
        .width(width)
        .icon(i16::FOLDER)
        .show(ui, |ui| {
            if ui
                .add(nf::MenuItem::new("Library root").checked(value.is_empty()))
                .clicked()
            {
                value.clear();
            }
            for folder in folders {
                if excluded
                    .is_some_and(|path| folder == path || folder.starts_with(&format!("{path}/")))
                {
                    continue;
                }
                if ui
                    .add(nf::MenuItem::new(folder).checked(value == folder))
                    .clicked()
                {
                    *value = folder.clone();
                }
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
/// Type icon of a symbol row (`NF2/Tree Item`, Type=Symbol).
fn kind_icon(kind: SymbolKind) -> neoflash_ui::Icon {
    match kind {
        SymbolKind::MovieClip => i16::MOVIECLIP,
        SymbolKind::Graphic => i16::GRAPHIC,
        SymbolKind::Button => i16::BUTTON_SYMBOL,
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

/// `NF2/Tree Item`; the linkage column appears only where the name keeps room.
pub(crate) fn tree_row(
    ui: &mut egui::Ui,
    name: &str,
    kind: TreeItemType,
    selected: bool,
    depth: usize,
    meta: &str,
) -> nf::tree::TreeItemResponse {
    let room = ui.available_width() - depth as f32 * metrics::LAYER_INDENT;
    let meta = if room >= 220. { meta } else { "" };
    nf::TreeItem::new(name, kind)
        .selected(selected)
        .depth(depth)
        .meta(meta)
        .show(ui)
}
