//! Bounded authoring clipboard data. Parsing never executes scripts, reads files,
//! or mutates a document; pasting validates a complete candidate before returning.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use thiserror::Error;

pub const CLIPBOARD_PREFIX: &str = "NEOFLASH-CLIPBOARD\n";
pub const MAX_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;
const VERSION: u32 = 1;
const MAX_SYMBOLS: usize = 1024;
const MAX_ELEMENTS: usize = 10_000;
const MAX_LAYERS: usize = 4096;
const MAX_KEYS: usize = 16_384;
const MAX_SCRIPTS: usize = 10_000;
const MAX_PATH_COMMANDS_TOTAL: usize = 200_000;

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("Invalid NeoFlash clipboard JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Unsupported NeoFlash clipboard version {0}; supported version is 1")]
    UnsupportedVersion(u32),
    #[error("NeoFlash clipboard exceeds the 8 MiB text limit; copy fewer objects")]
    TooLarge,
    #[error("Invalid NeoFlash clipboard: {0}")]
    Invalid(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardPayload {
    format_version: u32,
    source_session: String,
    anchor: [f64; 2],
    elements: Vec<Element>,
    symbols: Vec<Symbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<SourceUnit>,
}

#[derive(Clone, Debug)]
pub struct PasteResult {
    pub project: Project,
    /// Fresh placements in the same painter order as the captured selection.
    pub element_ids: Vec<Id>,
}

impl ClipboardPayload {
    /// Capture sampled, ordered placements and all symbol definitions reachable
    /// through every authored keyframe, plus explicitly exported runtime library
    /// roots. Library order and frame indices survive.
    pub fn capture(
        project: &Project,
        elements: Vec<Element>,
        source_session: &str,
        anchor: [f64; 2],
    ) -> Result<Self, ClipboardError> {
        validate_header(source_session, anchor)?;
        if elements.is_empty() || elements.len() > MAX_ELEMENTS {
            return invalid("selection must contain 1..=10000 placements");
        }
        validate_project(project, "source document")?;
        let closure = reachable(&elements, &project.symbols)?;
        let payload = Self {
            format_version: VERSION,
            source_session: source_session.to_owned(),
            anchor,
            // Transfer the program snapshot as a conservative dependency closure.
            // Class-level pruning can be added once all backends expose a graph.
            sources: project.sources.clone(),
            elements,
            symbols: project
                .symbols
                .iter()
                .filter(|s| closure.contains(&s.id))
                .cloned()
                .collect(),
        };
        payload.validate()?;
        payload.encode()?;
        Ok(payload)
    }

    /// Foreign clipboard text is ignored, including large ordinary text. Only
    /// our exact prefix opts a string into parsing and actionable diagnostics.
    pub fn from_text(text: &str) -> Result<Option<Self>, ClipboardError> {
        let Some(json) = text.strip_prefix(CLIPBOARD_PREFIX) else {
            return Ok(None);
        };
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        // Parse typed data first, retaining serde's duplicate-known-field and
        // recursion checks. Model structs intentionally remain forward-tolerant
        // for documents; clipboard transfer must not silently discard fields.
        let payload: Self = serde_json::from_str(json)?;
        if payload.format_version != VERSION {
            return Err(ClipboardError::UnsupportedVersion(payload.format_version));
        }
        let input: serde_json::Value = serde_json::from_str(json)?;
        reject_unknown_fields(&input, &serde_json::to_value(&payload)?, "clipboard")?;
        payload.validate()?;
        payload.encode()?;
        Ok(Some(payload))
    }

    pub fn to_text(&self) -> Result<String, ClipboardError> {
        self.validate()?;
        self.encode()
    }

    pub fn anchor(&self) -> [f64; 2] {
        self.anchor
    }

    /// Same-session live symbols retain their identity and current definition.
    /// Missing definitions are recovered from the snapshot. A different session
    /// imports fresh identities except for an identical explicit export closure.
    /// No FPS conversion, grouping, script rewriting, or mutation of `destination`.
    pub fn paste_into(
        &self,
        destination: &Project,
        destination_session: &str,
        symbol_id: &str,
        layer_id: &str,
        frame: u32,
        offset: [f64; 2],
    ) -> Result<PasteResult, ClipboardError> {
        self.validate()?;
        self.encode()?;
        validate_header(destination_session, offset)?;
        validate_project(destination, "destination document")?;
        let Some(symbol) = destination.symbol(symbol_id) else {
            return invalid("destination symbol does not exist");
        };
        if frame >= symbol.timeline.frame_count {
            return invalid("destination frame is outside the timeline");
        }
        let Some(layer) = symbol.timeline.layers.iter().find(|l| l.id == layer_id) else {
            return invalid("destination layer does not exist in the target symbol");
        };
        if layer.locked || !layer.visible {
            return invalid("destination layer is locked or hidden");
        }

        let same_session = self.source_session == destination_session;
        let snapshots: HashMap<&str, &Symbol> =
            self.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
        let mut pending: Vec<&str> = self.elements.iter().filter_map(reference).collect();
        pending.extend(
            self.symbols
                .iter()
                .filter(|symbol| symbol.linkage_id.is_some())
                .map(|symbol| symbol.id.as_str()),
        );
        let mut required = HashSet::new();
        let mut identities = HashMap::<Id, Id>::new();
        while let Some(id) = pending.pop() {
            if identities.contains_key(id) || required.contains(id) {
                continue;
            }
            if same_session && destination.symbol(id).is_some() {
                identities.insert(id.to_owned(), id.to_owned());
                // Live definitions may have changed dependencies after Copy.
                continue;
            }
            let Some(symbol) = snapshots.get(id) else {
                return invalid(format!("missing captured symbol {id}"));
            };
            if let Some(linkage) = &symbol.linkage_id
                && let Some(existing) = destination
                    .symbols
                    .iter()
                    .find(|s| s.linkage_id.as_ref() == Some(linkage))
            {
                let Some(source_closure) = canonical_symbol_closure(id, &self.symbols) else {
                    return invalid(
                        "captured exported symbol closure exceeds its dependency budget",
                    );
                };
                let Some(destination_closure) =
                    canonical_symbol_closure(&existing.id, &destination.symbols)
                else {
                    return invalid(
                        "destination exported symbol closure exceeds its dependency budget",
                    );
                };
                if source_closure.symbols != destination_closure.symbols {
                    return invalid(
                        "an export identifier conflicts with different destination symbol content; resolve the linkage conflict before pasting",
                    );
                }
                for (source_id, destination_id) in
                    source_closure.ids.into_iter().zip(destination_closure.ids)
                {
                    if identities
                        .get(&source_id)
                        .is_some_and(|existing| existing != &destination_id)
                    {
                        return invalid(
                            "exported closures have conflicting shared symbol identities; resolve the library conflict before pasting",
                        );
                    }
                    required.remove(&source_id);
                    identities.insert(source_id, destination_id);
                }
                continue;
            }
            required.insert(id.to_owned());
            pending.extend(symbol_elements(symbol).filter_map(reference));
        }

        let mut occupied = project_ids(destination);
        let mut forbidden = payload_ids(self);
        forbidden.extend(occupied.iter().cloned());
        // First allocate all entity identities; repeated placements across keys
        // share one mapping, and references do not depend on library ordering.
        for symbol in self.symbols.iter().filter(|s| required.contains(&s.id)) {
            allocate_identity(
                &symbol.id,
                "symbol",
                same_session,
                &mut identities,
                &mut occupied,
                &forbidden,
            );
            for layer in &symbol.timeline.layers {
                allocate_identity(
                    &layer.id,
                    "layer",
                    same_session,
                    &mut identities,
                    &mut occupied,
                    &forbidden,
                );
                for element in layer.keyframes.iter().flat_map(|k| &k.elements) {
                    allocate_identity(
                        &element.id,
                        "object",
                        same_session,
                        &mut identities,
                        &mut occupied,
                        &forbidden,
                    );
                }
            }
        }
        let mut candidate = destination.clone();
        for source in &self.sources {
            if let Some(existing) = candidate
                .sources
                .iter()
                .find(|unit| unit.path.eq_ignore_ascii_case(&source.path))
            {
                if !same_session
                    && (existing.language != source.language || existing.source != source.source)
                {
                    return invalid(
                        "a project source path conflicts with different destination code; resolve the source conflict before pasting",
                    );
                }
            } else {
                let mut imported = source.clone();
                imported.id = fresh_id("source", &mut occupied, &forbidden);
                candidate.sources.push(imported);
            }
        }
        for original in self.symbols.iter().filter(|s| required.contains(&s.id)) {
            let mut imported = original.clone();
            imported.id = identities[&original.id].clone();
            for layer in &mut imported.timeline.layers {
                layer.id = identities[&layer.id].clone();
                for element in layer.keyframes.iter_mut().flat_map(|k| &mut k.elements) {
                    element.id = identities[&element.id].clone();
                    remap_reference(element, &identities);
                }
            }
            candidate.symbols.push(imported);
        }
        if candidate.has_program() {
            candidate.enable_program();
        }
        let mut pasted = self.elements.clone();
        let mut element_ids = Vec::with_capacity(pasted.len());
        for element in &mut pasted {
            element.id = fresh_id("object", &mut occupied, &forbidden);
            element_ids.push(element.id.clone());
            remap_reference(element, &identities);
            element.transform.x += offset[0];
            element.transform.y += offset[1];
        }
        let layer = candidate
            .symbol_mut(symbol_id)
            .unwrap()
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .unwrap();
        if !layer.keyframes.iter().any(|k| k.frame == frame) {
            let elements = sample_layer(layer, frame);
            let tween = layer.key_at(frame).and_then(|k| k.tween);
            layer.keyframes.push(Keyframe {
                frame,
                elements,
                tween,
                label: String::new(),
            });
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        layer
            .keyframes
            .iter_mut()
            .find(|k| k.frame == frame)
            .unwrap()
            .elements
            .extend(pasted);
        validate_project(&candidate, "pasted document")?;
        crate::model::reserve_ids(&candidate);
        Ok(PasteResult {
            project: candidate,
            element_ids,
        })
    }

    fn validate(&self) -> Result<(), ClipboardError> {
        if self.format_version != VERSION {
            return Err(ClipboardError::UnsupportedVersion(self.format_version));
        }
        validate_header(&self.source_session, self.anchor)?;
        if self.elements.is_empty() {
            return invalid("selection is empty");
        }
        check_budgets(self)?;
        let closure = reachable(&self.elements, &self.symbols)?;
        if closure.len() != self.symbols.len() {
            return invalid("symbol closure contains duplicate or unreachable definitions");
        }
        let mut used = payload_ids(self);
        let wrapper_id = synthetic_id("clipboard-root", &mut used);
        let wrapper_layer = synthetic_id("clipboard-layer", &mut used);
        let mut symbols = self.symbols.clone();
        symbols.push(Symbol {
            class_name: None,
            linkage_id: None,
            id: wrapper_id.clone(),
            name: "Clipboard selection".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                scripts: vec![],
                layers: vec![Layer {
                    id: wrapper_layer,
                    name: "Selection".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: self.elements.clone(),
                        tween: None,
                        label: String::new(),
                    }],
                }],
            },
        });
        validate_project(
            &Project {
                sources: self.sources.clone(),
                document_class: None,
                format_version: if self.sources.is_empty()
                    && !symbols
                        .iter()
                        .any(|symbol| symbol.class_name.is_some() || symbol.linkage_id.is_some())
                {
                    1
                } else {
                    2
                },
                name: "Clipboard validation".into(),
                width: 1,
                height: 1,
                fps: 1,
                background: [0; 4],
                root: wrapper_id,
                symbols,
            },
            "captured content",
        )
    }

    fn encode(&self) -> Result<String, ClipboardError> {
        let mut writer = BoundedText {
            bytes: CLIPBOARD_PREFIX.as_bytes().to_vec(),
            exceeded: false,
        };
        if let Err(error) = serde_json::to_writer(&mut writer, self) {
            return Err(if writer.exceeded {
                ClipboardError::TooLarge
            } else {
                ClipboardError::Json(error)
            });
        }
        // serde_json writes UTF-8, and the prefix is ASCII.
        Ok(String::from_utf8(writer.bytes).expect("JSON serializer produced UTF-8"))
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ClipboardError> {
    Err(ClipboardError::Invalid(message.into()))
}

fn validate_header(session: &str, coordinates: [f64; 2]) -> Result<(), ClipboardError> {
    if session.is_empty() || session.len() > 256 || session.contains('\0') {
        return invalid("document-session token must contain 1..=256 bytes without NUL");
    }
    if !coordinates.iter().all(|v| v.is_finite()) {
        return invalid("anchor and paste offset coordinates must be finite");
    }
    Ok(())
}

fn validate_project(project: &Project, context: &str) -> Result<(), ClipboardError> {
    let issues = crate::validate(project);
    if issues.is_empty() {
        return Ok(());
    }
    let mut message = issues
        .iter()
        .take(8)
        .map(|i| format!("{}: {}", i.path, i.message))
        .collect::<Vec<_>>()
        .join("; ");
    if issues.len() > 8 {
        message.push_str(&format!("; and {} more issues", issues.len() - 8));
    }
    invalid(format!("{context}: {message}"))
}

fn reference(element: &Element) -> Option<&str> {
    match &element.kind {
        ElementKind::Instance { symbol_id, .. } => Some(symbol_id),
        _ => None,
    }
}
fn symbol_elements(symbol: &Symbol) -> impl Iterator<Item = &Element> {
    symbol
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
}

/// Compare reusable export content independently of document-local entity IDs.
/// Shared child/placement identities remain shared in the canonical closure.
struct CanonicalClosure {
    symbols: Vec<Symbol>,
    ids: Vec<Id>,
}
fn canonical_symbol_closure(root: &str, symbols: &[Symbol]) -> Option<CanonicalClosure> {
    let by_id: HashMap<_, _> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > MAX_SYMBOLS {
            return None;
        }
        let symbol = *by_id.get(id)?;
        pending.extend(symbol_elements(symbol).filter_map(reference));
        ordered.push(symbol);
    }
    let mut identities = HashMap::<Id, Id>::new();
    for symbol in &ordered {
        for id in std::iter::once(&symbol.id)
            .chain(symbol.timeline.layers.iter().map(|l| &l.id))
            .chain(symbol_elements(symbol).map(|e| &e.id))
        {
            let next = identities.len();
            identities
                .entry(id.clone())
                .or_insert_with(|| format!("entity-{next}"));
        }
    }
    let ids = ordered.iter().map(|symbol| symbol.id.clone()).collect();
    Some(CanonicalClosure {
        ids,
        symbols: ordered
            .into_iter()
            .map(|symbol| {
                let mut result = symbol.clone();
                result.id = identities[&result.id].clone();
                for layer in &mut result.timeline.layers {
                    layer.id = identities[&layer.id].clone();
                    for element in layer.keyframes.iter_mut().flat_map(|k| &mut k.elements) {
                        element.id = identities[&element.id].clone();
                        remap_reference(element, &identities);
                    }
                }
                result
            })
            .collect(),
    })
}
fn reachable(elements: &[Element], symbols: &[Symbol]) -> Result<HashSet<Id>, ClipboardError> {
    let by_id: HashMap<&str, &Symbol> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut pending: Vec<&str> = elements.iter().filter_map(reference).collect();
    // Dynamic creation can select exports through computed strings or class
    // references; preserve this bounded library catalog with the program data.
    pending.extend(
        symbols
            .iter()
            .filter(|symbol| symbol.linkage_id.is_some())
            .map(|symbol| symbol.id.as_str()),
    );
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.to_owned()) {
            continue;
        }
        if seen.len() > MAX_SYMBOLS {
            return invalid("symbol closure exceeds 1024 definitions; copy fewer objects");
        }
        let Some(symbol) = by_id.get(id) else {
            return invalid(format!("missing reachable symbol {id}"));
        };
        pending.extend(symbol_elements(symbol).filter_map(reference));
    }
    Ok(seen)
}

