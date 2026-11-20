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

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Abstract filesystem interface for capability-gated JSFL `FLfile` and `fl.runScript` operations,
/// keeping `neoflash-core` free of direct host `std::fs` calls.
pub trait JsflFileSystem: Send + Sync + fmt::Debug {
    fn read_file(&self, path: &Path) -> Result<String, String>;
    fn write_file(&self, path: &Path, content: &str) -> Result<(), String>;
    fn append_file(&self, path: &Path, content: &str) -> Result<(), String>;
    fn create_folder(&self, path: &Path) -> Result<(), String>;
    fn remove(&self, path: &Path) -> Result<bool, String>;
    fn list_folder(&self, path: &Path, files_only: bool) -> Result<Vec<String>, String>;
    fn exists(&self, path: &Path) -> bool;
    fn is_folder(&self, path: &Path) -> bool;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, String> {
        Ok(normalize_lexical_path(path))
    }
}

#[derive(Clone, Debug, Default)]
struct MemoryFsState {
    files: BTreeMap<PathBuf, String>,
    folders: std::collections::BTreeSet<PathBuf>,
    symlinks: BTreeMap<PathBuf, PathBuf>,
}

/// Deterministic in-memory `JsflFileSystem` implementation used in `neoflash-core` when no host
/// filesystem adapter is attached.
#[derive(Clone, Debug)]
pub struct MemoryJsflFileSystem {
    state: std::sync::Arc<std::sync::RwLock<MemoryFsState>>,
}

/// Type alias preserving compatibility for callers referencing `StdJsflFileSystem` on `neoflash-core`.
pub type StdJsflFileSystem = MemoryJsflFileSystem;

impl Default for MemoryJsflFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryJsflFileSystem {
    pub fn new() -> Self {
        let mut state = MemoryFsState::default();
        state.folders.insert(PathBuf::from("/"));
        Self {
            state: std::sync::Arc::new(std::sync::RwLock::new(state)),
        }
    }

    pub fn insert_folder(&self, path: impl AsRef<Path>) {
        let norm = normalize_lexical_path(path.as_ref());
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        for ancestor in norm.ancestors() {
            if !ancestor.as_os_str().is_empty() {
                guard.folders.insert(ancestor.to_path_buf());
            }
        }
    }

    pub fn insert_file(&self, path: impl AsRef<Path>, content: impl Into<String>) {
        let norm = normalize_lexical_path(path.as_ref());
        if let Some(parent) = norm.parent() {
            self.insert_folder(parent);
        }
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        guard.files.insert(norm, content.into());
    }

    pub fn insert_symlink(&self, link: impl AsRef<Path>, target: impl AsRef<Path>) {
        let norm_link = normalize_lexical_path(link.as_ref());
        let norm_target = normalize_lexical_path(target.as_ref());
        if let Some(parent) = norm_link.parent() {
            self.insert_folder(parent);
        }
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        guard.symlinks.insert(norm_link, norm_target);
    }

    fn resolve_symlinks(&self, path: &Path) -> Result<PathBuf, String> {
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        let mut current = normalize_lexical_path(path);
        for _ in 0..16 {
            let comps: Vec<Component<'_>> = current.components().collect();
            let mut accum = PathBuf::new();
            let mut replaced = None;
            for (idx, comp) in comps.iter().enumerate() {
                accum.push(comp.as_os_str());
                if let Some(target) = guard.symlinks.get(&accum) {
                    let mut next = if target.is_absolute() {
                        target.clone()
                    } else {
                        accum
                            .parent()
                            .unwrap_or_else(|| Path::new("/"))
                            .join(target)
                    };
                    for rem in &comps[idx + 1..] {
                        next.push(rem.as_os_str());
                    }
                    replaced = Some(normalize_lexical_path(&next));
                    break;
                }
            }
            if let Some(next) = replaced {
                current = next;
            } else {
                return Ok(current);
            }
        }
        Err("Too many levels of symbolic links.".to_owned())
    }
}

