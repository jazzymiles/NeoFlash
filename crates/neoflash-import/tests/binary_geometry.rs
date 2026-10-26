//! Original synthetic shape bytes. No corpus geometry, names or art are fixtures.
use neoflash_import::{
    BinaryFillEncoding, BinaryFillPayload, BinaryStreamRole, decode_binary_shape, inspect,
    read_binary_geometry,
};
use std::{
    fs,
    io::{Cursor, Write},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct Bytes(Vec<u8>);
impl Bytes {
    fn b(&mut self, x: u8) {
        self.0.push(x);
    }
    fn w(&mut self, x: u16) {
        self.0.extend(x.to_le_bytes());
    }
    fn d(&mut self, x: u32) {
        self.0.extend(x.to_le_bytes());
    }
    fn ints(&mut self, values: &[i32]) {
        for &v in values {
            self.d(v as u32);
        }
    }
    fn shorts(&mut self, values: &[i16]) {
        for &v in values {
            self.w(v as u16);
        }
    }
    fn matrix(&mut self) {
        self.ints(&[65536, 0, -123, 65536, 20, -40]);
    }
    fn header(shape: u8, data: u8) -> Self {
        let mut b = Self::default();
        b.b(shape);
        b.matrix();
        b.b(data);
        b.d(9876);
        b
    }
    fn paint(&mut self, flags: u8) {
        self.d(0xa17f3b29);
        self.b(flags);
        self.b(0x85);
    }
    fn finish(&mut self, data: u8) {
        if data >= 2 {
            self.b(0);
        }
        if data >= 5 {
            self.d(0);
        }
    }
}

fn geometry() -> Vec<u8> {
    let mut b = Bytes::header(4, 5);
    b.w(2);
    b.paint(0);
    b.paint(0);
    b.w(1);
    b.d(0xff5070a0);
    b.w(40);
    b.w(0);
    b.w(0x8000);
    b.0.extend([1, 3, 2, 1]);
    b.w(256);
    b.paint(0);
    b.b(0x65); // full style words, small start/control, wide end delta
    b.w(0x8001);
    b.w(0x8002);
    b.w(1);
    b.shorts(&[-128, 256]);
    b.shorts(&[128, -256]);
    b.ints(&[1024, 512]);
    b.b(0x30);
    b.shorts(&[8, -2]);
    b.b(1); // inherited styles, coarse straight edge
    b.b(0xc0);
    b.0.extend([0, 0, 1]);
    b.b(0); // byte styles, zero-length straight edge
    b.finish(5);
    b.0
}

#[test]
fn quadratic_deltas_selection_and_inherited_contour_styles_are_preserved() {
    let bytes = geometry();
    let shape = decode_binary_shape(&bytes).unwrap();
    assert_eq!(shape.original_bytes, bytes);
    assert_eq!(shape.metadata, 9876); // hint is deliberately unlike edge count
    assert_eq!(shape.edges.len(), 3);
    let q = &shape.edges[0];
    assert_eq!(q.start, [-128, 256]);
    assert_eq!(q.control, Some([0, 0]));
    assert_eq!(q.end, [896, 768]);
    assert_eq!(q.styles.stroke, 0x8001);
    assert_eq!(q.styles.fill_0, 0x8002);
    assert_eq!(q.styles.fill_1, 1);
    assert!(q.line_flags.is_none());
    let line = &shape.edges[1];
    assert_eq!(line.styles, q.styles);
    assert_eq!(line.start, q.end);
    assert_eq!(line.end, [1920, 512]);
    assert_eq!(line.line_flags, Some(1));
    assert_eq!(shape.edges[2].styles.fill_1, 1);
    assert_eq!(shape.edges[2].styles.stroke, 0);
    assert_eq!(shape.edges[2].start, shape.edges[2].end);
    assert_eq!(
        serde_json::from_str::<neoflash_import::BinaryShapeRecord>(
            &serde_json::to_string(&shape).unwrap()
        )
        .unwrap(),
        shape
    );
}

fn rich_styles() -> Vec<u8> {
    let mut b = Bytes::header(6, 5);
    b.w(3);
    b.paint(0x30);
    b.matrix();
    b.b(2);
    b.ints(&[-200]);
    b.d(0x80000003);
    b.b(0);
    b.d(0x10203040);
    b.b(255);
    b.d(0x50607080);
    b.paint(0x20);
    b.matrix();
    b.d(123);
    for v in [3, 4, 5, 6] {
        b.w(v);
    }
    b.paint(0x40);
    b.matrix();
    b.w(0xfe01);
    b.w(0);
    b.b(0);
    b.d(3); // three auxiliary curves, independent optional boundary points
    for flags in [1, 2, 0x84] {
        b.ints(&[1, -2, 3, -4, 5, -6, 7, -8]);
        b.b(1);
        b.ints(&[91, 92]);
        b.b(0x81);
        b.b(0xab);
        b.b(flags);
        if flags & 1 != 0 {
            b.ints(&[11, 12]);
        }
        if flags & 2 != 0 {
            b.ints(&[21, 22]);
        }
    }
    b.0
}

#[test]
fn gradient_precedence_bitmap_width_and_unresolved_payloads_keep_exact_boundaries() {
    let bytes = rich_styles();
    let shape = decode_binary_shape(&bytes).unwrap();
    let BinaryFillEncoding::Extended {
        flags,
        secondary_flags,
        payload,
    } = &shape.fills[0].encoding
    else {
        panic!()
    };
    assert_eq!((*flags, *secondary_flags), (0x30, 0x85));
    let BinaryFillPayload::Gradient {
        extension, stops, ..
    } = payload
    else {
        panic!()
    };
    assert_eq!(extension.as_ref().unwrap().focal_point, -200);
    assert_eq!(extension.as_ref().unwrap().flags, 0x80000003);
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[1].ratio, 255);
    assert!(matches!(
        shape.fills[1].encoding,
        BinaryFillEncoding::Extended {
            payload: BinaryFillPayload::Unresolved {
                value: 123,
                words: [3, 4, 5, 6],
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        shape.fills[2].encoding,
        BinaryFillEncoding::Extended {
            payload: BinaryFillPayload::Bitmap {
                media_id: 0xfe01,
                ..
            },
            ..
        }
    ));
    assert_eq!(shape.original_bytes, bytes);
}

#[test]
fn auxiliary_boundary_bits_are_independent_and_unknown_bits_are_retained() {
    let shape = decode_binary_shape(&rich_styles()).unwrap();
    assert_eq!(shape.auxiliary[0].boundary_start, Some([11, 12]));
    assert_eq!(shape.auxiliary[0].boundary_end, None);
    assert_eq!(shape.auxiliary[1].boundary_start, None);
    assert_eq!(shape.auxiliary[1].boundary_end, Some([21, 22]));
    assert_eq!(shape.auxiliary[2].boundary_flags, Some(0x84));
    assert_eq!(shape.auxiliary[2].boundary_start, None);
    let point = &shape.auxiliary[2].entries.as_ref().unwrap()[0];
    assert_eq!(point.point, [91, 92]);
    assert_eq!((point.control_flag, point.value), (0x81, 0xab));
}

#[test]
fn all_supported_revisions_have_distinct_empty_record_end_boundaries() {
    for shape_revision in 0..=6 {
        for data_revision in 0..=5 {
            let mut b = Bytes::header(shape_revision, data_revision);
            b.w(1);
            if data_revision < 3 {
                b.d(0x12345678);
                b.w(0xffff);
            } else {
                b.paint(0);
            }
            b.w(0);
            b.finish(data_revision);
            let decoded = decode_binary_shape(&b.0).unwrap();
            assert_eq!(decoded.original_bytes, b.0);
            assert_eq!(decoded.shape_revision, shape_revision);
            assert_eq!(decoded.data_revision, data_revision);
            if data_revision < 3 {
                assert!(matches!(
                    decoded.fills[0].encoding,
                    BinaryFillEncoding::Legacy { word: 0xffff }
                ));
            }
        }
    }
}

#[test]
fn every_truncated_prefix_and_trailing_byte_is_rejected_without_recovery() {
    for bytes in [geometry(), rich_styles()] {
        for end in 0..bytes.len() {
            let error = decode_binary_shape(&bytes[..end]).unwrap_err();
            assert!(error.offset <= end, "{end}: {error:?}");
            assert_eq!(error.code, "fla_truncated_record", "prefix {end}");
        }
        for suffix in [0, 0xff, 0x47] {
            let mut trailing = bytes.clone();
            trailing.push(suffix);
            let error = decode_binary_shape(&trailing).unwrap_err();
            assert_eq!(error.code, "fla_shape_trailing_data");
            assert_eq!(error.offset, bytes.len());
        }
    }
}

#[test]
fn missing_style_indices_stop_before_untrusted_coordinate_bytes() {
    for raw_styles in [[1, 0, 0], [0, 0x8001, 0], [0, 0, 0x4000]] {
        let mut b = Bytes::header(4, 5);
        b.w(0);
        b.w(0);
        b.b(0x6a);
        for value in raw_styles {
            b.w(value);
        }
        let boundary = b.0.len();
        b.0.extend([0xff; 80]);
        let error = decode_binary_shape(&b.0).unwrap_err();
        assert_eq!(error.code, "fla_shape_style_index");
        assert_eq!(error.offset, boundary);
    }
    // A selected 'no style' reference is still index zero, not index 32768.
    let mut b = Bytes::header(2, 2);
    b.w(0);
    b.w(0);
    b.b(0x40);
    for _ in 0..3 {
        b.w(0x8000);
    }
    b.b(0);
    assert!(decode_binary_shape(&b.0).is_ok());
}

#[test]
fn archive_coordinate_addition_uses_defined_wrapping_not_debug_overflow() {
    let mut b = Bytes::header(2, 2);
    b.w(0);
    b.w(0);
    b.b(0x22);
    b.ints(&[i32::MAX, i32::MIN, 1, -1]);
    b.b(0x21);
    b.shorts(&[1, -1]);
    b.ints(&[0, 0]);
    b.b(0);
    let shape = decode_binary_shape(&b.0).unwrap();
    assert_eq!(shape.edges[0].end, [i32::MIN, i32::MAX]);
    assert_eq!(shape.edges[1].start, [i32::MIN + 1, i32::MAX - 1]);
}

#[test]
fn allocation_and_revision_limits_fail_before_large_allocations() {
    let error = decode_binary_shape(&vec![0; 8 * 1024 * 1024 + 1]).unwrap_err();
    assert_eq!(error.code, "fla_shape_limit");
    let mut count = Bytes::header(6, 5);
    count.w(0);
    count.w(0);
    count.b(0);
    count.d(100001);
    assert_eq!(
        decode_binary_shape(&count.0).unwrap_err().code,
        "fla_shape_limit"
    );
    let mut gradient = Bytes::header(6, 5);
    gradient.w(1);
    gradient.paint(0x10);
    gradient.matrix();
    gradient.b(16);
    assert_eq!(
        decode_binary_shape(&gradient.0).unwrap_err().code,
        "fla_gradient_stop_limit"
    );
    assert_eq!(
        decode_binary_shape(&[7]).unwrap_err().code,
        "fla_record_revision_unsupported"
    );
    let unknown = Bytes::header(6, 6);
    assert_eq!(
        decode_binary_shape(&unknown.0).unwrap_err().code,
        "fla_record_revision_unsupported"
    );
    let mut edges = Bytes::header(2, 2);
    edges.w(0);
    edges.w(0);
    edges.0.resize(edges.0.len() + 100001, 0x80);
    edges.b(0);
    assert_eq!(
        decode_binary_shape(&edges.0).unwrap_err().code,
        "fla_shape_limit"
    );
}

#[test]
fn cfb_inspection_is_aggregate_only_but_explicit_preservation_returns_geometry() {
    let shape = geometry();
    let mut stream = Bytes(vec![0]); // shape root
    stream.w(0xffff);
    stream.w(1);
    stream.w(9);
    stream.0.extend(b"CPicShape");
    stream.0.extend([1, 0, 0, 0]);
    stream.ints(&[i32::MIN, i32::MIN]);
    let shape_offset = stream.0.len();
    stream.0.extend(&shape);
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb.create_stream("Contents")
        .unwrap()
        .write_all(&[0x3f])
        .unwrap();
    cfb.create_stream("Symbol 1")
        .unwrap()
        .write_all(&stream.0)
        .unwrap();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "neoflash-shape-{}-{}.fla",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, cfb.into_inner().into_inner()).unwrap();
    let report = inspect(&path).unwrap();
    assert!(!report.can_import);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("original_bytes"));
    assert!(!json.contains("start_delta"));
    let binary = report.binary_fla.unwrap();
    let s = binary
        .streams
        .iter()
        .find(|s| s.role == BinaryStreamRole::Symbol)
        .unwrap();
    assert!(s.structure_complete);
    assert_eq!(s.bytes_consumed, stream.0.len());
    assert_eq!(s.geometry.quadratic_edges, 1);
    assert_eq!(s.geometry.straight_edges, 2);
    assert_eq!(s.geometry.complete_shape_blocks, 1);
    let document = read_binary_geometry(&path).unwrap();
    fs::remove_file(path).unwrap();
    let retained = &document.streams[0].shapes[0];
    assert_eq!(retained.offset, shape_offset);
    assert_eq!(retained.record.original_bytes, shape);
}

