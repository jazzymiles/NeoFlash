//! Breadcrumb `▾` and `Symbols & scenes` popover (NF2 `Popover / Symbols & scenes`, ST-08;
//! reference `interface/*/png/editor-editing-symbol-in-place.png`).
//!
//! Lists scenes, recent symbols, expandable library folders and instant search matches (`< 1` frame
//! single-pass filtering). Supports `↑`/`↓` row movement, `←`/`→` folder collapse/expand, `↵` edit,
//! `⌥↵` edit in place, and `Esc` / click-outside dismissal.
use crate::app::Editor;
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::{Id, SymbolKind};
use neoflash_ui::components::{self as c, TreeItemType};
use neoflash_ui::icons::{Icon, i16};
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use std::collections::{BTreeMap, HashSet};

const MAX_RECENT_SYMBOLS: usize = 8;
const NEW_WINDOW_REASON: &str = "Edit in new window";

#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolsPopoverState {
    pub(crate) open: bool,
    pub(crate) just_opened: bool,
    pub(crate) trigger_rect: Option<Rect>,
    pub(crate) query: String,
    pub(crate) active: usize,
    pub(crate) open_folders: HashSet<String>,
    pub(crate) recent_symbols: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PopoverTarget {
    Scene(Id),
    Symbol(Id),
    Folder(String),
}

