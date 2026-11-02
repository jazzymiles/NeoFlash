//! Movie Explorer panel (`Panel / Movie Explorer`, `PP-12`; reference `document-dialogs.png`),
//! opened by Window ▸ Movie Explorer.
//!
//! Displays a live, searchable, filterable tree of the open document: scenes, layers, frames,
//! symbol instances, text fields, bitmaps/videos/frame sounds, timeline/object/project scripts,
//! and library symbol definitions, with click-to-jump navigation and breadcrumb footer.
use super::planned::mono;
use crate::app::Editor;
use crate::panels::actions_sidebar::ScriptLocation;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{Element, ElementKind, Id, Symbol, SymbolKind};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::HashSet;

pub(crate) const CAT_TEXT: usize = 0;
pub(crate) const CAT_SYMBOLS: usize = 1;
pub(crate) const CAT_SCRIPTS: usize = 2;
pub(crate) const CAT_MEDIA: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MovieTarget {
    Document,
    Folder,
    Timeline {
        symbol: Id,
    },
    Layer {
        symbol: Id,
        layer_index: usize,
    },
    Frame {
        symbol: Id,
        layer_index: Option<usize>,
        frame: u32,
    },
    Element {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
        instance_symbol: Option<Id>,
    },
    Script(ScriptLocation),
}

#[derive(Clone, Debug)]
pub(crate) struct MovieRow {
    pub key: String,
    pub label: String,
    pub breadcrumb: String,
    pub depth: usize,
    pub is_folder: bool,
    pub is_open: bool,
    pub leaf_icon: Option<neoflash_ui::Icon>,
    pub target: MovieTarget,
}

#[derive(Clone, Debug)]
pub(crate) struct MovieExplorerState {
    pub(crate) query: String,
    pub(crate) show_categories: [bool; 4],
    pub(crate) show_frames_and_layers: bool,
    pub(crate) show_symbol_definitions: bool,
    pub(crate) collapsed: HashSet<String>,
    pub(crate) selected_key: Option<String>,
    pub(crate) selected_breadcrumb: Option<String>,
}

impl Default for MovieExplorerState {
    fn default() -> Self {
        let mut collapsed = HashSet::new();
        collapsed.insert("symbol-defs".to_owned());
        Self {
            query: String::new(),
            show_categories: [true, true, true, true],
            show_frames_and_layers: true,
            show_symbol_definitions: true,
            collapsed,
            selected_key: None,
            selected_breadcrumb: None,
        }
    }
}

fn symbol_icon(kind: SymbolKind) -> neoflash_ui::Icon {
    match kind {
        SymbolKind::MovieClip => i16::MOVIECLIP,
        SymbolKind::Graphic => i16::GRAPHIC,
        SymbolKind::Button => i16::BUTTON_SYMBOL,
    }
}

