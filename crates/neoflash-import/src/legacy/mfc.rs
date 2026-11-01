//! Sequential bounded MFC structure reader. Unknown data stops parsing;
//! neither byte-pattern scans nor guessed record boundaries are used.
use super::{BinaryStreamInspection, RecordCounts, geometry, timeline::*};
mod actions;
mod filters;
mod instances;
mod morph;
mod properties;
mod text;

const MAX_RECORDS: usize = 50_000;
const MAX_DEPTH: usize = 32;
const MAX_STRING: usize = 1024 * 1024;
/// Opt-in document retention budgets, shared across all timeline streams.
pub(super) const DOCUMENT_GEOMETRY_ITEMS: usize = 4_000_000;
pub(super) const DOCUMENT_TIMELINE_ITEMS: usize = 1_000_000;
type Class = TimelineClass;
struct Stop {
    code: String,
    message: String,
}
type Result<T> = std::result::Result<T, Stop>;
fn stop<T>(code: &'static str, message: impl Into<String>) -> Result<T> {
    Err(Stop {
        code: code.into(),
        message: message.into(),
    })
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    refs: Vec<TimelineReference>,
    objects: Vec<TimelineObject>,
    pointers: Vec<TimelinePointer>,
    counts: RecordCounts,
    geometry: geometry::GeometryCounts,
    retained: Vec<geometry::LocatedBinaryShape>,
    retain_geometry: bool,
    retain_timeline: bool,
    retained_items_remaining: &'a mut usize,
    timeline_items_remaining: &'a mut usize,
    root_class: Option<Class>,
    root_schema: Option<u16>,
    root: Option<u32>,
    stream_kind: Option<u8>,
}
pub(super) struct MfcRead {
    pub shapes: Vec<geometry::LocatedBinaryShape>,
    pub timeline: Option<BinaryTimelineStream>,
}
pub(super) fn inspect(
    data: &[u8],
    stream: &mut BinaryStreamInspection,
    retain_geometry: bool,
    retain_timeline: bool,
    retained_items_remaining: &mut usize,
    timeline_items_remaining: &mut usize,
) -> MfcRead {
    let mut reader = Reader {
        data,
        pos: 0,
        refs: vec![TimelineReference::Null],
        objects: vec![],
        pointers: vec![],
        counts: RecordCounts::default(),
        geometry: geometry::GeometryCounts::default(),
        retained: vec![],
        retain_geometry: retain_geometry || retain_timeline,
        retain_timeline,
        retained_items_remaining,
        timeline_items_remaining,
        root_class: None,
        root_schema: None,
        root: None,
        stream_kind: None,
    };
    let result = reader.root();
    stream.bytes_consumed = reader.pos;
    stream.records = reader.counts;
    stream.geometry = reader.geometry;
    stream.root_class = reader.root_class.map(|c| c.name().to_owned());
    stream.root_schema = reader.root_schema;
    match result {
        Ok(()) => stream.structure_complete = true,
        Err(e) => {
            stream.stop_code = Some(e.code);
            stream.stop_message = Some(e.message);
        }
    }
    if retain_timeline {
        MfcRead {
            shapes: vec![],
            timeline: Some(BinaryTimelineStream {
                index: stream.index,
                stream_kind: reader.stream_kind,
                root: reader.root,
                references: reader.refs,
                pointers: reader.pointers,
                objects: reader.objects,
                shapes: reader.retained,
                original_bytes: data.to_vec(),
            }),
        }
    } else {
        MfcRead {
            shapes: reader.retained,
            timeline: None,
        }
    }
}
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.data.len() - self.pos {
            return stop(
                "fla_truncated_record",
                "Truncated MFC record; no boundary recovery attempted.",
            );
        }
        let value = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(value)
    }
    fn span(&mut self, n: usize) -> Result<TimelineByteSpan> {
        let start = self.pos;
        self.bytes(n)?;
        Ok(TimelineByteSpan {
            start,
            end: self.pos,
        })
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn word(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn dword(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn float(&mut self) -> Result<f32> {
        let value = f32::from_le_bytes(self.bytes(4)?.try_into().unwrap());
        if !value.is_finite() {
            return stop(
                "fla_nonfinite_number",
                "Nonfinite number in an authored record.",
            );
        }
        Ok(value)
    }
    fn double(&mut self) -> Result<f64> {
        let value = f64::from_le_bytes(self.bytes(8)?.try_into().unwrap());
        if !value.is_finite() {
            return stop(
                "fla_nonfinite_number",
                "Nonfinite number in an authored record.",
            );
        }
        Ok(value)
    }
    fn signed_words<const N: usize>(&mut self) -> Result<[i32; N]> {
        let mut out = [0; N];
        for word in &mut out {
            *word = self.dword()? as i32;
        }
        Ok(out)
    }
    fn revision(&mut self, max: u8, what: &str) -> Result<u8> {
        let v = self.byte()?;
        if v > max {
            return stop(
                "fla_record_revision_unsupported",
                format!("Unsupported {what} revision {v}; maximum decoded revision is {max}."),
            );
        }
        Ok(v)
    }
    fn collection(&mut self, count: usize, width: usize) -> Result<TimelineByteSpan> {
        if count > MAX_RECORDS {
            return stop("fla_record_limit", "Collection count exceeds 50000.");
        }
        let Some(bytes) = count.checked_mul(width) else {
            return stop("fla_record_limit", "Collection byte length overflow.");
        };
        self.span(bytes)
    }
    fn retain_budget(&mut self) -> Result<()> {
        if self.retain_timeline {
            if *self.timeline_items_remaining == 0 {
                return stop(
                    "fla_timeline_document_limit",
                    "Retained timeline identity/pointer/object data exceeds 1000000 items across this document.",
                );
            }
            *self.timeline_items_remaining -= 1;
        }
        Ok(())
    }
    fn string_length(&mut self) -> Result<Option<u32>> {
        let short = self.byte()?;
        if short < 255 {
            return Ok(Some(short.into()));
        }
        match self.word()? {
            0xfffe => Ok(None),
            0xffff => Ok(Some(self.dword()?)),
            value => Ok(Some(value.into())),
        }
    }
    fn string(&mut self) -> Result<TimelineString> {
        let start = self.pos;
        let first = self.string_length()?;
        let utf16 = first.is_none();
        let length = if let Some(n) = first {
            n
        } else {
            match self.string_length()? {
                Some(n) => n,
                None => return stop("fla_string_limit", "Repeated CString Unicode marker."),
            }
        };
        let length = if length == u32::MAX {
            0
        } else {
            length as usize
        };
        let width = if utf16 { 2 } else { 1 };
        if length > MAX_STRING / width {
            return stop("fla_string_limit", "String exceeds 1 MiB.");
        }
        let data = self.span(length * width)?;
        if utf16
            && char::decode_utf16(
                self.data[data.start..data.end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c)),
            )
            .any(|c| c.is_err())
        {
            return stop("fla_string_encoding", "Invalid UTF-16 string.");
        }
        Ok(TimelineString {
            encoded: TimelineByteSpan {
                start,
                end: self.pos,
            },
            data,
            utf16,
        })
    }
    fn object_index(&self, id: u32) -> usize {
        match self.refs[id as usize] {
            TimelineReference::Object { object_index } => object_index,
            _ => unreachable!("internally resolved object ID"),
        }
    }
    fn class(&self, id: u32) -> Class {
        self.objects[self.object_index(id)].class
    }
    fn add_reference(&mut self, reference: TimelineReference) -> Result<u32> {
        if self.refs.len() >= MAX_RECORDS {
            return stop(
                "fla_record_limit",
                "MFC class/object table exceeds 50000 entries.",
            );
        }
        self.retain_budget()?;
        let id = self.refs.len() as u32;
        self.refs.push(reference);
        Ok(id)
    }
    fn pointer(
        &mut self,
        source: Option<u32>,
        relation: TimelineRelation,
        target: Option<u32>,
        is_reference: bool,
        start: usize,
    ) -> Result<usize> {
        self.retain_budget()?;
        let index = self.pointers.len();
        self.pointers.push(TimelinePointer {
            source,
            relation,
            target,
            is_reference,
            encoded: TimelineByteSpan {
                start,
                end: self.pos,
            },
        });
        if relation == TimelineRelation::Child
            && let (Some(source), Some(target)) = (source, target)
        {
            let index = self.object_index(source);
            self.objects[index].children.push(target);
        }
        if relation == TimelineRelation::Root {
            self.root = target;
        }
        Ok(index)
    }
    fn root(&mut self) -> Result<()> {
        let kind = self.byte()?;
        self.stream_kind = Some(kind);
        if kind > 1 {
            return stop(
                "fla_page_kind_unsupported",
                "Only shape (0) and timeline (1) stream roots are decoded.",
            );
        }
        let root = self.object(0, None, TimelineRelation::Root)?;
        let expected = if kind == 0 { Class::Shape } else { Class::Page };
        if root.map(|id| self.class(id)) != Some(expected) {
            return stop(
                "fla_invalid_root",
                "MFC root class disagrees with stream kind.",
            );
        }
        if self.pos != self.data.len() {
            return stop(
                "fla_trailing_data",
                "Trailing bytes remain after the MFC root; no content is silently dropped.",
            );
        }
        Ok(())
    }
    fn object(
        &mut self,
        depth: usize,
        source: Option<u32>,
        relation: TimelineRelation,
    ) -> Result<Option<u32>> {
        if depth > MAX_DEPTH {
            return stop("fla_depth_limit", "MFC nesting exceeds 32.");
        }
        let start = self.pos;
        let small = self.word()?;
        let tag = if small == 0x7fff {
            self.dword()?
        } else if small & 0x8000 != 0 {
            0x80000000 | u32::from(small & 0x7fff)
        } else {
            u32::from(small)
        };
        if tag == 0 {
            self.pointer(source, relation, None, true, start)?;
            return Ok(None);
        }
        let (class, schema) = if small == 0xffff {
            let schema = self.word()?;
            let len = usize::from(self.word()?);
            if !(1..=63).contains(&len) {
                return stop("fla_mfc_class", "Invalid MFC class-name length.");
            }
            let encoded_name = self.span(len)?;
            let bytes = &self.data[encoded_name.start..encoded_name.end];
            if !bytes.iter().all(|b| (33..=126).contains(b)) {
                return stop("fla_mfc_class", "MFC class name is not ASCII.");
            }
            let class = Class::read(bytes);
            self.add_reference(TimelineReference::Class {
                class,
                schema,
                encoded_name,
            })?;
            (class, schema)
        } else {
            let is_class = tag & 0x80000000 != 0;
            let index = (tag & 0x7fffffff) as usize;
            match self.refs.get(index) {
                Some(TimelineReference::Class { class, schema, .. }) if is_class => {
                    (*class, *schema)
                }
                Some(TimelineReference::Object { .. }) if !is_class => {
                    self.pointer(source, relation, Some(tag), true, start)?;
                    // Layers may first be serialized through a mask/folder link,
                    // then referenced in the page's ordered child list. Reuse the
                    // completed object identity instead of reparsing or cloning it.
                    if matches!(
                        relation,
                        TimelineRelation::Child
                            | TimelineRelation::Root
                            | TimelineRelation::MorphShape
                            | TimelineRelation::MorphSegment
                            | TimelineRelation::MorphCurve
                            | TimelineRelation::MorphHint
                    ) && !self.objects[self.object_index(tag)].complete
                    {
                        return stop(
                            "fla_mfc_object_reference_unsupported",
                            "A child pointer targets an active, incomplete object; cyclic ownership is unsupported.",
                        );
                    }
                    return Ok(Some(tag));
                }
                _ => {
                    return stop(
                        "fla_mfc_reference",
                        "Invalid MFC reference index or reference kind.",
                    );
                }
            }
        };
        // Reserve object identity and incoming edge before the body. On any stop
        // its complete flag remains false, including unknown class/schema bodies.
        if self.retain_timeline && *self.timeline_items_remaining < 3 {
            return stop(
                "fla_timeline_document_limit",
                "Retained timeline identity/pointer/object data exceeds 1000000 items across this document.",
            );
        }
        self.retain_budget()?;
        let object_index = self.objects.len();
        let id = self.add_reference(TimelineReference::Object { object_index })?;
        // This reservation must have a target object even if pointer budget fails.
        self.objects.push(TimelineObject {
            object_id: id,
            class,
            schema,
            introduced_by_pointer: self.pointers.len(),
            owner: if relation == TimelineRelation::Child {
                source
            } else {
                None
            },
            children: vec![],
            encoded: TimelineByteSpan {
                start,
                end: self.pos,
            },
            body_offset: self.pos,
            complete: false,
            base: None,
            payload: None,
            shape_index: None,
        });
        let incoming = self.pointer(source, relation, Some(id), false, start)?;
        self.objects[object_index].introduced_by_pointer = incoming;
        self.counts.encountered += 1;
        if depth == 0 {
            self.root_class = Some(class);
            self.root_schema = Some(schema);
        }
        let parsed = (|| {
            if schema != 1 {
                return stop(
                    "fla_mfc_schema_unsupported",
                    format!("MFC schema {schema} is not implemented."),
                );
            }
            // Morph records are not CPicObj subclasses; each is accepted only
            // through the typed pointer that owns it in the archive layout.
            let expected = match class {
                Class::Morph => Some(TimelineRelation::MorphShape),
                Class::MorphSegment => Some(TimelineRelation::MorphSegment),
                Class::MorphCurve => Some(TimelineRelation::MorphCurve),
                Class::MorphHint => Some(TimelineRelation::MorphHint),
                _ => None,
            };
            if let Some(expected) = expected {
                if relation != expected {
                    return stop(
                        "fla_mfc_class_unsupported",
                        format!(
                            "{} is only decoded through its morph-shape pointer; parsing stops at its header.",
                            class.name()
                        ),
                    );
                }
                return match class {
                    Class::Morph => self.morph_shape(id, depth),
                    Class::MorphSegment => self.morph_segment(id, depth),
                    Class::MorphCurve => self.morph_curve(id),
                    _ => self.morph_hint(id),
                };
            }
            if matches!(
                relation,
                TimelineRelation::MorphSegment
                    | TimelineRelation::MorphCurve
                    | TimelineRelation::MorphHint
                    | TimelineRelation::MorphShape
            ) {
                return stop(
                    "fla_mfc_class_unsupported",
                    "A morph pointer targets a non-morph record; parsing stops at its header.",
                );
            }
            if !matches!(
                class,
                Class::Page
                    | Class::Layer
                    | Class::Frame
                    | Class::Shape
                    | Class::Bitmap
                    | Class::Symbol
                    | Class::Sprite
                    | Class::Button
                    | Class::Text
                    | Class::Rect
                    | Class::Oval
                    | Class::CompiledClip
                    | Class::VideoStream
            ) {
                // Unrecognized archive class names may be private; only
                // known Flash class names appear in public diagnostics.
                return stop(
                    "fla_mfc_class_unsupported",
                    if class == Class::Unknown {
                        "An unrecognized MFC class payload is not implemented; parsing stops at its header.".to_owned()
                    } else {
                        format!(
                            "{} payload is not implemented; parsing stops at its header.",
                            class.name()
                        )
                    },
                );
            }
            self.base(id, depth)?;
            match class {
                Class::Page => self.page(id)?,
                Class::Layer => self.layer(id, depth)?,
                Class::Shape => {
                    self.objects[object_index].payload = Some(TimelinePayload::Shape);
                    self.shape(id)?;
                }
                Class::Frame => {
                    self.shape(id)?;
                    self.frame(id, depth)?;
                }
                Class::Bitmap => self.bitmap(id)?,
                Class::Symbol | Class::Sprite | Class::Button => self.instance(id, class)?,
                Class::Text => self.text(id)?,
                Class::Rect | Class::Oval => {
                    self.shape(id)?;
                    self.primitive(id, class)?;
                }
                Class::CompiledClip => self.compiled_clip(id)?,
                Class::VideoStream => self.video(id)?,
                _ => unreachable!("class checked above"),
            }
            Ok(())
        })();
        self.objects[object_index].encoded.end = self.pos;
        if parsed.is_ok() {
            self.objects[object_index].complete = true;
            self.counts.complete += 1;
            match class {
                Class::Page => self.counts.pages += 1,
                Class::Layer => self.counts.layers += 1,
                Class::Frame => self.counts.frames += 1,
                Class::Shape | Class::Rect | Class::Oval => self.counts.shapes += 1,
                _ => {}
            }
        }
        parsed.map(|()| Some(id))
    }
    fn base(&mut self, id: u32, depth: usize) -> Result<()> {
        let mut base = TimelineBase::default();
        let parsed = (|| {
            let rev = self.revision(6, "CPicObj")?;
            base.revision = Some(rev);
            base.flags = Some(self.byte()?);
            while self
                .object(depth + 1, Some(id), TimelineRelation::Child)?
                .is_some()
            {}
            base.children_complete = true;
            if rev >= 1 {
                base.registration_words = Some(self.signed_words()?);
            }
            if rev >= 3 {
                base.drawing_object_flags = Some(self.byte()?);
            }
            if rev >= 4 {
                base.cache_as_bitmap = Some(self.byte()?);
            }
            if rev >= 6 {
                let start = self.pos;
                let mut terminated = false;
                for _ in 0..37 {
                    if self.byte()? == 0 {
                        terminated = true;
                        break;
                    }
                }
                if !terminated {
                    return stop(
                        "fla_identifier_limit",
                        "Object identifier exceeds 36 bytes.",
                    );
                }
                base.identifier = Some(TimelineByteSpan {
                    start,
                    end: self.pos,
                });
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].base = Some(base);
        parsed
    }
    fn page(&mut self, id: u32) -> Result<()> {
        let mut page = TimelinePage::default();
        let parsed = (|| {
            let rev = self.revision(7, "CPicPage")?;
            page.revision = Some(rev);
            if rev != 4 {
                page.next_layer_id = Some(self.word()?);
            }
            if rev >= 5 {
                page.next_folder_id = Some(self.word()?);
            }
            if rev >= 7 {
                page.current_frame = Some(self.dword()?);
            }
            if rev >= 3 {
                let count = self.dword()?;
                page.guide_count = Some(count);
                page.guides = Some(self.collection(count as usize, 8)?);
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Page(page));
        parsed
    }
    fn layer(&mut self, id: u32, depth: usize) -> Result<()> {
        let mut layer = TimelineLayer::default();
        let parsed = (|| {
            let rev = self.revision(13, "CPicLayer")?;
            layer.revision = Some(rev);
            layer.name = Some(self.string()?);
            layer.state_flags = Some(self.span(if rev <= 3 { 1 } else { 3 })?);
            if rev >= 5 {
                layer.height_literal = Some(self.dword()?);
            }
            if rev >= 6 {
                layer.outline_color_word = Some(self.dword()?);
                layer.outline = Some(self.dword()?);
            }
            if rev >= 8 {
                layer.height_multiplier = Some(self.dword()?);
            }
            layer.layer_type = Some(self.byte()?);
            self.object(depth + 1, Some(id), TimelineRelation::RelatedLayer)?;
            if (7..=8).contains(&rev) {
                self.object(depth + 1, Some(id), TimelineRelation::LegacyRelatedLayer)?;
            }
            if (2..=5).contains(&rev) {
                layer.legacy_flag = Some(self.byte()?);
            }
            if rev >= 3 {
                layer.mask_or_open = Some(self.byte()?);
            }
            if rev >= 10 {
                layer.auto_named = Some(self.byte()?);
            }
            if rev >= 12 {
                let kind = if rev >= 13 {
                    self.byte()?
                } else {
                    match layer.layer_type {
                        Some(6) => 2,
                        Some(5) => 1,
                        _ => 0,
                    }
                };
                layer.animation_type = Some(kind);
                if kind > 2 {
                    return stop(
                        "fla_layer_animation_unsupported",
                        "Unknown layer animation type.",
                    );
                }
                if kind == 2 {
                    let count = self.dword()? as usize;
                    if count > MAX_RECORDS {
                        return stop("fla_record_limit", "Layer animation table exceeds budget.");
                    }
                    for _ in 0..count {
                        layer.animation_values.push(self.dword()?);
                    }
                }
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Layer(layer));
        parsed
    }
    fn shape(&mut self, id: u32) -> Result<()> {
        match geometry::decode_prefix(&self.data[self.pos..]) {
            Ok(shape) => {
                if self.retain_geometry && shape.item_count() > *self.retained_items_remaining {
                    return stop(
                        "fla_geometry_document_limit",
                        "Retained geometry exceeds 4000000 items across this document.",
                    );
                }
                let offset = self.pos;
                self.pos += shape.original_bytes.len();
                self.geometry.add(&shape);
                if self.retain_geometry {
                    *self.retained_items_remaining -= shape.item_count();
                    let index = self.object_index(id);
                    self.objects[index].shape_index = Some(self.retained.len());
                    self.retained.push(geometry::LocatedBinaryShape {
                        offset,
                        record: shape,
                    });
                }
                Ok(())
            }
            Err(e) => {
                self.pos += e.offset;
                Err(Stop {
                    code: e.code,
                    message: e.message,
                })
            }
        }
    }
    fn frame(&mut self, id: u32, depth: usize) -> Result<()> {
        let mut frame = TimelineFrame::default();
        let parsed = (|| {
            let rev = self.revision(29, "CPicFrame")?;
            frame.revision = Some(rev);
            frame.duration = Some(self.word()?);
            frame.key_mode = Some(if rev <= 2 {
                u16::from(self.byte()?)
            } else {
                self.word()?
            });
            if rev >= 2 {
                frame.acceleration = Some(self.word()?);
            }
            if rev >= 5 {
                frame.sound_media_id = Some(self.word()?);
            }
            if rev >= 6 {
                let count = self.word()?;
                frame.sound_envelope_count = Some(count);
                frame.sound_envelope = Some(self.collection(count as usize, 8)?);
            }
            if rev >= 7 {
                frame.sound_loop_count = Some(self.word()?);
                frame.sound_sync = Some(self.byte()?);
                frame.sound_in_point = Some(self.dword()?);
                frame.sound_out_point = Some(self.dword()?);
            }
            if rev >= 8 {
                frame.sound_zoom = Some(self.word()?);
            }
            if rev >= 9 {
                frame.label = Some(self.string()?);
            }
            if rev >= 4 {
                let (actions, result) = self.actions(rev);
                frame.actions = Some(actions);
                result?;
            }
            if rev >= 11 {
                frame.rotation_mode = Some(self.dword()?);
                frame.rotation_count = Some(self.dword()?);
            }
            if rev >= 12 {
                frame.label_type = Some(self.dword()?);
            }
            if rev >= 13 {
                frame.morph_shape =
                    self.object(depth + 1, Some(id), TimelineRelation::MorphShape)?;
            }
            if rev >= 14 {
                frame.shape_tween_blend = Some(self.dword()?);
            }
            if rev >= 15 {
                self.object(depth + 1, Some(id), TimelineRelation::SoundSettings)?;
            }
            if rev >= 16 {
                frame.extension = Some(self.string()?);
            }
            if rev == 17 {
                return stop(
                    "fla_historical_frame_extension_unsupported",
                    "Frame revision 17 historical extension is unsupported.",
                );
            }
            if rev >= 20 {
                frame.reserved = Some(self.dword()?);
            }
            if rev >= 21 {
                frame.sound_effect = Some(self.dword()?);
            }
            if rev >= 22 {
                frame.bookmark = Some(self.dword()?);
            }
            if rev >= 24 {
                frame.single_ease_curve = Some(self.dword()?);
                let present = self.dword()?;
                frame.custom_ease_present = Some(present);
                if present != 0 {
                    let mut total = 0usize;
                    for _ in 0..6 {
                        let count = self.dword()? as usize;
                        total = total.checked_add(count).ok_or_else(|| Stop {
                            code: "fla_record_limit".into(),
                            message: "Ease count overflow.".into(),
                        })?;
                        if total > MAX_RECORDS || count > (self.data.len() - self.pos) / 48 {
                            return stop(
                                "fla_record_limit",
                                "Custom ease exceeds remaining input or item budget.",
                            );
                        }
                        let mut curve = Vec::with_capacity(count);
                        for _ in 0..count {
                            self.retain_budget()?;
                            let mut knot = [0.; 6];
                            for value in &mut knot {
                                *value = self.double()?;
                            }
                            curve.push(knot);
                        }
                        frame.custom_ease.push(curve);
                    }
                }
            }
            let kind = frame.key_mode.unwrap_or(0) & 255;
            if rev >= 27 && kind == 3 {
                frame.motion_xml = Some(self.string()?);
                frame.visible_animation_keyframes = Some(self.dword()?);
            }
            if rev >= 28 && kind == 3 {
                frame.tween_instance_name = Some(self.string()?);
            }
            if rev >= 29 && kind == 4 {
                return stop(
                    "fla_pose_animation_unsupported",
                    "Pose matrix animation requires a dedicated adapter.",
                );
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Frame(Box::new(frame)));
        parsed
    }
    fn actions(&mut self, frame_revision: u8) -> (TimelineActions, Result<()>) {
        let start = self.pos;
        let mut actions = TimelineActions {
            encoded: TimelineByteSpan { start, end: start },
            complete: false,
            presence: TimelineActionPresence::Unknown,
            revision: None,
            mode: None,
            identifier: None,
            breakpoint_count: None,
            breakpoints: None,
            collapse_word_count: None,
            collapse_words: None,
            source: None,
            compiled_revision: None,
            command_count: None,
            decoded: None,
        };
        let parsed = (|| {
            // Frame revisions 4-9 (and button revision 2, passed as 9) hold one
            // inline historical command; 10-18 (buttons 3-4, passed as 18) a list.
            if frame_revision <= 9 {
                actions.presence = TimelineActionPresence::Historical;
                return self.decode_actions(
                    &mut actions,
                    TimelineActionForm::HistoricalCommand,
                    None,
                    1,
                );
            }
            if frame_revision <= 18 {
                let count = self.dword()?;
                actions.command_count = Some(count);
                actions.presence = if count == 0 {
                    TimelineActionPresence::Empty
                } else {
                    TimelineActionPresence::Historical
                };
                if count != 0 {
                    return self.decode_actions(
                        &mut actions,
                        TimelineActionForm::HistoricalList,
                        None,
                        count,
                    );
                }
                return Ok(());
            }
            let rev = self.dword()?;
            actions.revision = Some(rev);
            if rev > 5 {
                return stop(
                    "fla_action_revision_unsupported",
                    "Unknown action source revision.",
                );
            }
            let mode = self.dword()?;
            actions.mode = Some(mode);
            if rev >= 1 {
                actions.identifier = Some(self.dword()?);
                let count = self.dword()?;
                actions.breakpoint_count = Some(count);
                actions.breakpoints = Some(self.collection(count as usize, 4)?);
            }
            if rev >= 5 {
                let count = self.dword()?;
                actions.collapse_word_count = Some(count);
                actions.collapse_words = Some(self.collection(count as usize, 4)?);
            }
            match mode {
                1 if rev != 3 => {
                    let source = self.string()?;
                    actions.presence = if source.data.start == source.data.end {
                        TimelineActionPresence::Empty
                    } else {
                        TimelineActionPresence::Source
                    };
                    actions.source = Some(source);
                    Ok(())
                }
                2 => {
                    actions.presence = TimelineActionPresence::Empty;
                    Ok(())
                }
                0 => {
                    let list_revision = self.dword()?;
                    actions.compiled_revision = Some(list_revision);
                    let count = self.dword()?;
                    actions.command_count = Some(count);
                    if count != 0 {
                        // The compiled count word is signed in the archive.
                        // Preserve negative sentinels without calling them code.
                        if (count as i32) < 0 {
                            return stop(
                                "fla_compiled_actions_unsupported",
                                "Negative compiled action counts are not decoded.",
                            );
                        }
                        actions.presence = TimelineActionPresence::Compiled;
                        return self.decode_actions(
                            &mut actions,
                            TimelineActionForm::Compiled,
                            Some(list_revision),
                            count,
                        );
                    }
                    actions.presence = TimelineActionPresence::Empty;
                    Ok(())
                }
                _ => stop(
                    "fla_action_mode_unsupported",
                    "Action source encoding/mode is not implemented.",
                ),
            }
        })();
        actions.encoded.end = self.pos;
        actions.complete = parsed.is_ok();
        (actions, parsed)
    }
    fn bitmap(&mut self, id: u32) -> Result<()> {
        let mut bitmap = TimelineBitmap::default();
        let parsed = (|| {
            let rev = self.revision(2, "CPicBitmap")?;
            bitmap.revision = Some(rev);
            if rev == 0 {
                return stop(
                    "fla_bitmap_revision_unsupported",
                    "Embedded historical raster records are not implemented.",
                );
            }
            bitmap.matrix_words = Some(self.signed_words()?);
            bitmap.media_id = Some(self.word()?);
            if rev == 2 {
                let present = self.byte()?;
                bitmap.extension_present = Some(present);
                if present != 0 {
                    return stop(
                        "fla_bitmap_extension_unsupported",
                        "Bitmap instance extension is not implemented.",
                    );
                }
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Bitmap(bitmap));
        parsed
    }
}
