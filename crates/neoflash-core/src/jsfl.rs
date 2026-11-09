//! Sandboxed JSFL authoring automation host, phased Flash/Animate DOM bindings,
//! capability-gated `FLfile` access, and native built-in Smart Batch Rename (`T-187`).
//!
//! JSFL is authoring automation executed against a candidate `Project` inside the editor;
//! it is never embedded in published HTML5 output or confused with runtime frame/object scripts.

use crate::{
    AuthoringGroup, BlendMode, Color, Easing, Element, ElementKind, FillRule, FrameScript, Id,
    InstancePlayback, Keyframe, Layer, LayerRole, PathCommand, Project, Script, ScriptLanguage,
    Symbol, SymbolKind, Transform, TweenKind, new_id, valid_library_folder, validate,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Resource limits enforced deterministically by `JsflSandbox`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsflLimits {
    pub max_source_bytes: usize,
    pub max_steps: usize,
    pub max_call_depth: usize,
    pub max_heap_nodes: usize,
    pub max_mutations: usize,
    pub max_output_lines: usize,
    pub max_script_recursion: usize,
}

impl Default for JsflLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 256 * 1024,
            max_steps: 50_000,
            max_call_depth: 32,
            max_heap_nodes: 16_384,
            max_mutations: 8_192,
            max_output_lines: 1_000,
            max_script_recursion: 8,
        }
    }
}

/// Explicit filesystem capabilities for `FLfile` and `fl.runScript`.
///
/// By default, all filesystem operations are denied (`allowed_root: None`, all booleans `false`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlFileCapabilities {
    pub allowed_root: Option<PathBuf>,
    pub allow_read: bool,
    pub allow_write: bool,
    pub allow_delete: bool,
}

/// Located diagnostic reported when a JSFL script fails to parse, throws, exceeds limits,
/// touches a stale handle, violates capabilities, or produces an invalid project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsflDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl JsflDiagnostic {
    pub fn new(
        file: impl Into<String>,
        line: u32,
        column: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line: line.max(1),
            column: column.max(1),
            message: message.into(),
        }
    }
}

impl fmt::Display for JsflDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for JsflDiagnostic {}

/// Opaque document handle exposed to JSFL scripts instead of raw references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocHandle(pub u64);

/// Opaque Library item handle (`Symbol` or logical folder).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemHandle {
    pub doc: DocHandle,
    pub item_id: Id,
    pub is_folder: bool,
}

/// Opaque Timeline handle referencing a scene or symbol timeline.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimelineHandle {
    pub doc: DocHandle,
    pub symbol_id: Id,
}

/// Opaque Layer handle referencing a layer by stable `Id`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerHandle {
    pub doc: DocHandle,
    pub symbol_id: Id,
    pub layer_id: Id,
}

/// Opaque Frame handle referencing a specific 0-based frame on a layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameHandle {
    pub doc: DocHandle,
    pub symbol_id: Id,
    pub layer_id: Id,
    pub frame: u32,
}

/// Opaque Element handle referencing an authored element on a keyframe by stable `Id`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ElementHandle {
    pub doc: DocHandle,
    pub symbol_id: Id,
    pub layer_id: Id,
    pub keyframe: u32,
    pub element_id: Id,
}

/// Editor selection, navigation, and dirty state snapshotted before a JSFL run and
/// updated only if the entire run succeeds and passes project validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsflAuthoringState {
    pub active_symbol: Id,
    pub current_layer: usize,
    pub current_frame: u32,
    pub selected_element_ids: Vec<Id>,
    pub selected_layers: Vec<usize>,
    /// Selected frame spans as `(layer_index, start_frame, end_frame_exclusive)`.
    pub selected_frames: Vec<(usize, u32, u32)>,
    pub selected_library_items: Vec<Id>,
    pub dirty: bool,
}

impl JsflAuthoringState {
    pub fn for_project(project: &Project) -> Self {
        Self {
            active_symbol: project.root.clone(),
            current_layer: 0,
            current_frame: 0,
            selected_element_ids: Vec::new(),
            selected_layers: vec![0],
            selected_frames: vec![(0, 0, 1)],
            selected_library_items: Vec::new(),
            dirty: false,
        }
    }
}

/// Runtime value inside the sandboxed JSFL interpreter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum JsflValue {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsflValue>),
    Object(BTreeMap<String, JsflValue>),
    Doc(DocHandle),
    Library(DocHandle),
    Timeline(TimelineHandle),
    Layer(LayerHandle),
    Frame(FrameHandle),
    Item(ItemHandle),
    Element(ElementHandle),
    FlGlobal,
    OutputPanel,
    FlFile,
}

impl JsflValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Bool(true) => Some(1.0),
            Self::Bool(false) | Self::Null => Some(0.0),
            Self::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn to_display_string(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            Self::String(s) => s.clone(),
            Self::Array(items) => {
                let parts: Vec<String> = items.iter().map(Self::to_display_string).collect();
                parts.join(",")
            }
            Self::Object(map) => {
                let parts: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{k}:{}", v.to_display_string()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Self::Doc(h) => format!("[object Document({})]", h.0),
            Self::Library(h) => format!("[object Library({})]", h.0),
            Self::Timeline(h) => format!("[object Timeline({})]", h.symbol_id),
            Self::Layer(h) => format!("[object Layer({})]", h.layer_id),
            Self::Frame(h) => format!("[object Frame({}:{})]", h.layer_id, h.frame),
            Self::Item(h) => format!("[object Item({})]", h.item_id),
            Self::Element(h) => format!("[object Element({})]", h.element_id),
            Self::FlGlobal => "[object Flash]".to_owned(),
            Self::OutputPanel => "[object outputPanel]".to_owned(),
            Self::FlFile => "[object FLfile]".to_owned(),
        }
    }
}

/// Validated outcome of a sandboxed JSFL execution.
#[derive(Clone, Debug, PartialEq)]
pub struct JsflResult {
    pub project: Project,
    pub state: JsflAuthoringState,
    pub output_lines: Vec<String>,
    pub undo_label: String,
    pub mutations: usize,
    pub steps_executed: usize,
    pub return_value: JsflValue,
}

/// Target scope for built-in Smart Batch Rename (`batch_rename_library_and_instances`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchRenameScope {
    LibraryItemsOnly,
    StageInstancesOnly,
    #[default]
    LibraryAndInstances,
}

/// Options for built-in Smart Batch Rename of Library items and matching Stage instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRenameOptions {
    pub scope: BatchRenameScope,
    /// Optional explicit symbol IDs to target. When `None`, all non-scene symbols/instances
    /// matching `name_filter` are targeted.
    pub target_symbol_ids: Option<Vec<Id>>,
    /// Optional filter applied to current names (supports `*` and `?` wildcards or substring match).
    pub name_filter: Option<String>,
    /// Optional substring or wildcard pattern (`*`, `?`) to replace with `replace`.
    pub find: Option<String>,
    pub replace: String,
    pub prefix: String,
    pub suffix: String,
    /// Optional starting number for sequential numbering (`Some(1)` -> `01`, `02`, ...).
    pub sequence_start: Option<u32>,
    pub sequence_step: u32,
    pub sequence_digits: usize,
    /// When true and renaming Library symbols in `LibraryAndInstances` scope, also updates the
    /// instance names of placed instances of those symbols across every timeline/layer/keyframe.
    pub sync_instance_names_to_symbol: bool,
}

impl Default for BatchRenameOptions {
    fn default() -> Self {
        Self {
            scope: BatchRenameScope::LibraryAndInstances,
            target_symbol_ids: None,
            name_filter: None,
            find: None,
            replace: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            sequence_start: None,
            sequence_step: 1,
            sequence_digits: 2,
            sync_instance_names_to_symbol: true,
        }
    }
}

/// Summary of items and instances renamed by `batch_rename_library_and_instances`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRenameReport {
    /// `(symbol_id, old_name, new_name)`
    pub renamed_symbols: Vec<(Id, String, String)>,
    /// `(symbol_id, layer_id, keyframe_frame, element_id, old_name, new_name)`
    pub renamed_instances: Vec<(Id, Id, u32, Id, String, String)>,
    pub undo_label: String,
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            if p[i - 1] == '*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if p[i - 1] == '?' || p[i - 1] == t[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[p.len()][t.len()]
}

fn format_renamed(
    original: &str,
    options: &BatchRenameOptions,
    seq_index: u32,
) -> Result<String, JsflDiagnostic> {
    let mut base = if let Some(find) = &options.find {
        if find.is_empty() {
            original.to_owned()
        } else if find.contains('*') || find.contains('?') {
            if wildcard_matches(find, original) {
                options.replace.clone()
            } else {
                original.to_owned()
            }
        } else {
            original.replace(find.as_str(), &options.replace)
        }
    } else if !options.replace.is_empty() {
        options.replace.clone()
    } else {
        original.to_owned()
    };

    let seq_str = options.sequence_start.map(|start| {
        let step = options.sequence_step.max(1);
        let num = start.saturating_add(seq_index.saturating_mul(step));
        let width = options.sequence_digits.clamp(1, 8);
        format!("{num:0width$}")
    });

    if let Some(seq) = &seq_str {
        if base.contains("{n}") {
            base = base.replace("{n}", seq);
        } else if base.contains('#') {
            base = base.replace('#', seq);
        } else {
            base = format!("{}{}{}{}", options.prefix, base, options.suffix, seq);
            return validate_renamed(&base);
        }
    }

    let result = format!("{}{}{}", options.prefix, base, options.suffix);
    validate_renamed(&result)
}

fn validate_renamed(name: &str) -> Result<String, JsflDiagnostic> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            "<batch_rename>",
            1,
            1,
            format!("Invalid target name '{name}': must be 1..=256 non-control characters."),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Built-in native Smart Batch Rename across Library symbols and matching Stage instances
/// across every symbol, layer, and keyframe in `project`.
///
/// Executes atomically on a cloned candidate project and validates with `crate::validate`
/// before committing.
pub fn batch_rename_library_and_instances(
    project: &mut Project,
    options: &BatchRenameOptions,
) -> Result<BatchRenameReport, JsflDiagnostic> {
    let mut candidate = project.clone();
    let mut report = BatchRenameReport {
        renamed_symbols: Vec::new(),
        renamed_instances: Vec::new(),
        undo_label: "Batch Rename Library & Instances".to_owned(),
    };

    let target_set: Option<HashSet<Id>> = options
        .target_symbol_ids
        .as_ref()
        .map(|ids| ids.iter().cloned().collect());

    let mut symbol_old_to_new: HashMap<Id, (String, String)> = HashMap::new();

    if matches!(
        options.scope,
        BatchRenameScope::LibraryItemsOnly | BatchRenameScope::LibraryAndInstances
    ) {
        let mut seq_idx = 0u32;
        let scene_ids: HashSet<Id> = candidate.scene_ids().into_iter().collect();
        for sym in &mut candidate.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            if let Some(targets) = &target_set
                && !targets.contains(&sym.id)
            {
                continue;
            }
            if let Some(filter) = &options.name_filter
                && !filter.is_empty()
                && !wildcard_matches(filter, &sym.name)
            {
                continue;
            }
            let new_name = format_renamed(&sym.name, options, seq_idx)?;
            seq_idx += 1;
            if new_name != sym.name {
                let old_name = sym.name.clone();
                sym.name = new_name.clone();
                symbol_old_to_new.insert(sym.id.clone(), (old_name.clone(), new_name.clone()));
                report
                    .renamed_symbols
                    .push((sym.id.clone(), old_name, new_name));
            }
        }
    }

    if matches!(
        options.scope,
        BatchRenameScope::StageInstancesOnly | BatchRenameScope::LibraryAndInstances
    ) {
        let mut inst_seq_idx = 0u32;
        for sym in &mut candidate.symbols {
            let holder_sym_id = sym.id.clone();
            for layer in &mut sym.timeline.layers {
                let layer_id = layer.id.clone();
                for key in &mut layer.keyframes {
                    let kf_frame = key.frame;
                    let mut element_renames: Vec<(String, String)> = Vec::new();
                    for el in &mut key.elements {
                        let ElementKind::Instance { symbol_id, .. } = &el.kind else {
                            continue;
                        };
                        if let Some(targets) = &target_set
                            && !targets.contains(symbol_id)
                        {
                            continue;
                        }
                        let source_name = if el.name.is_empty() {
                            symbol_old_to_new
                                .get(symbol_id)
                                .map(|(old, _)| old.as_str())
                                .unwrap_or("")
                        } else {
                            el.name.as_str()
                        };
                        if let Some(filter) = &options.name_filter
                            && !filter.is_empty()
                        {
                            let matches_el = wildcard_matches(filter, &el.name);
                            let matches_sym = symbol_old_to_new.contains_key(symbol_id)
                                || (!source_name.is_empty()
                                    && wildcard_matches(filter, source_name));
                            if !matches_el && !matches_sym {
                                continue;
                            }
                        }
                        let new_inst_name = if options.scope
                            == BatchRenameScope::LibraryAndInstances
                            && options.sync_instance_names_to_symbol
                            && let Some((old_sym_name, new_sym_name)) =
                                symbol_old_to_new.get(symbol_id)
                        {
                            if el.name.is_empty() || el.name == *old_sym_name {
                                format!("{new_sym_name}_{:02}", inst_seq_idx + 1)
                            } else if el.name.contains(old_sym_name.as_str()) {
                                el.name.replace(old_sym_name.as_str(), new_sym_name)
                            } else {
                                format_renamed(&el.name, options, inst_seq_idx)?
                            }
                        } else {
                            let base_for_inst = if source_name.is_empty() {
                                "instance"
                            } else {
                                source_name
                            };
                            format_renamed(base_for_inst, options, inst_seq_idx)?
                        };
                        inst_seq_idx += 1;
                        if new_inst_name != el.name {
                            let old_name = el.name.clone();
                            el.name = new_inst_name.clone();
                            if !old_name.is_empty() {
                                element_renames.push((old_name.clone(), new_inst_name.clone()));
                            }
                            report.renamed_instances.push((
                                holder_sym_id.clone(),
                                layer_id.clone(),
                                kf_frame,
                                el.id.clone(),
                                old_name,
                                new_inst_name,
                            ));
                        }
                    }
                    for (old_target, new_target) in element_renames {
                        for ov in &mut key.instance_overrides {
                            if ov.target_name == old_target {
                                ov.target_name = new_target.clone();
                            }
                        }
                    }
                }
            }
        }
    }

    candidate.upgrade_format_for_features();
    let issues = validate(&candidate);
    if let Some(first) = issues.first() {
        return Err(JsflDiagnostic::new(
            "<batch_rename>",
            1,
            1,
            format!(
                "Project validation failed after batch rename ({}): {}",
                first.path, first.message
            ),
        ));
    }

    *project = candidate;
    Ok(report)
}

/// Summary of a unique colour used across a `Project` (`collect_project_colors`, `T-197`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectColorEntry {
    /// Unmultiplied `[R, G, B, A]` colour.
    pub color: Color,
    /// Formatted hex string (`#rrggbb` when opaque, `#rrggbbaa` when `alpha < 255`).
    pub hex: String,
    /// Number of solid fill slots using this colour.
    pub fill_count: usize,
    /// Number of solid stroke slots using this colour.
    pub stroke_count: usize,
    /// Number of gradient stops (linear, radial, freeform, gradient mesh) using this colour.
    pub gradient_count: usize,
    /// Number of text elements / text format runs using this colour.
    pub text_count: usize,
    /// Whether this colour is also stored in the document's custom or global swatches.
    pub in_custom_swatches: bool,
}

impl ProjectColorEntry {
    pub fn total_uses(&self) -> usize {
        self.fill_count
            + self.stroke_count
            + self.gradient_count
            + self.text_count
            + usize::from(self.in_custom_swatches)
    }

    pub fn roles_summary(&self) -> String {
        let mut roles = Vec::new();
        if self.fill_count > 0 {
            roles.push("fill");
        }
        if self.stroke_count > 0 {
            roles.push("stroke");
        }
        if self.gradient_count > 0 {
            roles.push("gradient");
        }
        if self.text_count > 0 {
            roles.push("text");
        }
        if self.in_custom_swatches && roles.is_empty() {
            roles.push("swatch");
        }
        roles.join(", ")
    }
}

/// Options for project-wide or scoped colour replacement (`change_project_color`, `T-197`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeColorOptions {
    pub from: Color,
    pub to: Color,
    /// When `true`, matches `[R, G, B]` regardless of the slot's alpha channel.
    pub match_rgb_only: bool,
    /// When `true`, preserves each matched slot's original alpha channel instead of overwriting with `to[3]`.
    pub preserve_slot_alpha: bool,
    /// Optional filter restricting replacement to specific symbol IDs (`None` = all symbols).
    pub target_symbol_ids: Option<Vec<Id>>,
    /// Optional filter restricting replacement to specific `(symbol_id, layer_id, frame, element_id)` placements.
    pub target_elements: Option<Vec<(Id, Id, u32, Id)>>,
    /// Whether to also update matching entries in `project.custom_swatches()` and `project.global_swatches()`.
    pub include_custom_swatches: bool,
}

impl ChangeColorOptions {
    pub fn new(from: Color, to: Color) -> Self {
        Self {
            from,
            to,
            match_rgb_only: false,
            preserve_slot_alpha: false,
            target_symbol_ids: None,
            target_elements: None,
            include_custom_swatches: true,
        }
    }
}

/// Summary of colour slots replaced by `change_project_color` or `doc.changeColor(...)` (`T-197`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeColorReport {
    pub replaced_fills: usize,
    pub replaced_strokes: usize,
    pub replaced_gradient_stops: usize,
    pub replaced_texts: usize,
    pub replaced_swatches: usize,
    /// `(symbol_id, layer_id, keyframe_frame, element_id)`
    pub affected_elements: Vec<(Id, Id, u32, Id)>,
    pub undo_label: String,
}

impl ChangeColorReport {
    pub fn total_replaced(&self) -> usize {
        self.replaced_fills
            + self.replaced_strokes
            + self.replaced_gradient_stops
            + self.replaced_texts
            + self.replaced_swatches
    }
}

fn format_rgba_hex(c: Color) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// Canonical JSFL `change_color.jsfl` script (`T-197`).
///
/// Traverses `fl.getDocumentDOM()`, all scene timelines (`doc.timelines`) and all Library symbol
/// timelines (`doc.library.items`), inspecting every keyframe's elements and replacing `fromColor`
/// with `toColor` across solid fills, linear/radial gradient `colorArray` stops, strokes, and text.
pub const CHANGE_COLOR_JSFL_SCRIPT: &str = r##"
function normalizeHex(hex) {
    var s = String(hex).trim().toLowerCase();
    if (s.indexOf("#") !== 0) {
        s = "#" + s;
    }
    return s;
}

function changeColorInTimeline(tl, targetHex, replacementHex) {
    var replaced = 0;
    var layers = tl.layers;
    for (var l = 0; l < layers.length; l++) {
        var layer = layers[l];
        var frames = layer.frames;
        for (var f = 0; f < frames.length; f++) {
            var frame = frames[f];
            if (frame.startFrame !== f) {
                continue;
            }
            var elements = frame.elements;
            for (var e = 0; e < elements.length; e++) {
                var el = elements[e];
                var et = el.elementType;
                if (et === "shape" || et === "shapeObj") {
                    var fill = el.getCustomFill();
                    if (fill.style === "solid" && normalizeHex(fill.color) === targetHex) {
                        fill.color = replacementHex;
                        el.setCustomFill(fill);
                        replaced++;
                    } else if (fill.colorArray && fill.colorArray.length > 0) {
                        var fillChanged = false;
                        for (var k = 0; k < fill.colorArray.length; k++) {
                            if (normalizeHex(fill.colorArray[k]) === targetHex) {
                                fill.colorArray[k] = replacementHex;
                                fillChanged = true;
                                replaced++;
                            }
                        }
                        if (fillChanged) {
                            el.setCustomFill(fill);
                        }
                    }
                    var stroke = el.getCustomStroke();
                    if (stroke.style === "solid" && normalizeHex(stroke.color) === targetHex) {
                        stroke.color = replacementHex;
                        el.setCustomStroke(stroke);
                        replaced++;
                    } else if (stroke.colorArray && stroke.colorArray.length > 0) {
                        var strokeChanged = false;
                        for (var sk = 0; sk < stroke.colorArray.length; sk++) {
                            if (normalizeHex(stroke.colorArray[sk]) === targetHex) {
                                stroke.colorArray[sk] = replacementHex;
                                strokeChanged = true;
                                replaced++;
                            }
                        }
                        if (strokeChanged) {
                            el.setCustomStroke(stroke);
                        }
                    }
                } else if (et === "text") {
                    var tc = el.getTextAttr("fillColor");
                    if (tc && normalizeHex(tc) === targetHex) {
                        el.setTextAttr("fillColor", replacementHex);
                        replaced++;
                    }
                }
            }
        }
    }
    return replaced;
}

function changeColor(fromColor, toColor) {
    var doc = fl.getDocumentDOM();
    var targetHex = normalizeHex(fromColor);
    var replacementHex = normalizeHex(toColor);
    var total = 0;
    var scenes = doc.timelines;
    for (var s = 0; s < scenes.length; s++) {
        total += changeColorInTimeline(scenes[s], targetHex, replacementHex);
    }
    var items = doc.library.items;
    for (var i = 0; i < items.length; i++) {
        var item = items[i];
        if (item.itemType !== "folder" && item.timeline) {
            total += changeColorInTimeline(item.timeline, targetHex, replacementHex);
        }
    }
    fl.trace("Changed " + total + " colour occurrence(s) from " + targetHex + " to " + replacementHex);
    return total;
}
"##;

/// Execute the built-in `CHANGE_COLOR_JSFL_SCRIPT` (`change_color.jsfl`) in a sandboxed JSFL VM (`T-197`).
pub fn run_change_color_jsfl(
    project: &Project,
    state: &JsflAuthoringState,
    from: Color,
    to: Color,
) -> Result<JsflResult, JsflDiagnostic> {
    let from_hex = format_rgba_hex(from);
    let to_hex = format_rgba_hex(to);
    let mut sandbox = JsflSandbox::new();
    sandbox.register_virtual_script("file:///change_color.jsfl", CHANGE_COLOR_JSFL_SCRIPT);
    let invoke = format!(
        "fl.runScript(\"file:///change_color.jsfl\", \"changeColor\", {:?}, {:?});",
        from_hex, to_hex
    );
    sandbox.execute(project, state, "change_color.jsfl", &invoke)
}

/// Collect all unique visible colours used across `project` (or a scoped subset of symbols/elements)
/// in deterministic authoring order (`T-197`).
pub fn collect_project_colors(project: &Project) -> Vec<ProjectColorEntry> {
    collect_project_colors_scoped(project, None, None)
}

/// Collect unique visible colours restricted to `target_symbol_ids` and/or `target_elements` (`T-197`).
pub fn collect_project_colors_scoped(
    project: &Project,
    target_symbol_ids: Option<&HashSet<Id>>,
    target_elements: Option<&HashSet<(Id, Id, u32, Id)>>,
) -> Vec<ProjectColorEntry> {
    let mut entries: Vec<ProjectColorEntry> = Vec::new();
    let mut index_by_color: HashMap<Color, usize> = HashMap::new();

    #[derive(Clone, Copy)]
    enum Role {
        Fill,
        Stroke,
        Gradient,
        Text,
        Swatch,
    }

    let mut record = |c: Color, role: Role| {
        if c[3] == 0 {
            return;
        }
        let idx = *index_by_color.entry(c).or_insert_with(|| {
            let i = entries.len();
            entries.push(ProjectColorEntry {
                color: c,
                hex: format_rgba_hex(c),
                ..Default::default()
            });
            i
        });
        let entry = &mut entries[idx];
        match role {
            Role::Fill => entry.fill_count += 1,
            Role::Stroke => entry.stroke_count += 1,
            Role::Gradient => entry.gradient_count += 1,
            Role::Text => entry.text_count += 1,
            Role::Swatch => entry.in_custom_swatches = true,
        }
    };

    for sym in &project.symbols {
        if let Some(syms) = target_symbol_ids
            && !syms.contains(&sym.id)
        {
            continue;
        }
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            el.id.clone(),
                        ))
                    {
                        continue;
                    }
                    match &el.kind {
                        ElementKind::Rectangle {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            record(*fill, Role::Fill);
                            if *stroke_width > 0.0 {
                                record(*stroke, Role::Stroke);
                            }
                        }
                        ElementKind::PaintedPath {
                            fill,
                            stroke,
                            fill_gradient,
                            stroke_gradient,
                            stroke_width,
                            ..
                        } => {
                            if let Some(grad) = fill_gradient {
                                for stop in &grad.stops {
                                    record(stop.color, Role::Gradient);
                                }
                            } else {
                                record(*fill, Role::Fill);
                            }
                            if let Some(grad) = stroke_gradient {
                                for stop in &grad.stops {
                                    record(stop.color, Role::Gradient);
                                }
                            } else if *stroke_width > 0.0 {
                                record(*stroke, Role::Stroke);
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            record(*color, Role::Text);
                        }
                        ElementKind::TextBox { format, .. } => {
                            let c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            record(c, Role::Text);
                        }
                        _ => {}
                    }
                }
                for app in &key.appearances {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            app.element_id.clone(),
                        ))
                    {
                        continue;
                    }
                    for entry in &app.entries {
                        match entry {
                            crate::AppearancePaintEntry::Fill {
                                color, gradient, ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &grad.stops {
                                        record(s.color, Role::Gradient);
                                    }
                                } else {
                                    record(*color, Role::Fill);
                                }
                            }
                            crate::AppearancePaintEntry::Stroke {
                                color,
                                gradient,
                                width,
                                ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &grad.stops {
                                        record(s.color, Role::Gradient);
                                    }
                                } else if *width > 0.0 {
                                    record(*color, Role::Stroke);
                                }
                            }
                        }
                    }
                }
                for fx in &key.vector_effects {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            fx.element_id.clone(),
                        ))
                    {
                        continue;
                    }
                    if let Some(fg) = &fx.freeform_gradient {
                        for pt in &fg.points {
                            record(pt.color, Role::Gradient);
                        }
                        for line in &fg.lines {
                            record(line.start_color, Role::Gradient);
                            record(line.end_color, Role::Gradient);
                        }
                    }
                    if let Some(mesh) = &fx.gradient_mesh {
                        for v in &mesh.vertices {
                            record(v.color, Role::Gradient);
                        }
                    }
                }
            }
        }
    }

    if target_symbol_ids.is_none() && target_elements.is_none() {
        for sw in project.custom_swatches() {
            record(sw, Role::Swatch);
        }
        for gsw in project.global_swatches() {
            record(gsw.color, Role::Swatch);
        }
    }

    entries
}