fn truncate_inline(text: &str, max_chars: usize) -> String {
    let single: String = text
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect();
    let trimmed = single.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_owned()
    } else {
        let head: String = trimmed.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

struct LeafEntry {
    key: String,
    label: String,
    breadcrumb_leaf: String,
    icon: neoflash_ui::Icon,
    target: MovieTarget,
}

#[allow(clippy::too_many_arguments)]
fn collect_element_leaves(
    editor: &Editor,
    symbol_id: &str,
    layer_index: usize,
    frame: u32,
    el: &Element,
    cats: &[bool; 4],
    query: &str,
    out: &mut Vec<LeafEntry>,
) {
    let matches_query = |label: &str, extra: &str| {
        query.is_empty()
            || label.to_ascii_lowercase().contains(query)
            || extra.to_ascii_lowercase().contains(query)
    };
    let name_part = el.name.trim();
    match &el.kind {
        ElementKind::Instance {
            symbol_id: inst_sym,
            ..
        } if cats[CAT_SYMBOLS] => {
            let (sym_name, icon) = editor.project.symbol(inst_sym).map_or_else(
                || (inst_sym.as_str(), i16::MOVIECLIP),
                |s| (s.name.as_str(), symbol_icon(s.kind)),
            );
            let label = if name_part.is_empty() {
                sym_name.to_owned()
            } else {
                format!("{name_part}  ·  {sym_name}")
            };
            let bc = if name_part.is_empty() {
                sym_name.to_owned()
            } else {
                name_part.to_owned()
            };
            if matches_query(&label, sym_name) {
                out.push(LeafEntry {
                    key: format!("el:{symbol_id}:{layer_index}:{frame}:{}", el.id),
                    label,
                    breadcrumb_leaf: bc,
                    icon,
                    target: MovieTarget::Element {
                        symbol: symbol_id.to_owned(),
                        layer_index,
                        frame,
                        element_id: el.id.clone(),
                        instance_symbol: Some(inst_sym.clone()),
                    },
                });
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            ..
        } if cats[CAT_TEXT] => {
            let snippet = truncate_inline(text, 28);
            let label = format!(
                "{snippet:?}  ·  {font_family} {} pt",
                font_size.round() as i32
            );
            if matches_query(&label, text) {
                out.push(LeafEntry {
                    key: format!("el:{symbol_id}:{layer_index}:{frame}:{}", el.id),
                    label,
                    breadcrumb_leaf: if name_part.is_empty() {
                        format!("{snippet:?}")
                    } else {
                        name_part.to_owned()
                    },
                    icon: i16::FONT,
                    target: MovieTarget::Element {
                        symbol: symbol_id.to_owned(),
                        layer_index,
                        frame,
                        element_id: el.id.clone(),
                        instance_symbol: None,
                    },
                });
            }
        }
        ElementKind::TextBox { text, format, .. } if cats[CAT_TEXT] => {
            let snippet = truncate_inline(text, 28);
            let label = format!(
                "{snippet:?}  ·  {} {} pt",
                format.font,
                format.size.round() as i32
            );
            if matches_query(&label, text) {
                out.push(LeafEntry {
                    key: format!("el:{symbol_id}:{layer_index}:{frame}:{}", el.id),
                    label,
                    breadcrumb_leaf: if name_part.is_empty() {
                        format!("{snippet:?}")
                    } else {
                        name_part.to_owned()
                    },
                    icon: i16::FONT,
                    target: MovieTarget::Element {
                        symbol: symbol_id.to_owned(),
                        layer_index,
                        frame,
                        element_id: el.id.clone(),
                        instance_symbol: None,
                    },
                });
            }
        }
        ElementKind::Bitmap { resource_id, .. } if cats[CAT_MEDIA] => {
            let res_name = editor
                .project
                .resources
                .iter()
                .find(|r| &r.id == resource_id)
                .map_or(resource_id.as_str(), |r| r.path.as_str());
            let label = if name_part.is_empty() {
                res_name.to_owned()
            } else {
                format!("{name_part}  ·  {res_name}")
            };
            if matches_query(&label, res_name) {
                out.push(LeafEntry {
                    key: format!("el:{symbol_id}:{layer_index}:{frame}:{}", el.id),
                    label,
                    breadcrumb_leaf: res_name.to_owned(),
                    icon: i16::BITMAP,
                    target: MovieTarget::Element {
                        symbol: symbol_id.to_owned(),
                        layer_index,
                        frame,
                        element_id: el.id.clone(),
                        instance_symbol: None,
                    },
                });
            }
        }
        ElementKind::Video { width, height } if cats[CAT_MEDIA] => {
            let dims = format!("Video ({}×{})", width.round() as i32, height.round() as i32);
            let label = if name_part.is_empty() {
                dims.clone()
            } else {
                format!("{name_part}  ·  {dims}")
            };
            if matches_query(&label, &dims) {
                out.push(LeafEntry {
                    key: format!("el:{symbol_id}:{layer_index}:{frame}:{}", el.id),
                    label,
                    breadcrumb_leaf: if name_part.is_empty() {
                        dims
                    } else {
                        name_part.to_owned()
                    },
                    icon: i16::VIDEO,
                    target: MovieTarget::Element {
                        symbol: symbol_id.to_owned(),
                        layer_index,
                        frame,
                        element_id: el.id.clone(),
                        instance_symbol: None,
                    },
                });
            }
        }
        _ => {}
    }

    if cats[CAT_SCRIPTS] {
        for os in &el.scripts {
            if os.script.source.trim().is_empty() {
                continue;
            }
            let target_label = if name_part.is_empty() {
                "object"
            } else {
                name_part
            };
            let label = format!("Actions for {target_label} ({:?})", os.event);
            if matches_query(&label, &os.script.source) {
                out.push(LeafEntry {
                    key: format!(
                        "os:{symbol_id}:{layer_index}:{frame}:{}:{:?}",
                        el.id, os.event
                    ),
                    label: label.clone(),
                    breadcrumb_leaf: label,
                    icon: i16::CODE,
                    target: MovieTarget::Script(ScriptLocation::Object {
                        symbol: symbol_id.to_owned(),
                        frame,
                        element: el.id.clone(),
                        event: os.event,
                    }),
                });
            }
        }
    }
}

fn push_symbol_rows(
    editor: &Editor,
    sym: &Symbol,
    base_depth: usize,
    bc_prefix: &str,
    query: &str,
    rows: &mut Vec<MovieRow>,
) {
    let state = &editor.movie_explorer;
    let cats = &state.show_categories;
    let filtering = !query.is_empty();

    if state.show_frames_and_layers {
        // Choose which layer hosts frame scripts when structured view is enabled:
        // prefer an "actions" layer, otherwise the topmost layer.
        let top_layer_idx = sym.timeline.layers.len().checked_sub(1);
        let script_layer_idx = sym
            .timeline
            .layers
            .iter()
            .enumerate()
            .rev()
            .find(|(_, l)| l.name.eq_ignore_ascii_case("actions"))
            .map(|(i, _)| i)
            .or(top_layer_idx);

        for (layer_idx, layer) in sym.timeline.layers.iter().enumerate().rev() {
            let layer_key = format!("layer:{}:{}", sym.id, layer.id);
            let layer_bc = format!("{bc_prefix} › {}", layer.name);
            let mut frame_numbers: Vec<u32> = layer.keyframes.iter().map(|k| k.frame).collect();
            if script_layer_idx == Some(layer_idx) && cats[CAT_SCRIPTS] {
                for fs in &sym.timeline.scripts {
                    if !fs.script.source.trim().is_empty() && !frame_numbers.contains(&fs.frame) {
                        frame_numbers.push(fs.frame);
                    }
                }
                frame_numbers.sort_unstable();
            }

            let mut frame_blocks: Vec<(u32, String, String, Vec<LeafEntry>)> = Vec::new();
            for frame_num in frame_numbers {
                let key_opt = layer.keyframes.iter().find(|k| k.frame == frame_num);
                let frame_label = match key_opt.map(|k| k.label.trim()).filter(|l| !l.is_empty()) {
                    Some(lbl) => format!("Frame {} ({lbl:?})", frame_num + 1),
                    None => format!("Frame {}", frame_num + 1),
                };
                let frame_bc = format!("{layer_bc} › Frame {}", frame_num + 1);
                let mut leaves = Vec::new();
                if let Some(key) = key_opt {
                    for el in &key.elements {
                        collect_element_leaves(
                            editor,
                            &sym.id,
                            layer_idx,
                            key.frame,
                            el,
                            cats,
                            query,
                            &mut leaves,
                        );
                    }
                    if cats[CAT_MEDIA]
                        && let Some(sound) = &key.sound
                    {
                        let res_name = editor
                            .project
                            .resources
                            .iter()
                            .find(|r| r.id == sound.resource)
                            .map_or(sound.resource.as_str(), |r| r.path.as_str());
                        let label = format!("Sound  ·  {res_name}");
                        if !filtering
                            || label.to_ascii_lowercase().contains(query)
                            || res_name.to_ascii_lowercase().contains(query)
                        {
                            leaves.push(LeafEntry {
                                key: format!("snd:{}:{layer_idx}:{}", sym.id, key.frame),
                                label,
                                breadcrumb_leaf: res_name.to_owned(),
                                icon: i16::SOUND,
                                target: MovieTarget::Frame {
                                    symbol: sym.id.clone(),
                                    layer_index: Some(layer_idx),
                                    frame: key.frame,
                                },
                            });
                        }
                    }
                }
                if script_layer_idx == Some(layer_idx)
                    && cats[CAT_SCRIPTS]
                    && let Some(fs) = sym
                        .timeline
                        .scripts
                        .iter()
                        .find(|fs| fs.frame == frame_num && !fs.script.source.trim().is_empty())
                {
                    let label = format!("Actions for Frame {}", fs.frame + 1);
                    if !filtering
                        || label.to_ascii_lowercase().contains(query)
                        || fs.script.source.to_ascii_lowercase().contains(query)
                    {
                        leaves.push(LeafEntry {
                            key: format!("fs:{}:{}", sym.id, fs.frame),
                            label: label.clone(),
                            breadcrumb_leaf: label,
                            icon: i16::CODE,
                            target: MovieTarget::Script(ScriptLocation::Frame {
                                symbol: sym.id.clone(),
                                frame: fs.frame,
                            }),
                        });
                    }
                }

                let frame_matches = !filtering || frame_label.to_ascii_lowercase().contains(query);
                if !leaves.is_empty() || !filtering || frame_matches {
                    frame_blocks.push((frame_num, frame_label, frame_bc, leaves));
                }
            }

            let layer_label = format!("Layer {:?}", layer.name);
            let layer_matches = !filtering || layer_label.to_ascii_lowercase().contains(query);
            if filtering && frame_blocks.is_empty() && !layer_matches {
                continue;
            }

            let layer_open = filtering || !state.collapsed.contains(&layer_key);
            rows.push(MovieRow {
                key: layer_key,
                label: layer_label,
                breadcrumb: layer_bc,
                depth: base_depth,
                is_folder: true,
                is_open: layer_open,
                leaf_icon: None,
                target: MovieTarget::Layer {
                    symbol: sym.id.clone(),
                    layer_index: layer_idx,
                },
            });

            if layer_open {
                for (frame_num, frame_label, frame_bc, leaves) in frame_blocks {
                    let frame_key = format!("frame:{}:{}:{frame_num}", sym.id, layer.id);
                    let frame_open = filtering || !state.collapsed.contains(&frame_key);
                    rows.push(MovieRow {
                        key: frame_key,
                        label: frame_label,
                        breadcrumb: frame_bc.clone(),
                        depth: base_depth + 1,
                        is_folder: true,
                        is_open: frame_open,
                        leaf_icon: None,
                        target: MovieTarget::Frame {
                            symbol: sym.id.clone(),
                            layer_index: Some(layer_idx),
                            frame: frame_num,
                        },
                    });
                    if frame_open {
                        for leaf in leaves {
                            rows.push(MovieRow {
                                key: leaf.key,
                                label: leaf.label,
                                breadcrumb: format!("{frame_bc} › {}", leaf.breadcrumb_leaf),
                                depth: base_depth + 2,
                                is_folder: false,
                                is_open: false,
                                leaf_icon: Some(leaf.icon),
                                target: leaf.target,
                            });
                        }
                    }
                }
            }
        }
    } else {
        // Flat mode (`show_frames_and_layers == false`): list matching items directly under symbol/scene.
        for (layer_idx, layer) in sym.timeline.layers.iter().enumerate().rev() {
            for key in &layer.keyframes {
                let mut leaves = Vec::new();
                for el in &key.elements {
                    collect_element_leaves(
                        editor,
                        &sym.id,
                        layer_idx,
                        key.frame,
                        el,
                        cats,
                        query,
                        &mut leaves,
                    );
                }
                for leaf in leaves {
                    rows.push(MovieRow {
                        key: leaf.key,
                        label: leaf.label,
                        breadcrumb: format!(
                            "{bc_prefix} › {} › Frame {} › {}",
                            layer.name,
                            key.frame + 1,
                            leaf.breadcrumb_leaf
                        ),
                        depth: base_depth,
                        is_folder: false,
                        is_open: false,
                        leaf_icon: Some(leaf.icon),
                        target: leaf.target,
                    });
                }
            }
        }
        if cats[CAT_SCRIPTS] {
            for fs in &sym.timeline.scripts {
                if fs.script.source.trim().is_empty() {
                    continue;
                }
                let label = format!("Actions for Frame {}", fs.frame + 1);
                if !filtering
                    || label.to_ascii_lowercase().contains(query)
                    || fs.script.source.to_ascii_lowercase().contains(query)
                {
                    rows.push(MovieRow {
                        key: format!("fs:{}:{}", sym.id, fs.frame),
                        label: label.clone(),
                        breadcrumb: format!("{bc_prefix} › Frame {} › {label}", fs.frame + 1),
                        depth: base_depth,
                        is_folder: false,
                        is_open: false,
                        leaf_icon: Some(i16::CODE),
                        target: MovieTarget::Script(ScriptLocation::Frame {
                            symbol: sym.id.clone(),
                            frame: fs.frame,
                        }),
                    });
                }
            }
        }
    }
}

pub(crate) fn build_movie_explorer_rows(editor: &Editor) -> Vec<MovieRow> {
    let mut rows = Vec::new();
    let state = &editor.movie_explorer;
    let query = state.query.trim().to_ascii_lowercase();
    let filtering = !query.is_empty();

    let doc_label = editor
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map_or_else(|| format!("{}.nflash", editor.project.name), str::to_owned);
    let doc_key = "doc".to_owned();
    let doc_open = filtering || !state.collapsed.contains(&doc_key);
    rows.push(MovieRow {
        key: doc_key,
        label: doc_label.clone(),
        breadcrumb: doc_label,
        depth: 0,
        is_folder: true,
        is_open: doc_open,
        leaf_icon: None,
        target: MovieTarget::Document,
    });
    if !doc_open {
        return rows;
    }

    for scene_id in editor.project.scene_ids() {
        let Some(scene_sym) = editor.project.symbol(&scene_id) else {
            continue;
        };
        let scene_key = format!("scene:{scene_id}");
        let scene_open = filtering || !state.collapsed.contains(&scene_key);
        let mut child_rows = Vec::new();
        if scene_open {
            push_symbol_rows(
                editor,
                scene_sym,
                2,
                &scene_sym.name,
                &query,
                &mut child_rows,
            );
        }
        if filtering
            && child_rows.is_empty()
            && !scene_sym.name.to_ascii_lowercase().contains(&query)
        {
            continue;
        }
        rows.push(MovieRow {
            key: scene_key,
            label: scene_sym.name.clone(),
            breadcrumb: scene_sym.name.clone(),
            depth: 1,
            is_folder: true,
            is_open: scene_open,
            leaf_icon: None,
            target: MovieTarget::Timeline {
                symbol: scene_id.clone(),
            },
        });
        rows.extend(child_rows);
    }

    if state.show_symbol_definitions {
        let library_symbols: Vec<&Symbol> = editor
            .project
            .symbols
            .iter()
            .filter(|s| !editor.project.is_scene(&s.id))
            .collect();
        if !library_symbols.is_empty() {
            let defs_key = "symbol-defs".to_owned();
            let defs_open = filtering || !state.collapsed.contains(&defs_key);
            let mut def_rows = Vec::new();
            if defs_open {
                for sym in library_symbols {
                    let sym_key = format!("def:{}", sym.id);
                    let sym_open = filtering || !state.collapsed.contains(&sym_key);
                    let bc = format!("Symbol definitions › {}", sym.name);
                    let mut sub = Vec::new();
                    if sym_open {
                        push_symbol_rows(editor, sym, 3, &bc, &query, &mut sub);
                    }
                    if filtering
                        && sub.is_empty()
                        && !sym.name.to_ascii_lowercase().contains(&query)
                    {
                        continue;
                    }
                    def_rows.push(MovieRow {
                        key: sym_key,
                        label: sym.name.clone(),
                        breadcrumb: bc,
                        depth: 2,
                        is_folder: true,
                        is_open: sym_open,
                        leaf_icon: None,
                        target: MovieTarget::Timeline {
                            symbol: sym.id.clone(),
                        },
                    });
                    def_rows.extend(sub);
                }
            }
            if !filtering || !def_rows.is_empty() {
                rows.push(MovieRow {
                    key: defs_key,
                    label: "Symbol definitions".to_owned(),
                    breadcrumb: "Symbol definitions".to_owned(),
                    depth: 1,
                    is_folder: true,
                    is_open: defs_open,
                    leaf_icon: None,
                    target: MovieTarget::Folder,
                });
                rows.extend(def_rows);
            }
        }
    }

    if state.show_categories[CAT_SCRIPTS] && !editor.project.sources.is_empty() {
        let src_key = "project-sources".to_owned();
        let src_open = filtering || !state.collapsed.contains(&src_key);
        let mut src_rows = Vec::new();
        if src_open {
            for unit in &editor.project.sources {
                if filtering
                    && !unit.path.to_ascii_lowercase().contains(&query)
                    && !unit.source.to_ascii_lowercase().contains(&query)
                {
                    continue;
                }
                src_rows.push(MovieRow {
                    key: format!("src:{}", unit.id),
                    label: unit.path.clone(),
                    breadcrumb: format!("Project files › {}", unit.path),
                    depth: 2,
                    is_folder: false,
                    is_open: false,
                    leaf_icon: Some(i16::CODE),
                    target: MovieTarget::Script(ScriptLocation::Source(unit.id.clone())),
                });
            }
        }
        if !filtering || !src_rows.is_empty() {
            rows.push(MovieRow {
                key: src_key,
                label: "Project files".to_owned(),
                breadcrumb: "Project files".to_owned(),
                depth: 1,
                is_folder: true,
                is_open: src_open,
                leaf_icon: None,
                target: MovieTarget::Folder,
            });
            rows.extend(src_rows);
        }
    }

    rows
}

impl Editor {
    fn enter_explorer_timeline(&mut self, symbol_id: &str) {
        self.finish_edits();
        if self.symbol_id() != symbol_id {
            if self.project.is_scene(symbol_id) {
                self.switch_scene(symbol_id);
            } else {
                self.leave_to_level(0);
                if symbol_id != self.project.root {
                    self.enter_symbol(symbol_id.to_owned());
                }
            }
        }
    }

    pub(crate) fn navigate_movie_explorer_target(
        &mut self,
        target: &MovieTarget,
        double_click: bool,
    ) {
        match target {
            MovieTarget::Document | MovieTarget::Folder => {}
            MovieTarget::Timeline { symbol } => {
                self.enter_explorer_timeline(symbol);
            }
            MovieTarget::Layer {
                symbol,
                layer_index,
            } => {
                self.enter_explorer_timeline(symbol);
                if *layer_index < self.symbol().timeline.layers.len() {
                    self.layer = *layer_index;
                }
                self.normalize_selection();
            }
            MovieTarget::Frame {
                symbol,
                layer_index,
                frame,
            } => {
                self.enter_explorer_timeline(symbol);
                if let Some(li) = layer_index
                    && *li < self.symbol().timeline.layers.len()
                {
                    self.layer = *li;
                }
                self.set_playing(false);
                self.frame = *frame;
                self.normalize_selection();
            }
            MovieTarget::Element {
                symbol,
                layer_index,
                frame,
                element_id,
                instance_symbol,
            } => {
                self.enter_explorer_timeline(symbol);
                if *layer_index < self.symbol().timeline.layers.len() {
                    self.layer = *layer_index;
                }
                self.set_playing(false);
                self.frame = *frame;
                self.select_single_id(element_id.clone());
                if double_click
                    && let Some(inst_sym) = instance_symbol
                    && self.project.symbol(inst_sym).is_some()
                {
                    self.enter_symbol(inst_sym.clone());
                }
            }
            MovieTarget::Script(loc) => {
                self.open_script_location(loc.clone());
                self.open_actions();
            }
        }
    }

    fn default_explorer_breadcrumb(&self) -> String {
        let sym_name = &self.symbol().name;
        let layer_name = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .map_or("Layer 1", |l| l.name.as_str());
        if let Some(el) = self.selected_element()
            && !el.name.trim().is_empty()
        {
            format!(
                "{sym_name} › {layer_name} › Frame {} › {}",
                self.frame + 1,
                el.name.trim()
            )
        } else {
            format!("{sym_name} › {layer_name} › Frame {}", self.frame + 1)
        }
    }

    pub(crate) fn movie_explorer_panel_ui(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = space::S1;

        let active_solo = {
            let cats = self.movie_explorer.show_categories;
            let count = cats.iter().filter(|&&b| b).count();
            if count == 1 {
                cats.iter().position(|&b| b)
            } else {
                None
            }
        };

        let mut open_customise_popup = false;
        let bar_resp = ui
            .horizontal(|ui| {
                if let Some(clicked) = (c::IconGroup {
                    items: &[
                        (i16::FONT, "Show text"),
                        (i16::MOVIECLIP, "Show buttons, movie clips and graphics"),
                        (i16::CODE, "Show ActionScript"),
                        (i16::VIDEO, "Show video, sound and bitmaps"),
                    ],
                    selected: active_solo,
                })
                .show(ui)
                {
                    if active_solo == Some(clicked) {
                        self.movie_explorer.show_categories = [true, true, true, true];
                    } else {
                        let mut next = [false; 4];
                        next[clicked] = true;
                        self.movie_explorer.show_categories = next;
                    }
                }
                if let Some(clicked) = (c::IconGroup {
                    items: &[
                        (i16::FRAME, "Show frames and layers"),
                        (i16::SETTINGS, "Customise items to show"),
                    ],
                    selected: self.movie_explorer.show_frames_and_layers.then_some(0),
                })
                .show(ui)
                {
                    match clicked {
                        0 => {
                            self.movie_explorer.show_frames_and_layers =
                                !self.movie_explorer.show_frames_and_layers;
                        }
                        1 => {
                            open_customise_popup = true;
                        }
                        _ => {}
                    }
                }
            })
            .response;

        let popup_id = ui.id().with("movie-explorer-customise-popup");
        if open_customise_popup {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        egui::Popup::new(
            popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::ParentRect(bar_resp.rect),
            ui.layer_id(),
        )
        .frame(c::popover_frame(ui.ctx()))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(240.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            for (idx, label) in [
                (CAT_TEXT, "Show text"),
                (CAT_SYMBOLS, "Show buttons, movie clips and graphics"),
                (CAT_SCRIPTS, "Show ActionScript"),
                (CAT_MEDIA, "Show video, sound and bitmaps"),
            ] {
                if ui
                    .add(c::MenuItem::new(label).checked(self.movie_explorer.show_categories[idx]))
                    .clicked()
                {
                    self.movie_explorer.show_categories[idx] =
                        !self.movie_explorer.show_categories[idx];
                }
            }
            c::menu_separator(ui);
            if ui
                .add(
                    c::MenuItem::new("Show frames and layers")
                        .checked(self.movie_explorer.show_frames_and_layers),
                )
                .clicked()
            {
                self.movie_explorer.show_frames_and_layers =
                    !self.movie_explorer.show_frames_and_layers;
            }
            if ui
                .add(
                    c::MenuItem::new("Show symbol definitions")
                        .checked(self.movie_explorer.show_symbol_definitions),
                )
                .clicked()
            {
                self.movie_explorer.show_symbol_definitions =
                    !self.movie_explorer.show_symbol_definitions;
            }
            c::menu_separator(ui);
            if ui.add(c::MenuItem::new("Show all items")).clicked() {
                self.movie_explorer.show_categories = [true, true, true, true];
                self.movie_explorer.show_frames_and_layers = true;
                self.movie_explorer.show_symbol_definitions = true;
                ui.close();
            }
        });

        let w = ui.available_width();
        ui.add(c::Search::new(&mut self.movie_explorer.query, "Find in movie").width(w));

        let rows = build_movie_explorer_rows(self);
        let filtering = !self.movie_explorer.query.trim().is_empty();
        let mut toggle_folder: Option<String> = None;
        let mut clicked_nav: Option<(String, String, MovieTarget, bool)> = None;

        let tree_h = (ui.available_height() - 24.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("movie-explorer-tree")
            .max_height(tree_h)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                for row in &rows {
                    let kind = if row.is_folder {
                        if row.is_open {
                            c::TreeItemType::FolderOpen
                        } else {
                            c::TreeItemType::FolderClosed
                        }
                    } else {
                        c::TreeItemType::Symbol(row.leaf_icon.unwrap_or(i16::MOVIECLIP))
                    };
                    let is_selected =
                        self.movie_explorer.selected_key.as_deref() == Some(row.key.as_str());
                    let resp = c::TreeItem::new(&row.label, kind)
                        .depth(row.depth)
                        .selected(is_selected)
                        .show(ui);
                    if row.is_folder
                        && !filtering
                        && (resp.toggle
                            || (row.target == MovieTarget::Folder && resp.row.clicked()))
                    {
                        toggle_folder = Some(row.key.clone());
                    }
                    if resp.row.double_clicked() {
                        if row.is_folder && !filtering && row.target != MovieTarget::Folder {
                            toggle_folder = Some(row.key.clone());
                        }
                        clicked_nav = Some((
                            row.key.clone(),
                            row.breadcrumb.clone(),
                            row.target.clone(),
                            true,
                        ));
                    } else if resp.row.clicked() {
                        clicked_nav = Some((
                            row.key.clone(),
                            row.breadcrumb.clone(),
                            row.target.clone(),
                            false,
                        ));
                    }
                }
            });

        if let Some(key) = toggle_folder
            && !self.movie_explorer.collapsed.remove(&key)
        {
            self.movie_explorer.collapsed.insert(key);
        }
        if let Some((key, bc, target, dbl)) = clicked_nav {
            self.movie_explorer.selected_key = Some(key);
            self.movie_explorer.selected_breadcrumb = Some(bc);
            self.navigate_movie_explorer_target(&target, dbl);
        }

        let breadcrumb = self
            .movie_explorer
            .selected_key
            .as_deref()
            .and_then(|sel| rows.iter().find(|r| r.key == sel))
            .map(|r| r.breadcrumb.clone())
            .or_else(|| self.movie_explorer.selected_breadcrumb.clone())
            .unwrap_or_else(|| self.default_explorer_breadcrumb());
        mono(ui, &breadcrumb);
    }
}
