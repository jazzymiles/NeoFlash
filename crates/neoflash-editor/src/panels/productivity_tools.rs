//! Native Built-In Productivity & Animation Suite (`T-196`):
//! - **Library Batch & Cleanup Suite**:
//!   - `Batch Rename Library Items…` (`batch_rename_library_items`)
//!   - `Select Unused Library Items` & `Delete Unused Library Items` (`find_and_delete_unused_library_items`)
//!   - `Batch Set Linkage / Export for ActionScript…` (`batch_set_library_linkage`)
//!   - `Batch Set Bitmap Compression & Smoothing…` (`batch_set_bitmap_compression_and_smoothing`)
//! - **Justin Putney (Ajar Productions) Stage Productivity Dialogs**:
//!   - `Auto Instance Namer…` (`auto_name_stage_instances`)
//!   - `Randomize Selection…` (`randomize_selected_elements`)
//! - **NF2 Modal Host (`show_productivity_modals`)**:
//!   - Built exclusively with NF2 design system components (`NeoModal`, `SectionHeader`, `Field`,
//!     `Dropdown`, `Checkbox`, `Button`) and semantic palette tokens (`R-068`).

use super::dialog_helpers::group;
use crate::app::Editor;
use crate::theme::DesignUi as _;
use eframe::egui::{self, Ui};
use neoflash_core::{
    BatchRenameOptions, BatchRenameReport, BatchRenameScope, BitmapCompression,
    BitmapExportSettings, ElementKind, Id, InstancePlayback, Project, SymbolKind,
    VALID_JPEG_QUALITY_RANGE, batch_rename_library_and_instances,
};
use neoflash_ui::components::{
    self as c, Button, Checkbox, DialogHeader, Field, MenuItem, SectionHeader, Surface, TextField,
    surface::dialog_footer,
};
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// NF2 dropdown alias wrapping [`neoflash_ui::components::Select`].
pub type Dropdown<'a> = c::Select<'a>;

/// NF2-compliant modal dialog wrapper combining `Surface::Dialog`, `DialogHeader`, and `dialog_footer` (`R-068`).
pub struct NeoModal<'a> {
    pub id: egui::Id,
    pub title: &'a str,
    pub width: f32,
}

impl<'a> NeoModal<'a> {
    pub fn new(id_salt: impl std::hash::Hash, title: &'a str) -> Self {
        Self {
            id: egui::Id::new(id_salt),
            title,
            width: 480.0,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Show the NF2 modal dialog and return `(confirmed, cancelled)`.
    pub fn show(
        self,
        ctx: &egui::Context,
        confirm_label: &str,
        can_confirm: bool,
        body: impl FnOnce(&mut Ui),
    ) -> (bool, bool) {
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_ok = false;
        let width = self.width;
        let title = self.title;

        let modal = egui::Modal::new(self.id)
            .frame(Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(width);
                let header_top = ui.cursor().min;
                close_cancel |= DialogHeader { title }.show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + width,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: space::S5 as i8,
                        right: space::S5 as i8,
                        top: space::S4 as i8,
                        bottom: space::S3 as i8,
                    })
                    .show(ui, |ui| {
                        let inner_w = (width - 2.0 * space::S5).max(120.0);
                        ui.set_width(inner_w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                        body(ui);
                    });
                dialog_footer(
                    ui,
                    |_| {},
                    |ui| {
                        close_ok |= ui
                            .add_enabled_semantic(
                                can_confirm,
                                Button::primary(confirm_label).shortcut("↵"),
                            )
                            .clicked();
                        close_cancel |= ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
                    },
                );
            });

        if !close_cancel && !close_ok {
            let typing = ctx.wants_keyboard_input();
            let popup_open = egui::Popup::is_any_open(ctx);
            if !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                close_cancel = true;
            } else if !typing
                && !popup_open
                && can_confirm
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                close_ok = true;
            } else if modal.should_close() {
                close_cancel = true;
            }
        }

        (close_ok, close_cancel)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Library Batch & Cleanup Configuration & Reports
// ─────────────────────────────────────────────────────────────────────────────

/// Target scope for `Batch Rename Library Items…`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchRenameTarget {
    /// Selected Library symbol, resource, or folder (falls back to all symbols if none selected).
    #[default]
    Selected,
    /// All symbols inside a specific Library folder path (and its subfolders).
    Folder(String),
    /// All non-scene symbols in the active Library.
    AllSymbols,
    /// All non-scene symbols and user resources in the active Library.
    AllItems,
    /// Explicit list of symbol IDs.
    ExplicitSymbols(Vec<Id>),
}

impl BatchRenameTarget {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Selected => "Selected items / folder",
            Self::Folder(_) => "Current folder",
            Self::AllSymbols => "All library symbols",
            Self::AllItems => "All symbols & resources",
            Self::ExplicitSymbols(_) => "Selected symbol set",
        }
    }
}

/// Configuration for `Batch Rename Library Items…`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBatchRenameConfig {
    pub target: BatchRenameTarget,
    pub prefix: String,
    pub suffix: String,
    pub find: String,
    pub replace: String,
    pub use_numbering: bool,
    pub start_number: u32,
    pub pad_digits: usize,
    pub also_rename_stage_instances: bool,
}

impl Default for LibraryBatchRenameConfig {
    fn default() -> Self {
        Self {
            target: BatchRenameTarget::AllSymbols,
            prefix: String::new(),
            suffix: String::new(),
            find: String::new(),
            replace: String::new(),
            use_numbering: false,
            start_number: 1,
            pad_digits: 2,
            also_rename_stage_instances: true,
        }
    }
}

/// Summary of unused Library items discovered across a [`Project`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnusedLibraryReport {
    pub unused_symbol_ids: Vec<Id>,
    pub unused_symbol_names: Vec<String>,
    pub unused_resource_ids: Vec<Id>,
    pub unused_resource_paths: Vec<String>,
}

impl UnusedLibraryReport {
    #[allow(dead_code)]
    pub fn total_unused(&self) -> usize {
        self.unused_symbol_ids.len() + self.unused_resource_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.unused_symbol_ids.is_empty() && self.unused_resource_ids.is_empty()
    }
}

/// Optional per-resource ActionScript linkage record stored portably when audio resources are batch-linked.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLinkageEntry {
    pub resource_id: Id,
    pub export_for_actionscript: bool,
    pub export_in_first_frame: bool,
    pub identifier: String,
    pub class_name: String,
}

/// Configuration for `Batch Set Linkage / Export for ActionScript…`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBatchLinkageConfig {
    /// Optional explicit symbol/resource IDs; `None` targets all non-scene symbols (and audio resources when `include_sounds` is true).
    pub target_ids: Option<Vec<Id>>,
    pub class_prefix: String,
    pub export_for_actionscript: bool,
    pub export_in_first_frame: bool,
    pub include_sounds: bool,
}

impl Default for LibraryBatchLinkageConfig {
    fn default() -> Self {
        Self {
            target_ids: None,
            class_prefix: "app.assets.".to_owned(),
            export_for_actionscript: true,
            export_in_first_frame: true,
            include_sounds: true,
        }
    }
}

/// Per-item outcome from `batch_set_library_linkage`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchLinkageItem {
    pub item_id: Id,
    pub export_for_actionscript: bool,
    pub export_in_first_frame: bool,
    pub identifier: String,
    pub class_name: String,
}

/// Summary report from `batch_set_library_linkage`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchLinkageReport {
    pub items: Vec<BatchLinkageItem>,
}

