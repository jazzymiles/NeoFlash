//! Bounded authoring clipboard data. Parsing never executes scripts, reads files,
//! or mutates a document; pasting validates a complete candidate before returning.
use crate::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<AuthoringGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bitmap_fills: Vec<AuthoredBitmapFill>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    appearances: Vec<ElementAppearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video: Option<TimelineVideoSpan>,
    symbols: Vec<Symbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<SourceUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resources: Vec<ResourceUnit>,
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
        Self::capture_internal(project, None, elements, source_session, anchor)
    }

    /// Capture placements together with the exact keyframe-scoped `bitmap_fills` and
    /// `appearances` active on `(symbol_id, frame)`.
    pub fn capture_at_frame(
        project: &Project,
        symbol_id: &str,
        frame: u32,
        elements: Vec<Element>,
        source_session: &str,
        anchor: [f64; 2],
    ) -> Result<Self, ClipboardError> {
        Self::capture_internal(
            project,
            Some((symbol_id, frame)),
            elements,
            source_session,
            anchor,
        )
    }

    fn capture_internal(
        project: &Project,
        active_keyframe: Option<(&str, u32)>,
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
        let selected: HashSet<_> = elements.iter().map(|element| element.id.as_str()).collect();
        let groups = project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.properties.groups)
            .filter_map(|group| {
                let members = group
                    .members
                    .iter()
                    .filter(|id| selected.contains(id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>();
                // A group can span several keys. Capture its currently copied
                // members, not absent placements from another frame. Group IDs
                // are layer-local, while the clipboard has one selection layer.
                (!members.is_empty()).then(|| AuthoringGroup {
                    id: new_id("group"),
                    name: group.name.clone(),
                    members,
                })
            })
            .collect();
        let mut bitmap_fills = Vec::new();
        let mut seen_bf = HashSet::new();
        let mut appearances = Vec::new();
        let mut seen_app = HashSet::new();
        let has_video_element = elements
            .iter()
            .any(|el| matches!(el.kind, ElementKind::Video { .. }));
        let mut video = None;
        if let Some((sym_id, frame)) = active_keyframe
            && let Some(sym) = project.symbol(sym_id)
        {
            for layer in &sym.timeline.layers {
                if let Some(kf) = layer.key_at(frame) {
                    if has_video_element
                        && video.is_none()
                        && kf.video.is_some()
                        && kf.elements.iter().any(|el| {
                            selected.contains(el.id.as_str())
                                && matches!(el.kind, ElementKind::Video { .. })
                        })
                    {
                        video = kf.video.clone();
                    }
                    for bf in &kf.bitmap_fills {
                        if selected.contains(bf.element_id.as_str())
                            && seen_bf.insert(bf.element_id.clone())
                        {
                            bitmap_fills.push(bf.clone());
                        }
                    }
                    for app in &kf.appearances {
                        if selected.contains(app.element_id.as_str())
                            && seen_app.insert(app.element_id.clone())
                        {
                            appearances.push(app.clone());
                        }
                    }
                }
            }
        } else {
            for sym in &project.symbols {
                for layer in &sym.timeline.layers {
                    for kf in &layer.keyframes {
                        if has_video_element
                            && video.is_none()
                            && kf.video.is_some()
                            && kf.elements.iter().any(|el| {
                                selected.contains(el.id.as_str())
                                    && matches!(el.kind, ElementKind::Video { .. })
                            })
                        {
                            video = kf.video.clone();
                        }
                        for bf in &kf.bitmap_fills {
                            if selected.contains(bf.element_id.as_str())
                                && seen_bf.insert(bf.element_id.clone())
                            {
                                bitmap_fills.push(bf.clone());
                            }
                        }
                        for app in &kf.appearances {
                            if selected.contains(app.element_id.as_str())
                                && seen_app.insert(app.element_id.clone())
                            {
                                appearances.push(app.clone());
                            }
                        }
                    }
                }
            }
        }
        let payload = Self {
            format_version: VERSION,
            source_session: source_session.to_owned(),
            anchor,
            // Transfer the program snapshot as a conservative dependency closure.
            // Class-level pruning can be added once all backends expose a graph.
            sources: project.sources.clone(),
            resources: project.resources.clone(),
            elements,
            groups,
            bitmap_fills,
            appearances,
            video,
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

    pub const SVG_COMMENT_OPEN: &'static str = "<!--(neoflash)";
    pub const SVG_COMMENT_CLOSE: &'static str = "(/neoflash)-->";

    /// Embed a serialized `NEOFLASH-CLIPBOARD\n...` payload as a lossless base64 XML comment
    /// immediately after the root `<svg ...>` opening tag (`T-415`).
    pub fn embed_comment_in_svg(svg: &str, payload_text: &str) -> Result<String, ClipboardError> {
        if !payload_text.starts_with(CLIPBOARD_PREFIX) {
            return invalid("NeoFlash clipboard comment requires a NEOFLASH-CLIPBOARD payload");
        }
        if payload_text.len() > MAX_CLIPBOARD_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        let Some((tag_end, self_closing)) = find_svg_opening_tag_end(svg) else {
            return invalid("SVG markup has no closing '>' on its <svg> root tag");
        };
        let encoded = BASE64.encode(payload_text.as_bytes());
        let mut out = String::with_capacity(
            svg.len()
                + Self::SVG_COMMENT_OPEN.len()
                + encoded.len()
                + Self::SVG_COMMENT_CLOSE.len()
                + 8,
        );
        if self_closing {
            out.push_str(&svg[..tag_end - 2]);
            out.push('>');
            out.push_str(Self::SVG_COMMENT_OPEN);
            out.push_str(&encoded);
            out.push_str(Self::SVG_COMMENT_CLOSE);
            out.push_str("</svg>");
            out.push_str(&svg[tag_end..]);
        } else {
            out.push_str(&svg[..tag_end]);
            out.push_str(Self::SVG_COMMENT_OPEN);
            out.push_str(&encoded);
            out.push_str(Self::SVG_COMMENT_CLOSE);
            out.push_str(&svg[tag_end..]);
        }
        Ok(out)
    }

    /// Foreign clipboard text is ignored, including large ordinary text. Only
    /// our exact prefix or `<svg...><!--(neoflash)<b64>(/neoflash)-->` opts a
    /// string into parsing and actionable diagnostics.
    pub fn from_text(text: &str) -> Result<Option<Self>, ClipboardError> {
        if let Some(json) = text.strip_prefix(CLIPBOARD_PREFIX) {
            if text.len() > MAX_CLIPBOARD_BYTES {
                return Err(ClipboardError::TooLarge);
            }
            return Self::parse_json_payload(json).map(Some);
        }
        if let Some(decoded_text) = extract_svg_neoflash_payload_text(text)? {
            let Some(json) = decoded_text.strip_prefix(CLIPBOARD_PREFIX) else {
                return invalid("embedded NeoFlash SVG clipboard payload is missing its header");
            };
            return Self::parse_json_payload(json).map(Some);
        }
        Ok(None)
    }

    fn parse_json_payload(json: &str) -> Result<Self, ClipboardError> {
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
        Ok(payload)
    }

    pub fn to_text(&self) -> Result<String, ClipboardError> {
        self.validate()?;
        self.encode()
    }

    pub fn anchor(&self) -> [f64; 2] {
        self.anchor
    }

    pub fn source_session(&self) -> &str {
        &self.source_session
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub fn groups(&self) -> &[AuthoringGroup] {
        &self.groups
    }

    pub fn bitmap_fills(&self) -> &[AuthoredBitmapFill] {
        &self.bitmap_fills
    }

    pub fn appearances(&self) -> &[ElementAppearance] {
        &self.appearances
    }

    pub fn video(&self) -> Option<&TimelineVideoSpan> {
        self.video.as_ref()
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn sources(&self) -> &[SourceUnit] {
        &self.sources
    }

    pub fn resources(&self) -> &[ResourceUnit] {
        &self.resources
    }

    /// The wrapper selection and every imported definition share one document
    /// version. Inference must include top-level placements, not just symbols.
    fn document_version(&self) -> u32 {
        if self
            .symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .flat_map(|layer| &layer.keyframes)
            .any(|key| !key.motion_tracks.is_empty() || key.motion_keys.is_some())
        {
            return 10;
        }
        if self
            .symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .any(|layer| {
                !layer.properties.blend_mode.is_normal()
                    || layer.properties.sync_timeline
                    || layer.keyframes.iter().any(|key| {
                        key.motion_path.is_some()
                            || (key.snap_to_guide && key.tween_kind == TweenKind::Classic)
                    })
            })
        {
            return 8;
        }
        if self
            .elements
            .iter()
            .chain(self.symbols.iter().flat_map(symbol_elements))
            .any(|element| {
                element.transform.pivot.is_some()
                    || element.transform.primitive.is_some()
                    || !element.visible
                    || element.cache_as_bitmap
                    || !element.blend_mode.is_normal()
                    || !element.filters.is_empty()
            })
            || self
                .symbols
                .iter()
                .flat_map(|symbol| &symbol.timeline.layers)
                .flat_map(|layer| &layer.keyframes)
                .any(|key| {
                    matches!(key.tween, Some(Easing::Bounce | Easing::Elastic))
                        || !key.property_eases.is_empty()
                        || key.sync_symbols
                        || key.snap_to_guide
                        || key.sound.as_ref().is_some_and(|s| {
                            s.effect == SoundEffect::Custom
                                || s.envelope.is_some()
                                || s.in_point.is_some()
                                || s.out_point.is_some()
                        })
                })
        {
            7
        } else if self.symbols.iter().any(|symbol| {
            !symbol.timeline.guides.is_empty()
                || symbol.timeline.layers.iter().any(|layer| {
                    layer.properties.motion_guide.is_some()
                        || layer.keyframes.iter().any(|k| {
                            !k.tween_kind.is_classic()
                                || k.keyed_properties.is_some()
                                || k.orient_to_path
                                || k.sound.is_some()
                        })
                })
        }) {
            Guide::FORMAT_VERSION
        } else if self
            .elements
            .iter()
            .chain(self.symbols.iter().flat_map(symbol_elements))
            .any(|element| {
                element.transform.skew != 0.
                    || !element.transform.color_transform.is_identity()
                    || matches!(element.kind, ElementKind::PaintedPath { .. })
            })
            || self
                .symbols
                .iter()
                .flat_map(|symbol| &symbol.timeline.layers)
                .flat_map(|layer| &layer.keyframes)
                .any(|key| matches!(key.tween, Some(Easing::Flash { .. })))
            || self
                .symbols
                .iter()
                .flat_map(|symbol| &symbol.timeline.layers)
                .any(|layer| layer.properties.end_frame.is_some())
        {
            5
        } else if !self.groups.is_empty()
            || self.symbols.iter().any(|symbol| {
                !symbol.folder.is_empty()
                    || symbol
                        .timeline
                        .layers
                        .iter()
                        .any(|layer| !layer.properties.is_default())
            })
        {
            4
        } else if !self.resources.is_empty() {
            3
        } else if !self.sources.is_empty()
            || self
                .symbols
                .iter()
                .any(|symbol| symbol.class_name.is_some() || symbol.linkage_id.is_some())
        {
            2
        } else {
            1
        }
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
        if layer.properties.role == LayerRole::Folder
            || crate::layer_effectively_locked(&symbol.timeline, layer)
            || !crate::layer_effectively_visible(&symbol.timeline, layer)
        {
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
                let Some(source_closure) =
                    canonical_symbol_closure(id, &self.symbols, &self.resources)
                else {
                    return invalid(
                        "captured exported symbol closure exceeds its dependency budget",
                    );
                };
                let Some(destination_closure) = canonical_symbol_closure(
                    &existing.id,
                    &destination.symbols,
                    &destination.resources,
                ) else {
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
                for group in &layer.properties.groups {
                    allocate_identity(
                        &group.id,
                        "group",
                        same_session,
                        &mut identities,
                        &mut occupied,
                        &forbidden,
                    );
                }
                for key in &layer.keyframes {
                    for arm in &key.ik_armatures {
                        allocate_identity(
                            &arm.id,
                            "ik",
                            same_session,
                            &mut identities,
                            &mut occupied,
                            &forbidden,
                        );
                    }
                    for element in &key.elements {
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
        }
        let mut candidate = destination.clone();
        candidate.format_version = candidate.format_version.max(self.document_version());
        for resource in &self.resources {
            if let Some(existing) = candidate
                .resources
                .iter()
                .find(|existing| existing.path.to_lowercase() == resource.path.to_lowercase())
            {
                // Both documents were validated above, so canonical base64
                // equality is byte equality without another decoded allocation.
                if existing.media_type != resource.media_type
                    || (!same_session
                        && (existing.path != resource.path
                            || existing.data_base64 != resource.data_base64))
                {
                    return invalid(
                        "a resource path conflicts with different destination spelling, media type or bytes; resolve the resource conflict before pasting",
                    );
                }
                identities.insert(resource.id.clone(), existing.id.clone());
            } else {
                let mut imported = resource.clone();
                imported.id = fresh_id("resource", &mut occupied, &forbidden);
                identities.insert(resource.id.clone(), imported.id.clone());
                candidate.resources.push(imported);
            }
        }
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
                layer.properties.parent = layer
                    .properties
                    .parent
                    .as_ref()
                    .map(|id| identities[id].clone());
                layer.properties.mask = layer
                    .properties
                    .mask
                    .as_ref()
                    .map(|id| identities[id].clone());
                layer.properties.motion_guide = layer
                    .properties
                    .motion_guide
                    .as_ref()
                    .map(|id| identities[id].clone());
                layer.properties.linked_layer = layer
                    .properties
                    .linked_layer
                    .as_ref()
                    .map(|id| identities[id].clone());
                for group in &mut layer.properties.groups {
                    if let Some(gid) = identities.get(&group.id) {
                        group.id = gid.clone();
                    }
                    for member in &mut group.members {
                        *member = identities[member].clone();
                    }
                }
                for key in &mut layer.keyframes {
                    remap_symbol_keyframe_references(key, &identities);
                }
            }
            candidate
                .add_library_folder(&imported.folder)
                .map_err(ClipboardError::Invalid)?;
            candidate.symbols.push(imported);
        }
        if candidate
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|l| !l.properties.is_default())
        {
            candidate.format_version = candidate.format_version.max(4);
        }
        if candidate.has_program() {
            candidate.enable_program();
        }
        if !candidate.resources.is_empty() {
            candidate.format_version = candidate.format_version.max(3);
        }
        let mut pasted = self.elements.clone();
        let mut element_ids = Vec::with_capacity(pasted.len());
        let mut placement_ids = HashMap::new();
        for element in &mut pasted {
            let old = element.id.clone();
            element.id = fresh_id("object", &mut occupied, &forbidden);
            placement_ids.insert(old, element.id.clone());
            element_ids.push(element.id.clone());
            remap_reference(element, &identities);
            element.transform.x += offset[0];
            element.transform.y += offset[1];
        }
        let fps = f64::from(candidate.fps);
        let timeline = &mut candidate.symbol_mut(symbol_id).unwrap().timeline;
        let layer_idx = timeline
            .layers
            .iter()
            .position(|l| l.id == layer_id)
            .unwrap();
        if !timeline.layers[layer_idx]
            .keyframes
            .iter()
            .any(|k| k.frame == frame)
        {
            let elements =
                sample_layer_in_at_fps(timeline, &timeline.layers[layer_idx], frame, fps);
            let layer = &mut timeline.layers[layer_idx];
            let active = layer
                .keyframes
                .iter()
                .filter(|k| k.frame < frame)
                .max_by_key(|k| k.frame);
            let (tween, tween_kind, orient_to_path) = active
                .filter(|k| {
                    k.tween.is_some()
                        && matches!(k.tween_kind, TweenKind::Classic | TweenKind::Shape)
                })
                .map_or((None, TweenKind::Classic, false), |k| {
                    (k.tween, k.tween_kind, k.orient_to_path)
                });
            layer.keyframes.push(Keyframe {
                frame,
                elements,
                tween,
                tween_kind,
                orient_to_path,
                label: String::new(),
                ..Default::default()
            });
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        let layer = &mut timeline.layers[layer_idx];
        let dest_key = layer
            .keyframes
            .iter_mut()
            .find(|k| k.frame == frame)
            .unwrap();
        dest_key.elements.extend(pasted);
        for original_bf in &self.bitmap_fills {
            if let Some(new_eid) = placement_ids.get(&original_bf.element_id) {
                let mut bf = original_bf.clone();
                bf.element_id = new_eid.clone();
                if let Some(new_rid) = identities.get(&bf.resource_id) {
                    bf.resource_id = new_rid.clone();
                }
                dest_key.set_bitmap_fill(bf);
            }
        }
        for original_app in &self.appearances {
            if let Some(new_eid) = placement_ids.get(&original_app.element_id) {
                let mut app = original_app.clone();
                app.element_id = new_eid.clone();
                dest_key.set_appearance(app);
            }
        }
        if let Some(original_video) = &self.video
            && dest_key.video.is_none()
            && dest_key.keyed_properties.is_none()
        {
            let mut video = original_video.clone();
            if let Some(new_rid) = identities.get(&video.resource) {
                video.resource = new_rid.clone();
            }
            dest_key.video = Some(video);
        }
        layer.normalize_motion_keys();
        for original in &self.groups {
            let mut group = original.clone();
            group.id = fresh_id("group", &mut occupied, &forbidden);
            for member in &mut group.members {
                *member = placement_ids[member].clone();
            }
            layer.properties.groups.push(group);
        }
        if !self.groups.is_empty() {
            candidate.format_version = candidate.format_version.max(4);
        }
        let uses_v5 = candidate.symbols.iter().any(|s| {
            s.timeline.layers.iter().any(|l| {
                l.properties.end_frame.is_some()
                    || l.keyframes.iter().any(|k| {
                        matches!(k.tween, Some(Easing::Flash { .. }))
                            || k.property_eases
                                .iter()
                                .any(|(_, ease)| matches!(ease, Easing::Flash { .. }))
                            || k.elements.iter().any(|e| {
                                e.transform.skew != 0.0
                                    || !e.transform.color_transform.is_identity()
                                    || matches!(e.kind, ElementKind::PaintedPath { .. })
                            })
                    })
            })
        });
        if uses_v5 {
            candidate.format_version = candidate.format_version.max(5);
        }
        candidate.upgrade_format_for_features();
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
            folder: String::new(),
            class_name: None,
            linkage_id: None,
            id: wrapper_id.clone(),
            name: "Clipboard selection".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                guides: vec![],
                frame_count: 1,
                scripts: vec![],
                layers: vec![Layer {
                    properties: LayerProperties {
                        groups: self.groups.clone(),
                        ..Default::default()
                    },
                    id: wrapper_layer,
                    name: "Selection".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: self.elements.clone(),
                        bitmap_fills: self.bitmap_fills.clone(),
                        appearances: self.appearances.clone(),
                        video: self.video.clone(),
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    }],
                }],
            },
        });
        let mut synthetic = Project {
            library_folders: self
                .symbols
                .iter()
                .map(|symbol| symbol.folder.clone())
                .filter(|folder| !folder.is_empty())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            sources: self.sources.clone(),
            resources: self.resources.clone(),
            document_class: None,
            scenes: vec![],
            script_language: None,
            also_compile_compatible_scripts: true,
            format_version: self.document_version(),
            name: "Clipboard validation".into(),
            width: 1,
            height: 1,
            fps: 1,
            background: [0; 4],
            root: wrapper_id,
            symbols,
        };
        synthetic.upgrade_format_for_features();
        validate_project(&synthetic, "captured content")
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

fn floor_char_boundary_at(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn find_svg_opening_tag_end(svg: &str) -> Option<(usize, bool)> {
    let head = &svg[..floor_char_boundary_at(svg, 65_536)];
    let lower = head.to_ascii_lowercase();
    let mut search = 0usize;
    let svg_start = loop {
        let pos = lower[search..].find("<svg")? + search;
        let next = head.as_bytes().get(pos + 4).copied()?;
        if next.is_ascii_whitespace() || next == b'>' || next == b'/' {
            break pos;
        }
        search = pos + 4;
    };
    let bytes = svg.as_bytes();
    let mut i = svg_start + 4;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'>' {
            let self_closing = i > svg_start + 4 && bytes[i - 1] == b'/';
            return Some((i + 1, self_closing));
        }
        i += 1;
    }
    None
}

fn decode_bounded_svg_base64(raw: &str, max_bytes: usize) -> Result<Vec<u8>, ClipboardError> {
    let max_encoded = (max_bytes / 3 + 1) * 4 + 4096;
    if raw.len() > max_encoded {
        return Err(ClipboardError::TooLarge);
    }
    let sextet = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    };
    let cap = (raw.len() / 4 * 3).min(max_bytes).min(256 * 1024);
    let mut out = Vec::with_capacity(cap);
    let mut quantum = [0u8; 4];
    let mut qlen = 0usize;
    let mut saw_pad = false;
    for &b in raw.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            saw_pad = true;
            continue;
        }
        if saw_pad {
            return invalid("invalid padding inside NeoFlash SVG clipboard base64");
        }
        let Some(v) = sextet(b) else {
            return invalid("invalid base64 character in NeoFlash SVG clipboard comment");
        };
        quantum[qlen] = v;
        qlen += 1;
        if qlen == 4 {
            if out.len().saturating_add(3) > max_bytes {
                return Err(ClipboardError::TooLarge);
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
            out.push((quantum[2] << 6) | quantum[3]);
            qlen = 0;
        }
    }
    match qlen {
        0 => {}
        1 => return invalid("truncated base64 quantum in NeoFlash SVG clipboard comment"),
        2 => {
            if out.len().saturating_add(1) > max_bytes {
                return Err(ClipboardError::TooLarge);
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
        }
        3 => {
            if out.len().saturating_add(2) > max_bytes {
                return Err(ClipboardError::TooLarge);
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
        }
        _ => unreachable!(),
    }
    if out.is_empty() {
        return invalid("empty NeoFlash SVG clipboard comment");
    }
    Ok(out)
}

fn extract_svg_neoflash_payload_text(text: &str) -> Result<Option<String>, ClipboardError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with('<') {
        return Ok(None);
    }
    let head = &trimmed[..floor_char_boundary_at(trimmed, 65_536)];
    let Some(open_idx) = head.find(ClipboardPayload::SVG_COMMENT_OPEN) else {
        return Ok(None);
    };
    if !head[..open_idx].to_ascii_lowercase().contains("<svg") {
        return Ok(None);
    }
    let content_start = open_idx + ClipboardPayload::SVG_COMMENT_OPEN.len();
    let max_encoded = (MAX_CLIPBOARD_BYTES / 3 + 1) * 4 + 4096;
    let search_limit = floor_char_boundary_at(
        trimmed,
        content_start
            .saturating_add(max_encoded)
            .saturating_add(ClipboardPayload::SVG_COMMENT_CLOSE.len()),
    );
    let Some(rel_close) =
        trimmed[content_start..search_limit].find(ClipboardPayload::SVG_COMMENT_CLOSE)
    else {
        return Err(
            if trimmed.len().saturating_sub(content_start) > max_encoded {
                ClipboardError::TooLarge
            } else {
                ClipboardError::Invalid("unterminated NeoFlash SVG clipboard comment".into())
            },
        );
    };
    let b64_slice = &trimmed[content_start..content_start + rel_close];
    let bytes = decode_bounded_svg_base64(b64_slice, MAX_CLIPBOARD_BYTES)?;
    if bytes.len() > MAX_CLIPBOARD_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    let decoded = String::from_utf8(bytes).map_err(|_| {
        ClipboardError::Invalid("NeoFlash SVG clipboard comment is not UTF-8".into())
    })?;
    Ok(Some(decoded))
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
fn canonical_symbol_closure(
    root: &str,
    symbols: &[Symbol],
    resources: &[ResourceUnit],
) -> Option<CanonicalClosure> {
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
            .chain(
                symbol
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| k.ik_armatures.iter().map(|a| &a.id)),
            )
            .chain(symbol_elements(symbol).map(|e| &e.id))
        {
            let next = identities.len();
            identities
                .entry(id.clone())
                .or_insert_with(|| format!("entity-{next}"));
        }
    }
    for resource in resources {
        identities.insert(resource.id.clone(), format!("resource:{}", resource.path));
    }
    let ids = ordered.iter().map(|symbol| symbol.id.clone()).collect();
    Some(CanonicalClosure {
        ids,
        symbols: ordered
            .into_iter()
            .map(|symbol| {
                let mut result = symbol.clone();
                // Library organization does not change reusable runtime content.
                result.folder.clear();
                result.id = identities[&result.id].clone();
                for layer in &mut result.timeline.layers {
                    layer.id = identities[&layer.id].clone();
                    // Selection groups do not affect reusable runtime content.
                    layer.properties.groups.clear();
                    layer.properties.parent = layer
                        .properties
                        .parent
                        .as_ref()
                        .map(|id| identities[id].clone());
                    layer.properties.mask = layer
                        .properties
                        .mask
                        .as_ref()
                        .map(|id| identities[id].clone());
                    layer.properties.motion_guide = layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .map(|id| identities[id].clone());
                    layer.properties.linked_layer = layer
                        .properties
                        .linked_layer
                        .as_ref()
                        .map(|id| identities[id].clone());
                    for key in &mut layer.keyframes {
                        remap_symbol_keyframe_references(key, &identities);
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
                commands = commands.saturating_add(key.motion_path.as_ref().map_or(0, Vec::len));
            }
        }
    }
    for element in payload
        .elements
        .iter()
        .chain(payload.symbols.iter().flat_map(symbol_elements))
    {
        scripts = scripts.saturating_add(element.scripts.len());
        if let ElementKind::Path { commands: path, .. }
        | ElementKind::PaintedPath { commands: path, .. } = &element.kind
        {
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
        ids.extend(layer.properties.groups.iter().map(|g| g.id.clone()));
        for key in &layer.keyframes {
            ids.extend(key.ik_armatures.iter().map(|a| a.id.clone()));
            ids.extend(key.elements.iter().map(|e| e.id.clone()));
        }
    }
}
fn project_ids(project: &Project) -> HashSet<Id> {
    let mut ids: HashSet<Id> = project
        .sources
        .iter()
        .map(|source| source.id.clone())
        .collect();
    ids.extend(project.resources.iter().map(|resource| resource.id.clone()));
    for symbol in &project.symbols {
        symbol_ids(symbol, &mut ids);
    }
    ids
}
fn payload_ids(payload: &ClipboardPayload) -> HashSet<Id> {
    let mut ids: HashSet<_> = payload.elements.iter().map(|e| e.id.clone()).collect();
    ids.extend(payload.groups.iter().map(|g| g.id.clone()));
    ids.extend(payload.sources.iter().map(|source| source.id.clone()));
    ids.extend(payload.resources.iter().map(|resource| resource.id.clone()));
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
    let id = match &mut element.kind {
        ElementKind::Instance { symbol_id, .. } => symbol_id,
        ElementKind::Bitmap { resource_id, .. } => resource_id,
        _ => return,
    };
    if let Some(mapped) = map.get(id) {
        *id = mapped.clone();
    }
}

fn remap_symbol_keyframe_references(key: &mut Keyframe, map: &HashMap<Id, Id>) {
    if let Some(sound) = &mut key.sound
        && let Some(rid) = map.get(&sound.resource)
    {
        sound.resource = rid.clone();
    }
    if let Some(video) = &mut key.video
        && let Some(rid) = map.get(&video.resource)
    {
        video.resource = rid.clone();
    }
    remap_motion_element_references(key, map);
    for ov in &mut key.instance_overrides {
        if let Some(eid) = map.get(&ov.instance_id) {
            ov.instance_id = eid.clone();
        }
    }
    for bf in &mut key.bitmap_fills {
        if let Some(eid) = map.get(&bf.element_id) {
            bf.element_id = eid.clone();
        }
        if let Some(rid) = map.get(&bf.resource_id) {
            bf.resource_id = rid.clone();
        }
    }
    for warp in &mut key.asset_warps {
        if let Some(eid) = map.get(&warp.element_id) {
            warp.element_id = eid.clone();
        }
    }
    for pl in &mut key.element_parents {
        if let Some(eid) = map.get(&pl.child_id) {
            pl.child_id = eid.clone();
        }
        if let Some(pid) = map.get(&pl.parent_id) {
            pl.parent_id = pid.clone();
        }
    }
    for cid in &mut key.controller_elements {
        if let Some(eid) = map.get(cid) {
            *cid = eid.clone();
        }
    }
    for arm in &mut key.ik_armatures {
        if let Some(aid) = map.get(&arm.id) {
            arm.id = aid.clone();
        }
        for bone in &mut arm.bones {
            if let Some(eid) = map.get(&bone.element_id) {
                bone.element_id = eid.clone();
            }
        }
    }
    for app in &mut key.appearances {
        if let Some(eid) = map.get(&app.element_id) {
            app.element_id = eid.clone();
        }
    }
    for acc in &mut key.extended_accessibility {
        if let Some(eid) = map.get(&acc.element_id) {
            acc.element_id = eid.clone();
        }
    }
    for fx in &mut key.vector_effects {
        if let Some(eid) = map.get(&fx.element_id) {
            fx.element_id = eid.clone();
        }
        if let Some(blend) = &mut fx.vector_blend
            && let Some(tid) = map.get(&blend.target_element_id)
        {
            blend.target_element_id = tid.clone();
        }
    }
    for element in &mut key.elements {
        if let Some(eid) = map.get(&element.id) {
            element.id = eid.clone();
        }
        remap_reference(element, map);
    }
}

fn remap_motion_element_references(key: &mut Keyframe, map: &HashMap<Id, Id>) {
    for track in &mut key.motion_tracks {
        if let Some(eid) = map.get(&track.element_id) {
            track.element_id = eid.clone();
        }
    }
    if let Some(object_keys) = &mut key.motion_keys {
        for object_key in object_keys {
            if let Some(eid) = map.get(&object_key.element_id) {
                object_key.element_id = eid.clone();
            }
        }
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
                } else if !((key == "accessibility"
                    && value.is_null()
                    && b.contains_key("transform")
                    && b.contains_key("kind"))
                    || (key == "folder"
                        && value.as_str() == Some("")
                        && b.contains_key("timeline")))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([220, 40, 60, 255]));
        let mut bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        bytes
    }

    #[test]
    fn capture_and_cross_session_paste_remap_format_9_side_tables_and_stage_bitmap_fills() {
        let mut src = Project::blank();
        let png = tiny_png();
        src.resources.push(
            ResourceUnit::from_bytes("bmp-res-a", "bitmaps/tile.png", "image/png", &png).unwrap(),
        );
        src.resources.push(
            ResourceUnit::from_bytes("vid-res-a", "video/clip.mp4", "video/mp4", b"fake-mp4-data")
                .unwrap(),
        );

        let shape_a = Element {
            id: "inner-shape".into(),
            name: "TileShape".into(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 20.0, y: 0.0 },
                    PathCommand::LineTo { x: 20.0, y: 20.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::NonZero,
                fill: [255, 255, 255, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let shape_b = Element {
            id: "inner-child".into(),
            name: "ChildShape".into(),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [20, 120, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let inner_kf = Keyframe {
            frame: 0,
            elements: vec![shape_a, shape_b],
            video: Some(TimelineVideoSpan {
                resource: "vid-res-a".into(),
                ..TimelineVideoSpan::default()
            }),
            bitmap_fills: vec![AuthoredBitmapFill {
                element_id: "inner-shape".into(),
                resource_id: "bmp-res-a".into(),
                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                repeat: true,
                smoothing: true,
            }],
            asset_warps: vec![AssetWarpRig {
                element_id: "inner-shape".into(),
                pins: vec![WarpPin {
                    id: "pin-1".into(),
                    kind: WarpPinKind::Open,
                    rest_pos: [5.0, 5.0],
                    pos: [6.0, 7.0],
                    rotation: 0.0,
                    stiffness: 1.0,
                }],
                envelop_subdivisions: 4,
            }],
            element_parents: vec![ElementParentLink {
                child_id: "inner-child".into(),
                parent_id: "inner-shape".into(),
            }],
            appearances: vec![ElementAppearance {
                element_id: "inner-shape".into(),
                style_id: Some("style-1".into()),
                ..ElementAppearance::default()
            }],
            vector_effects: vec![ElementVectorEffects {
                element_id: "inner-shape".into(),
                vector_blend: Some(VectorBlendSpec {
                    target_element_id: "inner-child".into(),
                    spacing: VectorBlendSpacing::SpecifiedSteps(4),
                    align_to_path: false,
                    spine: None,
                }),
                ..ElementVectorEffects::default()
            }],
            ..Keyframe::default()
        };
        src.symbols.push(Symbol {
            id: "sym-rich".into(),
            name: "RichClip".into(),
            folder: "UI".into(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "layer-rich".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        scale9_grid: Some([2.0, 2.0, 16.0, 16.0]),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![inner_kf],
                }],
                scripts: vec![],
                guides: vec![],
            },
        });
        src.library_folders.push("UI".into());

        let stage_shape = Element {
            id: "stage-filled".into(),
            name: "StageFilled".into(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 30.0, y: 0.0 },
                    PathCommand::LineTo { x: 30.0, y: 30.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::NonZero,
                fill: [255, 255, 255, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let stage_inst = Element {
            id: "stage-inst".into(),
            name: "PlacedRich".into(),
            kind: ElementKind::Instance {
                symbol_id: "sym-rich".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        };
        let root_kf = &mut src.symbols[0].timeline.layers[0].keyframes[0];
        root_kf.elements = vec![stage_shape.clone(), stage_inst.clone()];
        root_kf.bitmap_fills.push(AuthoredBitmapFill {
            element_id: "stage-filled".into(),
            resource_id: "bmp-res-a".into(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            repeat: false,
            smoothing: true,
        });
        root_kf.appearances.push(ElementAppearance {
            element_id: "stage-filled".into(),
            style_id: Some("style-stage".into()),
            ..ElementAppearance::default()
        });
        src.upgrade_format_for_features();
        src.validate().unwrap();

        let root_id = src.root.clone();
        let payload = ClipboardPayload::capture_at_frame(
            &src,
            &root_id,
            0,
            vec![stage_shape, stage_inst],
            "tab-a",
            [0.0, 0.0],
        )
        .unwrap();
        let roundtripped = ClipboardPayload::from_text(&payload.to_text().unwrap())
            .unwrap()
            .unwrap();

        let dst = Project::blank();
        let dst_root = dst.root.clone();
        let dst_layer = dst.symbols[0].timeline.layers[0].id.clone();
        let res = roundtripped
            .paste_into(&dst, "tab-b", &dst_root, &dst_layer, 0, [12.0, 8.0])
            .unwrap();
        res.project.validate().unwrap();

        let dst_kf = &res.project.symbols[0].timeline.layers[0].keyframes[0];
        assert_eq!(dst_kf.bitmap_fills.len(), 1);
        assert_eq!(dst_kf.bitmap_fills[0].element_id, res.element_ids[0]);
        assert_ne!(dst_kf.bitmap_fills[0].resource_id, "bmp-res-a");
        assert_eq!(dst_kf.appearances.len(), 1);
        assert_eq!(dst_kf.appearances[0].element_id, res.element_ids[0]);

        let imported_sym = res
            .project
            .symbols
            .iter()
            .find(|s| s.name == "RichClip")
            .unwrap();
        assert_eq!(
            imported_sym.timeline.layers[0].properties.scale9_grid,
            Some([2.0, 2.0, 16.0, 16.0])
        );
        let ikf = &imported_sym.timeline.layers[0].keyframes[0];
        assert_ne!(ikf.video.as_ref().unwrap().resource, "vid-res-a");
        assert_eq!(ikf.bitmap_fills[0].element_id, ikf.elements[0].id);
        assert_eq!(ikf.asset_warps[0].element_id, ikf.elements[0].id);
        assert_eq!(ikf.element_parents[0].child_id, ikf.elements[1].id);
        assert_eq!(ikf.element_parents[0].parent_id, ikf.elements[0].id);
        assert_eq!(ikf.vector_effects[0].element_id, ikf.elements[0].id);
        assert_eq!(
            ikf.vector_effects[0]
                .vector_blend
                .as_ref()
                .unwrap()
                .target_element_id,
            ikf.elements[1].id
        );

        // Folder layer and child layer under a locked Folder must be rejected by `paste_into`.
        let mut locked_folder_dst = res.project.clone();
        locked_folder_dst.format_version = locked_folder_dst.format_version.max(4);
        let mut folder_layer = Layer {
            id: "folder-locked".into(),
            name: "Folder".into(),
            visible: true,
            locked: false,
            keyframes: vec![],
            properties: LayerProperties {
                role: LayerRole::Folder,
                ..LayerProperties::default()
            },
        };
        let child_under_locked = Layer {
            id: "child-unlocked".into(),
            name: "Child".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe::default()],
            properties: LayerProperties {
                parent: Some("folder-locked".into()),
                ..LayerProperties::default()
            },
        };
        let mut unlocked_folder_dst = locked_folder_dst.clone();
        unlocked_folder_dst.symbols[0]
            .timeline
            .layers
            .push(folder_layer.clone());
        unlocked_folder_dst.validate().unwrap();
        assert!(
            roundtripped
                .paste_into(
                    &unlocked_folder_dst,
                    "tab-b",
                    &dst_root,
                    "folder-locked",
                    0,
                    [0.0, 0.0],
                )
                .is_err(),
            "paste_into must reject pasting directly onto a Folder layer"
        );

        folder_layer.locked = true;
        locked_folder_dst.symbols[0]
            .timeline
            .layers
            .push(folder_layer);
        locked_folder_dst.symbols[0]
            .timeline
            .layers
            .push(child_under_locked);
        locked_folder_dst.validate().unwrap();
        assert!(
            roundtripped
                .paste_into(
                    &locked_folder_dst,
                    "tab-b",
                    &dst_root,
                    "child-unlocked",
                    0,
                    [0.0, 0.0],
                )
                .is_err(),
            "paste_into must reject child layer whose ancestor Folder is locked"
        );
    }

    #[test]
    fn svg_embedded_neoflash_comment_roundtrips_and_enforces_byte_budget() {
        let mut src = Project::blank();
        let el = Element {
            id: "hero-path".into(),
            name: "HeroStar".into(),
            transform: Transform {
                x: 42.5,
                y: 87.25,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 64.0,
                height: 48.0,
                fill: [240, 80, 40, 255],
                stroke: [20, 30, 40, 255],
                stroke_width: 3.0,
            },
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('lossless');".into(),
                },
            }],
            ..Element::default()
        };
        src.symbols[0].timeline.layers[0].keyframes[0].elements = vec![el.clone()];
        let payload =
            ClipboardPayload::capture(&src, vec![el], "sess-svg-1", [74.5, 111.25]).unwrap();
        let raw_text = payload.to_text().unwrap();

        let clean_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48" viewBox="42.5 87.25 64 48"><rect x="42.5" y="87.25" width="64" height="48" fill="#f05028"/></svg>"##;
        let tagged_svg = ClipboardPayload::embed_comment_in_svg(clean_svg, &raw_text).unwrap();
        assert!(tagged_svg.contains("<!--(neoflash)"));
        assert!(tagged_svg.contains("(/neoflash)-->"));

        // Clean SVG without comment returns Ok(None) so SVG importer handles it.
        assert!(ClipboardPayload::from_text(clean_svg).unwrap().is_none());

        // Tagged SVG decodes losslessly back into the exact ClipboardPayload!
        let decoded = ClipboardPayload::from_text(&tagged_svg)
            .unwrap()
            .expect("tagged SVG must decode NeoFlash payload");
        assert_eq!(decoded.source_session(), "sess-svg-1");
        assert_eq!(decoded.anchor(), [74.5, 111.25]);
        assert_eq!(decoded.elements().len(), 1);
        assert_eq!(decoded.elements()[0].scripts.len(), 1);

        // Self-closing <svg .../> also embeds well-formed comment + </svg>.
        let self_closing = ClipboardPayload::embed_comment_in_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#,
            &raw_text,
        )
        .unwrap();
        assert!(self_closing.ends_with("</svg>"));
        assert!(
            ClipboardPayload::from_text(&self_closing)
                .unwrap()
                .is_some()
        );

        // Malformed base64 or unterminated comment inside <svg> fails with Invalid.
        assert!(ClipboardPayload::from_text("<svg><!--(neoflash)%%%(/neoflash)--></svg>").is_err());
        assert!(ClipboardPayload::from_text("<svg><!--(neoflash)YWJj</svg>").is_err());

        // Oversized base64 payload (> MAX_CLIPBOARD_BYTES) fails with TooLarge.
        let huge_b64 = "AAAA".repeat(MAX_CLIPBOARD_BYTES / 3 + 16);
        let oversized_svg = format!("<svg><!--(neoflash){huge_b64}(/neoflash)--></svg>");
        assert!(matches!(
            ClipboardPayload::from_text(&oversized_svg),
            Err(ClipboardError::TooLarge)
        ));
    }
}
