//! Independently authored binary fixtures, no imported document content.
use neoflash_import::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Archive(PathBuf);
impl Archive {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "neoflash-timeline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, page: &[u8]) -> PathBuf {
        self.files(&[page])
    }
    fn files(&self, pages: &[&[u8]]) -> PathBuf {
        let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        c.create_stream("Contents")
            .unwrap()
            .write_all(&[0x20])
            .unwrap();
        for (i, page) in pages.iter().enumerate() {
            c.create_stream(format!("Page {:X}", i + 1))
                .unwrap()
                .write_all(page)
                .unwrap();
        }
        let path = self.0.join("synthetic.fla");
        fs::write(&path, c.into_inner().into_inner()).unwrap();
        path
    }
    fn read(&self, page: &[u8]) -> BinaryTimelineDocument {
        read_binary_timeline(&self.file(page)).unwrap()
    }
}
impl Drop for Archive {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Writer {
    bytes: Vec<u8>,
    classes: BTreeMap<&'static str, u16>,
    next: u16,
}
impl Writer {
    fn new() -> Self {
        Self {
            bytes: vec![1],
            classes: BTreeMap::new(),
            next: 1,
        }
    }
    fn byte(&mut self, v: u8) {
        self.bytes.push(v);
    }
    fn word(&mut self, v: u16) {
        self.bytes.extend(v.to_le_bytes());
    }
    fn dword(&mut self, v: u32) {
        self.bytes.extend(v.to_le_bytes());
    }
    fn string(&mut self, value: &str) {
        assert!(value.len() < 255);
        self.byte(value.len() as u8);
        self.bytes.extend(value.bytes());
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(id) = self.classes.get(class).copied() {
            self.word(0x8000 | id);
        } else {
            self.word(0xffff);
            self.word(1);
            self.word(class.len() as u16);
            self.bytes.extend(class.bytes());
            self.classes.insert(class, self.next);
            self.next += 1;
        }
        let id = self.next;
        self.next += 1;
        id
    }
    fn base_start(&mut self) {
        self.byte(1);
        self.byte(3);
    }
    fn base_end(&mut self) {
        self.word(0);
        self.dword(i32::MIN as u32);
        self.dword(i32::MIN as u32);
    }
    fn shape(&mut self) {
        self.byte(2);
        for n in [65536, 0, 0, 65536, 20, 40] {
            self.dword(n);
        }
        self.byte(5);
        self.bytes.extend([0; 9]);
        self.dword(0);
    }
    fn frame(&mut self, duration: u16, label: &str, source: Option<&str>, child: bool) -> u16 {
        let id = self.begin("CPicFrame");
        self.base_start();
        if child {
            self.begin("CPicShape");
            self.base_start();
            self.base_end();
            self.shape();
        }
        self.base_end();
        self.shape();
        self.frame_fields(21, duration, label, source);
        id
    }
    fn frame_fields(&mut self, rev: u8, duration: u16, label: &str, source: Option<&str>) {
        self.frame_fields_with(rev, duration, label, source, |w| w.word(0));
    }
    fn frame_fields_with(
        &mut self,
        rev: u8,
        duration: u16,
        label: &str,
        source: Option<&str>,
        morph: impl FnOnce(&mut Self),
    ) {
        self.byte(rev);
        self.word(duration);
        self.word(0);
        self.word(7);
        self.word(0);
        self.word(0);
        self.word(1);
        self.byte(0);
        self.dword(0);
        self.dword(0);
        self.word(0);
        self.string(label);
        if rev >= 19 {
            self.dword(0);
            self.dword(if source.is_some() { 1 } else { 2 });
            if let Some(source) = source {
                self.string(source);
            }
        } else {
            self.dword(0);
        }
        if rev >= 11 {
            self.dword(0);
            self.dword(0);
        }
        if rev >= 12 {
            self.dword(0);
        }
        if rev >= 13 {
            morph(self);
        }
        if rev >= 14 {
            self.dword(0);
        }
        if rev >= 15 {
            self.word(0);
        }
        if rev >= 16 {
            self.string("");
        }
        if rev >= 20 {
            self.dword(0);
        }
        if rev >= 21 {
            self.dword(0);
        }
    }
    fn layer_tail(&mut self, name: &str, related: u16) {
        self.layer_tail_with(name, |writer| writer.word(related));
    }
    fn layer_tail_with(&mut self, name: &str, related: impl FnOnce(&mut Self)) {
        self.byte(8);
        self.string(name);
        self.bytes.extend([1, 2, 3]);
        self.dword(22);
        self.dword(0xff112233);
        self.dword(0);
        self.dword(1);
        self.byte(0);
        related(self);
        self.word(0);
        self.byte(1);
    }
    fn page_tail(&mut self) {
        self.byte(3);
        self.word(3);
        self.dword(0);
    }
}
fn timeline() -> Vec<u8> {
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    let first = w.begin("CPicLayer");
    w.base_start();
    w.frame(1, "opening", Some("this.stop(); // original fixture"), true);
    w.frame(3, "held", None, false);
    w.base_end();
    w.layer_tail("original layer", 0);
    w.begin("CPicLayer");
    w.base_start();
    w.base_end();
    w.layer_tail("related layer", first);
    w.base_end();
    w.page_tail();
    w.bytes
}
fn object(stream: &BinaryTimelineStream, id: u32) -> &TimelineObject {
    match stream.references[id as usize] {
        TimelineReference::Object { object_index } => &stream.objects[object_index],
        _ => panic!("not object"),
    }
}
fn text<'a>(stream: &'a BinaryTimelineStream, value: &TimelineString) -> &'a [u8] {
    &stream.original_bytes[value.data.start..value.data.end]
}
fn assert_graph(stream: &BinaryTimelineStream) {
    for (i, reference) in stream.references.iter().enumerate() {
        if let TimelineReference::Object { object_index } = reference {
            assert_eq!(stream.objects[*object_index].object_id, i as u32);
        }
    }
    for (index, o) in stream.objects.iter().enumerate() {
        assert_eq!(object(stream, o.object_id).object_id, o.object_id);
        assert!(
            o.encoded.start <= o.body_offset
                && o.body_offset <= o.encoded.end
                && o.encoded.end <= stream.original_bytes.len()
        );
        let pointer = &stream.pointers[o.introduced_by_pointer];
        assert_eq!(pointer.target, Some(o.object_id));
        assert!(!pointer.is_reference);
        if let Some(owner) = o.owner {
            assert_eq!(pointer.relation, TimelineRelation::Child);
            assert_eq!(pointer.source, Some(owner));
            assert!(object(stream, owner).children.contains(&o.object_id));
        }
        if let Some(shape) = o.shape_index {
            assert!(matches!(
                o.class,
                TimelineClass::Shape
                    | TimelineClass::Frame
                    | TimelineClass::Rect
                    | TimelineClass::Oval
            ));
            let s = &stream.shapes[shape];
            assert_eq!(
                &stream.original_bytes[s.offset..s.offset + s.record.original_bytes.len()],
                s.record.original_bytes
            );
            assert!(s.offset >= o.body_offset);
            assert!(s.offset + s.record.original_bytes.len() <= o.encoded.end);
        }
        assert_eq!(
            match stream.references[o.object_id as usize] {
                TimelineReference::Object { object_index } => object_index,
                _ => usize::MAX,
            },
            index
        );
    }
    for pointer in &stream.pointers {
        assert!(pointer.encoded.end <= stream.original_bytes.len());
        if let Some(target) = pointer.target {
            object(stream, target);
        }
        if let Some(source) = pointer.source {
            object(stream, source);
        }
    }
}
#[test]
fn identity_order_ownership_durations_labels_and_source_are_retained() {
    let a = Archive::new();
    let bytes = timeline();
    let d = a.read(&bytes);
    let s = &d.streams[0];
    assert_graph(s);
    assert_eq!(s.original_bytes, bytes);
    let root = object(s, s.root.unwrap());
    assert_eq!(root.class, TimelineClass::Page);
    assert_eq!(root.children.len(), 2);
    assert!(s.objects.iter().all(|o| o.complete));
    let first = object(s, root.children[0]);
    assert_eq!(first.children.len(), 2);
    let frames: Vec<_> = first.children.iter().map(|id| object(s, *id)).collect();
    assert_eq!(frames[0].children.len(), 1);
    assert_eq!(s.shapes.len(), 3);
    let TimelinePayload::Frame(f) = frames[0].payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(f.duration, Some(1));
    assert_eq!(f.acceleration, Some(7));
    assert_eq!(text(s, f.label.as_ref().unwrap()), b"opening");
    let actions = f.actions.as_ref().unwrap();
    assert!(actions.complete);
    assert_eq!(actions.presence, TimelineActionPresence::Source);
    assert_eq!(
        text(s, actions.source.as_ref().unwrap()),
        b"this.stop(); // original fixture"
    );
    let TimelinePayload::Frame(f) = frames[1].payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(f.duration, Some(3));
    assert_eq!(
        f.actions.as_ref().unwrap().presence,
        TimelineActionPresence::Empty
    );
    let related = s
        .pointers
        .iter()
        .find(|p| p.relation == TimelineRelation::RelatedLayer && p.target.is_some())
        .unwrap();
    assert_eq!(related.source, Some(root.children[1]));
    assert_eq!(related.target, Some(first.object_id));
    assert_eq!(first.owner, Some(root.object_id));
    assert!(related.is_reference);
    let report = inspect(&a.file(&bytes)).unwrap();
    let json = serde_json::to_string(&report).unwrap();
    for private in ["opening", "original layer", "this.stop()", "original_bytes"] {
        assert!(!json.contains(private));
    }
    assert!(!report.can_import);
    let serialized = serde_json::to_string(&d).unwrap();
    let round: BinaryTimelineDocument = serde_json::from_str(&serialized).unwrap();
    assert_graph(&round.streams[0]);
    assert_eq!(round.streams[0].original_bytes, bytes);
}
#[test]
fn every_truncation_keeps_only_proven_ids_and_explicit_pending_ancestors() {
    let a = Archive::new();
    let bytes = timeline();
    for n in 0..bytes.len() {
        let d = a.read(&bytes[..n]);
        let s = &d.streams[0];
        assert_graph(s);
        let status = d
            .inspection
            .streams
            .iter()
            .find(|x| x.index == s.index)
            .unwrap();
        assert!(!status.structure_complete);
        assert!(status.bytes_consumed <= n);
        assert_eq!(
            status.stop_code.as_deref(),
            Some("fla_truncated_record"),
            "n={n}"
        );
        if let Some(root) = s.root {
            assert!(!object(s, root).complete);
        }
        for o in &s.objects {
            if o.complete {
                assert!(o.base.as_ref().unwrap().children_complete);
            }
        }
    }
}
#[test]
fn unknown_child_keeps_pending_parent_and_its_ordered_identity_edge() {
    let a = Archive::new();
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicMorphShape");
    let end = w.bytes.len();
    w.bytes.extend([199; 50]);
    let d = a.read(&w.bytes);
    let s = &d.streams[0];
    assert_graph(s);
    assert_eq!(s.objects.len(), 2);
    assert!(s.objects.iter().all(|o| !o.complete));
    let root = object(s, s.root.unwrap());
    assert_eq!(root.children, vec![s.objects[1].object_id]);
    assert!(!root.base.as_ref().unwrap().children_complete);
    assert_eq!(root.encoded.end, end);
    assert_eq!(s.objects[1].encoded.end, end);
    assert_eq!(
        d.inspection
            .streams
            .iter()
            .find(|x| x.index == s.index)
            .unwrap()
            .bytes_consumed,
        end
    );
}
#[test]
fn cyclic_child_is_an_explicit_rejected_reference_not_new_ownership() {
    let a = Archive::new();
    let mut w = Writer::new();
    let root = w.begin("CPicPage");
    w.base_start();
    w.word(root);
    let d = a.read(&w.bytes);
    let s = &d.streams[0];
    assert_graph(s);
    assert_eq!(s.objects.len(), 1);
    assert_eq!(s.objects[0].owner, None);
    assert_eq!(s.objects[0].children, vec![u32::from(root)]);
    assert!(s.pointers.last().unwrap().is_reference);
    assert!(!s.objects[0].complete);
    assert_eq!(
        d.inspection
            .streams
            .iter()
            .find(|x| x.index == s.index)
            .unwrap()
            .stop_code
            .as_deref(),
        Some("fla_mfc_object_reference_unsupported")
    );
}
#[test]
fn layer_first_serialized_by_related_pointer_reuses_identity_in_page_children() {
    let archive = Archive::new();
    let mut writer = Writer::new();
    let root = writer.begin("CPicPage");
    writer.base_start();
    let first = writer.begin("CPicLayer");
    writer.base_start();
    writer.base_end();
    let mut related = 0;
    writer.layer_tail_with("masked layer", |writer| {
        related = writer.begin("CPicLayer");
        writer.base_start();
        writer.frame(3, "mask frame", None, false);
        writer.base_end();
        writer.layer_tail("mask layer", 0);
    });
    writer.word(related);
    writer.base_end();
    writer.page_tail();
    let document = archive.read(&writer.bytes);
    let stream = &document.streams[0];
    assert_graph(stream);
    assert!(stream.objects.iter().all(|object| object.complete));
    assert_eq!(
        object(stream, root.into()).children,
        [u32::from(first), u32::from(related)]
    );
    let target = object(stream, related.into());
    assert_eq!(target.owner, None);
    assert_eq!(
        stream.pointers[target.introduced_by_pointer].relation,
        TimelineRelation::RelatedLayer
    );
    let reused = stream
        .pointers
        .iter()
        .find(|pointer| {
            pointer.relation == TimelineRelation::Child && pointer.target == Some(related.into())
        })
        .unwrap();
    assert!(reused.is_reference);
    assert_eq!(reused.source, Some(root.into()));
    assert_eq!(
        stream
            .objects
            .iter()
            .filter(|object| object.object_id == u32::from(related))
            .count(),
        1
    );
    assert!(
        document
            .inspection
            .streams
            .iter()
            .find(|inspection| inspection.index == stream.index)
            .unwrap()
            .structure_complete
    );
}
#[test]
fn empty_f3_actions_can_finish_but_nonempty_and_revision17_are_explicit_stops() {
    let a = Archive::new();
    for rev in 10..=18 {
        let mut w = Writer::new();
        w.begin("CPicPage");
        w.base_start();
        w.begin("CPicFrame");
        w.base_start();
        w.base_end();
        w.shape();
        w.frame_fields(rev, 2, "label", None);
        w.base_end();
        w.page_tail();
        let d = a.read(&w.bytes);
        let s = &d.streams[0];
        assert_graph(s);
        let frame = s
            .objects
            .iter()
            .find(|o| o.class == TimelineClass::Frame)
            .unwrap();
        let TimelinePayload::Frame(f) = frame.payload.as_ref().unwrap() else {
            panic!()
        };
        assert_eq!(
            f.actions.as_ref().unwrap().presence,
            TimelineActionPresence::Empty
        );
        assert_eq!(frame.complete, rev != 17);
        if rev == 17 {
            assert_eq!(
                d.inspection
                    .streams
                    .iter()
                    .find(|x| x.index == s.index)
                    .unwrap()
                    .stop_code
                    .as_deref(),
                Some("fla_historical_frame_extension_unsupported")
            );
        }
        // One historical "stop" command (revision 4): kind, three empty
        // strings, frame word, no-wait sentinel, label, three operands, level
        // and seven button-event words.
        let action_start = f.actions.as_ref().unwrap().encoded.start;
        let mut command = vec![4, 2, 0, 0, 0, 1, 0, 0x80, 0xc1, 0, 0, 0, 0, 1, 0, 0, 0];
        command.extend([0; 28]);
        let mut nonempty = w.bytes.clone();
        nonempty.splice(
            action_start..action_start + 4,
            1u32.to_le_bytes()
                .into_iter()
                .chain(command.iter().copied()),
        );
        let d = a.read(&nonempty);
        let s = &d.streams[0];
        let frame = s
            .objects
            .iter()
            .find(|o| o.class == TimelineClass::Frame)
            .unwrap();
        let TimelinePayload::Frame(f) = frame.payload.as_ref().unwrap() else {
            panic!()
        };
        let actions = f.actions.as_ref().unwrap();
        assert_eq!(actions.presence, TimelineActionPresence::Historical);
        assert!(actions.complete);
        assert_eq!(actions.encoded.end, action_start + 4 + command.len());
        let decoded = actions.decoded.as_ref().unwrap();
        assert_eq!(decoded.form, TimelineActionForm::HistoricalList);
        assert_eq!(decoded.source, "stop();\n");
        assert_eq!(frame.complete, rev != 17);
        // A truncated command is an explicit stop, never a guessed boundary.
        let d = a.read(&nonempty[..action_start + 4 + 20]);
        let frame = d.streams[0]
            .objects
            .iter()
            .find(|o| o.class == TimelineClass::Frame)
            .unwrap();
        let TimelinePayload::Frame(f) = frame.payload.as_ref().unwrap() else {
            panic!()
        };
        let actions = f.actions.as_ref().unwrap();
        assert!(!actions.complete && !frame.complete);
        assert!(!actions.decoded.as_ref().unwrap().commands[0].complete);
    }
}
#[test]
fn dangling_reference_never_becomes_an_identity_edge() {
    let a = Archive::new();
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    w.word(700);
    let d = a.read(&w.bytes);
    let s = &d.streams[0];
    assert_graph(s);
    assert_eq!(s.pointers.len(), 1);
    assert!(s.objects[0].children.is_empty());
    assert_eq!(
        d.inspection
            .streams
            .iter()
            .find(|x| x.index == s.index)
            .unwrap()
            .stop_code
            .as_deref(),
        Some("fla_mfc_reference")
    );
}