impl BatchLinkageReport {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Configuration for `Batch Set Bitmap Compression & Smoothing…`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBatchBitmapConfig {
    /// Optional explicit image resource IDs; `None` targets all `image/*` resources in the Library.
    pub target_resource_ids: Option<Vec<Id>>,
    pub compression: BitmapCompression,
    pub jpeg_quality: u8,
    pub use_imported_jpeg_data: bool,
    pub allow_smoothing: bool,
}

impl Default for LibraryBatchBitmapConfig {
    fn default() -> Self {
        Self {
            target_resource_ids: None,
            compression: BitmapCompression::PhotoJpeg,
            jpeg_quality: 80,
            use_imported_jpeg_data: false,
            allow_smoothing: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Justin Putney Stage Productivity Configurations
// ─────────────────────────────────────────────────────────────────────────────

/// Naming pattern for `Auto Instance Namer…` (`auto_name_stage_instances`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoNameMode {
    /// `<prefix><index>` (e.g. `item_0`, `item_1` or `clip_01`, `clip_02`).
    #[default]
    PrefixSequential,
    /// `<sanitized_symbol_name>_<index>` (e.g. `hero_btn_1`, `hero_btn_2`).
    SymbolDerived,
}

impl AutoNameMode {
    pub const ALL: [Self; 2] = [Self::PrefixSequential, Self::SymbolDerived];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PrefixSequential => "Prefix + sequence (<prefix><index>)",
            Self::SymbolDerived => "Symbol name + sequence (<symbol>_<index>)",
        }
    }
}

/// Configuration for `Auto Instance Namer…` (`auto_name_stage_instances`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoInstanceNamerConfig {
    pub mode: AutoNameMode,
    pub prefix: String,
    pub start_index: u32,
    pub step: u32,
    pub pad_digits: usize,
    /// When no instances are explicitly selected on Stage, only rename instances whose `name` is currently empty.
    pub only_unnamed_when_unselected: bool,
}

impl Default for AutoInstanceNamerConfig {
    fn default() -> Self {
        Self {
            mode: AutoNameMode::PrefixSequential,
            prefix: "item_".to_owned(),
            start_index: 0,
            step: 1,
            pad_digits: 0,
            only_unnamed_when_unselected: true,
        }
    }
}

/// Configuration for `Randomize Selection…` (`randomize_selected_elements`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RandomizeSelectionConfig {
    pub seed: u64,
    pub offset_x_range: [f64; 2],
    pub offset_y_range: [f64; 2],
    pub scale_range_pct: [f64; 2],
    pub uniform_scale: bool,
    pub rotation_range_deg: [f64; 2],
    pub alpha_range_pct: [f64; 2],
}

impl Default for RandomizeSelectionConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            offset_x_range: [-20.0, 20.0],
            offset_y_range: [-20.0, 20.0],
            scale_range_pct: [80.0, 120.0],
            uniform_scale: true,
            rotation_range_deg: [-15.0, 15.0],
            alpha_range_pct: [100.0, 100.0],
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. String Sanitization, Unused Scanning & Deterministic PRNG Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Internal system resource paths that store portable document settings rather than user media.
pub(crate) fn is_internal_system_resource_path(path: &str) -> bool {
    path.eq_ignore_ascii_case(Project::SWATCHES_RESOURCE_PATH)
        || path.eq_ignore_ascii_case(Project::GLOBAL_SWATCHES_RESOURCE_PATH)
        || path.eq_ignore_ascii_case(Project::VECTOR_STYLES_RESOURCE_PATH)
        || path.eq_ignore_ascii_case(Project::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
        || path.eq_ignore_ascii_case(Project::SHARED_LIBRARIES_RESOURCE_PATH)
        || path.eq_ignore_ascii_case("fonts/embedding.json")
}

/// Convert an arbitrary symbol or resource name into a valid ActionScript `PascalCase` class identifier.
pub fn sanitize_pascal_case(raw: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    let mut prev_was_lower = false;

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize_next || (prev_was_lower && ch.is_ascii_uppercase()) {
                out.push(ch.to_ascii_uppercase());
            } else {
                out.push(ch.to_ascii_lowercase());
            }
            prev_was_lower = ch.is_ascii_lowercase();
            capitalize_next = false;
        } else {
            capitalize_next = true;
            prev_was_lower = false;
        }
    }

    if out.is_empty() {
        return "ExportedAsset".to_owned();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert_str(0, "Asset");
    }
    out
}

/// Convert an arbitrary symbol name into a clean lowercase `snake_case` instance prefix.
pub fn sanitize_instance_stem(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !out.is_empty() && !prev_underscore {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_underscore = false;
        } else if !out.is_empty() && !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        return "instance".to_owned();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert_str(0, "inst_");
    }
    out
}

/// Sanitize a dotted ActionScript package prefix (e.g. `"app.assets."` -> `"app.assets"`).
pub fn sanitize_class_prefix(prefix: &str) -> String {
    let mut parts = Vec::new();
    for raw_part in prefix.trim().trim_matches('.').split('.') {
        let mut clean: String = raw_part
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if clean.is_empty() {
            continue;
        }
        if clean.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            clean.insert(0, '_');
        }
        parts.push(clean);
    }
    parts.join(".")
}

/// True when a symbol has runtime ActionScript linkage, class binding, or shared library export/import.
pub fn symbol_has_linkage(project: &Project, symbol_id: &str) -> bool {
    let Some(sym) = project.symbol(symbol_id) else {
        return false;
    };
    if sym
        .linkage_id
        .as_ref()
        .is_some_and(|id| !id.trim().is_empty())
    {
        return true;
    }
    if sym
        .class_name
        .as_ref()
        .is_some_and(|cl| !cl.trim().is_empty())
    {
        return true;
    }
    if project.shared_library(symbol_id).is_some_and(|sh| {
        sh.export_for_runtime || sh.import_for_runtime || !sh.url.trim().is_empty()
    }) {
        return true;
    }
    false
}

/// Scan `project` for unused non-root, non-scene symbols and unused user resources.
///
/// Preserves:
/// - `project.root` and all `project.scenes`
/// - Any symbol with ActionScript linkage (`linkage_id`), `class_name`, or `SharedLibraryLinkage`
/// - Any symbol instantiated via `ElementKind::Instance` across used symbols/scenes
/// - Any resource referenced by `ElementKind::Bitmap`, `Keyframe.bitmap_fills`, `Keyframe.sound`,
///   or `Keyframe.video` across used symbols/scenes, or internal metadata resources.
pub fn scan_unused_library_items(project: &Project) -> UnusedLibraryReport {
    let scene_set: HashSet<Id> = project.scene_ids().into_iter().collect();

    // Seed reachable symbols with all scenes (including root) and all linkage-exported symbols.
    let mut reachable_symbols: HashSet<Id> = HashSet::new();
    let mut stack: Vec<Id> = Vec::new();
    for sym in &project.symbols {
        if (scene_set.contains(&sym.id) || symbol_has_linkage(project, &sym.id))
            && reachable_symbols.insert(sym.id.clone())
        {
            stack.push(sym.id.clone());
        }
    }

    // Also count direct `Instance` references across all symbols so a symbol referenced by any
    // non-deleted symbol is preserved.
    while let Some(sym_id) = stack.pop() {
        let Some(sym) = project.symbol(&sym_id) else {
            continue;
        };
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind
                        && reachable_symbols.insert(symbol_id.clone())
                    {
                        stack.push(symbol_id.clone());
                    }
                }
            }
        }
    }

    // Collect resources referenced by reachable symbols.
    let mut used_resources: HashSet<Id> = HashSet::new();
    for sym in &project.symbols {
        if !reachable_symbols.contains(&sym.id) {
            continue;
        }
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                if let Some(snd) = &key.sound {
                    used_resources.insert(snd.resource.clone());
                }
                if let Some(vid) = &key.video
                    && !vid.resource.is_empty()
                {
                    used_resources.insert(vid.resource.clone());
                }
                for bf in &key.bitmap_fills {
                    used_resources.insert(bf.resource_id.clone());
                }
                for el in &key.elements {
                    if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                        used_resources.insert(resource_id.clone());
                    }
                }
            }
        }
    }

    let mut report = UnusedLibraryReport::default();
    for sym in &project.symbols {
        if !scene_set.contains(&sym.id) && !reachable_symbols.contains(&sym.id) {
            report.unused_symbol_ids.push(sym.id.clone());
            report.unused_symbol_names.push(sym.name.clone());
        }
    }
    for res in &project.resources {
        if is_internal_system_resource_path(&res.path) {
            continue;
        }
        if !used_resources.contains(&res.id) {
            report.unused_resource_ids.push(res.id.clone());
            report.unused_resource_paths.push(res.path.clone());
        }
    }
    report
}

