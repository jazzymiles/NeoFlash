//! All fixtures are synthetic and created here. No legacy source/art is embedded.
use neoflash_import::{BinaryStreamRole, ImportFormat, import_project, inspect};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "neoflash-cfb-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("synthetic.bin");
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cfb(version: cfb::Version, contents: &[u8], page: &[u8]) -> Vec<u8> {
    let mut compound =
        cfb::CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
    compound
        .create_stream("Contents")
        .unwrap()
        .write_all(contents)
        .unwrap();
    compound
        .create_stream("Page 1")
        .unwrap()
        .write_all(page)
        .unwrap();
    compound.into_inner().into_inner()
}
fn class(out: &mut Vec<u8>, name: &str) {
    out.extend_from_slice(&[255, 255, 1, 0]);
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}
fn point(out: &mut Vec<u8>) {
    out.extend_from_slice(&i32::MIN.to_le_bytes());
    out.extend_from_slice(&i32::MIN.to_le_bytes());
}
fn empty_shape(out: &mut Vec<u8>) {
    out.extend_from_slice(&[1, 0, 0, 0]);
    point(out); // CPicObj, no children
    out.push(2); // CPicShape revision, identity matrix
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.push(5);
    out.extend_from_slice(&[0; 9]); // shape metadata, zero fills/strokes, edge terminator
    out.extend_from_slice(&[0; 4]); // zero auxiliary curves
}
fn frame(out: &mut Vec<u8>, duration: u16) {
    empty_shape(out);
    out.push(0);
    out.extend_from_slice(&duration.to_le_bytes());
    out.push(0);
}
fn timeline() -> Vec<u8> {
    let mut p = vec![1];
    class(&mut p, "CPicPage");
    p.extend_from_slice(&[1, 0]);
    class(&mut p, "CPicLayer");
    p.extend_from_slice(&[1, 0]);
    class(&mut p, "CPicFrame");
    frame(&mut p, 1);
    p.extend_from_slice(&0x8005_u16.to_le_bytes());
    frame(&mut p, 3); // combined table class ref
    p.extend_from_slice(&[0, 0]);
    point(&mut p); // end layer children
    p.push(8);
    p.extend_from_slice(&[4, b'T', b'e', b's', b't']);
    p.extend_from_slice(&[0; 3]);
    p.extend_from_slice(&[0; 16]);
    p.push(0);
    p.extend_from_slice(&[0; 5]); // two null related-layer refs and legacy mask
    p.extend_from_slice(&[0, 0]);
    point(&mut p); // end root children
    p.push(3);
    p.extend_from_slice(&1_u16.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    p
}

#[test]
fn real_cfb_and_mfc_records_are_decoded_without_claiming_editable_import() {
    let temp = Temp::new();
    for version in [cfb::Version::V3, cfb::Version::V4] {
        let path = temp.file(&cfb(version, &[0x3f], &timeline()));
        let report = inspect(&path).unwrap();
        assert_eq!(report.format, ImportFormat::LegacyBinaryFla); // extension-independent
        assert_eq!(report.version.as_deref(), Some("Flash 8"));
        assert!(!report.can_import);
        assert!(import_project(&path).is_err());
        assert_eq!(report.statistics.elements, 0);
        let binary = report.binary_fla.unwrap();
        assert_eq!(
            binary.cfb_major_version,
            if version == cfb::Version::V3 { 3 } else { 4 }
        );
        let page = binary
            .streams
            .iter()
            .find(|s| s.role == BinaryStreamRole::Scene)
            .unwrap();
        assert!(page.structure_complete, "{:?}", page);
        assert_eq!(page.bytes_consumed as u64, page.byte_length);
        assert_eq!(
            (page.records.pages, page.records.layers, page.records.frames),
            (1, 1, 2)
        );
        assert_eq!(page.records.encountered, 4);
        assert_eq!(page.records.complete, 4);
    }
}

#[test]
fn save_target_mapping_is_exact_and_unknown_revisions_remain_unknown() {
    let temp = Temp::new();
    for (revision, version) in [
        (0x10, "Flash 1"),
        (0x11, "Flash 2"),
        (0x14, "Flash 3"),
        (0x1b, "Flash 4"),
        (0x20, "Flash 5"),
        (0x29, "Flash MX"),
        (0x38, "Flash MX 2004"),
        (0x3f, "Flash 8"),
        (0x43, "Flash CS3"),
        (0x47, "Flash CS4"),
    ] {
        let p = temp.file(&cfb(cfb::Version::V3, &[revision], &timeline()));
        assert_eq!(inspect(&p).unwrap().version.as_deref(), Some(version));
    }
    let p = temp.file(&cfb(cfb::Version::V3, &[0x48], &timeline()));
    let report = inspect(&p).unwrap();
    assert!(report.version.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_revision_unsupported")
    );
}

#[test]
fn every_truncated_mfc_prefix_and_trailing_payload_is_rejected() {
    let temp = Temp::new();
    let page = timeline();
    for end in 0..page.len() {
        let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &page[..end]));
        let report = inspect(&p).unwrap();
        let binary = report.binary_fla.unwrap();
        let stream = binary
            .streams
            .iter()
            .find(|s| s.role == BinaryStreamRole::Scene)
            .unwrap();
        assert!(!stream.structure_complete, "prefix {end}");
        assert!(stream.bytes_consumed <= end);
        assert!(stream.stop_code.is_some());
    }
    let mut trailing = page;
    trailing.push(0);
    let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &trailing));
    assert!(
        inspect(&p)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_trailing_data")
    );
}