#[test]
fn extended_class_and_object_tags_share_the_same_identity_table() {
    let archive = Archive::new();
    let bytes = timeline();
    let before = archive.read(&bytes);
    let stream = &before.streams[0];
    let frame_class = stream
        .references
        .iter()
        .position(|r| {
            matches!(
                r,
                TimelineReference::Class {
                    class: TimelineClass::Frame,
                    ..
                }
            )
        })
        .unwrap() as u32;
    let second_frame = stream
        .objects
        .iter()
        .filter(|o| o.class == TimelineClass::Frame)
        .nth(1)
        .unwrap();
    let class_pointer = &stream.pointers[second_frame.introduced_by_pointer];
    let related_pointer = stream
        .pointers
        .iter()
        .find(|p| p.relation == TimelineRelation::RelatedLayer && p.target.is_some())
        .unwrap();
    let mut edits = [
        (class_pointer.encoded, 0x80000000 | frame_class),
        (related_pointer.encoded, related_pointer.target.unwrap()),
    ];
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    let mut changed = bytes.clone();
    for (span, tag) in edits {
        let replacement: Vec<_> = 0x7fffu16
            .to_le_bytes()
            .into_iter()
            .chain(tag.to_le_bytes())
            .collect();
        changed.splice(span.start..span.end, replacement);
    }
    let after = archive.read(&changed);
    let stream = &after.streams[0];
    assert_graph(stream);
    assert!(stream.objects.iter().all(|o| o.complete));
    assert_eq!(
        stream
            .objects
            .iter()
            .map(|o| o.object_id)
            .collect::<Vec<_>>(),
        before.streams[0]
            .objects
            .iter()
            .map(|o| o.object_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(stream.pointers.len(), before.streams[0].pointers.len());
    assert_eq!(
        stream.pointers[class_pointer_index(&before.streams[0], second_frame.object_id)]
            .encoded
            .end
            - stream.pointers[class_pointer_index(&before.streams[0], second_frame.object_id)]
                .encoded
                .start,
        6
    );
}

fn class_pointer_index(stream: &BinaryTimelineStream, id: u32) -> usize {
    object(stream, id).introduced_by_pointer
}

#[test]
fn action_presence_requires_proven_content_and_nonempty_compiled_actions_stop() {
    let archive = Archive::new();
    let bytes = timeline();
    let original = archive.read(&bytes);
    let original_frame = original.streams[0]
        .objects
        .iter()
        .find(|o| o.class == TimelineClass::Frame)
        .unwrap();
    let TimelinePayload::Frame(fields) = original_frame.payload.as_ref().unwrap() else {
        panic!()
    };
    let span = fields.actions.as_ref().unwrap().encoded;
    for (count, expected) in [
        (0, TimelineActionPresence::Empty),
        (1, TimelineActionPresence::Compiled),
        (u32::MAX, TimelineActionPresence::Unknown),
    ] {
        // Compiled list revision 0 stores the simple "play" command as its type only.
        let payload: Vec<_> = [0u32, 0, 0, count]
            .into_iter()
            .chain((count == 1).then_some(1))
            .flat_map(u32::to_le_bytes)
            .collect();
        let mut changed = bytes.clone();
        changed.splice(span.start..span.end, payload);
        let parsed = archive.read(&changed);
        let stream = &parsed.streams[0];
        assert_graph(stream);
        let frame = object(stream, original_frame.object_id);
        let TimelinePayload::Frame(fields) = frame.payload.as_ref().unwrap() else {
            panic!()
        };
        let actions = fields.actions.as_ref().unwrap();
        assert_eq!(actions.presence, expected);
        assert_eq!(actions.command_count, Some(count));
        assert_eq!(actions.complete, count != u32::MAX);
        assert_eq!(frame.complete, count != u32::MAX);
        assert_eq!(
            actions.encoded.end,
            span.start + if count == 1 { 20 } else { 16 }
        );
        if count == 1 {
            assert_eq!(actions.decoded.as_ref().unwrap().source, "play();\n");
        }
        if count == u32::MAX {
            assert_eq!(
                parsed
                    .inspection
                    .streams
                    .iter()
                    .find(|s| s.index == stream.index)
                    .unwrap()
                    .stop_code
                    .as_deref(),
                Some("fla_compiled_actions_unsupported")
            );
        }
    }
    for end in [span.start + 5, span.start + 8, span.start + 10] {
        let parsed = archive.read(&bytes[..end]);
        let frame = object(&parsed.streams[0], original_frame.object_id);
        let TimelinePayload::Frame(fields) = frame.payload.as_ref().unwrap() else {
            panic!()
        };
        let actions = fields.actions.as_ref().unwrap();
        assert_eq!(actions.presence, TimelineActionPresence::Unknown);
        assert!(!actions.complete);
    }
}

#[test]
fn timeline_retention_budget_is_shared_and_keeps_a_valid_partial_graph() {
    let archive = Archive::new();
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    let mut last = 0;
    for _ in 0..12_600 {
        last = w.begin("CPicShape");
        w.base_start();
        w.base_end();
        w.shape();
    }
    // Repeated child references to a complete identity each retain a pointer.
    // About 650000 items per stream; two streams exceed the shared
    // 1000000-item document budget.
    for _ in 0..600_000 {
        w.word(last);
    }
    w.base_end();
    w.page_tail();
    let path = archive.files(&[&w.bytes, &w.bytes]);
    let d = read_binary_timeline(&path).unwrap();
    assert_eq!(d.streams.len(), 2);
    for stream in &d.streams {
        assert_graph(stream);
    }
    assert!(
        d.inspection
            .streams
            .iter()
            .any(|s| s.stop_code.as_deref() == Some("fla_timeline_document_limit"))
    );
    assert!(d.streams[1].pointers.len() < d.streams[0].pointers.len());
    // Import readiness uses the same retained graph as editable conversion.
    // A budget stop is always reported; only the records read before it can
    // be converted, never content fabricated past the stop.
    let report = inspect(&path).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "fla_timeline_document_limit")
    );
    let ordinary = report.binary_fla.unwrap();
    let retained: Vec<_> = d
        .inspection
        .streams
        .iter()
        .map(|stream| (stream.index, stream.bytes_consumed, &stream.stop_code))
        .collect();
    let inspected: Vec<_> = ordinary
        .streams
        .iter()
        .map(|stream| (stream.index, stream.bytes_consumed, &stream.stop_code))
        .collect();
    assert_eq!(inspected, retained);
}

