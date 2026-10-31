//! Actions sidebar: toolbox (search + API tree) and Script navigator.
//!
//! NF2 reference: `panels/*/png/panel-actions-floating.png` and `interface/*/png/editor-actions-tab.png`,
//! outline `panel-actions-floating.txt` › Sidebar (248 pt incl. its 1 pt border; 8 pt inner padding,
//! `NF2/Search` + `NF2/Tree Item` rows, overline "Script navigator").
//!
//! The toolbox lists only APIs the runtime implements (see `crate::script_api`); double-clicking an
//! entry inserts it at the caret, and AS3 classes from a package also get their `import` line. The
//! navigator lists every script of the document and opens it on click.
use crate::app::{Editor, ScriptTarget};
use crate::code_tools::CodeCommand;
use crate::script_api::{self, Member};
use eframe::egui;
use neoflash_core::{ObjectEvent, ScriptLanguage};
use neoflash_ui::components::{self as nf, TreeItemType};
use neoflash_ui::icons::{Icon, i16};
use neoflash_ui::tokens::{TextStyle, space};
use std::collections::HashSet;

/// Sidebar width including its right border (outline: Sidebar 248).
pub(crate) const SIDEBAR_WIDTH: f32 = 248.;
/// Below this panel width the sidebar is hidden so the code keeps a usable width.
pub(crate) const SIDEBAR_MIN_PANEL: f32 = 640.;

/// Actions panel state that is not part of the document (not saved, not in history).
pub(crate) struct ActionsUi {
    /// "Show toolbox sidebar" toggle.
    pub sidebar: bool,
    pub search: String,
    /// Open toolbox/navigator folders by key; the navigator's default folders start open.
    pub open: HashSet<String>,
    pub selected: Option<String>,
    /// Command for the visible code editor, applied on its next frame (toolbar, toolbox).
    pub pending: Option<CodeCommand>,
    /// Last text change (editor id, time) for the live check debounce.
    pub edited: Option<(egui::Id, f64)>,
}

impl Default for ActionsUi {
    fn default() -> Self {
        Self {
            sidebar: true,
            search: String::new(),
            open: ["nav:current", "nav:root"].map(String::from).into(),
            selected: None,
            pending: None,
            edited: None,
        }
    }
}

/// Where a navigator entry leads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScriptLocation {
    Frame {
        symbol: String,
        frame: u32,
    },
    Object {
        symbol: String,
        frame: u32,
        element: String,
        event: ObjectEvent,
    },
    Source(String),
}

struct NavEntry {
    label: String,
    icon: Icon,
    location: ScriptLocation,
}

fn member_icon(member: &Member) -> Icon {
    match member.kind.as_str() {
        "class" | "interface" => i16::CLASS,
        "property" | "constant" => i16::VARIABLE,
        "event" | "callback" => i16::FLAG,
        _ => i16::CODE,
    }
}

fn member_tooltip(member: &Member) -> String {
    let mut text = match &member.params {
        Some(params) => format!("{}({params})", member.name),
        None => member.name.clone(),
    };
    if !member.ty.is_empty() && member.kind != "class" {
        text.push_str(&format!(" : {}", member.ty));
    }
    if !member.owner.is_empty() {
        text.push_str(&format!("\n{}", member.owner));
    }
    text.push_str(&format!("\n{}", member.kind));
    text
}

impl Editor {
    /// Language whose toolbox is shown: the open script's, or the selected project file's.
    fn sidebar_language(&self, script_language: ScriptLanguage) -> ScriptLanguage {
        if self.script_target == ScriptTarget::Source {
            self.project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
                .map_or(script_language, |s| s.language)
        } else {
            script_language
        }
    }