/// Replace `options.from` with `options.to` across `project` (solid fills, strokes, linear/radial/
/// freeform/mesh gradients, text elements, appearance stacks, and custom/global swatches) and
/// validate atomically before committing (`T-197`).
pub fn change_project_color(
    project: &mut Project,
    options: &ChangeColorOptions,
) -> Result<ChangeColorReport, JsflDiagnostic> {
    let mut candidate = project.clone();
    let mut report = ChangeColorReport {
        undo_label: format!(
            "Change colour {} → {}",
            format_rgba_hex(options.from),
            format_rgba_hex(options.to)
        ),
        ..Default::default()
    };

    let target_syms: Option<HashSet<Id>> = options
        .target_symbol_ids
        .as_ref()
        .map(|v| v.iter().cloned().collect());
    let target_els: Option<HashSet<(Id, Id, u32, Id)>> = options
        .target_elements
        .as_ref()
        .map(|v| v.iter().cloned().collect());

    let matches_color = |c: Color| -> bool {
        if c[3] == 0 && options.from[3] > 0 {
            return false;
        }
        if options.match_rgb_only {
            c[0] == options.from[0] && c[1] == options.from[1] && c[2] == options.from[2]
        } else {
            c == options.from
        }
    };

    let apply_slot = |dst: &mut Color| -> bool {
        if !matches_color(*dst) {
            return false;
        }
        let mut next = options.to;
        if options.preserve_slot_alpha {
            next[3] = dst[3];
        }
        if *dst != next {
            *dst = next;
            true
        } else {
            false
        }
    };

    let mut push_affected = |loc: (Id, Id, u32, Id)| {
        if !report.affected_elements.contains(&loc) {
            report.affected_elements.push(loc);
        }
    };

    for sym in &mut candidate.symbols {
        if let Some(syms) = &target_syms
            && !syms.contains(&sym.id)
        {
            continue;
        }
        let sym_id = sym.id.clone();
        for layer in &mut sym.timeline.layers {
            let layer_id = layer.id.clone();
            for key in &mut layer.keyframes {
                let kf_frame = key.frame;
                for el in &mut key.elements {
                    let loc = (sym_id.clone(), layer_id.clone(), kf_frame, el.id.clone());
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut el_changed = false;
                    match &mut el.kind {
                        ElementKind::Rectangle {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            if apply_slot(fill) {
                                report.replaced_fills += 1;
                                el_changed = true;
                            }
                            if *stroke_width > 0.0 && apply_slot(stroke) {
                                report.replaced_strokes += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::PaintedPath {
                            fill,
                            stroke,
                            fill_gradient,
                            stroke_gradient,
                            stroke_width,
                            ..
                        } => {
                            if let Some(grad) = fill_gradient {
                                let mut grad_changed = false;
                                for stop in &mut grad.stops {
                                    if apply_slot(&mut stop.color) {
                                        report.replaced_gradient_stops += 1;
                                        grad_changed = true;
                                        el_changed = true;
                                    }
                                }
                                if grad_changed && let Some(first) = grad.stops.first() {
                                    *fill = first.color;
                                }
                            } else if apply_slot(fill) {
                                report.replaced_fills += 1;
                                el_changed = true;
                            }
                            if let Some(grad) = stroke_gradient {
                                let mut grad_changed = false;
                                for stop in &mut grad.stops {
                                    if apply_slot(&mut stop.color) {
                                        report.replaced_gradient_stops += 1;
                                        grad_changed = true;
                                        el_changed = true;
                                    }
                                }
                                if grad_changed && let Some(first) = grad.stops.first() {
                                    *stroke = first.color;
                                }
                            } else if *stroke_width > 0.0 && apply_slot(stroke) {
                                report.replaced_strokes += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            if apply_slot(color) {
                                report.replaced_texts += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::TextBox { format, .. } => {
                            let mut c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            if apply_slot(&mut c) {
                                format.color = (u32::from(c[0]) << 16)
                                    | (u32::from(c[1]) << 8)
                                    | u32::from(c[2]);
                                report.replaced_texts += 1;
                                el_changed = true;
                            }
                        }
                        _ => {}
                    }
                    if el_changed {
                        push_affected(loc);
                    }
                }
                for app in &mut key.appearances {
                    let loc = (
                        sym_id.clone(),
                        layer_id.clone(),
                        kf_frame,
                        app.element_id.clone(),
                    );
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut app_changed = false;
                    for entry in &mut app.entries {
                        match entry {
                            crate::AppearancePaintEntry::Fill {
                                color, gradient, ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &mut grad.stops {
                                        if apply_slot(&mut s.color) {
                                            report.replaced_gradient_stops += 1;
                                            app_changed = true;
                                        }
                                    }
                                } else if apply_slot(color) {
                                    report.replaced_fills += 1;
                                    app_changed = true;
                                }
                            }
                            crate::AppearancePaintEntry::Stroke {
                                color,
                                gradient,
                                width,
                                ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &mut grad.stops {
                                        if apply_slot(&mut s.color) {
                                            report.replaced_gradient_stops += 1;
                                            app_changed = true;
                                        }
                                    }
                                } else if *width > 0.0 && apply_slot(color) {
                                    report.replaced_strokes += 1;
                                    app_changed = true;
                                }
                            }
                        }
                    }
                    if app_changed {
                        push_affected(loc);
                    }
                }
                for fx in &mut key.vector_effects {
                    let loc = (
                        sym_id.clone(),
                        layer_id.clone(),
                        kf_frame,
                        fx.element_id.clone(),
                    );
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut fx_changed = false;
                    if let Some(fg) = &mut fx.freeform_gradient {
                        for pt in &mut fg.points {
                            if apply_slot(&mut pt.color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                        for line in &mut fg.lines {
                            if apply_slot(&mut line.start_color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                            if apply_slot(&mut line.end_color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                    }
                    if let Some(mesh) = &mut fx.gradient_mesh {
                        for v in &mut mesh.vertices {
                            if apply_slot(&mut v.color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                    }
                    if fx_changed {
                        push_affected(loc);
                    }
                }
            }
        }
    }

    if options.include_custom_swatches && target_syms.is_none() && target_els.is_none() {
        let mut custom = candidate.custom_swatches();
        let mut custom_changed = false;
        for sw in &mut custom {
            if apply_slot(sw) {
                report.replaced_swatches += 1;
                custom_changed = true;
            }
        }
        if custom_changed {
            let mut deduped = Vec::with_capacity(custom.len());
            for sw in custom {
                if !deduped.contains(&sw) {
                    deduped.push(sw);
                }
            }
            candidate.set_custom_swatches(&deduped);
        }

        let mut global = candidate.global_swatches();
        let mut global_changed = false;
        for gsw in &mut global {
            if apply_slot(&mut gsw.color) {
                report.replaced_swatches += 1;
                global_changed = true;
            }
        }
        if global_changed {
            candidate.set_global_swatches(&global);
        }
    }

    candidate.upgrade_format_for_features();
    let issues = validate(&candidate);
    if let Some(first) = issues.first() {
        return Err(JsflDiagnostic::new(
            "<change_color>",
            1,
            1,
            format!(
                "Project validation failed after colour replacement ({}): {}",
                first.path, first.message
            ),
        ));
    }

    *project = candidate;
    Ok(report)
}

// ---------------------------------------------------------------------------
// Lexer & Recursive-Descent Parser for JSFL
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Span {
    line: u32,
    column: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(f64),
    Str(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Semi,
    Colon,
    Question,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    PlusPlus,
    MinusMinus,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    EqEq,
    BangEq,
    EqEqEq,
    BangEqEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

fn tokenize(file: &str, src: &str) -> Result<Vec<Token>, JsflDiagnostic> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;
    let mut tokens = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        let span = Span { line, column: col };
        if c == '\n' {
            line += 1;
            col = 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            col += 1;
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                col += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            col += 2;
            let mut closed = false;
            while i + 1 < chars.len() {
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    col += 2;
                    closed = true;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
            if !closed {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    "Unterminated block comment.",
                ));
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            col += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == quote {
                    i += 1;
                    col += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    i += 1;
                    col += 1;
                    if i >= chars.len() {
                        break;
                    }
                    let esc = chars[i];
                    match esc {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        other => s.push(other),
                    }
                    i += 1;
                    col += 1;
                } else {
                    if ch == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    s.push(ch);
                    i += 1;
                }
            }
            if !closed {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    "Unterminated string literal.",
                ));
            }
            tokens.push(Token {
                kind: TokenKind::Str(s),
                span,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                i += 2;
                col += 2;
                let hex_start = i;
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    i += 1;
                    col += 1;
                }
                let hex_str: String = chars[hex_start..i].iter().collect();
                let val = u64::from_str_radix(&hex_str, 16).map_err(|_| {
                    JsflDiagnostic::new(file, span.line, span.column, "Invalid hex literal.")
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(val as f64),
                    span,
                });
                continue;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
                col += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let val = num_str.parse::<f64>().map_err(|_| {
                JsflDiagnostic::new(file, span.line, span.column, "Invalid numeric literal.")
            })?;
            tokens.push(Token {
                kind: TokenKind::Number(val),
                span,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
                col += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Ident(ident),
                span,
            });
            continue;
        }

        let next = chars.get(i + 1).copied();
        let next2 = chars.get(i + 2).copied();
        let (kind, advance) = match (c, next, next2) {
            ('=', Some('='), Some('=')) => (TokenKind::EqEqEq, 3),
            ('!', Some('='), Some('=')) => (TokenKind::BangEqEq, 3),
            ('=', Some('='), _) => (TokenKind::EqEq, 2),
            ('!', Some('='), _) => (TokenKind::BangEq, 2),
            ('<', Some('='), _) => (TokenKind::LtEq, 2),
            ('>', Some('='), _) => (TokenKind::GtEq, 2),
            ('&', Some('&'), _) => (TokenKind::AndAnd, 2),
            ('|', Some('|'), _) => (TokenKind::OrOr, 2),
            ('+', Some('+'), _) => (TokenKind::PlusPlus, 2),
            ('-', Some('-'), _) => (TokenKind::MinusMinus, 2),
            ('+', Some('='), _) => (TokenKind::PlusEq, 2),
            ('-', Some('='), _) => (TokenKind::MinusEq, 2),
            ('*', Some('='), _) => (TokenKind::StarEq, 2),
            ('/', Some('='), _) => (TokenKind::SlashEq, 2),
            ('(', _, _) => (TokenKind::LParen, 1),
            (')', _, _) => (TokenKind::RParen, 1),
            ('{', _, _) => (TokenKind::LBrace, 1),
            ('}', _, _) => (TokenKind::RBrace, 1),
            ('[', _, _) => (TokenKind::LBracket, 1),
            (']', _, _) => (TokenKind::RBracket, 1),
            ('.', _, _) => (TokenKind::Dot, 1),
            (',', _, _) => (TokenKind::Comma, 1),
            (';', _, _) => (TokenKind::Semi, 1),
            (':', _, _) => (TokenKind::Colon, 1),
            ('?', _, _) => (TokenKind::Question, 1),
            ('+', _, _) => (TokenKind::Plus, 1),
            ('-', _, _) => (TokenKind::Minus, 1),
            ('*', _, _) => (TokenKind::Star, 1),
            ('/', _, _) => (TokenKind::Slash, 1),
            ('%', _, _) => (TokenKind::Percent, 1),
            ('!', _, _) => (TokenKind::Bang, 1),
            ('=', _, _) => (TokenKind::Eq, 1),
            ('<', _, _) => (TokenKind::Lt, 1),
            ('>', _, _) => (TokenKind::Gt, 1),
            _ => {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    format!("Unexpected character '{c}'."),
                ));
            }
        };
        tokens.push(Token { kind, span });
        i += advance;
        col += advance as u32;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { line, column: col },
    });
    Ok(tokens)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnaryOp {
    Not,
    Neg,
    Pos,
    Typeof,
}

#[derive(Clone, Debug)]
struct Expr {
    kind: ExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
enum ExprKind {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Ident(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Member {
        object: Box<Expr>,
        property: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Update {
        target: Box<Expr>,
        increment: bool,
        prefix: bool,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Conditional {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
struct Stmt {
    kind: StmtKind,
    span: Span,
}

#[derive(Clone, Debug)]
enum StmtKind {
    Block(Vec<Stmt>),
    VarDecl(Vec<(String, Option<Expr>)>),
    FuncDecl {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForOf {
        var_name: String,
        iterable: Expr,
        body: Box<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    TryCatch {
        try_block: Vec<Stmt>,
        catch_param: Option<String>,
        catch_block: Vec<Stmt>,
    },
    Expr(Expr),
    Empty,
}

struct Parser<'a> {
    file: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(file: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            file,
            tokens,
            pos: 0,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn span(&self) -> Span {
        self.peek().span
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check_ident(&self, kw: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(s) if s == kw)
    }

    fn match_ident(&mut self, kw: &str) -> bool {
        if self.check_ident(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_kind(&mut self, expected: &TokenKind) -> bool {
        if &self.peek().kind == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_kind(&mut self, expected: &TokenKind, msg: &str) -> Result<Token, JsflDiagnostic> {
        let tok = self.peek().clone();
        if &tok.kind == expected {
            Ok(self.advance())
        } else {
            Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                msg,
            ))
        }
    }

    fn expect_ident(&mut self, msg: &str) -> Result<String, JsflDiagnostic> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = tok.kind {
            self.advance();
            Ok(name)
        } else {
            Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                msg,
            ))
        }
    }

    fn consume_optional_semi(&mut self) {
        let _ = self.match_kind(&TokenKind::Semi);
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, JsflDiagnostic> {
        let mut stmts = Vec::new();
        while !self.is_eof() {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, JsflDiagnostic> {
        self.expect_kind(&TokenKind::LBrace, "Expected '{'.")?;
        let mut stmts = Vec::new();
        while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect_kind(&TokenKind::RBrace, "Expected '}'.")?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, JsflDiagnostic> {
        let span = self.span();
        if self.match_kind(&TokenKind::Semi) {
            return Ok(Stmt {
                kind: StmtKind::Empty,
                span,
            });
        }
        if matches!(self.peek().kind, TokenKind::LBrace) {
            return Ok(Stmt {
                kind: StmtKind::Block(self.parse_block()?),
                span,
            });
        }
        if self.match_ident("var") || self.match_ident("let") || self.match_ident("const") {
            let decl = self.parse_var_decl_list()?;
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::VarDecl(decl),
                span,
            });
        }
        if self.match_ident("function") {
            let name = self.expect_ident("Expected function name.")?;
            self.expect_kind(&TokenKind::LParen, "Expected '(' after function name.")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokenKind::RParen) {
                loop {
                    params.push(self.expect_ident("Expected parameter name.")?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect_kind(&TokenKind::RParen, "Expected ')' after parameters.")?;
            let body = self.parse_block()?;
            return Ok(Stmt {
                kind: StmtKind::FuncDecl { name, params, body },
                span,
            });
        }
        if self.match_ident("if") {
            self.expect_kind(&TokenKind::LParen, "Expected '(' after 'if'.")?;
            let cond = self.parse_expr()?;
            self.expect_kind(&TokenKind::RParen, "Expected ')' after if condition.")?;
            let then_branch = Box::new(self.parse_stmt()?);
            let else_branch = if self.match_ident("else") {
                Some(Box::new(self.parse_stmt()?))
            } else {
                None
            };
            return Ok(Stmt {
                kind: StmtKind::If {
                    cond,
                    then_branch,
                    else_branch,
                },
                span,
            });
        }
        if self.match_ident("while") {
            self.expect_kind(&TokenKind::LParen, "Expected '(' after 'while'.")?;
            let cond = self.parse_expr()?;
            self.expect_kind(&TokenKind::RParen, "Expected ')' after while condition.")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt {
                kind: StmtKind::While { cond, body },
                span,
            });
        }
        if self.match_ident("for") {
            return self.parse_for_stmt(span);
        }
        if self.match_ident("return") {
            let expr = if matches!(
                self.peek().kind,
                TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
            ) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Return(expr),
                span,
            });
        }
        if self.match_ident("break") {
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Break,
                span,
            });
        }
        if self.match_ident("continue") {
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Continue,
                span,
            });
        }
        if self.match_ident("throw") {
            let expr = self.parse_expr()?;
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Throw(expr),
                span,
            });
        }
        if self.match_ident("try") {
            let try_block = self.parse_block()?;
            self.expect_ident("Expected 'catch' after try block.")?;
            let catch_param = if self.match_kind(&TokenKind::LParen) {
                let p = self.expect_ident("Expected catch parameter name.")?;
                self.expect_kind(&TokenKind::RParen, "Expected ')' after catch parameter.")?;
                Some(p)
            } else {
                None
            };
            let catch_block = self.parse_block()?;
            return Ok(Stmt {
                kind: StmtKind::TryCatch {
                    try_block,
                    catch_param,
                    catch_block,
                },
                span,
            });
        }

        let expr = self.parse_expr()?;
        self.consume_optional_semi();
        Ok(Stmt {
            kind: StmtKind::Expr(expr),
            span,
        })
    }

    fn parse_var_decl_list(&mut self) -> Result<Vec<(String, Option<Expr>)>, JsflDiagnostic> {
        let mut list = Vec::new();
        loop {
            let name = self.expect_ident("Expected variable name.")?;
            let init = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            list.push((name, init));
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }
        Ok(list)
    }

    fn parse_for_stmt(&mut self, span: Span) -> Result<Stmt, JsflDiagnostic> {
        self.expect_kind(&TokenKind::LParen, "Expected '(' after 'for'.")?;
        let has_var =
            self.match_ident("var") || self.match_ident("let") || self.match_ident("const");
        if has_var {
            let first_name = self.expect_ident("Expected loop variable name.")?;
            if self.match_ident("of") || self.match_ident("in") {
                let iterable = self.parse_expr()?;
                self.expect_kind(&TokenKind::RParen, "Expected ')' after for-of header.")?;
                let body = Box::new(self.parse_stmt()?);
                return Ok(Stmt {
                    kind: StmtKind::ForOf {
                        var_name: first_name,
                        iterable,
                        body,
                    },
                    span,
                });
            }
            let first_init = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            let mut decls = vec![(first_name, first_init)];
            while self.match_kind(&TokenKind::Comma) {
                let n = self.expect_ident("Expected variable name.")?;
                let v = if self.match_kind(&TokenKind::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                decls.push((n, v));
            }
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for initializer.")?;
            let cond = if matches!(self.peek().kind, TokenKind::Semi) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for condition.")?;
            let update = if matches!(self.peek().kind, TokenKind::RParen) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_kind(&TokenKind::RParen, "Expected ')' after for clauses.")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt {
                kind: StmtKind::For {
                    init: Some(Box::new(Stmt {
                        kind: StmtKind::VarDecl(decls),
                        span,
                    })),
                    cond,
                    update,
                    body,
                },
                span,
            });
        }

        let init = if self.match_kind(&TokenKind::Semi) {
            None
        } else {
            let e = self.parse_expr()?;
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for initializer.")?;
            Some(Box::new(Stmt {
                kind: StmtKind::Expr(e),
                span,
            }))
        };
        let cond = if matches!(self.peek().kind, TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_kind(&TokenKind::Semi, "Expected ';' after for condition.")?;
        let update = if matches!(self.peek().kind, TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_kind(&TokenKind::RParen, "Expected ')' after for clauses.")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt {
            kind: StmtKind::For {
                init,
                cond,
                update,
                body,
            },
            span,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, JsflDiagnostic> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, JsflDiagnostic> {
        let left = self.parse_conditional()?;
        let op = match self.peek().kind {
            TokenKind::Eq => Some(AssignOp::Assign),
            TokenKind::PlusEq => Some(AssignOp::AddAssign),
            TokenKind::MinusEq => Some(AssignOp::SubAssign),
            TokenKind::StarEq => Some(AssignOp::MulAssign),
            TokenKind::SlashEq => Some(AssignOp::DivAssign),
            _ => None,
        };
        if let Some(op) = op {
            let span = self.advance().span;
            let right = self.parse_assignment()?;
            return Ok(Expr {
                kind: ExprKind::Assign {
                    op,
                    target: Box::new(left),
                    value: Box::new(right),
                },
                span,
            });
        }
        Ok(left)
    }

    fn parse_conditional(&mut self) -> Result<Expr, JsflDiagnostic> {
        let expr = self.parse_logical_or()?;
        if self.match_kind(&TokenKind::Question) {
            let span = expr.span;
            let then_expr = self.parse_assignment()?;
            self.expect_kind(&TokenKind::Colon, "Expected ':' in conditional expression.")?;
            let else_expr = self.parse_assignment()?;
            return Ok(Expr {
                kind: ExprKind::Conditional {
                    cond: Box::new(expr),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                },
                span,
            });
        }
        Ok(expr)
    }

    fn parse_logical_or(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_logical_and()?;
        while self.match_kind(&TokenKind::OrOr) {
            let span = left.span;
            let right = self.parse_logical_and()?;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_equality()?;
        while self.match_kind(&TokenKind::AndAnd) {
            let span = left.span;
            let right = self.parse_equality()?;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_relational()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq => Some(BinaryOp::Eq),
                TokenKind::BangEq => Some(BinaryOp::Ne),
                TokenKind::EqEqEq => Some(BinaryOp::StrictEq),
                TokenKind::BangEqEq => Some(BinaryOp::StrictNe),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_relational()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Lt => Some(BinaryOp::Lt),
                TokenKind::LtEq => Some(BinaryOp::Le),
                TokenKind::Gt => Some(BinaryOp::Gt),
                TokenKind::GtEq => Some(BinaryOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_additive()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_multiplicative()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => Some(BinaryOp::Mul),
                TokenKind::Slash => Some(BinaryOp::Div),
                TokenKind::Percent => Some(BinaryOp::Rem),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_unary()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, JsflDiagnostic> {
        let span = self.span();
        if self.match_kind(&TokenKind::Bang) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::Minus) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::Plus) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_ident("typeof") {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Typeof,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_ident("new") {
            return self.parse_postfix();
        }
        if self.match_kind(&TokenKind::PlusPlus) {
            return Ok(Expr {
                kind: ExprKind::Update {
                    target: Box::new(self.parse_postfix()?),
                    increment: true,
                    prefix: true,
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::MinusMinus) {
            return Ok(Expr {
                kind: ExprKind::Update {
                    target: Box::new(self.parse_postfix()?),
                    increment: false,
                    prefix: true,
                },
                span,
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut expr = self.parse_primary()?;
        loop {
            let span = self.span();
            if self.match_kind(&TokenKind::Dot) {
                let property = self.expect_ident("Expected property name after '.'.")?;
                expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(expr),
                        property,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::LBracket) {
                let index = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBracket, "Expected ']' after index expression.")?;
                expr = Expr {
                    kind: ExprKind::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect_kind(&TokenKind::RParen, "Expected ')' after call arguments.")?;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::PlusPlus) {
                expr = Expr {
                    kind: ExprKind::Update {
                        target: Box::new(expr),
                        increment: true,
                        prefix: false,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::MinusMinus) {
                expr = Expr {
                    kind: ExprKind::Update {
                        target: Box::new(expr),
                        increment: false,
                        prefix: false,
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, JsflDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Number(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Number(n),
                    span: tok.span,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Str(s),
                    span: tok.span,
                })
            }
            TokenKind::Ident(ident) => {
                self.advance();
                let kind = match ident.as_str() {
                    "true" => ExprKind::Bool(true),
                    "false" => ExprKind::Bool(false),
                    "null" => ExprKind::Null,
                    "undefined" => ExprKind::Undefined,
                    _ => ExprKind::Ident(ident),
                };
                Ok(Expr {
                    kind,
                    span: tok.span,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect_kind(&TokenKind::RParen, "Expected ')'.")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBracket) {
                    items.push(self.parse_expr()?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RBracket, "Expected ']'.")?;
                Ok(Expr {
                    kind: ExprKind::Array(items),
                    span: tok.span,
                })
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                    let key = match self.peek().kind.clone() {
                        TokenKind::Ident(k) | TokenKind::Str(k) => {
                            self.advance();
                            k
                        }
                        _ => {
                            return Err(JsflDiagnostic::new(
                                self.file,
                                self.span().line,
                                self.span().column,
                                "Expected object property key.",
                            ));
                        }
                    };
                    self.expect_kind(&TokenKind::Colon, "Expected ':' after object property key.")?;
                    let val = self.parse_expr()?;
                    entries.push((key, val));
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RBrace, "Expected '}'.")?;
                Ok(Expr {
                    kind: ExprKind::Object(entries),
                    span: tok.span,
                })
            }
            _ => Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                format!("Unexpected token {:?} in expression.", tok.kind),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// JsflSandbox & Capability-Gated FLfile Helpers
// ---------------------------------------------------------------------------

/// Sandboxed, deterministic, interruptible JSFL executor (`T-187`).
#[derive(Clone, Debug, Default)]
pub struct JsflSandbox {
    pub limits: JsflLimits,
    pub flfile_caps: FlFileCapabilities,
    pub version: String,
    pub config_uri: String,
    pub script_uri: String,
    pub virtual_scripts: HashMap<String, String>,
}

impl JsflSandbox {
    pub fn new() -> Self {
        Self {
            limits: JsflLimits::default(),
            flfile_caps: FlFileCapabilities::default(),
            version: "NF2 12,0,0,0".to_owned(),
            config_uri: "file:///NeoFlash/Configuration/".to_owned(),
            script_uri: "file:///NeoFlash/Commands/script.jsfl".to_owned(),
            virtual_scripts: HashMap::new(),
        }
    }

    pub fn with_limits(mut self, limits: JsflLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_flfile_capabilities(mut self, caps: FlFileCapabilities) -> Self {
        self.flfile_caps = caps;
        self
    }

    pub fn register_virtual_script(&mut self, uri: impl Into<String>, source: impl Into<String>) {
        self.virtual_scripts.insert(uri.into(), source.into());
    }

    /// Execute `source` against a cloned candidate `Project` and `JsflAuthoringState`,
    /// validating the candidate project before returning `JsflResult`.
    pub fn execute(
        &self,
        project: &Project,
        state: &JsflAuthoringState,
        script_name: &str,
        source: &str,
    ) -> Result<JsflResult, JsflDiagnostic> {
        if source.len() > self.limits.max_source_bytes {
            return Err(JsflDiagnostic::new(
                script_name,
                1,
                1,
                format!(
                    "JSFL script exceeds maximum source size ({} > {} bytes).",
                    source.len(),
                    self.limits.max_source_bytes
                ),
            ));
        }
        let tokens = tokenize(script_name, source)?;
        let mut parser = Parser::new(script_name, tokens);
        let stmts = parser.parse_program()?;

        let mut vm = JsflVm::new(self, project.clone(), state.clone(), script_name.to_owned());
        let return_value = vm.run_program(&stmts, Span { line: 1, column: 1 })?;

        // Normalize and validate candidate project before committing.
        for sym in &mut vm.project.symbols {
            for layer in &mut sym.timeline.layers {
                layer.normalize_motion_keys();
            }
        }
        vm.project.prune_authoring_groups();
        vm.project.prune_motion_guides();
        vm.project.upgrade_format_for_features();

        let issues = validate(&vm.project);
        if let Some(first) = issues.first() {
            return Err(JsflDiagnostic::new(
                script_name,
                vm.last_span.line,
                vm.last_span.column,
                format!(
                    "Candidate project failed validation after JSFL execution ({}): {}",
                    first.path, first.message
                ),
            ));
        }

        if vm.mutations > 0 {
            vm.state.dirty = true;
        }
        let clean_name = script_name
            .rsplit('/')
            .next()
            .unwrap_or(script_name)
            .trim_end_matches(".jsfl");
        let undo_label = if clean_name.is_empty() {
            "Run JSFL Command".to_owned()
        } else {
            format!("Run JSFL: {clean_name}")
        };

        Ok(JsflResult {
            project: vm.project,
            state: vm.state,
            output_lines: vm.output_lines,
            undo_label,
            mutations: vm.mutations,
            steps_executed: vm.steps,
            return_value,
        })
    }

    /// Execute `source` and commit `project` and `state` in place only if execution and
    /// `project.validate()` succeed; otherwise leave `project` and `state` untouched.
    pub fn execute_in_place(
        &self,
        project: &mut Project,
        state: &mut JsflAuthoringState,
        script_name: &str,
        source: &str,
    ) -> Result<JsflResult, JsflDiagnostic> {
        let res = self.execute(project, state, script_name, source)?;
        *project = res.project.clone();
        *state = res.state.clone();
        Ok(res)
    }
}

fn parse_color_value(val: &JsflValue) -> Option<Color> {
    match val {
        JsflValue::String(s) => {
            let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some([r, g, b, 255])
            } else if hex.len() == 8 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some([r, g, b, a])
            } else {
                None
            }
        }
        JsflValue::Number(n) => {
            if !n.is_finite() || *n < 0.0 || *n > 0xFF_FFFF as f64 {
                return None;
            }
            let u = *n as u32;
            Some([
                ((u >> 16) & 0xFF) as u8,
                ((u >> 8) & 0xFF) as u8,
                (u & 0xFF) as u8,
                255,
            ])
        }
        _ => None,
    }
}

fn format_hex_color(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn platform_path_to_uri(file: &str, span: Span, raw: &str) -> Result<String, JsflDiagnostic> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "Invalid platform path: empty or contains control characters.",
        ));
    }
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with("file:///") {
        Ok(normalized)
    } else if normalized.starts_with('/') {
        Ok(format!("file://{normalized}"))
    } else {
        Ok(format!("file:///{normalized}"))
    }
}

fn uri_to_platform_path(file: &str, span: Span, uri: &str) -> Result<String, JsflDiagnostic> {
    if uri.is_empty() || uri.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "Invalid file URI: empty or contains control characters.",
        ));
    }
    let Some(rest) = uri.strip_prefix("file://") else {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Unsupported URI scheme in '{uri}': only 'file:///' URIs are permitted."),
        ));
    };
    if !rest.starts_with('/') {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Remote host file URI '{uri}' is forbidden."),
        ));
    }
    Ok(rest.to_owned())
}

fn resolve_scoped_flfile_path(
    caps: &FlFileCapabilities,
    file: &str,
    span: Span,
    uri: &str,
) -> Result<PathBuf, JsflDiagnostic> {
    let Some(root) = &caps.allowed_root else {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "FLfile access denied: no allowed_root configured in FlFileCapabilities.",
        ));
    };
    let raw_path = uri_to_platform_path(file, span, uri)?;
    let path = Path::new(&raw_path);
    for comp in path.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(JsflDiagnostic::new(
                file,
                span.line,
                span.column,
                format!("Path traversal ('..') is forbidden in FLfile URI '{uri}'."),
            ));
        }
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|e| {
        JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Failed to canonicalize FLfile allowed_root: {e}"),
        )
    })?;

    let joined = if path.is_absolute() {
        if path.starts_with(&canonical_root) || path.starts_with(root) {
            path.to_path_buf()
        } else {
            let rel = raw_path.trim_start_matches('/');
            canonical_root.join(rel)
        }
    } else {
        canonical_root.join(path)
    };

    // Walk up to the nearest existing ancestor to canonicalize and detect symlink escapes.
    let mut existing = joined.as_path();
    let mut tail_parts: Vec<&std::ffi::OsStr> = Vec::new();
    while !existing.exists() {
        if let Some(name) = existing.file_name() {
            tail_parts.push(name);
        }
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent;
    }
    let canonical_existing = std::fs::canonicalize(existing).map_err(|e| {
        JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Failed to resolve FLfile path '{uri}': {e}"),
        )
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("FLfile path '{uri}' escapes allowed_root via symlink or outside path."),
        ));
    }
    let mut resolved = canonical_existing;
    for part in tail_parts.into_iter().rev() {
        resolved.push(part);
    }
    if resolved.exists() {
        let final_canon = std::fs::canonicalize(&resolved).map_err(|e| {
            JsflDiagnostic::new(
                file,
                span.line,
                span.column,
                format!("Failed to canonicalize target '{uri}': {e}"),
            )
        })?;
        if !final_canon.starts_with(&canonical_root) {
            return Err(JsflDiagnostic::new(
                file,
                span.line,
                span.column,
                format!("FLfile target '{uri}' escapes allowed_root via symlink."),
            ));
        }
        return Ok(final_canon);
    }
    Ok(resolved)
}

#[derive(Clone, Debug)]
struct UserFunction {
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
enum Flow {
    None,
    Return(JsflValue),
    Break,
    Continue,
}

struct JsflVm<'a> {
    sandbox: &'a JsflSandbox,
    project: Project,
    state: JsflAuthoringState,
    current_file: String,
    last_span: Span,
    steps: usize,
    call_depth: usize,
    script_recursion: usize,
    heap_nodes: usize,
    mutations: usize,
    output_lines: Vec<String>,
    scopes: Vec<HashMap<String, JsflValue>>,
    functions: HashMap<String, UserFunction>,
}

impl<'a> JsflVm<'a> {
    fn new(
        sandbox: &'a JsflSandbox,
        project: Project,
        state: JsflAuthoringState,
        current_file: String,
    ) -> Self {
        let mut root_scope = HashMap::new();
        root_scope.insert("fl".to_owned(), JsflValue::FlGlobal);
        root_scope.insert("flash".to_owned(), JsflValue::FlGlobal);
        root_scope.insert("an".to_owned(), JsflValue::FlGlobal);
        root_scope.insert("FLfile".to_owned(), JsflValue::FlFile);
        Self {
            sandbox,
            project,
            state,
            current_file,
            last_span: Span { line: 1, column: 1 },
            steps: 0,
            call_depth: 0,
            script_recursion: 0,
            heap_nodes: 0,
            mutations: 0,
            output_lines: Vec::new(),
            scopes: vec![root_scope],
            functions: HashMap::new(),
        }
    }

    fn err(&self, span: Span, msg: impl Into<String>) -> JsflDiagnostic {
        JsflDiagnostic::new(&self.current_file, span.line, span.column, msg)
    }

    fn step(&mut self, span: Span) -> Result<(), JsflDiagnostic> {
        self.last_span = span;
        self.steps += 1;
        if self.steps > self.sandbox.limits.max_steps {
            return Err(self.err(
                span,
                format!(
                    "JSFL execution exceeded step limit ({}).",
                    self.sandbox.limits.max_steps
                ),
            ));
        }
        Ok(())
    }

    fn alloc_heap(&mut self, count: usize, span: Span) -> Result<(), JsflDiagnostic> {
        self.heap_nodes = self.heap_nodes.saturating_add(count);
        if self.heap_nodes > self.sandbox.limits.max_heap_nodes {
            return Err(self.err(
                span,
                format!(
                    "JSFL execution exceeded heap allocation limit ({}).",
                    self.sandbox.limits.max_heap_nodes
                ),
            ));
        }
        Ok(())
    }

    fn record_mutation(&mut self, span: Span) -> Result<(), JsflDiagnostic> {
        self.mutations += 1;
        if self.mutations > self.sandbox.limits.max_mutations {
            return Err(self.err(
                span,
                format!(
                    "JSFL execution exceeded mutation limit ({}).",
                    self.sandbox.limits.max_mutations
                ),
            ));
        }
        Ok(())
    }

    fn push_trace(&mut self, line: String, span: Span) -> Result<(), JsflDiagnostic> {
        if self.output_lines.len() >= self.sandbox.limits.max_output_lines {
            return Err(self.err(
                span,
                format!(
                    "JSFL output exceeded maximum trace lines ({}).",
                    self.sandbox.limits.max_output_lines
                ),
            ));
        }
        self.output_lines.push(line);
        Ok(())
    }

    fn get_var(&self, name: &str, span: Span) -> Result<JsflValue, JsflDiagnostic> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Ok(val.clone());
            }
        }
        match name {
            "XMLUI" | "SWFPanel" | "CEP" | "CSXS" | "MMExecute" => Err(self.err(
                span,
                format!(
                    "Unsupported JSFL host object '{name}': legacy panel/extension APIs are not available in the NF2 sandbox."
                ),
            )),
            _ => Err(self.err(span, format!("ReferenceError: '{name}' is not defined."))),
        }
    }

    fn define_var(&mut self, name: String, val: JsflValue) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, val);
        }
    }

    fn set_var(&mut self, name: &str, val: JsflValue) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_owned(), val);
                return;
            }
        }
        self.define_var(name.to_owned(), val);
    }

    fn run_program(&mut self, stmts: &[Stmt], span: Span) -> Result<JsflValue, JsflDiagnostic> {
        self.step(span)?;
        for stmt in stmts {
            if let StmtKind::FuncDecl { name, params, body } = &stmt.kind {
                self.functions.insert(
                    name.clone(),
                    UserFunction {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
            }
        }
        let mut last = JsflValue::Undefined;
        for stmt in stmts {
            if matches!(stmt.kind, StmtKind::FuncDecl { .. }) {
                continue;
            }
            if let StmtKind::Expr(expr) = &stmt.kind {
                last = self.eval_expr(expr)?;
                continue;
            }
            match self.exec_stmt(stmt)? {
                Flow::None => {}
                Flow::Return(v) => return Ok(v),
                Flow::Break => return Err(self.err(stmt.span, "Illegal 'break' statement.")),
                Flow::Continue => return Err(self.err(stmt.span, "Illegal 'continue' statement.")),
            }
        }
        Ok(last)
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Flow, JsflDiagnostic> {
        for stmt in stmts {
            let flow = self.exec_stmt(stmt)?;
            if !matches!(flow, Flow::None) {
                return Ok(flow);
            }
        }
        Ok(Flow::None)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Flow, JsflDiagnostic> {
        self.step(stmt.span)?;
        match &stmt.kind {
            StmtKind::Empty => Ok(Flow::None),
            StmtKind::Block(stmts) => self.exec_block(stmts),
            StmtKind::VarDecl(decls) => {
                for (name, init) in decls {
                    let val = match init {
                        Some(expr) => self.eval_expr(expr)?,
                        None => JsflValue::Undefined,
                    };
                    self.define_var(name.clone(), val);
                }
                Ok(Flow::None)
            }
            StmtKind::FuncDecl { name, params, body } => {
                self.functions.insert(
                    name.clone(),
                    UserFunction {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                Ok(Flow::None)
            }
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.eval_expr(cond)?;
                if c.is_truthy() {
                    self.exec_stmt(then_branch)
                } else if let Some(eb) = else_branch {
                    self.exec_stmt(eb)
                } else {
                    Ok(Flow::None)
                }
            }
            StmtKind::While { cond, body } => {
                loop {
                    self.step(stmt.span)?;
                    let c = self.eval_expr(cond)?;
                    if !c.is_truthy() {
                        break;
                    }
                    match self.exec_stmt(body)? {
                        Flow::None | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::None)
            }
            StmtKind::For {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    self.exec_stmt(init_stmt)?;
                }
                loop {
                    self.step(stmt.span)?;
                    if let Some(c_expr) = cond {
                        let c = self.eval_expr(c_expr)?;
                        if !c.is_truthy() {
                            break;
                        }
                    }
                    match self.exec_stmt(body)? {
                        Flow::None | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                    if let Some(u_expr) = update {
                        self.eval_expr(u_expr)?;
                    }
                }
                Ok(Flow::None)
            }
            StmtKind::ForOf {
                var_name,
                iterable,
                body,
            } => {
                let iter_val = self.eval_expr(iterable)?;
                let items: Vec<JsflValue> = match iter_val {
                    JsflValue::Array(arr) => arr,
                    JsflValue::Object(map) => map.keys().cloned().map(JsflValue::String).collect(),
                    _ => Vec::new(),
                };
                for item in items {
                    self.step(stmt.span)?;
                    self.set_var(var_name, item);
                    match self.exec_stmt(body)? {
                        Flow::None | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::None)
            }
            StmtKind::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => JsflValue::Undefined,
                };
                Ok(Flow::Return(val))
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
            StmtKind::Throw(expr) => {
                let val = self.eval_expr(expr)?;
                Err(self.err(
                    stmt.span,
                    format!("Uncaught JSFL exception: {}", val.to_display_string()),
                ))
            }
            StmtKind::TryCatch {
                try_block,
                catch_param,
                catch_block,
            } => match self.exec_block(try_block) {
                Ok(flow) => Ok(flow),
                Err(diag) => {
                    // Fuel/step/heap/mutation limits are non-catchable sandbox traps!
                    if diag.message.contains("exceeded step limit")
                        || diag.message.contains("exceeded heap allocation limit")
                        || diag.message.contains("exceeded mutation limit")
                        || diag.message.contains("exceeded maximum trace lines")
                        || diag.message.contains("recursion limit")
                    {
                        return Err(diag);
                    }
                    if let Some(param) = catch_param {
                        self.define_var(param.clone(), JsflValue::String(diag.message));
                    }
                    self.exec_block(catch_block)
                }
            },
            StmtKind::Expr(expr) => {
                self.eval_expr(expr)?;
                Ok(Flow::None)
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<JsflValue, JsflDiagnostic> {
        self.step(expr.span)?;
        match &expr.kind {
            ExprKind::Undefined => Ok(JsflValue::Undefined),
            ExprKind::Null => Ok(JsflValue::Null),
            ExprKind::Bool(b) => Ok(JsflValue::Bool(*b)),
            ExprKind::Number(n) => Ok(JsflValue::Number(*n)),
            ExprKind::Str(s) => Ok(JsflValue::String(s.clone())),
            ExprKind::Ident(name) => self.get_var(name, expr.span),
            ExprKind::Array(items) => {
                self.alloc_heap(items.len() + 1, expr.span)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval_expr(item)?);
                }
                Ok(JsflValue::Array(out))
            }
            ExprKind::Object(entries) => {
                self.alloc_heap(entries.len() + 1, expr.span)?;
                let mut map = BTreeMap::new();
                for (k, v) in entries {
                    map.insert(k.clone(), self.eval_expr(v)?);
                }
                Ok(JsflValue::Object(map))
            }
            ExprKind::Member { object, property } => {
                let obj = self.eval_expr(object)?;
                self.get_property(&obj, property, expr.span)
            }
            ExprKind::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                self.get_index(&obj, &idx, expr.span)
            }
            ExprKind::Call { callee, args } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args {
                    eval_args.push(self.eval_expr(a)?);
                }
                match &callee.kind {
                    ExprKind::Member { object, property } => {
                        let recv = self.eval_expr(object)?;
                        self.call_method(&recv, property, &eval_args, expr.span)
                    }
                    ExprKind::Ident(func_name) => {
                        self.call_global_or_user(func_name, &eval_args, expr.span)
                    }
                    _ => Err(self.err(expr.span, "Expression is not callable.")),
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                if matches!(op, UnaryOp::Typeof)
                    && let ExprKind::Ident(name) = &inner.kind
                {
                    let ty = match self.get_var(name, expr.span) {
                        Ok(v) => Self::typeof_str(&v),
                        Err(_) => "undefined",
                    };
                    return Ok(JsflValue::String(ty.to_owned()));
                }
                let val = self.eval_expr(inner)?;
                match op {
                    UnaryOp::Not => Ok(JsflValue::Bool(!val.is_truthy())),
                    UnaryOp::Neg => Ok(JsflValue::Number(-val.as_number().unwrap_or(f64::NAN))),
                    UnaryOp::Pos => Ok(JsflValue::Number(val.as_number().unwrap_or(f64::NAN))),
                    UnaryOp::Typeof => Ok(JsflValue::String(Self::typeof_str(&val).to_owned())),
                }
            }
            ExprKind::Update {
                target,
                increment,
                prefix,
            } => self.eval_update(target, *increment, *prefix, expr.span),
            ExprKind::Binary { op, left, right } => self.eval_binary(*op, left, right, expr.span),
            ExprKind::Assign { op, target, value } => {
                self.eval_assign(*op, target, value, expr.span)
            }
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                let c = self.eval_expr(cond)?;
                if c.is_truthy() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }
        }
    }

    fn typeof_str(val: &JsflValue) -> &'static str {
        match val {
            JsflValue::Undefined => "undefined",
            JsflValue::Bool(_) => "boolean",
            JsflValue::Number(_) => "number",
            JsflValue::String(_) => "string",
            _ => "object",
        }
    }

    fn eval_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        _span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        if op == BinaryOp::And {
            let l = self.eval_expr(left)?;
            if !l.is_truthy() {
                return Ok(l);
            }
            return self.eval_expr(right);
        }
        if op == BinaryOp::Or {
            let l = self.eval_expr(left)?;
            if l.is_truthy() {
                return Ok(l);
            }
            return self.eval_expr(right);
        }
        let l = self.eval_expr(left)?;
        let r = self.eval_expr(right)?;
        match op {
            BinaryOp::Add => {
                if matches!(l, JsflValue::String(_)) || matches!(r, JsflValue::String(_)) {
                    Ok(JsflValue::String(format!(
                        "{}{}",
                        l.to_display_string(),
                        r.to_display_string()
                    )))
                } else {
                    Ok(JsflValue::Number(
                        l.as_number().unwrap_or(f64::NAN) + r.as_number().unwrap_or(f64::NAN),
                    ))
                }
            }
            BinaryOp::Sub => Ok(JsflValue::Number(
                l.as_number().unwrap_or(f64::NAN) - r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Mul => Ok(JsflValue::Number(
                l.as_number().unwrap_or(f64::NAN) * r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Div => Ok(JsflValue::Number(
                l.as_number().unwrap_or(f64::NAN) / r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Rem => Ok(JsflValue::Number(
                l.as_number().unwrap_or(f64::NAN) % r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Eq | BinaryOp::StrictEq => Ok(JsflValue::Bool(Self::values_equal(&l, &r))),
            BinaryOp::Ne | BinaryOp::StrictNe => Ok(JsflValue::Bool(!Self::values_equal(&l, &r))),
            BinaryOp::Lt => Ok(JsflValue::Bool(
                l.as_number().unwrap_or(f64::NAN) < r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Le => Ok(JsflValue::Bool(
                l.as_number().unwrap_or(f64::NAN) <= r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Gt => Ok(JsflValue::Bool(
                l.as_number().unwrap_or(f64::NAN) > r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::Ge => Ok(JsflValue::Bool(
                l.as_number().unwrap_or(f64::NAN) >= r.as_number().unwrap_or(f64::NAN),
            )),
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }

    fn values_equal(a: &JsflValue, b: &JsflValue) -> bool {
        match (a, b) {
            (JsflValue::Number(x), JsflValue::Number(y)) => (x - y).abs() <= 1e-9,
            (JsflValue::Null, JsflValue::Undefined) | (JsflValue::Undefined, JsflValue::Null) => {
                true
            }
            _ => a == b,
        }
    }
}

impl<'a> JsflVm<'a> {
    fn eval_assign(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value_expr: &Expr,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let rhs = self.eval_expr(value_expr)?;
        match &target.kind {
            ExprKind::Ident(name) => {
                let final_val = if op == AssignOp::Assign {
                    rhs
                } else {
                    let cur = self.get_var(name, span)?;
                    self.apply_assign_op(op, &cur, &rhs)
                };
                self.set_var(name, final_val.clone());
                Ok(final_val)
            }
            ExprKind::Member { object, property } => {
                let recv = self.eval_expr(object)?;
                let final_val = if op == AssignOp::Assign {
                    rhs
                } else {
                    let cur = self.get_property(&recv, property, span)?;
                    self.apply_assign_op(op, &cur, &rhs)
                };
                if let ExprKind::Ident(var_name) = &object.kind
                    && let JsflValue::Object(mut map) = recv
                {
                    map.insert(property.clone(), final_val.clone());
                    self.set_var(var_name, JsflValue::Object(map));
                    return Ok(final_val);
                }
                self.set_property(&recv, property, final_val.clone(), span)?;
                Ok(final_val)
            }
            ExprKind::Index { object, index } => {
                let recv = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                let final_val = if op == AssignOp::Assign {
                    rhs
                } else {
                    let cur = self.get_index(&recv, &idx, span)?;
                    self.apply_assign_op(op, &cur, &rhs)
                };
                if let ExprKind::Ident(var_name) = &object.kind {
                    match (recv, &idx) {
                        (JsflValue::Array(mut arr), JsflValue::Number(n)) if *n >= 0.0 => {
                            let i = *n as usize;
                            if i >= arr.len() {
                                self.alloc_heap(i + 1 - arr.len(), span)?;
                                arr.resize(i + 1, JsflValue::Undefined);
                            }
                            arr[i] = final_val.clone();
                            self.set_var(var_name, JsflValue::Array(arr));
                            return Ok(final_val);
                        }
                        (JsflValue::Object(mut map), _) => {
                            map.insert(idx.to_display_string(), final_val.clone());
                            self.set_var(var_name, JsflValue::Object(map));
                            return Ok(final_val);
                        }
                        _ => {}
                    }
                } else if let ExprKind::Member {
                    object: inner_obj,
                    property,
                } = &object.kind
                    && let ExprKind::Ident(var_name) = &inner_obj.kind
                    && let Ok(JsflValue::Object(mut parent_map)) = self.get_var(var_name, span)
                    && let (JsflValue::Array(mut arr), JsflValue::Number(n)) = (recv, &idx)
                    && *n >= 0.0
                {
                    let i = *n as usize;
                    if i >= arr.len() {
                        self.alloc_heap(i + 1 - arr.len(), span)?;
                        arr.resize(i + 1, JsflValue::Undefined);
                    }
                    arr[i] = final_val.clone();
                    parent_map.insert(property.clone(), JsflValue::Array(arr));
                    self.set_var(var_name, JsflValue::Object(parent_map));
                    return Ok(final_val);
                }
                Err(self.err(span, "Invalid index assignment target."))
            }
            _ => Err(self.err(span, "Invalid assignment left-hand side.")),
        }
    }

    fn apply_assign_op(&self, op: AssignOp, cur: &JsflValue, rhs: &JsflValue) -> JsflValue {
        match op {
            AssignOp::Assign => rhs.clone(),
            AssignOp::AddAssign => {
                if matches!(cur, JsflValue::String(_)) || matches!(rhs, JsflValue::String(_)) {
                    JsflValue::String(format!(
                        "{}{}",
                        cur.to_display_string(),
                        rhs.to_display_string()
                    ))
                } else {
                    JsflValue::Number(
                        cur.as_number().unwrap_or(f64::NAN) + rhs.as_number().unwrap_or(f64::NAN),
                    )
                }
            }
            AssignOp::SubAssign => JsflValue::Number(
                cur.as_number().unwrap_or(f64::NAN) - rhs.as_number().unwrap_or(f64::NAN),
            ),
            AssignOp::MulAssign => JsflValue::Number(
                cur.as_number().unwrap_or(f64::NAN) * rhs.as_number().unwrap_or(f64::NAN),
            ),
            AssignOp::DivAssign => JsflValue::Number(
                cur.as_number().unwrap_or(f64::NAN) / rhs.as_number().unwrap_or(f64::NAN),
            ),
        }
    }

    fn eval_update(
        &mut self,
        target: &Expr,
        increment: bool,
        prefix: bool,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let delta = if increment { 1.0 } else { -1.0 };
        match &target.kind {
            ExprKind::Ident(name) => {
                let old = self.get_var(name, span)?.as_number().unwrap_or(f64::NAN);
                let new_val = old + delta;
                self.set_var(name, JsflValue::Number(new_val));
                Ok(JsflValue::Number(if prefix { new_val } else { old }))
            }
            ExprKind::Member { object, property } => {
                let recv = self.eval_expr(object)?;
                let old = self
                    .get_property(&recv, property, span)?
                    .as_number()
                    .unwrap_or(f64::NAN);
                let new_val = old + delta;
                self.set_property(&recv, property, JsflValue::Number(new_val), span)?;
                Ok(JsflValue::Number(if prefix { new_val } else { old }))
            }
            _ => Err(self.err(span, "Invalid update expression target.")),
        }
    }

    fn call_global_or_user(
        &mut self,
        func_name: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match func_name {
            "parseInt" => {
                let s = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let radix = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n as u32)
                    .unwrap_or(10)
                    .clamp(2, 36);
                let val = i64::from_str_radix(s.trim(), radix)
                    .map(|v| v as f64)
                    .unwrap_or(f64::NAN);
                return Ok(JsflValue::Number(val));
            }
            "parseFloat" | "Number" => {
                let val = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(f64::NAN);
                return Ok(JsflValue::Number(val));
            }
            "String" => {
                let s = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                return Ok(JsflValue::String(s));
            }
            "Boolean" => {
                let b = args.first().is_some_and(JsflValue::is_truthy);
                return Ok(JsflValue::Bool(b));
            }
            "Array" => {
                return Ok(JsflValue::Array(args.to_vec()));
            }
            "Object" => {
                return Ok(JsflValue::Object(BTreeMap::new()));
            }
            "MMExecute" => {
                return Err(self.err(
                    span,
                    "Unsupported JSFL function 'MMExecute': Flash panel ActionScript bridging is outside the NF2 sandbox.",
                ));
            }
            _ => {}
        }

        let Some(user_fn) = self.functions.get(func_name).cloned() else {
            return Err(self.err(span, format!("TypeError: '{func_name}' is not a function.")));
        };
        if self.call_depth >= self.sandbox.limits.max_call_depth {
            return Err(self.err(
                span,
                format!(
                    "JSFL execution exceeded call depth limit ({}).",
                    self.sandbox.limits.max_call_depth
                ),
            ));
        }
        self.call_depth += 1;
        let mut frame = HashMap::new();
        for (i, param) in user_fn.params.iter().enumerate() {
            frame.insert(
                param.clone(),
                args.get(i).cloned().unwrap_or(JsflValue::Undefined),
            );
        }
        self.scopes.push(frame);
        let result = match self.exec_block(&user_fn.body) {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(JsflValue::Undefined),
            Err(e) => Err(e),
        };
        self.scopes.pop();
        self.call_depth -= 1;
        result
    }

    // -----------------------------------------------------------------------
    // Handle Resolution & Stale-Handle Guards
    // -----------------------------------------------------------------------

    fn ensure_doc(&self, doc: DocHandle, span: Span) -> Result<(), JsflDiagnostic> {
        if doc.0 != 0 {
            return Err(self.err(
                span,
                format!("Stale or closed Document handle DocHandle({}).", doc.0),
            ));
        }
        Ok(())
    }

    fn symbol_ref(&self, sym_id: &str, span: Span) -> Result<&Symbol, JsflDiagnostic> {
        self.project
            .symbol(sym_id)
            .ok_or_else(|| self.err(span, format!("Stale symbol/timeline handle '{sym_id}'.")))
    }

    fn symbol_mut(&mut self, sym_id: &str, span: Span) -> Result<&mut Symbol, JsflDiagnostic> {
        let err = self.err(span, format!("Stale symbol/timeline handle '{sym_id}'."));
        self.project.symbol_mut(sym_id).ok_or(err)
    }

    fn resolve_layer(&self, h: &LayerHandle, span: Span) -> Result<&Layer, JsflDiagnostic> {
        self.ensure_doc(h.doc, span)?;
        let sym = self.symbol_ref(&h.symbol_id, span)?;
        sym.timeline
            .layers
            .iter()
            .find(|l| l.id == h.layer_id)
            .ok_or_else(|| {
                self.err(
                    span,
                    format!("Stale LayerHandle '{}': layer was deleted.", h.layer_id),
                )
            })
    }

    fn resolve_layer_mut(
        &mut self,
        h: &LayerHandle,
        span: Span,
    ) -> Result<&mut Layer, JsflDiagnostic> {
        self.ensure_doc(h.doc, span)?;
        let err = self.err(
            span,
            format!("Stale LayerHandle '{}': layer was deleted.", h.layer_id),
        );
        let sym = self.symbol_mut(&h.symbol_id, span)?;
        sym.timeline
            .layers
            .iter_mut()
            .find(|l| l.id == h.layer_id)
            .ok_or(err)
    }

    fn resolve_frame_keyframe(
        &self,
        h: &FrameHandle,
        span: Span,
    ) -> Result<(&Layer, &Keyframe, u32), JsflDiagnostic> {
        self.ensure_doc(h.doc, span)?;
        let sym = self.symbol_ref(&h.symbol_id, span)?;
        if h.frame >= sym.timeline.frame_count {
            return Err(self.err(
                span,
                format!(
                    "Stale FrameHandle '{}:{}': frame is outside timeline (frame_count={}).",
                    h.layer_id, h.frame, sym.timeline.frame_count
                ),
            ));
        }
        let layer = sym
            .timeline
            .layers
            .iter()
            .find(|l| l.id == h.layer_id)
            .ok_or_else(|| {
                self.err(
                    span,
                    format!("Stale FrameHandle: layer '{}' was deleted.", h.layer_id),
                )
            })?;
        let kf = layer.key_at(h.frame).ok_or_else(|| {
            self.err(
                span,
                format!(
                    "Stale FrameHandle '{}:{}': no keyframe span.",
                    h.layer_id, h.frame
                ),
            )
        })?;
        let next_start = layer
            .keyframes
            .iter()
            .filter(|k| k.frame > kf.frame)
            .map(|k| k.frame)
            .min()
            .unwrap_or(sym.timeline.frame_count);
        let duration = next_start.saturating_sub(kf.frame).max(1);
        Ok((layer, kf, duration))
    }

    fn resolve_element(&self, h: &ElementHandle, span: Span) -> Result<&Element, JsflDiagnostic> {
        self.ensure_doc(h.doc, span)?;
        let sym = self.symbol_ref(&h.symbol_id, span)?;
        let layer = sym
            .timeline
            .layers
            .iter()
            .find(|l| l.id == h.layer_id)
            .ok_or_else(|| {
                self.err(
                    span,
                    format!("Stale ElementHandle '{}': layer was deleted.", h.element_id),
                )
            })?;
        let kf = layer
            .keyframes
            .iter()
            .find(|k| k.frame == h.keyframe)
            .or_else(|| layer.key_at(h.keyframe))
            .ok_or_else(|| {
                self.err(
                    span,
                    format!(
                        "Stale ElementHandle '{}': keyframe {} was deleted.",
                        h.element_id, h.keyframe
                    ),
                )
            })?;
        kf.elements
            .iter()
            .find(|e| e.id == h.element_id)
            .ok_or_else(|| {
                self.err(
                    span,
                    format!(
                        "Stale ElementHandle '{}': element no longer exists.",
                        h.element_id
                    ),
                )
            })
    }

    fn resolve_element_mut(
        &mut self,
        h: &ElementHandle,
        span: Span,
    ) -> Result<&mut Element, JsflDiagnostic> {
        self.ensure_doc(h.doc, span)?;
        let err_layer = self.err(
            span,
            format!("Stale ElementHandle '{}': layer was deleted.", h.element_id),
        );
        let err_kf = self.err(
            span,
            format!(
                "Stale ElementHandle '{}': keyframe {} was deleted.",
                h.element_id, h.keyframe
            ),
        );
        let err_el = self.err(
            span,
            format!(
                "Stale ElementHandle '{}': element no longer exists.",
                h.element_id
            ),
        );
        let sym = self.symbol_mut(&h.symbol_id, span)?;
        let layer = sym
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == h.layer_id)
            .ok_or(err_layer)?;
        let kf_idx = layer
            .keyframes
            .iter()
            .position(|k| k.frame == h.keyframe)
            .or_else(|| {
                layer
                    .keyframes
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| k.frame <= h.keyframe)
                    .max_by_key(|(_, k)| k.frame)
                    .map(|(idx, _)| idx)
            })
            .ok_or(err_kf)?;
        layer.keyframes[kf_idx]
            .elements
            .iter_mut()
            .find(|e| e.id == h.element_id)
            .ok_or(err_el)
    }

    fn get_index(
        &mut self,
        obj: &JsflValue,
        idx: &JsflValue,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match obj {
            JsflValue::Array(items) => {
                let Some(n) = idx.as_number() else {
                    return Ok(JsflValue::Undefined);
                };
                if n < 0.0 {
                    return Ok(JsflValue::Undefined);
                }
                Ok(items
                    .get(n as usize)
                    .cloned()
                    .unwrap_or(JsflValue::Undefined))
            }
            JsflValue::Object(map) => {
                let k = idx.to_display_string();
                Ok(map.get(&k).cloned().unwrap_or(JsflValue::Undefined))
            }
            _ => {
                let prop = idx.to_display_string();
                self.get_property(obj, &prop, span)
            }
        }
    }
}

impl<'a> JsflVm<'a> {
    fn get_property(
        &mut self,
        obj: &JsflValue,
        prop: &str,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match obj {
            JsflValue::Array(items) => match prop {
                "length" => Ok(JsflValue::Number(items.len() as f64)),
                _ => Err(self.err(span, format!("Unsupported Array property '{prop}'."))),
            },
            JsflValue::String(s) => match prop {
                "length" => Ok(JsflValue::Number(s.chars().count() as f64)),
                _ => Err(self.err(span, format!("Unsupported String property '{prop}'."))),
            },
            JsflValue::Object(map) => Ok(map.get(prop).cloned().unwrap_or(JsflValue::Undefined)),
            JsflValue::FlGlobal => match prop {
                "version" => Ok(JsflValue::String(self.sandbox.version.clone())),
                "configURI" => Ok(JsflValue::String(self.sandbox.config_uri.clone())),
                "scriptURI" => Ok(JsflValue::String(self.sandbox.script_uri.clone())),
                "outputPanel" => Ok(JsflValue::OutputPanel),
                "documents" => Ok(JsflValue::Array(vec![JsflValue::Doc(DocHandle(0))])),
                "xmlui" | "swfPanels" | "componentsPanel" | "presetPanel" | "compilerErrors" => {
                    Err(self.err(
                        span,
                        format!(
                            "Unsupported JSFL property 'fl.{prop}': legacy UI/extension panels are not supported in the NF2 sandbox."
                        ),
                    ))
                }
                _ => Err(self.err(span, format!("Unsupported property 'fl.{prop}'."))),
            },
            JsflValue::Doc(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "width" => Ok(JsflValue::Number(self.project.width as f64)),
                    "height" => Ok(JsflValue::Number(self.project.height as f64)),
                    "frameRate" => Ok(JsflValue::Number(self.project.fps as f64)),
                    "backgroundColor" => {
                        Ok(JsflValue::String(format_hex_color(self.project.background)))
                    }
                    "name" => Ok(JsflValue::String(self.project.name.clone())),
                    "docClass" => Ok(JsflValue::String(
                        self.project.document_class.clone().unwrap_or_default(),
                    )),
                    "library" => Ok(JsflValue::Library(*doc)),
                    "timelines" => {
                        let timelines: Vec<JsflValue> = self
                            .project
                            .scene_ids()
                            .into_iter()
                            .map(|symbol_id| JsflValue::Timeline(TimelineHandle { doc: *doc, symbol_id }))
                            .collect();
                        Ok(JsflValue::Array(timelines))
                    }
                    "currentTimeline" => {
                        let idx = self
                            .project
                            .scene_ids()
                            .iter()
                            .position(|id| *id == self.state.active_symbol)
                            .unwrap_or(0);
                        Ok(JsflValue::Number(idx as f64))
                    }
                    "selection" => self.collect_doc_selection(*doc, span),
                    _ => Err(self.err(span, format!("Unsupported Document property '{prop}'."))),
                }
            }
            JsflValue::Library(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "items" => Ok(JsflValue::Array(self.collect_library_items(*doc))),
                    _ => Err(self.err(span, format!("Unsupported Library property '{prop}'."))),
                }
            }
            JsflValue::Timeline(tl) => {
                self.ensure_doc(tl.doc, span)?;
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(sym.name.clone())),
                    "currentFrame" => Ok(JsflValue::Number(self.state.current_frame as f64)),
                    "currentLayer" => Ok(JsflValue::Number(self.state.current_layer as f64)),
                    "frameCount" => Ok(JsflValue::Number(sym.timeline.frame_count as f64)),
                    "layerCount" => Ok(JsflValue::Number(sym.timeline.layers.len() as f64)),
                    "layers" => {
                        let layers: Vec<JsflValue> = sym
                            .timeline
                            .layers
                            .iter()
                            .map(|l| {
                                JsflValue::Layer(LayerHandle {
                                    doc: tl.doc,
                                    symbol_id: tl.symbol_id.clone(),
                                    layer_id: l.id.clone(),
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(layers))
                    }
                    _ => Err(self.err(span, format!("Unsupported Timeline property '{prop}'."))),
                }
            }
            JsflValue::Layer(lh) => {
                let sym = self.symbol_ref(&lh.symbol_id, span)?;
                let frame_count = sym.timeline.frame_count;
                let layer = self.resolve_layer(lh, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(layer.name.clone())),
                    "visible" => Ok(JsflValue::Bool(layer.visible)),
                    "locked" => Ok(JsflValue::Bool(layer.locked)),
                    "outline" => Ok(JsflValue::Bool(layer.properties.outline)),
                    "color" => {
                        let rgb = layer.properties.outline_color.unwrap_or([74, 144, 226]);
                        Ok(JsflValue::String(format!(
                            "#{:02x}{:02x}{:02x}",
                            rgb[0], rgb[1], rgb[2]
                        )))
                    }
                    "layerType" => {
                        let lt = if layer.properties.motion_guide.is_some() {
                            "guided"
                        } else if layer.properties.mask.is_some() {
                            "masked"
                        } else {
                            match layer.properties.role {
                                LayerRole::Normal => "normal",
                                LayerRole::Guide => "guide",
                                LayerRole::Mask => "mask",
                                LayerRole::Folder => "folder",
                            }
                        };
                        Ok(JsflValue::String(lt.to_owned()))
                    }
                    "frameCount" => Ok(JsflValue::Number(frame_count as f64)),
                    "frames" => {
                        let frames: Vec<JsflValue> = (0..frame_count)
                            .map(|f| {
                                JsflValue::Frame(FrameHandle {
                                    doc: lh.doc,
                                    symbol_id: lh.symbol_id.clone(),
                                    layer_id: lh.layer_id.clone(),
                                    frame: f,
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(frames))
                    }
                    _ => Err(self.err(span, format!("Unsupported Layer property '{prop}'."))),
                }
            }
            JsflValue::Frame(fh) => {
                let sym = self.symbol_ref(&fh.symbol_id, span)?;
                let script_src = sym
                    .timeline
                    .scripts
                    .iter()
                    .find(|s| s.frame == fh.frame)
                    .map(|s| s.script.source.clone())
                    .unwrap_or_default();
                let (_layer, kf, duration) = self.resolve_frame_keyframe(fh, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(kf.label.clone())),
                    "labelType" => Ok(JsflValue::String(
                        if kf.label.is_empty() {
                            "none"
                        } else if kf.label.starts_with("//") {
                            "comment"
                        } else {
                            "name"
                        }
                        .to_owned(),
                    )),
                    "actionScript" => Ok(JsflValue::String(script_src)),
                    "tweenType" => {
                        let tt = match kf.tween {
                            None => "none",
                            Some(_) => match kf.tween_kind {
                                TweenKind::Classic | TweenKind::Motion => "motion",
                                TweenKind::Shape => "shape",
                            },
                        };
                        Ok(JsflValue::String(tt.to_owned()))
                    }
                    "tweenEasing" => {
                        let ease_val = match kf.tween {
                            Some(Easing::Flash { acceleration }) => acceleration,
                            Some(Easing::EaseIn) => -100.0,
                            Some(Easing::EaseOut) => 100.0,
                            _ => 0.0,
                        };
                        Ok(JsflValue::Number(ease_val))
                    }
                    "startFrame" => Ok(JsflValue::Number(kf.frame as f64)),
                    "duration" => Ok(JsflValue::Number(duration as f64)),
                    "elements" => {
                        let kf_frame = kf.frame;
                        let els: Vec<JsflValue> = kf
                            .elements
                            .iter()
                            .map(|e| {
                                JsflValue::Element(ElementHandle {
                                    doc: fh.doc,
                                    symbol_id: fh.symbol_id.clone(),
                                    layer_id: fh.layer_id.clone(),
                                    keyframe: kf_frame,
                                    element_id: e.id.clone(),
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(els))
                    }
                    _ => Err(self.err(span, format!("Unsupported Frame property '{prop}'."))),
                }
            }
            JsflValue::Item(ih) => {
                self.ensure_doc(ih.doc, span)?;
                if ih.is_folder {
                    let folders = self.project.library_folder_paths();
                    if !folders.iter().any(|f| f == &ih.item_id) {
                        return Err(self.err(
                            span,
                            format!("Stale ItemHandle '{}': folder no longer exists.", ih.item_id),
                        ));
                    }
                    return match prop {
                        "name" => Ok(JsflValue::String(ih.item_id.clone())),
                        "itemType" => Ok(JsflValue::String("folder".to_owned())),
                        "timeline" => Ok(JsflValue::Undefined),
                        _ => Err(self.err(span, format!("Unsupported Folder Item property '{prop}'."))),
                    };
                }
                let sym = self.symbol_ref(&ih.item_id, span)?;
                match prop {
                    "name" => {
                        let full = if sym.folder.is_empty() {
                            sym.name.clone()
                        } else {
                            format!("{}/{}", sym.folder, sym.name)
                        };
                        Ok(JsflValue::String(full))
                    }
                    "itemType" => {
                        let kind = match sym.kind {
                            SymbolKind::MovieClip => "movie clip",
                            SymbolKind::Graphic => "graphic",
                            SymbolKind::Button => "button",
                        };
                        Ok(JsflValue::String(kind.to_owned()))
                    }
                    "timeline" => Ok(JsflValue::Timeline(TimelineHandle {
                        doc: ih.doc,
                        symbol_id: sym.id.clone(),
                    })),
                    "linkageIdentifier" => Ok(JsflValue::String(
                        sym.linkage_id.clone().unwrap_or_default(),
                    )),
                    "linkageClassName" => Ok(JsflValue::String(
                        sym.class_name.clone().unwrap_or_default(),
                    )),
                    "linkageExportForAS" => Ok(JsflValue::Bool(
                        sym.linkage_id.is_some() || sym.class_name.is_some(),
                    )),
                    _ => Err(self.err(span, format!("Unsupported Item property '{prop}'."))),
                }
            }
            JsflValue::Element(eh) => {
                let layer = self.resolve_layer(
                    &LayerHandle {
                        doc: eh.doc,
                        symbol_id: eh.symbol_id.clone(),
                        layer_id: eh.layer_id.clone(),
                    },
                    span,
                )?;
                let is_grouped = layer
                    .properties
                    .groups
                    .iter()
                    .any(|g| g.members.contains(&eh.element_id));
                let el = self.resolve_element(eh, span)?;
                match prop {
                    "elementType" => {
                        let et = match &el.kind {
                            ElementKind::Instance { .. } => "instance",
                            ElementKind::Text { .. } | ElementKind::TextBox { .. } => "text",
                            ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. } => {
                                if el.merge { "shape" } else { "shapeObj" }
                            }
                            ElementKind::Bitmap { .. } | ElementKind::Video { .. } => "instance",
                        };
                        Ok(JsflValue::String(et.to_owned()))
                    }
                    "name" => Ok(JsflValue::String(el.name.clone())),
                    "x" | "left" => Ok(JsflValue::Number(el.transform.x)),
                    "y" | "top" => Ok(JsflValue::Number(el.transform.y)),
                    "scaleX" => Ok(JsflValue::Number(el.transform.scale_x)),
                    "scaleY" => Ok(JsflValue::Number(el.transform.scale_y)),
                    "rotation" => Ok(JsflValue::Number(el.transform.rotation)),
                    "skewX" | "skewY" => Ok(JsflValue::Number(el.transform.skew)),
                    "visible" => Ok(JsflValue::Bool(el.visible)),
                    "blendMode" => Ok(JsflValue::String(el.blend_mode.as_str().to_owned())),
                    "isGroup" => Ok(JsflValue::Bool(is_grouped || !el.merge)),
                    "width" => Ok(JsflValue::Number(
                        Self::element_local_size(el).0 * el.transform.scale_x.abs(),
                    )),
                    "height" => Ok(JsflValue::Number(
                        Self::element_local_size(el).1 * el.transform.scale_y.abs(),
                    )),
                    "matrix" => {
                        let [a, b, c, d, tx, ty] = crate::evaluate::transform_matrix(el.transform);
                        let mut m = BTreeMap::new();
                        m.insert("a".to_owned(), JsflValue::Number(a));
                        m.insert("b".to_owned(), JsflValue::Number(b));
                        m.insert("c".to_owned(), JsflValue::Number(c));
                        m.insert("d".to_owned(), JsflValue::Number(d));
                        m.insert("tx".to_owned(), JsflValue::Number(tx));
                        m.insert("ty".to_owned(), JsflValue::Number(ty));
                        Ok(JsflValue::Object(m))
                    }
                    "libraryItem" => match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => Ok(JsflValue::Item(ItemHandle {
                            doc: eh.doc,
                            item_id: symbol_id.clone(),
                            is_folder: false,
                        })),
                        _ => Ok(JsflValue::Null),
                    },
                    "symbolType" => match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            let sym = self.symbol_ref(symbol_id, span)?;
                            let st = match sym.kind {
                                SymbolKind::MovieClip => "movie clip",
                                SymbolKind::Graphic => "graphic",
                                SymbolKind::Button => "button",
                            };
                            Ok(JsflValue::String(st.to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "firstFrame" => match &el.kind {
                        ElementKind::Instance { playback, .. } => {
                            let ff = match playback {
                                InstancePlayback::LoopFrom(f)
                                | InstancePlayback::PlayOnceFrom(f)
                                | InstancePlayback::SingleFrame(f) => *f,
                                _ => 0,
                            };
                            Ok(JsflValue::Number(ff as f64))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "loop" => match &el.kind {
                        ElementKind::Instance { playback, .. } => {
                            let lp = match playback {
                                InstancePlayback::Independent
                                | InstancePlayback::Loop
                                | InstancePlayback::LoopFrom(_) => "loop",
                                InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                                    "play once"
                                }
                                InstancePlayback::SingleFrame(_) => "single frame",
                            };
                            Ok(JsflValue::String(lp.to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "contours" | "edges" | "vertices" => {
                        let cmds = el.to_path_commands().map(|(c, _)| c).unwrap_or_default();
                        let count = cmds.len();
                        let mut arr = Vec::with_capacity(count);
                        for (idx, cmd) in cmds.into_iter().enumerate() {
                            let mut obj = BTreeMap::new();
                            obj.insert("index".to_owned(), JsflValue::Number(idx as f64));
                            match cmd {
                                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                                    obj.insert("x".to_owned(), JsflValue::Number(x));
                                    obj.insert("y".to_owned(), JsflValue::Number(y));
                                }
                                PathCommand::QuadraticTo { x, y, .. }
                                | PathCommand::CubicTo { x, y, .. } => {
                                    obj.insert("x".to_owned(), JsflValue::Number(x));
                                    obj.insert("y".to_owned(), JsflValue::Number(y));
                                }
                                PathCommand::Close => {}
                            }
                            arr.push(JsflValue::Object(obj));
                        }
                        Ok(JsflValue::Array(arr))
                    }
                    "fillColor" | "color" => match &el.kind {
                        ElementKind::Rectangle { fill, .. }
                        | ElementKind::Ellipse { fill, .. }
                        | ElementKind::Path { fill, .. }
                        | ElementKind::PaintedPath { fill, .. } => {
                            if fill[3] == 0 {
                                Ok(JsflValue::String(String::new()))
                            } else {
                                Ok(JsflValue::String(format_hex_color(*fill)))
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            Ok(JsflValue::String(format_hex_color(*color)))
                        }
                        ElementKind::TextBox { format, .. } => {
                            let c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            Ok(JsflValue::String(format_hex_color(c)))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "strokeColor" => match &el.kind {
                        ElementKind::Rectangle {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::PaintedPath {
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            if *stroke_width <= 0.0 || stroke[3] == 0 {
                                Ok(JsflValue::String(String::new()))
                            } else {
                                Ok(JsflValue::String(format_hex_color(*stroke)))
                            }
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    _ => Err(self.err(span, format!("Unsupported Element property '{prop}'."))),
                }
            }
            _ => Err(self.err(
                span,
                format!("Cannot read property '{prop}' of {}.", obj.to_display_string()),
            )),
        }
    }

    fn element_local_size(el: &Element) -> (f64, f64) {
        match &el.kind {
            ElementKind::Rectangle { width, height, .. }
            | ElementKind::Ellipse { width, height, .. }
            | ElementKind::Video { width, height }
            | ElementKind::TextBox { width, height, .. } => (*width, *height),
            ElementKind::Bitmap { width, height, .. } => (*width as f64, *height as f64),
            ElementKind::Text {
                text, font_size, ..
            } => (
                (text.chars().count().max(1) as f64) * font_size * 0.6,
                *font_size,
            ),
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                if let Ok(Some(b)) = crate::geometry::path_bounds(commands) {
                    ((b.x_max - b.x_min).max(1.0), (b.y_max - b.y_min).max(1.0))
                } else {
                    (50.0, 50.0)
                }
            }
            ElementKind::Instance { .. } => (100.0, 100.0),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn set_property(
        &mut self,
        obj: &JsflValue,
        prop: &str,
        val: JsflValue,
        span: Span,
    ) -> Result<(), JsflDiagnostic> {
        self.record_mutation(span)?;
        match obj {
            JsflValue::Doc(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "width" => {
                        let w = val
                            .as_number()
                            .ok_or_else(|| self.err(span, "Document.width must be a number."))?;
                        if !w.is_finite() || w < 1.0 || w > 8192.0 {
                            return Err(self
                                .err(span, format!("Document.width ({w}) must be in 1..=8192.")));
                        }
                        self.project.width = w.round() as u32;
                        Ok(())
                    }
                    "height" => {
                        let h = val
                            .as_number()
                            .ok_or_else(|| self.err(span, "Document.height must be a number."))?;
                        if !h.is_finite() || h < 1.0 || h > 8192.0 {
                            return Err(self
                                .err(span, format!("Document.height ({h}) must be in 1..=8192.")));
                        }
                        self.project.height = h.round() as u32;
                        Ok(())
                    }
                    "frameRate" => {
                        let fps = val.as_number().ok_or_else(|| {
                            self.err(span, "Document.frameRate must be a number.")
                        })?;
                        if !fps.is_finite() || fps < 1.0 || fps > 240.0 {
                            return Err(self.err(
                                span,
                                format!("Document.frameRate ({fps}) must be in 1..=240."),
                            ));
                        }
                        self.project.fps = fps.round() as u32;
                        Ok(())
                    }
                    "backgroundColor" => {
                        let color = parse_color_value(&val).ok_or_else(|| {
                            self.err(
                                span,
                                "Document.backgroundColor must be a '#RRGGBB' string or 0xRRGGBB integer.",
                            )
                        })?;
                        self.project.background = color;
                        Ok(())
                    }
                    "name" => {
                        self.project.name = val.to_display_string();
                        Ok(())
                    }
                    "docClass" => {
                        let s = val.to_display_string();
                        self.project.document_class = if s.trim().is_empty() {
                            None
                        } else {
                            self.project.enable_program();
                            Some(s.trim().to_owned())
                        };
                        Ok(())
                    }
                    "currentTimeline" => {
                        let idx = val.as_number().unwrap_or(0.0) as usize;
                        let scenes = self.project.scene_ids();
                        let Some(target) = scenes.get(idx) else {
                            return Err(self.err(
                                span,
                                format!("Invalid scene index {idx} for Document.currentTimeline."),
                            ));
                        };
                        self.state.active_symbol = target.clone();
                        self.state.current_layer = 0;
                        self.state.current_frame = 0;
                        self.state.selected_element_ids.clear();
                        Ok(())
                    }
                    "selection" => {
                        let JsflValue::Array(items) = val else {
                            return Err(self.err(
                                span,
                                "Document.selection must be assigned an Array of Element handles.",
                            ));
                        };
                        let mut ids = Vec::with_capacity(items.len());
                        for item in items {
                            let JsflValue::Element(eh) = item else {
                                return Err(self.err(
                                    span,
                                    "Document.selection array must contain Element handles.",
                                ));
                            };
                            let _ = self.resolve_element(&eh, span)?;
                            if !ids.contains(&eh.element_id) {
                                ids.push(eh.element_id);
                            }
                        }
                        self.state.selected_element_ids = ids;
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Cannot assign to Document property '{prop}'."),
                    )),
                }
            }
            JsflValue::Timeline(tl) => {
                self.ensure_doc(tl.doc, span)?;
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                let frame_count = sym.timeline.frame_count;
                let layer_count = sym.timeline.layers.len();
                match prop {
                    "name" => {
                        let new_name = val.to_display_string();
                        let sym = self.symbol_mut(&tl.symbol_id, span)?;
                        sym.name = new_name;
                        Ok(())
                    }
                    "currentFrame" => {
                        let f = val.as_number().unwrap_or(0.0).max(0.0) as u32;
                        if f >= frame_count {
                            return Err(self.err(
                                span,
                                format!(
                                    "Timeline.currentFrame ({f}) out of range 0..{frame_count}."
                                ),
                            ));
                        }
                        self.state.current_frame = f;
                        Ok(())
                    }
                    "currentLayer" => {
                        let l = val.as_number().unwrap_or(0.0).max(0.0) as usize;
                        if l >= layer_count {
                            return Err(self.err(
                                span,
                                format!(
                                    "Timeline.currentLayer ({l}) out of range 0..{layer_count}."
                                ),
                            ));
                        }
                        self.state.current_layer = l;
                        self.state.selected_layers = vec![l];
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Cannot assign to Timeline property '{prop}'."),
                    )),
                }
            }
            JsflValue::Layer(lh) => match prop {
                "name" => {
                    let s = val.to_display_string();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.name = s;
                    Ok(())
                }
                "visible" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.visible = b;
                    Ok(())
                }
                "locked" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.locked = b;
                    Ok(())
                }
                "outline" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.properties.outline = b;
                    Ok(())
                }
                "color" => {
                    let c = parse_color_value(&val).ok_or_else(|| {
                        self.err(span, "Layer.color must be '#RRGGBB' or 0xRRGGBB.")
                    })?;
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.properties.outline_color = Some([c[0], c[1], c[2]]);
                    Ok(())
                }
                "layerType" => {
                    let lt = val.to_display_string().to_lowercase();
                    let stale_err = self.err(span, format!("Stale LayerHandle '{}'.", lh.layer_id));
                    let bad_lt_err = self.err(span, format!("Unsupported Layer.layerType '{lt}'."));
                    let sym = self.symbol_mut(&lh.symbol_id, span)?;
                    let idx = sym
                        .timeline
                        .layers
                        .iter()
                        .position(|l| l.id == lh.layer_id)
                        .ok_or(stale_err)?;
                    match lt.as_str() {
                        "normal" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.motion_guide = None;
                            sym.timeline.layers[idx].properties.mask = None;
                        }
                        "guide" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Guide;
                            sym.timeline.layers[idx].properties.motion_guide = None;
                        }
                        "guided" => {
                            let guide_id = sym.timeline.layers[..idx]
                                .iter()
                                .rev()
                                .find(|l| l.properties.role == LayerRole::Guide)
                                .or_else(|| {
                                    sym.timeline
                                        .layers
                                        .iter()
                                        .find(|l| l.properties.role == LayerRole::Guide)
                                })
                                .map(|l| l.id.clone());
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.motion_guide = guide_id;
                        }
                        "mask" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Mask;
                            sym.timeline.layers[idx].properties.mask = None;
                        }
                        "masked" => {
                            let mask_id = sym.timeline.layers[..idx]
                                .iter()
                                .rev()
                                .find(|l| l.properties.role == LayerRole::Mask)
                                .or_else(|| {
                                    sym.timeline
                                        .layers
                                        .iter()
                                        .find(|l| l.properties.role == LayerRole::Mask)
                                })
                                .map(|l| l.id.clone());
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.mask = mask_id;
                        }
                        "folder" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Folder;
                        }
                        _ => {
                            return Err(bad_lt_err);
                        }
                    }
                    Ok(())
                }
                _ => Err(self.err(span, format!("Cannot assign to Layer property '{prop}'."))),
            },
            JsflValue::Frame(fh) => {
                let (_, kf_ref, _) = self.resolve_frame_keyframe(fh, span)?;
                let kf_start = kf_ref.frame;
                match prop {
                    "name" => {
                        let label = val.to_display_string();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            kf.label = label;
                        }
                        Ok(())
                    }
                    "labelType" => {
                        let lt = val.to_display_string().to_lowercase();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            if lt == "none" {
                                kf.label.clear();
                            } else if lt == "comment" && !kf.label.starts_with("//") {
                                kf.label = format!("//{}", kf.label);
                            }
                        }
                        Ok(())
                    }
                    "actionScript" => {
                        let code = val.to_display_string();
                        let sym = self.symbol_mut(&fh.symbol_id, span)?;
                        if code.trim().is_empty() {
                            sym.timeline.scripts.retain(|s| s.frame != kf_start);
                        } else if let Some(slot) = sym
                            .timeline
                            .scripts
                            .iter_mut()
                            .find(|s| s.frame == kf_start)
                        {
                            slot.script.source = code;
                        } else {
                            sym.timeline.scripts.push(FrameScript {
                                frame: kf_start,
                                script: Script {
                                    language: ScriptLanguage::ActionScript2,
                                    source: code,
                                },
                            });
                            sym.timeline.scripts.sort_by_key(|s| s.frame);
                        }
                        Ok(())
                    }
                    "tweenType" => {
                        let tt = val.to_display_string().to_lowercase();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            match tt.as_str() {
                                "none" => {
                                    kf.tween = None;
                                    kf.tween_kind = TweenKind::Classic;
                                    kf.keyed_properties = None;
                                }
                                "motion" | "classic" => {
                                    kf.tween = Some(kf.tween.unwrap_or(Easing::Linear));
                                    kf.tween_kind = if tt == "motion" {
                                        TweenKind::Motion
                                    } else {
                                        TweenKind::Classic
                                    };
                                }
                                "shape" => {
                                    kf.tween = Some(kf.tween.unwrap_or(Easing::Linear));
                                    kf.tween_kind = TweenKind::Shape;
                                    kf.keyed_properties = None;
                                }
                                _ => {
                                    return Err(self.err(
                                        span,
                                        format!("Unsupported Frame.tweenType '{tt}'."),
                                    ));
                                }
                            }
                        }
                        layer.normalize_motion_keys();
                        Ok(())
                    }
                    "tweenEasing" => {
                        let acc = val.as_number().unwrap_or(0.0).clamp(-100.0, 100.0);
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            kf.tween = Some(if acc.abs() <= 1e-6 {
                                Easing::Linear
                            } else {
                                Easing::Flash { acceleration: acc }
                            });
                        }
                        Ok(())
                    }
                    _ => Err(self.err(span, format!("Cannot assign to Frame property '{prop}'."))),
                }
            }
            JsflValue::Item(ih) => {
                self.ensure_doc(ih.doc, span)?;
                if ih.is_folder {
                    if prop == "name" {
                        let new_folder = val.to_display_string();
                        self.project
                            .relocate_library_folder(&ih.item_id, &new_folder)
                            .map_err(|e| self.err(span, e))?;
                        return Ok(());
                    }
                    return Err(self.err(
                        span,
                        format!("Cannot assign to Folder Item property '{prop}'."),
                    ));
                }
                match prop {
                    "name" => {
                        let raw = val.to_display_string();
                        let leaf = raw.rsplit('/').next().unwrap_or(&raw).trim().to_owned();
                        if leaf.is_empty() {
                            return Err(self.err(span, "Item.name cannot be empty."));
                        }
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.name = leaf;
                        Ok(())
                    }
                    "linkageIdentifier" => {
                        let s = val.to_display_string();
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.linkage_id = if s.trim().is_empty() {
                            None
                        } else {
                            Some(s.trim().to_owned())
                        };
                        self.project.enable_program();
                        Ok(())
                    }
                    "linkageClassName" => {
                        let s = val.to_display_string();
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.class_name = if s.trim().is_empty() {
                            None
                        } else {
                            Some(s.trim().to_owned())
                        };
                        self.project.enable_program();
                        Ok(())
                    }
                    "linkageExportForAS" => Ok(()),
                    _ => Err(self.err(span, format!("Cannot assign to Item property '{prop}'."))),
                }
            }
            JsflValue::Element(eh) => self.set_element_property(eh, prop, val, span),
            _ => Err(self.err(
                span,
                format!(
                    "Cannot set property '{prop}' on {}.",
                    obj.to_display_string()
                ),
            )),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn set_element_property(
        &mut self,
        eh: &ElementHandle,
        prop: &str,
        val: JsflValue,
        span: Span,
    ) -> Result<(), JsflDiagnostic> {
        match prop {
            "name" => {
                let s = val.to_display_string();
                let el = self.resolve_element_mut(eh, span)?;
                el.name = s;
                Ok(())
            }
            "x" | "left" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.x must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.x = n;
                Ok(())
            }
            "y" | "top" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.y must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.y = n;
                Ok(())
            }
            "scaleX" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.scaleX must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.scale_x = n;
                Ok(())
            }
            "scaleY" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.scaleY must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.scale_y = n;
                Ok(())
            }
            "rotation" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.rotation must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.rotation = n;
                Ok(())
            }
            "skewX" | "skewY" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.skew must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.skew = n;
                Ok(())
            }
            "width" => {
                let w = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.width must be a number."))?;
                if !w.is_finite() || w <= 0.0 {
                    return Err(self.err(span, "Element.width must be positive."));
                }
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { width, .. }
                    | ElementKind::Ellipse { width, .. }
                    | ElementKind::Video { width, .. }
                    | ElementKind::TextBox { width, .. } => {
                        *width = w / el.transform.scale_x.abs().max(1e-6);
                    }
                    _ => {
                        let base_w = Self::element_local_size(el).0.max(1e-6);
                        el.transform.scale_x = w / base_w;
                    }
                }
                Ok(())
            }
            "height" => {
                let h = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.height must be a number."))?;
                if !h.is_finite() || h <= 0.0 {
                    return Err(self.err(span, "Element.height must be positive."));
                }
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { height, .. }
                    | ElementKind::Ellipse { height, .. }
                    | ElementKind::Video { height, .. }
                    | ElementKind::TextBox { height, .. } => {
                        *height = h / el.transform.scale_y.abs().max(1e-6);
                    }
                    _ => {
                        let base_h = Self::element_local_size(el).1.max(1e-6);
                        el.transform.scale_y = h / base_h;
                    }
                }
                Ok(())
            }
            "visible" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                el.visible = b;
                Ok(())
            }
            "blendMode" => {
                let s = val.to_display_string().to_lowercase();
                let bm = BlendMode::from_name(&s).ok_or_else(|| {
                    self.err(span, format!("Unsupported Element.blendMode '{s}'."))
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                el.blend_mode = bm;
                Ok(())
            }
            "matrix" => {
                let JsflValue::Object(map) = val else {
                    return Err(self.err(
                        span,
                        "Element.matrix must be an Object {a, b, c, d, tx, ty}.",
                    ));
                };
                let get_f = |k: &str, def: f64| -> f64 {
                    map.get(k).and_then(JsflValue::as_number).unwrap_or(def)
                };
                let mat = [
                    get_f("a", 1.0),
                    get_f("b", 0.0),
                    get_f("c", 0.0),
                    get_f("d", 1.0),
                    get_f("tx", 0.0),
                    get_f("ty", 0.0),
                ];
                let new_t = Transform::from_matrix(mat).ok_or_else(|| {
                    self.err(
                        span,
                        "Element.matrix values are non-finite or out of bounds.",
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.x = new_t.x;
                el.transform.y = new_t.y;
                el.transform.scale_x = new_t.scale_x;
                el.transform.scale_y = new_t.scale_y;
                el.transform.rotation = new_t.rotation;
                el.transform.skew = new_t.skew;
                Ok(())
            }
            "libraryItem" => {
                let target_sym_id = match val {
                    JsflValue::Item(ih) if !ih.is_folder => ih.item_id,
                    JsflValue::String(path) => {
                        self.find_library_symbol_by_path(&path).ok_or_else(|| {
                            self.err(span, format!("Library symbol '{path}' not found."))
                        })?
                    }
                    _ => {
                        return Err(self.err(
                            span,
                            "Element.libraryItem must be a symbol ItemHandle or symbol path.",
                        ));
                    }
                };
                let _ = self.symbol_ref(&target_sym_id, span)?;
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { symbol_id, .. } = &mut el.kind {
                    *symbol_id = target_sym_id;
                    Ok(())
                } else {
                    Err(self.err(
                        span,
                        "Element.libraryItem can only be set on a SymbolInstance.",
                    ))
                }
            }
            "firstFrame" => {
                let ff = val.as_number().unwrap_or(0.0).max(0.0) as u32;
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { playback, .. } = &mut el.kind {
                    *playback = match *playback {
                        InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(ff),
                        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                            if ff == 0 {
                                InstancePlayback::PlayOnce
                            } else {
                                InstancePlayback::PlayOnceFrom(ff)
                            }
                        }
                        _ => {
                            if ff == 0 {
                                InstancePlayback::Loop
                            } else {
                                InstancePlayback::LoopFrom(ff)
                            }
                        }
                    };
                    Ok(())
                } else {
                    Err(self.err(
                        span,
                        "Element.firstFrame can only be set on a SymbolInstance.",
                    ))
                }
            }
            "loop" => {
                let lp = val.to_display_string().to_lowercase();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { playback, .. } = &mut el.kind {
                    let cur_ff = match *playback {
                        InstancePlayback::LoopFrom(f)
                        | InstancePlayback::PlayOnceFrom(f)
                        | InstancePlayback::SingleFrame(f) => f,
                        _ => 0,
                    };
                    *playback = match lp.as_str() {
                        "loop" => {
                            if cur_ff == 0 {
                                InstancePlayback::Loop
                            } else {
                                InstancePlayback::LoopFrom(cur_ff)
                            }
                        }
                        "play once" => {
                            if cur_ff == 0 {
                                InstancePlayback::PlayOnce
                            } else {
                                InstancePlayback::PlayOnceFrom(cur_ff)
                            }
                        }
                        "single frame" => InstancePlayback::SingleFrame(cur_ff),
                        _ => {
                            return Err(self.err(
                                span,
                                format!("Unsupported SymbolInstance.loop mode '{lp}'."),
                            ));
                        }
                    };
                    Ok(())
                } else {
                    Err(self.err(span, "Element.loop can only be set on a SymbolInstance."))
                }
            }
            "fillColor" | "color" => {
                let new_color = parse_color_value(&val).ok_or_else(|| {
                    self.err(
                        span,
                        format!("Element.{prop} must be a '#RRGGBB' string or 0xRRGGBB integer."),
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. } => {
                        *fill = new_color;
                        Ok(())
                    }
                    ElementKind::PaintedPath {
                        fill,
                        fill_gradient,
                        ..
                    } => {
                        *fill = new_color;
                        *fill_gradient = None;
                        Ok(())
                    }
                    ElementKind::Text { color, .. } => {
                        *color = new_color;
                        Ok(())
                    }
                    ElementKind::TextBox { format, .. } => {
                        format.color = (u32::from(new_color[0]) << 16)
                            | (u32::from(new_color[1]) << 8)
                            | u32::from(new_color[2]);
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Element.{prop} can only be set on a Shape or Text element."),
                    )),
                }
            }
            "strokeColor" => {
                let new_color = parse_color_value(&val).ok_or_else(|| {
                    self.err(
                        span,
                        "Element.strokeColor must be a '#RRGGBB' string or 0xRRGGBB integer.",
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { stroke, .. }
                    | ElementKind::Ellipse { stroke, .. }
                    | ElementKind::Path { stroke, .. } => {
                        *stroke = new_color;
                        Ok(())
                    }
                    ElementKind::PaintedPath {
                        stroke,
                        stroke_gradient,
                        ..
                    } => {
                        *stroke = new_color;
                        *stroke_gradient = None;
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        "Element.strokeColor can only be set on a Shape element.",
                    )),
                }
            }
            _ => Err(self.err(span, format!("Cannot assign to Element property '{prop}'."))),
        }
    }

    fn collect_doc_selection(
        &self,
        doc: DocHandle,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let sym = self.symbol_ref(&self.state.active_symbol, span)?;
        let selected_set: HashSet<&str> = self
            .state
            .selected_element_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut out = Vec::new();
        for layer in &sym.timeline.layers {
            if let Some(kf) = layer.key_at(self.state.current_frame) {
                for el in &kf.elements {
                    if selected_set.contains(el.id.as_str()) {
                        out.push(JsflValue::Element(ElementHandle {
                            doc,
                            symbol_id: sym.id.clone(),
                            layer_id: layer.id.clone(),
                            keyframe: kf.frame,
                            element_id: el.id.clone(),
                        }));
                    }
                }
            }
        }
        Ok(JsflValue::Array(out))
    }

    fn collect_library_items(&self, doc: DocHandle) -> Vec<JsflValue> {
        let mut items = Vec::new();
        for folder in self.project.library_folder_paths() {
            items.push(JsflValue::Item(ItemHandle {
                doc,
                item_id: folder,
                is_folder: true,
            }));
        }
        let scene_ids: HashSet<Id> = self.project.scene_ids().into_iter().collect();
        for sym in &self.project.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            items.push(JsflValue::Item(ItemHandle {
                doc,
                item_id: sym.id.clone(),
                is_folder: false,
            }));
        }
        items
    }

    fn find_library_symbol_by_path(&self, name_path: &str) -> Option<Id> {
        let trimmed = name_path.trim().trim_matches('/');
        let scene_ids: HashSet<Id> = self.project.scene_ids().into_iter().collect();
        for sym in &self.project.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            let full = if sym.folder.is_empty() {
                sym.name.clone()
            } else {
                format!("{}/{}", sym.folder, sym.name)
            };
            if full == trimmed || sym.name == trimmed || sym.id == trimmed {
                return Some(sym.id.clone());
            }
        }
        None
    }

    fn call_method(
        &mut self,
        recv: &JsflValue,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.step(span)?;
        match recv {
            JsflValue::FlGlobal => self.call_fl_method(method, args, span),
            JsflValue::OutputPanel => match method {
                "trace" => {
                    let msg = args
                        .iter()
                        .map(JsflValue::to_display_string)
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.push_trace(msg, span)?;
                    Ok(JsflValue::Undefined)
                }
                "clear" => {
                    self.output_lines.clear();
                    Ok(JsflValue::Undefined)
                }
                _ => Err(self.err(
                    span,
                    format!("Unsupported outputPanel method 'fl.outputPanel.{method}'."),
                )),
            },
            JsflValue::FlFile => self.call_flfile_method(method, args, span),
            JsflValue::Doc(doc) => self.call_doc_method(*doc, method, args, span),
            JsflValue::Timeline(tl) => self.call_timeline_method(tl.clone(), method, args, span),
            JsflValue::Library(doc) => self.call_library_method(*doc, method, args, span),
            JsflValue::Element(eh) => self.call_element_method(eh.clone(), method, args, span),
            JsflValue::String(s) => match method {
                "toLowerCase" => Ok(JsflValue::String(s.to_lowercase())),
                "toUpperCase" => Ok(JsflValue::String(s.to_uppercase())),
                "trim" => Ok(JsflValue::String(s.trim().to_owned())),
                "indexOf" => {
                    let needle = args
                        .first()
                        .map(JsflValue::to_display_string)
                        .unwrap_or_default();
                    let idx = s.find(&needle).map(|i| i as i64).unwrap_or(-1);
                    Ok(JsflValue::Number(idx as f64))
                }
                "replace" => {
                    let from = args
                        .first()
                        .map(JsflValue::to_display_string)
                        .unwrap_or_default();
                    let to = args
                        .get(1)
                        .map(JsflValue::to_display_string)
                        .unwrap_or_default();
                    Ok(JsflValue::String(s.replacen(&from, &to, 1)))
                }
                _ => Err(self.err(span, format!("Unsupported String method '{method}'."))),
            },
            JsflValue::Array(items) => match method {
                "join" => {
                    let sep = args
                        .first()
                        .map(JsflValue::to_display_string)
                        .unwrap_or_else(|| ",".to_owned());
                    let joined = items
                        .iter()
                        .map(JsflValue::to_display_string)
                        .collect::<Vec<_>>()
                        .join(&sep);
                    Ok(JsflValue::String(joined))
                }
                "indexOf" => {
                    let target = args.first().cloned().unwrap_or(JsflValue::Undefined);
                    let pos = items
                        .iter()
                        .position(|item| Self::values_equal(item, &target))
                        .map(|i| i as i64)
                        .unwrap_or(-1);
                    Ok(JsflValue::Number(pos as f64))
                }
                _ => Err(self.err(span, format!("Unsupported Array method '{method}'."))),
            },
            _ => Err(self.err(
                span,
                format!(
                    "Unsupported method '{method}' on {}.",
                    recv.to_display_string()
                ),
            )),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn call_fl_method(
        &mut self,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match method {
            "getDocumentDOM" => Ok(JsflValue::Doc(DocHandle(0))),
            "trace" => {
                let msg = args
                    .iter()
                    .map(JsflValue::to_display_string)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.push_trace(msg, span)?;
                Ok(JsflValue::Undefined)
            }
            "runScript" => {
                let uri = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                if uri.trim().is_empty() {
                    return Err(self.err(span, "fl.runScript requires a non-empty fileURI."));
                }
                if self.script_recursion >= self.sandbox.limits.max_script_recursion {
                    return Err(self.err(
                        span,
                        format!(
                            "JSFL nested fl.runScript exceeded script recursion limit ({}).",
                            self.sandbox.limits.max_script_recursion
                        ),
                    ));
                }
                let script_source = if let Some(src) = self.sandbox.virtual_scripts.get(&uri) {
                    src.clone()
                } else {
                    if !self.sandbox.flfile_caps.allow_read {
                        return Err(self.err(
                            span,
                            format!(
                                "fl.runScript('{uri}') denied: read capability is not enabled."
                            ),
                        ));
                    }
                    let path = resolve_scoped_flfile_path(
                        &self.sandbox.flfile_caps,
                        &self.current_file,
                        span,
                        &uri,
                    )?;
                    std::fs::read_to_string(&path).map_err(|e| {
                        self.err(span, format!("fl.runScript failed to read '{uri}': {e}"))
                    })?
                };
                if script_source.len() > self.sandbox.limits.max_source_bytes {
                    return Err(self.err(
                        span,
                        format!(
                            "fl.runScript('{uri}') exceeds maximum source size ({} bytes).",
                            self.sandbox.limits.max_source_bytes
                        ),
                    ));
                }
                let prev_file = self.current_file.clone();
                self.current_file = uri.clone();
                self.script_recursion += 1;
                let nested_tokens = tokenize(&uri, &script_source)?;
                let mut nested_parser = Parser::new(&uri, nested_tokens);
                let nested_stmts = nested_parser.parse_program()?;
                let mut res = self.run_program(&nested_stmts, span);
                if res.is_ok()
                    && let Some(func_arg) = args.get(1)
                {
                    let func_name = func_arg.to_display_string();
                    if !func_name.is_empty() && func_name != "undefined" && func_name != "null" {
                        let extra_args = if args.len() > 2 { &args[2..] } else { &[] };
                        res = self.call_global_or_user(&func_name, extra_args, span);
                    }
                }
                self.script_recursion -= 1;
                self.current_file = prev_file;
                res
            }
            _ => Err(self.err(
                span,
                format!("Unsupported Flash authoring method 'fl.{method}'."),
            )),
        }
    }

    fn call_flfile_method(
        &mut self,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let first_str = args
            .first()
            .map(JsflValue::to_display_string)
            .unwrap_or_default();
        match method {
            "platformPathToURI" => {
                let uri = platform_path_to_uri(&self.current_file, span, &first_str)?;
                Ok(JsflValue::String(uri))
            }
            "uriToPlatformPath" => {
                let path = uri_to_platform_path(&self.current_file, span, &first_str)?;
                Ok(JsflValue::String(path))
            }
            "exists" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(self.err(
                        span,
                        "FLfile.exists denied: read capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                Ok(JsflValue::Bool(path.exists()))
            }
            "read" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(
                        self.err(span, "FLfile.read denied: read capability is not enabled.")
                    );
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    self.err(span, format!("FLfile.read('{first_str}') failed: {e}"))
                })?;
                Ok(JsflValue::String(content))
            }
            "write" => {
                if !self.sandbox.flfile_caps.allow_write {
                    return Err(self.err(
                        span,
                        "FLfile.write denied: write capability is not enabled.",
                    ));
                }
                let text = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let mode = args
                    .get(2)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                if mode.eq_ignore_ascii_case("append") && path.exists() {
                    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
                    existing.push_str(&text);
                    std::fs::write(&path, existing).map_err(|e| {
                        self.err(span, format!("FLfile.write('{first_str}') failed: {e}"))
                    })?;
                } else {
                    std::fs::write(&path, text).map_err(|e| {
                        self.err(span, format!("FLfile.write('{first_str}') failed: {e}"))
                    })?;
                }
                Ok(JsflValue::Bool(true))
            }
            "createFolder" => {
                if !self.sandbox.flfile_caps.allow_write {
                    return Err(self.err(
                        span,
                        "FLfile.createFolder denied: write capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                std::fs::create_dir_all(&path).map_err(|e| {
                    self.err(
                        span,
                        format!("FLfile.createFolder('{first_str}') failed: {e}"),
                    )
                })?;
                Ok(JsflValue::Bool(true))
            }
            "remove" => {
                if !self.sandbox.flfile_caps.allow_delete {
                    return Err(self.err(
                        span,
                        "FLfile.remove denied: delete capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                if path.is_dir() {
                    std::fs::remove_dir_all(&path).map_err(|e| {
                        self.err(span, format!("FLfile.remove('{first_str}') failed: {e}"))
                    })?;
                } else if path.exists() {
                    std::fs::remove_file(&path).map_err(|e| {
                        self.err(span, format!("FLfile.remove('{first_str}') failed: {e}"))
                    })?;
                } else {
                    return Ok(JsflValue::Bool(false));
                }
                Ok(JsflValue::Bool(true))
            }
            "listFolder" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(self.err(
                        span,
                        "FLfile.listFolder denied: read capability is not enabled.",
                    ));
                }
                let files_only = args.get(1).is_some_and(JsflValue::is_truthy);
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let rd = std::fs::read_dir(&path).map_err(|e| {
                    self.err(
                        span,
                        format!("FLfile.listFolder('{first_str}') failed: {e}"),
                    )
                })?;
                let mut names = Vec::new();
                for entry in rd.flatten() {
                    if files_only && entry.path().is_dir() {
                        continue;
                    }
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_owned());
                    }
                }
                names.sort();
                Ok(JsflValue::Array(
                    names.into_iter().map(JsflValue::String).collect(),
                ))
            }
            _ => Err(self.err(
                span,
                format!("Unsupported FLfile method 'FLfile.{method}'."),
            )),
        }
    }

    fn call_element_method(
        &mut self,
        eh: ElementHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match method {
            "getTextString" => {
                let el = self.resolve_element(&eh, span)?;
                match &el.kind {
                    ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                        Ok(JsflValue::String(text.clone()))
                    }
                    _ => Err(self.err(span, "Element.getTextString() requires a Text element.")),
                }
            }
            "setTextString" => {
                self.record_mutation(span)?;
                let new_text = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                        *text = new_text;
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "Element.setTextString() requires a Text element.")),
                }
            }
            "getTextAttr" => {
                let attr = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "fillColor".to_owned());
                let el = self.resolve_element(&eh, span)?;
                match attr.as_str() {
                    "fillColor" | "color" => match &el.kind {
                        ElementKind::Text { color, .. } => {
                            Ok(JsflValue::String(format_rgba_hex(*color)))
                        }
                        ElementKind::TextBox { format, .. } => {
                            let c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            Ok(JsflValue::String(format_hex_color(c)))
                        }
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    "size" => match &el.kind {
                        ElementKind::Text { font_size, .. } => Ok(JsflValue::Number(*font_size)),
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Number(format.size)),
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    "face" => match &el.kind {
                        ElementKind::Text { font_family, .. } => {
                            Ok(JsflValue::String(font_family.clone()))
                        }
                        ElementKind::TextBox { format, .. } => {
                            Ok(JsflValue::String(format.font.clone()))
                        }
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    _ => Ok(JsflValue::Undefined),
                }
            }
            "setTextAttr" => {
                self.record_mutation(span)?;
                let attr = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "fillColor".to_owned());
                let val = args.get(1).cloned().unwrap_or(JsflValue::Undefined);
                match attr.as_str() {
                    "fillColor" | "color" => {
                        let new_color = parse_color_value(&val).ok_or_else(|| {
                            self.err(
                                span,
                                "Element.setTextAttr('fillColor', value) expects a '#RRGGBB' string or 0xRRGGBB integer.",
                            )
                        })?;
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { color, .. } => {
                                *color = new_color;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                let rgb = (u32::from(new_color[0]) << 16)
                                    | (u32::from(new_color[1]) << 8)
                                    | u32::from(new_color[2]);
                                format.color = rgb;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    "size" => {
                        let sz = val.as_number().unwrap_or(16.0).clamp(1.0, 500.0);
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { font_size, .. } => {
                                *font_size = sz;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                format.size = sz;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    "face" => {
                        let face = val.to_display_string();
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { font_family, .. } => {
                                *font_family = face;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                format.font = face;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    _ => Ok(JsflValue::Undefined),
                }
            }
            "getCustomFill" => {
                let el = self.resolve_element(&eh, span)?;
                let mut obj = BTreeMap::new();
                if let ElementKind::PaintedPath {
                    fill_gradient: Some(grad),
                    ..
                } = &el.kind
                {
                    let style = match grad.kind {
                        crate::GradientKind::Linear => "linearGradient",
                        crate::GradientKind::Radial => "radialGradient",
                    };
                    obj.insert("style".to_owned(), JsflValue::String(style.to_owned()));
                    let colors: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::String(format_rgba_hex(s.color)))
                        .collect();
                    let positions: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::Number(f64::from(s.ratio)))
                        .collect();
                    let first_color = grad
                        .stops
                        .first()
                        .map(|s| s.color)
                        .unwrap_or([0, 0, 0, 255]);
                    obj.insert(
                        "color".to_owned(),
                        JsflValue::String(format_rgba_hex(first_color)),
                    );
                    obj.insert("colorArray".to_owned(), JsflValue::Array(colors));
                    obj.insert("posArray".to_owned(), JsflValue::Array(positions));
                    return Ok(JsflValue::Object(obj));
                }
                let fill_color = match &el.kind {
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. }
                    | ElementKind::PaintedPath { fill, .. } => *fill,
                    _ => [0, 0, 0, 255],
                };
                obj.insert(
                    "style".to_owned(),
                    JsflValue::String(
                        if fill_color[3] == 0 {
                            "noFill"
                        } else {
                            "solid"
                        }
                        .to_owned(),
                    ),
                );
                obj.insert(
                    "color".to_owned(),
                    JsflValue::String(format_rgba_hex(fill_color)),
                );
                Ok(JsflValue::Object(obj))
            }
            "setCustomFill" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(span, "setCustomFill expects a Fill object."));
                };
                let style = map
                    .get("style")
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "solid".to_owned());
                let parsed_stops: Option<Vec<crate::GradientStop>> =
                    if let Some(JsflValue::Array(color_arr)) = map.get("colorArray") {
                        let pos_arr = match map.get("posArray") {
                            Some(JsflValue::Array(p)) => Some(p.as_slice()),
                            _ => None,
                        };
                        let len = color_arr.len().max(1);
                        let mut stops = Vec::with_capacity(color_arr.len());
                        for (idx, c_val) in color_arr.iter().enumerate() {
                            let c = parse_color_value(c_val).unwrap_or([0, 0, 0, 255]);
                            let default_ratio = if len <= 1 {
                                0u8
                            } else {
                                ((idx * 255) / (len - 1)).min(255) as u8
                            };
                            let ratio = pos_arr
                                .and_then(|p| p.get(idx))
                                .and_then(JsflValue::as_number)
                                .map(|n| n.round().clamp(0.0, 255.0) as u8)
                                .unwrap_or(default_ratio);
                            stops.push(crate::GradientStop { ratio, color: c });
                        }
                        if stops.is_empty() { None } else { Some(stops) }
                    } else {
                        None
                    };
                let new_fill = if style == "noFill" {
                    [0, 0, 0, 0]
                } else if let Some(stops) = &parsed_stops {
                    stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255])
                } else {
                    map.get("color")
                        .and_then(parse_color_value)
                        .unwrap_or([0, 0, 0, 255])
                };
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::PaintedPath {
                        fill,
                        fill_gradient,
                        ..
                    } => {
                        *fill = new_fill;
                        if style == "linearGradient" || style == "radialGradient" {
                            let kind = if style == "radialGradient" {
                                crate::GradientKind::Radial
                            } else {
                                crate::GradientKind::Linear
                            };
                            if let Some(grad) = fill_gradient {
                                grad.kind = kind;
                                if let Some(stops) = parsed_stops {
                                    grad.stops = stops;
                                }
                            } else if let Some(stops) = parsed_stops {
                                *fill_gradient = Some(crate::GradientPaint {
                                    kind,
                                    matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                    stops,
                                    ..Default::default()
                                });
                            }
                        } else if style == "solid" || style == "noFill" {
                            *fill_gradient = None;
                        }
                        Ok(JsflValue::Undefined)
                    }
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. } => {
                        *fill = new_fill;
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "setCustomFill requires a Shape element.")),
                }
            }
            "getCustomStroke" => {
                let el = self.resolve_element(&eh, span)?;
                let mut obj = BTreeMap::new();
                if let ElementKind::PaintedPath {
                    stroke_gradient: Some(grad),
                    stroke_width,
                    ..
                } = &el.kind
                {
                    let style = match grad.kind {
                        crate::GradientKind::Linear => "linearGradient",
                        crate::GradientKind::Radial => "radialGradient",
                    };
                    obj.insert("style".to_owned(), JsflValue::String(style.to_owned()));
                    let colors: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::String(format_rgba_hex(s.color)))
                        .collect();
                    let positions: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::Number(f64::from(s.ratio)))
                        .collect();
                    let first_color = grad
                        .stops
                        .first()
                        .map(|s| s.color)
                        .unwrap_or([0, 0, 0, 255]);
                    obj.insert(
                        "color".to_owned(),
                        JsflValue::String(format_rgba_hex(first_color)),
                    );
                    obj.insert("thickness".to_owned(), JsflValue::Number(*stroke_width));
                    obj.insert("colorArray".to_owned(), JsflValue::Array(colors));
                    obj.insert("posArray".to_owned(), JsflValue::Array(positions));
                    return Ok(JsflValue::Object(obj));
                }
                let (stroke_color, width) = match &el.kind {
                    ElementKind::Rectangle {
                        stroke,
                        stroke_width,
                        ..
                    }
                    | ElementKind::Ellipse {
                        stroke,
                        stroke_width,
                        ..
                    }
                    | ElementKind::Path {
                        stroke,
                        stroke_width,
                        ..
                    }
                    | ElementKind::PaintedPath {
                        stroke,
                        stroke_width,
                        ..
                    } => (*stroke, *stroke_width),
                    _ => ([0, 0, 0, 0], 0.0),
                };
                obj.insert(
                    "style".to_owned(),
                    JsflValue::String(
                        if width <= 0.0 || stroke_color[3] == 0 {
                            "noStroke"
                        } else {
                            "solid"
                        }
                        .to_owned(),
                    ),
                );
                obj.insert(
                    "color".to_owned(),
                    JsflValue::String(format_rgba_hex(stroke_color)),
                );
                obj.insert("thickness".to_owned(), JsflValue::Number(width));
                Ok(JsflValue::Object(obj))
            }
            "setCustomStroke" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(span, "setCustomStroke expects a Stroke object."));
                };
                let style = map
                    .get("style")
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "solid".to_owned());
                let thickness = map
                    .get("thickness")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(1.0)
                    .max(0.0);
                let parsed_stops: Option<Vec<crate::GradientStop>> =
                    if let Some(JsflValue::Array(color_arr)) = map.get("colorArray") {
                        let pos_arr = match map.get("posArray") {
                            Some(JsflValue::Array(p)) => Some(p.as_slice()),
                            _ => None,
                        };
                        let len = color_arr.len().max(1);
                        let mut stops = Vec::with_capacity(color_arr.len());
                        for (idx, c_val) in color_arr.iter().enumerate() {
                            let c = parse_color_value(c_val).unwrap_or([0, 0, 0, 255]);
                            let default_ratio = if len <= 1 {
                                0u8
                            } else {
                                ((idx * 255) / (len - 1)).min(255) as u8
                            };
                            let ratio = pos_arr
                                .and_then(|p| p.get(idx))
                                .and_then(JsflValue::as_number)
                                .map(|n| n.round().clamp(0.0, 255.0) as u8)
                                .unwrap_or(default_ratio);
                            stops.push(crate::GradientStop { ratio, color: c });
                        }
                        if stops.is_empty() { None } else { Some(stops) }
                    } else {
                        None
                    };
                let new_stroke = if style == "noStroke" || thickness <= 0.0 {
                    [0, 0, 0, 0]
                } else if let Some(stops) = &parsed_stops {
                    stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255])
                } else {
                    map.get("color")
                        .and_then(parse_color_value)
                        .unwrap_or([0, 0, 0, 255])
                };
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::PaintedPath {
                        stroke,
                        stroke_gradient,
                        stroke_width,
                        ..
                    } => {
                        *stroke = new_stroke;
                        *stroke_width = if style == "noStroke" { 0.0 } else { thickness };
                        if style == "linearGradient" || style == "radialGradient" {
                            let kind = if style == "radialGradient" {
                                crate::GradientKind::Radial
                            } else {
                                crate::GradientKind::Linear
                            };
                            if let Some(grad) = stroke_gradient {
                                grad.kind = kind;
                                if let Some(stops) = parsed_stops {
                                    grad.stops = stops;
                                }
                            } else if let Some(stops) = parsed_stops {
                                *stroke_gradient = Some(crate::GradientPaint {
                                    kind,
                                    matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                    stops,
                                    ..Default::default()
                                });
                            }
                        } else if style == "solid" || style == "noStroke" {
                            *stroke_gradient = None;
                        }
                        Ok(JsflValue::Undefined)
                    }
                    ElementKind::Rectangle {
                        stroke,
                        stroke_width,
                        ..
                    }
                    | ElementKind::Ellipse {
                        stroke,
                        stroke_width,
                        ..
                    }
                    | ElementKind::Path {
                        stroke,
                        stroke_width,
                        ..
                    } => {
                        *stroke = new_stroke;
                        *stroke_width = if style == "noStroke" { 0.0 } else { thickness };
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "setCustomStroke requires a Shape element.")),
                }
            }
            _ => Err(self.err(span, format!("Unsupported Element method '{method}'."))),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn ensure_active_keyframe_mut(&mut self, span: Span) -> Result<&mut Keyframe, JsflDiagnostic> {
        let active_sym = self.state.active_symbol.clone();
        let cur_layer = self.state.current_layer;
        let cur_frame = self.state.current_frame;
        let err_layer = self.err(span, "Active layer index is out of bounds.");
        let sym = self.symbol_mut(&active_sym, span)?;
        let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
        if let Some(pos) = layer.keyframes.iter().position(|k| k.frame == cur_frame) {
            return Ok(&mut layer.keyframes[pos]);
        }
        let Some(pos) = layer
            .keyframes
            .iter()
            .enumerate()
            .filter(|(_, k)| k.frame <= cur_frame)
            .max_by_key(|(_, k)| k.frame)
            .map(|(i, _)| i)
        else {
            layer.keyframes.push(Keyframe {
                frame: 0,
                ..Default::default()
            });
            return Ok(&mut layer.keyframes[0]);
        };
        Ok(&mut layer.keyframes[pos])
    }

    fn call_doc_method(
        &mut self,
        doc: DocHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(doc, span)?;
        match method {
            "getTimeline" => Ok(JsflValue::Timeline(TimelineHandle {
                doc,
                symbol_id: self.state.active_symbol.clone(),
            })),
            "selectAll" => {
                self.record_mutation(span)?;
                let sym = self.symbol_ref(&self.state.active_symbol, span)?;
                let mut ids = Vec::new();
                for layer in &sym.timeline.layers {
                    if !layer.visible || layer.locked {
                        continue;
                    }
                    if let Some(kf) = layer.key_at(self.state.current_frame) {
                        for el in &kf.elements {
                            ids.push(el.id.clone());
                        }
                    }
                }
                self.state.selected_element_ids = ids;
                Ok(JsflValue::Undefined)
            }
            "selectNone" => {
                self.record_mutation(span)?;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "group" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(span, "Document.group() requires a non-empty selection."));
                }
                let members = self.state.selected_element_ids.clone();
                let active_sym = self.state.active_symbol.clone();
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Active layer index is out of bounds.");
                let sym = self.symbol_mut(&active_sym, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer.properties.groups.push(AuthoringGroup {
                    id: new_id("group"),
                    name: "Group".to_owned(),
                    members,
                });
                Ok(JsflValue::Undefined)
            }
            "unGroup" => {
                self.record_mutation(span)?;
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let active_sym = self.state.active_symbol.clone();
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Active layer index is out of bounds.");
                let sym = self.symbol_mut(&active_sym, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer
                    .properties
                    .groups
                    .retain(|g| !g.members.iter().any(|m| selected.contains(m)));
                Ok(JsflValue::Undefined)
            }
            "breakApart" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(
                        span,
                        "Document.breakApart() requires a non-empty selection.",
                    ));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let sym_map: HashMap<Id, Vec<Element>> = self
                    .project
                    .symbols
                    .iter()
                    .map(|s| {
                        let first_els: Vec<Element> = s
                            .timeline
                            .layers
                            .iter()
                            .filter_map(|l| l.key_at(0))
                            .flat_map(|k| k.elements.iter().cloned())
                            .collect();
                        (s.id.clone(), first_els)
                    })
                    .collect();

                let kf = self.ensure_active_keyframe_mut(span)?;
                let mut new_elements = Vec::new();
                let mut new_selection = Vec::new();
                for el in kf.elements.drain(..) {
                    if !selected.contains(&el.id) {
                        new_elements.push(el);
                        continue;
                    }
                    match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            if let Some(children) = sym_map.get(symbol_id)
                                && !children.is_empty()
                            {
                                for mut child in children.clone() {
                                    child.id = new_id("el");
                                    child.transform.x += el.transform.x;
                                    child.transform.y += el.transform.y;
                                    child.transform.scale_x *= el.transform.scale_x;
                                    child.transform.scale_y *= el.transform.scale_y;
                                    child.transform.rotation += el.transform.rotation;
                                    new_selection.push(child.id.clone());
                                    new_elements.push(child);
                                }
                            } else {
                                let mut path_el = el.clone();
                                path_el.kind = ElementKind::Path {
                                    commands: crate::geometry::rectangle_path_commands(
                                        40.0,
                                        40.0,
                                        Default::default(),
                                    ),
                                    fill_rule: FillRule::NonZero,
                                    fill: [128, 128, 128, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                };
                                path_el.merge = true;
                                new_selection.push(path_el.id.clone());
                                new_elements.push(path_el);
                            }
                        }
                        ElementKind::Rectangle {
                            width,
                            height,
                            fill,
                            stroke,
                            stroke_width,
                        } => {
                            let cmds = crate::geometry::rectangle_path_commands(
                                *width,
                                *height,
                                el.rectangle_options(),
                            );
                            let mut broken = el.clone();
                            broken.kind = ElementKind::Path {
                                commands: cmds,
                                fill_rule: FillRule::NonZero,
                                fill: *fill,
                                stroke: *stroke,
                                stroke_width: *stroke_width,
                            };
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                        ElementKind::Ellipse {
                            width,
                            height,
                            fill,
                            stroke,
                            stroke_width,
                        } => {
                            let cmds = crate::geometry::oval_path_commands(
                                *width,
                                *height,
                                el.oval_options(),
                            );
                            let mut broken = el.clone();
                            broken.kind = ElementKind::Path {
                                commands: cmds,
                                fill_rule: FillRule::NonZero,
                                fill: *fill,
                                stroke: *stroke,
                                stroke_width: *stroke_width,
                            };
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                        _ => {
                            let mut broken = el;
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                    }
                }
                kf.elements = new_elements;
                self.state.selected_element_ids = new_selection;
                Ok(JsflValue::Undefined)
            }
            "convertToSymbol" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(
                        span,
                        "Document.convertToSymbol() requires a non-empty selection.",
                    ));
                }
                let type_str = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "movie clip".to_owned())
                    .to_lowercase();
                let kind = match type_str.as_str() {
                    "movie clip" | "movieclip" => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => {
                        return Err(
                            self.err(span, format!("Invalid convertToSymbol type '{type_str}'."))
                        );
                    }
                };
                let raw_name = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let sym_name = if raw_name.trim().is_empty() {
                    format!("Symbol {}", self.project.symbols.len())
                } else {
                    raw_name.trim().to_owned()
                };
                let reg_point = args
                    .get(2)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "center".to_owned())
                    .to_lowercase();

                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                let mut extracted = Vec::new();
                let mut remaining = Vec::new();
                let mut insert_idx = None;
                for el in kf.elements.drain(..) {
                    if selected.contains(&el.id) {
                        if insert_idx.is_none() {
                            insert_idx = Some(remaining.len());
                        }
                        extracted.push(el);
                    } else {
                        remaining.push(el);
                    }
                }
                if extracted.is_empty() {
                    kf.elements = remaining;
                    return Err(self.err(
                        span,
                        "Selected elements were not found on the active keyframe.",
                    ));
                }

                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for el in &extracted {
                    let (w, h) = Self::element_local_size(el);
                    min_x = min_x.min(el.transform.x);
                    min_y = min_y.min(el.transform.y);
                    max_x = max_x.max(el.transform.x + w * el.transform.scale_x.abs());
                    max_y = max_y.max(el.transform.y + h * el.transform.scale_y.abs());
                }
                let (origin_x, origin_y) = match reg_point.as_str() {
                    "top left" => (min_x, min_y),
                    "bottom right" => (max_x, max_y),
                    _ => (0.5 * (min_x + max_x), 0.5 * (min_y + max_y)),
                };
                for el in &mut extracted {
                    el.transform.x -= origin_x;
                    el.transform.y -= origin_y;
                }

                let sym_id = new_id("sym");
                let inst_id = new_id("inst");
                let inst = Element {
                    id: inst_id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: origin_x,
                        y: origin_y,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: sym_id.clone(),
                        playback: match kind {
                            SymbolKind::Graphic => InstancePlayback::Loop,
                            _ => InstancePlayback::Independent,
                        },
                    },
                    ..Default::default()
                };
                let idx = insert_idx.unwrap_or(remaining.len());
                remaining.insert(idx, inst);
                kf.elements = remaining;

                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: sym_name,
                    folder: String::new(),
                    kind,
                    timeline: crate::Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: "Layer 1".to_owned(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: extracted,
                                ..Default::default()
                            }],
                            properties: Default::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.state.selected_element_ids = vec![inst_id];
                Ok(JsflValue::Item(ItemHandle {
                    doc,
                    item_id: sym_id,
                    is_folder: false,
                }))
            }
            "moveSelectionBy" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(
                        span,
                        "Document.moveSelectionBy expects a {x, y} distance object.",
                    ));
                };
                let dx = map.get("x").and_then(JsflValue::as_number).unwrap_or(0.0);
                let dy = map.get("y").and_then(JsflValue::as_number).unwrap_or(0.0);
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.x += dx;
                        el.transform.y += dy;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "scaleSelection" => {
                self.record_mutation(span)?;
                let sx = args.first().and_then(JsflValue::as_number).unwrap_or(1.0);
                let sy = args.get(1).and_then(JsflValue::as_number).unwrap_or(1.0);
                if !sx.is_finite() || !sy.is_finite() || sx.abs() < 1e-6 || sy.abs() < 1e-6 {
                    return Err(self.err(
                        span,
                        "Document.scaleSelection requires finite non-zero scale factors.",
                    ));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.scale_x *= sx;
                        el.transform.scale_y *= sy;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "rotateSelection" => {
                self.record_mutation(span)?;
                let angle = args.first().and_then(JsflValue::as_number).unwrap_or(0.0);
                if !angle.is_finite() {
                    return Err(self.err(span, "Document.rotateSelection requires a finite angle."));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.rotation += angle;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "deleteSelection" => {
                self.record_mutation(span)?;
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.retain(|el| !selected.contains(&el.id));
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "addNewRectangle" | "addNewOval" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(rect)) = args.first() else {
                    return Err(self.err(
                        span,
                        format!(
                            "Document.{method} requires a {{left, top, right, bottom}} rectangle."
                        ),
                    ));
                };
                let left = rect
                    .get("left")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let top = rect
                    .get("top")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let right = rect
                    .get("right")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(100.0);
                let bottom = rect
                    .get("bottom")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(100.0);
                let width = (right - left).abs().max(1.0);
                let height = (bottom - top).abs().max(1.0);
                let id = new_id("shape");
                let mut el = Element {
                    id: id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: left.min(right),
                        y: top.min(bottom),
                        ..Default::default()
                    },
                    kind: if method == "addNewRectangle" {
                        ElementKind::Rectangle {
                            width,
                            height,
                            fill: [74, 144, 226, 255],
                            stroke: [20, 20, 20, 255],
                            stroke_width: 1.0,
                        }
                    } else {
                        ElementKind::Ellipse {
                            width,
                            height,
                            fill: [74, 144, 226, 255],
                            stroke: [20, 20, 20, 255],
                            stroke_width: 1.0,
                        }
                    },
                    ..Default::default()
                };
                if method == "addNewRectangle"
                    && let Some(r) = args.get(1).and_then(JsflValue::as_number)
                    && r > 0.0
                {
                    el.set_rectangle_options(crate::RectangleOptions {
                        corner_radii: [r; 4],
                        corners_locked: true,
                    });
                }
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.push(el);
                self.state.selected_element_ids = vec![id];
                Ok(JsflValue::Undefined)
            }
            "addNewText" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(rect)) = args.first() else {
                    return Err(self.err(
                        span,
                        "Document.addNewText requires a {left, top, right, bottom} rectangle.",
                    ));
                };
                let left = rect
                    .get("left")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let top = rect
                    .get("top")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let text_str = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "Text".to_owned());
                let id = new_id("text");
                let el = Element {
                    id: id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: left,
                        y: top,
                        ..Default::default()
                    },
                    kind: ElementKind::Text {
                        text: text_str,
                        font_family: "sans-serif".to_owned(),
                        font_size: 16.0,
                        color: [0, 0, 0, 255],
                    },
                    ..Default::default()
                };
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.push(el);
                self.state.selected_element_ids = vec![id];
                Ok(JsflValue::Undefined)
            }
            "addItem" => {
                let pos = args.first().cloned().unwrap_or(JsflValue::Undefined);
                let Some(JsflValue::Item(ih)) = args.get(1) else {
                    return Err(self.err(span, "Document.addItem expects (position, Item)."));
                };
                if ih.is_folder {
                    return Err(self.err(span, "Cannot place a folder Item onto the stage."));
                }
                let sym_id = ih.item_id.clone();
                self.place_symbol_on_stage(doc, &pos, &sym_id, span)
            }
            "exitEditMode" => {
                self.record_mutation(span)?;
                self.state.active_symbol = self.project.root.clone();
                self.state.current_layer = 0;
                self.state.current_frame = 0;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "batchRename" => self.call_jsfl_batch_rename(args, span),
            "getProjectColors" => {
                let entries = collect_project_colors(&self.project);
                let arr: Vec<JsflValue> = entries
                    .into_iter()
                    .map(|entry| {
                        let mut m = BTreeMap::new();
                        m.insert(
                            "color".to_owned(),
                            JsflValue::String(format_rgba_hex(entry.color)),
                        );
                        m.insert(
                            "fillCount".to_owned(),
                            JsflValue::Number(entry.fill_count as f64),
                        );
                        m.insert(
                            "strokeCount".to_owned(),
                            JsflValue::Number(entry.stroke_count as f64),
                        );
                        m.insert(
                            "gradientCount".to_owned(),
                            JsflValue::Number(entry.gradient_count as f64),
                        );
                        m.insert(
                            "textCount".to_owned(),
                            JsflValue::Number(entry.text_count as f64),
                        );
                        m.insert(
                            "inSwatches".to_owned(),
                            JsflValue::Bool(entry.in_custom_swatches),
                        );
                        m.insert(
                            "totalUses".to_owned(),
                            JsflValue::Number(entry.total_uses() as f64),
                        );
                        JsflValue::Object(m)
                    })
                    .collect();
                Ok(JsflValue::Array(arr))
            }
            "changeColor" | "replaceColor" => self.call_jsfl_change_color(args, span),
            _ => Err(self.err(span, format!("Unsupported Document method 'doc.{method}'."))),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn call_timeline_method(
        &mut self,
        tl: TimelineHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(tl.doc, span)?;
        match method {
            "getSelectedLayers" => {
                let arr = self
                    .state
                    .selected_layers
                    .iter()
                    .map(|&i| JsflValue::Number(i as f64))
                    .collect();
                Ok(JsflValue::Array(arr))
            }
            "setSelectedLayers" => {
                self.record_mutation(span)?;
                let idx = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let replace = args.get(1).map(JsflValue::is_truthy).unwrap_or(true);
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                if idx >= sym.timeline.layers.len() {
                    return Err(self.err(span, format!("Layer index {idx} out of bounds.")));
                }
                self.state.current_layer = idx;
                if replace {
                    self.state.selected_layers = vec![idx];
                } else if !self.state.selected_layers.contains(&idx) {
                    self.state.selected_layers.push(idx);
                }
                Ok(JsflValue::Undefined)
            }
            "getSelectedFrames" => {
                let mut flat = Vec::new();
                for &(layer_idx, start_f, end_f) in &self.state.selected_frames {
                    flat.push(JsflValue::Number(layer_idx as f64));
                    flat.push(JsflValue::Number(start_f as f64));
                    flat.push(JsflValue::Number(end_f as f64));
                }
                Ok(JsflValue::Array(flat))
            }
            "setSelectedFrames" => {
                self.record_mutation(span)?;
                if let Some(JsflValue::Array(arr)) = args.first() {
                    let mut spans = Vec::new();
                    for chunk in arr.chunks(3) {
                        if chunk.len() == 3 {
                            let l = chunk[0].as_number().unwrap_or(0.0).max(0.0) as usize;
                            let s = chunk[1].as_number().unwrap_or(0.0).max(0.0) as u32;
                            let e = chunk[2].as_number().unwrap_or((s + 1) as f64).max(0.0) as u32;
                            spans.push((l, s, e.max(s + 1)));
                        }
                    }
                    if let Some(&(l, s, _)) = spans.first() {
                        self.state.current_layer = l;
                        self.state.current_frame = s;
                    }
                    self.state.selected_frames = spans;
                    return Ok(JsflValue::Undefined);
                }
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as u32;
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .unwrap_or((start_f + 1) as f64)
                    .max((start_f + 1) as f64) as u32;
                let replace = args.get(2).map(JsflValue::is_truthy).unwrap_or(true);
                let l = self.state.current_layer;
                self.state.current_frame = start_f;
                if replace {
                    self.state.selected_frames = vec![(l, start_f, end_f)];
                } else {
                    self.state.selected_frames.push((l, start_f, end_f));
                }
                Ok(JsflValue::Undefined)
            }
            "addNewLayer" => {
                self.record_mutation(span)?;
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let name = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined")
                    .unwrap_or_else(|| format!("Layer {}", sym.timeline.layers.len() + 1));
                let layer_type = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "normal".to_owned())
                    .to_lowercase();
                let role = match layer_type.as_str() {
                    "guide" => LayerRole::Guide,
                    "mask" => LayerRole::Mask,
                    "folder" => LayerRole::Folder,
                    _ => LayerRole::Normal,
                };
                let mut new_layer = Layer {
                    id: new_id("layer"),
                    name,
                    visible: true,
                    locked: false,
                    keyframes: if role == LayerRole::Folder {
                        Vec::new()
                    } else {
                        vec![Keyframe {
                            frame: 0,
                            ..Default::default()
                        }]
                    },
                    properties: Default::default(),
                };
                new_layer.properties.role = role;
                sym.timeline.layers.push(new_layer);
                let new_idx = sym.timeline.layers.len() - 1;
                self.state.current_layer = new_idx;
                self.state.selected_layers = vec![new_idx];
                Ok(JsflValue::Number(new_idx as f64))
            }
            "deleteLayer" => {
                self.record_mutation(span)?;
                let idx = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as usize)
                    .unwrap_or(self.state.current_layer);
                let err_min = self.err(span, "Cannot delete the last remaining layer.");
                let err_oob = self.err(span, format!("Layer index {idx} out of bounds."));
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                if sym.timeline.layers.len() <= 1 {
                    return Err(err_min);
                }
                if idx >= sym.timeline.layers.len() {
                    return Err(err_oob);
                }
                sym.timeline.layers.remove(idx);
                let max_idx = sym.timeline.layers.len() - 1;
                self.state.current_layer = self.state.current_layer.min(max_idx);
                self.state.selected_layers = vec![self.state.current_layer];
                self.project.prune_motion_guides();
                Ok(JsflValue::Undefined)
            }
            "reorderLayer" => {
                self.record_mutation(span)?;
                let from = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let to = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let add_before = args.get(2).map(JsflValue::is_truthy).unwrap_or(true);
                let err_oob = self.err(span, "reorderLayer index out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let len = sym.timeline.layers.len();
                if from >= len || to >= len {
                    return Err(err_oob);
                }
                if from != to {
                    let layer = sym.timeline.layers.remove(from);
                    let mut target = if from < to { to - 1 } else { to };
                    if !add_before {
                        target = (target + 1).min(sym.timeline.layers.len());
                    }
                    sym.timeline.layers.insert(target, layer);
                    self.state.current_layer = target;
                }
                Ok(JsflValue::Undefined)
            }
            "insertFrames" => {
                self.record_mutation(span)?;
                let num = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                let all_layers = args.get(1).map(JsflValue::is_truthy).unwrap_or(false);
                let at_frame = args
                    .get(2)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let cur_layer = self.state.current_layer;
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                sym.timeline.frame_count = sym.timeline.frame_count.saturating_add(num);
                for (idx, layer) in sym.timeline.layers.iter_mut().enumerate() {
                    if !all_layers && idx != cur_layer {
                        continue;
                    }
                    for kf in &mut layer.keyframes {
                        if kf.frame > at_frame {
                            kf.frame = kf.frame.saturating_add(num);
                        }
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "removeFrames" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let count = end_f.saturating_sub(start_f).max(1);
                let cur_layer = self.state.current_layer;
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                if let Some(layer) = sym.timeline.layers.get_mut(cur_layer) {
                    layer
                        .keyframes
                        .retain(|k| k.frame == 0 || k.frame < start_f || k.frame >= end_f);
                    for kf in &mut layer.keyframes {
                        if kf.frame >= end_f {
                            kf.frame = kf.frame.saturating_sub(count);
                        }
                    }
                    if layer.keyframes.is_empty() && layer.properties.role != LayerRole::Folder {
                        layer.keyframes.push(Keyframe {
                            frame: 0,
                            ..Default::default()
                        });
                    }
                    layer.normalize_motion_keys();
                }
                let max_kf = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .map(|k| k.frame + 1)
                    .max()
                    .unwrap_or(1);
                sym.timeline.frame_count = sym
                    .timeline
                    .frame_count
                    .saturating_sub(count)
                    .max(max_kf)
                    .max(1);
                let new_fc = sym.timeline.frame_count;
                self.state.current_frame = self.state.current_frame.min(new_fc - 1);
                Ok(JsflValue::Undefined)
            }
            "insertKeyframe" | "insertBlankKeyframe" => {
                self.record_mutation(span)?;
                let f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let blank = method == "insertBlankKeyframe";
                let cur_layer = self.state.current_layer;
                let fps = self.project.fps as f64;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                if f >= sym.timeline.frame_count {
                    sym.timeline.frame_count = f + 1;
                }
                let sampled_elements = if blank {
                    Vec::new()
                } else {
                    let layer_ref = sym
                        .timeline
                        .layers
                        .get(cur_layer)
                        .ok_or_else(|| err_layer.clone())?;
                    crate::evaluate::sample_layer_in_at_fps(&sym.timeline, layer_ref, f, fps)
                };
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == f) {
                    if blank {
                        existing.elements.clear();
                    }
                } else {
                    layer.keyframes.push(Keyframe {
                        frame: f,
                        elements: sampled_elements,
                        ..Default::default()
                    });
                    layer.keyframes.sort_by_key(|k| k.frame);
                }
                self.state.current_frame = f;
                Ok(JsflValue::Undefined)
            }
            "clearKeyframes" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer
                    .keyframes
                    .retain(|k| k.frame == 0 || k.frame < start_f || k.frame >= end_f);
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            "createMotionTween" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                let target_start = layer.key_at(start_f).map(|k| k.frame).unwrap_or(0);
                for kf in &mut layer.keyframes {
                    if kf.frame >= target_start && kf.frame < end_f {
                        kf.tween = Some(Easing::Linear);
                        kf.tween_kind = TweenKind::Classic;
                    }
                }
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            "reverseFrames" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(0);
                let sym_fc = self.symbol_ref(&tl.symbol_id, span)?.timeline.frame_count;
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(sym_fc);
                if end_f <= start_f + 1 {
                    return Ok(JsflValue::Undefined);
                }
                let last_inclusive = end_f - 1;
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                for kf in &mut layer.keyframes {
                    if kf.frame >= start_f && kf.frame <= last_inclusive {
                        kf.frame = start_f + (last_inclusive - kf.frame);
                    }
                }
                layer.keyframes.sort_by_key(|k| k.frame);
                if let Some(first) = layer.keyframes.first_mut()
                    && first.frame != 0
                {
                    first.frame = 0;
                }
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            _ => Err(self.err(
                span,
                format!("Unsupported Timeline method 'timeline.{method}'."),
            )),
        }
    }
}