fn check_budgets(payload: &ClipboardPayload) -> Result<(), ClipboardError> {
    if payload.symbols.len() > MAX_SYMBOLS {
        return invalid("symbol closure exceeds 1024 definitions");
    }
    let mut layers = 0usize;
    let mut keys = 0usize;
    let mut elements = payload.elements.len();
    let mut scripts = 0usize;
    let mut commands = 0usize;
    for symbol in &payload.symbols {
        layers = layers.saturating_add(symbol.timeline.layers.len());
        scripts = scripts.saturating_add(symbol.timeline.scripts.len());
        for layer in &symbol.timeline.layers {
            keys = keys.saturating_add(layer.keyframes.len());
            for key in &layer.keyframes {
                elements = elements.saturating_add(key.elements.len());
            }
        }
    }
    for element in payload
        .elements
        .iter()
        .chain(payload.symbols.iter().flat_map(symbol_elements))
    {
        scripts = scripts.saturating_add(element.scripts.len());
        if let ElementKind::Path { commands: path, .. } = &element.kind {
            commands = commands.saturating_add(path.len());
        }
    }
    if elements > MAX_ELEMENTS
        || layers > MAX_LAYERS
        || keys > MAX_KEYS
        || scripts > MAX_SCRIPTS
        || commands > MAX_PATH_COMMANDS_TOTAL
    {
        return invalid(
            "content budget exceeded: maximum 10000 placements, 4096 layers, 16384 keys, 10000 scripts, 200000 path commands",
        );
    }
    Ok(())
}