/// Deterministic SplitMix64 pseudo-random number generator.
#[derive(Clone, Copy, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f64` in `[0.0, 1.0]`.
    fn next_unit(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        (bits as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    /// Sample uniformly within `[range[0], range[1]]` (handles inverted bounds gracefully).
    fn sample_range(&mut self, range: [f64; 2]) -> f64 {
        let (lo, hi) = if range[0] <= range[1] {
            (range[0], range[1])
        } else {
            (range[1], range[0])
        };
        if (hi - lo).abs() <= 1e-12 {
            lo
        } else {
            lo + (hi - lo) * self.next_unit()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Editor Methods for Library Batch Suite & Stage Productivity
// ─────────────────────────────────────────────────────────────────────────────

impl Editor {
    fn resolve_target_symbol_ids(&self, target: &BatchRenameTarget) -> Option<Vec<Id>> {
        match target {
            BatchRenameTarget::AllSymbols | BatchRenameTarget::AllItems => None,
            BatchRenameTarget::ExplicitSymbols(ids) => Some(ids.clone()),
            BatchRenameTarget::Folder(folder) => {
                let prefix = format!("{folder}/");
                let ids: Vec<Id> = self
                    .project
                    .symbols
                    .iter()
                    .filter(|s| {
                        !self.project.is_scene(&s.id)
                            && (folder.is_empty()
                                || s.folder == *folder
                                || s.folder.starts_with(&prefix))
                    })
                    .map(|s| s.id.clone())
                    .collect();
                Some(ids)
            }
            BatchRenameTarget::Selected => {
                if let Some(sym_id) = &self.library_selected
                    && !self.project.is_scene(sym_id)
                {
                    Some(vec![sym_id.clone()])
                } else if !self.library_panel.selected_folder.is_empty() {
                    let folder = &self.library_panel.selected_folder;
                    let prefix = format!("{folder}/");
                    let ids: Vec<Id> = self
                        .project
                        .symbols
                        .iter()
                        .filter(|s| {
                            !self.project.is_scene(&s.id)
                                && (s.folder == *folder || s.folder.starts_with(&prefix))
                        })
                        .map(|s| s.id.clone())
                        .collect();
                    Some(ids)
                } else {
                    None
                }
            }
        }
    }

    /// Batch rename Library items (and optionally matching Stage instances) in one `"Batch rename library items"` Undo step.
    pub fn batch_rename_library_items(
        &mut self,
        config: &LibraryBatchRenameConfig,
    ) -> Result<BatchRenameReport, String> {
        self.finish_edits();
        let target_symbol_ids = self.resolve_target_symbol_ids(&config.target);
        let scope = if config.also_rename_stage_instances {
            BatchRenameScope::LibraryAndInstances
        } else {
            BatchRenameScope::LibraryItemsOnly
        };
        let options = BatchRenameOptions {
            scope,
            target_symbol_ids,
            name_filter: None,
            find: (!config.find.is_empty()).then(|| config.find.clone()),
            replace: config.replace.clone(),
            prefix: config.prefix.clone(),
            suffix: config.suffix.clone(),
            sequence_start: config.use_numbering.then_some(config.start_number),
            sequence_step: 1,
            sequence_digits: config.pad_digits.clamp(1, 8),
            sync_instance_names_to_symbol: config.also_rename_stage_instances,
        };

        let mut candidate = self.project.clone();
        let report =
            batch_rename_library_and_instances(&mut candidate, &options).map_err(|e| e.message)?;

        let mut resources_renamed = 0usize;
        if matches!(config.target, BatchRenameTarget::AllItems) {
            let mut occupied_lower: HashSet<String> = candidate
                .resources
                .iter()
                .map(|r| r.path.to_lowercase())
                .collect();
            for (idx, res) in candidate.resources.iter_mut().enumerate() {
                if is_internal_system_resource_path(&res.path) {
                    continue;
                }
                let path_obj = Path::new(&res.path);
                let parent = path_obj.parent().and_then(|p| p.to_str()).unwrap_or("");
                let stem = path_obj
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&res.path);
                let ext = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");
                let replaced = if !config.find.is_empty() {
                    stem.replace(&config.find, &config.replace)
                } else {
                    stem.to_owned()
                };
                let seq = if config.use_numbering {
                    let num = config.start_number.saturating_add(idx as u32);
                    let width = config.pad_digits.clamp(1, 8);
                    format!("{num:0width$}")
                } else {
                    String::new()
                };
                let new_stem = format!("{}{}{}{}", config.prefix, replaced, config.suffix, seq);
                let new_filename = if ext.is_empty() {
                    new_stem
                } else {
                    format!("{new_stem}.{ext}")
                };
                let new_path = if parent.is_empty() {
                    new_filename
                } else {
                    format!("{parent}/{new_filename}")
                };
                if new_path != res.path && !new_path.trim().is_empty() {
                    occupied_lower.remove(&res.path.to_lowercase());
                    if occupied_lower.insert(new_path.to_lowercase()) {
                        res.path = new_path;
                        resources_renamed += 1;
                    } else {
                        occupied_lower.insert(res.path.to_lowercase());
                    }
                }
            }
        }

        if !report.renamed_symbols.is_empty()
            || !report.renamed_instances.is_empty()
            || resources_renamed > 0
        {
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Batch rename library items", |editor| {
                editor.project = candidate;
            });
            self.resource_panel.baseline = None;
        }
        Ok(report)
    }

    /// Scan and delete all unused non-root, non-scene Library symbols (without linkage/class)
    /// and unused user resources in one atomic Undo step.
    pub fn find_and_delete_unused_library_items(&mut self) -> UnusedLibraryReport {
        self.finish_edits();
        let report = scan_unused_library_items(&self.project);
        if report.is_empty() {
            return report;
        }
        let unused_syms: HashSet<&str> = report
            .unused_symbol_ids
            .iter()
            .map(String::as_str)
            .collect();
        let unused_res: HashSet<&str> = report
            .unused_resource_ids
            .iter()
            .map(String::as_str)
            .collect();

        let mut candidate = self.project.clone();
        candidate
            .symbols
            .retain(|s| !unused_syms.contains(s.id.as_str()));
        candidate
            .resources
            .retain(|r| !unused_res.contains(r.id.as_str()));
        candidate.upgrade_format_for_features();

        if candidate.validate().is_ok() {
            let clear_sym = self
                .library_selected
                .as_deref()
                .is_some_and(|id| unused_syms.contains(id));
            let clear_res = self
                .resource_panel
                .selected
                .as_deref()
                .is_some_and(|id| unused_res.contains(id));
            self.transact_named("Delete unused items", |editor| {
                editor.project = candidate;
            });
            if clear_sym {
                self.library_selected = None;
            }
            if clear_res {
                self.resource_panel.selected = None;
            }
            self.message(format!(
                "Deleted {} unused symbol(s) and {} unused resource(s).",
                report.unused_symbol_ids.len(),
                report.unused_resource_ids.len()
            ));
            report
        } else {
            UnusedLibraryReport::default()
        }
    }

    /// Batch configure ActionScript linkage (`export_for_actionscript`, `export_in_first_frame`,
    /// `linkage_id`, and `class_name` = `<class_prefix><PascalCaseName>`) in one Undo step.
    pub fn batch_set_library_linkage(
        &mut self,
        config: &LibraryBatchLinkageConfig,
    ) -> Result<BatchLinkageReport, String> {
        self.finish_edits();
        let mut candidate = self.project.clone();
        let scene_ids: HashSet<Id> = candidate.scene_ids().into_iter().collect();
        let target_filter: Option<HashSet<Id>> = config
            .target_ids
            .as_ref()
            .map(|ids| ids.iter().cloned().collect());

        let clean_prefix = sanitize_class_prefix(&config.class_prefix);

        // Pre-seed occupied linkage IDs and class names from untargeted symbols so we never collide.
        let mut used_linkage_ids: HashSet<String> = HashSet::new();
        let mut used_class_names: HashSet<String> = HashSet::new();
        for sym in &candidate.symbols {
            let is_targeted = !scene_ids.contains(&sym.id)
                && target_filter
                    .as_ref()
                    .is_none_or(|set| set.contains(&sym.id));
            if !is_targeted {
                if let Some(l) = &sym.linkage_id {
                    used_linkage_ids.insert(l.clone());
                }
                if let Some(c) = &sym.class_name {
                    used_class_names.insert(c.clone());
                }
            }
        }

        let mut report = BatchLinkageReport::default();
        let mut promoted_to_movieclip: HashSet<Id> = HashSet::new();

        for sym in &mut candidate.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            if let Some(set) = &target_filter
                && !set.contains(&sym.id)
            {
                continue;
            }
            if !config.export_for_actionscript {
                sym.linkage_id = None;
                sym.class_name = None;
                report.items.push(BatchLinkageItem {
                    item_id: sym.id.clone(),
                    export_for_actionscript: false,
                    export_in_first_frame: false,
                    identifier: String::new(),
                    class_name: String::new(),
                });
                continue;
            }

            if sym.kind == SymbolKind::Graphic {
                sym.kind = SymbolKind::MovieClip;
                promoted_to_movieclip.insert(sym.id.clone());
            }

            let pascal_base = sanitize_pascal_case(&sym.name);
            let mut attempt = 1u32;
            let (identifier, class_name) = loop {
                let suffix = if attempt == 1 {
                    String::new()
                } else {
                    attempt.to_string()
                };
                let pascal = format!("{pascal_base}{suffix}");
                let full_class = if clean_prefix.is_empty() {
                    pascal.clone()
                } else {
                    format!("{clean_prefix}.{pascal}")
                };
                if !used_linkage_ids.contains(&full_class)
                    && !used_class_names.contains(&full_class)
                {
                    used_linkage_ids.insert(full_class.clone());
                    used_class_names.insert(full_class.clone());
                    break (full_class.clone(), full_class);
                }
                attempt += 1;
            };

            sym.linkage_id = Some(identifier.clone());
            sym.class_name = Some(class_name.clone());
            report.items.push(BatchLinkageItem {
                item_id: sym.id.clone(),
                export_for_actionscript: true,
                export_in_first_frame: config.export_in_first_frame,
                identifier,
                class_name,
            });
        }

        if !promoted_to_movieclip.is_empty() {
            for owner in &mut candidate.symbols {
                for layer in &mut owner.timeline.layers {
                    for key in &mut layer.keyframes {
                        for el in &mut key.elements {
                            if let ElementKind::Instance {
                                symbol_id,
                                playback,
                            } = &mut el.kind
                                && promoted_to_movieclip.contains(symbol_id)
                            {
                                *playback = InstancePlayback::Independent;
                            }
                        }
                    }
                }
            }
        }

        if config.include_sounds {
            for res in &candidate.resources {
                if !res.is_audio() {
                    continue;
                }
                if let Some(set) = &target_filter
                    && !set.contains(&res.id)
                {
                    continue;
                }
                let stem = Path::new(&res.path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&res.path);
                let pascal = sanitize_pascal_case(stem);
                let full_class = if clean_prefix.is_empty() {
                    pascal.clone()
                } else {
                    format!("{clean_prefix}.{pascal}")
                };
                report.items.push(BatchLinkageItem {
                    item_id: res.id.clone(),
                    export_for_actionscript: config.export_for_actionscript,
                    export_in_first_frame: config.export_in_first_frame,
                    identifier: if config.export_for_actionscript {
                        full_class.clone()
                    } else {
                        String::new()
                    },
                    class_name: if config.export_for_actionscript {
                        full_class
                    } else {
                        String::new()
                    },
                });
            }
        }

        if candidate != self.project {
            if candidate
                .symbols
                .iter()
                .any(|s| s.linkage_id.is_some() || s.class_name.is_some())
            {
                candidate.enable_program();
            }
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Batch linkage (ActionScript)", |editor| {
                editor.project = candidate;
            });
        }
        Ok(report)
    }

    /// Batch set `BitmapExportSettings` (compression, quality, smoothing) across targeted `image/*`
    /// Library resources and their placed bitmap instances/fills in one Undo step.
    pub fn batch_set_bitmap_compression_and_smoothing(
        &mut self,
        config: &LibraryBatchBitmapConfig,
    ) -> Result<usize, String> {
        self.finish_edits();
        let target_set: Option<HashSet<Id>> = config
            .target_resource_ids
            .as_ref()
            .map(|ids| ids.iter().cloned().collect());
        let quality = config.jpeg_quality.clamp(
            *VALID_JPEG_QUALITY_RANGE.start().max(&1),
            *VALID_JPEG_QUALITY_RANGE.end(),
        );
        let settings = BitmapExportSettings {
            compression: config.compression,
            use_imported_jpeg_data: config.use_imported_jpeg_data,
            jpeg_quality: quality,
            allow_smoothing: config.allow_smoothing,
        };

        let mut candidate = self.project.clone();
        let mut updated_ids: HashSet<Id> = HashSet::new();
        for res in &mut candidate.resources {
            if !res.is_image() {
                continue;
            }
            if let Some(set) = &target_set
                && !set.contains(&res.id)
            {
                continue;
            }
            res.bitmap_export = Some(settings);
            updated_ids.insert(res.id.clone());
        }

        if updated_ids.is_empty() {
            return Ok(0);
        }

        for sym in &mut candidate.symbols {
            for layer in &mut sym.timeline.layers {
                for key in &mut layer.keyframes {
                    for el in &mut key.elements {
                        if let ElementKind::Bitmap {
                            resource_id,
                            smoothing,
                            ..
                        } = &mut el.kind
                            && updated_ids.contains(resource_id)
                        {
                            *smoothing = config.allow_smoothing;
                        }
                    }
                    for bf in &mut key.bitmap_fills {
                        if updated_ids.contains(&bf.resource_id) {
                            bf.smoothing = config.allow_smoothing;
                        }
                    }
                }
            }
        }

        candidate.format_version = candidate.format_version.max(3);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        let count = updated_ids.len();
        self.transact_named("Batch bitmap compression & smoothing", |editor| {
            editor.project = candidate;
        });
        self.resource_panel.baseline = None;
        Ok(count)
    }

    /// Assign sequential or symbol-derived instance names (`<prefix><index>` or `<symbol>_<index>`)
    /// to selected Stage instances (or `Instance` elements on the active frame/layer) in one Undo step.
    pub fn auto_name_stage_instances(
        &mut self,
        config: &AutoInstanceNamerConfig,
    ) -> Result<Vec<(Id, String)>, String> {
        self.finish_edits();
        let active_sym_id = self.symbol_id().to_owned();
        let frame = self.frame;

        let selected_set: HashSet<(Id, Id)> = if self.selection.symbol == active_sym_id {
            self.selection
                .targets
                .iter()
                .map(|t| (t.layer.clone(), t.element.clone()))
                .collect()
        } else {
            HashSet::new()
        };

        let symbol_names: HashMap<Id, String> = self
            .project
            .symbols
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();

        let mut candidate = self.project.clone();
        let Some(sym) = candidate.symbol_mut(&active_sym_id) else {
            return Err("Active symbol not found.".to_owned());
        };

        let has_selected_instances = !selected_set.is_empty()
            && sym.timeline.layers.iter().any(|l| {
                l.key_at(frame).is_some_and(|k| {
                    k.elements.iter().any(|e| {
                        matches!(e.kind, ElementKind::Instance { .. })
                            && selected_set.contains(&(l.id.clone(), e.id.clone()))
                    })
                })
            });

        let mut assigned: Vec<(Id, String)> = Vec::new();
        let mut seq_offset = 0u32;
        let step = config.step.max(1);

        for layer in &mut sym.timeline.layers {
            let layer_id = layer.id.clone();
            let Some(key) = layer.key_at_mut(frame) else {
                continue;
            };
            let mut old_to_new: Vec<(String, String)> = Vec::new();
            for el in &mut key.elements {
                let ElementKind::Instance { symbol_id, .. } = &el.kind else {
                    continue;
                };
                if has_selected_instances {
                    if !selected_set.contains(&(layer_id.clone(), el.id.clone())) {
                        continue;
                    }
                } else if config.only_unnamed_when_unselected && !el.name.trim().is_empty() {
                    continue;
                }

                let idx_val = config
                    .start_index
                    .saturating_add(seq_offset.saturating_mul(step));
                seq_offset += 1;
                let idx_str = if config.pad_digits > 1 {
                    let width = config.pad_digits.clamp(1, 8);
                    format!("{idx_val:0width$}")
                } else {
                    idx_val.to_string()
                };

                let new_name = match config.mode {
                    AutoNameMode::PrefixSequential => {
                        let pfx = if config.prefix.trim().is_empty() {
                            "item_"
                        } else {
                            config.prefix.trim()
                        };
                        format!("{pfx}{idx_str}")
                    }
                    AutoNameMode::SymbolDerived => {
                        let raw_sym = symbol_names
                            .get(symbol_id)
                            .map(String::as_str)
                            .unwrap_or("instance");
                        let stem = sanitize_instance_stem(raw_sym);
                        format!("{stem}_{idx_str}")
                    }
                };

                let old_name = el.name.clone();
                el.name = new_name.clone();
                if !old_name.is_empty() && old_name != new_name {
                    old_to_new.push((old_name, new_name.clone()));
                }
                assigned.push((el.id.clone(), new_name));
            }
            for (old_target, new_target) in old_to_new {
                for ov in &mut key.instance_overrides {
                    if ov.target_name == old_target {
                        ov.target_name = new_target.clone();
                    }
                }
            }
        }

        if !assigned.is_empty() {
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Auto instance namer", |editor| {
                editor.project = candidate;
            });
        }
        Ok(assigned)
    }

    /// Deterministically randomize selected Stage elements' `Transform` (X/Y offset, scale,
    /// rotation) and color alpha using `config.seed` in one `"Randomize selection"` Undo step.
    pub fn randomize_selected_elements(
        &mut self,
        config: &RandomizeSelectionConfig,
    ) -> Result<usize, String> {
        self.finish_edits();
        let active_sym_id = self.symbol_id().to_owned();
        let frame = self.frame;

        let selected_set: HashSet<(Id, Id)> = if self.selection.symbol == active_sym_id {
            self.selection
                .targets
                .iter()
                .map(|t| (t.layer.clone(), t.element.clone()))
                .collect()
        } else {
            HashSet::new()
        };

        let mut candidate = self.project.clone();
        let Some(sym) = candidate.symbol_mut(&active_sym_id) else {
            return Err("Active symbol not found.".to_owned());
        };

        let mut rng = DeterministicRng::new(config.seed);
        let mut count = 0usize;

        for layer in &mut sym.timeline.layers {
            let layer_id = layer.id.clone();
            let Some(key) = layer.key_at_mut(frame) else {
                continue;
            };
            for el in &mut key.elements {
                if !selected_set.is_empty()
                    && !selected_set.contains(&(layer_id.clone(), el.id.clone()))
                {
                    continue;
                }
                let dx = rng.sample_range(config.offset_x_range);
                let dy = rng.sample_range(config.offset_y_range);
                let (sx, sy) = if config.uniform_scale {
                    let s = (rng.sample_range(config.scale_range_pct) / 100.0).clamp(0.01, 100.0);
                    (s, s)
                } else {
                    let sx = (rng.sample_range(config.scale_range_pct) / 100.0).clamp(0.01, 100.0);
                    let sy = (rng.sample_range(config.scale_range_pct) / 100.0).clamp(0.01, 100.0);
                    (sx, sy)
                };
                let drot = rng.sample_range(config.rotation_range_deg);
                let alpha_pct = rng.sample_range(config.alpha_range_pct).clamp(0.0, 100.0);

                el.transform.x = (el.transform.x + dx).clamp(-1e9, 1e9);
                el.transform.y = (el.transform.y + dy).clamp(-1e9, 1e9);
                el.transform.scale_x = (el.transform.scale_x * sx).clamp(-1e4, 1e4);
                el.transform.scale_y = (el.transform.scale_y * sy).clamp(-1e4, 1e4);
                el.transform.rotation = (el.transform.rotation + drot).clamp(-1e6, 1e6);
                el.transform.alpha = (alpha_pct / 100.0).clamp(0.0, 1.0);
                count += 1;
            }
        }

        if count > 0 {
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Randomize selection", |editor| {
                editor.project = candidate;
            });
        }
        Ok(count)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. NF2 Modal Dialog State & Host (`show_productivity_modals`)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum ProductivityModalDraft {
    BatchRename(LibraryBatchRenameConfig),
    BatchLinkage(LibraryBatchLinkageConfig),
    BatchBitmap(LibraryBatchBitmapConfig),
    AutoInstanceNamer(AutoInstanceNamerConfig),
    RandomizeSelection(RandomizeSelectionConfig),
}