#[test]
fn invalid_references_unknown_classes_and_schemas_stop_at_exact_boundaries() {
    let temp = Temp::new();
    for (data, code) in [
        (vec![1, 1, 128], "fla_mfc_reference"),
        (vec![1, 255, 127, 1, 0, 0, 128], "fla_mfc_reference"),
    ] {
        let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &data));
        assert!(
            inspect(&p)
                .unwrap()
                .diagnostics
                .iter()
                .any(|d| d.code == code)
        );
    }
    let mut unknown = vec![1];
    class(&mut unknown, "PrivateUnrecognizedClass");
    unknown.extend_from_slice(&[17; 100]);
    let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &unknown));
    let report = inspect(&p).unwrap();
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("PrivateUnrecognizedClass"));
    let page = report
        .binary_fla
        .unwrap()
        .streams
        .into_iter()
        .find(|s| s.role == BinaryStreamRole::Scene)
        .unwrap();
    assert_eq!(page.bytes_consumed, 7 + "PrivateUnrecognizedClass".len());
    assert!(!page.structure_complete);
    let mut schema = timeline();
    schema[3] = 2;
    let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &schema));
    assert!(
        inspect(&p)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_mfc_schema_unsupported")
    );
}

#[test]
fn file_and_stream_budgets_and_fat_corruption_are_rejected() {
    let temp = Temp::new();
    let mut bytes = cfb(cfb::Version::V3, &[0x20], &timeline());
    bytes[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(inspect(&temp.file(&bytes)).is_err());
    let bytes = cfb(cfb::Version::V3, &[0x20], &vec![0; 8 * 1024 * 1024 + 1]);
    assert!(inspect(&temp.file(&bytes)).is_err());
    let path = temp.0.join("oversize.fla");
    fs::File::create(&path)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 512)
        .unwrap();
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])
        .unwrap();
    assert!(inspect(&path).is_err());
}

#[test]
fn ordinary_compound_documents_are_not_fla_even_with_fla_extension() {
    let temp = Temp::new();
    let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    compound
        .create_stream("Contents")
        .unwrap()
        .write_all(&[0x3f])
        .unwrap();
    let path = temp.0.join("ordinary.fla");
    fs::write(&path, compound.into_inner().into_inner()).unwrap();
    let report = inspect(&path).unwrap();
    assert_eq!(report.format, ImportFormat::Unknown);
    assert!(report.binary_fla.is_none());
}

#[test]
fn extra_allocated_fat_sectors_are_legal_and_directory_cycles_are_rejected() {
    let temp = Temp::new();
    let mut bytes = cfb(cfb::Version::V3, &[0x20], &timeline());
    let new_sector = bytes.len() / 512 - 1;
    let fat = u32::from_le_bytes(bytes[76..80].try_into().unwrap()) as usize;
    let original_fat_count = u32::from_le_bytes(bytes[44..48].try_into().unwrap());
    assert_eq!(original_fat_count, 1);
    assert!(new_sector < 128);
    bytes[44..48].copy_from_slice(&2_u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&(new_sector as u32).to_le_bytes());
    let marker = (fat + 1) * 512 + new_sector * 4;
    bytes[marker..marker + 4].copy_from_slice(&0xffff_fffd_u32.to_le_bytes());
    bytes.extend_from_slice(&[255; 512]);
    assert_eq!(
        inspect(&temp.file(&bytes)).unwrap().format,
        ImportFormat::LegacyBinaryFla
    );
    let directory = u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize;
    let child = (directory + 1) * 512 + 76;
    bytes[child..child + 4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(inspect(&temp.file(&bytes)).is_err());
}

#[test]
fn regular_sector_streams_are_read_and_mfc_depth_is_bounded() {
    let temp = Temp::new();
    let mut contents = vec![0; 8192];
    contents[0] = 0x20;
    let p = temp.file(&cfb(cfb::Version::V3, &contents, &timeline()));
    let report = inspect(&p).unwrap();
    let stream = report
        .binary_fla
        .unwrap()
        .streams
        .into_iter()
        .find(|s| s.role == BinaryStreamRole::Contents)
        .unwrap();
    assert_eq!(stream.byte_length, 8192);
    assert_eq!(stream.bytes_consumed, 1);
    assert!(!stream.structure_complete);
    let mut nested = vec![1];
    for _ in 0..35 {
        class(&mut nested, "CPicPage");
        nested.extend_from_slice(&[1, 0]);
    }
    let p = temp.file(&cfb(cfb::Version::V3, &[0x20], &nested));
    assert!(
        inspect(&p)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_depth_limit")
    );
}
