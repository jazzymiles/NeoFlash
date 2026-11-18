//! Actions sidebar: toolbox (search + API tree) and Script navigator.
//!
//! NF2 reference: `panels/*/png/panel-actions-floating.png` and `interface/*/png/editor-actions-tab.png`,
//! outline `panel-actions-floating.txt` › Sidebar (`width/tree` incl. its 1 pt border; control-gap padding,
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

/// Sidebar width including its right border (`width/tree`).
pub(crate) fn sidebar_width() -> f32 {
    neoflash_ui::metrics::tree_width()
}
/// Below this panel width the sidebar is hidden so the code keeps a usable width: the sidebar, the
/// same again for code and one `width/field-m` of slack.
pub(crate) fn sidebar_min_panel() -> f32 {
    2. * sidebar_width() + neoflash_ui::metrics::field_m()
}
/// Height of the Script navigator below the toolbox: 42 % of the sidebar, 5 to 11 tree rows.
pub(crate) fn navigator_height(sidebar_height: f32) -> f32 {
    let row = neoflash_ui::metrics::row();
    (sidebar_height * 0.42).clamp(5. * row, 11. * row)
}

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
    /// Script tabs pinned to the bottom bar of `Actions` (`T-364`).
    pub pinned_tabs: Vec<ScriptLocation>,
    /// Currently active pinned tab (`None` = follow live Stage/Timeline selection).
    pub active_pinned: Option<ScriptLocation>,
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
            pinned_tabs: Vec::new(),
            active_pinned: None,
        }
    }
}