impl Writer {
    fn placement(&mut self, class: &'static str, name: &str, source: &str) {
        self.begin(class);
        self.base_start();
        self.base_end();
        self.byte(22); // CS4 placement, independent of extension revision.
        for n in [65536, 0, 0, 65536, 120, 240] {
            self.dword(n);
        }
        self.word(3);
        self.word(1);
        self.byte(1); // versioned color transform, alpha then RGB pairs.
        for pair in [[128, 0], [256, 4], [256, 5], [256, 6]] {
            self.word(pair[0]);
            self.word(pair[1]);
        }
        self.word(0);
        self.word(0);
        self.dword(0);
        self.string("");
        self.dword(19);
        self.bytes.extend([0, 0, 0, 0, 0]); // effects, persistent, extensible, filters, blend.
        self.word(0);
        for i in 0..16 {
            self.bytes
                .extend((if i % 5 == 0 { 1.0_f32 } else { 0.0 }).to_le_bytes());
        }
        for _ in 0..3 {
            self.bytes.extend(0.0_f64.to_le_bytes());
        }
        for _ in 0..3 {
            self.dword(0);
        }
        self.word(0); // animation settings.
        self.byte(if class == "CPicSprite" { 8 } else { 11 });
        self.dword(1);
        self.dword(1);
        self.dword(77);
        self.dword(0);
        self.string(source);
        if class == "CPicButton" {
            self.byte(0);
        }
        self.string(name);
        if class == "CPicSprite" {
            self.byte(2);
            self.dword(0);
            self.dword(1);
        }
        self.dword(0); // accessibility.
        if class == "CPicSprite" {
            self.dword(0);
            self.byte(0);
            self.dword(0);
            self.string(""); // component XML.
        }
    }
    fn static_text(&mut self) {
        self.begin("CPicText");
        self.base_start();
        self.base_end();
        self.byte(8);
        for n in [65536, 0, 0, 65536, 0, 0, 0, 2000, 0, 400] {
            self.dword(n);
        }
        self.bytes.extend([1, 0]);
        self.dword(0);
        self.word(0);
        self.string("");
        self.word(5);
        self.byte(8);
        self.word(240);
        self.string("Arial\0");
        self.dword(0xff336699);
        self.bytes.extend([0, 0, 1, 0, 0, 1, 0, 2]);
        for _ in 0..5 {
            self.word(0);
        }
        self.string("");
        self.bytes.extend([0, 0, 0, 0]);
        self.string("");
        self.bytes.extend(b"Hello");
        self.word(0);
        self.string("");
        self.dword(0);
    }
}