fn symbol_ids(symbol: &Symbol, ids: &mut HashSet<Id>) {
    ids.insert(symbol.id.clone());
    for layer in &symbol.timeline.layers {
        ids.insert(layer.id.clone());
        ids.extend(
            layer
                .keyframes
                .iter()
                .flat_map(|k| k.elements.iter().map(|e| e.id.clone())),
        );
    }
}
fn project_ids(project: &Project) -> HashSet<Id> {
    let mut ids = project
        .sources
        .iter()
        .map(|source| source.id.clone())
        .collect();
    for symbol in &project.symbols {
        symbol_ids(symbol, &mut ids);
    }
    ids
}
fn payload_ids(payload: &ClipboardPayload) -> HashSet<Id> {
    let mut ids: HashSet<_> = payload.elements.iter().map(|e| e.id.clone()).collect();
    ids.extend(payload.sources.iter().map(|source| source.id.clone()));
    for symbol in &payload.symbols {
        symbol_ids(symbol, &mut ids);
    }
    ids
}
fn synthetic_id(prefix: &str, occupied: &mut HashSet<Id>) -> Id {
    for index in 0u64.. {
        let id = format!("{prefix}-{index}");
        if occupied.insert(id.clone()) {
            return id;
        }
    }
    unreachable!("clipboard identity budget is finite")
}
fn fresh_id(prefix: &str, occupied: &mut HashSet<Id>, forbidden: &HashSet<Id>) -> Id {
    loop {
        let id = new_id(prefix);
        if !forbidden.contains(&id) && occupied.insert(id.clone()) {
            return id;
        }
    }
}
fn allocate_identity(
    id: &str,
    prefix: &str,
    preserve: bool,
    map: &mut HashMap<Id, Id>,
    occupied: &mut HashSet<Id>,
    forbidden: &HashSet<Id>,
) {
    if map.contains_key(id) {
        return;
    }
    let allocated = if preserve && occupied.insert(id.to_owned()) {
        id.to_owned()
    } else {
        fresh_id(prefix, occupied, forbidden)
    };
    map.insert(id.to_owned(), allocated);
}
fn remap_reference(element: &mut Element, map: &HashMap<Id, Id>) {
    if let ElementKind::Instance { symbol_id, .. } = &mut element.kind {
        *symbol_id = map[symbol_id].clone();
    }
}

fn reject_unknown_fields(
    input: &serde_json::Value,
    canonical: &serde_json::Value,
    path: &str,
) -> Result<(), ClipboardError> {
    match (input, canonical) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (key, value) in a {
                if let Some(expected) = b.get(key) {
                    reject_unknown_fields(value, expected, &format!("{path}.{key}"))?;
                } else if !(key == "accessibility"
                    && value.is_null()
                    && b.contains_key("transform")
                    && b.contains_key("kind"))
                {
                    return invalid(format!("unknown field {path}.{key}"));
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                reject_unknown_fields(a, b, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct BoundedText {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl Write for BoundedText {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_CLIPBOARD_BYTES.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("clipboard text exceeds 8 MiB"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
