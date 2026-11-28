use crate::{BlendMode, Color, Element, ElementKind, Id, Project, new_id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum steps allowed in a single saved/replayed authoring command.
pub const MAX_COMMAND_STEPS: usize = 256;

/// Bounded, deterministic, side-effect-free authoring command step that can be recorded from
/// History, copied as JSON, saved as a named command, and replayed on another selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AuthoringCommandStep {
    Translate { dx: f64, dy: f64 },
    Scale { sx: f64, sy: f64 },
    Rotate { degrees: f64 },
    Skew { degrees: f64 },
    SetAlpha { alpha: f64 },
    SetFillColor { color: Color },
    SetStrokeColor { color: Color },
    SetStrokeWidth { width: f64 },
    SetVisible { visible: bool },
    SetBlendMode { blend_mode: BlendMode },
    RenameSelection { name: String },
    DuplicateSelection { offset_x: f64, offset_y: f64 },
    DeleteSelection,
    SwapSymbol { target_symbol_id: Id },
}

/// Named sequence of replayable `AuthoringCommandStep`s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedCommand {
    pub name: String,
    pub steps: Vec<AuthoringCommandStep>,
}

impl SavedCommand {
    pub fn validate(&self) -> Result<(), String> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
            return Err("Command name must be 1..=128 non-control characters.".into());
        }
        if self.steps.is_empty() || self.steps.len() > MAX_COMMAND_STEPS {
            return Err(format!(
                "Command must contain 1..={MAX_COMMAND_STEPS} steps."
            ));
        }
        for step in &self.steps {
            match step {
                AuthoringCommandStep::Translate { dx, dy } => {
                    if !dx.is_finite() || !dy.is_finite() || dx.abs() > 1e6 || dy.abs() > 1e6 {
                        return Err("Translate delta must be finite and within ±1e6.".into());
                    }
                }
                AuthoringCommandStep::Scale { sx, sy } => {
                    if !sx.is_finite()
                        || !sy.is_finite()
                        || sx.abs() < 1e-4
                        || sy.abs() < 1e-4
                        || sx.abs() > 1e4
                        || sy.abs() > 1e4
                    {
                        return Err(
                            "Scale factors must be finite and within [1e-4, 1e4] in magnitude."
                                .into(),
                        );
                    }
                }
                AuthoringCommandStep::Rotate { degrees }
                | AuthoringCommandStep::Skew { degrees } => {
                    if !degrees.is_finite() || degrees.abs() > 36000.0 {
                        return Err("Rotation/skew degrees must be finite.".into());
                    }
                }
                AuthoringCommandStep::SetAlpha { alpha } => {
                    if !alpha.is_finite() || !(0.0..=1.0).contains(alpha) {
                        return Err("Alpha must be in 0.0..=1.0.".into());
                    }
                }
                AuthoringCommandStep::SetStrokeWidth { width } => {
                    if !width.is_finite() || !(0.0..=10000.0).contains(width) {
                        return Err("Stroke width must be in 0.0..=10000.0.".into());
                    }
                }
                AuthoringCommandStep::DuplicateSelection { offset_x, offset_y } => {
                    if !offset_x.is_finite()
                        || !offset_y.is_finite()
                        || offset_x.abs() > 1e6
                        || offset_y.abs() > 1e6
                    {
                        return Err("Duplicate offset must be finite and within ±1e6.".into());
                    }
                }
                AuthoringCommandStep::RenameSelection { name } => {
                    if name.len() > 256 || name.chars().any(char::is_control) {
                        return Err("Element name exceeds 256 bytes or contains controls.".into());
                    }
                }
                AuthoringCommandStep::SwapSymbol { target_symbol_id } => {
                    if target_symbol_id.trim().is_empty() {
                        return Err("SwapSymbol target_symbol_id cannot be empty.".into());
                    }
                }
                AuthoringCommandStep::SetFillColor { .. }
                | AuthoringCommandStep::SetStrokeColor { .. }
                | AuthoringCommandStep::SetVisible { .. }
                | AuthoringCommandStep::SetBlendMode { .. }
                | AuthoringCommandStep::DeleteSelection => {}
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        if raw.len() > 256 * 1024 {
            return Err("Command JSON exceeds 256 KiB.".into());
        }
        let cmd: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        cmd.validate()?;
        Ok(cmd)
    }
}

/// Capability permission required by a sandboxed declarative extension (`T-167`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPermission {
    StageTransform,
    StyleMutation,
    SymbolMutation,
    StructureMutation,
}

impl AuthoringCommandStep {
    /// Required capability permission for this step inside an `AuthoringExtensionManifest`.
    pub fn required_permission(&self) -> ExtensionPermission {
        match self {
            Self::Translate { .. }
            | Self::Scale { .. }
            | Self::Rotate { .. }
            | Self::Skew { .. } => ExtensionPermission::StageTransform,
            Self::SetAlpha { .. }
            | Self::SetFillColor { .. }
            | Self::SetStrokeColor { .. }
            | Self::SetStrokeWidth { .. }
            | Self::SetVisible { .. }
            | Self::SetBlendMode { .. } => ExtensionPermission::StyleMutation,
            Self::SwapSymbol { .. } => ExtensionPermission::SymbolMutation,
            Self::RenameSelection { .. }
            | Self::DuplicateSelection { .. }
            | Self::DeleteSelection => ExtensionPermission::StructureMutation,
        }
    }
}

/// Safe, capability-bounded declarative extension package (`T-167`, modern `.nfext.json` replacement
/// for legacy JSFL packages without arbitrary filesystem or network access).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoringExtensionManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub permissions: Vec<ExtensionPermission>,
    pub commands: Vec<SavedCommand>,
}