fn modal_draft_id() -> egui::Id {
    egui::Id::new("nf2-productivity-modal-draft")
}

fn modal_pass_guard_id() -> egui::Id {
    egui::Id::new("nf2-productivity-modal-pass-guard")
}

#[allow(dead_code)]
pub fn has_productivity_modal(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<ProductivityModalDraft>(modal_draft_id()))
        .is_some()
}

#[allow(dead_code)]
pub fn active_productivity_modal(ctx: &egui::Context) -> Option<ProductivityModalDraft> {
    ctx.data(|d| d.get_temp::<ProductivityModalDraft>(modal_draft_id()))
}

pub fn close_productivity_modal(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<ProductivityModalDraft>(modal_draft_id()));
}

impl Editor {
    pub fn open_batch_rename_library_modal(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let target =
            if self.library_selected.is_some() || !self.library_panel.selected_folder.is_empty() {
                BatchRenameTarget::Selected
            } else {
                BatchRenameTarget::AllSymbols
            };
        let cfg = LibraryBatchRenameConfig {
            target,
            ..LibraryBatchRenameConfig::default()
        };
        ctx.data_mut(|d| d.insert_temp(modal_draft_id(), ProductivityModalDraft::BatchRename(cfg)));
    }

    pub fn open_batch_linkage_modal(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let target_ids = self.library_selected.as_ref().map(|id| vec![id.clone()]);
        let cfg = LibraryBatchLinkageConfig {
            target_ids,
            ..LibraryBatchLinkageConfig::default()
        };
        ctx.data_mut(|d| {
            d.insert_temp(modal_draft_id(), ProductivityModalDraft::BatchLinkage(cfg))
        });
    }