#[derive(Clone, Debug)]
pub(crate) struct PopoverRow {
    pub(crate) label: String,
    pub(crate) kind: TreeItemType,
    pub(crate) depth: usize,
    pub(crate) meta: String,
    pub(crate) target: PopoverTarget,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PopoverSection {
    pub(crate) title: String,
    pub(crate) rows: Vec<PopoverRow>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolsPopoverModel {
    pub(crate) sections: Vec<PopoverSection>,
}

impl SymbolsPopoverModel {
    pub(crate) fn flat_targets(&self) -> Vec<PopoverTarget> {
        self.sections
            .iter()
            .flat_map(|s| s.rows.iter().map(|r| r.target.clone()))
            .collect()
    }
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-symbols-popover")
}

pub(crate) fn popover_state(ctx: &egui::Context) -> SymbolsPopoverState {
    ctx.data(|d| d.get_temp::<SymbolsPopoverState>(state_id()))
        .unwrap_or_default()
}

pub(crate) fn store_popover_state(ctx: &egui::Context, state: SymbolsPopoverState) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

pub(crate) fn is_symbols_popover_open(ctx: &egui::Context) -> bool {
    popover_state(ctx).open
}

fn symbol_icon(kind: SymbolKind) -> Icon {
    match kind {
        SymbolKind::Graphic => i16::GRAPHIC,
        SymbolKind::Button => i16::BUTTON_SYMBOL,
        SymbolKind::MovieClip => i16::MOVIECLIP,
    }
}

impl Editor {
    /// Keep `recent_symbols` pruned and record any symbol currently on the editing stack.
    pub(crate) fn sync_recent_symbols(&self, state: &mut SymbolsPopoverState) {
        state
            .recent_symbols
            .retain(|id| !self.project.is_scene(id) && self.project.symbol(id).is_some());
        for id in self.editing.iter().skip(1) {
            if !self.project.is_scene(id) && self.project.symbol(id).is_some() {
                state.recent_symbols.retain(|existing| existing != id);
                state.recent_symbols.insert(0, id.clone());
            }
        }
        state.recent_symbols.truncate(MAX_RECENT_SYMBOLS);
    }

    pub(crate) fn record_recent_symbol(&self, ctx: &egui::Context, id: &str) {
        if self.project.is_scene(id) || self.project.symbol(id).is_none() {
            return;
        }
        let mut state = popover_state(ctx);
        state.recent_symbols.retain(|existing| existing != id);
        state.recent_symbols.insert(0, id.to_owned());
        state.recent_symbols.truncate(MAX_RECENT_SYMBOLS);
        store_popover_state(ctx, state);
    }

    pub(crate) fn open_symbols_popover(&self, ctx: &egui::Context, trigger_rect: Option<Rect>) {
        let mut state = popover_state(ctx);
        self.sync_recent_symbols(&mut state);
        state.open = true;
        state.just_opened = true;
        if trigger_rect.is_some() {
            state.trigger_rect = trigger_rect;
        }
        state.active = 0;
        store_popover_state(ctx, state);
    }

    pub(crate) fn toggle_symbols_popover(&self, ctx: &egui::Context, trigger_rect: Option<Rect>) {
        if is_symbols_popover_open(ctx) {
            self.close_symbols_popover(ctx);
        } else {
            self.open_symbols_popover(ctx, trigger_rect);
        }
    }

    pub(crate) fn close_symbols_popover(&self, ctx: &egui::Context) {
        let mut state = popover_state(ctx);
        state.open = false;
        state.just_opened = false;
        store_popover_state(ctx, state);
    }

    #[cfg(test)]
    pub(crate) fn set_symbols_popover_query(&self, ctx: &egui::Context, query: impl Into<String>) {
        let mut state = popover_state(ctx);
        self.sync_recent_symbols(&mut state);
        state.query = query.into();
        state.active = 0;
        store_popover_state(ctx, state);
    }

    /// Build the filtered/grouped rows for the current popover state in a single pass.
    pub(crate) fn build_symbols_popover_model(
        &self,
        state: &SymbolsPopoverState,
    ) -> SymbolsPopoverModel {
        let scenes = self.scene_list();
        let current_scene = self.current_scene();
        let editing_scene_root = self.editing.len() == 1;

        let mut non_scene: Vec<_> = self
            .project
            .symbols
            .iter()
            .filter(|s| !self.project.is_scene(&s.id))
            .collect();
        non_scene.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        let total_non_scene = non_scene.len();

        let trimmed = state.query.trim();
        let mut sections = Vec::new();

        if !trimmed.is_empty() {
            let needle = trimmed.to_ascii_lowercase();
            let matching_scenes: Vec<PopoverRow> = scenes
                .iter()
                .filter(|(_, name)| name.to_ascii_lowercase().contains(&needle))
                .map(|(id, name)| PopoverRow {
                    label: name.clone(),
                    kind: TreeItemType::Symbol(i16::SCENE),
                    depth: 0,
                    meta: if id == current_scene && editing_scene_root {
                        "✓".into()
                    } else {
                        String::new()
                    },
                    target: PopoverTarget::Scene(id.clone()),
                })
                .collect();
            if !matching_scenes.is_empty() {
                sections.push(PopoverSection {
                    title: format!("Scenes · {}", matching_scenes.len()),
                    rows: matching_scenes,
                });
            }

            let matching_symbols: Vec<PopoverRow> = non_scene
                .iter()
                .filter(|sym| {
                    sym.name.to_ascii_lowercase().contains(&needle)
                        || (!sym.folder.is_empty()
                            && sym.folder.to_ascii_lowercase().contains(&needle))
                })
                .map(|sym| PopoverRow {
                    label: sym.name.clone(),
                    kind: TreeItemType::Symbol(symbol_icon(sym.kind)),
                    depth: 0,
                    meta: sym.folder.clone(),
                    target: PopoverTarget::Symbol(sym.id.clone()),
                })
                .collect();
            sections.push(PopoverSection {
                title: format!(
                    "Matches in symbols · {} of {total_non_scene}",
                    matching_symbols.len()
                ),
                rows: matching_symbols,
            });
            return SymbolsPopoverModel { sections };
        }

        // Empty query: Scenes -> Recent -> Browse.
        let scene_title = if scenes.len() > 1 {
            format!("Scenes · {}", scenes.len())
        } else {
            "Scenes".into()
        };
        let scene_rows = scenes
            .into_iter()
            .map(|(id, name)| PopoverRow {
                label: name,
                kind: TreeItemType::Symbol(i16::SCENE),
                depth: 0,
                meta: if id == current_scene && editing_scene_root {
                    "✓".into()
                } else {
                    String::new()
                },
                target: PopoverTarget::Scene(id),
            })
            .collect();
        sections.push(PopoverSection {
            title: scene_title,
            rows: scene_rows,
        });

        let recent_rows: Vec<PopoverRow> = state
            .recent_symbols
            .iter()
            .filter(|id| !self.project.is_scene(id))
            .filter_map(|id| self.project.symbol(id))
            .map(|sym| PopoverRow {
                label: sym.name.clone(),
                kind: TreeItemType::Symbol(symbol_icon(sym.kind)),
                depth: 0,
                meta: if self.symbol_id() == sym.id {
                    "✓".into()
                } else {
                    String::new()
                },
                target: PopoverTarget::Symbol(sym.id.clone()),
            })
            .collect();
        if !recent_rows.is_empty() {
            sections.push(PopoverSection {
                title: "Recent".into(),
                rows: recent_rows,
            });
        }

        if total_non_scene > 0 {
            let mut folders: BTreeMap<String, Vec<&neoflash_core::Symbol>> = BTreeMap::new();
            let mut root_symbols = Vec::new();
            for sym in &non_scene {
                if sym.folder.is_empty() {
                    root_symbols.push(*sym);
                } else {
                    folders.entry(sym.folder.clone()).or_default().push(*sym);
                }
            }
            let mut browse_rows = Vec::new();
            for (folder, members) in folders {
                let open = state.open_folders.contains(&folder);
                browse_rows.push(PopoverRow {
                    label: format!("{folder}  ({})", members.len()),
                    kind: if open {
                        TreeItemType::FolderOpen
                    } else {
                        TreeItemType::FolderClosed
                    },
                    depth: 0,
                    meta: String::new(),
                    target: PopoverTarget::Folder(folder),
                });
                if open {
                    for sym in members {
                        browse_rows.push(PopoverRow {
                            label: sym.name.clone(),
                            kind: TreeItemType::Symbol(symbol_icon(sym.kind)),
                            depth: 1,
                            meta: if self.symbol_id() == sym.id {
                                "✓".into()
                            } else {
                                String::new()
                            },
                            target: PopoverTarget::Symbol(sym.id.clone()),
                        });
                    }
                }
            }
            for sym in root_symbols {
                browse_rows.push(PopoverRow {
                    label: sym.name.clone(),
                    kind: TreeItemType::Symbol(symbol_icon(sym.kind)),
                    depth: 0,
                    meta: if self.symbol_id() == sym.id {
                        "✓".into()
                    } else {
                        String::new()
                    },
                    target: PopoverTarget::Symbol(sym.id.clone()),
                });
            }
            sections.push(PopoverSection {
                title: "Browse".into(),
                rows: browse_rows,
            });
        }

        SymbolsPopoverModel { sections }
    }

    /// Edit a library symbol from the popover: `in_place = false` (`↵`) opens `[current_scene, id]`;
    /// `in_place = true` (`⌥↵`) pushes onto the current editing stack (or truncates to it if already
    /// present so recursive breadcrumb loops cannot form).
    pub(crate) fn edit_symbol_from_popover(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        in_place: bool,
    ) {
        if self.project.is_scene(id) || self.project.symbol(id).is_none() {
            return;
        }
        self.record_recent_symbol(ctx, id);
        if in_place {
            if self.editing.last().map(String::as_str) == Some(id) {
                return;
            }
            if let Some(level) = self.editing.iter().position(|entry| entry == id) {
                self.leave_to_level(level);
            } else {
                self.enter_symbol(id.to_owned());
            }
        } else {
            self.finish_edits();
            self.timeline_key_drag = None;
            let scene = self.current_scene().to_owned();
            self.editing = vec![scene, id.to_owned()];
            self.reset_authored_preview();
            self.frame = 0;
            self.layer = 0;
            self.clear_selection();
            self.playing = false;
            self.pan = Vec2::ZERO;
        }
    }

    pub(crate) fn activate_symbols_popover_target(
        &mut self,
        ctx: &egui::Context,
        target: PopoverTarget,
        in_place: bool,
    ) {
        match target {
            PopoverTarget::Scene(id) => {
                self.switch_scene(&id);
                self.close_symbols_popover(ctx);
            }
            PopoverTarget::Symbol(id) => {
                self.edit_symbol_from_popover(ctx, &id, in_place);
                self.close_symbols_popover(ctx);
            }
            PopoverTarget::Folder(folder) => {
                let mut state = popover_state(ctx);
                if !state.open_folders.remove(&folder) {
                    state.open_folders.insert(folder);
                }
                store_popover_state(ctx, state);
            }
        }
    }

    pub(crate) fn symbols_popover(&mut self, ctx: &egui::Context, canvas: Rect) {
        let mut state = popover_state(ctx);
        self.sync_recent_symbols(&mut state);
        if !state.open {
            store_popover_state(ctx, state);
            return;
        }

        let trigger = state.trigger_rect.unwrap_or_else(|| {
            Rect::from_min_size(canvas.min + Vec2::splat(space::S3), egui::vec2(120.0, 28.0))
        });
        let pos = Pos2::new(trigger.left(), trigger.bottom() + space::S1);

        let nf = neoflash_ui::palette(ctx);
        let query_was_empty = state.query.is_empty();
        let mut activated: Option<(PopoverTarget, bool)> = None;
        let mut toggled_folder: Option<String> = None;

        let area = egui::Area::new(egui::Id::new("nf2-symbols-popover-area"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain_to(ctx.content_rect())
            .show(ctx, |ui| {
                c::popover_frame(ctx).show(ui, |ui| {
                    ui.set_width(312.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(space::S1);
                    let search = ui
                        .add(c::Search::new(&mut state.query, "Find symbol or scene").width(312.0));
                    if state.just_opened {
                        ui.ctx()
                            .memory_mut(|m| m.request_focus(search.id.with("edit")));
                    }
                    if search.changed() {
                        state.active = 0;
                    }
                    ui.add_space(space::S1);

                    let model = self.build_symbols_popover_model(&state);
                    let flat = model.flat_targets();
                    if flat.is_empty() {
                        state.active = 0;
                    } else if state.active >= flat.len() {
                        state.active = flat.len() - 1;
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("nf2-symbols-popover-scroll")
                        .max_height(360.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut flat_index = 0usize;
                            for section in &model.sections {
                                c::menu_section(ui, &section.title);
                                for row in &section.rows {
                                    let is_selected = flat_index == state.active;
                                    let item = c::TreeItem::new(&row.label, row.kind)
                                        .depth(row.depth)
                                        .meta(&row.meta)
                                        .selected(is_selected)
                                        .show(ui);
                                    if item.row.hovered() {
                                        state.active = flat_index;
                                    }
                                    if item.toggle {
                                        if let PopoverTarget::Folder(f) = &row.target {
                                            toggled_folder = Some(f.clone());
                                        }
                                    } else if item.row.clicked() {
                                        let alt = ui.input(|i| i.modifiers.alt);
                                        activated = Some((row.target.clone(), alt));
                                    }
                                    flat_index += 1;
                                }
                            }
                        });

                    c::menu_separator(ui);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
                        ui.add_space(space::S2);
                        for (key, action) in [("↑↓", "move"), ("↵", "edit"), ("⌥↵", "in place")]
                        {
                            c::kbd(ui, key, false);
                            ui.label(rich(action, TextStyle::Small).color(nf.text_secondary));
                            ui.add_space(space::S1);
                        }
                        let nw = ui
                            .scope(|ui| {
                                ui.disable();
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = space::S1;
                                    c::kbd(ui, "⌘↵", false);
                                    ui.label(
                                        rich("new window", TextStyle::Small)
                                            .color(nf.text_disabled),
                                    );
                                });
                            })
                            .response;
                        let _ = c::not_available(nw, NEW_WINDOW_REASON);
                    });
                });
            });

        if let Some(folder) = toggled_folder
            && !state.open_folders.remove(&folder)
        {
            state.open_folders.insert(folder);
        }

        let model = self.build_symbols_popover_model(&state);
        let flat = model.flat_targets();
        let count = flat.len();

        let mut close = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            if count > 0 {
                state.active = (state.active + 1) % count;
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            if count > 0 {
                state.active = (state.active + count - 1) % count;
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)) {
            if let Some(PopoverTarget::Folder(f)) = flat.get(state.active) {
                state.open_folders.insert(f.clone());
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
            if let Some(PopoverTarget::Folder(f)) = flat.get(state.active) {
                state.open_folders.remove(f);
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            self.message("Edit in new window is not available yet.");
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::Enter)) {
            if let Some(target) = flat.get(state.active).cloned() {
                activated = Some((target, true));
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            if let Some(target) = flat.get(state.active).cloned() {
                activated = Some((target, false));
            }
        } else if query_was_empty
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            close = true;
        }

        let clicked_trigger = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| trigger.contains(p))
        });
        if !state.just_opened
            && !clicked_trigger
            && ctx.memory(|m| m.areas().visible_last_frame(&area.response.layer_id))
            && area.response.clicked_elsewhere()
        {
            close = true;
        }

        state.just_opened = false;
        if close {
            state.open = false;
        }
        store_popover_state(ctx, state);

        if let Some((target, in_place)) = activated {
            self.activate_symbols_popover_target(ctx, target, in_place);
        }
    }
}