fn placement_text_ease_family() -> Vec<u8> {
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    w.placement("CPicSprite", "child", "trace('clip');");
    w.placement("CPicButton", "button", "on(release) { trace('release'); }");
    w.static_text();
    w.base_end();
    w.shape();
    w.frame_fields(24, 12, "moving", Some("trace('frame');"));
    w.dword(0);
    w.dword(1);
    w.dword(1); // bookmark, single-curve, custom present.
    for _ in 0..6 {
        w.dword(2);
        for knot in [[0., 0., 0., 0., 0.25, 0.1], [0.75, 0.9, 1., 1., 1., 1.]] {
            for value in knot {
                w.bytes.extend(f64::to_le_bytes(value));
            }
        }
    }
    w.base_end();
    w.layer_tail("motion", 0);
    w.base_end();
    w.page_tail();
    w.bytes
}

#[test]
fn placements_text_actions_and_custom_ease_keep_typed_ownership_and_raw_spans() {
    let archive = Archive::new();
    let bytes = placement_text_ease_family();
    let d = archive.read(&bytes);
    let s = &d.streams[0];
    let status = d
        .inspection
        .streams
        .iter()
        .find(|x| x.index == s.index)
        .unwrap();
    assert!(status.structure_complete, "{status:?}");
    assert_graph(s);
    let mut places = 0;
    let mut texts = 0;
    for object in &s.objects {
        match object.payload.as_ref().unwrap() {
            TimelinePayload::Instance(value) => {
                places += 1;
                assert_eq!(value.library_id, Some(19));
                assert_eq!(value.first_frame, Some(3));
                assert_eq!(value.loop_mode, Some(1));
                assert_eq!(
                    value.color_transform.as_ref().unwrap().multipliers,
                    [256, 256, 256, 128]
                );
                assert!(value.actions.as_ref().unwrap().complete);
                assert_eq!(
                    value.actions.as_ref().unwrap().presence,
                    TimelineActionPresence::Source
                );
                assert!(object.owner.is_some());
                assert_eq!(value.matrix_3d.unwrap()[15], 1.);
            }
            TimelinePayload::Text(value) => {
                texts += 1;
                assert_eq!(value.runs.len(), 1);
                let run = &value.runs[0];
                assert_eq!(text(s, &run.characters), b"Hello");
                assert_eq!(text(s, run.style.font_family.as_ref().unwrap()), b"Arial");
                assert_eq!(run.style.font_size_twips, Some(240));
                assert_eq!(run.style.alignment, Some(2));
            }
            TimelinePayload::Frame(value) => {
                assert_eq!(value.duration, Some(12));
                assert_eq!(value.custom_ease.len(), 6);
                assert!(value.custom_ease.iter().all(|v| v.len() == 2));
                assert_eq!(value.custom_ease[0][0][4], 0.25);
                assert_eq!(
                    text(s, value.actions.as_ref().unwrap().source.as_ref().unwrap()),
                    b"trace('frame');"
                );
            }
            _ => {}
        }
    }
    assert_eq!((places, texts), (2, 1));
    // Cut inside every newly decoded payload and within the final easing table.
    // Completed ancestors must never be fabricated from a partial stream.
    let mut cuts: Vec<_> = s
        .objects
        .iter()
        .filter(|o| {
            matches!(
                o.class,
                TimelineClass::Sprite | TimelineClass::Button | TimelineClass::Text
            )
        })
        .flat_map(|o| [o.body_offset + 1, o.encoded.end - 1])
        .collect();
    cuts.extend([bytes.len() - 40, bytes.len() - 100]);
    for end in cuts {
        let partial = archive.read(&bytes[..end]);
        let p = &partial.streams[0];
        assert_graph(p);
        let status = partial
            .inspection
            .streams
            .iter()
            .find(|x| x.index == p.index)
            .unwrap();
        assert!(!status.structure_complete);
        assert!(status.bytes_consumed <= end);
        assert!(!object(p, p.root.unwrap()).complete);
    }
}

