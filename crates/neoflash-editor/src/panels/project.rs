//! Project panel (NF2 `Panel / Project`, PP-05 / W0.12; reference `scripting-panels.png`):
//! project selector, file search, hierarchical tree of `src` (`project.sources` + virtual folders),
//! `assets` (`project.resources`) and the open `.nflash` document, bottom `New file` · `New folder` ·
//! `Delete` · `Publish` bar, and status/linkage caption.
use super::planned::caption;
use crate::app::{Editor, Pending, ScriptTarget};
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::{BTreeSet, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProjectSelection {
    RootSrc,
    Folder(String),
    Source(Id),
    RootAssets,
    Resource(Id),
    Document,
}

#[derive(Clone, Debug)]
struct RenameDraft {
    source_id: Id,
    text: String,
    focus: bool,
    had_focus: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectPanelState {
    pub(crate) filter: String,
    pub(crate) selected: Option<ProjectSelection>,
    pub(crate) folders: BTreeSet<String>,
    pub(crate) collapsed: HashSet<String>,
    rename: Option<RenameDraft>,
}

impl Default for ProjectPanelState {
    fn default() -> Self {
        let mut collapsed = HashSet::new();
        collapsed.insert("assets".to_owned());
        Self {
            filter: String::new(),
            selected: None,
            folders: BTreeSet::new(),
            collapsed,
            rename: None,
        }
    }
}

fn resource_icon(mime: &str) -> neoflash_ui::Icon {
    if mime.starts_with("audio/") {
        i16::SOUND
    } else if mime.starts_with("image/") {
        i16::BITMAP
    } else if mime.starts_with("video/") {
        i16::VIDEO
    } else if mime.starts_with("font/") {
        i16::TEXT
    } else {
        i16::NEW_FILE
    }
}

fn valid_source_path(path: &str) -> bool {
    let has_ext = [".as", ".js", ".ts"]
        .iter()
        .any(|ext| path.len() > ext.len() && path.to_ascii_lowercase().ends_with(ext));
    has_ext
        && path.len() <= 1024
        && !path.contains(['\\', '\0', ':'])
        && !path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
}

fn source_template(language: ScriptLanguage, folder: &str, stem: &str) -> String {
    let pkg = folder.replace('/', ".");
    match language {
        ScriptLanguage::ActionScript3 => {
            if pkg.is_empty() {
                format!(
                    "package {{\n    public class {stem} {{\n        public function {stem}() {{\n        }}\n    }}\n}}\n"
                )
            } else {
                format!(
                    "package {pkg} {{\n    public class {stem} {{\n        public function {stem}() {{\n        }}\n    }}\n}}\n"
                )
            }
        }
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => {
            let qualified = if pkg.is_empty() {
                stem.to_owned()
            } else {
                format!("{pkg}.{stem}")
            };
            format!("class {qualified} {{\n    public function {stem}() {{\n    }}\n}}\n")
        }
        ScriptLanguage::TypeScript => {
            format!("export class {stem} {{\n    constructor() {{\n    }}\n}}\n")
        }
        ScriptLanguage::JavaScript => {
            format!("export class {stem} {{\n    constructor() {{\n    }}\n}}\n")
        }
    }
}

fn extension_for(language: ScriptLanguage) -> &'static str {
    match language {
        ScriptLanguage::JavaScript => "js",
        ScriptLanguage::TypeScript => "ts",
        _ => "as",
    }
}

impl Editor {
    fn active_project_folder(&self) -> String {
        match &self.project_panel.selected {
            Some(ProjectSelection::Folder(path)) => path.clone(),
            Some(ProjectSelection::Source(id)) => self
                .project
                .sources
                .iter()
                .find(|s| &s.id == id)
                .and_then(|s| s.path.rsplit_once('/').map(|(dir, _)| dir.to_owned()))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Create a new project source file inside the selected `src` folder, select it and open Actions.
    pub(crate) fn project_new_source_file(&mut self, language: ScriptLanguage) -> Id {
        let folder = self.active_project_folder();
        let ext = extension_for(language);
        let mut index = 1;
        let (stem, full_path) = loop {
            let stem = format!("Class{index}");
            let candidate = if folder.is_empty() {
                format!("{stem}.{ext}")
            } else {
                format!("{folder}/{stem}.{ext}")
            };
            let exists = self
                .project
                .sources
                .iter()
                .any(|s| s.path.eq_ignore_ascii_case(&candidate));
            if !exists {
                break (stem, candidate);
            }
            index += 1;
        };
        let source = source_template(language, &folder, &stem);
        let id = new_id("source");
        let unit = SourceUnit {
            id: id.clone(),
            path: full_path,
            language,
            source,
        };
        self.project_panel.collapsed.remove("src");
        if !folder.is_empty() {
            let mut prefix = String::new();
            for seg in folder.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(seg);
                self.project_panel
                    .collapsed
                    .remove(&format!("dir:{prefix}"));
            }
        }
        self.transact_named("New source file", |s| {
            s.project.enable_program();
            s.project.sources.push(unit);
        });
        self.selected_source = Some(id.clone());
        self.project_panel.selected = Some(ProjectSelection::Source(id.clone()));
        self.script_target = ScriptTarget::Source;
        self.show_panel(PanelId::Actions);
        id
    }

    /// Create a new virtual folder inside `src` (persists once files are placed inside it).
    pub(crate) fn project_new_folder(&mut self) -> String {
        let parent = self.active_project_folder();
        let all_dirs = self.all_source_folders();
        let mut index = 1;
        let path = loop {
            let leaf = if index == 1 {
                "folder".to_owned()
            } else {
                format!("folder{index}")
            };
            let candidate = if parent.is_empty() {
                leaf
            } else {
                format!("{parent}/{leaf}")
            };
            if !all_dirs.iter().any(|d| d.eq_ignore_ascii_case(&candidate)) {
                break candidate;
            }
            index += 1;
        };
        self.project_panel.collapsed.remove("src");
        self.project_panel.folders.insert(path.clone());
        self.project_panel.selected = Some(ProjectSelection::Folder(path.clone()));
        path
    }

    /// Delete the selected source file or folder in the Project panel.
    pub(crate) fn project_delete_selected(&mut self) -> bool {
        match self.project_panel.selected.clone() {
            Some(ProjectSelection::Source(id)) => {
                if !self.project.sources.iter().any(|s| s.id == id) {
                    self.project_panel.selected = None;
                    return false;
                }
                self.transact_named("Delete source file", |s| {
                    s.project.sources.retain(|u| u.id != id);
                });
                if self.selected_source.as_deref() == Some(id.as_str()) {
                    self.selected_source = self.project.sources.first().map(|s| s.id.clone());
                }
                self.project_panel.selected =
                    self.selected_source.clone().map(ProjectSelection::Source);
                true
            }
            Some(ProjectSelection::Folder(folder)) => {
                let prefix = format!("{folder}/");
                let has_sources = self
                    .project
                    .sources
                    .iter()
                    .any(|s| s.path.starts_with(&prefix));
                if has_sources {
                    self.transact_named("Delete source folder", |s| {
                        s.project.sources.retain(|u| !u.path.starts_with(&prefix));
                    });
                }
                self.project_panel
                    .folders
                    .retain(|f| f != &folder && !f.starts_with(&prefix));
                if self
                    .selected_source
                    .as_ref()
                    .is_some_and(|id| !self.project.sources.iter().any(|s| &s.id == id))
                {
                    self.selected_source = self.project.sources.first().map(|s| s.id.clone());
                }
                self.project_panel.selected = None;
                true
            }
            _ => false,
        }
    }

    fn all_source_folders(&self) -> BTreeSet<String> {
        let mut dirs = self.project_panel.folders.clone();
        for unit in &self.project.sources {
            let mut acc = String::new();
            let parts: Vec<&str> = unit.path.split('/').collect();
            for seg in parts.iter().take(parts.len().saturating_sub(1)) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(seg);
                dirs.insert(acc.clone());
            }
        }
        dirs
    }

    fn project_status_caption(&self) -> String {
        let dirty = self.dirty();
        let selected_unit = match &self.project_panel.selected {
            Some(ProjectSelection::Source(id)) => self.project.sources.iter().find(|s| &s.id == id),
            _ => self
                .selected_source
                .as_ref()
                .and_then(|id| self.project.sources.iter().find(|s| &s.id == id)),
        };
        let linkage = selected_unit.map(|unit| {
            let file = unit.path.rsplit('/').next().unwrap_or(&unit.path);
            let stem = unit.path.split('.').next().unwrap_or(&unit.path);
            let dotted = stem.replace('/', ".");
            let short = file.split('.').next().unwrap_or(file);
            if self
                .project
                .document_class
                .as_deref()
                .is_some_and(|dc| dc == dotted || dc == short)
            {
                format!("{file} is the document class")
            } else if let Some(sym) = self.project.symbols.iter().find(|sym| {
                sym.class_name
                    .as_deref()
                    .is_some_and(|cn| cn == dotted || cn == short)
            }) {
                format!("{file} is linked to symbol \"{}\"", sym.name)
            } else {
                format!("src/{}", unit.path)
            }
        });
        match (dirty, linkage) {
            (true, Some(info)) => format!("● unsaved changes · {info}"),
            (true, None) => "● unsaved changes".to_owned(),
            (false, Some(info)) => info,
            (false, None) => {
                let n = self.project.sources.len();
                let r = self.project.resources.len();
                format!(
                    "{n} source file{} · {r} asset{}",
                    if n == 1 { "" } else { "s" },
                    if r == 1 { "" } else { "s" }
                )
            }
        }
    }

    pub(crate) fn project_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::Project, "Project");
        let ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing.y = space::S1;
        let w = ui.available_width();

        if self.project_panel.selected.is_none()
            && let Some(id) = &self.selected_source
            && self.project.sources.iter().any(|s| &s.id == id)
        {
            self.project_panel.selected = Some(ProjectSelection::Source(id.clone()));
        }

        let header_label = format!("{} (project)", self.project.name);
        c::Select::new("nf2-project-panel-select", &header_label)
            .width(w)
            .show(ui, |ui| {
                let _ = ui.add(c::MenuItem::new(&self.project.name).checked(true));
                c::menu_separator(ui);
                if ui.add(c::MenuItem::new("New").shortcut("⌘N")).clicked() {
                    self.request(Pending::New, &ctx);
                    ui.close();
                }
                if ui.add(c::MenuItem::new("Open…").shortcut("⌘O")).clicked() {
                    self.request(Pending::Open, &ctx);
                    ui.close();
                }
            });

        ui.add(c::Search::new(&mut self.project_panel.filter, "Find file").width(w));

        let query = self.project_panel.filter.trim().to_lowercase();
        let footer_reserve = crate::theme::CONTROL_HEIGHT + 20.0 + space::S1 * 3.0;
        let tree_h = (ui.available_height() - footer_reserve).max(crate::theme::CONTROL_HEIGHT);

        let all_dirs = self.all_source_folders();
        let sources_snapshot = self.project.sources.clone();
        let resources_snapshot = self.project.resources.clone();
        let doc_filename = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.nflash", self.project.name));
        let doc_row_label = if self.dirty() {
            format!("{doc_filename}  ●")
        } else {
            doc_filename
        };

        let mut open_source: Option<Id> = None;
        let mut commit_rename: Option<(Id, String)> = None;
        let mut cancel_rename = false;

        egui::ScrollArea::vertical()
            .id_salt("project-panel-tree")
            .auto_shrink([false, false])
            .max_height(tree_h)
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                // ── src ──
                let src_open = !query.is_empty() || !self.project_panel.collapsed.contains("src");
                let src_row = c::TreeItem::new(
                    "src",
                    if src_open {
                        c::TreeItemType::FolderOpen
                    } else {
                        c::TreeItemType::FolderClosed
                    },
                )
                .selected(self.project_panel.selected == Some(ProjectSelection::RootSrc))
                .show(ui);
                if src_row.toggle || src_row.row.double_clicked() {
                    if !self.project_panel.collapsed.remove("src") {
                        self.project_panel.collapsed.insert("src".to_owned());
                    }
                } else if src_row.row.clicked() {
                    self.project_panel.selected = Some(ProjectSelection::RootSrc);
                }

                if src_open {
                    // Build sorted interleaved entries (folders + files) under `src`.
                    let mut entries: Vec<(bool, String, usize, Option<SourceUnit>)> = Vec::new();
                    for dir in &all_dirs {
                        if !query.is_empty()
                            && !dir.to_lowercase().contains(&query)
                            && !sources_snapshot.iter().any(|s| {
                                s.path.starts_with(&format!("{dir}/"))
                                    && s.path.to_lowercase().contains(&query)
                            })
                        {
                            continue;
                        }
                        let depth = dir.chars().filter(|c| *c == '/').count() + 1;
                        entries.push((true, dir.clone(), depth, None));
                    }
                    for unit in &sources_snapshot {
                        if !query.is_empty() && !unit.path.to_lowercase().contains(&query) {
                            continue;
                        }
                        let depth = unit.path.chars().filter(|c| *c == '/').count() + 1;
                        entries.push((false, unit.path.clone(), depth, Some(unit.clone())));
                    }
                    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)));

                    for (is_dir, full_path, depth, unit) in entries {
                        // Skip children of collapsed ancestor folders when not searching.
                        if query.is_empty() {
                            let mut anc = String::new();
                            let parts: Vec<&str> = full_path.split('/').collect();
                            let mut hidden = false;
                            for seg in parts.iter().take(parts.len().saturating_sub(1)) {
                                if !anc.is_empty() {
                                    anc.push('/');
                                }
                                anc.push_str(seg);
                                if self.project_panel.collapsed.contains(&format!("dir:{anc}")) {
                                    hidden = true;
                                    break;
                                }
                            }
                            if hidden {
                                continue;
                            }
                        }

                        let leaf = full_path.rsplit('/').next().unwrap_or(&full_path);
                        if is_dir {
                            let key = format!("dir:{full_path}");
                            let dir_open =
                                !query.is_empty() || !self.project_panel.collapsed.contains(&key);
                            let selected = self.project_panel.selected
                                == Some(ProjectSelection::Folder(full_path.clone()));
                            let row = c::TreeItem::new(
                                leaf,
                                if dir_open {
                                    c::TreeItemType::FolderOpen
                                } else {
                                    c::TreeItemType::FolderClosed
                                },
                            )
                            .depth(depth)
                            .selected(selected)
                            .show(ui);
                            if row.toggle || row.row.double_clicked() {
                                if !self.project_panel.collapsed.remove(&key) {
                                    self.project_panel.collapsed.insert(key);
                                }
                            } else if row.row.clicked() {
                                self.project_panel.selected =
                                    Some(ProjectSelection::Folder(full_path));
                            }
                        } else if let Some(unit) = unit {
                            let renaming = self
                                .project_panel
                                .rename
                                .as_ref()
                                .is_some_and(|r| r.source_id == unit.id);
                            if renaming && let Some(draft) = self.project_panel.rename.as_mut() {
                                let field = ui.add(
                                    c::TextField::new(&mut draft.text, "Source path")
                                        .width(ui.available_width())
                                        .mono(),
                                );
                                let edit_id = c::TextField::edit_id(field.id);
                                if draft.focus {
                                    ui.ctx().memory_mut(|m| m.request_focus(edit_id));
                                    draft.focus = false;
                                }
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                let has_focus = ui.ctx().memory(|m| m.has_focus(edit_id));
                                let lost = draft.had_focus && !has_focus;
                                draft.had_focus = has_focus;
                                if esc {
                                    cancel_rename = true;
                                } else if enter || lost {
                                    commit_rename = Some((unit.id.clone(), draft.text.clone()));
                                }
                                continue;
                            }
                            let selected = self.project_panel.selected
                                == Some(ProjectSelection::Source(unit.id.clone()));
                            let row =
                                c::TreeItem::new(leaf, c::TreeItemType::Symbol(i16::FILE_CODE))
                                    .depth(depth)
                                    .selected(selected)
                                    .show(ui);
                            if row.row.double_clicked() {
                                self.project_panel.rename = Some(RenameDraft {
                                    source_id: unit.id.clone(),
                                    text: unit.path.clone(),
                                    focus: true,
                                    had_focus: false,
                                });
                            } else if row.row.clicked() {
                                open_source = Some(unit.id.clone());
                            }
                        }
                    }
                }

                // ── assets ──
                let assets_open =
                    !query.is_empty() || !self.project_panel.collapsed.contains("assets");
                let assets_row = c::TreeItem::new(
                    "assets",
                    if assets_open {
                        c::TreeItemType::FolderOpen
                    } else {
                        c::TreeItemType::FolderClosed
                    },
                )
                .selected(self.project_panel.selected == Some(ProjectSelection::RootAssets))
                .show(ui);
                if assets_row.toggle || assets_row.row.double_clicked() {
                    if !self.project_panel.collapsed.remove("assets") {
                        self.project_panel.collapsed.insert("assets".to_owned());
                    }
                } else if assets_row.row.clicked() {
                    self.project_panel.selected = Some(ProjectSelection::RootAssets);
                }
                if assets_open {
                    for res in &resources_snapshot {
                        if !query.is_empty() && !res.path.to_lowercase().contains(&query) {
                            continue;
                        }
                        let leaf = res.path.rsplit('/').next().unwrap_or(&res.path);
                        let selected = self.project_panel.selected
                            == Some(ProjectSelection::Resource(res.id.clone()));
                        let row = c::TreeItem::new(
                            leaf,
                            c::TreeItemType::Symbol(resource_icon(&res.media_type)),
                        )
                        .depth(1)
                        .selected(selected)
                        .show(ui);
                        if row.row.clicked() {
                            self.project_panel.selected =
                                Some(ProjectSelection::Resource(res.id.clone()));
                        }
                    }
                }

                // ── <document>.nflash ──
                if query.is_empty() || doc_row_label.to_lowercase().contains(&query) {
                    let doc_row =
                        c::TreeItem::new(&doc_row_label, c::TreeItemType::Symbol(i16::NEW_FILE))
                            .selected(
                                self.project_panel.selected == Some(ProjectSelection::Document),
                            )
                            .show(ui);
                    if doc_row.row.clicked() {
                        self.project_panel.selected = Some(ProjectSelection::Document);
                    }
                }
            });

        if cancel_rename {
            self.project_panel.rename = None;
        } else if let Some((id, next_path)) = commit_rename {
            self.project_panel.rename = None;
            let trimmed = next_path.trim().trim_matches('/').to_owned();
            if valid_source_path(&trimmed) {
                let duplicate = self
                    .project
                    .sources
                    .iter()
                    .any(|s| s.id != id && s.path.eq_ignore_ascii_case(&trimmed));
                if !duplicate {
                    self.transact_named("Rename source file", |s| {
                        if let Some(unit) = s.project.sources.iter_mut().find(|u| u.id == id)
                            && unit.path != trimmed
                        {
                            unit.path = trimmed;
                        }
                    });
                } else {
                    self.message("A source file with this path already exists.");
                }
            } else {
                self.message("Invalid source path. Use relative folders and .as, .js or .ts.");
            }
        }

        if let Some(id) = open_source {
            self.project_panel.selected = Some(ProjectSelection::Source(id.clone()));
            self.selected_source = Some(id);
            self.script_target = ScriptTarget::Source;
            self.show_panel(PanelId::Actions);
        }

        let can_delete = matches!(
            self.project_panel.selected,
            Some(ProjectSelection::Source(_) | ProjectSelection::Folder(_))
        );

        ui.horizontal(|ui| {
            let new_btn = ui.add(c::IconButton::new(i16::NEW_FILE, "New file"));
            if new_btn.clicked() {
                let default_lang = self
                    .project
                    .sources
                    .first()
                    .map_or(ScriptLanguage::ActionScript3, |s| s.language);
                self.project_new_source_file(default_lang);
            }
            c::context_menu(&new_btn, |ui| {
                for (label, lang) in [
                    ("ActionScript 3.0 (.as)", ScriptLanguage::ActionScript3),
                    ("ActionScript 2.0 (.as)", ScriptLanguage::ActionScript2),
                    ("TypeScript (.ts)", ScriptLanguage::TypeScript),
                    ("JavaScript (.js)", ScriptLanguage::JavaScript),
                ] {
                    if ui.add(c::MenuItem::new(label)).clicked() {
                        self.project_new_source_file(lang);
                        ui.close();
                    }
                }
            });
            if ui
                .add(c::IconButton::new(i16::FOLDERPLUS, "New folder"))
                .clicked()
            {
                self.project_new_folder();
            }
            if ui
                .add_enabled(can_delete, c::IconButton::new(i16::TRASH, "Delete"))
                .clicked()
            {
                self.project_delete_selected();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(c::Button::new("Publish")).clicked() {
                    self.export(false);
                }
            });
        });

        let status = self.project_status_caption();
        caption(ui, &status);
    }
}
