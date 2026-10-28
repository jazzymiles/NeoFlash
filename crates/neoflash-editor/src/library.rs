//! Authoring library: portable folders live in the document; view preferences do not.
use crate::{
    app::Editor,
    ui_icons::{Icon, paint_icon},
    workspace::PanelId,
};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::SymbolKind;
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct LibraryPanel {
    pub selected_folder: String,
    search: String,
    sort: LibrarySort,
    descending: bool,
    collapsed: HashSet<String>,
    editing: Option<LibraryEdit>,
    pub(crate) drag: Option<LibraryDrag>,
}

#[derive(Clone)]
pub(crate) enum LibraryDrag {
    Symbol(String),
    Resource(String),
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
        ui.add_space(4.);
        self.panel_title(ui, PanelId::Library, "Library");
        ui.separator();
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
        egui::CollapsingHeader::new("Preview")
            .id_salt("library-preview")
            .default_open(height >= 330.)
            .show(ui, |ui| {
                if let Some(symbol) = selected {
                    self.paint_library_preview(ui, &symbol.id);
                    ui.add(egui::Label::new(egui::RichText::new(&symbol.name).strong()).truncate())
                        .on_hover_text(&symbol.name);
                    ui.small(format!(
                        "{} · {} frame{}",
                        kind_label(symbol.kind),
                        symbol.frames,
                        if symbol.frames == 1 { "" } else { "s" }
                    ));
                    if !symbol.linkage.is_empty() {
                        ui.add(egui::Label::new(format!("Linkage: {}", symbol.linkage)).truncate())
                            .on_hover_text(&symbol.linkage);
                    }
                    if !symbol.class.is_empty() {
                        ui.add(egui::Label::new(format!("Class: {}", symbol.class)).truncate())
                            .on_hover_text(&symbol.class);
                    }
                } else {
                    let (rect, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 72.), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 2., ui.visuals().extreme_bg_color);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "Select a symbol",
                        FontId::proportional(12.),
                        ui.visuals().weak_text_color(),
                    );
                }
            });
        ui.separator();
        // Preview stays anchored. Only the controls/tree below it can overflow.
        let body_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("library-body")
            .auto_shrink([false, false])
            .show(ui, |ui| self.library_browser(ui, body_height));
    }

    fn library_browser(&mut self, ui: &mut egui::Ui, body_height: f32) {
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
        ui.add(
            egui::TextEdit::singleline(&mut self.library_panel.search)
                .hint_text("Search symbols, folders, linkage")
                .desired_width(ui.available_width()),
        );
        if ui
            .add_sized(
                [ui.available_width(), 24.],
                egui::Button::new("Import graphics…"),
            )
            .on_hover_text("Place SVG, PNG or JPEG on the Stage")
            .clicked()
        {
            self.import_graphics_dialog();
        }
        let mut new_folder = false;
        ui.horizontal(|ui| {
            new_folder = ui
                .add_sized([78., 24.], egui::Button::new("New folder"))
                .clicked();
            egui::ComboBox::from_id_salt("library-sort")
                .width((ui.available_width() - 46.).max(50.))
                .selected_text(self.library_panel.sort.label())
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
        if new_folder {
            self.start_library_edit(
                EditTarget::NewFolder,
                &self.library_panel.selected_folder.clone(),
            );
        }
        self.library_edit_form(ui, &folders);
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
        let tree_height = (body_height - 220.).max(50.);
        egui::ScrollArea::vertical()
            .id_salt("library-tree")
            .auto_shrink([false, false])
            .max_height(tree_height)
            .show(ui, |ui| {
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
                    if ui.button("New folder").clicked() {
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
                                if ui.button("New folder here").clicked() {
                                    action = Some(Action::NewFolder(path.clone()));
                                    ui.close();
                                }
                                if ui.button("Rename / move folder…").clicked() {
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
                            if response.drag_started() {
                                self.library_panel.drag =
                                    Some(LibraryDrag::Symbol(symbol.id.clone()));
                                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                            }
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
                                if ui.button("Edit symbol").clicked() {
                                    action = Some(Action::EditSymbol(symbol.id.clone()));
                                    ui.close();
                                }
                                if ui.button("Rename / move symbol…").clicked() {
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
        ui.vertical(|ui| {
            ui.small(format!(
                "{} symbols · {} folders",
                symbols.len(),
                folders.len()
            ));
            if let Some(symbol) = selected {
                ui.horizontal(|ui| {
                    let width = ((ui.available_width() - 12.) / 3.).max(40.);
                    if ui
                        .add_sized([width, 24.], egui::Button::new("Place"))
                        .on_hover_text("Place instance on Stage")
                        .clicked()
                        && let Err(error) = self.place_library_symbol(ui.ctx(), &symbol.id, None)
                    {
                        self.message(error);
                    }
                    if ui
                        .add_sized([width, 24.], egui::Button::new("Edit"))
                        .clicked()
                    {
                        self.enter_symbol(symbol.id.clone());
                    }
                    if ui
                        .add_sized([width, 24.], egui::Button::new("Organize"))
                        .clicked()
                    {
                        self.start_library_edit(EditTarget::Symbol(symbol.id.clone()), "");
                    }
                });
            } else if !self.library_panel.selected_folder.is_empty()
                && ui.button("Rename / move folder…").clicked()
            {
                self.start_library_edit(
                    EditTarget::Folder(self.library_panel.selected_folder.clone()),
                    "",
                );
            }
            if ui
                .add_enabled(
                    self.can_convert_selection(),
                    egui::Button::new("Convert to Symbol   F8"),
                )
                .clicked()
            {
                self.convert_to_symbol();
            }
            self.resource_library(ui);
            ui.add_space(8.);
            ui.collapsing("Document", |ui| {
                ui.label(&self.project.name);
                ui.small(format!(
                    "{} × {} px · {} fps",
                    self.project.width, self.project.height, self.project.fps
                ));
                ui.small("Target: HTML5");
            });
        });
    }

    fn start_library_edit(&mut self, target: EditTarget, parent: &str) {
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
                EditTarget::Symbol(_) => "Organize symbol",
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
                        candidate.move_library_symbol(id, &edit.parent).map(|()| {
                            candidate.symbol_mut(id).unwrap().name = name.to_owned();
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
fn library_row(
    ui: &mut egui::Ui,
    name: &str,
    linkage: &str,
    depth: usize,
    folder: bool,
    selected: bool,
    expanded: bool,
) -> egui::Response {
    let height = if linkage.is_empty() { 23. } else { 36. };
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
        painter.rect_filled(rect, 2., visuals.weak_bg_fill);
    }
    let x = rect.left() + depth as f32 * 12. + 3.;
    let center = Pos2::new(x + 19., rect.top() + 11.5);
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
    painter.text(
        Pos2::new(x + 31., rect.top() + 11.5),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.),
        visuals.text_color(),
    );
    if !linkage.is_empty() {
        painter.text(
            Pos2::new(x + 31., rect.top() + 28.),
            Align2::LEFT_CENTER,
            format!("ID: {linkage}"),
            FontId::proportional(10.),
            if selected {
                visuals.text_color()
            } else {
                ui.visuals().weak_text_color()
            },
        );
    }
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