impl Writer {
    fn float(&mut self, v: f32) {
        self.bytes.extend(v.to_le_bytes());
    }
    fn ints(&mut self, values: &[i32]) {
        for v in values {
            self.dword(*v as u32);
        }
    }
    /// One page, one layer, one frame whose display children are written by
    /// `children`; the frame tail optionally carries a morph pointer.
    fn single_frame(
        &mut self,
        duration: u16,
        children: impl FnOnce(&mut Self),
        morph: impl FnOnce(&mut Self),
    ) {
        self.begin("CPicPage");
        self.base_start();
        self.begin("CPicLayer");
        self.base_start();
        self.begin("CPicFrame");
        self.base_start();
        children(self);
        self.base_end();
        self.shape();
        self.frame_fields_with(21, duration, "", None, morph);
        self.base_end();
        self.layer_tail("layer", 0);
        self.base_end();
        self.page_tail();
    }
    /// CPicSymbol revision 16: filters (revision >= 15) then blend (>= 16).
    fn filtered_symbol(&mut self, filters: impl FnOnce(&mut Self), blend: u8) {
        self.begin("CPicSymbol");
        self.base_start();
        self.base_end();
        self.byte(16);
        self.ints(&[65536, 0, 0, 65536, 20, 40]);
        self.word(0);
        self.word(0);
        self.byte(1);
        for _ in 0..4 {
            self.word(256);
            self.word(0);
        }
        self.word(0);
        self.word(0);
        self.dword(0);
        self.string("");
        self.dword(5);
        self.bytes.extend([0, 0, 0]); // effect, persistent data, extensible.
        filters(self);
        self.byte(blend);
    }
    fn filter_state(&mut self, enabled: u32) {
        self.byte(1);
        self.dword(enabled);
    }
    /// Shared shadow settings, revision 3 or 4.
    fn shadow_settings(&mut self, rev: u8, color: u32) {
        self.byte(rev);
        self.filter_state(1);
        self.dword(color);
        self.float(2.); // distance
        self.float(5.); // blur x
        self.float(6.); // blur y
        self.float(0.5); // angle
        self.ints(&[0, 1, 3, 150]); // inner, knockout, quality, strength
        if rev >= 4 {
            self.dword(1);
        }
    }
    fn gradient_block(&mut self) {
        self.byte(1);
        self.shadow_settings(4, 0xff00_0000);
        self.dword(2);
        self.dword(9); // gradient value
        self.dword(1); // full
        self.dword(0);
        self.dword(0xff00_00ff);
        self.dword(255);
        self.dword(0x8000_ff00);
    }
}

fn status(d: &BinaryTimelineDocument) -> &BinaryStreamInspection {
    d.inspection
        .streams
        .iter()
        .find(|x| x.index == d.streams[0].index)
        .unwrap()
}

fn all_filters(w: &mut Writer) {
    w.byte(1);
    w.dword(12);
    // Drop shadow, shared settings revision 4.
    w.byte(0);
    w.shadow_settings(4, 0x8000_0000);
    // Blur revisions 1, 2 and 3.
    w.byte(1);
    w.byte(1);
    w.dword(0xffff_ffff);
    w.float(3.);
    w.float(4.);
    w.byte(1);
    w.byte(2);
    w.float(7.);
    w.float(8.);
    w.filter_state(0);
    w.dword(2);
    w.byte(1);
    w.byte(3);
    w.shadow_settings(3, 0);
    // Glow revisions 1 and 2.
    w.byte(2);
    w.byte(1);
    w.dword(0xff00_00ff);
    w.float(10.);
    w.ints(&[1, 0]);
    w.byte(2);
    w.byte(2);
    w.dword(0xff00_00ff);
    w.float(10.);
    w.ints(&[0, 0]);
    w.filter_state(1);
    w.dword(3);
    // Bevel revisions 2 and 3.
    w.byte(3);
    w.byte(2);
    w.float(0.25);
    w.dword(0xff00_0000);
    w.float(4.);
    w.float(1.);
    w.ints(&[1, 0]);
    w.filter_state(1);
    w.dword(1);
    w.byte(7);
    w.dword(0xffff_ffff);
    w.dword(11);
    w.dword(0);
    w.byte(3);
    w.byte(3);
    w.shadow_settings(4, 0xff00_0000);
    w.dword(0xffff_ffff);
    w.dword(1);
    // Gradient glow; adjust color; gradient bevel; settings-free shadow.
    w.byte(4);
    w.gradient_block();
    w.byte(6);
    w.byte(1);
    w.filter_state(1);
    for v in [10., -20., -8., -115.] {
        w.float(v);
    }
    w.byte(7);
    w.byte(1);
    w.gradient_block();
    w.byte(0);
    w.byte(0);
}