/// Where a navigator entry leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScriptLocation {
    Frame {
        symbol: String,
        layer: Option<String>,
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
        if let Some(ScriptLocation::Source(id)) = self.effective_actions_location() {
            self.project
                .sources
                .iter()
                .find(|s| s.id == id)
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
        let navigator_h = navigator_height(full.height());
        ui.add(nf::Search::new(
            &mut self.actions_ui.search,
            "Search toolbox",
        ));
        ui.add_space(space::s1());
        let toolbox_h = (ui.available_height() - navigator_h - space::s2()).max(row_min());
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
        ui.add_space(space::s1());
        let y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            egui::Stroke::new(1.0_f32, p.border_subtle),
        );
        ui.add_space(space::s3());
        ui.label(
            neoflash_ui::typography::rich("Script navigator", TextStyle::Overline)
                .color(p.text_secondary),
        );
        ui.add_space(space::s1());
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
            ui.add_space(space::s2());
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
                let tip = format!("{class}\n{}", script_api::class_support(api, class));
                nf::tooltip(row, tip.trim_end_matches('.'), None);
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
        let tip = member_tooltip(member);
        let row = nf::tooltip(row.row, tip.trim_end_matches('.'), None);
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
            let icon = match self.effective_actions_location() {
                Some(ScriptLocation::Source(_)) => i16::FILE_CODE,
                Some(ScriptLocation::Object { .. }) => i16::MOVIECLIP,
                _ => i16::FRAME,
            };
            nf::TreeItem::new(current_label, TreeItemType::Symbol(icon))
                .depth(1)
                .selected(true)
                .show(ui);
        }
        let mut chosen = None;
        for (idx, scene_id) in self.project.scene_ids().into_iter().enumerate() {
            let scene_name = self
                .project
                .symbol(&scene_id)
                .map_or("Scene", |s| s.name.as_str())
                .to_owned();
            let folder_key = if idx == 0 {
                "nav:root".to_owned()
            } else {
                format!("nav:scene:{scene_id}")
            };
            if self.folder_row(ui, &folder_key, &scene_name, i16::SCENE, 0) {
                for entry in self.navigator_entries(&scene_id) {
                    chosen = chosen.or(self.navigator_leaf(ui, &entry, 1));
                }
            }
        }
        if self.folder_row(ui, "nav:symbols", "Symbol definitions", i16::SYMBOL, 0) {
            let symbols: Vec<String> = self
                .project
                .symbols
                .iter()
                .filter(|s| !self.project.is_scene(&s.id))
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
        let current = self.effective_actions_location().as_ref() == Some(&entry.location);
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
        let mut frame_scripts: Vec<_> = symbol
            .timeline
            .scripts
            .iter()
            .filter(|s| !s.script.source.trim().is_empty())
            .collect();
        frame_scripts.sort_by_key(|script| script.frame);
        let mut entries: Vec<NavEntry> = frame_scripts
            .into_iter()
            .map(|script| {
                let layer = script.layer.clone().or_else(|| {
                    symbol
                        .timeline
                        .action_layer_for_script(script)
                        .and_then(|index| symbol.timeline.layers.get(index))
                        .map(|layer| layer.id.clone())
                });
                let layer_name = layer
                    .as_deref()
                    .and_then(|id| symbol.timeline.layers.iter().find(|item| item.id == id))
                    .map(|layer| layer.name.as_str());
                NavEntry {
                    label: layer_name.map_or_else(
                        || format!("{} : Frame {}", symbol.name, script.frame + 1),
                        |name| format!("{name} : Frame {}", script.frame + 1),
                    ),
                    icon: i16::FRAME,
                    location: ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        layer,
                        frame: script.frame,
                    },
                }
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
                layer: self
                    .symbol()
                    .timeline
                    .layers
                    .get(self.layer)
                    .map(|layer| layer.id.clone()),
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
        let enter = |editor: &mut Editor, symbol: &str, frame: u32| -> bool {
            if editor.project.symbol(symbol).is_none() {
                return false;
            }
            if editor.symbol_id() == symbol {
                if editor.editing_in_place() {
                    editor.enter_symbol_isolated(symbol.to_owned());
                }
            } else if editor.project.is_scene(symbol) {
                editor.switch_scene(symbol);
            } else {
                editor.leave_to_level(0);
                if symbol != editor.project.root {
                    editor.enter_symbol_isolated(symbol.to_owned());
                }
            }
            editor.set_playing(false);
            let max_frame = editor.symbol().timeline.frame_count.saturating_sub(1);
            editor.frame = frame.min(max_frame);
            true
        };
        match location {
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                if !enter(self, &symbol, frame) {
                    return;
                }
                self.layer = layer
                    .as_deref()
                    .and_then(|id| {
                        self.symbol()
                            .timeline
                            .layers
                            .iter()
                            .position(|item| item.id == id)
                    })
                    .or_else(|| self.symbol().timeline.action_layer_for_frame(self.frame))
                    .unwrap_or(self.layer);
                self.script_target = ScriptTarget::Frame;
                self.normalize_selection();
            }
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => {
                if !enter(self, &symbol, frame) {
                    return;
                }
                let found_on_frame =
                    self.symbol()
                        .timeline
                        .layers
                        .iter()
                        .enumerate()
                        .find_map(|(idx, layer)| {
                            let count = self.symbol().timeline.frame_count;
                            let end = layer.properties.end_frame.unwrap_or(count).min(count);
                            (self.frame < end
                                && layer
                                    .key_at(self.frame)
                                    .is_some_and(|k| k.elements.iter().any(|e| e.id == element)))
                            .then(|| (idx, layer.id.clone(), self.frame))
                        })
                        .or_else(|| {
                            self.symbol().timeline.layers.iter().enumerate().find_map(
                                |(idx, layer)| {
                                    layer
                                        .keyframes
                                        .iter()
                                        .find(|k| k.elements.iter().any(|e| e.id == element))
                                        .map(|k| (idx, layer.id.clone(), k.frame))
                                },
                            )
                        });
                if let Some((layer_idx, layer_id, resolved_frame)) = found_on_frame {
                    let max_frame = self.symbol().timeline.frame_count.saturating_sub(1);
                    self.frame = resolved_frame.min(max_frame);
                    if !self.is_layer_id_effectively_soloed(&layer_id) {
                        self.clear_all_solo_layers();
                    }
                    self.layer = layer_idx;
                }
                self.select_single_id(element);
                self.script_target = ScriptTarget::Object;
                self.script_event = event;
                self.normalize_selection();
            }
            ScriptLocation::Source(id) => {
                if self.project.sources.iter().any(|u| u.id == id) {
                    self.set_playing(false);
                    self.script_target = ScriptTarget::Source;
                    self.selected_source = Some(id);
                }
            }
        }
        self.activate_current_actions_location();
    }

    /// Normalize a [`ScriptLocation`] against `self.project`, returning `None` if its owning
    /// symbol, layer, element, or source file no longer exists.
    pub(crate) fn canonical_script_location(
        &self,
        location: &ScriptLocation,
    ) -> Option<ScriptLocation> {
        match location {
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                let sym = self.project.symbol(symbol)?;
                if sym.timeline.layers.is_empty() || *frame >= 1_000_000 {
                    return None;
                }
                let resolved_layer = if let Some(layer_id) = layer {
                    if !sym.timeline.layers.iter().any(|l| &l.id == layer_id) {
                        return None;
                    }
                    Some(layer_id.clone())
                } else {
                    sym.timeline
                        .action_layer_for_frame(*frame)
                        .and_then(|idx| sym.timeline.layers.get(idx))
                        .or_else(|| sym.timeline.layers.first())
                        .map(|l| l.id.clone())
                };
                Some(ScriptLocation::Frame {
                    symbol: symbol.clone(),
                    layer: resolved_layer,
                    frame: *frame,
                })
            }
            ScriptLocation::Object {
                symbol,
                frame: _,
                element,
                event,
            } => {
                let sym = self.project.symbol(symbol)?;
                let key_frame = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .find(|k| k.elements.iter().any(|e| &e.id == element))
                    .map(|k| k.frame)?;
                Some(ScriptLocation::Object {
                    symbol: symbol.clone(),
                    frame: key_frame,
                    element: element.clone(),
                    event: *event,
                })
            }
            ScriptLocation::Source(id) => self
                .project
                .sources
                .iter()
                .any(|s| &s.id == id)
                .then(|| ScriptLocation::Source(id.clone())),
        }
    }

    pub(crate) fn prune_stale_pinned_script_tabs(&mut self) {
        let mut kept = Vec::with_capacity(self.actions_ui.pinned_tabs.len());
        for loc in &self.actions_ui.pinned_tabs {
            if let Some(canon) = self.canonical_script_location(loc)
                && !kept.contains(&canon)
            {
                kept.push(canon);
            }
        }
        self.actions_ui.pinned_tabs = kept;
        if let Some(active) = &self.actions_ui.active_pinned {
            self.actions_ui.active_pinned = self
                .canonical_script_location(active)
                .filter(|canon| self.actions_ui.pinned_tabs.contains(canon));
        }
    }

    /// Script location currently open in the `Actions` editor (`active_pinned` when set and valid,
    /// otherwise the live Stage/Timeline `current_script_location()`).
    pub(crate) fn effective_actions_location(&self) -> Option<ScriptLocation> {
        if let Some(pinned) = &self.actions_ui.active_pinned
            && let Some(valid) = self.canonical_script_location(pinned)
        {
            return Some(valid);
        }
        self.current_script_location()
            .and_then(|loc| self.canonical_script_location(&loc).or(Some(loc)))
    }

    /// Switch `Actions` to follow `current_script_location()` (selecting its pinned tab if that
    /// location happens to be pinned, or `None` to show the unpinned live-selection tab).
    pub(crate) fn activate_current_actions_location(&mut self) {
        self.prune_stale_pinned_script_tabs();
        let cur = self
            .current_script_location()
            .and_then(|loc| self.canonical_script_location(&loc));
        self.actions_ui.active_pinned = cur.filter(|loc| self.actions_ui.pinned_tabs.contains(loc));
    }

    pub(crate) fn pin_script_tab(&mut self, location: ScriptLocation) {
        let Some(canon) = self.canonical_script_location(&location) else {
            return;
        };
        self.prune_stale_pinned_script_tabs();
        if !self.actions_ui.pinned_tabs.contains(&canon) {
            self.actions_ui.pinned_tabs.push(canon.clone());
        }
        self.actions_ui.active_pinned = Some(canon);
    }

    pub(crate) fn unpin_script_tab(&mut self, location: &ScriptLocation) {
        let target = self
            .canonical_script_location(location)
            .unwrap_or_else(|| location.clone());
        self.actions_ui
            .pinned_tabs
            .retain(|item| item != &target && item != location);
        if self
            .actions_ui
            .active_pinned
            .as_ref()
            .is_some_and(|active| active == &target || active == location)
        {
            self.actions_ui.active_pinned = None;
        }
    }

    pub(crate) fn unpin_other_script_tabs(&mut self, keep: &ScriptLocation) {
        let target = self
            .canonical_script_location(keep)
            .unwrap_or_else(|| keep.clone());
        self.actions_ui
            .pinned_tabs
            .retain(|item| item == &target || item == keep);
        if let Some(active) = &self.actions_ui.active_pinned
            && !self.actions_ui.pinned_tabs.contains(active)
        {
            self.actions_ui.active_pinned = self.actions_ui.pinned_tabs.first().cloned();
        }
    }

    pub(crate) fn toggle_pin_script_tab(&mut self, location: ScriptLocation) {
        let target = self
            .canonical_script_location(&location)
            .unwrap_or(location);
        if self.actions_ui.pinned_tabs.contains(&target) {
            self.unpin_script_tab(&target);
        } else {
            self.pin_script_tab(target);
        }
    }

    pub(crate) fn script_location_tab_label(&self, location: &ScriptLocation) -> (String, Icon) {
        match location {
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                let sym = self.project.symbol(symbol);
                let sym_name = sym.map_or("Scene", |s| s.name.as_str());
                let layer_name = sym.and_then(|s| {
                    layer
                        .as_deref()
                        .and_then(|id| s.timeline.layers.iter().find(|l| l.id == id))
                        .map(|l| l.name.as_str())
                });
                let multi_layer = sym.is_some_and(|s| s.timeline.layers.len() > 1);
                let label = if let Some(lname) = layer_name
                    && (multi_layer || lname != "Layer 1")
                {
                    if symbol != &self.project.root && !self.project.is_scene(symbol) {
                        format!("{sym_name} · {lname} : Frame {}", frame + 1)
                    } else {
                        format!("{lname} : Frame {}", frame + 1)
                    }
                } else {
                    format!("{sym_name} : Frame {}", frame + 1)
                };
                (label, i16::FRAME)
            }
            ScriptLocation::Object {
                symbol,
                element,
                event,
                ..
            } => {
                let el = self.project.symbol(symbol).and_then(|s| {
                    s.timeline
                        .layers
                        .iter()
                        .flat_map(|l| &l.keyframes)
                        .flat_map(|k| &k.elements)
                        .find(|e| &e.id == element)
                });
                let name = el.map_or("Object", |e| {
                    if e.name.is_empty() {
                        "Object"
                    } else {
                        e.name.as_str()
                    }
                });
                (format!("{name} : {event:?}"), i16::MOVIECLIP)
            }
            ScriptLocation::Source(id) => {
                let path = self
                    .project
                    .sources
                    .iter()
                    .find(|u| &u.id == id)
                    .map_or("Source.as", |u| u.path.as_str());
                (path.to_owned(), i16::FILE_CODE)
            }
        }
    }
}

fn row_min() -> f32 {
    2. * neoflash_ui::metrics::control()
}

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