impl<'a> JsflVm<'a> {
    fn call_library_method(
        &mut self,
        doc: DocHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(doc, span)?;
        match method {
            "itemExists" => {
                let path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let trimmed = path.trim().trim_matches('/');
                let exists = self.find_library_symbol_by_path(trimmed).is_some()
                    || self
                        .project
                        .library_folder_paths()
                        .iter()
                        .any(|f| f == trimmed);
                Ok(JsflValue::Bool(exists))
            }
            "findItemIndex" => {
                let path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let trimmed = path.trim().trim_matches('/');
                let items = self.collect_library_items(doc);
                let mut found: i64 = -1;
                for (i, item) in items.iter().enumerate() {
                    if let JsflValue::Item(ih) = item {
                        if ih.is_folder {
                            if ih.item_id == trimmed {
                                found = i as i64;
                                break;
                            }
                        } else if let Some(sym) = self.project.symbol(&ih.item_id) {
                            let full = if sym.folder.is_empty() {
                                sym.name.clone()
                            } else {
                                format!("{}/{}", sym.folder, sym.name)
                            };
                            if full == trimmed || sym.name == trimmed {
                                found = i as i64;
                                break;
                            }
                        }
                    }
                }
                Ok(JsflValue::Number(found as f64))
            }
            "newFolder" => {
                self.record_mutation(span)?;
                let folder_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "untitled folder 1".to_owned());
                let trimmed = folder_path.trim().trim_matches('/').to_owned();
                if !valid_library_folder(&trimmed) || trimmed.is_empty() {
                    return Err(self.err(
                        span,
                        format!("Invalid Library folder path '{folder_path}'."),
                    ));
                }
                self.project
                    .add_library_folder(&trimmed)
                    .map_err(|e| self.err(span, e))?;
                self.state.selected_library_items = vec![trimmed];
                Ok(JsflValue::Bool(true))
            }
            "addNewItem" => {
                self.record_mutation(span)?;
                let type_str = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "movie clip".to_owned())
                    .to_lowercase();
                let name_path = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| format!("Symbol {}", self.project.symbols.len()));
                let trimmed = name_path.trim().trim_matches('/');
                if type_str == "folder" {
                    return self.call_library_method(
                        doc,
                        "newFolder",
                        &[JsflValue::String(trimmed.to_owned())],
                        span,
                    );
                }
                let kind = match type_str.as_str() {
                    "movie clip" | "movieclip" => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => {
                        return Err(self.err(
                            span,
                            format!("Unsupported Library.addNewItem type '{type_str}'."),
                        ));
                    }
                };
                let (folder, leaf) = match trimmed.rsplit_once('/') {
                    Some((f, l)) => (f.to_owned(), l.to_owned()),
                    None => (String::new(), trimmed.to_owned()),
                };
                if leaf.is_empty() || leaf.chars().any(char::is_control) {
                    return Err(self.err(span, "Library item name cannot be empty."));
                }
                if !folder.is_empty() {
                    self.project
                        .add_library_folder(&folder)
                        .map_err(|e| self.err(span, e))?;
                }
                let sym_id = new_id("sym");
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: leaf,
                    folder,
                    kind,
                    timeline: crate::Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: "Layer 1".to_owned(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                ..Default::default()
                            }],
                            properties: Default::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.state.selected_library_items = vec![sym_id];
                Ok(JsflValue::Bool(true))
            }
            "selectItem" => {
                self.record_mutation(span)?;
                let name_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let replace = args.get(1).map(JsflValue::is_truthy).unwrap_or(true);
                let trimmed = name_path.trim().trim_matches('/');
                let resolved = self.find_library_symbol_by_path(trimmed).or_else(|| {
                    self.project
                        .library_folder_paths()
                        .into_iter()
                        .find(|f| f == trimmed)
                });
                let Some(item_id) = resolved else {
                    return Ok(JsflValue::Bool(false));
                };
                if replace {
                    self.state.selected_library_items = vec![item_id];
                } else if !self.state.selected_library_items.contains(&item_id) {
                    self.state.selected_library_items.push(item_id);
                }
                Ok(JsflValue::Bool(true))
            }
            "getSelectedItems" => {
                let folders: HashSet<String> =
                    self.project.library_folder_paths().into_iter().collect();
                let mut out = Vec::new();
                for id in &self.state.selected_library_items {
                    if folders.contains(id) {
                        out.push(JsflValue::Item(ItemHandle {
                            doc,
                            item_id: id.clone(),
                            is_folder: true,
                        }));
                    } else if self.project.symbol(id).is_some() {
                        out.push(JsflValue::Item(ItemHandle {
                            doc,
                            item_id: id.clone(),
                            is_folder: false,
                        }));
                    }
                }
                Ok(JsflValue::Array(out))
            }
            "renameItem" => {
                self.record_mutation(span)?;
                let new_name = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let leaf = new_name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&new_name)
                    .trim()
                    .to_owned();
                if leaf.is_empty() || leaf.chars().any(char::is_control) {
                    return Err(self.err(span, "Library.renameItem requires a valid name."));
                }
                let target_id = if let Some(path_arg) = args.get(1) {
                    let p = path_arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state
                        .selected_library_items
                        .first()
                        .cloned()
                        .or_else(|| {
                            let scenes: HashSet<Id> =
                                self.project.scene_ids().into_iter().collect();
                            self.project
                                .symbols
                                .iter()
                                .rev()
                                .find(|s| !scenes.contains(&s.id))
                                .map(|s| s.id.clone())
                        })
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                let sym = self.symbol_mut(&sym_id, span)?;
                sym.name = leaf;
                Ok(JsflValue::Bool(true))
            }
            "moveToFolder" => {
                self.record_mutation(span)?;
                let folder_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default()
                    .trim()
                    .trim_matches('/')
                    .to_owned();
                let item_to_move = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined");
                let target_ids: Vec<Id> = if let Some(p) = item_to_move {
                    if let Some(id) = self.find_library_symbol_by_path(&p) {
                        vec![id]
                    } else {
                        return Ok(JsflValue::Bool(false));
                    }
                } else {
                    self.state.selected_library_items.clone()
                };
                if target_ids.is_empty() {
                    return Ok(JsflValue::Bool(false));
                }
                for sym_id in target_ids {
                    if self.project.symbol(&sym_id).is_some() {
                        self.project
                            .move_library_symbol(&sym_id, &folder_path)
                            .map_err(|e| self.err(span, e))?;
                    }
                }
                Ok(JsflValue::Bool(true))
            }
            "duplicateItem" => {
                self.record_mutation(span)?;
                let target_id = if let Some(arg) = args.first() {
                    let p = arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state.selected_library_items.first().cloned()
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                let mut cloned = self.symbol_ref(&sym_id, span)?.clone();
                cloned.id = new_id("sym");
                cloned.name = format!("{} copy", cloned.name);
                cloned.linkage_id = None;
                cloned.class_name = None;
                for layer in &mut cloned.timeline.layers {
                    layer.id = new_id("layer");
                    for kf in &mut layer.keyframes {
                        for el in &mut kf.elements {
                            el.id = new_id("el");
                        }
                    }
                }
                let new_sym_id = cloned.id.clone();
                self.project.symbols.push(cloned);
                self.state.selected_library_items = vec![new_sym_id];
                Ok(JsflValue::Bool(true))
            }
            "deleteItem" => {
                self.record_mutation(span)?;
                let target_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined");
                if let Some(p) = &target_path {
                    let trimmed = p.trim().trim_matches('/');
                    if self
                        .project
                        .library_folder_paths()
                        .iter()
                        .any(|f| f == trimmed)
                    {
                        let prefix = format!("{trimmed}/");
                        self.project
                            .library_folders
                            .retain(|f| f != trimmed && !f.starts_with(&prefix));
                        let removed_ids: HashSet<Id> = self
                            .project
                            .symbols
                            .iter()
                            .filter(|s| s.folder == trimmed || s.folder.starts_with(&prefix))
                            .map(|s| s.id.clone())
                            .collect();
                        self.project
                            .symbols
                            .retain(|s| !removed_ids.contains(&s.id));
                        self.purge_deleted_symbol_instances(&removed_ids);
                        return Ok(JsflValue::Bool(true));
                    }
                }
                let target_id = match target_path {
                    Some(p) => self.find_library_symbol_by_path(&p),
                    None => self.state.selected_library_items.first().cloned(),
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                if self.project.is_scene(&sym_id) {
                    return Err(self.err(span, "Cannot delete a Scene symbol from Library."));
                }
                self.project.symbols.retain(|s| s.id != sym_id);
                let mut removed = HashSet::new();
                removed.insert(sym_id.clone());
                self.purge_deleted_symbol_instances(&removed);
                self.state.selected_library_items.retain(|id| id != &sym_id);
                if self.state.active_symbol == sym_id {
                    self.state.active_symbol = self.project.root.clone();
                }
                Ok(JsflValue::Bool(true))
            }
            "editItem" => {
                self.record_mutation(span)?;
                let Some(arg) = args.first() else {
                    self.state.active_symbol = self.project.root.clone();
                    self.state.current_layer = 0;
                    self.state.current_frame = 0;
                    self.state.selected_element_ids.clear();
                    return Ok(JsflValue::Bool(true));
                };
                let path = arg.to_display_string();
                let Some(sym_id) = self.find_library_symbol_by_path(&path) else {
                    return Ok(JsflValue::Bool(false));
                };
                self.state.active_symbol = sym_id;
                self.state.current_layer = 0;
                self.state.current_frame = 0;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Bool(true))
            }
            "addItemToDocument" => {
                let pos = args.first().cloned().unwrap_or(JsflValue::Undefined);
                let target_id = if let Some(arg) = args.get(1) {
                    let p = arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state.selected_library_items.first().cloned()
                };
                let Some(sym_id) = target_id else {
                    return Err(
                        self.err(span, "Library.addItemToDocument: target symbol not found.")
                    );
                };
                self.place_symbol_on_stage(doc, &pos, &sym_id, span)
            }
            "batchRename" => self.call_jsfl_batch_rename(args, span),
            _ => Err(self.err(
                span,
                format!("Unsupported Library method 'library.{method}'."),
            )),
        }
    }

    fn purge_deleted_symbol_instances(&mut self, removed_ids: &HashSet<Id>) {
        for sym in &mut self.project.symbols {
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    kf.elements.retain(|el| {
                        !matches!(&el.kind, ElementKind::Instance { symbol_id, .. } if removed_ids.contains(symbol_id))
                    });
                }
            }
        }
    }

    fn place_symbol_on_stage(
        &mut self,
        doc: DocHandle,
        pos: &JsflValue,
        sym_id: &str,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        if sym_id == self.state.active_symbol {
            return Err(self.err(span, "A symbol cannot be instantiated inside itself."));
        }
        let kind = self.symbol_ref(sym_id, span)?.kind;
        let (x, y) = if let JsflValue::Object(map) = pos {
            (
                map.get("x").and_then(JsflValue::as_number).unwrap_or(0.0),
                map.get("y").and_then(JsflValue::as_number).unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0)
        };
        let inst_id = new_id("inst");
        let inst = Element {
            id: inst_id.clone(),
            name: String::new(),
            transform: Transform {
                x,
                y,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: sym_id.to_owned(),
                playback: match kind {
                    SymbolKind::Graphic => InstancePlayback::Loop,
                    _ => InstancePlayback::Independent,
                },
            },
            ..Default::default()
        };
        let kf = self.ensure_active_keyframe_mut(span)?;
        let kf_frame = kf.frame;
        kf.elements.push(inst);
        self.state.selected_element_ids = vec![inst_id.clone()];
        let layer_id = self
            .symbol_ref(&self.state.active_symbol, span)?
            .timeline
            .layers[self.state.current_layer]
            .id
            .clone();
        Ok(JsflValue::Element(ElementHandle {
            doc,
            symbol_id: self.state.active_symbol.clone(),
            layer_id,
            keyframe: kf_frame,
            element_id: inst_id,
        }))
    }

    fn call_jsfl_batch_rename(
        &mut self,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        let Some(JsflValue::Object(map)) = args.first() else {
            return Err(self.err(span, "batchRename expects an options Object."));
        };
        let mut opts = BatchRenameOptions::default();
        if let Some(scope_val) = map.get("scope") {
            opts.scope = match scope_val.to_display_string().to_lowercase().as_str() {
                "library" | "library_only" | "libraryitemsonly" => {
                    BatchRenameScope::LibraryItemsOnly
                }
                "instances" | "stage" | "stageinstancesonly" => {
                    BatchRenameScope::StageInstancesOnly
                }
                _ => BatchRenameScope::LibraryAndInstances,
            };
        }
        if let Some(f) = map.get("filter").or_else(|| map.get("nameFilter")) {
            let s = f.to_display_string();
            if !s.is_empty() && s != "undefined" {
                opts.name_filter = Some(s);
            }
        }
        if let Some(find) = map.get("find") {
            let s = find.to_display_string();
            if !s.is_empty() && s != "undefined" {
                opts.find = Some(s);
            }
        }
        if let Some(rep) = map.get("replace") {
            opts.replace = rep.to_display_string();
        }
        if let Some(pre) = map.get("prefix") {
            opts.prefix = pre.to_display_string();
        }
        if let Some(suf) = map.get("suffix") {
            opts.suffix = suf.to_display_string();
        }
        if let Some(seq) = map
            .get("sequenceStart")
            .or_else(|| map.get("sequence_start"))
            .and_then(JsflValue::as_number)
        {
            opts.sequence_start = Some(seq.max(0.0) as u32);
        }
        if let Some(digits) = map
            .get("sequenceDigits")
            .or_else(|| map.get("sequence_digits"))
            .and_then(JsflValue::as_number)
        {
            opts.sequence_digits = (digits.max(1.0) as usize).min(8);
        }
        if let Some(sync) = map.get("syncInstances") {
            opts.sync_instance_names_to_symbol = sync.is_truthy();
        }
        let report = batch_rename_library_and_instances(&mut self.project, &opts)
            .map_err(|d| self.err(span, d.message))?;
        let mut out = BTreeMap::new();
        out.insert(
            "renamedSymbols".to_owned(),
            JsflValue::Number(report.renamed_symbols.len() as f64),
        );
        out.insert(
            "renamedInstances".to_owned(),
            JsflValue::Number(report.renamed_instances.len() as f64),
        );
        Ok(JsflValue::Object(out))
    }

    fn call_jsfl_change_color(
        &mut self,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        let opts = if let Some(JsflValue::Object(map)) = args.first() {
            let from = map
                .get("from")
                .or_else(|| map.get("fromColor"))
                .and_then(parse_color_value)
                .ok_or_else(|| {
                    self.err(
                        span,
                        "doc.changeColor({ from, to }) requires a valid 'from' colour.",
                    )
                })?;
            let to = map
                .get("to")
                .or_else(|| map.get("toColor"))
                .and_then(parse_color_value)
                .ok_or_else(|| {
                    self.err(
                        span,
                        "doc.changeColor({ from, to }) requires a valid 'to' colour.",
                    )
                })?;
            let mut o = ChangeColorOptions::new(from, to);
            if let Some(rgb_only) = map.get("matchRgbOnly") {
                o.match_rgb_only = rgb_only.is_truthy();
            }
            if let Some(preserve_alpha) = map.get("preserveAlpha") {
                o.preserve_slot_alpha = preserve_alpha.is_truthy();
            }
            if let Some(swatches) = map.get("includeSwatches") {
                o.include_custom_swatches = swatches.is_truthy();
            }
            o
        } else {
            let from = args.first().and_then(parse_color_value).ok_or_else(|| {
                self.err(
                    span,
                    "doc.changeColor(fromColor, toColor, [options]) expects a '#RRGGBB' string or 0xRRGGBB integer for fromColor.",
                )
            })?;
            let to = args.get(1).and_then(parse_color_value).ok_or_else(|| {
                self.err(
                    span,
                    "doc.changeColor(fromColor, toColor, [options]) expects a '#RRGGBB' string or 0xRRGGBB integer for toColor.",
                )
            })?;
            let mut o = ChangeColorOptions::new(from, to);
            if let Some(JsflValue::Object(map)) = args.get(2) {
                if let Some(rgb_only) = map.get("matchRgbOnly") {
                    o.match_rgb_only = rgb_only.is_truthy();
                }
                if let Some(preserve_alpha) = map.get("preserveAlpha") {
                    o.preserve_slot_alpha = preserve_alpha.is_truthy();
                }
                if let Some(swatches) = map.get("includeSwatches") {
                    o.include_custom_swatches = swatches.is_truthy();
                }
            }
            o
        };
        let report = change_project_color(&mut self.project, &opts)
            .map_err(|d| self.err(span, d.message))?;
        let mut out = BTreeMap::new();
        out.insert(
            "totalReplaced".to_owned(),
            JsflValue::Number(report.total_replaced() as f64),
        );
        out.insert(
            "replacedFills".to_owned(),
            JsflValue::Number(report.replaced_fills as f64),
        );
        out.insert(
            "replacedStrokes".to_owned(),
            JsflValue::Number(report.replaced_strokes as f64),
        );
        out.insert(
            "replacedGradientStops".to_owned(),
            JsflValue::Number(report.replaced_gradient_stops as f64),
        );
        out.insert(
            "replacedTexts".to_owned(),
            JsflValue::Number(report.replaced_texts as f64),
        );
        out.insert(
            "replacedSwatches".to_owned(),
            JsflValue::Number(report.replaced_swatches as f64),
        );
        out.insert(
            "affectedElements".to_owned(),
            JsflValue::Number(report.affected_elements.len() as f64),
        );
        Ok(JsflValue::Object(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_project() -> Project {
        Project {
            format_version: 6,
            name: "TestDoc".to_owned(),
            width: 800,
            height: 600,
            fps: 24,
            background: [255, 255, 255, 255],
            root: "root-sym".to_owned(),
            symbols: vec![Symbol {
                id: "root-sym".to_owned(),
                name: "Scene 1".to_owned(),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: crate::Timeline {
                    frame_count: 1,
                    layers: vec![Layer {
                        id: "layer-1".to_owned(),
                        name: "Layer 1".to_owned(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: Vec::new(),
                            ..Default::default()
                        }],
                        properties: Default::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            }],
            library_folders: Vec::new(),
            sources: Vec::new(),
            resources: Vec::new(),
            document_class: None,
            scenes: Vec::new(),
        }
    }

    #[test]
    fn native_and_jsfl_smart_batch_rename_across_library_and_nested_instances() {
        let mut project = test_project();
        project.symbols.push(Symbol {
            id: "sym-enemy-a".to_owned(),
            name: "old_enemy_grunt".to_owned(),
            folder: "Enemies".to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: crate::Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "enemy-layer".to_owned(),
                    name: "Body".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: Vec::new(),
                        ..Default::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        project.symbols.push(Symbol {
            id: "sym-enemy-b".to_owned(),
            name: "old_enemy_boss".to_owned(),
            folder: "Enemies".to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: crate::Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "boss-layer".to_owned(),
                    name: "Hold".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "nested-inst".to_owned(),
                            name: "old_enemy_grunt_minion".to_owned(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: "sym-enemy-a".to_owned(),
                                playback: InstancePlayback::Independent,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        project.library_folders.push("Enemies".to_owned());

        // Place instances across multiple keyframes on root timeline.
        project.symbols[0].timeline.frame_count = 5;
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "stage-inst-1".to_owned(),
                name: "old_enemy_grunt".to_owned(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-enemy-a".to_owned(),
                    playback: InstancePlayback::Independent,
                },
                ..Default::default()
            });
        project.symbols[0].timeline.layers[0]
            .keyframes
            .push(Keyframe {
                frame: 3,
                elements: vec![Element {
                    id: "stage-inst-2".to_owned(),
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: "sym-enemy-b".to_owned(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Default::default()
                }],
                ..Default::default()
            });

        let opts = BatchRenameOptions {
            scope: BatchRenameScope::LibraryAndInstances,
            target_symbol_ids: None,
            name_filter: Some("old_enemy_*".to_owned()),
            find: Some("old_enemy_".to_owned()),
            replace: "foe_".to_owned(),
            prefix: "ui_".to_owned(),
            suffix: "_v2_".to_owned(),
            sequence_start: Some(1),
            sequence_step: 1,
            sequence_digits: 3,
            sync_instance_names_to_symbol: true,
        };
        let report = batch_rename_library_and_instances(&mut project, &opts).unwrap();
        assert_eq!(report.renamed_symbols.len(), 2);
        assert_eq!(
            project.symbol("sym-enemy-a").unwrap().name,
            "ui_foe_grunt_v2_001"
        );
        assert_eq!(
            project.symbol("sym-enemy-b").unwrap().name,
            "ui_foe_boss_v2_002"
        );
        assert_eq!(report.renamed_instances.len(), 3);
        assert_eq!(
            project.symbols[0].timeline.layers[0].keyframes[0].elements[0].name,
            "ui_foe_grunt_v2_001_01"
        );
        assert_eq!(
            project.symbols[0].timeline.layers[0].keyframes[1].elements[0].name,
            "ui_foe_boss_v2_002_02"
        );
        assert_eq!(
            project.symbol("sym-enemy-b").unwrap().timeline.layers[0].keyframes[0].elements[0].name,
            "ui_foe_grunt_v2_001_minion"
        );
        assert!(validate(&project).is_empty());

        // Also test invoking batchRename from JSFL DOM and inspecting items/elements via JSFL.
        let sandbox = JsflSandbox::new();
        let state = JsflAuthoringState::for_project(&project);
        let script = r##"
            var doc = fl.getDocumentDOM();
            var res = doc.library.batchRename({
                scope: "library_and_instances",
                filter: "ui_foe_*",
                find: "ui_foe_",
                replace: "actor_",
                sequenceStart: 1,
                sequenceDigits: 2
            });
            fl.trace("symbols=" + res.renamedSymbols + " instances=" + res.renamedInstances);
        "##;
        let jsfl_res = sandbox
            .execute(&project, &state, "batch_rename.jsfl", script)
            .unwrap();
        assert_eq!(jsfl_res.output_lines, vec!["symbols=2 instances=3"]);
        assert_eq!(
            jsfl_res.project.symbol("sym-enemy-a").unwrap().name,
            "actor_grunt_v2_00101"
        );
    }

    #[test]
    fn jsfl_builds_transforms_groups_converts_artwork_and_constructs_tween_timeline() {
        let mut project = test_project();
        let mut state = JsflAuthoringState::for_project(&project);
        let sandbox = JsflSandbox::new();

        let script = r##"
            var doc = fl.getDocumentDOM();
            doc.width = 1280;
            doc.height = 720;
            doc.frameRate = 60;
            doc.backgroundColor = "#102030";

            // Build shapes and text
            doc.addNewRectangle({left: 10, top: 20, right: 110, bottom: 70}, 8);
            var sel = doc.selection;
            sel[0].setCustomFill({style: "solid", color: "#ff3366"});
            sel[0].setCustomStroke({style: "solid", color: "#112233", thickness: 3});

            doc.addNewOval({left: 120, top: 20, right: 180, bottom: 80});
            doc.selectAll();
            doc.group();
            doc.moveSelectionBy({x: 15, y: 25});
            doc.scaleSelection(2.0, 1.5);
            doc.rotateSelection(30);
            doc.unGroup();

            // Convert selection to MovieClip symbol in Library and organize folders
            var symItem = doc.convertToSymbol("movie clip", "HeroBadge", "center");
            symItem.linkageIdentifier = "hero_badge_id";
            doc.library.newFolder("UI/Badges");
            doc.library.moveToFolder("UI/Badges", "HeroBadge", true);
            doc.library.duplicateItem("UI/Badges/HeroBadge");

            // Add text element and update its string
            doc.addNewText({left: 40, top: 200, right: 240, bottom: 240}, "Initial");
            doc.selection[0].setTextString("Score: 100");
            doc.selection[0].name = "score_txt";

            // Timeline & Layer construction, tweening, labels, and frame scripts
            var tl = doc.getTimeline();
            var fxLayerIdx = tl.addNewLayer("FX Layer", "normal");
            tl.reorderLayer(fxLayerIdx, 0, true);
            tl.setSelectedLayers(1, true);
            tl.insertFrames(9, true, 0);
            tl.insertKeyframe(9);
            doc.selectAll();
            doc.moveSelectionBy({x: 200, y: 50});
            tl.createMotionTween(0, 9);

            var mainLayer = tl.layers[1];
            mainLayer.frames[0].name = "intro";
            mainLayer.frames[0].actionScript = "stop();";
            mainLayer.frames[0].tweenEasing = 50;

            an.outputPanel.clear();
            fl.trace(doc.width, doc.height, doc.frameRate, doc.backgroundColor);
            fl.trace(tl.layerCount, tl.frameCount, mainLayer.frames[0].tweenType);
            fl.trace(doc.library.itemExists("UI/Badges/HeroBadge"));
        "##;

        let res = sandbox
            .execute_in_place(&mut project, &mut state, "author_scene.jsfl", script)
            .unwrap();
        assert_eq!(res.undo_label, "Run JSFL: author_scene");
        assert!(state.dirty);
        assert_eq!(project.width, 1280);
        assert_eq!(project.height, 720);
        assert_eq!(project.fps, 60);
        assert_eq!(project.background, [0x10, 0x20, 0x30, 0xFF]);
        assert_eq!(
            res.output_lines,
            vec!["1280 720 60 #102030", "2 10 motion", "true"]
        );
        let root_sym = project.symbol("root-sym").unwrap();
        assert_eq!(root_sym.timeline.layers.len(), 2);
        assert_eq!(root_sym.timeline.layers[0].name, "FX Layer");
        assert_eq!(root_sym.timeline.frame_count, 10);
        assert_eq!(root_sym.timeline.layers[1].keyframes[0].label, "intro");
        assert_eq!(
            root_sym.timeline.layers[1].keyframes[0].tween,
            Some(Easing::Flash { acceleration: 50.0 })
        );
        assert_eq!(root_sym.timeline.scripts[0].script.source, "stop();");
        let badge = project
            .symbols
            .iter()
            .find(|s| s.name == "HeroBadge")
            .unwrap();
        assert_eq!(badge.folder, "UI/Badges");
        assert_eq!(badge.linkage_id.as_deref(), Some("hero_badge_id"));
        assert!(project.symbols.iter().any(|s| s.name == "HeroBadge copy"));
        assert!(validate(&project).is_empty());
    }

    #[test]
    fn atomic_rollback_on_throw_timeout_stale_handle_and_invalid_project() {
        let original_project = test_project();
        let original_state = JsflAuthoringState::for_project(&original_project);

        // 1. Uncaught throw after mutating document and layers
        let mut p1 = original_project.clone();
        let mut s1 = original_state.clone();
        let sandbox = JsflSandbox::new();
        let err_throw = sandbox
            .execute_in_place(
                &mut p1,
                &mut s1,
                "fail_throw.jsfl",
                "var doc = fl.getDocumentDOM();\ndoc.width = 1024;\ndoc.getTimeline().addNewLayer('Temp');\nthrow 'boom';\n",
            )
            .unwrap_err();
        assert_eq!(err_throw.file, "fail_throw.jsfl");
        assert_eq!(err_throw.line, 4);
        assert!(err_throw.message.contains("boom"));
        assert_eq!(p1, original_project);
        assert_eq!(s1, original_state);

        // 2. Step/fuel timeout cannot be swallowed by try/catch and rolls back
        let mut p2 = original_project.clone();
        let mut s2 = original_state.clone();
        let tight_sandbox = JsflSandbox::new().with_limits(JsflLimits {
            max_steps: 60,
            ..Default::default()
        });
        let err_timeout = tight_sandbox
            .execute_in_place(
                &mut p2,
                &mut s2,
                "infinite.jsfl",
                "fl.getDocumentDOM().width = 999;\ntry { while (true) {} } catch (e) {}\n",
            )
            .unwrap_err();
        assert_eq!(err_timeout.file, "infinite.jsfl");
        assert_eq!(err_timeout.line, 2);
        assert!(err_timeout.message.contains("exceeded step limit"));
        assert_eq!(p2, original_project);
        assert_eq!(s2, original_state);

        // 3. Stale handle access after deleting layer or element rolls back
        let mut p3 = original_project.clone();
        let mut s3 = original_state.clone();
        let err_stale = sandbox
            .execute_in_place(
                &mut p3,
                &mut s3,
                "stale.jsfl",
                "var tl = fl.getDocumentDOM().getTimeline();\nvar idx = tl.addNewLayer('Temp');\nvar staleLayer = tl.layers[idx];\ntl.deleteLayer(idx);\nstaleLayer.name = 'Ghost';\n",
            )
            .unwrap_err();
        assert_eq!(err_stale.file, "stale.jsfl");
        assert_eq!(err_stale.line, 5);
        assert!(err_stale.message.contains("Stale LayerHandle"));
        assert_eq!(p3, original_project);
        assert_eq!(s3, original_state);

        // 4. Invalid project property rolls back
        let mut p4 = original_project.clone();
        let mut s4 = original_state.clone();
        let err_invalid = sandbox
            .execute_in_place(
                &mut p4,
                &mut s4,
                "invalid_doc.jsfl",
                "var doc = fl.getDocumentDOM();\ndoc.frameRate = 120;\ndoc.width = 999999;\n",
            )
            .unwrap_err();
        assert_eq!(err_invalid.file, "invalid_doc.jsfl");
        assert_eq!(err_invalid.line, 3);
        assert_eq!(p4, original_project);
        assert_eq!(s4, original_state);

        // 5. Post-execution project.validate() failure (duplicate linkageIdentifier) rolls back
        let mut p5 = original_project.clone();
        let mut s5 = original_state.clone();
        let err_val = sandbox
            .execute_in_place(
                &mut p5,
                &mut s5,
                "dup_linkage.jsfl",
                "var lib = fl.getDocumentDOM().library;\nlib.addNewItem('movie clip', 'SymA');\nlib.addNewItem('movie clip', 'SymB');\nlib.items[0].linkageIdentifier = 'dup_id';\nlib.items[1].linkageIdentifier = 'dup_id';\n",
            )
            .unwrap_err();
        assert_eq!(err_val.file, "dup_linkage.jsfl");
        assert!(err_val.message.contains("failed validation"));
        assert_eq!(p5, original_project);
        assert_eq!(s5, original_state);
    }

    #[test]
    fn flfile_capabilities_traversal_symlink_escape_output_cap_and_run_script_guards() {
        let project = test_project();
        let state = JsflAuthoringState::for_project(&project);

        // 1. Default sandbox denies FLfile read/write/delete
        let default_sb = JsflSandbox::new();
        let deny_err = default_sb
            .execute(
                &project,
                &state,
                "deny.jsfl",
                "FLfile.exists('file:///tmp/test.txt');",
            )
            .unwrap_err();
        assert!(deny_err.message.contains("denied"));

        // 2. Scoped temp directory with read-only first, then read+write+delete
        let tmp_base =
            std::env::temp_dir().join(format!("neoflash_jsfl_test_{}", std::process::id()));
        let allowed_dir = tmp_base.join("allowed");
        let outside_dir = tmp_base.join("outside");
        let _ = std::fs::remove_dir_all(&tmp_base);
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();
        let secret_file = outside_dir.join("secret.txt");
        std::fs::write(&secret_file, "top-secret").unwrap();

        #[cfg(unix)]
        {
            let symlink_path = allowed_dir.join("escape_link.txt");
            let _ = std::os::unix::fs::symlink(&secret_file, &symlink_path);
        }

        let ro_sb = JsflSandbox::new().with_flfile_capabilities(FlFileCapabilities {
            allowed_root: Some(allowed_dir.clone()),
            allow_read: true,
            allow_write: false,
            allow_delete: false,
        });

        // Write and delete rejected in read-only capability mode
        assert!(
            ro_sb
                .execute(
                    &project,
                    &state,
                    "ro_write.jsfl",
                    "FLfile.write('file:///notes.txt', 'hello');",
                )
                .is_err()
        );
        assert!(
            ro_sb
                .execute(
                    &project,
                    &state,
                    "ro_del.jsfl",
                    "FLfile.remove('file:///notes.txt');",
                )
                .is_err()
        );

        // Path traversal rejected
        let trav_err = ro_sb
            .execute(
                &project,
                &state,
                "traversal.jsfl",
                "FLfile.read('file:///../outside/secret.txt');",
            )
            .unwrap_err();
        assert!(trav_err.message.contains("traversal"));

        // Symlink escape rejected
        #[cfg(unix)]
        {
            let sym_err = ro_sb
                .execute(
                    &project,
                    &state,
                    "symlink.jsfl",
                    "FLfile.read('file:///escape_link.txt');",
                )
                .unwrap_err();
            assert!(sym_err.message.contains("escapes allowed_root"));
        }

        // Full read/write/delete within allowed_root succeeds
        let rw_sb = JsflSandbox::new().with_flfile_capabilities(FlFileCapabilities {
            allowed_root: Some(allowed_dir.clone()),
            allow_read: true,
            allow_write: true,
            allow_delete: true,
        });
        let file_res = rw_sb
            .execute(
                &project,
                &state,
                "file_ops.jsfl",
                r#"
                FLfile.createFolder("file:///sub");
                FLfile.write("file:///sub/data.txt", "alpha");
                FLfile.write("file:///sub/data.txt", "-beta", "append");
                var existsBefore = FLfile.exists("file:///sub/data.txt");
                var content = FLfile.read("file:///sub/data.txt");
                var listing = FLfile.listFolder("file:///sub", true);
                FLfile.remove("file:///sub/data.txt");
                var existsAfter = FLfile.exists("file:///sub/data.txt");
                fl.trace(existsBefore, content, listing.join(","), existsAfter);
            "#,
            )
            .unwrap();
        assert_eq!(
            file_res.output_lines,
            vec!["true alpha-beta data.txt false"]
        );
        let _ = std::fs::remove_dir_all(&tmp_base);

        // 3. Output flooding cap
        let cap_sb = JsflSandbox::new().with_limits(JsflLimits {
            max_output_lines: 5,
            ..Default::default()
        });
        let flood_err = cap_sb
            .execute(
                &project,
                &state,
                "flood.jsfl",
                "for (var i = 0; i < 20; i++) { fl.trace(i); }",
            )
            .unwrap_err();
        assert!(flood_err.message.contains("maximum trace lines"));

        // 4. Nested fl.runScript recursion guard and unsupported XMLUI diagnostic
        let mut rec_sb = JsflSandbox::new().with_limits(JsflLimits {
            max_script_recursion: 3,
            ..Default::default()
        });
        rec_sb.register_virtual_script(
            "file:///Commands/self.jsfl",
            "fl.runScript('file:///Commands/self.jsfl');",
        );
        let rec_err = rec_sb
            .execute(
                &project,
                &state,
                "entry.jsfl",
                "fl.runScript('file:///Commands/self.jsfl');",
            )
            .unwrap_err();
        assert!(rec_err.message.contains("recursion limit"));

        let xmlui_err = default_sb
            .execute(&project, &state, "panel.jsfl", "var x = fl.xmlui;")
            .unwrap_err();
        assert!(
            xmlui_err
                .message
                .contains("Unsupported JSFL property 'fl.xmlui'")
        );
    }

    #[test]
    fn jsfl_change_color_script_replaces_color_across_entire_project_dom() {
        let mut project = test_project();
        let target_red = [230, 57, 70, 255];
        let replacement_blue = [29, 53, 87, 255];
        let untouched_green = [42, 157, 143, 255];

        // 1. Root scene elements: solid rectangle, gradient painted path, and text
        project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "rect-1".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 120.0,
                    height: 80.0,
                    fill: target_red,
                    stroke: untouched_green,
                    stroke_width: 2.0,
                },
                ..Default::default()
            },
            Element {
                id: "grad-1".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::PaintedPath {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 50.0, y: 0.0 },
                        PathCommand::LineTo { x: 50.0, y: 50.0 },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: target_red,
                    stroke: target_red,
                    fill_gradient: Some(crate::GradientPaint {
                        kind: crate::GradientKind::Linear,
                        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                        stops: vec![
                            crate::GradientStop {
                                ratio: 0,
                                color: target_red,
                            },
                            crate::GradientStop {
                                ratio: 255,
                                color: untouched_green,
                            },
                        ],
                        ..Default::default()
                    }),
                    stroke_gradient: None,
                    stroke_width: 3.0,
                },
                ..Default::default()
            },
            Element {
                id: "text-1".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Text {
                    text: "Heading".to_owned(),
                    font_family: "sans-serif".to_owned(),
                    font_size: 24.0,
                    color: target_red,
                },
                ..Default::default()
            },
        ];

        // 2. Nested Library symbol with a stroke and another keyframe
        project.symbols.push(Symbol {
            id: "sym-badge".to_owned(),
            name: "Badge".to_owned(),
            folder: "UI".to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: crate::Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "badge-layer".to_owned(),
                    name: "Icon".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "badge-ellipse".to_owned(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Ellipse {
                                    width: 40.0,
                                    height: 40.0,
                                    fill: untouched_green,
                                    stroke: target_red,
                                    stroke_width: 4.0,
                                },
                                ..Default::default()
                            }],
                            ..Default::default()
                        },
                        Keyframe {
                            frame: 2,
                            elements: vec![Element {
                                id: "badge-rect-f2".to_owned(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Rectangle {
                                    width: 40.0,
                                    height: 40.0,
                                    fill: target_red,
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Default::default()
                            }],
                            ..Default::default()
                        },
                    ],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        project.library_folders.push("UI".to_owned());

        let state = JsflAuthoringState::for_project(&project);
        let res = run_change_color_jsfl(&project, &state, target_red, replacement_blue).unwrap();

        // 6 occurrences replaced:
        // rect-1 fill (1), grad-1 stop 0 (1), grad-1 stroke (1), text-1 (1),
        // badge-ellipse stroke (1), badge-rect-f2 fill (1)
        assert_eq!(res.return_value, JsflValue::Number(6.0));
        assert_eq!(
            res.output_lines,
            vec!["Changed 6 colour occurrence(s) from #e63946 to #1d3557"]
        );

        let updated = &res.project;
        let root_els = &updated.symbols[0].timeline.layers[0].keyframes[0].elements;
        match &root_els[0].kind {
            ElementKind::Rectangle { fill, stroke, .. } => {
                assert_eq!(*fill, replacement_blue);
                assert_eq!(*stroke, untouched_green);
            }
            other => panic!("unexpected element kind: {other:?}"),
        }
        match &root_els[1].kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(grad),
                stroke,
                ..
            } => {
                assert_eq!(grad.stops[0].color, replacement_blue);
                assert_eq!(grad.stops[1].color, untouched_green);
                assert_eq!(*stroke, replacement_blue);
            }
            other => panic!("unexpected element kind: {other:?}"),
        }
        match &root_els[2].kind {
            ElementKind::Text { color, .. } => assert_eq!(*color, replacement_blue),
            other => panic!("unexpected element kind: {other:?}"),
        }

        let badge = updated.symbol("sym-badge").unwrap();
        match &badge.timeline.layers[0].keyframes[0].elements[0].kind {
            ElementKind::Ellipse { fill, stroke, .. } => {
                assert_eq!(*fill, untouched_green);
                assert_eq!(*stroke, replacement_blue);
            }
            other => panic!("unexpected element kind: {other:?}"),
        }
        match &badge.timeline.layers[0].keyframes[1].elements[0].kind {
            ElementKind::Rectangle { fill, .. } => assert_eq!(*fill, replacement_blue),
            other => panic!("unexpected element kind: {other:?}"),
        }
    }

    #[test]
    fn native_and_jsfl_doc_change_color_and_project_color_inventory() {
        let mut project = test_project();
        let c_primary = [244, 162, 97, 255];
        let c_accent = [231, 111, 81, 255];
        let c_new = [38, 70, 83, 255];

        project.set_custom_swatches(&[c_primary, c_accent]);
        project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "shape-a".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 100.0,
                    height: 60.0,
                    fill: c_primary,
                    stroke: c_accent,
                    stroke_width: 2.0,
                },
                ..Default::default()
            },
            Element {
                id: "text-a".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Text {
                    text: "Label".to_owned(),
                    font_family: "sans-serif".to_owned(),
                    font_size: 18.0,
                    color: c_primary,
                },
                ..Default::default()
            },
        ];

        let inventory = collect_project_colors(&project);
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory[0].color, c_primary);
        assert_eq!(inventory[0].fill_count, 1);
        assert_eq!(inventory[0].text_count, 1);
        assert!(inventory[0].in_custom_swatches);
        assert_eq!(inventory[1].color, c_accent);
        assert_eq!(inventory[1].stroke_count, 1);

        // Execute via JSFL `doc.getProjectColors()` and `doc.changeColor(...)`
        let state = JsflAuthoringState::for_project(&project);
        let sb = JsflSandbox::new();
        let jsfl_res = sb
            .execute(
                &project,
                &state,
                "doc_change_color.jsfl",
                r##"
                var doc = fl.getDocumentDOM();
                var before = doc.getProjectColors();
                var rep = doc.changeColor("#f4a261", "#264653");
                var after = doc.getProjectColors();
                fl.trace("before=" + before.length, "replaced=" + rep.totalReplaced, "after=" + after[0].color);
                rep.totalReplaced;
            "##,
            )
            .unwrap();
        assert_eq!(
            jsfl_res.output_lines,
            vec!["before=2 replaced=3 after=#264653"]
        );
        assert_eq!(jsfl_res.return_value, JsflValue::Number(3.0));
        assert_eq!(jsfl_res.project.custom_swatches(), vec![c_new, c_accent]);
    }
}