#[test]
fn document_retention_budget_is_shared_across_streams_and_stops_explicitly() {
    let mut shape = Bytes::header(2, 2);
    shape.w(0);
    shape.w(0);
    shape.0.resize(shape.0.len() + 100_000, 0x80);
    shape.b(0);
    let mut stream = Bytes(vec![0]);
    stream.w(0xffff);
    stream.w(1);
    stream.w(9);
    stream.0.extend(b"CPicShape");
    stream.0.extend([1, 0, 0, 0]);
    stream.ints(&[i32::MIN, i32::MIN]);
    let shape_offset = stream.0.len();
    stream.0.extend(shape.0);
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb.create_stream("Contents")
        .unwrap()
        .write_all(&[0x20])
        .unwrap();
    for index in 1..=3 {
        cfb.create_stream(format!("Symbol {index}"))
            .unwrap()
            .write_all(&stream.0)
            .unwrap();
    }
    let path = std::env::temp_dir().join(format!(
        "neoflash-geometry-budget-{}.fla",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(&cfb.into_inner().into_inner()).unwrap();
    drop(file);
    let document = read_binary_geometry(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(document.streams.len(), 2);
    assert_eq!(
        document
            .streams
            .iter()
            .map(|s| s.shapes.len())
            .sum::<usize>(),
        2
    );
    let stopped = document
        .inspection
        .streams
        .iter()
        .find(|s| s.stop_code.as_deref() == Some("fla_geometry_document_limit"))
        .unwrap();
    assert!(!stopped.structure_complete);
    assert_eq!(stopped.bytes_consumed, shape_offset);
    assert_eq!(stopped.geometry.complete_shape_blocks, 0);
}
