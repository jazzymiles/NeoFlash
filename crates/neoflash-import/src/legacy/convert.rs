//! Native binary authoring import. Source records remain embedded verbatim;
//! unsupported visual or executable records stop conversion with a location.
mod assets;
mod shape;
#[cfg(test)]
mod tests;
mod text;

use super::{BinaryTimelineDocument, contents::*, timeline::*};
use crate::ImportReport;
use neoflash_core::*;
use std::collections::BTreeMap;

pub(super) fn read_string(bytes: &[u8], value: &ContentsString) -> Result<String, String> {
    let raw = bytes
        .get(value.data.start..value.data.end)
        .ok_or("String span outside source")?;
    if value.utf16 {
        if raw.len() % 2 != 0 {
            return Err("Odd UTF-16 byte count".into());
        }
        String::from_utf16(
            &raw.as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid UTF-16 source".into())
    } else if raw.is_ascii() {
        Ok(String::from_utf8(raw.to_vec()).unwrap())
    } else {
        Err("ANSI text needs an explicit source code page; original bytes are retained".into())
    }
}
fn sid(index: usize) -> String {
    format!("fla-symbol-{index}")
}
fn lid(index: usize, id: u32) -> String {
    format!("fla-layer-{index}-{id}")
}
fn object(stream: &BinaryTimelineStream, id: u32) -> Result<&TimelineObject, String> {
    stream
        .objects
        .binary_search_by_key(&id, |o| o.object_id)
        .ok()
        .map(|i| &stream.objects[i])
        .ok_or_else(|| format!("Unresolved object #{id}"))
}
fn optional_text(bytes: &[u8], span: Option<&ContentsString>) -> Result<String, String> {
    span.map(|v| read_string(bytes, v))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn convert(
    document: &BinaryTimelineDocument,
    report: &mut ImportReport,
) -> Option<Project> {
    report.binary_fla = Some(document.inspection.clone());
    for s in &document.inspection.streams {
        if let Some(code) = &s.stop_code
            && s.role != super::BinaryStreamRole::Media
        {
            report.error(
                code,
                format!("CFB stream #{} @0x{:x}", s.index, s.bytes_consumed),
                s.stop_message
                    .as_deref()
                    .unwrap_or("Incomplete binary stream"),
            );
        }
    }
    match build(document, report) {
        Ok(project) => {
            if let Some(binary) = &mut report.binary_fla {
                for stream in &mut binary.streams {
                    if stream.role == super::BinaryStreamRole::Media {
                        stream.stop_code = None;
                        stream.stop_message = None;
                        stream.bytes_consumed = stream.byte_length as usize;
                        stream.structure_complete = true;
                    }
                }
            }
            Some(project)
        }
        Err(message) => {
            report.error("fla_semantic_conversion", "Binary FLA", message);
            None
        }
    }
}

fn build(document: &BinaryTimelineDocument, report: &mut ImportReport) -> Result<Project, String> {
    let c = &document.contents;
    if !matches!(c.revision, Some(0x29 | 0x47)) {
        return Err(
            "Editable binary import currently supports the MX and CS4 layout profiles".into(),
        );
    }
    if c.extensions.as_ref().is_some_and(|e| e.profiles.len() != 1) {
        return Err("An unambiguous single publish profile is required".into());
    }
    if c.stop.is_some()
        || c.bytes_consumed != c.original_bytes.len()
        || document
            .streams
            .iter()
            .any(|s| s.objects.iter().any(|o| !o.complete))
    {
        return Err(
            "Complete Contents and timeline records are required before editable conversion".into(),
        );
    }
    if c.scenes.len() != 1 {
        return Err(
            "This import profile requires one scene; multiple scenes remain in the original source"
                .into(),
        );
    }
    let root_descriptor = c
        .descriptors
        .iter()
        .find(|d| d.object_id == c.scenes[0])
        .ok_or("Missing scene descriptor")?;
    let root_index = root_descriptor.stream_index.ok_or("Missing scene stream")?;
    let settings = c.settings.as_ref().ok_or("Missing document settings")?;
    let size = settings.size_twips().ok_or("Invalid stage bounds")?;
    if settings.bounds_words[0] != 0 || settings.bounds_words[2] != 0 {
        return Err("A nonzero stage bounds origin requires an explicit coordinate adapter".into());
    }
    if size.iter().any(|s| s % 20 != 0) || settings.frame_rate().fract() != 0. {
        return Err("Fractional stage pixels or frame rate need a document-model adapter".into());
    }
    let language = match c
        .publish_property(0, "Vector::ActionScriptVersion")
        .as_deref()
    {
        Some("2") => ScriptLanguage::ActionScript2,
        Some("3") => ScriptLanguage::ActionScript3,
        None if c.revision.is_some_and(|r| r <= 0x29) => ScriptLanguage::ActionScript1,
        _ => return Err("ActionScript publishing version is missing or unsupported".into()),
    };
    let include_hidden = match c.publish_property(0, "Vector::Invisible Layer").as_deref() {
        Some("0") => false,
        Some("1") | None => true,
        _ => return Err("Unknown hidden-layer publish flag".into()),
    };
    let (mut resources, bitmaps) = assets::resources(document, report)?;
    let (folders, folder_paths) = assets::folders(c)?;
    let mut library = BTreeMap::new();
    for d in &c.descriptors {
        if let (Some(id), Some(index)) = (d.library_id, d.stream_index)
            && d.object_id != root_descriptor.object_id
            && library.insert(id, sid(index)).is_some()
        {
            return Err("Duplicate library ID".into());
        }
    }
    let mut context = Context {
        report,
        library,
        bitmaps,
        language,
        include_hidden,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let mut symbols = Vec::new();
    for d in &c.descriptors {
        if d.class != ContentsDescriptorClass::DocumentPage {
            continue;
        }
        let index = d.stream_index.ok_or("Library descriptor has no stream")?;
        let stream = document
            .streams
            .iter()
            .find(|s| s.index == index)
            .ok_or("Library timeline is missing")?;
        let name = read_string(&c.original_bytes, &d.name)?;
        let kind = if index == root_index {
            SymbolKind::MovieClip
        } else {
            match d.symbol_type {
                Some(0) => SymbolKind::Graphic,
                Some(1) => SymbolKind::Button,
                Some(2) => SymbolKind::MovieClip,
                _ => return Err(format!("Unsupported symbol type in {name}")),
            }
        };
        let parent = d
            .library_metadata
            .as_ref()
            .and_then(|m| m.parent)
            .filter(|id| *id != [0, 0]);
        let folder = parent
            .map(|id| {
                folder_paths
                    .get(&id)
                    .cloned()
                    .ok_or("Unresolved library folder".to_owned())
            })
            .transpose()?
            .unwrap_or_default();
        let linkage = d
            .linkage
            .as_ref()
            .or_else(|| d.library_metadata.as_ref().and_then(|m| m.linkage.as_ref()));
        let linkage_id = linkage
            .map(|l| read_string(&c.original_bytes, &l.identifier))
            .transpose()?
            .filter(|v| !v.is_empty());
        let class_name = optional_text(
            &c.original_bytes,
            linkage.and_then(|l| l.class_name.as_ref()),
        )?;
        let timeline = context.timeline(stream)?;
        symbols.push(Symbol {
            id: sid(index),
            name,
            folder,
            kind,
            timeline,
            class_name: (!class_name.is_empty()).then_some(class_name),
            linkage_id,
        });
    }
    symbols.append(&mut context.generated);
    resources.push(
        ResourceUnit::from_bytes(
            "fla-original",
            "legacy/source.fla",
            "application/x-neoflash-fla-source",
            &document.original_bytes,
        )
        .map_err(|e| e.to_string())?,
    );
    let metadata = serde_json::json!({"schema":1,"source_resource":"fla-original","authoring_version":document.inspection.authoring_version,"contents_revision":c.revision,"language":language,"publish_hidden_layers":include_hidden,"container_reading_view_offsets":document.inspection.metadata_compatibility_offsets,"descriptors":c.descriptors,"folders":c.extensions.as_ref().map(|e|&e.folders),"placements":context.provenance,"note":"Offsets refer to original logical streams. Source bytes retain authoring properties, editor visibility, guides, metadata and unresolved source references."});
    resources.push(
        ResourceUnit::from_bytes(
            "fla-provenance",
            "legacy/provenance.json",
            "application/json",
            &serde_json::to_vec(&metadata).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?,
    );
    if !document
        .inspection
        .metadata_compatibility_offsets
        .is_empty()
    {
        context.report.warning("fla_container_compatibility","CFB directory","Read compatibility normalized stream timestamps and red/black directory colors in memory. Original FLA and all logical stream bytes are unchanged and retained.")
    }
    let project = Project {
        format_version: 5,
        name: std::path::Path::new(&context.report.source)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width: u32::try_from(size[0] / 20).map_err(|_| "Stage width overflow")?,
        height: u32::try_from(size[1] / 20).map_err(|_| "Stage height overflow")?,
        fps: settings.frame_rate() as u32,
        background: shape::color(settings.background_rgba_word),
        root: sid(root_index),
        symbols,
        library_folders: folders,
        sources: vec![],
        resources,
        document_class: None,
    };
    project
        .validate()
        .map_err(|e| format!("Converted document validation: {e}"))?;
    // Import reserves authored IDs through the same parser as opening .nflash.
    let project = Project::from_json(&project.to_json().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    context.report.statistics.symbols = project.symbols.len();
    for symbol in &project.symbols {
        context.report.statistics.layers += symbol.timeline.layers.len();
        context.report.statistics.scripts += symbol.timeline.scripts.len();
        for layer in &symbol.timeline.layers {
            context.report.statistics.keyframes += layer.keyframes.len();
            for key in &layer.keyframes {
                context.report.statistics.elements += key.elements.len();
                context.report.statistics.scripts +=
                    key.elements.iter().map(|e| e.scripts.len()).sum::<usize>();
            }
        }
    }
    Ok(project)
}

struct Context<'a> {
    report: &'a mut ImportReport,
    library: BTreeMap<u32, String>,
    bitmaps: BTreeMap<u16, assets::BitmapAsset>,
    language: ScriptLanguage,
    include_hidden: bool,
    generated: Vec<Symbol>,
    provenance: Vec<serde_json::Value>,
    missing_bitmaps: BTreeMap<u16, String>,
}
impl Context<'_> {
    fn actions(
        &mut self,
        stream: &BinaryTimelineStream,
        actions: Option<&TimelineActions>,
    ) -> Result<Option<Script>, String> {
        let Some(a) = actions else { return Ok(None) };
        match a.presence {
            TimelineActionPresence::Empty => Ok(None),
            TimelineActionPresence::Source if a.complete => Ok(Some(Script {
                language: self.language,
                source: read_string(
                    &stream.original_bytes,
                    a.source.as_ref().ok_or("Missing action source")?,
                )?,
            })),
            _ => Err(format!(
                "Stream #{} @0x{:x}: compiled/historical action records need a dedicated importer",
                stream.index, a.encoded.start
            )),
        }
    }
    fn timeline(&mut self, s: &BinaryTimelineStream) -> Result<Timeline, String> {
        let page = object(s, s.root.ok_or("Timeline has no root")?)?;
        let mut timeline = Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
        };
        for id in &page.children {
            let o = object(s, *id)?;
            let Some(TimelinePayload::Layer(l)) = &o.payload else {
                return Err("Non-layer page child".into());
            };
            let name = optional_text(&s.original_bytes, l.name.as_ref())?;
            let layer_id = lid(s.index, *id);
            let state = l
                .state_flags
                .and_then(|v| s.original_bytes.get(v.start..v.end));
            let editor_visible = state.map_or(l.legacy_flag != Some(0), |v| v.get(1) == Some(&0));
            let locked = state.map_or(l.legacy_flag == Some(1), |v| {
                v.get(2).is_some_and(|v| *v != 0)
            });
            let role = match l.layer_type.unwrap_or(0) {
                0 | 2 | 5 => LayerRole::Normal,
                1 => LayerRole::Guide,
                3 => LayerRole::Folder,
                4 => LayerRole::Mask,
                _ => return Err(format!("Unsupported layer type in {name}")),
            };
            let related = s
                .pointers
                .iter()
                .find(|p| {
                    p.source == Some(*id)
                        && matches!(
                            p.relation,
                            TimelineRelation::RelatedLayer | TimelineRelation::LegacyRelatedLayer
                        )
                })
                .and_then(|p| p.target);
            let mut properties = LayerProperties {
                role,
                outline: l.outline.unwrap_or(0) != 0,
                outline_color: l.outline_color_word.map(|v| {
                    let b = v.to_le_bytes();
                    [b[0], b[1], b[2]]
                }),
                collapsed: role == LayerRole::Folder && l.mask_or_open == Some(0),
                ..Default::default()
            };
            if let Some(parent) = related {
                if !page.children.contains(&parent) {
                    return Err(format!("Layer {name} parent is outside timeline"));
                }
                let Some(TimelinePayload::Layer(p)) = &object(s, parent)?.payload else {
                    return Err("Layer parent is not a layer".into());
                };
                match p.layer_type {
                    Some(4) => properties.mask = Some(lid(s.index, parent)),
                    Some(3) => properties.parent = Some(lid(s.index, parent)),
                    Some(1) => return Err("Guided motion needs a motion-path adapter".into()),
                    _ => return Err("Unknown layer relationship".into()),
                }
            }
            if l.animation_type.unwrap_or(0) != 0 {
                return Err(format!("Layer {name} uses motion-object or IK animation"));
            }
            if !editor_visible && self.include_hidden {
                self.report.warning("fla_publish_hidden_layer",format!("stream #{} layer {name}",s.index),"Layer is visible according to the source publish profile; its original editor eye toggle is retained in provenance.")
            }
            self.provenance.push(serde_json::json!({"stream":s.index,"object":id,"target":layer_id,"editor_visible":editor_visible,"source":o.encoded}));
            let mut layer = Layer {
                id: layer_id,
                name,
                visible: editor_visible || self.include_hidden,
                locked,
                keyframes: vec![],
                properties,
            };
            let mut cursor = 0u32;
            for frame_id in &o.children {
                let frame = object(s, *frame_id)?;
                let Some(TimelinePayload::Frame(f)) = &frame.payload else {
                    return Err("Layer child is not a frame".into());
                };
                if f.sound_media_id.unwrap_or(0) != 0 {
                    return Err("Timeline sound requires an authoring sound adapter".into());
                }
                if f.rotation_mode.unwrap_or(0) != 0
                    || f.rotation_count.unwrap_or(0) != 0
                    || f.motion_xml
                        .as_ref()
                        .is_some_and(|v| v.data.end > v.data.start)
                {
                    return Err("Directed rotation or motion XML needs a tween adapter".into());
                }
                let mut elements = Vec::new();
                let provenance_start = self.provenance.len();
                for (slot, child) in frame.children.iter().enumerate() {
                    elements.extend(self.element(
                        s,
                        *child,
                        &format!("fla-{}-{}-{}", s.index, id, child),
                        0,
                    )?);
                    self.provenance.push(serde_json::json!({"stream":s.index,"object":child,"layer":layer.id,"frame":cursor,"slot":slot}));
                }
                if let Some(index) = frame.shape_index {
                    elements.extend(shape::convert_shape(
                        &s.shapes[index].record,
                        &format!("fla-{}-{}-shape-{frame_id}", s.index, id),
                        self.report,
                        &format!("stream #{} frame {cursor}", s.index),
                    )?);
                }
                let tween = ease(f)?;
                if let Some(previous) = layer.keyframes.last() {
                    for (a, b) in previous.elements.iter().zip(elements.iter_mut()) {
                        if placement_matches(a, b) {
                            let source_id = std::mem::replace(&mut b.id, a.id.clone());
                            for mapping in &mut self.provenance[provenance_start..] {
                                if mapping["target"].as_str() == Some(source_id.as_str()) {
                                    mapping["target"] = serde_json::json!(b.id);
                                }
                            }
                        }
                    }
                    if previous.tween.is_some()
                        && (previous.elements.len() != 1
                            || elements.len() != 1
                            || previous.elements[0].id != elements[0].id)
                    {
                        return Err(format!(
                            "Layer {} frame {cursor}: motion tween needs one stable symbol placement",
                            layer.name
                        ));
                    }
                }
                if let Some(script) = self.actions(s, f.actions.as_ref())? {
                    timeline.scripts.push(FrameScript {
                        frame: cursor,
                        script,
                    })
                }
                layer.keyframes.push(Keyframe {
                    frame: cursor,
                    elements,
                    tween,
                    label: optional_text(&s.original_bytes, f.label.as_ref())?,
                });
                let duration = u32::from(f.duration.ok_or("Frame duration absent")?);
                if duration == 0 {
                    return Err("Zero frame duration".into());
                }
                cursor = cursor
                    .checked_add(duration)
                    .ok_or("Timeline length overflow")?;
            }
            if layer.keyframes.last().is_some_and(|k| k.tween.is_some()) {
                return Err(format!(
                    "Layer {} ends with an unresolved tween",
                    layer.name
                ));
            }
            timeline.frame_count = timeline.frame_count.max(cursor);
            timeline.layers.push(layer);
        }
        for layer in &mut timeline.layers {
            let source_id = page
                .children
                .iter()
                .find(|id| lid(s.index, **id) == layer.id)
                .copied()
                .ok_or("Missing source layer")?;
            let end = object(s, source_id)?
                .children
                .iter()
                .try_fold(0u32, |total, id| {
                    let Some(TimelinePayload::Frame(frame)) = &object(s, *id)?.payload else {
                        return Err("Invalid frame".to_owned());
                    };
                    total
                        .checked_add(u32::from(frame.duration.unwrap_or(0)))
                        .ok_or("Frame duration overflow".to_owned())
                })?;
            if end < timeline.frame_count && layer.properties.role != LayerRole::Folder {
                layer.keyframes.push(Keyframe {
                    frame: end,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
                });
            }
        }
        Ok(timeline)
    }
    fn element(
        &mut self,
        s: &BinaryTimelineStream,
        object_id: u32,
        id: &str,
        depth: usize,
    ) -> Result<Vec<Element>, String> {
        if depth > 48 {
            return Err("Nested source group limit exceeded".into());
        }
        let o = object(s, object_id)?;
        let location = format!(
            "stream #{} object #{object_id} @0x{:x}",
            s.index, o.encoded.start
        );
        let mut element = Element {
            id: id.into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 0.,
                height: 0.,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            scripts: vec![],
            accessibility: None,
        };
        match &o.payload {
            Some(TimelinePayload::Instance(i)) => {
                if i.blend_mode.is_some_and(|v| v > 1)
                    || i.is_extensible.is_some_and(|v| v != 0)
                    || !i.animation_frame_settings.is_empty()
                    || i.property_count.is_some_and(|v| v != 0)
                {
                    return Err(format!(
                        "{location}: unsupported instance effect/component/animation settings"
                    ));
                }
                if let Some(span) = &i.component_xml {
                    let xml = read_string(&s.original_bytes, span)?;
                    if !xml.is_empty() {
                        let document = roxmltree::Document::parse(&xml)
                            .map_err(|_| format!("{location}: invalid component metadata"))?;
                        let root = document.root_element();
                        if root.tag_name().name() != "component"
                            || root.children().any(|n| {
                                n.is_element()
                                    || n.is_text() && !n.text().unwrap_or("").trim().is_empty()
                            })
                            || root.attributes().any(|a| {
                                !matches!(
                                    a.name(),
                                    "metaDataFetched"
                                        | "schemaUrl"
                                        | "schemaOperation"
                                        | "sceneRootLabel"
                                        | "oldCopiedComponentPath"
                                ) || matches!(a.name(), "schemaUrl" | "schemaOperation")
                                    && !a.value().is_empty()
                            })
                        {
                            return Err(format!(
                                "{location}: component data binding requires a component adapter"
                            ));
                        }
                    }
                }
                if i.matrix_3d.is_some_and(|m| {
                    m != [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ]
                }) {
                    return Err(format!("{location}: nonidentity 3D transform"));
                }
                element.name = optional_text(
                    &s.original_bytes,
                    i.instance_name.as_ref().or(i.legacy_name.as_ref()),
                )?;
                element.transform = Transform::from_matrix(shape::matrix(
                    i.matrix_words.ok_or("Missing instance matrix")?,
                ))
                .ok_or("Invalid instance matrix")?;
                if let Some(c) = &i.color_transform {
                    element.transform.color_transform = color_transform(c)?;
                }
                let symbol_id = self
                    .library
                    .get(&i.library_id.ok_or("Missing library identity")?)
                    .ok_or_else(|| format!("{location}: unresolved library reference"))?
                    .clone();
                let first = u32::from(i.first_frame.unwrap_or(0));
                let playback =
                    if o.class == TimelineClass::Sprite || o.class == TimelineClass::Button {
                        InstancePlayback::Independent
                    } else {
                        match i.loop_mode.unwrap_or(0) {
                            0 if first == 0 => InstancePlayback::Loop,
                            1 if first == 0 => InstancePlayback::PlayOnce,
                            2 => InstancePlayback::SingleFrame(first),
                            _ => {
                                return Err(format!(
                                    "{location}: graphic playback with offset needs an adapter"
                                ));
                            }
                        }
                    };
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback,
                };
                if let Some(script) = self.actions(s, i.actions.as_ref())? {
                    element.scripts.push(ObjectScript {
                        event: ObjectEvent::Load,
                        script,
                    });
                }
                if i.accessibility.as_ref().is_some_and(|a| a.revision != 0) {
                    self.report.warning("fla_accessibility_metadata",&location,"Source accessibility metadata remains embedded; no inferred role was assigned.");
                }
            }
            Some(TimelinePayload::Text(t)) => {
                element = text::convert_text(s, t, id, "", self.report)?;
            }
            Some(TimelinePayload::Bitmap(b)) => {
                let media = b.media_id.ok_or("Missing bitmap identity")?;
                element.transform = Transform::from_matrix(shape::matrix(
                    b.matrix_words.ok_or("Missing bitmap matrix")?,
                ))
                .ok_or("Invalid bitmap matrix")?;
                if let Some(asset) = self.bitmaps.get(&media) {
                    element.kind = ElementKind::Bitmap {
                        resource_id: asset.resource_id.clone(),
                        width: asset.width as u32,
                        height: asset.height as u32,
                        smoothing: asset.smoothing,
                    };
                } else {
                    let symbol_id = if let Some(id) = self.missing_bitmaps.get(&media) {
                        id.clone()
                    } else {
                        let id = format!("fla-missing-bitmap-{media}");
                        self.generated.push(group_symbol(
                            &id,
                            format!("Unresolved bitmap #{media}"),
                            vec![],
                        ));
                        self.missing_bitmaps.insert(media, id.clone());
                        id
                    };
                    element.kind = ElementKind::Instance {
                        symbol_id,
                        playback: InstancePlayback::SingleFrame(0),
                    };
                    element.name = format!("Unresolved source bitmap #{media}");
                    self.report.warning("fla_source_bitmap_missing",&location,format!("Bitmap #{media} is already absent from the source archive. The placement and unresolved reference are retained; no image was invented."));
                }
            }
            Some(TimelinePayload::Shape) => {
                let mut members = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.extend(self.element(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    )?)
                }
                if let Some(index) = o.shape_index {
                    members.extend(shape::convert_shape(
                        &s.shapes[index].record,
                        &format!("{id}-paint"),
                        self.report,
                        &location,
                    )?)
                }
                // A one-frame graphic keeps group membership and vector paint order editable.
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            _ => {
                return Err(format!(
                    "{location}: unsupported display record {:?}",
                    o.class
                ));
            }
        }
        self.provenance.push(
            serde_json::json!({"stream":s.index,"object":object_id,"target":id,"source":o.encoded}),
        );
        Ok(vec![element])
    }
}
fn group_symbol(id: &str, name: String, elements: Vec<Element>) -> Symbol {
    Symbol {
        id: id.into(),
        name,
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![Layer {
                id: format!("{id}-layer"),
                name: "Members".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    tween: None,
                    label: String::new(),
                }],
                properties: Default::default(),
            }],
            scripts: vec![],
        },
        class_name: None,
        linkage_id: None,
    }
}
fn placement_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (
                ElementKind::Bitmap { resource_id: a, .. },
                ElementKind::Bitmap { resource_id: b, .. },
            ) => a == b,
            _ => false,
        }
}
fn color_transform(c: &TimelineColorTransform) -> Result<ColorTransform, String> {
    let mut result = ColorTransform::default();
    match c.effect_type.unwrap_or(0) {
        0 => {}
        1 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            for k in 0..3 {
                result.multipliers[k] = 1. - p.abs();
                result.offsets[k] = p.max(0.) * 255.;
            }
        }
        2 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            let color = shape::color(c.effect_rgba.unwrap_or(0));
            for (k, ch) in color.iter().enumerate().take(3) {
                result.multipliers[k] = 1. - p;
                result.offsets[k] = p * f64::from(*ch);
            }
        }
        4 => result.multipliers[3] = c.multipliers[3] as f64 / 256.,
        3 => {
            result.multipliers = c.multipliers.map(|v| v as f64 / 256.);
            result.offsets = c.offsets.map(f64::from);
        }
        _ => return Err("Unknown color effect".into()),
    }
    Ok(result)
}
fn ease(f: &TimelineFrame) -> Result<Option<Easing>, String> {
    match f.key_mode.unwrap_or(0) & 255 {
        0 => Ok(None),
        1 => {
            if f.custom_ease_present.unwrap_or(0) != 0 {
                if f.single_ease_curve != Some(1) {
                    return Err(
                        "Separate per-property easing curves require a tween adapter".into(),
                    );
                }
                let c = f
                    .custom_ease
                    .get(5)
                    .ok_or("Missing global custom-ease curve")?;
                if c.len() != 2 || c[0][2..4] != [0., 0.] || c[1][2..4] != [1., 1.] {
                    return Err("Multi-segment custom easing requires a tween adapter".into());
                }
                Ok(Some(Easing::CubicBezier {
                    x1: c[0][4],
                    y1: c[0][5],
                    x2: c[1][0],
                    y2: c[1][1],
                }))
            } else {
                Ok(Some(Easing::Flash {
                    acceleration: f.acceleration.unwrap_or(0) as i16 as f64,
                }))
            }
        }
        _ => Err("Shape/IK tween requires a separate animation adapter".into()),
    }
}