impl AuthoringExtensionManifest {
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;
    pub const MAX_EXTENSION_COMMANDS: usize = 64;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != Self::CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported extension schema_version {} (expected {}).",
                self.schema_version,
                Self::CURRENT_SCHEMA_VERSION
            ));
        }
        let id = self.id.trim();
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(
                "Extension id must be 1..=64 ASCII alphanumeric, '.', '-' or '_' characters."
                    .into(),
            );
        }
        let name = self.name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err("Extension name must be 1..=128 non-control characters.".into());
        }
        if self.commands.is_empty() || self.commands.len() > Self::MAX_EXTENSION_COMMANDS {
            return Err(format!(
                "Extension must declare 1..={} commands.",
                Self::MAX_EXTENSION_COMMANDS
            ));
        }
        let granted: HashSet<ExtensionPermission> = self.permissions.iter().copied().collect();
        let mut seen_names = HashSet::new();
        for cmd in &self.commands {
            cmd.validate()?;
            if !seen_names.insert(cmd.name.trim().to_lowercase()) {
                return Err(format!(
                    "Duplicate command name '{}' in extension.",
                    cmd.name
                ));
            }
            for step in &cmd.steps {
                let req = step.required_permission();
                if !granted.contains(&req) {
                    return Err(format!(
                        "Command '{}' uses step requiring permission {:?}, which is not declared in manifest permissions.",
                        cmd.name, req
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        if raw.len() > 512 * 1024 {
            return Err("Extension manifest JSON exceeds 512 KiB.".into());
        }
        let manifest: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        manifest.validate()?;
        Ok(manifest)
    }
}

/// Apply a sequence of `AuthoringCommandStep`s to `elements` for the IDs in `selected_ids`.
pub fn apply_command_steps_to_elements(
    elements: &mut Vec<Element>,
    selected_ids: &mut Vec<Id>,
    steps: &[AuthoringCommandStep],
    valid_symbol_ids: &HashSet<&str>,
) -> Result<usize, String> {
    if steps.is_empty() || steps.len() > MAX_COMMAND_STEPS {
        return Err(format!(
            "Command replay requires 1..={MAX_COMMAND_STEPS} steps."
        ));
    }
    if selected_ids.is_empty() {
        return Err("Select at least one stage element to replay command steps.".into());
    }
    let mut applied = 0usize;
    for step in steps {
        match step {
            AuthoringCommandStep::Translate { dx, dy } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.x += *dx;
                    el.transform.y += *dy;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Scale { sx, sy } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.scale_x *= *sx;
                    el.transform.scale_y *= *sy;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Rotate { degrees } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.rotation += *degrees;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Skew { degrees } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.skew += *degrees;
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetAlpha { alpha } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.alpha = alpha.clamp(0.0, 1.0);
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetFillColor { color } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { fill, .. }
                        | ElementKind::Ellipse { fill, .. }
                        | ElementKind::Path { fill, .. }
                        | ElementKind::PaintedPath { fill, .. } => {
                            *fill = *color;
                            applied += 1;
                        }
                        ElementKind::Text { color: tc, .. } => {
                            *tc = *color;
                            applied += 1;
                        }
                        ElementKind::TextBox { format, .. } => {
                            format.color = (u32::from(color[0]) << 16)
                                | (u32::from(color[1]) << 8)
                                | u32::from(color[2]);
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetStrokeColor { color } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { stroke, .. }
                        | ElementKind::Ellipse { stroke, .. }
                        | ElementKind::Path { stroke, .. }
                        | ElementKind::PaintedPath { stroke, .. } => {
                            *stroke = *color;
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetStrokeWidth { width } => {
                let w = width.clamp(0.0, 10000.0);
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { stroke_width, .. }
                        | ElementKind::Ellipse { stroke_width, .. }
                        | ElementKind::Path { stroke_width, .. }
                        | ElementKind::PaintedPath { stroke_width, .. } => {
                            *stroke_width = w;
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetVisible { visible } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.visible = *visible;
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetBlendMode { blend_mode } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.blend_mode = *blend_mode;
                    applied += 1;
                }
            }
            AuthoringCommandStep::RenameSelection { name } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.name = name.clone();
                    applied += 1;
                }
            }
            AuthoringCommandStep::DuplicateSelection { offset_x, offset_y } => {
                let mut clones = Vec::new();
                let mut new_selected = Vec::new();
                for el in elements.iter().filter(|e| selected_ids.contains(&e.id)) {
                    let mut dup = el.clone();
                    dup.id = new_id("element");
                    dup.transform.x += *offset_x;
                    dup.transform.y += *offset_y;
                    new_selected.push(dup.id.clone());
                    clones.push(dup);
                    applied += 1;
                }
                if !clones.is_empty() {
                    elements.extend(clones);
                    *selected_ids = new_selected;
                }
            }
            AuthoringCommandStep::DeleteSelection => {
                let before = elements.len();
                elements.retain(|e| !selected_ids.contains(&e.id));
                let removed = before.saturating_sub(elements.len());
                applied += removed;
                selected_ids.clear();
            }
            AuthoringCommandStep::SwapSymbol { target_symbol_id } => {
                if !valid_symbol_ids.contains(target_symbol_id.as_str()) {
                    return Err(format!("Target symbol {target_symbol_id} does not exist."));
                }
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    if let ElementKind::Instance { symbol_id, .. } = &mut el.kind {
                        *symbol_id = target_symbol_id.clone();
                        applied += 1;
                    }
                }
            }
        }
    }
    Ok(applied)
}

/// Process-wide source of history revisions, so a replaced `History` never repeats one.
static REVISIONS: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}

/// Deduplicate `Arc<str>` resource buffers against `reference` so history snapshots
/// share identical resource allocations even after reload or copy-on-write edits.
fn share_resources_slice(reference: &[crate::ResourceUnit], target: &mut [crate::ResourceUnit]) {
    for unit in target.iter_mut() {
        if let Some(existing) = reference.iter().find(|r| r.id == unit.id)
            && !std::sync::Arc::ptr_eq(&existing.data_base64, &unit.data_base64)
            && *existing.data_base64 == *unit.data_base64
        {
            unit.data_base64 = existing.data_base64.clone();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ProjectHeader {
    format_version: u32,
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    background: Color,
    root: Id,
    library_folders: Vec<String>,
    sources: Vec<crate::SourceUnit>,
    document_class: Option<String>,
    scenes: Vec<Id>,
    script_language: Option<crate::ScriptLanguage>,
    also_compile_compatible_scripts: bool,
}

impl ProjectHeader {
    fn from_project(p: &Project) -> Self {
        Self {
            format_version: p.format_version,
            name: p.name.clone(),
            width: p.width,
            height: p.height,
            fps: p.fps,
            background: p.background,
            root: p.root.clone(),
            library_folders: p.library_folders.clone(),
            sources: p.sources.clone(),
            document_class: p.document_class.clone(),
            scenes: p.scenes.clone(),
            script_language: p.script_language,
            also_compile_compatible_scripts: p.also_compile_compatible_scripts,
        }
    }

    fn matches_project(&self, p: &Project) -> bool {
        self.format_version == p.format_version
            && self.name == p.name
            && self.width == p.width
            && self.height == p.height
            && self.fps == p.fps
            && self.background == p.background
            && self.root == p.root
            && self.library_folders == p.library_folders
            && self.sources == p.sources
            && self.document_class == p.document_class
            && self.scenes == p.scenes
            && self.script_language == p.script_language
            && self.also_compile_compatible_scripts == p.also_compile_compatible_scripts
    }

    fn apply_to_project(&self, p: &mut Project) -> Self {
        let prev = Self::from_project(p);
        p.format_version = self.format_version;
        p.name.clone_from(&self.name);
        p.width = self.width;
        p.height = self.height;
        p.fps = self.fps;
        p.background = self.background;
        p.root.clone_from(&self.root);
        p.library_folders.clone_from(&self.library_folders);
        p.sources.clone_from(&self.sources);
        p.document_class.clone_from(&self.document_class);
        p.scenes.clone_from(&self.scenes);
        p.script_language = self.script_language;
        p.also_compile_compatible_scripts = self.also_compile_compatible_scripts;
        prev
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SymbolMeta {
    id: Id,
    name: String,
    folder: String,
    kind: crate::SymbolKind,
    class_name: Option<String>,
    linkage_id: Option<String>,
    frame_count: u32,
    scripts: Vec<crate::FrameScript>,
    guides: Vec<crate::Guide>,
}

impl SymbolMeta {
    fn from_symbol(s: &crate::Symbol) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            folder: s.folder.clone(),
            kind: s.kind,
            class_name: s.class_name.clone(),
            linkage_id: s.linkage_id.clone(),
            frame_count: s.timeline.frame_count,
            scripts: s.timeline.scripts.clone(),
            guides: s.timeline.guides.clone(),
        }
    }

    fn matches_symbol(&self, s: &crate::Symbol) -> bool {
        self.id == s.id
            && self.name == s.name
            && self.folder == s.folder
            && self.kind == s.kind
            && self.class_name == s.class_name
            && self.linkage_id == s.linkage_id
            && self.frame_count == s.timeline.frame_count
            && self.scripts == s.timeline.scripts
            && self.guides == s.timeline.guides
    }

    fn apply_to_symbol(&self, s: &mut crate::Symbol) -> Self {
        let prev = Self::from_symbol(s);
        s.id.clone_from(&self.id);
        s.name.clone_from(&self.name);
        s.folder.clone_from(&self.folder);
        s.kind = self.kind;
        s.class_name.clone_from(&self.class_name);
        s.linkage_id.clone_from(&self.linkage_id);
        s.timeline.frame_count = self.frame_count;
        s.timeline.scripts.clone_from(&self.scripts);
        s.timeline.guides.clone_from(&self.guides);
        prev
    }
}

#[derive(Clone, Debug, PartialEq)]
struct LayerMeta {
    id: Id,
    name: String,
    visible: bool,
    locked: bool,
    properties: crate::LayerProperties,
}

impl LayerMeta {
    fn from_layer(l: &crate::Layer) -> Self {
        Self {
            id: l.id.clone(),
            name: l.name.clone(),
            visible: l.visible,
            locked: l.locked,
            properties: l.properties.clone(),
        }
    }

    fn matches_layer(&self, l: &crate::Layer) -> bool {
        self.id == l.id
            && self.name == l.name
            && self.visible == l.visible
            && self.locked == l.locked
            && self.properties == l.properties
    }

    fn apply_to_layer(&self, l: &mut crate::Layer) -> Self {
        let prev = Self::from_layer(l);
        l.id.clone_from(&self.id);
        l.name.clone_from(&self.name);
        l.visible = self.visible;
        l.locked = self.locked;
        l.properties.clone_from(&self.properties);
        prev
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SharedSymbolSpine {
    meta: SymbolMeta,
    layers: Vec<std::sync::Arc<crate::Layer>>,
}

impl SharedSymbolSpine {
    fn from_symbol(sym: &crate::Symbol, cached: Option<&Self>) -> Self {
        let meta = SymbolMeta::from_symbol(sym);
        let mut layers = Vec::with_capacity(sym.timeline.layers.len());
        for (idx, layer) in sym.timeline.layers.iter().enumerate() {
            if let Some(prev_spine) = cached {
                if let Some(prev_arc) = prev_spine.layers.get(idx)
                    && prev_arc.as_ref() == layer
                {
                    layers.push(std::sync::Arc::clone(prev_arc));
                    continue;
                }
                if let Some(prev_arc) = prev_spine
                    .layers
                    .iter()
                    .find(|l| l.id == layer.id && l.as_ref() == layer)
                {
                    layers.push(std::sync::Arc::clone(prev_arc));
                    continue;
                }
            }
            layers.push(std::sync::Arc::new(layer.clone()));
        }
        Self { meta, layers }
    }

    fn matches_symbol(&self, sym: &crate::Symbol) -> bool {
        self.meta.matches_symbol(sym)
            && self.layers.len() == sym.timeline.layers.len()
            && self
                .layers
                .iter()
                .zip(&sym.timeline.layers)
                .all(|(arc_l, l)| arc_l.as_ref() == l)
    }

    fn to_symbol(&self) -> crate::Symbol {
        crate::Symbol {
            id: self.meta.id.clone(),
            name: self.meta.name.clone(),
            folder: self.meta.folder.clone(),
            kind: self.meta.kind,
            timeline: crate::Timeline {
                frame_count: self.meta.frame_count,
                layers: self.layers.iter().map(|l| (**l).clone()).collect(),
                scripts: self.meta.scripts.clone(),
                guides: self.meta.guides.clone(),
            },
            class_name: self.meta.class_name.clone(),
            linkage_id: self.meta.linkage_id.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CachedProjectBaseline {
    header: ProjectHeader,
    resources: Vec<crate::ResourceUnit>,
    symbols: Vec<SharedSymbolSpine>,
}

impl CachedProjectBaseline {
    fn capture(project: &Project, prev: Option<&Self>) -> Self {
        let header = ProjectHeader::from_project(project);
        let mut resources = project.resources.clone();
        if let Some(p) = prev {
            share_resources_slice(&p.resources, &mut resources);
        }
        let mut symbols = Vec::with_capacity(project.symbols.len());
        for (idx, sym) in project.symbols.iter().enumerate() {
            let cached_sym = prev.and_then(|p| {
                p.symbols
                    .get(idx)
                    .filter(|s| s.meta.id == sym.id)
                    .or_else(|| p.symbols.iter().find(|s| s.meta.id == sym.id))
            });
            symbols.push(SharedSymbolSpine::from_symbol(sym, cached_sym));
        }
        Self {
            header,
            resources,
            symbols,
        }
    }

    fn matches_project(&self, project: &Project) -> bool {
        self.header.matches_project(project)
            && self.resources == project.resources
            && self.symbols.len() == project.symbols.len()
            && self
                .symbols
                .iter()
                .zip(&project.symbols)
                .all(|(spine, sym)| spine.matches_symbol(sym))
    }

    fn to_project(&self) -> Project {
        Project {
            format_version: self.header.format_version,
            name: self.header.name.clone(),
            width: self.header.width,
            height: self.header.height,
            fps: self.header.fps,
            background: self.header.background,
            root: self.header.root.clone(),
            symbols: self
                .symbols
                .iter()
                .map(SharedSymbolSpine::to_symbol)
                .collect(),
            library_folders: self.header.library_folders.clone(),
            sources: self.header.sources.clone(),
            resources: self.resources.clone(),
            document_class: self.header.document_class.clone(),
            scenes: self.header.scenes.clone(),
            script_language: self.header.script_language,
            also_compile_compatible_scripts: self.header.also_compile_compatible_scripts,
        }
    }

    /// Materialize a lightweight `Project` containing only symbols that actually differ from
    /// `after`, so pre-commit normalization hooks (`rebase_motion_paths`,
    /// `record_motion_property_keys`) never clone untouched symbols.
    fn to_changed_symbols_project(&self, after: &Project) -> Project {
        let mut changed_symbols = Vec::new();
        for sym in &after.symbols {
            if let Some(spine) = self.symbols.iter().find(|s| s.meta.id == sym.id)
                && !spine.matches_symbol(sym)
            {
                changed_symbols.push(spine.to_symbol());
            }
        }
        Project {
            format_version: self.header.format_version,
            name: self.header.name.clone(),
            width: self.header.width,
            height: self.header.height,
            fps: self.header.fps,
            background: self.header.background,
            root: self.header.root.clone(),
            symbols: changed_symbols,
            library_folders: self.header.library_folders.clone(),
            sources: self.header.sources.clone(),
            resources: self.resources.clone(),
            document_class: self.header.document_class.clone(),
            scenes: self.header.scenes.clone(),
            script_language: self.header.script_language,
            also_compile_compatible_scripts: self.header.also_compile_compatible_scripts,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum KeyframeDelta {
    /// Element count is identical; stores only changed element slots and optional non-element metadata shell.
    ElementPatch {
        shell: Option<Box<crate::Keyframe>>,
        elements: Vec<(usize, crate::Element)>,
    },
    /// Elements were added or removed on this keyframe.
    Full(Box<crate::Keyframe>),
}

impl KeyframeDelta {
    fn retained_elements(&self) -> usize {
        match self {
            Self::ElementPatch { elements, .. } => elements.len(),
            Self::Full(kf) => kf.elements.len(),
        }
    }
}

fn keyframe_shell_without_elements(kf: &crate::Keyframe) -> crate::Keyframe {
    crate::Keyframe {
        frame: kf.frame,
        elements: Vec::new(),
        tween: kf.tween,
        label: kf.label.clone(),
        label_type: kf.label_type,
        tween_kind: kf.tween_kind,
        keyed_properties: kf.keyed_properties.clone(),
        orient_to_path: kf.orient_to_path,
        property_eases: kf.property_eases.clone(),
        motion_path: kf.motion_path.clone(),
        motion_tracks: kf.motion_tracks.clone(),
        motion_keys: kf.motion_keys.clone(),
        sync_symbols: kf.sync_symbols,
        snap_to_guide: kf.snap_to_guide,
        sound: kf.sound.clone(),
        shape_hints: kf.shape_hints.clone(),
        classic_rotate_turns: kf.classic_rotate_turns,
        classic_rotate_dir: kf.classic_rotate_dir,
        tween_scale: kf.tween_scale,
        roving: kf.roving,
        instance_overrides: kf.instance_overrides.clone(),
        bitmap_fills: kf.bitmap_fills.clone(),
        asset_warps: kf.asset_warps.clone(),
        camera: kf.camera,
        element_parents: kf.element_parents.clone(),
        controller_elements: kf.controller_elements.clone(),
        ik_armatures: kf.ik_armatures.clone(),
        ik_joint_specs: kf.ik_joint_specs.clone(),
        ik_skins: kf.ik_skins.clone(),
        appearances: kf.appearances.clone(),
        video: kf.video.clone(),
        extended_accessibility: kf.extended_accessibility.clone(),
        vector_effects: kf.vector_effects.clone(),
        property_expressions: kf.property_expressions.clone(),
        perspective_grid: kf.perspective_grid,
        work_area: kf.work_area,
        markers: kf.markers.clone(),
        filters: kf.filters.clone(),
    }
}

fn keyframe_shells_equal(a: &crate::Keyframe, b: &crate::Keyframe) -> bool {
    a.frame == b.frame
        && a.tween == b.tween
        && a.label == b.label
        && a.label_type == b.label_type
        && a.tween_kind == b.tween_kind
        && a.keyed_properties == b.keyed_properties
        && a.orient_to_path == b.orient_to_path
        && a.property_eases == b.property_eases
        && a.motion_path == b.motion_path
        && a.motion_tracks == b.motion_tracks
        && a.motion_keys == b.motion_keys
        && a.sync_symbols == b.sync_symbols
        && a.snap_to_guide == b.snap_to_guide
        && a.sound == b.sound
        && a.shape_hints == b.shape_hints
        && a.classic_rotate_turns == b.classic_rotate_turns
        && a.classic_rotate_dir == b.classic_rotate_dir
        && a.tween_scale == b.tween_scale
        && a.roving == b.roving
        && a.instance_overrides == b.instance_overrides
        && a.bitmap_fills == b.bitmap_fills
        && a.asset_warps == b.asset_warps
        && a.camera == b.camera
        && a.element_parents == b.element_parents
        && a.controller_elements == b.controller_elements
        && a.ik_armatures == b.ik_armatures
        && a.ik_joint_specs == b.ik_joint_specs
        && a.ik_skins == b.ik_skins
        && a.appearances == b.appearances
        && a.video == b.video
        && a.extended_accessibility == b.extended_accessibility
        && a.vector_effects == b.vector_effects
        && a.property_expressions == b.property_expressions
        && a.perspective_grid == b.perspective_grid
        && a.work_area == b.work_area
        && a.markers == b.markers
        && a.filters == b.filters
}

fn diff_keyframe(before: &crate::Keyframe, after: &crate::Keyframe) -> Option<KeyframeDelta> {
    if before.elements.len() == after.elements.len() {
        let shell_same = keyframe_shells_equal(before, after);
        let mut changed_elements = Vec::new();
        for (e_idx, (ea, eb)) in before.elements.iter().zip(&after.elements).enumerate() {
            if ea != eb {
                changed_elements.push((e_idx, ea.clone()));
            }
        }
        if shell_same && changed_elements.is_empty() {
            return None;
        }
        let shell = (!shell_same).then(|| Box::new(keyframe_shell_without_elements(before)));
        Some(KeyframeDelta::ElementPatch {
            shell,
            elements: changed_elements,
        })
    } else {
        Some(KeyframeDelta::Full(Box::new(before.clone())))
    }
}

fn apply_and_invert_keyframe_delta(
    slot: &mut crate::Keyframe,
    delta: KeyframeDelta,
) -> KeyframeDelta {
    match delta {
        KeyframeDelta::ElementPatch { shell, elements } => {
            let inv_shell = shell.map(|mut new_shell| {
                let mut cur_elements = std::mem::take(&mut slot.elements);
                std::mem::swap(&mut new_shell.elements, &mut cur_elements);
                let mut old_kf = std::mem::replace(slot, *new_shell);
                old_kf.elements = Vec::new();
                Box::new(old_kf)
            });
            let mut inv_elements = Vec::with_capacity(elements.len());
            for (e_idx, elem) in elements {
                if let Some(el_slot) = slot.elements.get_mut(e_idx) {
                    let prev_el = std::mem::replace(el_slot, elem);
                    inv_elements.push((e_idx, prev_el));
                }
            }
            KeyframeDelta::ElementPatch {
                shell: inv_shell,
                elements: inv_elements,
            }
        }
        KeyframeDelta::Full(new_kf) => {
            let prev_kf = std::mem::replace(slot, *new_kf);
            KeyframeDelta::Full(Box::new(prev_kf))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum LayerDelta {
    /// Keyframe count is identical; only the listed keyframe indices changed.
    InPlace {
        meta: Option<Box<LayerMeta>>,
        keyframes: Vec<(usize, KeyframeDelta)>,
    },
    /// Keyframes were added or removed on this layer; holds the shared pre-edit `Arc<Layer>`.
    Full(std::sync::Arc<crate::Layer>),
}

#[derive(Clone, Debug, PartialEq)]
enum LayersDelta {
    /// Layer count and IDs are identical; only the listed layer indices changed.
    InPlace(Vec<(usize, LayerDelta)>),
    /// Layers were added, removed, or reordered; holds the shared pre-edit `Arc<Layer>` list.
    Structural(Vec<std::sync::Arc<crate::Layer>>),
}

#[derive(Clone, Debug, PartialEq)]
struct SymbolDelta {
    meta: Option<Box<SymbolMeta>>,
    layers: LayersDelta,
}

#[derive(Clone, Debug, PartialEq)]
enum SymbolsDelta {
    /// Symbol count and IDs are identical; only the listed symbol indices changed.
    InPlace(Vec<(usize, SymbolDelta)>),
    /// Symbols were added, removed, or reordered; holds the shared pre-edit symbol spines.
    Structural(Vec<SharedSymbolSpine>),
}

#[derive(Clone, Debug, PartialEq)]
struct ProjectDelta {
    header: Option<Box<ProjectHeader>>,
    resources: Option<Vec<crate::ResourceUnit>>,
    symbols: SymbolsDelta,
}

fn diff_layer(before: &std::sync::Arc<crate::Layer>, after: &crate::Layer) -> Option<LayerDelta> {
    if before.as_ref() == after {
        return None;
    }
    if before.keyframes.len() == after.keyframes.len() {
        let meta = if LayerMeta::from_layer(before).matches_layer(after) {
            None
        } else {
            Some(Box::new(LayerMeta::from_layer(before)))
        };
        let mut keyframes = Vec::new();
        for (k_idx, (ka, kb)) in before.keyframes.iter().zip(&after.keyframes).enumerate() {
            if let Some(kd) = diff_keyframe(ka, kb) {
                keyframes.push((k_idx, kd));
            }
        }
        if meta.is_none() && keyframes.is_empty() {
            return None;
        }
        Some(LayerDelta::InPlace { meta, keyframes })
    } else {
        Some(LayerDelta::Full(std::sync::Arc::clone(before)))
    }
}

fn diff_symbol(before: &SharedSymbolSpine, after: &crate::Symbol) -> Option<SymbolDelta> {
    if before.matches_symbol(after) {
        return None;
    }
    let meta = if before.meta.matches_symbol(after) {
        None
    } else {
        Some(Box::new(before.meta.clone()))
    };
    let same_layer_ids = before.layers.len() == after.timeline.layers.len()
        && before
            .layers
            .iter()
            .zip(&after.timeline.layers)
            .all(|(la, lb)| la.id == lb.id);
    let layers = if same_layer_ids {
        let mut modified = Vec::new();
        for (l_idx, (la, lb)) in before.layers.iter().zip(&after.timeline.layers).enumerate() {
            if let Some(ld) = diff_layer(la, lb) {
                modified.push((l_idx, ld));
            }
        }
        LayersDelta::InPlace(modified)
    } else {
        LayersDelta::Structural(before.layers.clone())
    };
    Some(SymbolDelta { meta, layers })
}

fn diff_baseline_to_project(
    before: &CachedProjectBaseline,
    after: &Project,
) -> Option<ProjectDelta> {
    let header = if before.header.matches_project(after) {
        None
    } else {
        Some(Box::new(before.header.clone()))
    };
    let resources = if before.resources == after.resources {
        None
    } else {
        let mut res = before.resources.clone();
        share_resources_slice(&after.resources, &mut res);
        Some(res)
    };
    let same_symbol_ids = before.symbols.len() == after.symbols.len()
        && before
            .symbols
            .iter()
            .zip(&after.symbols)
            .all(|(sa, sb)| sa.meta.id == sb.id);
    let symbols = if same_symbol_ids {
        let mut modified = Vec::new();
        for (s_idx, (sa, sb)) in before.symbols.iter().zip(&after.symbols).enumerate() {
            if let Some(sd) = diff_symbol(sa, sb) {
                modified.push((s_idx, sd));
            }
        }
        if header.is_none() && resources.is_none() && modified.is_empty() {
            return None;
        }
        SymbolsDelta::InPlace(modified)
    } else {
        SymbolsDelta::Structural(before.symbols.clone())
    };
    Some(ProjectDelta {
        header,
        resources,
        symbols,
    })
}

fn apply_and_invert_delta(project: &mut Project, delta: ProjectDelta) -> ProjectDelta {
    let inv_header = delta.header.map(|h| Box::new(h.apply_to_project(project)));
    let inv_resources = delta.resources.map(|mut new_res| {
        share_resources_slice(&project.resources, &mut new_res);
        std::mem::replace(&mut project.resources, new_res)
    });
    let inv_symbols = match delta.symbols {
        SymbolsDelta::InPlace(mod_syms) => {
            let mut inv_mod_syms = Vec::with_capacity(mod_syms.len());
            for (s_idx, sym_delta) in mod_syms {
                if let Some(sym) = project.symbols.get_mut(s_idx) {
                    let inv_meta = sym_delta.meta.map(|m| Box::new(m.apply_to_symbol(sym)));
                    let inv_layers = match sym_delta.layers {
                        LayersDelta::InPlace(mod_layers) => {
                            let mut inv_mod_layers = Vec::with_capacity(mod_layers.len());
                            for (l_idx, layer_delta) in mod_layers {
                                if let Some(layer) = sym.timeline.layers.get_mut(l_idx) {
                                    let inv_ld = match layer_delta {
                                        LayerDelta::InPlace { meta, keyframes } => {
                                            let inv_lmeta =
                                                meta.map(|m| Box::new(m.apply_to_layer(layer)));
                                            let mut inv_kfs = Vec::with_capacity(keyframes.len());
                                            for (k_idx, kf_delta) in keyframes {
                                                if let Some(slot) = layer.keyframes.get_mut(k_idx) {
                                                    let prev_kd = apply_and_invert_keyframe_delta(
                                                        slot, kf_delta,
                                                    );
                                                    inv_kfs.push((k_idx, prev_kd));
                                                }
                                            }
                                            LayerDelta::InPlace {
                                                meta: inv_lmeta,
                                                keyframes: inv_kfs,
                                            }
                                        }
                                        LayerDelta::Full(arc_layer) => {
                                            let prev_layer =
                                                std::mem::replace(layer, (*arc_layer).clone());
                                            LayerDelta::Full(std::sync::Arc::new(prev_layer))
                                        }
                                    };
                                    inv_mod_layers.push((l_idx, inv_ld));
                                }
                            }
                            LayersDelta::InPlace(inv_mod_layers)
                        }
                        LayersDelta::Structural(arc_layers) => {
                            let new_layers: Vec<crate::Layer> =
                                arc_layers.iter().map(|l| (**l).clone()).collect();
                            let old_layers =
                                std::mem::replace(&mut sym.timeline.layers, new_layers);
                            LayersDelta::Structural(
                                old_layers.into_iter().map(std::sync::Arc::new).collect(),
                            )
                        }
                    };
                    inv_mod_syms.push((
                        s_idx,
                        SymbolDelta {
                            meta: inv_meta,
                            layers: inv_layers,
                        },
                    ));
                }
            }
            SymbolsDelta::InPlace(inv_mod_syms)
        }
        SymbolsDelta::Structural(spines) => {
            let new_symbols: Vec<crate::Symbol> =
                spines.iter().map(SharedSymbolSpine::to_symbol).collect();
            let old_symbols = std::mem::replace(&mut project.symbols, new_symbols);
            let old_spines = old_symbols
                .iter()
                .map(|s| SharedSymbolSpine::from_symbol(s, None))
                .collect();
            SymbolsDelta::Structural(old_spines)
        }
    };
    ProjectDelta {
        header: inv_header,
        resources: inv_resources,
        symbols: inv_symbols,
    }
}

fn infer_command_step_from_baseline(
    before: &CachedProjectBaseline,
    after: &Project,
) -> Option<AuthoringCommandStep> {
    for (spine_a, sym_b) in before.symbols.iter().zip(&after.symbols) {
        if spine_a.meta.id != sym_b.id {
            continue;
        }
        for (layer_a, layer_b) in spine_a.layers.iter().zip(&sym_b.timeline.layers) {
            if layer_a.as_ref() == layer_b {
                continue;
            }
            for (key_a, key_b) in layer_a.keyframes.iter().zip(&layer_b.keyframes) {
                if key_a == key_b {
                    continue;
                }
                if key_b.elements.len() < key_a.elements.len() {
                    return Some(AuthoringCommandStep::DeleteSelection);
                }
                if key_b.elements.len() > key_a.elements.len()
                    && let Some(added) = key_b.elements.last()
                    && let Some(orig) = key_a.elements.last()
                {
                    return Some(AuthoringCommandStep::DuplicateSelection {
                        offset_x: added.transform.x - orig.transform.x,
                        offset_y: added.transform.y - orig.transform.y,
                    });
                }
                for (el_a, el_b) in key_a.elements.iter().zip(&key_b.elements) {
                    if el_a.id != el_b.id || el_a == el_b {
                        continue;
                    }
                    let dx = el_b.transform.x - el_a.transform.x;
                    let dy = el_b.transform.y - el_a.transform.y;
                    if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Translate { dx, dy });
                    }
                    if (el_b.transform.rotation - el_a.transform.rotation).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Rotate {
                            degrees: el_b.transform.rotation - el_a.transform.rotation,
                        });
                    }
                    if (el_b.transform.skew - el_a.transform.skew).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Skew {
                            degrees: el_b.transform.skew - el_a.transform.skew,
                        });
                    }
                    if el_a.transform.scale_x.abs() > 1e-6
                        && el_a.transform.scale_y.abs() > 1e-6
                        && ((el_b.transform.scale_x - el_a.transform.scale_x).abs() > 1e-6
                            || (el_b.transform.scale_y - el_a.transform.scale_y).abs() > 1e-6)
                    {
                        return Some(AuthoringCommandStep::Scale {
                            sx: el_b.transform.scale_x / el_a.transform.scale_x,
                            sy: el_b.transform.scale_y / el_a.transform.scale_y,
                        });
                    }
                    if (el_b.transform.alpha - el_a.transform.alpha).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::SetAlpha {
                            alpha: el_b.transform.alpha,
                        });
                    }
                    if el_b.visible != el_a.visible {
                        return Some(AuthoringCommandStep::SetVisible {
                            visible: el_b.visible,
                        });
                    }
                    if el_b.blend_mode != el_a.blend_mode {
                        return Some(AuthoringCommandStep::SetBlendMode {
                            blend_mode: el_b.blend_mode,
                        });
                    }
                    if el_b.name != el_a.name {
                        return Some(AuthoringCommandStep::RenameSelection {
                            name: el_b.name.clone(),
                        });
                    }
                    match (&el_a.kind, &el_b.kind) {
                        (
                            ElementKind::Rectangle {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::Ellipse {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::Path {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::PaintedPath {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            },
                            ElementKind::Rectangle {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::Ellipse {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::Path {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::PaintedPath {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            },
                        ) => {
                            if fa != fb {
                                return Some(AuthoringCommandStep::SetFillColor { color: *fb });
                            }
                            if sa != sb {
                                return Some(AuthoringCommandStep::SetStrokeColor { color: *sb });
                            }
                            if (wa - wb).abs() > 1e-6 {
                                return Some(AuthoringCommandStep::SetStrokeWidth { width: *wb });
                            }
                        }
                        (
                            ElementKind::Instance { symbol_id: sa, .. },
                            ElementKind::Instance { symbol_id: sb, .. },
                        ) if sa != sb => {
                            return Some(AuthoringCommandStep::SwapSymbol {
                                target_symbol_id: sb.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
struct PendingEntry {
    baseline: CachedProjectBaseline,
    label: String,
    command_step: Option<AuthoringCommandStep>,
}

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    delta: ProjectDelta,
    label: String,
    command_step: Option<AuthoringCommandStep>,
}

/// Default maximum retained element payloads across Undo/Redo deltas (`~2,000,000` elements)
/// so `>100,000`-node scenes never accumulate unbounded cloned geometry across 300 Undo steps,
/// while always retaining at least the most recent Undo step.
pub const DEFAULT_HISTORY_ELEMENT_BUDGET: usize = 2_000_000;

/// Structural-delta history with explicit transaction boundaries and `Arc`-shared layer spines.
/// Unchanged symbols, layers, keyframes, and elements are never duplicated across Undo steps.
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    pending: Option<PendingEntry>,
    clean_baseline: Option<CachedProjectBaseline>,
    capacity: usize,
    element_budget: usize,
    revision: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(300)
    }
}

impl History {
    const DEFAULT_LABEL: &'static str = "Edit";

    pub fn new(capacity: usize) -> Self {
        Self::with_element_budget(capacity, DEFAULT_HISTORY_ELEMENT_BUDGET)
    }

    pub fn with_element_budget(capacity: usize, element_budget: usize) -> Self {
        Self {
            undo: vec![],
            redo: vec![],
            pending: None,
            clean_baseline: None,
            capacity,
            element_budget: element_budget.max(1),
            revision: next_revision(),
        }
    }

    /// Maximum undo steps. Lowering it drops the oldest steps now; redo steps are kept.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn element_budget(&self) -> usize {
        self.element_budget
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.trim_undo_to_capacity();
    }

    pub fn set_element_budget(&mut self, element_budget: usize) {
        self.element_budget = element_budget.max(1);
        self.trim_undo_to_capacity();
    }

    fn seed_baseline_layer_ptrs(&self) -> std::collections::HashSet<usize> {
        self.clean_baseline
            .iter()
            .flat_map(|b| &b.symbols)
            .flat_map(|s| &s.layers)
            .map(|l| std::sync::Arc::as_ptr(l) as usize)
            .collect()
    }

    fn delta_retained_elements_dedup(
        d: &ProjectDelta,
        seen_layers: &mut std::collections::HashSet<usize>,
    ) -> usize {
        let mut charge_layer = |l: &std::sync::Arc<crate::Layer>| -> usize {
            if seen_layers.insert(std::sync::Arc::as_ptr(l) as usize) {
                l.keyframes.iter().map(|k| k.elements.len()).sum()
            } else {
                0
            }
        };
        match &d.symbols {
            SymbolsDelta::InPlace(syms) => syms
                .iter()
                .map(|(_, sd)| match &sd.layers {
                    LayersDelta::InPlace(layers) => layers
                        .iter()
                        .map(|(_, ld)| match ld {
                            LayerDelta::InPlace { keyframes, .. } => keyframes
                                .iter()
                                .map(|(_, kd)| kd.retained_elements())
                                .sum::<usize>(),
                            LayerDelta::Full(l) => charge_layer(l),
                        })
                        .sum::<usize>(),
                    LayersDelta::Structural(layers) => layers.iter().map(&mut charge_layer).sum(),
                })
                .sum(),
            SymbolsDelta::Structural(spines) => spines
                .iter()
                .flat_map(|s| &s.layers)
                .map(&mut charge_layer)
                .sum(),
        }
    }

    fn trim_undo_to_capacity(&mut self) {
        if self.undo.len() > self.capacity {
            let excess = self.undo.len() - self.capacity;
            self.undo.drain(..excess);
        }
        if self.undo.len() > 1 {
            // Walk newest-to-oldest, seeding with live `clean_baseline` Arc<Layer> pointers so
            // untouched shared layers on structural edits are never double-charged, and determine
            // the oldest suffix within `element_budget`.
            let mut seen = self.seed_baseline_layer_ptrs();
            let mut total = 0usize;
            let mut keep_from = self.undo.len();
            for idx in (0..self.undo.len()).rev() {
                let cost = Self::delta_retained_elements_dedup(&self.undo[idx].delta, &mut seen);
                if idx + 1 < self.undo.len() && total.saturating_add(cost) > self.element_budget {
                    break;
                }
                total = total.saturating_add(cost);
                keep_from = idx;
            }
            if keep_from > 0 {
                self.undo.drain(..keep_from);
            }
        }
    }

    fn normalize_label(raw: String) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            Self::DEFAULT_LABEL.to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// Repeated begin calls preserve the original baseline until commit or cancel.
    /// Reuses `Arc<Layer>` allocations from `clean_baseline` for all untouched layers.
    pub fn begin(&mut self, project: &Project) {
        if self.pending.is_none() {
            let baseline = CachedProjectBaseline::capture(project, self.clean_baseline.as_ref());
            self.clean_baseline = Some(baseline.clone());
            self.pending = Some(PendingEntry {
                baseline,
                label: Self::DEFAULT_LABEL.to_owned(),
                command_step: None,
            });
        }
    }

    /// Repeated begin calls preserve the original baseline; an unnamed (`"Edit"`) pending
    /// transaction upgrades its label when a named command runs inside it.
    pub fn begin_named(&mut self, project: &Project, label: impl Into<String>) {
        let label = Self::normalize_label(label.into());
        match &mut self.pending {
            None => {
                let baseline =
                    CachedProjectBaseline::capture(project, self.clean_baseline.as_ref());
                self.clean_baseline = Some(baseline.clone());
                self.pending = Some(PendingEntry {
                    baseline,
                    label,
                    command_step: None,
                });
            }
            Some(pending)
                if pending.label == Self::DEFAULT_LABEL && label != Self::DEFAULT_LABEL =>
            {
                pending.label = label;
            }
            Some(_) => {}
        }
    }

    /// Set or replace the label of the open pending transaction, if any.
    pub fn set_label(&mut self, label: impl Into<String>) {
        let label = Self::normalize_label(label.into());
        if let Some(pending) = &mut self.pending {
            pending.label = label;
        }
    }

    /// Explicitly attach a replayable `AuthoringCommandStep` to the open pending transaction
    /// (or the most recently committed undo step if no transaction is open).
    pub fn record_command_step(&mut self, step: AuthoringCommandStep) {
        if let Some(pending) = &mut self.pending {
            pending.command_step = Some(step);
        } else if let Some(last) = self.undo.last_mut() {
            last.command_step = Some(step);
        }
    }

    /// Commit after letting `normalize` adjust the edited project with a pre-edit view
    /// containing any modified symbols.
    pub fn commit_with(
        &mut self,
        project: &mut Project,
        normalize: impl FnOnce(&Project, &mut Project),
    ) -> bool {
        if let Some(pending) = &self.pending {
            let before = pending.baseline.to_changed_symbols_project(project);
            normalize(&before, project);
        }
        self.commit(project)
    }

    pub fn commit(&mut self, project: &Project) -> bool {
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        let Some(delta) = diff_baseline_to_project(&pending.baseline, project) else {
            return false;
        };
        if pending.command_step.is_none() {
            pending.command_step = infer_command_step_from_baseline(&pending.baseline, project);
        }
        self.clean_baseline = Some(CachedProjectBaseline::capture(
            project,
            Some(&pending.baseline),
        ));
        self.redo.clear();
        self.revision = next_revision();
        if self.capacity > 0 {
            self.undo.push(HistoryEntry {
                delta,
                label: pending.label,
                command_step: pending.command_step,
            });
            self.trim_undo_to_capacity();
        }
        true
    }

    pub fn cancel(&mut self, project: &mut Project) -> bool {
        if let Some(pending) = self.pending.take() {
            if !pending.baseline.matches_project(project) {
                *project = pending.baseline.to_project();
            }
            self.clean_baseline = Some(pending.baseline);
            self.revision = next_revision();
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let redo_delta = apply_and_invert_delta(project, entry.delta);
        if let Some(prev_base) = self.clean_baseline.take() {
            self.clean_baseline = Some(CachedProjectBaseline::capture(project, Some(&prev_base)));
        }
        self.redo.push(HistoryEntry {
            delta: redo_delta,
            label: entry.label,
            command_step: entry.command_step,
        });
        self.revision = next_revision();
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let undo_delta = apply_and_invert_delta(project, entry.delta);
        if let Some(prev_base) = self.clean_baseline.take() {
            self.clean_baseline = Some(CachedProjectBaseline::capture(project, Some(&prev_base)));
        }
        if self.capacity > 0 {
            self.undo.push(HistoryEntry {
                delta: undo_delta,
                label: entry.label,
                command_step: entry.command_step,
            });
            self.trim_undo_to_capacity();
        }
        self.revision = next_revision();
        true
    }

    /// Count of retained keyframe payloads across the Undo/Redo stacks (for memory/delta diagnostics).
    pub fn retained_delta_keyframes(&self) -> usize {
        fn count_delta(d: &ProjectDelta) -> usize {
            match &d.symbols {
                SymbolsDelta::InPlace(syms) => syms
                    .iter()
                    .map(|(_, sd)| match &sd.layers {
                        LayersDelta::InPlace(layers) => layers
                            .iter()
                            .map(|(_, ld)| match ld {
                                LayerDelta::InPlace { keyframes, .. } => keyframes.len(),
                                LayerDelta::Full(l) => l.keyframes.len(),
                            })
                            .sum::<usize>(),
                        LayersDelta::Structural(layers) => {
                            layers.iter().map(|l| l.keyframes.len()).sum()
                        }
                    })
                    .sum(),
                SymbolsDelta::Structural(spines) => spines
                    .iter()
                    .flat_map(|s| &s.layers)
                    .map(|l| l.keyframes.len())
                    .sum(),
            }
        }
        self.undo
            .iter()
            .chain(&self.redo)
            .map(|e| count_delta(&e.delta))
            .sum()
    }

    /// Count of retained `Element` payloads across Undo/Redo deltas (for `>100k`-node memory budget checks).
    pub fn retained_delta_elements(&self) -> usize {
        let mut seen = self.seed_baseline_layer_ptrs();
        self.undo
            .iter()
            .rev()
            .chain(self.redo.iter().rev())
            .map(|e| Self::delta_retained_elements_dedup(&e.delta, &mut seen))
            .sum()
    }

    /// Return the replayable `AuthoringCommandStep` recorded for 0-based undo step `index`.
    pub fn step_command(&self, index: usize) -> Option<&AuthoringCommandStep> {
        self.undo.get(index).and_then(|e| e.command_step.as_ref())
    }

    /// Return the most recently committed replayable `AuthoringCommandStep`.
    pub fn last_command_step(&self) -> Option<&AuthoringCommandStep> {
        self.undo.iter().rev().find_map(|e| e.command_step.as_ref())
    }

    /// Collect replayable `AuthoringCommandStep`s for the requested 0-based undo step indices.
    pub fn collect_command_steps(
        &self,
        indices: &[usize],
    ) -> Result<Vec<AuthoringCommandStep>, String> {
        if indices.is_empty() || indices.len() > MAX_COMMAND_STEPS {
            return Err(format!(
                "Select 1..={MAX_COMMAND_STEPS} history steps to replay or save."
            ));
        }
        let mut steps = Vec::with_capacity(indices.len());
        for &idx in indices {
            let Some(entry) = self.undo.get(idx) else {
                return Err(format!("History step {} is out of range.", idx + 1));
            };
            let Some(cmd) = &entry.command_step else {
                return Err(format!(
                    "Step \"{}\" cannot be replayed on a selection.",
                    entry.label
                ));
            };
            steps.push(cmd.clone());
        }
        Ok(steps)
    }

    /// Step backward or forward to `target_undo_len` (`0` = initial baseline before all steps,
    /// `i + 1` = state after 0-based step `i`). Commits any pending transaction first.
    pub fn step_to(&mut self, project: &mut Project, target_undo_len: usize) -> bool {
        self.commit(project);
        let total = self.undo.len() + self.redo.len();
        let target = target_undo_len.min(total);
        if target == self.undo.len() {
            return false;
        }
        while self.undo.len() > target {
            if !self.undo(project) {
                break;
            }
        }
        while self.undo.len() < target {
            if !self.redo(project) {
                break;
            }
        }
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|entry| entry.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.label.as_str())
    }
    pub fn undo_labels(&self) -> impl ExactSizeIterator<Item = &str> {
        self.undo.iter().map(|entry| entry.label.as_str())
    }
    pub fn redo_labels(&self) -> impl ExactSizeIterator<Item = &str> {
        self.redo.iter().rev().map(|entry| entry.label.as_str())
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.clean_baseline = None;
        self.revision = next_revision();
    }

    /// Synchronize `project.name` and `sources[0].path` across all cached `History` baselines
    /// (`clean_baseline`, `pending`, `undo`, and `redo`) when a standalone `.as` file is renamed via `Save As…`.
    pub fn sync_standalone_script_identity(&mut self, project_name: &str, source_path: &str) {
        if let Some(base) = &mut self.clean_baseline {
            base.header.name = project_name.to_owned();
            if let Some(unit) = base.header.sources.first_mut() {
                unit.path = source_path.to_owned();
            }
        }
        if let Some(pending) = &mut self.pending {
            pending.baseline.header.name = project_name.to_owned();
            if let Some(unit) = pending.baseline.header.sources.first_mut() {
                unit.path = source_path.to_owned();
            }
        }
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            if let Some(header) = &mut entry.delta.header {
                header.name = project_name.to_owned();
                if let Some(unit) = header.sources.first_mut() {
                    unit.path = source_path.to_owned();
                }
            }
        }
        self.revision = next_revision();
    }

    /// Changes whenever a committed edit, Undo, Redo, cancel or clear changes the document this
    /// history tracks; unique across all histories in the process. Caches of rendered document
    /// content key on it.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Keyframe, Layer, LayerProperties, ResourceUnit, Symbol, SymbolKind, Timeline, Transform,
    };
    use std::sync::Arc;

    #[test]
    fn history_deduplicates_resource_arc_buffers_across_snapshots() {
        let mut project = Project::blank();
        project.format_version = 3;
        let bytes = vec![42u8; 4096];
        let unit =
            ResourceUnit::from_bytes("res-1", "images/hero.png", "image/png", &bytes).unwrap();
        project.resources.push(unit);

        let mut history = History::new(50);
        // Simulate a round-trip / distinct Arc<str> allocation with identical base64 content.
        history.begin(&project);
        project.name = "Edited".into();
        project.resources[0].data_base64 = Arc::from(project.resources[0].data_base64.as_ref());
        assert!(history.commit(&project));

        let live_arc = project.resources[0].data_base64.clone();
        assert!(history.undo(&mut project));
        assert!(
            Arc::ptr_eq(&project.resources[0].data_base64, &live_arc),
            "Undo snapshot must share the same Arc<str> buffer pointer"
        );
    }

    #[test]
    fn structural_delta_history_retains_only_modified_keyframes_across_300_steps() {
        let mut project = Project::blank();
        // Add 20 symbols, each with 5 layers and 10 keyframes (1,000 total keyframes in the project).
        for s in 0..20 {
            let mut layers = Vec::new();
            for l in 0..5 {
                let mut keyframes = Vec::new();
                for k in 0..10 {
                    keyframes.push(Keyframe {
                        frame: k * 2,
                        elements: vec![Element {
                            id: format!("el-{s}-{l}-{k}"),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 50.0,
                                height: 40.0,
                                fill: [255, 0, 0, 255],
                                stroke: [0, 0, 0, 255],
                                stroke_width: 1.0,
                            },
                            visible: true,
                            blend_mode: BlendMode::Normal,
                            filters: vec![],
                            cache_as_bitmap: false,
                            merge: false,
                            accessibility: None,
                            scripts: vec![],
                            ..Element::default()
                        }],
                        ..Default::default()
                    });
                }
                layers.push(Layer {
                    id: format!("layer-{s}-{l}"),
                    name: format!("Layer {l}"),
                    visible: true,
                    locked: false,
                    keyframes,
                    properties: LayerProperties::default(),
                });
            }
            project.symbols.push(Symbol {
                id: format!("sym-{s}"),
                name: format!("Symbol {s}"),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 20,
                    layers,
                    scripts: vec![],
                    guides: vec![],
                },
                class_name: None,
                linkage_id: None,
            });
        }

        let mut history = History::default();
        assert_eq!(history.capacity(), 300);

        // Perform 300 single-keyframe edits on symbol 1, layer 0, keyframe 0.
        for step in 1..=300 {
            history.begin_named(&project, format!("Move {step}"));
            project.symbols[1].timeline.layers[0].keyframes[0].elements[0]
                .transform
                .x = step as f64;
            assert!(history.commit(&project));
        }

        assert_eq!(history.undo_len(), 300);
        // Each of the 300 undo steps modified exactly 1 keyframe (300 retained keyframes total,
        // NOT 300 * 1,001 = 300,300 keyframes!).
        assert_eq!(history.retained_delta_keyframes(), 300);

        // Undo 150 steps and verify exact coordinate restoration.
        for expected in (150..300).rev() {
            assert!(history.undo(&mut project));
            assert_eq!(
                project.symbols[1].timeline.layers[0].keyframes[0].elements[0]
                    .transform
                    .x,
                expected as f64
            );
        }
        assert_eq!(history.undo_len(), 150);
        assert_eq!(history.redo_len(), 150);
        assert_eq!(history.retained_delta_keyframes(), 300);

        // Redo all 150 steps back to 300.
        for expected in 151..=300 {
            assert!(history.redo(&mut project));
            assert_eq!(
                project.symbols[1].timeline.layers[0].keyframes[0].elements[0]
                    .transform
                    .x,
                expected as f64
            );
        }
    }

    #[test]
    fn redo_respects_reduced_history_capacity() {
        let mut project = Project::blank();
        let mut history = History::new(3);
        for step in 1..=3 {
            history.begin(&project);
            project.name = format!("Step {step}");
            assert!(history.commit(&project));
        }
        for _ in 0..3 {
            assert!(history.undo(&mut project));
        }
        assert_eq!(history.undo_len(), 0);
        assert_eq!(history.redo_len(), 3);

        history.set_capacity(1);
        for _ in 0..3 {
            assert!(history.redo(&mut project));
            assert!(history.undo_len() <= 1);
        }
        assert_eq!(project.name, "Step 3");
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.redo_len(), 0);
    }

    #[test]
    fn sync_standalone_script_identity_preserves_new_filename_across_undo_redo() {
        let mut project = Project::blank();
        project.name = "Foo.as".into();
        project.sources = vec![crate::SourceUnit {
            id: "src-1".into(),
            path: "Foo.as".into(),
            language: crate::ScriptLanguage::ActionScript2,
            source: "class Foo {}\n".into(),
        }];

        let mut history = History::new(50);
        history.begin_named(&project, "Edit class");
        project.sources[0].source = "class Bar {}\n".into();
        assert!(history.commit(&project));

        history.begin_named(&project, "Add method");
        project.sources[0].source = "class Bar {\n    function run() {}\n}\n".into();
        assert!(history.commit(&project));
        assert!(history.undo(&mut project));

        // Simulate Save As `Bar.as` while both undo and redo snapshots exist (plus open pending).
        history.begin_named(&project, "Format scripts on save");
        project.sources[0].source = "class Bar {\n}\n".into();
        project.name = "Bar.as".into();
        project.sources[0].path = "Bar.as".into();
        history.sync_standalone_script_identity("Bar.as", "Bar.as");
        assert!(history.commit(&project));

        let saved_snapshot = project.clone();
        assert!(history.undo(&mut project));
        assert_eq!(project.name, "Bar.as");
        assert_eq!(project.sources[0].path, "Bar.as");
        assert_eq!(project.sources[0].source, "class Bar {}\n");

        assert!(history.undo(&mut project));
        assert_eq!(project.name, "Bar.as");
        assert_eq!(project.sources[0].path, "Bar.as");
        assert_eq!(project.sources[0].source, "class Foo {}\n");

        assert!(history.redo(&mut project));
        assert!(history.redo(&mut project));
        assert_eq!(project, saved_snapshot);
    }

    #[test]
    fn t497_large_105k_node_scene_uses_element_patch_and_enforces_element_budget() {
        let mut project = Project::blank();
        let elements = &mut project.symbols[0].timeline.layers[0].keyframes[0].elements;
        elements.reserve(105_000);
        for i in 0..105_000usize {
            elements.push(Element {
                id: format!("el-{i}"),
                kind: ElementKind::Rectangle {
                    width: 4.0,
                    height: 4.0,
                    fill: [100, 150, 200, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        }

        let mut history = History::new(300);
        for step in 1..=20usize {
            history.begin_named(&project, format!("Move {step}"));
            project.symbols[0].timeline.layers[0].keyframes[0].elements[step]
                .transform
                .x = step as f64 * 10.0;
            assert!(history.commit(&project));
        }
        assert_eq!(history.undo_len(), 20);
        assert_eq!(
            history.retained_delta_elements(),
            20,
            "20 single-element edits on a 105k-node keyframe must retain 20 elements, not 2.1M"
        );

        assert!(history.undo(&mut project));
        assert_eq!(
            project.symbols[0].timeline.layers[0].keyframes[0].elements[20]
                .transform
                .x,
            0.0
        );
        assert!(history.redo(&mut project));
        assert_eq!(
            project.symbols[0].timeline.layers[0].keyframes[0].elements[20]
                .transform
                .x,
            200.0
        );

        // Structural element-count mutations use KeyframeDelta::Full, bounded by element_budget
        // while always retaining at least 1 Undo step.
        let mut bounded = History::with_element_budget(300, 150_000);
        for step in 0..3usize {
            bounded.begin_named(&project, format!("Push {step}"));
            project.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .push(Element {
                    id: format!("added-{step}"),
                    ..Element::default()
                });
            assert!(bounded.commit(&project));
        }
        assert_eq!(
            bounded.undo_len(),
            1,
            "element_budget must evict older 105k snapshots while preserving the latest Undo step"
        );
        assert!(bounded.undo(&mut project));
        assert_eq!(
            project.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .len(),
            105_002
        );

        // Structural layer additions that keep the 105k-element layer untouched via Arc<Layer>
        // must charge 0 cloned elements against element_budget.
        let mut arc_shared = History::with_element_budget(300, 150_000);
        for step in 0..5usize {
            arc_shared.begin_named(&project, format!("Add layer {step}"));
            project.symbols[0].timeline.layers.push(crate::Layer {
                id: format!("empty-layer-{step}"),
                name: format!("Layer {}", step + 2),
                visible: true,
                locked: false,
                keyframes: vec![crate::Keyframe {
                    frame: 0,
                    ..crate::Keyframe::default()
                }],
                properties: Default::default(),
            });
            assert!(arc_shared.commit(&project));
        }
        assert_eq!(
            arc_shared.undo_len(),
            5,
            "shared untouched Arc<Layer> spines must not be double-charged on structural edits"
        );
        assert_eq!(arc_shared.retained_delta_elements(), 0);
    }
}