    /// Sidebar contents in `rect` (the caller reserves the space and draws the border).
    pub(crate) fn actions_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        script_language: ScriptLanguage,
        current_label: &str,
    ) {
        let language = self.sidebar_language(script_language);
        let full = ui.available_rect_before_wrap();
        let navigator_h = (full.height() * 0.42).clamp(120., 260.);
        ui.add(nf::Search::new(
            &mut self.actions_ui.search,
            "Search toolbox",
        ));
        ui.add_space(space::S1);
        let toolbox_h = (ui.available_height() - navigator_h - space::S2).max(ROW_MIN);
        egui::ScrollArea::vertical()
            .id_salt("actions-toolbox")
            .max_height(toolbox_h)
            .min_scrolled_height(toolbox_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                self.toolbox_tree(ui, language);
            });
        let p = neoflash_ui::palette(ui.ctx());
        ui.add_space(space::S1);
        let y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            egui::Stroke::new(1.0_f32, p.border_subtle),
        );
        ui.add_space(space::S3);
        ui.label(
            neoflash_ui::typography::rich("Script navigator", TextStyle::Overline)
                .color(p.text_secondary),
        );
        ui.add_space(space::S1);
        egui::ScrollArea::vertical()
            .id_salt("actions-navigator")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                self.script_navigator(ui, current_label);
            });
    }

    fn folder_row(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        icon: Icon,
        depth: usize,
    ) -> bool {
        self.folder_row_response(ui, key, label, icon, depth).0
    }

    fn folder_row_response(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        icon: Icon,
        depth: usize,
    ) -> (bool, egui::Response) {
        let open = self.actions_ui.open.contains(key);
        let kind = if open {
            TreeItemType::FolderOpen
        } else {
            TreeItemType::FolderClosed
        };
        let row = nf::TreeItem::new(label, kind)
            .icon(icon)
            .depth(depth)
            .show(ui);
        if row.toggle || row.row.clicked() {
            if open {
                self.actions_ui.open.remove(key);
            } else {
                self.actions_ui.open.insert(key.to_owned());
            }
        }
        (open, row.row)
    }

    fn toolbox_tree(&mut self, ui: &mut egui::Ui, language: ScriptLanguage) {
        let Some(api) = script_api::api(language) else {
            let p = neoflash_ui::palette(ui.ctx());
            ui.add_space(space::S2);
            ui.label(
                neoflash_ui::typography::rich_block(
                    "The toolbox lists the ActionScript 2.0 and 3.0 APIs NeoFlash runs. JavaScript and TypeScript call the browser host directly.",
                    TextStyle::Small,
                )
                .color(p.text_tertiary),
            );
            return;
        };
        let project = script_api::project_classes(&self.project, language);
        let needle = self.actions_ui.search.trim().to_owned();
        if !needle.is_empty() {
            let results = script_api::search(&project, api, &needle, 200);
            if results.is_empty() {
                let p = neoflash_ui::palette(ui.ctx());
                ui.label(
                    neoflash_ui::typography::rich(
                        format!("No toolbox entries match “{needle}”."),
                        TextStyle::Small,
                    )
                    .color(p.text_tertiary),
                );
            }
            for member in results {
                let owner = script_api::short_name(&member.owner).to_owned();
                let meta = if member.kind == "class" {
                    String::new()
                } else {
                    owner
                };
                self.toolbox_leaf(ui, &member, 0, &meta, language);
            }
            return;
        }
        for folder in script_api::toolbox(&project, api) {
            let key = format!("box:{}", folder.label);
            let icon = if folder.label == script_api::PROJECT_CLASSES {
                i16::FILE_CODE
            } else {
                i16::PACKAGE
            };
            if !self.folder_row(ui, &key, &folder.label, icon, 0) {
                continue;
            }
            for class in &folder.classes {
                let class_key = format!("box:class:{class}");
                let (open, row) = self.folder_row_response(
                    ui,
                    &class_key,
                    script_api::short_name(class),
                    i16::CLASS,
                    1,
                );
                row.on_hover_text(format!(
                    "{class}\n{}",
                    script_api::class_support(api, class)
                ));
                if open {
                    for member in script_api::own_members(&project, api, class) {
                        let meta = member.ty.clone();
                        self.toolbox_leaf(ui, &member, 2, script_api::short_name(&meta), language);
                    }
                }
            }
            for member in &folder.entries {
                self.toolbox_leaf(ui, member, 1, script_api::short_name(&member.ty), language);
            }
        }
    }

    fn toolbox_leaf(
        &mut self,
        ui: &mut egui::Ui,
        member: &Member,
        depth: usize,
        meta: &str,
        language: ScriptLanguage,
    ) {
        let key = format!("box:{}:{}:{}", member.owner, member.name, member.kind);
        let label = member.label();
        let row = nf::TreeItem::new(&label, TreeItemType::Symbol(member_icon(member)))
            .depth(depth)
            .meta(meta)
            .selected(self.actions_ui.selected.as_deref() == Some(&key))
            .show(ui);
        let row = row.row.on_hover_text(member_tooltip(member));
        if row.clicked() {
            self.actions_ui.selected = Some(key);
        }
        if row.double_clicked() {
            self.actions_ui.pending = Some(toolbox_insertion(member, language));
        }
    }

    /// Every script in the document, grouped as in Flash: current selection, the main timeline,
    /// symbol definitions and project files.
    fn script_navigator(&mut self, ui: &mut egui::Ui, current_label: &str) {
        if self.folder_row(ui, "nav:current", "Current selection", i16::TARGET, 0) {
            let icon = if self.script_target == ScriptTarget::Source {
                i16::FILE_CODE
            } else {
                i16::FRAME
            };
            nf::TreeItem::new(current_label, TreeItemType::Symbol(icon))
                .depth(1)
                .selected(true)
                .show(ui);
        }
        let root = self.project.root.clone();
        let root_name = self
            .project
            .symbol(&root)
            .map_or("Scene", |s| s.name.as_str())
            .to_owned();
        let mut chosen = None;
        if self.folder_row(ui, "nav:root", &root_name, i16::SCENE, 0) {
            for entry in self.navigator_entries(&root) {
                chosen = chosen.or(self.navigator_leaf(ui, &entry, 1));
            }
        }
        if self.folder_row(ui, "nav:symbols", "Symbol definitions", i16::SYMBOL, 0) {
            let symbols: Vec<String> = self
                .project
                .symbols
                .iter()
                .filter(|s| s.id != root)
                .map(|s| s.id.clone())
                .collect();
            for symbol in symbols {
                for entry in self.navigator_entries(&symbol) {
                    chosen = chosen.or(self.navigator_leaf(ui, &entry, 1));
                }
            }
        }
        if !self.project.sources.is_empty()
            && self.folder_row(ui, "nav:sources", "Project files", i16::FOLDER, 0)
        {
            let sources: Vec<NavEntry> = self
                .project
                .sources
                .iter()
                .map(|s| NavEntry {
                    label: s.path.clone(),
                    icon: i16::FILE_CODE,
                    location: ScriptLocation::Source(s.id.clone()),
                })
                .collect();
            for entry in sources {
                chosen = chosen.or(self.navigator_leaf(ui, &entry, 1));
            }
        }
        if let Some(location) = chosen {
            self.open_script_location(location);
        }
    }

    fn navigator_leaf(
        &self,
        ui: &mut egui::Ui,
        entry: &NavEntry,
        depth: usize,
    ) -> Option<ScriptLocation> {
        let current = self.current_script_location().as_ref() == Some(&entry.location);
        let row = nf::TreeItem::new(&entry.label, TreeItemType::Symbol(entry.icon))
            .depth(depth)
            .selected(current)
            .show(ui);
        row.row.clicked().then(|| entry.location.clone())
    }

    /// Frame and object scripts of one symbol, labelled like the script tab ("Name : Frame 3").
    fn navigator_entries(&self, symbol_id: &str) -> Vec<NavEntry> {
        let Some(symbol) = self.project.symbol(symbol_id) else {
            return Vec::new();
        };
        let mut frames: Vec<u32> = symbol
            .timeline
            .scripts
            .iter()
            .filter(|s| !s.script.source.trim().is_empty())
            .map(|s| s.frame)
            .collect();
        frames.sort_unstable();
        frames.dedup();
        let mut entries: Vec<NavEntry> = frames
            .into_iter()
            .map(|frame| NavEntry {
                label: format!("{} : Frame {}", symbol.name, frame + 1),
                icon: i16::FRAME,
                location: ScriptLocation::Frame {
                    symbol: symbol.id.clone(),
                    frame,
                },
            })
            .collect();
        let mut seen = HashSet::new();
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for script in element
                        .scripts
                        .iter()
                        .filter(|s| !s.script.source.trim().is_empty())
                    {
                        if !seen.insert((element.id.clone(), format!("{:?}", script.event))) {
                            continue;
                        }
                        let name = if element.name.is_empty() {
                            "Object"
                        } else {
                            &element.name
                        };
                        entries.push(NavEntry {
                            label: format!("{name} : {:?}", script.event),
                            icon: i16::MOVIECLIP,
                            location: ScriptLocation::Object {
                                symbol: symbol.id.clone(),
                                frame: key.frame,
                                element: element.id.clone(),
                                event: script.event,
                            },
                        });
                    }
                }
            }
        }
        entries
    }

    pub(crate) fn current_script_location(&self) -> Option<ScriptLocation> {
        match self.script_target {
            ScriptTarget::Frame => Some(ScriptLocation::Frame {
                symbol: self.symbol_id().to_owned(),
                frame: self.frame,
            }),
            ScriptTarget::Object => self.selected_element().map(|element| {
                let frame = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .find(|k| k.elements.iter().any(|e| e.id == element.id))
                    .map_or(self.frame, |k| k.frame);
                ScriptLocation::Object {
                    symbol: self.symbol_id().to_owned(),
                    frame,
                    element: element.id.clone(),
                    event: self.script_event,
                }
            }),
            ScriptTarget::Source => self.selected_source.clone().map(ScriptLocation::Source),
        }
    }

    /// Open a script in the Actions editor: enter its timeline, move to its frame, select its object.
    pub(crate) fn open_script_location(&mut self, location: ScriptLocation) {
        self.finish_edits();
        let enter = |editor: &mut Editor, symbol: &str, frame: u32| {
            if editor.symbol_id() != symbol {
                editor.leave_to_level(0);
                if symbol != editor.project.root {
                    editor.enter_symbol(symbol.to_owned());
                }
            }
            editor.set_playing(false);
            editor.frame = frame;
        };
        match location {
            ScriptLocation::Frame { symbol, frame } => {
                enter(self, &symbol, frame);
                self.script_target = ScriptTarget::Frame;
            }
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => {
                enter(self, &symbol, frame);
                self.select_single_id(element);
                self.script_target = ScriptTarget::Object;
                self.script_event = event;
            }
            ScriptLocation::Source(id) => {
                self.script_target = ScriptTarget::Source;
                self.selected_source = Some(id);
            }
        }
    }
}

const ROW_MIN: f32 = 2. * neoflash_ui::metrics::CONTROL;

/// What double-clicking a toolbox entry inserts.
pub(crate) fn toolbox_insertion(member: &Member, language: ScriptLanguage) -> CodeCommand {
    if member.kind == "class" {
        let import = (language == ScriptLanguage::ActionScript3)
            .then(|| member.owner.clone())
            .filter(|q| !script_api::package_of(q).is_empty());
        return CodeCommand::Insert {
            text: member.name.clone(),
            caret: member.name.chars().count(),
            import,
        };
    }
    let (text, caret) = member.insertion();
    let import = (language == ScriptLanguage::ActionScript3 && member.is_static)
        .then(|| member.owner.clone())
        .filter(|q| !script_api::package_of(q).is_empty());
    // Static members are written through their class: `MouseEvent.CLICK`.
    if member.is_static && !member.owner.is_empty() {
        let prefix = format!("{}.", script_api::short_name(&member.owner));
        let caret = prefix.chars().count() + caret;
        return CodeCommand::Insert {
            text: format!("{prefix}{text}"),
            caret,
            import,
        };
    }
    CodeCommand::Insert {
        text,
        caret,
        import,
    }
}