impl JsflFileSystem for MemoryJsflFileSystem {
    fn read_file(&self, path: &Path) -> Result<String, String> {
        let p = self.canonicalize(path)?;
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        guard
            .files
            .get(&p)
            .cloned()
            .ok_or_else(|| format!("No such file: '{}'", p.display()))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<(), String> {
        let p = self.canonicalize(path)?;
        self.insert_file(&p, content.to_owned());
        Ok(())
    }

    fn append_file(&self, path: &Path, content: &str) -> Result<(), String> {
        let p = self.canonicalize(path)?;
        if let Some(parent) = p.parent() {
            self.insert_folder(parent);
        }
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        guard.files.entry(p).or_default().push_str(content);
        Ok(())
    }

    fn create_folder(&self, path: &Path) -> Result<(), String> {
        let p = self.canonicalize(path)?;
        self.insert_folder(&p);
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<bool, String> {
        let p = self.canonicalize(path)?;
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        if guard.files.remove(&p).is_some() {
            return Ok(true);
        }
        if guard.folders.contains(&p) {
            guard.folders.retain(|f| f != &p && !f.starts_with(&p));
            guard.files.retain(|f, _| !f.starts_with(&p));
            guard.symlinks.retain(|l, _| l != &p && !l.starts_with(&p));
            return Ok(true);
        }
        if guard.symlinks.remove(&p).is_some() {
            return Ok(true);
        }
        Ok(false)
    }

    fn list_folder(&self, path: &Path, files_only: bool) -> Result<Vec<String>, String> {
        let p = self.canonicalize(path)?;
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        if !guard.folders.contains(&p) {
            return Err(format!("No such directory: '{}'", p.display()));
        }
        let mut names = std::collections::BTreeSet::new();
        for file_path in guard.files.keys() {
            if file_path.parent() == Some(p.as_path())
                && let Some(name) = file_path.file_name().and_then(|n| n.to_str())
            {
                names.insert(name.to_owned());
            }
        }
        if !files_only {
            for dir_path in &guard.folders {
                if dir_path != &p
                    && dir_path.parent() == Some(p.as_path())
                    && let Some(name) = dir_path.file_name().and_then(|n| n.to_str())
                {
                    names.insert(name.to_owned());
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    fn exists(&self, path: &Path) -> bool {
        let Ok(p) = self.canonicalize(path) else {
            return false;
        };
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        guard.files.contains_key(&p) || guard.folders.contains(&p)
    }

    fn is_folder(&self, path: &Path) -> bool {
        let Ok(p) = self.canonicalize(path) else {
            return false;
        };
        let guard = self.state.read().unwrap_or_else(|e| e.into_inner());
        guard.folders.contains(&p)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, String> {
        self.resolve_symlinks(path)
    }
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

mod batch_color;
mod dom_elements;
mod dom_library;
mod dom_props;
mod dom_timeline;
mod flfile;
mod parser;
#[cfg(test)]
mod tests;

pub use batch_color::*;
use parser::*;

// ---------------------------------------------------------------------------
// JsflSandbox & Capability-Gated FLfile Helpers
// ---------------------------------------------------------------------------

/// Sandboxed, deterministic, interruptible JSFL executor (`T-187`).
#[derive(Clone, Debug)]
pub struct JsflSandbox {
    pub limits: JsflLimits,
    pub flfile_caps: FlFileCapabilities,
    pub fs: std::sync::Arc<dyn JsflFileSystem>,
    pub version: String,
    pub config_uri: String,
    pub script_uri: String,
    pub virtual_scripts: HashMap<String, String>,
}

impl Default for JsflSandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl JsflSandbox {
    pub fn new() -> Self {
        Self {
            limits: JsflLimits::default(),
            flfile_caps: FlFileCapabilities::default(),
            fs: std::sync::Arc::new(MemoryJsflFileSystem::new()),
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
        if let Some(root) = &caps.allowed_root {
            let _ = self.fs.create_folder(root);
        }
        self.flfile_caps = caps;
        self
    }

    pub fn with_file_system(mut self, fs: std::sync::Arc<dyn JsflFileSystem>) -> Self {
        if let Some(root) = &self.flfile_caps.allowed_root {
            let _ = fs.create_folder(root);
        }
        self.fs = fs;
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
