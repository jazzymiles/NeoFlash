//! Original synthetic archives only. No private document bytes or strings.
use neoflash_import::{
    BinaryContentsPrefix, BinaryStreamRole, ContentsReference, inspect, read_binary_contents,
};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Archive {
    dir: PathBuf,
}
impl Archive {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-contents-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self { dir }
    }
    fn path(&self, bytes: &[u8], names: &[&str]) -> PathBuf {
        let mut archive = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        archive
            .create_stream("Contents")
            .unwrap()
            .write_all(bytes)
            .unwrap();
        for name in names {
            archive
                .create_stream(name)
                .unwrap()
                .write_all(&[1])
                .unwrap();
        }
        let path = self.dir.join("synthetic.fla");
        fs::write(&path, archive.into_inner().into_inner()).unwrap();
        path
    }
    fn read(&self, bytes: &[u8], names: &[&str]) -> BinaryContentsPrefix {
        read_binary_contents(&self.path(bytes, names))
            .unwrap()
            .contents
    }
}
impl Drop for Archive {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn u16b(out: &mut Vec<u8>, n: u16) {
    out.extend(n.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn string(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend(s.bytes());
}
fn compact(out: &mut Vec<u8>, s: &str, unicode: bool) {
    out.push(s.len() as u8);
    if unicode {
        for c in s.bytes() {
            u16b(out, u16::from(c));
        }
    } else {
        out.extend(s.bytes());
    }
}
fn class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xffff);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend(name.bytes());
}
fn header(v: u8) -> Vec<u8> {
    let mut out = vec![v];
    if v >= 0x20 {
        u32b(&mut out, 1);
    }
    u32b(&mut out, 0);
    out.push(0);
    if v >= 0x15 {
        out.push(0);
    }
    for gate in [0x21, 0x2b, 0x3b, 0x40, 0x44] {
        if v >= gate {
            u32b(&mut out, 0);
        }
    }
    out
}
fn linkage(out: &mut Vec<u8>, v: u8) {
    out.push(v);
    u32b(out, 0);
    string(out, "id");
    string(out, "");
    if v >= 5 {
        string(out, "");
    }
    if v >= 1 {
        out.push(0);
    }
    if v >= 2 {
        u32b(out, 1);
        string(out, "");
        string(out, "");
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
    }
    if v >= 6 {
        out.push(0);
    }
    if v >= 7 {
        string(out, "");
    }
}
fn metadata(out: &mut Vec<u8>, v: u32) {
    u32b(out, v);
    u32b(out, 0);
    if v > 1 {
        u32b(out, 0);
    }
    if v > 2 {
        u32b(out, 0);
    }
    if v >= 5 {
        linkage(out, 2);
    }
    if v >= 6 {
        out.push(0);
    }
}
fn properties(out: &mut Vec<u8>) {
    out.push(1);
    u32b(out, 0);
}
fn movie(out: &mut Vec<u8>, v: u8) {
    out.push(v);
    if v >= 1 {
        string(out, "clip");
        u32b(out, 3);
        out.extend([1, 2, 3]);
    }
    if v >= 2 {
        u32b(out, 0);
    }
}
fn component(out: &mut Vec<u8>, v: u8) {
    out.push(v);
    u32b(out, 0);
    string(out, "");
    out.extend([0; 12]);
    string(out, "");
    for _ in 0..3 {
        movie(out, 2);
    }
    if v >= 1 {
        string(out, "");
        properties(out);
    }
    if v >= 2 {
        string(out, "");
    }
    if v >= 4 {
        u32b(out, u32::MAX);
    }
    if v >= 5 {
        u32b(out, 0);
    }
    if v >= 6 {
        u32b(out, 0);
    }
    if v >= 7 {
        u32b(out, 0);
        string(out, "");
        out.extend([0; 8]);
    }
    if v >= 8 {
        movie(out, 3);
    }
}
fn page(out: &mut Vec<u8>, v: u8, name: &str) {
    page_fields(out, v, name, None, None);
}
fn page_fields(out: &mut Vec<u8>, v: u8, name: &str, meta: Option<&[u8]>, props: Option<&[u8]>) {
    out.push(v);
    compact(out, name, v >= 21);
    string(out, "private synthetic descriptor");
    if v >= 2 {
        u32b(out, 7);
    }
    if v >= 3 {
        out.push(0);
    }
    if v >= 6 {
        string(out, "");
    }
    if v >= 7 {
        u32b(out, 0);
    }
    if v >= 5 {
        if let Some(meta) = meta {
            out.extend(meta);
        } else {
            metadata(out, if v >= 18 { 6 } else { 3 });
        }
    }
    if v >= 8 {
        u32b(out, 0);
    }
    if v >= 10 {
        string(out, "");
        string(out, "");
        if let Some(props) = props {
            out.extend(props);
        } else {
            properties(out);
        }
    }
    if v >= 11 {
        u32b(out, 0);
    }
    if v >= 12 {
        linkage(out, if v >= 18 { 7 } else { 2 });
    }
    if (14..=17).contains(&v) {
        string(out, "");
    }
    if (15..=17).contains(&v) {
        movie(out, 2);
        movie(out, 0);
    }
    if (16..=17).contains(&v) {
        u32b(out, 0);
    }
    if v == 17 {
        u32b(out, 0);
        string(out, "");
        u32b(out, 0);
    }
    if v >= 18 {
        component(out, 8);
    }
    if v >= 20 {
        string(out, "");
        string(out, "");
    }
    if v >= 22 {
        u32b(out, 0);
        string(out, "");
    }
    if v >= 23 {
        out.extend([0; 20]);
    }
    if v >= 24 {
        out.extend([0; 9]);
    }
    if v >= 25 {
        out.extend([0; 2]);
    }
}
fn media(out: &mut Vec<u8>, base: u8, sub: u8, sound: bool) {
    out.push(base);
    compact(out, "Media 1", base >= 6);
    string(out, "private media name");
    u16b(out, 1);
    if base >= 2 {
        string(out, "");
    }
    if base >= 3 {
        u32b(out, 0);
    }
    if base >= 4 {
        metadata(out, 3);
    }
    if base >= 5 {
        u32b(out, 0);
    }
    out.push(sub);
    if sound {
        if sub == 2 {
            string(out, "");
        }
        out.extend([0; 6]);
        if sub >= 4 {
            out.extend([0; 4]);
        }
        if sub >= 6 {
            out.extend([0; 5]);
        }
        if sub >= 7 {
            out.extend([0; 6]);
        }
        if sub >= 9 {
            string(out, "");
        }
        if sub >= 10 {
            out.extend([0; 4]);
        }
    } else {
        if sub < 2 {
            string(out, "");
        }
        if sub >= 1 {
            out.extend([0; 2]);
        }
        if sub >= 2 {
            out.push(0);
        }
        if sub >= 3 {
            out.extend([0; 4]);
        }
        if sub >= 4 {
            out.push(0);
        }
    }
}
fn settings(out: &mut Vec<u8>) {
    u16b(out, 2);
    out.push(0);
    for word in [-10i32, 11001, 20, 8021] {
        out.extend(word.to_le_bytes());
    }
    u32b(out, 20 * 65536);
    out.extend([0; 5]);
    out.extend([0; 16]);
    out.extend([0; 4]);
    u32b(out, 65536);
    u16b(out, 3);
    out.extend([8, 9, 10]);
    u16b(out, 2);
    out.extend([11, 12]);
    u32b(out, 0x80402010);
    u32b(out, 0xff010203);
    u32b(out, 29 * 65536 + 32768);
    out.extend([0; 3]);
    u32b(out, 20);
}
fn fixture(v: u8, pv: u8, with_media: bool) -> (Vec<u8>, usize) {
    let mut out = header(v);
    class(&mut out, "CDocumentPage");
    page(&mut out, pv, "Page 1");
    u16b(&mut out, 0);
    u16b(&mut out, 2);
    u16b(&mut out, 3);
    u16b(&mut out, 2);
    u16b(&mut out, 0x8001);
    page(&mut out, pv, "Symbol 1");
    u16b(&mut out, 0);
    u16b(&mut out, 4);
    if with_media {
        class(&mut out, "CMediaBits");
        media(&mut out, if v >= 0x3f { 6 } else { 3 }, 2, false);
    }
    u16b(&mut out, 0);
    u16b(&mut out, 5);
    settings(&mut out);
    let end = out.len();
    out.extend([91, 17, 231, 33]);
    (out, end)
}

#[test]
fn four_revision_prefixes_preserve_identity_raw_settings_and_exact_suffix() {
    let archive = Archive::new();
    for (v, pv) in [(0x14, 4), (0x20, 12), (0x29, 17), (0x3f, 23)] {
        let (bytes, end) = fixture(v, pv, true);
        let path = archive.path(&bytes, &["Page 1", "Symbol 1", "Media 1"]);
        let document = read_binary_contents(&path).unwrap();
        let prefix = document.contents;
        assert_eq!(
            prefix.stop.code, "fla_contents_suffix_unsupported",
            "{:?}",
            prefix.stop
        );
        assert_eq!(prefix.bytes_consumed, end);
        assert_eq!(prefix.original_bytes, bytes);
        assert_eq!(prefix.scenes, vec![2]);
        assert_eq!(prefix.selected_scene, Some(2));
        assert_eq!(prefix.library, vec![3]);
        assert_eq!(prefix.media, vec![5]);
        assert!(matches!(
            prefix.references[1],
            ContentsReference::Class { .. }
        ));
        assert!(matches!(
            prefix.references[3],
            ContentsReference::Object {
                descriptor_index: Some(1)
            }
        ));
        assert_eq!(prefix.descriptors.len(), 3);
        assert!(prefix.descriptors.iter().all(|d| d.stream_index.is_some()));
        let settings = prefix.settings.unwrap();
        assert_eq!(settings.size_twips(), Some([11011, 8001]));
        assert_eq!(settings.frame_rate(), 29.5);
        assert_eq!(settings.background_rgba_word, 0x80402010);
        let report = inspect(&path).unwrap();
        assert!(!report.can_import);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private synthetic"));
        assert!(!json.contains("private media"));
        assert!(!json.contains("bounds_words"));
        assert!(!json.contains("original_bytes"));
        let stream = report
            .binary_fla
            .unwrap()
            .streams
            .into_iter()
            .find(|s| s.role == BinaryStreamRole::Contents)
            .unwrap();
        assert!(!stream.structure_complete);
        let counts = stream.contents.unwrap();
        assert!(counts.settings_reached);
        assert_eq!(
            (
                counts.scene_entries,
                counts.library_entries,
                counts.media_entries
            ),
            (1, 1, 1)
        );
    }
}
#[test]
fn every_truncated_prefix_stops_without_settings_or_resynchronization() {
    let archive = Archive::new();
    for (v, pv) in [(0x14, 4), (0x20, 12), (0x29, 17), (0x3f, 23)] {
        let (bytes, end) = fixture(v, pv, true);
        for n in 0..end {
            let prefix = archive.read(&bytes[..n], &["Page 1", "Symbol 1", "Media 1"]);
            assert_eq!(
                prefix.stop.code, "fla_truncated_record",
                "v={v:x} n={n} {:?}",
                prefix.stop
            );
            assert!(prefix.settings.is_none());
            assert!(prefix.bytes_consumed <= n);
            assert_eq!(prefix.original_bytes, &bytes[..n]);
        }
    }
}
#[test]
fn references_require_identity_type_and_scene_membership() {
    let archive = Archive::new();
    let mut bytes = header(0x14);
    class(&mut bytes, "CDocumentPage");
    page(&mut bytes, 4, "Page 1");
    u16b(&mut bytes, 0);
    u16b(&mut bytes, 0);
    u16b(&mut bytes, 0);
    let pointer = bytes.len();
    u16b(&mut bytes, 2);
    for tag in [1u16, 999, 0x8002] {
        let mut invalid = bytes.clone();
        invalid[pointer..pointer + 2].copy_from_slice(&tag.to_le_bytes());
        let p = archive.read(&invalid, &["Page 1"]);
        assert_eq!(p.stop.code, "fla_contents_invalid_record");
        assert_eq!(p.bytes_consumed, pointer + 2);
    }
    // Extended object identity resolves to the same scene; no copied descriptor.
    bytes.truncate(pointer);
    u16b(&mut bytes, 0x7fff);
    u32b(&mut bytes, 2);
    u16b(&mut bytes, 0);
    u16b(&mut bytes, 0);
    u16b(&mut bytes, 0);
    u16b(&mut bytes, 0);
    settings(&mut bytes);
    let p = archive.read(&bytes, &["Page 1"]);
    assert!(p.settings.is_some());
    assert_eq!(p.descriptors.len(), 1);
    assert_eq!(p.selected_scene, Some(2));
}
#[test]
fn missing_wrong_role_and_duplicate_stream_targets_are_blocking() {
    let archive = Archive::new();
    let (bytes, _) = fixture(0x14, 4, false);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_stream_reference");
    assert!(p.settings.is_none());
    for target in ["Page 1", "Contents", "Symbol 1"] {
        let mut bytes = header(0x14);
        class(&mut bytes, "CDocumentPage");
        page(&mut bytes, 4, "Page 1");
        u16b(&mut bytes, 0x8001);
        page(&mut bytes, 4, target);
        let p = archive.read(&bytes, &["Page 1", "Symbol 1"]);
        assert_eq!(p.stop.code, "fla_contents_stream_reference");
        assert_eq!(p.scenes.len(), 1);
        assert!(p.settings.is_none());
    }
}
#[test]
fn unknown_records_and_budgets_stop_at_the_first_unproven_boundary() {
    let archive = Archive::new();
    let mut bytes = header(0x14);
    class(&mut bytes, "PrivateUnknownDescriptor");
    let end = bytes.len();
    bytes.extend([0; 100]);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_record_unsupported");
    assert_eq!(p.bytes_consumed, end);
    assert_eq!(p.descriptors.len(), 0);
    assert!(matches!(
        p.references.last().unwrap(),
        ContentsReference::Object {
            descriptor_index: None
        }
    ));
    let mut bytes = header(0x14);
    bytes[1..5].copy_from_slice(&50_001u32.to_le_bytes());
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_budget");
    let mut bytes = header(0x14);
    class(&mut bytes, "CDocumentPage");
    bytes.push(4);
    compact(&mut bytes, "Page 1", false);
    bytes.extend([255, 255, 255]);
    u32b(&mut bytes, 1024 * 1024 + 1);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_budget");
}
#[test]
fn bounds_are_preserved_without_sorting_sentinel_or_integer_overflow() {
    let archive = Archive::new();
    let (bytes, _) = fixture(0x14, 4, false);
    let mut p = archive.read(&bytes, &["Page 1", "Symbol 1"]);
    let settings = p.settings.as_mut().unwrap();
    for words in [[i32::MIN, 100, 0, 200], [30, 10, 0, 200], [0, 0, 0, 200]] {
        settings.bounds_words = words;
        assert_eq!(settings.size_twips(), None);
    }
    settings.bounds_words = [i32::MIN + 1, i32::MAX, -10, i32::MAX];
    assert_eq!(settings.size_twips(), Some([4294967294, 2147483657]));
}

fn one_scene(v: u8, body: &[u8]) -> (Vec<u8>, usize) {
    let mut out = header(v);
    class(&mut out, "CDocumentPage");
    out.extend(body);
    u16b(&mut out, 0);
    u16b(&mut out, 0);
    u16b(&mut out, 0);
    u16b(&mut out, 2);
    u16b(&mut out, 0);
    u16b(&mut out, 0);
    u16b(&mut out, 0);
    u16b(&mut out, 0);
    settings(&mut out);
    let end = out.len();
    out.extend([199, 200]);
    (out, end)
}
fn rich_metadata() -> Vec<u8> {
    let mut out = Vec::new();
    u32b(&mut out, 7);
    // Two real identity words, plus attributes and full linkage.
    for n in [1, 0x10203040, 0x50607080, 1, 0x90a0b0c0, 0xd0e0f000, 7] {
        u32b(&mut out, n);
    }
    linkage(&mut out, 7);
    out.push(1);
    u32b(&mut out, 0);
    u32b(&mut out, 6);
    for kind in [0, 1, 3, 4, 5, 7] {
        out.push(kind);
        string(&mut out, "field");
        match kind {
            0 => u32b(&mut out, 42),
            1 => {
                u32b(&mut out, 2);
                u32b(&mut out, 42);
                u32b(&mut out, 43);
            }
            3 => out.extend(1.5_f64.to_le_bytes()),
            4 => {
                u32b(&mut out, 1);
                out.extend(2.5_f64.to_le_bytes());
            }
            5 => string(&mut out, "value"),
            7 => {
                u32b(&mut out, 3);
                out.extend([0, 128, 255]);
            }
            _ => unreachable!(),
        }
    }
    out
}
fn rich_properties() -> Vec<u8> {
    let mut out = vec![2];
    u32b(&mut out, 1);
    out.push(3);
    string(&mut out, "table");
    out.push(8);
    u16b(&mut out, 13);
    u32b(&mut out, 1);
    u32b(&mut out, 0);
    u32b(&mut out, 1);
    string(&mut out, "named");
    string(&mut out, "entry");
    u16b(&mut out, 7);
    string(&mut out, "collection");
    string(&mut out, "item");
    string(&mut out, "first");
    u32b(&mut out, 2);
    for _ in 0..2 {
        string(&mut out, "column");
        u16b(&mut out, 5);
        string(&mut out, "default");
        string(&mut out, "");
    }
    u32b(&mut out, 1);
    string(&mut out, "cell1");
    string(&mut out, "cell2");
    u32b(&mut out, 3);
    out.extend([1, 2, 3]);
    string(&mut out, "variable");
    out.push(0);
    out.extend([7; 16]);
    string(&mut out, "category");
    u32b(&mut out, 0);
    u32b(&mut out, 1);
    out
}
#[test]
fn nonempty_nested_metadata_and_property_tables_are_bounded_and_retained() {
    let archive = Archive::new();
    let meta = rich_metadata();
    let props = rich_properties();
    let mut body = Vec::new();
    page_fields(&mut body, 23, "Page 1", Some(&meta), Some(&props));
    let (bytes, end) = one_scene(0x3f, &body);
    let p = archive.read(&bytes, &["Page 1"]);
    assert!(p.settings.is_some(), "{:?}", p.stop);
    assert_eq!(p.bytes_consumed, end);
    let d = &p.descriptors[0];
    assert_eq!(
        &p.original_bytes[d.encoded.end - body.len()..d.encoded.end],
        body
    );
    for n in 0..end {
        let p = archive.read(&bytes[..n], &["Page 1"]);
        assert_eq!(p.stop.code, "fla_truncated_record", "n={n} {:?}", p.stop);
        assert!(p.settings.is_none());
    }
    let mut huge = vec![2];
    u32b(&mut huge, 50_001);
    let mut body = Vec::new();
    page_fields(&mut body, 12, "Page 1", None, Some(&huge));
    let (bytes, _) = one_scene(0x20, &body);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_budget");
    let mut body = Vec::new();
    page_fields(&mut body, 12, "Page 1", None, Some(&[3]));
    let (bytes, _) = one_scene(0x20, &body);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_record_unsupported");
    assert!(p.descriptors.is_empty());
}
#[test]
fn media_revision_gates_include_sound_and_do_not_read_payload_streams() {
    let archive = Archive::new();
    for sound in [false, true] {
        for base in [0, 3, 5, 6, 7] {
            for sub in 0..=if sound { 10 } else { 4 } {
                let mut out = header(0x3f);
                class(&mut out, "CDocumentPage");
                page(&mut out, 23, "Page 1");
                u16b(&mut out, 0);
                u16b(&mut out, 0);
                u16b(&mut out, 0);
                u16b(&mut out, 2);
                u16b(&mut out, 0);
                u16b(&mut out, 0);
                class(&mut out, if sound { "CMediaSound" } else { "CMediaBits" });
                media(&mut out, base, sub, sound);
                u16b(&mut out, 0);
                u16b(&mut out, 1);
                settings(&mut out);
                let end = out.len();
                out.push(255);
                let p = archive.read(&out, &["Page 1", "Media 1"]);
                assert!(
                    p.settings.is_some(),
                    "base={base} sub={sub} sound={sound} {:?}",
                    p.stop
                );
                assert_eq!(p.bytes_consumed, end);
                assert_eq!(p.descriptors[1].subtype_revision, Some(sub));
            }
        }
    }
}
#[test]
fn cstring_encoding_compact_names_and_counted_ole_have_distinct_boundaries() {
    let archive = Archive::new();
    // Unicode CString including a surrogate pair; no guessed ANSI code page.
    let mut body = vec![4];
    compact(&mut body, "Page 1\0", false);
    body.extend([255, 254, 255, 3]);
    for word in [0x41, 0xd83d, 0xde80] {
        u16b(&mut body, word);
    }
    u32b(&mut body, 1);
    body.push(0);
    let (bytes, _) = one_scene(0x14, &body);
    let p = archive.read(&bytes, &["Page 1"]);
    assert!(p.settings.is_some());
    assert!(p.descriptors[0].name.utf16);
    assert_eq!(p.descriptors[0].stream_name, "Page 1");
    let mut invalid = body.clone();
    let unicode = 1 + 1 + 7;
    invalid[unicode + 4..unicode + 6].copy_from_slice(&0xdc00_u16.to_le_bytes());
    let (bytes, _) = one_scene(0x14, &invalid);
    let p = archive.read(&bytes, &["Page 1"]);
    assert_eq!(p.stop.code, "fla_contents_invalid_record");
    // Repeated Unicode marker is not a second encoding header.
    let mut invalid = vec![4];
    compact(&mut invalid, "Page 1", false);
    invalid.extend([255, 254, 255, 255, 254, 255]);
    let (bytes, _) = one_scene(0x14, &invalid);
    assert_eq!(
        archive.read(&bytes, &["Page 1"]).stop.code,
        "fla_contents_invalid_record"
    );
    let mut sentinel = vec![4];
    compact(&mut sentinel, "Page 1", false);
    sentinel.extend([255, 255, 255]);
    u32b(&mut sentinel, u32::MAX);
    u32b(&mut sentinel, 1);
    sentinel.push(0);
    let (bytes, _) = one_scene(0x14, &sentinel);
    let p = archive.read(&bytes, &["Page 1"]);
    assert!(p.settings.is_some());
    assert_eq!(
        p.descriptors[0].name.data.start,
        p.descriptors[0].name.data.end
    );
    // Two null pointers consume exactly two entries before reserved/header fields.
    let (mut bytes, end) = fixture(0x14, 4, false);
    bytes[1..5].copy_from_slice(&2u32.to_le_bytes());
    bytes.splice(5..5, [0; 4]);
    let p = archive.read(&bytes, &["Page 1", "Symbol 1"]);
    assert!(p.settings.is_some());
    assert_eq!(p.bytes_consumed, end + 4);
    assert_eq!(p.ole_count, Some(2));
    bytes[5] = 1;
    let p = archive.read(&bytes, &["Page 1", "Symbol 1"]);
    assert_eq!(p.stop.code, "fla_contents_record_unsupported");
    assert_eq!(p.bytes_consumed, 7);
}
#[test]
fn shared_work_budget_stops_before_allocating_more_identity_records() {
    let archive = Archive::new();
    let (mut bytes, _) = fixture(0x14, 4, false);
    bytes[1..5].copy_from_slice(&50_000u32.to_le_bytes());
    bytes.splice(5..5, std::iter::repeat_n(0, 100_000));
    let p = archive.read(&bytes, &["Page 1", "Symbol 1"]);
    assert_eq!(p.stop.code, "fla_contents_budget");
    assert_eq!(p.references.len(), 1);
    assert!(p.descriptors.is_empty());
}

#[test]
fn document_page_revision_gates_and_unknown_schema_stop_exactly() {
    let archive = Archive::new();
    for revision in 0..=25 {
        let mut body = Vec::new();
        page(&mut body, revision, "Page 1");
        let (bytes, end) = one_scene(0x3f, &body);
        let prefix = archive.read(&bytes, &["Page 1"]);
        if revision == 9 {
            assert_eq!(prefix.stop.code, "fla_contents_record_unsupported");
            assert!(prefix.settings.is_none());
            assert!(prefix.descriptors.is_empty());
        } else {
            assert!(
                prefix.settings.is_some(),
                "revision={revision}: {:?}",
                prefix.stop
            );
            assert_eq!(prefix.bytes_consumed, end);
        }
    }
    let mut bytes = header(0x14);
    let start = bytes.len();
    class(&mut bytes, "CDocumentPage");
    bytes[start + 2..start + 4].copy_from_slice(&2_u16.to_le_bytes());
    let end = bytes.len();
    bytes.extend([0; 100]);
    let prefix = archive.read(&bytes, &["Page 1"]);
    assert_eq!(prefix.stop.code, "fla_contents_record_unsupported");
    assert_eq!(prefix.bytes_consumed, end);
}
