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
            self.word(0);
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
        self.byte(8);
        self.string(name);
        self.bytes.extend([1, 2, 3]);
        self.dword(22);
        self.dword(0xff112233);
        self.dword(0);
        self.dword(1);
        self.byte(0);
        self.word(related);
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
                TimelineClass::Shape | TimelineClass::Frame
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
    w.begin("CPicText");
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
fn cyclic_or_shared_child_is_an_explicit_rejected_reference_not_new_ownership() {
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
        let action_start = f.actions.as_ref().unwrap().encoded.start;
        let mut nonempty = w.bytes.clone();
        nonempty[action_start..action_start + 4].copy_from_slice(&1u32.to_le_bytes());
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
        assert_eq!(
            f.actions.as_ref().unwrap().presence,
            TimelineActionPresence::Historical
        );
        assert!(!f.actions.as_ref().unwrap().complete);
        assert!(!frame.complete);
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
        let payload: Vec<_> = [0u32, 0, 0, count]
            .into_iter()
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
        assert_eq!(actions.complete, count == 0);
        assert_eq!(frame.complete, count == 0);
        assert_eq!(actions.encoded.end, span.start + 16);
        if count != 0 {
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
    for _ in 0..12_600 {
        w.begin("CPicShape");
        w.base_start();
        w.base_end();
        w.shape();
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
    assert!(d.streams[1].objects.len() < d.streams[0].objects.len());
    let ordinary = inspect(&path).unwrap().binary_fla.unwrap();
    assert!(
        ordinary
            .streams
            .iter()
            .filter(|s| s.role == BinaryStreamRole::Scene)
            .all(|s| s.structure_complete)
    );
}