#[test]
fn every_filter_kind_and_blend_mode_is_typed_and_parsing_continues() {
    let archive = Archive::new();
    let mut w = Writer::new();
    w.single_frame(
        1,
        |w| {
            w.filtered_symbol(all_filters, 3);
            w.filtered_symbol(|w| w.byte(0), 1);
        },
        |w| w.word(0),
    );
    let bytes = w.bytes.clone();
    let d = archive.read(&bytes);
    assert!(status(&d).structure_complete, "{:?}", status(&d));
    let s = &d.streams[0];
    assert_graph(s);
    let instances: Vec<_> = s
        .objects
        .iter()
        .filter_map(|o| match o.payload.as_ref() {
            Some(TimelinePayload::Instance(i)) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(instances.len(), 2);
    let (filtered, plain) = (instances[0], instances[1]);
    assert!(plain.filters.is_empty());
    assert_eq!(plain.blend(), Some(TimelineBlendMode::Normal));
    assert_eq!(filtered.blend(), Some(TimelineBlendMode::Multiply));
    assert_eq!(filtered.library_id, Some(5));
    let f = &filtered.filters;
    use TimelineFilterKind::*;
    assert_eq!(
        f.iter().map(|f| (f.kind, f.revision)).collect::<Vec<_>>(),
        [
            (DropShadow, 4),
            (Blur, 1),
            (Blur, 2),
            (Blur, 3),
            (Glow, 1),
            (Glow, 2),
            (Bevel, 2),
            (Bevel, 3),
            (GradientGlow, 1),
            (AdjustColor, 1),
            (GradientBevel, 1),
            (DropShadow, 0),
        ]
    );
    // Exact contiguous spans from the count word through the blend byte.
    let list = filtered.filters_encoded.unwrap();
    assert_eq!(f[0].encoded.start, list.start + 5);
    for pair in f.windows(2) {
        assert_eq!(pair[0].encoded.end, pair[1].encoded.start);
    }
    assert_eq!(f[11].encoded.end, list.end);
    assert_eq!(bytes[list.end], 3);
    let shadow = &f[0].params;
    assert_eq!(f[0].enabled, Some(true));
    assert_eq!(shadow.color_rgba, Some(0x8000_0000));
    assert_eq!(
        (
            shadow.distance,
            shadow.blur_x,
            shadow.blur_y,
            shadow.angle_radians
        ),
        (Some(2.), Some(5.), Some(6.), Some(0.5))
    );
    assert_eq!(
        (
            shadow.knockout,
            shadow.quality,
            shadow.strength_percent,
            shadow.hide_object
        ),
        (Some(1), Some(3), Some(150), Some(1))
    );
    assert_eq!(f[1].enabled, None);
    assert_eq!(f[1].params.legacy_color_rgba, Some(0xffff_ffff));
    assert_eq!(
        (f[1].params.blur_x, f[1].params.blur_y),
        (Some(3.), Some(4.))
    );
    assert_eq!(f[2].enabled, Some(false));
    assert_eq!(f[2].params.quality, Some(2));
    assert_eq!(f[3].params.settings_revision, Some(3));
    assert_eq!(f[3].params.hide_object, None);
    assert_eq!(f[4].params.inner, Some(1));
    assert_eq!(f[5].params.quality, Some(3));
    assert_eq!(f[6].params.legacy_byte, Some(7));
    assert_eq!(f[6].params.legacy_word, Some(11));
    assert_eq!(f[6].params.highlight_color_rgba, Some(0xffff_ffff));
    assert_eq!(f[7].params.full, Some(1));
    for gradient in [&f[8], &f[10]] {
        let p = &gradient.params;
        assert_eq!(p.gradient_revision, Some(1));
        assert_eq!(p.gradient_value, Some(9));
        assert_eq!(p.full, Some(1));
        assert_eq!(
            p.gradient_stops,
            [
                TimelineFilterGradientStop {
                    ratio: 0,
                    color_rgba: 0xff00_00ff
                },
                TimelineFilterGradientStop {
                    ratio: 255,
                    color_rgba: 0x8000_ff00
                }
            ]
        );
    }
    let adjust = &f[9].params;
    assert_eq!(
        (
            adjust.brightness,
            adjust.contrast,
            adjust.saturation,
            adjust.hue
        ),
        (Some(10.), Some(-20.), Some(-8.), Some(-115.))
    );
    assert_eq!(
        f[11].params,
        TimelineFilterParams {
            settings_revision: Some(0),
            ..Default::default()
        }
    );
    // Every cut inside the filter list leaves an incomplete instance and a
    // valid partial graph; no later field is guessed.
    for end in list.start + 1..list.end {
        let partial = archive.read(&bytes[..end]);
        assert_graph(&partial.streams[0]);
        // A count larger than the remaining input is rejected before reading.
        assert!(
            matches!(
                status(&partial).stop_code.as_deref(),
                Some("fla_truncated_record" | "fla_record_limit")
            ),
            "cut {end}"
        );
    }
}

#[test]
fn unknown_filter_types_and_unproven_revisions_stop_explicitly() {
    let archive = Archive::new();
    for (filter, code) in [
        (vec![5, 1], "fla_filter_type_unsupported"),
        (vec![3, 1], "fla_record_revision_unsupported"),
        (vec![0, 5], "fla_record_revision_unsupported"),
        (vec![4, 1, 0, 16, 0, 0, 0], "fla_gradient_stop_limit"),
    ] {
        let mut w = Writer::new();
        let mut list = 0;
        w.single_frame(
            1,
            |w| {
                w.filtered_symbol(
                    |w| {
                        w.byte(1);
                        w.dword(1);
                        list = w.bytes.len();
                        w.bytes.extend(&filter);
                    },
                    1,
                )
            },
            |w| w.word(0),
        );
        let d = archive.read(&w.bytes);
        assert_graph(&d.streams[0]);
        assert_eq!(status(&d).stop_code.as_deref(), Some(code), "{filter:?}");
        assert!(status(&d).bytes_consumed > list);
    }
}

fn morph_shape(w: &mut Writer) {
    w.begin("CPicMorphShape");
    w.byte(2);
    w.ints(&[65536, 0, 0, 65536, 0, 0]);
    w.ints(&[65536, 0, 0, 65536, 200, -100]);
    w.dword(7);
    w.ints(&[-30]);
    w.word(2); // segments
    w.begin("CMorphSegment");
    w.ints(&[0, 0, 0, 1, 100, 200, 300, 400]);
    w.word(2); // curves
    w.begin("CMorphCurve");
    w.ints(&[1, 2, 3, 4, 5, 6, 7, 8]);
    w.dword(1);
    w.word(0); // A null curve entry is a value, not a terminator.
    w.word(0); // Null segment entry.
    w.word(0xffff); // Extended count form.
    w.dword(1);
    w.begin("CMorphHintItem");
    w.ints(&[10, 20, 30, 40, 0]);
    w.word(2); // Fills: plain, then a gradient without the focal extension.
    w.dword(0xff33_2211);
    w.bytes.extend([0, 0]);
    w.dword(0xff00_0000);
    w.bytes.extend([0x10, 0]);
    w.ints(&[1392, 0, 0, 1392, -200, -105]);
    w.byte(1);
    w.byte(255);
    w.dword(0xff00_00ff);
    w.word(1); // Strokes, without extension fields.
    w.dword(0xff00_0000);
    w.word(20);
    w.word(0);
    w.word(1);
}

#[test]
fn shape_tween_morph_records_are_typed_identities_and_parsing_continues() {
    let archive = Archive::new();
    let mut w = Writer::new();
    let mut morph_start = 0;
    let mut morph_end = 0;
    w.single_frame(
        5,
        |_| {},
        |w| {
            morph_start = w.bytes.len();
            morph_shape(w);
            morph_end = w.bytes.len();
        },
    );
    let bytes = w.bytes.clone();
    let d = archive.read(&bytes);
    assert!(status(&d).structure_complete, "{:?}", status(&d));
    let s = &d.streams[0];
    assert_graph(s);
    let frame = s
        .objects
        .iter()
        .find(|o| o.class == TimelineClass::Frame)
        .unwrap();
    let TimelinePayload::Frame(fields) = frame.payload.as_ref().unwrap() else {
        panic!()
    };
    let morph = object(s, fields.morph_shape.unwrap());
    assert_eq!(morph.class, TimelineClass::Morph);
    assert!(morph.complete && morph.base.is_none() && morph.owner.is_none());
    assert_eq!(
        (morph.encoded.start, morph.encoded.end),
        (morph_start, morph_end)
    );
    let TimelinePayload::Morph(m) = morph.payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(m.matrix_words_end, Some([65536, 0, 0, 65536, 200, -100]));
    assert_eq!((m.extension_value, m.acceleration), (Some(7), Some(-30)));
    assert_eq!(m.segments.len(), 2);
    assert_eq!(m.segments[1], None);
    assert_eq!(m.hints.len(), 1);
    assert_eq!(m.fills.len(), 2);
    assert!(matches!(
        &m.fills[1].encoding,
        BinaryFillEncoding::Extended {
            payload: BinaryFillPayload::Gradient { extension: None, stops, .. },
            ..
        } if stops.len() == 1
    ));
    assert_eq!(m.strokes.len(), 1);
    assert!(m.strokes[0].extension.is_none());
    let segment = object(s, m.segments[0].unwrap());
    let TimelinePayload::MorphSegment(seg) = segment.payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(
        (seg.fill_index_1, seg.fill_index_2, seg.start_point_b),
        (Some(0), Some(1), Some([300, 400]))
    );
    assert_eq!(seg.curves.len(), 2);
    assert_eq!(seg.curves[1], None);
    let curve = object(s, seg.curves[0].unwrap());
    let TimelinePayload::MorphCurve(curve) = curve.payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(curve.anchor_point_b, Some([7, 8]));
    assert_eq!(curve.line_flags, Some(1));
    let hint = object(s, m.hints[0].unwrap());
    let TimelinePayload::MorphHint(hint) = hint.payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!((hint.point_b, hint.index), (Some([30, 40]), Some(0)));
    for (relation, source) in [
        (TimelineRelation::MorphShape, frame.object_id),
        (TimelineRelation::MorphSegment, morph.object_id),
        (TimelineRelation::MorphCurve, segment.object_id),
        (TimelineRelation::MorphHint, morph.object_id),
    ] {
        assert!(
            s.pointers
                .iter()
                .any(|p| p.relation == relation && p.source == Some(source))
        );
    }
    for end in morph_start..morph_end {
        let partial = archive.read(&bytes[..end]);
        assert_graph(&partial.streams[0]);
        assert!(!status(&partial).structure_complete);
        assert!(status(&partial).bytes_consumed <= end);
    }
}

#[test]
fn morph_classes_are_only_accepted_through_their_owning_pointer() {
    let archive = Archive::new();
    // A morph segment where a morph shape is expected.
    let mut w = Writer::new();
    w.single_frame(
        1,
        |_| {},
        |w| {
            w.begin("CMorphSegment");
            w.bytes.extend([0; 40]);
        },
    );
    let d = archive.read(&w.bytes);
    assert_graph(&d.streams[0]);
    assert_eq!(
        status(&d).stop_code.as_deref(),
        Some("fla_mfc_class_unsupported")
    );
    // A display record inside a morph segment list.
    let mut w = Writer::new();
    w.single_frame(
        1,
        |_| {},
        |w| {
            w.begin("CPicMorphShape");
            w.byte(0);
            w.ints(&[0; 12]);
            w.word(1);
            w.begin("CPicShape");
            w.bytes.extend([0; 40]);
        },
    );
    let d = archive.read(&w.bytes);
    assert_graph(&d.streams[0]);
    assert_eq!(
        status(&d).stop_code.as_deref(),
        Some("fla_mfc_class_unsupported")
    );
}

#[test]
fn zero_duration_leading_frame_occupies_no_time_and_parsing_continues() {
    let archive = Archive::new();
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.frame(0, "", None, false);
    w.frame(4, "", None, false);
    w.base_end();
    w.layer_tail("layer", 0);
    w.base_end();
    w.page_tail();
    let d = archive.read(&w.bytes);
    assert!(status(&d).structure_complete);
    let durations: Vec<_> = d.streams[0]
        .objects
        .iter()
        .filter_map(|o| match o.payload.as_ref() {
            Some(TimelinePayload::Frame(f)) => f.duration,
            _ => None,
        })
        .collect();
    assert_eq!(durations, [0, 4]);
}

fn typed_placements(w: &mut Writer) {
    w.begin("CPicRect");
    w.base_start();
    w.base_end();
    w.shape();
    w.byte(1);
    w.byte(1);
    w.ints(&[20, 40, 60, 80]);
    w.begin("CPicOval");
    w.base_start();
    w.base_end();
    w.shape();
    w.byte(3);
    w.byte(0);
    w.ints(&[0, 2000, 0, 1000]);
    w.float(0.);
    w.float(90.);
    w.float(25.);
    w.begin("CPicSwf");
    w.base_start();
    w.base_end();
    w.byte(7);
    w.ints(&[65536, 0, 0, 65536, 0, 0]);
    w.dword(1);
    w.word(12);
    w.ints(&[1, 1, 77, 0]); // Source actions: revision, mode, id, breakpoints.
    w.string("");
    w.string("clip");
    w.byte(2); // Property list revision 2 with two typed entries.
    w.dword(2);
    w.byte(3);
    w.string("speed");
    w.byte(8);
    w.word(17);
    w.word(4); // Selection word of list type 17.
    w.dword(1);
    w.dword(0);
    w.dword(1);
    w.string("fast");
    w.string("10");
    w.word(3);
    w.dword(2);
    w.bytes.extend([9, 9]);
    w.string("speedVar");
    w.bytes.extend([1; 17]);
    w.string("Motion");
    w.dword(1);
    w.byte(3);
    w.string("rows");
    w.byte(5);
    w.word(13);
    w.dword(0);
    w.dword(0);
    w.dword(0);
    w.string("Collection");
    w.string("Item");
    w.string("label");
    w.dword(1);
    w.string("label");
    w.word(1);
    w.string("");
    w.string("");
    w.dword(2);
    w.string("a");
    w.string("b");
    w.string("rowsVar");
    w.bytes.extend([2; 17]);
    w.string("");
    w.dword(0);
    w.dword(1); // Parameters are new.
    w.dword(0); // Accessibility revision 0.
    w.byte(1);
    w.dword(5);
    w.string("");
    w.byte(0);
    w.begin("CPicSwf");
    w.base_start();
    w.base_end();
    w.byte(4);
    w.ints(&[65536, 0, 0, 65536, 0, 0]);
    w.dword(0);
    w.ints(&[1, 2, 0, 0]); // Empty source actions.
    w.string("legacy");
    w.byte(0); // Legacy property list.
    w.dword(1);
    w.dword(3);
    w.string("name");
    w.string("value");
    w.begin("CPicVideoStream");
    w.base_start();
    w.base_end();
    w.byte(4);
    w.ints(&[65536, 0, 0, 65536, 0, 0]);
    w.ints(&[0, 3200, 0, 2400]);
    w.byte(0);
    w.string("");
    w.string("video");
    w.dword(1);
    w.word(6);
}

#[test]
fn primitives_compiled_clips_video_and_component_parameters_are_typed() {
    let archive = Archive::new();
    let mut w = Writer::new();
    w.single_frame(1, typed_placements, |w| w.word(0));
    let d = archive.read(&w.bytes);
    assert!(status(&d).structure_complete, "{:?}", status(&d));
    let s = &d.streams[0];
    assert_graph(s);
    let mut seen = [0; 3];
    for o in &s.objects {
        match o.payload.as_ref().unwrap() {
            TimelinePayload::Primitive(p) => {
                seen[0] += 1;
                assert!(o.shape_index.is_some());
                if o.class == TimelineClass::Rect {
                    assert_eq!(p.corner_radii, Some([20, 40, 60, 80]));
                } else {
                    assert_eq!(p.bounds_words, Some([0, 2000, 0, 1000]));
                    assert_eq!(p.inner_radius_percent, Some(25.));
                }
            }
            TimelinePayload::CompiledClip(c) => {
                seen[1] += 1;
                let props = c.properties.as_ref().unwrap();
                if c.revision == Some(7) {
                    assert_eq!(c.media_id, Some(12));
                    assert_eq!(text(s, c.instance_name.as_ref().unwrap()), b"clip");
                    assert_eq!(props.entries.len(), 2);
                    assert_eq!(props.parameters_are_new, Some(1));
                    let speed = props.entries[0].value.as_ref().unwrap();
                    assert_eq!((speed.value_type, speed.selection_word), (17, Some(4)));
                    assert_eq!(text(s, &speed.entries[0].value), b"10");
                    assert_eq!(speed.data.map(|d| d.end - d.start), Some(2));
                    assert_eq!(props.entries[0].verbosity, Some(1));
                    assert!(props.entries[1].value.as_ref().unwrap().table.is_some());
                    assert_eq!(c.reserved_byte, Some(0));
                } else {
                    assert_eq!(c.media_id, None);
                    assert_eq!(props.revision, 0);
                    assert_eq!(props.entries[0].legacy_type, Some(3));
                    assert_eq!(
                        text(s, props.entries[0].legacy_value.as_ref().unwrap()),
                        b"value"
                    );
                }
            }
            TimelinePayload::Video(v) => {
                seen[2] += 1;
                assert_eq!(v.media_id, Some(6));
                assert_eq!(v.bounds_words, Some([0, 3200, 0, 2400]));
                assert_eq!(text(s, v.instance_name.as_ref().unwrap()), b"video");
            }
            _ => {}
        }
    }
    assert_eq!(seen, [2, 2, 1]);
}

/// Revision-4 historical command: kind, empty URL/window/scene, frame, no wait,
/// label, three operands, level 1 and seven button-event words.
fn historical_command(kind: u8, label: &str, release: bool) -> Vec<u8> {
    let mut w = Writer::new();
    w.bytes.clear();
    w.byte(4);
    w.byte(kind);
    w.bytes.extend([0, 0, 0]);
    w.word(1);
    w.word(0xc180);
    w.string(label);
    w.bytes.extend([0, 0, 0]);
    w.dword(1);
    w.dword(u32::from(release));
    w.bytes.extend([0; 24]);
    w.bytes
}

#[test]
fn historical_frame_and_button_actions_decode_through_the_timeline_reader() {
    let archive = Archive::new();
    // Frame revision 9 stores exactly one inline historical command.
    let mut w = Writer::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    w.base_end();
    w.shape();
    w.byte(9);
    w.word(1);
    for v in [0, 7, 0, 0, 1] {
        w.word(v);
    }
    w.byte(0);
    w.dword(0);
    w.dword(0);
    w.word(0);
    w.string("");
    let command = historical_command(10, "intro", false);
    w.bytes.extend(&command);
    w.base_end();
    w.page_tail();
    let d = archive.read(&w.bytes);
    let s = &d.streams[0];
    assert!(d.inspection.streams[0].structure_complete);
    let frame = s
        .objects
        .iter()
        .find(|o| o.class == TimelineClass::Frame)
        .unwrap();
    let TimelinePayload::Frame(f) = frame.payload.as_ref().unwrap() else {
        panic!()
    };
    let actions = f.actions.as_ref().unwrap();
    assert_eq!(actions.presence, TimelineActionPresence::Historical);
    assert_eq!(actions.encoded.end - actions.encoded.start, command.len());
    let decoded = actions.decoded.as_ref().unwrap();
    assert_eq!(decoded.form, TimelineActionForm::HistoricalCommand);
    assert_eq!(decoded.source, "gotoAndPlay(\"intro\");\n");

    // Button revision 4 stores a counted list followed by track-as-menu.
    let bytes = placement_text_ease_family();
    let before = archive.read(&bytes);
    let s = &before.streams[0];
    let button = s
        .objects
        .iter()
        .find(|o| o.class == TimelineClass::Button)
        .unwrap();
    let TimelinePayload::Instance(i) = button.payload.as_ref().unwrap() else {
        panic!()
    };
    let start = i.actions.as_ref().unwrap().encoded.start;
    let mut list = vec![4];
    list.extend(3u32.to_le_bytes());
    for (kind, label, release) in [(20, "", true), (10, "next", false), (21, "", false)] {
        list.extend(historical_command(kind, label, release));
    }
    list.push(1);
    let mut changed = bytes.clone();
    changed.splice(start - 1..button.encoded.end, list);
    let d = archive.read(&changed);
    let status = &d.inspection.streams[0];
    assert!(status.structure_complete, "{status:?}");
    let button = d.streams[0]
        .objects
        .iter()
        .find(|o| o.class == TimelineClass::Button)
        .unwrap();
    let TimelinePayload::Instance(i) = button.payload.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(i.extension_revision, Some(4));
    assert_eq!(i.track_as_menu, Some(1));
    let decoded = i.actions.as_ref().unwrap().decoded.as_ref().unwrap();
    assert_eq!(decoded.commands.len(), 3);
    assert_eq!(
        decoded.source,
        "on (release) {\n    gotoAndPlay(\"next\");\n}\n"
    );
}