    pub fn open_batch_bitmap_compression_modal(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let target_resource_ids = self
            .resource_panel
            .selected
            .as_ref()
            .filter(|id| {
                self.project
                    .resources
                    .iter()
                    .any(|r| &r.id == *id && r.is_image())
            })
            .map(|id| vec![id.clone()]);
        let cfg = LibraryBatchBitmapConfig {
            target_resource_ids,
            ..LibraryBatchBitmapConfig::default()
        };
        ctx.data_mut(|d| d.insert_temp(modal_draft_id(), ProductivityModalDraft::BatchBitmap(cfg)));
    }

    pub fn open_auto_instance_namer_modal(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        ctx.data_mut(|d| {
            d.insert_temp(
                modal_draft_id(),
                ProductivityModalDraft::AutoInstanceNamer(AutoInstanceNamerConfig::default()),
            )
        });
    }

    pub fn open_randomize_selection_modal(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        ctx.data_mut(|d| {
            d.insert_temp(
                modal_draft_id(),
                ProductivityModalDraft::RandomizeSelection(RandomizeSelectionConfig::default()),
            )
        });
    }
}

/// Host and render any active productivity modal dialog (`R-068`).
///
/// Safe to call from both `library.rs` and `app.rs` in the same frame; an egui pass guard
/// ensures the active modal is rendered at most once per pass.
pub fn show_productivity_modals(editor: &mut Editor, ctx: &egui::Context) {
    let pass_nr = ctx.cumulative_pass_nr();
    let already_shown = ctx.data_mut(|d| {
        let prev = d.get_temp::<u64>(modal_pass_guard_id());
        if prev == Some(pass_nr) {
            true
        } else {
            d.insert_temp(modal_pass_guard_id(), pass_nr);
            false
        }
    });
    if already_shown {
        return;
    }

    let Some(mut draft) = ctx.data(|d| d.get_temp::<ProductivityModalDraft>(modal_draft_id()))
    else {
        return;
    };

    let p = neoflash_ui::palette(ctx);
    let (confirmed, cancelled) = match &mut draft {
        ProductivityModalDraft::BatchRename(cfg) => {
            let can_confirm = !cfg.prefix.is_empty()
                || !cfg.suffix.is_empty()
                || !cfg.find.is_empty()
                || cfg.use_numbering;
            NeoModal::new("nf2-batch-rename-library", "Batch rename library items")
                .width(480.0)
                .show(ctx, "Rename", can_confirm, |ui| {
                    SectionHeader::new("Scope")
                        .id_salt("batch-rename-scope-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            Dropdown::new("batch-rename-target-select", cfg.target.label())
                                .width(ui.available_width())
                                .show(ui, |ui| {
                                    for choice in [
                                        BatchRenameTarget::Selected,
                                        BatchRenameTarget::AllSymbols,
                                        BatchRenameTarget::AllItems,
                                    ] {
                                        if ui
                                            .add(
                                                MenuItem::new(choice.label())
                                                    .checked(cfg.target == choice),
                                            )
                                            .clicked()
                                        {
                                            cfg.target = choice;
                                            ui.close();
                                        }
                                    }
                                });
                            ui.add(Checkbox::new(
                                &mut cfg.also_rename_stage_instances,
                                "Also rename matching Stage instances",
                            ));
                        });

                    SectionHeader::new("Affixes & Find / Replace")
                        .id_salt("batch-rename-affix-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    TextField::new(&mut cfg.prefix, "Prefix")
                                        .prefix("Prefix")
                                        .width(col),
                                );
                                ui.add(
                                    TextField::new(&mut cfg.suffix, "Suffix")
                                        .prefix("Suffix")
                                        .width(col),
                                );
                            });
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    TextField::new(&mut cfg.find, "Find")
                                        .prefix("Find")
                                        .width(col),
                                );
                                ui.add(
                                    TextField::new(&mut cfg.replace, "Replace")
                                        .prefix("Replace")
                                        .width(col),
                                );
                            });
                        });

                    SectionHeader::new("Sequential Numbering")
                        .id_salt("batch-rename-seq-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(Checkbox::new(
                                &mut cfg.use_numbering,
                                "Append sequential numbering",
                            ));
                            ui.add_enabled_ui(cfg.use_numbering, |ui| {
                                ui.horizontal(|ui| {
                                    let col = (ui.available_width() - space::S2) * 0.5;
                                    let mut start = f64::from(cfg.start_number);
                                    if ui
                                        .add(
                                            Field::new(&mut start, "Start number")
                                                .prefix("Start")
                                                .range(0.0..=99999.0)
                                                .decimals(0)
                                                .width(col),
                                        )
                                        .changed()
                                    {
                                        cfg.start_number = start.round().max(0.0) as u32;
                                    }
                                    let mut digits = cfg.pad_digits as f64;
                                    if ui
                                        .add(
                                            Field::new(&mut digits, "Pad digits")
                                                .prefix("Digits")
                                                .range(1.0..=8.0)
                                                .decimals(0)
                                                .width(col),
                                        )
                                        .changed()
                                    {
                                        cfg.pad_digits = digits.round().clamp(1.0, 8.0) as usize;
                                    }
                                });
                            });
                        });
                })
        }
        ProductivityModalDraft::BatchLinkage(cfg) => NeoModal::new(
            "nf2-batch-linkage",
            "Batch linkage (Export for ActionScript)",
        )
        .width(480.0)
        .show(ctx, "Apply", true, |ui| {
            let scope_all = cfg.target_ids.is_none();
            let scope_label = if scope_all {
                "Scope: All library symbols & sounds"
            } else {
                "Scope: Selected library item"
            };
            Dropdown::new("batch-linkage-scope", scope_label)
                .width(ui.available_width())
                .show(ui, |ui| {
                    if ui
                        .add(MenuItem::new("All library symbols & sounds").checked(scope_all))
                        .clicked()
                    {
                        cfg.target_ids = None;
                        ui.close();
                    }
                    if let Some(sel) = &editor.library_selected {
                        let sel_clone = sel.clone();
                        if ui
                            .add(MenuItem::new("Selected library item").checked(!scope_all))
                            .clicked()
                        {
                            cfg.target_ids = Some(vec![sel_clone]);
                            ui.close();
                        }
                    }
                });

            SectionHeader::new("ActionScript Linkage")
                .id_salt("batch-linkage-sec")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(Checkbox::new(
                        &mut cfg.export_for_actionscript,
                        "Export for ActionScript",
                    ));
                    ui.add_enabled_ui(cfg.export_for_actionscript, |ui| {
                        ui.add(Checkbox::new(
                            &mut cfg.export_in_first_frame,
                            "Export in frame 1",
                        ));
                        ui.add(
                            TextField::new(&mut cfg.class_prefix, "Class prefix")
                                .prefix("Prefix")
                                .width(ui.available_width()),
                        );
                        let sample =
                            format!("{}.HeroButton", sanitize_class_prefix(&cfg.class_prefix));
                        ui.label(
                            rich(
                                format!("Example class: {}", sample.trim_start_matches('.')),
                                TextStyle::Small,
                            )
                            .color(p.text_tertiary),
                        );
                    });
                });
        }),
        ProductivityModalDraft::BatchBitmap(cfg) => NeoModal::new(
            "nf2-batch-bitmap-compression",
            "Batch bitmap compression & smoothing",
        )
        .width(480.0)
        .show(ctx, "Apply", true, |ui| {
            let scope_all = cfg.target_resource_ids.is_none();
            let scope_label = if scope_all {
                "Target: All bitmap resources"
            } else {
                "Target: Selected bitmap resource"
            };
            Dropdown::new("batch-bitmap-scope", scope_label)
                .width(ui.available_width())
                .show(ui, |ui| {
                    if ui
                        .add(MenuItem::new("All bitmap resources").checked(scope_all))
                        .clicked()
                    {
                        cfg.target_resource_ids = None;
                        ui.close();
                    }
                    if let Some(sel) = &editor.resource_panel.selected {
                        let sel_clone = sel.clone();
                        if ui
                            .add(MenuItem::new("Selected bitmap resource").checked(!scope_all))
                            .clicked()
                        {
                            cfg.target_resource_ids = Some(vec![sel_clone]);
                            ui.close();
                        }
                    }
                });

            group(ui, "Compression & Quality");
            let comp_label = format!("Compression: {}", cfg.compression.label());
            Dropdown::new("batch-bitmap-compression-select", &comp_label)
                .width(ui.available_width())
                .show(ui, |ui| {
                    for comp in BitmapCompression::ALL {
                        if ui
                            .add(MenuItem::new(comp.label()).checked(cfg.compression == comp))
                            .clicked()
                        {
                            cfg.compression = comp;
                            ui.close();
                        }
                    }
                });

            ui.add_enabled_ui(cfg.compression == BitmapCompression::PhotoJpeg, |ui| {
                let mut q = f64::from(cfg.jpeg_quality);
                if ui
                    .add(
                        Field::new(&mut q, "JPEG quality")
                            .prefix("Quality")
                            .unit("%")
                            .range(1.0..=100.0)
                            .decimals(0)
                            .width(160.0),
                    )
                    .changed()
                {
                    cfg.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                }
            });
            ui.add(Checkbox::new(&mut cfg.allow_smoothing, "Allow smoothing"));
        }),
        ProductivityModalDraft::AutoInstanceNamer(cfg) => {
            NeoModal::new("nf2-auto-instance-namer", "Auto Instance Namer")
                .width(460.0)
                .show(ctx, "Name instances", true, |ui| {
                    SectionHeader::new("Naming Pattern")
                        .id_salt("auto-namer-pattern-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            Dropdown::new("auto-namer-mode-select", cfg.mode.label())
                                .width(ui.available_width())
                                .show(ui, |ui| {
                                    for mode in AutoNameMode::ALL {
                                        if ui
                                            .add(
                                                MenuItem::new(mode.label())
                                                    .checked(cfg.mode == mode),
                                            )
                                            .clicked()
                                        {
                                            cfg.mode = mode;
                                            ui.close();
                                        }
                                    }
                                });
                            ui.add_enabled_ui(cfg.mode == AutoNameMode::PrefixSequential, |ui| {
                                ui.add(
                                    TextField::new(&mut cfg.prefix, "Instance prefix")
                                        .prefix("Prefix")
                                        .width(ui.available_width()),
                                );
                            });
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                let mut start = f64::from(cfg.start_index);
                                if ui
                                    .add(
                                        Field::new(&mut start, "Start index")
                                            .prefix("Start")
                                            .range(0.0..=99999.0)
                                            .decimals(0)
                                            .width(col),
                                    )
                                    .changed()
                                {
                                    cfg.start_index = start.round().max(0.0) as u32;
                                }
                                let mut pad = cfg.pad_digits as f64;
                                if ui
                                    .add(
                                        Field::new(&mut pad, "Pad digits")
                                            .prefix("Digits")
                                            .range(0.0..=8.0)
                                            .decimals(0)
                                            .width(col),
                                    )
                                    .changed()
                                {
                                    cfg.pad_digits = pad.round().clamp(0.0, 8.0) as usize;
                                }
                            });
                            ui.add(Checkbox::new(
                                &mut cfg.only_unnamed_when_unselected,
                                "Only rename unnamed instances when nothing is selected",
                            ));
                        });
                })
        }
        ProductivityModalDraft::RandomizeSelection(cfg) => {
            NeoModal::new("nf2-randomize-selection", "Randomize Selection")
                .width(480.0)
                .show(ctx, "Randomize", true, |ui| {
                    SectionHeader::new("Deterministic Seed")
                        .id_salt("randomize-seed-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            let mut seed_val = cfg.seed as f64;
                            if ui
                                .add(
                                    Field::new(&mut seed_val, "PRNG seed")
                                        .prefix("Seed")
                                        .range(0.0..=1_000_000.0)
                                        .decimals(0)
                                        .width(ui.available_width()),
                                )
                                .changed()
                            {
                                cfg.seed = seed_val.round().max(0.0) as u64;
                            }
                        });

                    SectionHeader::new("Transform & Alpha Ranges")
                        .id_salt("randomize-ranges-sec")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    Field::new(&mut cfg.offset_x_range[0], "Min X offset")
                                        .prefix("ΔX min")
                                        .unit("px")
                                        .width(col),
                                );
                                ui.add(
                                    Field::new(&mut cfg.offset_x_range[1], "Max X offset")
                                        .prefix("ΔX max")
                                        .unit("px")
                                        .width(col),
                                );
                            });
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    Field::new(&mut cfg.offset_y_range[0], "Min Y offset")
                                        .prefix("ΔY min")
                                        .unit("px")
                                        .width(col),
                                );
                                ui.add(
                                    Field::new(&mut cfg.offset_y_range[1], "Max Y offset")
                                        .prefix("ΔY max")
                                        .unit("px")
                                        .width(col),
                                );
                            });
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    Field::new(&mut cfg.scale_range_pct[0], "Min scale")
                                        .prefix("Scale min")
                                        .unit("%")
                                        .range(1.0..=1000.0)
                                        .width(col),
                                );
                                ui.add(
                                    Field::new(&mut cfg.scale_range_pct[1], "Max scale")
                                        .prefix("Scale max")
                                        .unit("%")
                                        .range(1.0..=1000.0)
                                        .width(col),
                                );
                            });
                            ui.add(Checkbox::new(&mut cfg.uniform_scale, "Uniform X/Y scale"));
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    Field::new(&mut cfg.rotation_range_deg[0], "Min rotation")
                                        .prefix("Rot min")
                                        .unit("°")
                                        .width(col),
                                );
                                ui.add(
                                    Field::new(&mut cfg.rotation_range_deg[1], "Max rotation")
                                        .prefix("Rot max")
                                        .unit("°")
                                        .width(col),
                                );
                            });
                            ui.horizontal(|ui| {
                                let col = (ui.available_width() - space::S2) * 0.5;
                                ui.add(
                                    Field::new(&mut cfg.alpha_range_pct[0], "Min alpha")
                                        .prefix("Alpha min")
                                        .unit("%")
                                        .range(0.0..=100.0)
                                        .width(col),
                                );
                                ui.add(
                                    Field::new(&mut cfg.alpha_range_pct[1], "Max alpha")
                                        .prefix("Alpha max")
                                        .unit("%")
                                        .range(0.0..=100.0)
                                        .width(col),
                                );
                            });
                        });
                })
        }
    };

    if cancelled {
        close_productivity_modal(ctx);
        return;
    }

    if confirmed {
        let res = match &draft {
            ProductivityModalDraft::BatchRename(cfg) => {
                editor.batch_rename_library_items(cfg).map(|r| {
                    format!(
                        "Batch renamed {} symbol(s) and {} instance(s).",
                        r.renamed_symbols.len(),
                        r.renamed_instances.len()
                    )
                })
            }
            ProductivityModalDraft::BatchLinkage(cfg) => editor
                .batch_set_library_linkage(cfg)
                .map(|r| format!("Updated ActionScript linkage on {} item(s).", r.len())),
            ProductivityModalDraft::BatchBitmap(cfg) => editor
                .batch_set_bitmap_compression_and_smoothing(cfg)
                .map(|n| format!("Updated bitmap compression & smoothing on {n} image(s).")),
            ProductivityModalDraft::AutoInstanceNamer(cfg) => editor
                .auto_name_stage_instances(cfg)
                .map(|v| format!("Named {} stage instance(s).", v.len())),
            ProductivityModalDraft::RandomizeSelection(cfg) => editor
                .randomize_selected_elements(cfg)
                .map(|n| format!("Randomized {n} stage element(s).")),
        };
        match res {
            Ok(msg) => {
                editor.message(msg);
                close_productivity_modal(ctx);
            }
            Err(err) => {
                editor.message(err);
                ctx.data_mut(|d| d.insert_temp(modal_draft_id(), draft));
            }
        }
    } else {
        ctx.data_mut(|d| d.insert_temp(modal_draft_id(), draft));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        Element, Keyframe, Layer, LayerProperties, ResourceUnit, Symbol, Timeline, Transform,
    };

    fn tiny_png_bytes() -> Vec<u8> {
        let mut png_bytes = Vec::new();
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([255, 128, 64, 255]));
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("encode 1x1 test png");
        png_bytes
    }

    fn make_empty_symbol(id: &str, name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: id.to_owned(),
            name: name.to_owned(),
            folder: String::new(),
            kind,
            timeline: Timeline {
                frame_count: if kind == SymbolKind::Button { 4 } else { 1 },
                layers: vec![Layer {
                    id: format!("layer-{id}"),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        }
    }

    #[test]
    fn batch_rename_library_items_supports_affixes_find_replace_numbering_and_single_undo() {
        let mut project = Project::blank();
        project.symbols.push(make_empty_symbol(
            "sym-hero",
            "btn_old_hero",
            SymbolKind::MovieClip,
        ));
        project.symbols.push(make_empty_symbol(
            "sym-quit",
            "btn_old_quit",
            SymbolKind::Button,
        ));
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "inst-hero".to_owned(),
                name: "btn_old_hero".to_owned(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-hero".to_owned(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            });

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        let before = editor.project.clone();

        let cfg = LibraryBatchRenameConfig {
            target: BatchRenameTarget::AllSymbols,
            prefix: "ui_".to_owned(),
            suffix: "_v2_".to_owned(),
            find: "old".to_owned(),
            replace: "new".to_owned(),
            use_numbering: true,
            start_number: 1,
            pad_digits: 2,
            also_rename_stage_instances: true,
        };
        let report = editor.batch_rename_library_items(&cfg).unwrap();
        assert_eq!(report.renamed_symbols.len(), 2);
        assert_eq!(report.renamed_instances.len(), 1);
        assert_eq!(
            editor.project.symbol("sym-hero").unwrap().name,
            "ui_btn_new_hero_v2_01"
        );
        assert_eq!(
            editor.project.symbol("sym-quit").unwrap().name,
            "ui_btn_new_quit_v2_02"
        );
        let inst_name = &editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].name;
        assert_eq!(inst_name, "ui_btn_new_hero_v2_01_01");
        assert!(editor.project.validate().is_ok());

        // Single Undo restores exact previous state.
        editor.undo();
        assert_eq!(editor.project, before);
    }

    #[test]
    fn find_and_delete_unused_library_items_preserves_used_and_linkage_and_undoes_atomically() {
        let mut project = Project::blank();
        project.format_version = 3;
        project.enable_program();

        // 1. Used symbol placed on root.
        project.symbols.push(make_empty_symbol(
            "sym-used",
            "UsedClip",
            SymbolKind::MovieClip,
        ));
        // 2. Unused symbol with ActionScript linkage (must be preserved).
        let mut linked_sym =
            make_empty_symbol("sym-linked", "ExportedWidget", SymbolKind::MovieClip);
        linked_sym.linkage_id = Some("app.ExportedWidget".to_owned());
        linked_sym.class_name = Some("app.ExportedWidget".to_owned());
        project.symbols.push(linked_sym);
        // 3. Unused symbol without linkage (must be deleted).
        project.symbols.push(make_empty_symbol(
            "sym-unused",
            "OrphanClip",
            SymbolKind::Graphic,
        ));

        // Resources: one used bitmap, one unused bitmap.
        let png = tiny_png_bytes();
        let used_res = ResourceUnit::from_bytes("res-used", "used.png", "image/png", &png).unwrap();
        let unused_res =
            ResourceUnit::from_bytes("res-unused", "unused.png", "image/png", &png).unwrap();
        project.resources.push(used_res);
        project.resources.push(unused_res);

        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "inst-used".to_owned(),
                name: "used_inst".to_owned(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-used".to_owned(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            });
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "bmp-used".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Bitmap {
                    resource_id: "res-used".to_owned(),
                    width: 1,
                    height: 1,
                    smoothing: false,
                },
                ..Element::default()
            });
        project.validate().unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        let before = editor.project.clone();

        let (sel_syms, sel_res) = editor.select_unused_library_items();
        assert_eq!(sel_syms, vec!["sym-unused".to_owned()]);
        assert_eq!(sel_res, vec!["res-unused".to_owned()]);

        let report = editor.find_and_delete_unused_library_items();
        assert_eq!(report.unused_symbol_ids, vec!["sym-unused".to_owned()]);
        assert_eq!(report.unused_resource_ids, vec!["res-unused".to_owned()]);
        assert!(editor.project.symbol("sym-used").is_some());
        assert!(editor.project.symbol("sym-linked").is_some());
        assert!(editor.project.symbol("sym-unused").is_none());
        assert!(editor.project.resource("res-used").is_some());
        assert!(editor.project.resource("res-unused").is_none());
        assert!(editor.project.validate().is_ok());

        editor.undo();
        assert_eq!(editor.project, before);
    }

    #[test]
    fn batch_set_library_linkage_populates_pascal_case_classes_and_undoes_atomically() {
        let mut project = Project::blank();
        project.symbols.push(make_empty_symbol(
            "sym-1",
            "hero button_01",
            SymbolKind::MovieClip,
        ));
        project.symbols.push(make_empty_symbol(
            "sym-2",
            "enemy-ship",
            SymbolKind::Graphic,
        ));

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        let before = editor.project.clone();

        let cfg = LibraryBatchLinkageConfig {
            target_ids: None,
            class_prefix: "app.assets.".to_owned(),
            export_for_actionscript: true,
            export_in_first_frame: true,
            include_sounds: true,
        };
        let report = editor.batch_set_library_linkage(&cfg).unwrap();
        assert_eq!(report.len(), 2);

        let sym1 = editor.project.symbol("sym-1").unwrap();
        assert_eq!(sym1.class_name.as_deref(), Some("app.assets.HeroButton01"));
        assert_eq!(sym1.linkage_id.as_deref(), Some("app.assets.HeroButton01"));

        let sym2 = editor.project.symbol("sym-2").unwrap();
        assert_eq!(sym2.kind, SymbolKind::MovieClip);
        assert_eq!(sym2.class_name.as_deref(), Some("app.assets.EnemyShip"));
        assert_eq!(sym2.linkage_id.as_deref(), Some("app.assets.EnemyShip"));
        assert!(editor.project.validate().is_ok());

        editor.undo();
        assert_eq!(editor.project, before);
    }

    #[test]
    fn batch_set_bitmap_compression_and_smoothing_updates_all_images_in_one_undo_step() {
        let mut project = Project::blank();
        project.format_version = 3;
        let png = tiny_png_bytes();
        project
            .resources
            .push(ResourceUnit::from_bytes("img-1", "a.png", "image/png", &png).unwrap());
        project
            .resources
            .push(ResourceUnit::from_bytes("img-2", "b.png", "image/png", &png).unwrap());

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        let before = editor.project.clone();

        let cfg = LibraryBatchBitmapConfig {
            target_resource_ids: None,
            compression: BitmapCompression::PhotoJpeg,
            jpeg_quality: 65,
            use_imported_jpeg_data: false,
            allow_smoothing: true,
        };
        let updated = editor
            .batch_set_bitmap_compression_and_smoothing(&cfg)
            .unwrap();
        assert_eq!(updated, 2);

        for id in ["img-1", "img-2"] {
            let exp = editor.project.resource(id).unwrap().bitmap_export.unwrap();
            assert_eq!(exp.compression, BitmapCompression::PhotoJpeg);
            assert_eq!(exp.jpeg_quality, 65);
            assert!(exp.allow_smoothing);
        }
        assert!(editor.project.validate().is_ok());

        editor.undo();
        assert_eq!(editor.project, before);
    }

    #[test]
    fn auto_name_stage_instances_and_randomize_selected_elements_are_deterministic_and_undoable() {
        let mut project = Project::blank();
        project.symbols.push(make_empty_symbol(
            "sym-star",
            "Star Badge",
            SymbolKind::MovieClip,
        ));
        for i in 0..3 {
            project.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .push(Element {
                    id: format!("el-{i}"),
                    name: String::new(),
                    transform: Transform {
                        x: f64::from(i * 50),
                        y: 100.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-star".to_owned(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
        }

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        let before = editor.project.clone();

        // 1. Auto Instance Namer (prefix sequential).
        let named = editor
            .auto_name_stage_instances(&AutoInstanceNamerConfig {
                mode: AutoNameMode::PrefixSequential,
                prefix: "item_".to_owned(),
                start_index: 0,
                step: 1,
                pad_digits: 0,
                only_unnamed_when_unselected: true,
            })
            .unwrap();
        assert_eq!(named.len(), 3);
        let elems = &editor.project.symbols[0].timeline.layers[0].keyframes[0].elements;
        assert_eq!(elems[0].name, "item_0");
        assert_eq!(elems[1].name, "item_1");
        assert_eq!(elems[2].name, "item_2");
        assert!(editor.project.validate().is_ok());

        editor.undo();
        assert_eq!(editor.project, before);

        // 2. Randomize Selection with deterministic seed.
        editor.select_all();
        let rand_cfg = RandomizeSelectionConfig {
            seed: 1337,
            offset_x_range: [-25.0, 25.0],
            offset_y_range: [-15.0, 15.0],
            scale_range_pct: [75.0, 125.0],
            uniform_scale: true,
            rotation_range_deg: [-30.0, 30.0],
            alpha_range_pct: [50.0, 95.0],
        };
        let count = editor.randomize_selected_elements(&rand_cfg).unwrap();
        assert_eq!(count, 3);
        assert!(editor.project.validate().is_ok());
        let first_pass_transforms: Vec<Transform> = editor.project.symbols[0].timeline.layers[0]
            .keyframes[0]
            .elements
            .iter()
            .map(|e| e.transform)
            .collect();
        for t in &first_pass_transforms {
            assert!((t.scale_x - t.scale_y).abs() < 1e-9);
            assert!((0.50..=0.95).contains(&t.alpha));
        }

        // Single Undo restores exact original transforms.
        editor.undo();
        assert_eq!(editor.project, before);

        // Re-running with the same seed produces the exact same transforms.
        editor.select_all();
        editor.randomize_selected_elements(&rand_cfg).unwrap();
        let second_pass_transforms: Vec<Transform> = editor.project.symbols[0].timeline.layers[0]
            .keyframes[0]
            .elements
            .iter()
            .map(|e| e.transform)
            .collect();
        assert_eq!(first_pass_transforms, second_pass_transforms);

        // Headless modal smoke check across all 5 productivity modals.
        editor.open_batch_rename_library_modal(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            show_productivity_modals(&mut editor, ctx);
        });
        close_productivity_modal(&ctx);
    }
}
