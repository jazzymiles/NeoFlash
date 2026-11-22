//! Integration tests for [T-450]: Binary FLA (Flash 3–CS4) rare record decoding
//! (`ConvolutionFilter`, `CPicFrame` rev 17, `CDocumentPage` rev 9, `[MS-OLEPS]`
//! summary streams), exact model conversions (authored cubic Bézier recovery,
//! bitmap registration origins, multi-segment custom ease, classic tween spans,
//! zero-frame layers), and oversized-document vector/bitmap handling.
//!
//! 100% synthetic binary FLA fixtures; no private corpus bytes or filenames.

use image::ExtendedColorType;
use neoflash_core::*;
use neoflash_import::*;
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t450-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn f32b(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn f64b(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    for ch in s.encode_utf16() {
        u16b(out, ch);
    }
}

fn push_mfc_new_class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xffff);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());
}

fn push_contents_header(out: &mut Vec<u8>, rev: u8) {
    out.push(rev);
    if rev >= 0x20 {
        u32b(out, 1);
    }
    u32b(out, 0);
    out.push(0);
    if rev >= 0x15 {
        out.push(0);
    }
    for gate in [0x21, 0x2b, 0x3b, 0x40, 0x44] {
        if rev >= gate {
            u32b(out, 0);
        }
    }
}

fn push_contents_settings_v3f(out: &mut Vec<u8>, media_counter: u16) {
    u16b(out, media_counter);
    u16b(out, 0); // ruler_unit
    out.push(0); // grid_display_mode
    for word in [0_i32, 11000, 0, 8000] {
        i32b(out, word);
    }
    u32b(out, 20 * 65536);
    out.extend_from_slice(&[0; 5]);
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(&[0; 4]);
    i32b(out, 65536);
    u16b(out, 0);
    u16b(out, 0);
    u32b(out, 0xffff_ffff);
    u32b(out, 0);
    u32b(out, 24 * 65536);
    out.extend_from_slice(&[0; 3]);
    i32b(out, 20);
}

/// Build a valid Flash 8 (`0x3f`) `Contents` stream with 1 scene (`Page 1`),
/// optional library symbols `(lib_id, stream_name, name, symbol_type)` (`1` = MovieClip, `2` = Graphic),
/// and optional bitmap media descriptors `(media_id, stream_name, name, encoded_jpeg_len)`.
fn build_contents_v3f(
    symbols: &[(u32, &str, &str, u8)],
    bitmaps: &[(u16, &str, &str, i32)],
) -> Vec<u8> {
    let mut out = Vec::new();
    push_contents_header(&mut out, 0x3f);

    // Scene list: null-terminated MFC pointer list
    push_mfc_new_class(&mut out, "CDocumentPage"); // ref 1 (class), ref 2 (Scene 1)
    let mut next_ref = 3_u16;
    out.push(5); // CDocumentPage rev 5
    mfc_str(&mut out, "Page 1");
    mfc_str(&mut out, "Scene 1");
    u32b(&mut out, 0);
    out.push(2);
    u32b(&mut out, 3);
    u32b(&mut out, 0);
    u32b(&mut out, 0);
    u32b(&mut out, 0);
    u16b(&mut out, 0); // end scenes

    u16b(&mut out, 1); // next_scene_id
    u16b(&mut out, 1); // next_item_tag
    u16b(&mut out, 2); // selected_scene = ref 2

    // Library list: null-terminated
    for &(lib_id, stream_name, sym_name, sym_type) in symbols {
        u16b(&mut out, 0x8001); // CDocumentPage class ref 1
        next_ref += 1;
        out.push(5);
        mfc_str(&mut out, stream_name);
        mfc_str(&mut out, sym_name);
        u32b(&mut out, lib_id);
        out.push(sym_type);
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
    }
    u16b(&mut out, 0); // end library
    u16b(&mut out, 100); // library_counter

    // Media list: null-terminated
    let mut bmp_cls: Option<u16> = None;
    for &(media_id, stream_name, bmp_name, enc_len) in bitmaps {
        if let Some(cid) = bmp_cls {
            u16b(&mut out, 0x8000 | cid);
        } else {
            push_mfc_new_class(&mut out, "CMediaBits");
            bmp_cls = Some(next_ref);
            next_ref += 1;
        }
        next_ref += 1;
        out.push(6); // descriptor rev 6 (UTF-16 stream_name)
        compact_utf16(&mut out, stream_name);
        mfc_str(&mut out, bmp_name);
        u16b(&mut out, media_id);
        mfc_str(&mut out, "");
        u32b(&mut out, 0);
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        // CMediaBits subtype rev 3
        out.push(3);
        out.push(if enc_len > 0 { 0 } else { 1 });
        out.push(80);
        out.push(1);
        i32b(&mut out, enc_len);
    }
    u16b(&mut out, 0); // end media

    push_contents_settings_v3f(&mut out, (bitmaps.len() + 1) as u16);
    out
}

struct StreamWriter {
    bytes: Vec<u8>,
    classes: std::collections::BTreeMap<&'static str, u16>,
    next_id: u16,
}
impl StreamWriter {
    fn new() -> Self {
        Self {
            bytes: vec![1],
            classes: std::collections::BTreeMap::new(),
            next_id: 1,
        }
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(&cid) = self.classes.get(class) {
            u16b(&mut self.bytes, 0x8000 | cid);
        } else {
            push_mfc_new_class(&mut self.bytes, class);
            self.classes.insert(class, self.next_id);
            self.next_id += 1;
        }
        let oid = self.next_id;
        self.next_id += 1;
        oid
    }
    fn base_start(&mut self) {
        self.bytes.push(1);
        self.bytes.push(3);
    }
    fn base_end(&mut self) {
        u16b(&mut self.bytes, 0);
        i32b(&mut self.bytes, i32::MIN);
        i32b(&mut self.bytes, i32::MIN);
    }
    fn empty_shape(&mut self) {
        self.bytes.push(2);
        for w in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, w);
        }
        self.bytes.push(5);
        self.bytes.extend_from_slice(&[0; 9]);
        u32b(&mut self.bytes, 0);
    }
    fn simple_rect_shape(&mut self, size_px: i32, rgba: u32) {
        self.bytes.push(2); // shape_revision = 2
        for w in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, w);
        }
        self.bytes.push(2); // data_revision = 2
        u32b(&mut self.bytes, 0); // metadata
        u16b(&mut self.bytes, 1); // 1 fill
        u32b(&mut self.bytes, rgba);
        self.bytes.push(0);
        self.bytes.push(0);
        u16b(&mut self.bytes, 0); // 0 strokes
        let s = size_px * 5120;
        let corners = [[0, 0], [s, 0], [s, s], [0, s], [0, 0]];
        for (ei, w) in corners.windows(2).enumerate() {
            let [x0, y0] = w[0];
            let [x1, y1] = w[1];
            // geometry.rs: 0x40 = has_styles, 0x80 = u8 style indices,
            // start_mode = flags & 3 (0 => [0,0]), ctrl_mode = (flags>>2)&3 (0 => straight),
            // end_mode = (flags>>4)&3 (2 => i32 pair => 2 << 4 = 0x20)
            if ei == 0 {
                self.bytes.push(0x80 | 0x40 | 0x20);
                self.bytes.extend_from_slice(&[0, 0, 1]); // stroke=0, fill_0=0, fill_1=1
            } else {
                self.bytes.push(0x20);
            }
            i32b(&mut self.bytes, x1 - x0);
            i32b(&mut self.bytes, y1 - y0);
        }
        self.bytes.push(0); // terminator
    }
    fn layer_tail_rev2(&mut self, name: &str) {
        self.bytes.push(2);
        mfc_str(&mut self.bytes, name);
        self.bytes.extend_from_slice(&[1, 0, 0]);
        u16b(&mut self.bytes, 0);
    }
    fn page_tail(&mut self) {
        self.bytes.push(1);
        u16b(&mut self.bytes, 1);
    }
}

fn write_cfb_fla(path: &std::path::Path, streams: &[(&str, &[u8])]) {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for &(name, data) in streams {
        cfb.create_stream(name).unwrap().write_all(data).unwrap();
    }
    fs::write(path, cfb.into_inner().into_inner()).unwrap();
}

fn build_symbol_stream_with_rect(size_px: i32, rgba: u32) -> Vec<u8> {
    let mut w = StreamWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    w.base_end();
    w.simple_rect_shape(size_px, rgba);
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);
    w.bytes.push(0);
    w.base_end();
    w.layer_tail_rev2("Art");
    w.base_end();
    w.page_tail();
    w.bytes
}

#[test]
fn convolution_filter_decodes_and_converts_enabled_disabled_and_clamped_variants() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[(10, "Symbol 1", "ClipSym", 1)], &[]);
    let sym_bytes = build_symbol_stream_with_rect(40, 0xff22_88ee);

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();

    // CPicSymbol rev 16 (filters + blend_mode)
    page.begin("CPicSymbol");
    page.base_start();
    page.base_end();
    page.bytes.push(16);
    for m in [65536_i32, 0, 0, 65536, 400, 600] {
        i32b(&mut page.bytes, m);
    }
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    page.bytes.push(1); // color_transform rev 1
    for _ in 0..4 {
        u16b(&mut page.bytes, 256);
        u16b(&mut page.bytes, 0);
    }
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, ""); // legacy_name (rev >= 6)
    u32b(&mut page.bytes, 10); // library_id = 10
    page.bytes.extend_from_slice(&[0, 0, 0]); // timelineEffect=0, persistentData=0, isExtensible=0

    // filters: presence=1, count=3
    page.bytes.push(1);
    u32b(&mut page.bytes, 3);

    // Filter 0: enabled 3x3 sharpen (type 5, rev 1)
    page.bytes.push(5);
    page.bytes.push(1);
    page.bytes.push(1); // state_revision = 1
    u32b(&mut page.bytes, 1); // enabled = true
    page.bytes.push(3); // matrix_x = 3
    page.bytes.push(3); // matrix_y = 3
    f32b(&mut page.bytes, 1.0); // divisor
    f32b(&mut page.bytes, 4.0); // bias
    for coeff in [0.0_f32, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0] {
        f32b(&mut page.bytes, coeff);
    }
    u32b(&mut page.bytes, 0x8040_2010);
    page.bytes.push(0x03); // clamp | preserve_alpha

    // Filter 1: disabled 3x1 with divisor == 0.0 -> sum([1,2,1]) = 4.0
    page.bytes.push(5);
    page.bytes.push(1);
    page.bytes.push(1);
    u32b(&mut page.bytes, 0); // enabled = false
    page.bytes.push(3);
    page.bytes.push(1);
    f32b(&mut page.bytes, 0.0);
    f32b(&mut page.bytes, 0.0);
    for coeff in [1.0_f32, 2.0, 1.0] {
        f32b(&mut page.bytes, coeff);
    }
    u32b(&mut page.bytes, 0xff00_0000);
    page.bytes.push(0x02); // clamp=true, preserve_alpha=false

    // Filter 2: out-of-range coefficient 100_000.0 -> clamped to 65_536.0 with fla_filter_approximated
    page.bytes.push(5);
    page.bytes.push(1);
    page.bytes.push(1);
    u32b(&mut page.bytes, 1);
    page.bytes.push(1);
    page.bytes.push(1);
    f32b(&mut page.bytes, 1.0);
    f32b(&mut page.bytes, 0.0);
    f32b(&mut page.bytes, 100_000.0);
    u32b(&mut page.bytes, 0);
    page.bytes.push(0x01); // clamp=false, preserve_alpha=true

    page.bytes.push(1); // blend_mode = Normal

    page.base_end();
    page.empty_shape();
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    page.base_end();
    page.layer_tail_rev2("Main");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("convolution.fla");
    write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Symbol 1", &sym_bytes),
        ],
    );

    let report = inspect(&fla_path).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_filter_approximated"),
        "out-of-range 100_000.0 coefficient must emit fla_filter_approximated: {:?}",
        report.diagnostics
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    let root = project.symbol(&project.root).unwrap();
    let inst = &root.timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(inst.filters.len(), 3);

    assert!(inst.filters[0].is_enabled());
    match &inst.filters[0] {
        FilterSpec::Convolution {
            matrix_x,
            matrix_y,
            matrix,
            divisor,
            bias,
            preserve_alpha,
            clamp,
            color,
            alpha,
        } => {
            assert_eq!((*matrix_x, *matrix_y), (3, 3));
            assert_eq!(matrix, &[0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0]);
            assert_eq!((*divisor, *bias), (1.0, 4.0));
            assert!(*clamp && *preserve_alpha);
            assert_eq!(*color, 0x102040);
            assert!((*alpha - 128.0 / 255.0).abs() < 1e-6);
        }
        other => panic!("expected Convolution filter 0, got {other:?}"),
    }

    assert!(!inst.filters[1].is_enabled());
    if let FilterSpec::Convolution {
        divisor,
        clamp,
        preserve_alpha,
        ..
    } = &inst.filters[1]
    {
        assert_eq!(*divisor, 4.0);
        assert!(*clamp && !*preserve_alpha);
    } else {
        panic!("expected Convolution filter 1");
    }

    if let FilterSpec::Convolution { matrix, .. } = &inst.filters[2] {
        assert_eq!(matrix, &[65_536.0]);
    } else {
        panic!("expected Convolution filter 2");
    }
}

#[test]
fn document_page_rev9_historical_action_lists_and_frame_rev17_pose_decode_cleanly() {
    let tmp = TempDir::new();

    // Contents v=0x3f with CDocumentPage revision 9 carrying 4 historical action lists:
    // list 0 has 1 historical stop command (rev 4), lists 1..3 are empty.
    let mut contents = Vec::new();
    push_contents_header(&mut contents, 0x3f);
    push_mfc_new_class(&mut contents, "CDocumentPage"); // ref 1 (class), ref 2 (Scene 1)
    contents.push(9); // CDocumentPage rev 9!
    mfc_str(&mut contents, "Page 1");
    mfc_str(&mut contents, "Scene 1");
    u32b(&mut contents, 0); // library_id
    contents.push(2); // symbol_type
    mfc_str(&mut contents, "");
    u32b(&mut contents, 0);
    // library_metadata rev 3
    u32b(&mut contents, 3);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0); // rev >= 8
    // rev == 9: 4 historical action lists!
    u32b(&mut contents, 1); // list 0: 1 historical command (rev 4)
    contents.extend_from_slice(&[4, 2, 0, 0, 0, 1, 0, 0x80, 0xc1, 0, 0, 0, 0, 1, 0, 0, 0]);
    contents.extend_from_slice(&[0; 28]);
    for _ in 0..3 {
        u32b(&mut contents, 0);
    }
    u16b(&mut contents, 0); // end scenes
    u16b(&mut contents, 1); // next_scene_id
    u16b(&mut contents, 1); // next_item_tag
    u16b(&mut contents, 2); // selected_scene = ref 2
    u16b(&mut contents, 0); // end library
    u16b(&mut contents, 1); // library_counter
    u16b(&mut contents, 0); // end media
    push_contents_settings_v3f(&mut contents, 1);

    // Page 1 with CPicFrame rev 17 having non-identity pose_extension
    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    page.base_end();
    page.empty_shape();
    page.bytes.push(17); // CPicFrame rev 17!
    u16b(&mut page.bytes, 2);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "Rev17");
    u32b(&mut page.bytes, 0); // empty historical actions
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // morph = null
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // sound_settings = null
    mfc_str(&mut page.bytes, "");
    // rev 17 pose extension!
    u16b(&mut page.bytes, 1);
    for w in [65536_i32, 0, 0, 65536, 200, 100] {
        i32b(&mut page.bytes, w);
    }
    page.bytes.push(1);
    page.bytes.push(1);
    for _ in 0..4 {
        u16b(&mut page.bytes, 256);
        u16b(&mut page.bytes, 0);
    }
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    page.bytes.push(0);

    page.base_end();
    page.layer_tail_rev2("Layer 1");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("rev9_rev17.fla");
    write_cfb_fla(
        &fla_path,
        &[("Contents", &contents), ("Page 1", &page.bytes)],
    );

    let cdoc = read_binary_contents(&fla_path).unwrap();
    assert!(cdoc.contents.settings.is_some());

    let rep = inspect(&fla_path).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_layer_animation"),
        "non-identity rev-17 pose extension must emit fla_layer_animation: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    assert_eq!(
        proj.symbol(&proj.root).unwrap().timeline.layers[0].keyframes[0].label,
        "Rev17"
    );
}

#[test]
fn ole_property_sets_validate_without_fla_stream_retained_while_unknown_streams_warn() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[], &[]);
    let page = build_symbol_stream_with_rect(20, 0xff00_ff00);

    let mut sum_info = Vec::new();
    u16b(&mut sum_info, 0xfffe);
    u16b(&mut sum_info, 0);
    u32b(&mut sum_info, 0x0002_0005);
    sum_info.extend_from_slice(&[0; 16]);
    u32b(&mut sum_info, 1);
    sum_info.extend_from_slice(&[
        0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3,
        0xd9,
    ]);
    u32b(&mut sum_info, 48);
    u32b(&mut sum_info, 48);
    u32b(&mut sum_info, 2);
    u32b(&mut sum_info, 1);
    u32b(&mut sum_info, 24);
    u32b(&mut sum_info, 2);
    u32b(&mut sum_info, 32);
    u32b(&mut sum_info, 0x0002);
    u16b(&mut sum_info, 1252);
    u16b(&mut sum_info, 0);
    u32b(&mut sum_info, 0x001e);
    u32b(&mut sum_info, 8);
    sum_info.extend_from_slice(b"NeoDemo\0");

    let mut doc_sum = Vec::new();
    u16b(&mut doc_sum, 0xfffe);
    u16b(&mut doc_sum, 0);
    u32b(&mut doc_sum, 0x0002_0005);
    doc_sum.extend_from_slice(&[0; 16]);
    u32b(&mut doc_sum, 2);
    doc_sum.extend_from_slice(&[
        0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9,
        0xae,
    ]);
    u32b(&mut doc_sum, 68);
    doc_sum.extend_from_slice(&[
        0x05, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9,
        0xae,
    ]);
    u32b(&mut doc_sum, 92);
    u32b(&mut doc_sum, 24);
    u32b(&mut doc_sum, 1);
    u32b(&mut doc_sum, 1);
    u32b(&mut doc_sum, 16);
    u32b(&mut doc_sum, 0x0002);
    u16b(&mut doc_sum, 1252);
    u16b(&mut doc_sum, 0);
    u32b(&mut doc_sum, 48);
    u32b(&mut doc_sum, 2);
    u32b(&mut doc_sum, 0);
    u32b(&mut doc_sum, 24);
    u32b(&mut doc_sum, 1);
    u32b(&mut doc_sum, 40);
    u32b(&mut doc_sum, 1);
    u32b(&mut doc_sum, 2);
    u32b(&mut doc_sum, 4);
    doc_sum.extend_from_slice(b"Key\0");
    u32b(&mut doc_sum, 0x0002);
    u16b(&mut doc_sum, 1252);
    u16b(&mut doc_sum, 0);

    let clean_path = tmp.0.join("ole_clean.fla");
    write_cfb_fla(
        &clean_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("\u{0005}SummaryInformation", &sum_info),
            ("\u{0005}DocumentSummaryInformation", &doc_sum),
        ],
    );
    let rep_clean = inspect(&clean_path).unwrap();
    assert!(
        rep_clean
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_stream_retained"),
        "valid MS-OLEPS streams must not emit fla_stream_retained: {:?}",
        rep_clean.diagnostics
    );
    let proj_clean = import_project(&clean_path).unwrap();
    proj_clean.validate().unwrap();

    let unknown_path = tmp.0.join("ole_unknown.fla");
    write_cfb_fla(
        &unknown_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("MysteryVendorStream", &[1, 2, 3, 4, 5]),
        ],
    );
    let rep_unknown = inspect(&unknown_path).unwrap();
    assert!(
        rep_unknown
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_stream_retained"),
        "unrecognized auxiliary stream must still emit fla_stream_retained"
    );
}

#[test]
fn complete_zero_frame_layer_imports_cleanly_while_incomplete_layer_warns() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[], &[]);

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.base_end();
    page.layer_tail_rev2("EmptyCompleteLayer");
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    page.base_end();
    page.simple_rect_shape(20, 0xffaa_3311);
    page.bytes.push(1);
    u16b(&mut page.bytes, 4);
    page.bytes.push(0);
    page.base_end();
    page.layer_tail_rev2("ContentLayer");
    page.base_end();
    page.page_tail();

    let ok_path = tmp.0.join("zero_frame_layer.fla");
    write_cfb_fla(
        &ok_path,
        &[("Contents", &contents), ("Page 1", &page.bytes)],
    );
    let rep = inspect(&ok_path).unwrap();
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_layer_skipped"),
        "complete 0-frame CPicLayer must not emit fla_layer_skipped: {:?}",
        rep.diagnostics
    );
    let proj = import_project(&ok_path).unwrap();
    proj.validate().unwrap();
    let root = proj.symbol(&proj.root).unwrap();
    assert_eq!(root.timeline.layers.len(), 2);
    assert_eq!(root.timeline.layers[0].name, "EmptyCompleteLayer");
    assert!(root.timeline.layers[0].keyframes.is_empty());
    assert_eq!(root.timeline.layers[0].properties.end_frame, Some(0));
    assert_eq!(root.timeline.frame_count, 4);

    // Incomplete CPicLayer (truncated before layer settings) still emits fla_layer_skipped:
    let mut broken_page = StreamWriter::new();
    broken_page.begin("CPicPage");
    broken_page.base_start();
    broken_page.begin("CPicLayer");
    broken_page.base_start();
    broken_page.begin("CPicFrame");
    broken_page.base_start();
    broken_page.base_end();
    broken_page.simple_rect_shape(20, 0xffaa_3311);
    broken_page.bytes.push(1);
    u16b(&mut broken_page.bytes, 2);
    broken_page.bytes.push(0);
    broken_page.base_end();
    broken_page.layer_tail_rev2("GoodLayer");
    // Second layer has an unparsable frame child and stops before completion:
    broken_page.begin("CPicLayer");
    broken_page.base_start();
    broken_page.begin("CPicFrame");
    broken_page.base_start();
    // Truncated right here!
    let broken_path = tmp.0.join("incomplete_layer.fla");
    write_cfb_fla(
        &broken_path,
        &[("Contents", &contents), ("Page 1", &broken_page.bytes)],
    );
    let rep_broken = inspect(&broken_path).unwrap();
    assert!(
        rep_broken
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_layer_skipped"),
        "incomplete layer with no readable frame must still emit fla_layer_skipped: {:?}",
        rep_broken.diagnostics
    );
}

#[test]
fn panoramic_jpeg_downscales_without_skip_and_bitmap_origin_translates_exact_bounds() {
    let tmp = TempDir::new();

    // Media 1: 5000 x 200 panoramic JPEG (> 4096 px wide, 1M pixels < 16.7M cap)
    let rgb_wide = vec![120_u8; 5000 * 200 * 3];
    let mut jpg_wide = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg_wide, 80)
        .encode(&rgb_wide, 5000, 200, ExtendedColorType::Rgb8)
        .unwrap();
    let enc_len_1 = jpg_wide.len() as i32;
    for b in [0_i32, 5000 * 20, 0, 200 * 20] {
        i32b(&mut jpg_wide, b);
    }

    // Media 2: 40 x 20 JPEG with non-zero registration bounds [-200, 600, -400, 0] twips = [-10..30] x [-20..0] px
    let rgb_off = vec![200_u8; 40 * 20 * 3];
    let mut jpg_off = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg_off, 85)
        .encode(&rgb_off, 40, 20, ExtendedColorType::Rgb8)
        .unwrap();
    let enc_len_2 = jpg_off.len() as i32;
    for b in [-200_i32, 600, -400, 0] {
        i32b(&mut jpg_off, b);
    }

    let contents = build_contents_v3f(
        &[],
        &[
            (1, "Media 1", "Panorama", enc_len_1),
            (2, "Media 2", "OffsetOrigin", enc_len_2),
        ],
    );

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();

    page.begin("CPicBitmap");
    page.base_start();
    page.base_end();
    page.bytes.push(1);
    for m in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page.bytes, m);
    }
    u16b(&mut page.bytes, 1);

    page.begin("CPicBitmap");
    page.base_start();
    page.base_end();
    page.bytes.push(1);
    for m in [131_072_i32, 0, 0, 131_072, 2000, 4000] {
        i32b(&mut page.bytes, m);
    }
    u16b(&mut page.bytes, 2);

    page.base_end();
    page.empty_shape();
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    page.base_end();
    page.layer_tail_rev2("Bitmaps");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("panoramic_and_origin.fla");
    write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Media 1", &jpg_wide),
            ("Media 2", &jpg_off),
        ],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_bitmap_skipped" && d.code != "fla_bitmap_origin"),
        "expected zero fla_bitmap_skipped and zero fla_bitmap_origin: {:?}",
        rep.diagnostics
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_bitmap_downscaled" && d.location == "media 1")
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    let elems = &proj.symbol(&proj.root).unwrap().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elems.len(), 2);

    match &elems[0].kind {
        ElementKind::Bitmap { width, height, .. } => {
            assert_eq!(*width, 4096);
            assert_eq!(*height, 164);
            assert!((f64::from(*width) * elems[0].transform.scale_x - 5000.0).abs() < 1.0);
        }
        other => panic!("expected Bitmap 0, got {other:?}"),
    }

    let ElementKind::Bitmap { resource_id, .. } = &elems[1].kind else {
        panic!("expected offset JPEG bitmap");
    };
    let resource = proj
        .resources
        .iter()
        .find(|r| &r.id == resource_id)
        .unwrap();
    assert_eq!(resource.media_type, "image/jpeg");
    assert_eq!(
        resource.decoded_bytes().unwrap().as_slice(),
        &jpg_off[..enc_len_2 as usize]
    );
    assert!((elems[1].transform.x - 80.0).abs() < 1e-4);
    assert!((elems[1].transform.y - 160.0).abs() < 1e-4);
    assert!((elems[1].transform.scale_x - 2.0).abs() < 1e-4);
    assert!((elems[1].transform.scale_y - 2.0).abs() < 1e-4);
}

#[test]
fn multi_segment_custom_ease_splits_exact_cubic_bezier_spans_and_1frame_boundary_clears() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[(10, "Symbol 1", "Mover", 1)], &[]);
    let sym_bytes = build_symbol_stream_with_rect(20, 0xff11_66cc);

    let knots: [[f64; 6]; 3] = [
        [0.0, 0.0, 0.0, 0.0, 0.2, 0.05],
        [0.4, 0.65, 0.55, 0.7, 0.75, 0.75],
        [0.9, 0.98, 1.0, 1.0, 1.0, 1.0],
    ];

    let push_symbol_inst = |w: &mut StreamWriter, tx_twips: i32| {
        w.begin("CPicSymbol");
        w.base_start();
        w.base_end();
        w.bytes.push(7); // CPicSymbol rev 7
        for m in [65536_i32, 0, 0, 65536, tx_twips, 1000] {
            i32b(&mut w.bytes, m);
        }
        u16b(&mut w.bytes, 0);
        u16b(&mut w.bytes, 0);
        w.bytes.push(1);
        for _ in 0..4 {
            u16b(&mut w.bytes, 256);
            u16b(&mut w.bytes, 0);
        }
        u16b(&mut w.bytes, 0);
        u16b(&mut w.bytes, 0);
        u32b(&mut w.bytes, 0);
        mfc_str(&mut w.bytes, ""); // legacy_name (rev >= 6)
        u32b(&mut w.bytes, 10); // library_id = 10
    };

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();

    // Frame 0 (duration 10, classic tween with 3-knot global custom ease): x = 0 px
    page.begin("CPicFrame");
    page.base_start();
    push_symbol_inst(&mut page, 0);
    page.base_end();
    page.empty_shape();
    page.bytes.push(24);
    u16b(&mut page.bytes, 10);
    u16b(&mut page.bytes, 1);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 2); // empty actions
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 1); // single_ease_curve = 1
    u32b(&mut page.bytes, 1); // custom_ease_present = 1
    for ch in 0..6 {
        if ch < 5 {
            u32b(&mut page.bytes, 0);
        } else {
            u32b(&mut page.bytes, 3);
            for knot in &knots {
                for &val in knot {
                    f64b(&mut page.bytes, val);
                }
            }
        }
    }

    // Frame 10 (duration 1, Flash UI left key_mode = 1 before blank Frame 11!): x = 200 px (4000 twips)
    page.begin("CPicFrame");
    page.base_start();
    push_symbol_inst(&mut page, 4000);
    page.base_end();
    page.empty_shape();
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);
    page.bytes.push(1); // key_mode = 1 (no-op 1-frame boundary flag)

    // Frame 11 (duration 3, blank keyframe)
    page.begin("CPicFrame");
    page.base_start();
    page.base_end();
    page.empty_shape();
    page.bytes.push(1);
    u16b(&mut page.bytes, 3);
    page.bytes.push(0);

    page.base_end();
    page.layer_tail_rev2("Anim");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("multi_ease.fla");
    write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Symbol 1", &sym_bytes),
        ],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_tween_easing" && d.code != "fla_tween_dropped"),
        "expected zero fla_tween_easing and zero fla_tween_dropped: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    let layer = &proj.symbol(&proj.root).unwrap().timeline.layers[0];

    let eval_seg = |p0: f64, p1: f64, p2: f64, p3: f64, s: f64| -> f64 {
        let u = 1.0 - s;
        u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
    };
    for frame in 0..=10_u32 {
        let u = f64::from(frame) / 10.0;
        let expected_v = if u <= 0.55 {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if eval_seg(0.0, 0.2, 0.4, 0.55, mid) < u {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            eval_seg(0.0, 0.05, 0.65, 0.7, 0.5 * (lo + hi))
        } else {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if eval_seg(0.55, 0.75, 0.9, 1.0, mid) < u {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            eval_seg(0.7, 0.75, 0.98, 1.0, 0.5 * (lo + hi))
        };
        let sampled = sample_layer(layer, frame);
        assert_eq!(sampled.len(), 1);
        assert!(
            (sampled[0].transform.x - 200.0 * expected_v).abs() < 1e-3,
            "frame {frame}: actual_x={} != expected_x={}",
            sampled[0].transform.x,
            200.0 * expected_v
        );
    }
    assert!(sample_layer(layer, 11).is_empty());
}

#[test]
fn authored_cubic_bezier_recovery_emits_cubic_to_and_retains_warning_when_straight_only() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[], &[]);

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    page.base_end();

    // CPicShape rev 6, data_rev 5
    page.bytes.push(6);
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page.bytes, w);
    }
    page.bytes.push(5);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1); // 1 fill
    u32b(&mut page.bytes, 0xff33_66ff);
    page.bytes.push(0);
    page.bytes.push(0);
    u16b(&mut page.bytes, 0); // 0 strokes

    // geometry.rs edge encoding:
    // flags: 0x80 (u8 style indices) | 0x40 (has styles) | 0x20 (end_mode=2) | 0x08 (ctrl_mode=2) | 0x00 (start_mode=0)
    // Note: control = start + control_delta, end = start + end_delta!
    // Edge 0: quadratic [0,0] -> ctrl [30*256, 18*256] -> end [30*256, 50*256]
    page.bytes.push(0x80 | 0x40 | 0x20 | 0x08);
    page.bytes.extend_from_slice(&[0, 0, 1]); // stroke=0, fill_0=0, fill_1=1
    i32b(&mut page.bytes, 30 * 256); // ctrl_delta.x from [0,0]
    i32b(&mut page.bytes, 18 * 256); // ctrl_delta.y from [0,0]
    i32b(&mut page.bytes, 30 * 256); // end_delta.x from [0,0]
    i32b(&mut page.bytes, 50 * 256); // end_delta.y from [0,0]

    // Edge 1: quadratic [30*256, 50*256] -> ctrl [30*256, 82*256] -> end [0, 100*256]
    page.bytes.push(0x20 | 0x08);
    i32b(&mut page.bytes, 0); // ctrl_delta.x from [30*256, 50*256]
    i32b(&mut page.bytes, 32 * 256); // ctrl_delta.y -> 82*256
    i32b(&mut page.bytes, -30 * 256); // end_delta.x -> 0
    i32b(&mut page.bytes, 50 * 256); // end_delta.y -> 100*256

    // Edge 2: straight [0, 100*256] -> [0, 0] (rev >= 4 has line_flags byte!)
    page.bytes.push(0x20);
    i32b(&mut page.bytes, 0);
    i32b(&mut page.bytes, -100 * 256);
    page.bytes.push(0); // line_flags = 0

    page.bytes.push(0); // edge terminator

    // Auxiliary curves (data_rev >= 5): count = 2
    u32b(&mut page.bytes, 2);
    // Aux 0: curved cubic [0,0, 40,0, 40,100, 0,100] twips
    for w in [0_i32, 0, 40, 0, 40, 100, 0, 100] {
        i32b(&mut page.bytes, w);
    }
    page.bytes.push(5); // 5 entries
    for (x, y, cflag) in [
        (0, 0, 0_u8),
        (30, 18, 1),
        (30, 50, 0),
        (30, 82, 1),
        (0, 100, 0),
    ] {
        i32b(&mut page.bytes, x);
        i32b(&mut page.bytes, y);
        page.bytes.push(cflag);
        page.bytes.push(0);
    }
    page.bytes.push(0); // boundary_flags = 0
    // Aux 1: retracted straight [0,100, 0,100, 0,0, 0,0] twips
    for w in [0_i32, 100, 0, 100, 0, 0, 0, 0] {
        i32b(&mut page.bytes, w);
    }
    page.bytes.push(2);
    for (x, y) in [(0, 100), (0, 0)] {
        i32b(&mut page.bytes, x);
        i32b(&mut page.bytes, y);
        page.bytes.push(0);
        page.bytes.push(0);
    }
    page.bytes.push(0);

    page.bytes.push(1);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    page.base_end();
    page.layer_tail_rev2("CubicArt");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("cubic_recovered.fla");
    write_cfb_fla(
        &fla_path,
        &[("Contents", &contents), ("Page 1", &page.bytes)],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_cubic_editing_metadata"),
        "bijectively recovered authored cubic must omit fla_cubic_editing_metadata: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    let el = &proj.symbol(&proj.root).unwrap().timeline.layers[0].keyframes[0].elements[0];
    match &el.kind {
        ElementKind::PaintedPath { commands, .. } => {
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, PathCommand::CubicTo { .. })),
                "expected native PathCommand::CubicTo in {commands:?}"
            );
            assert!(
                !commands
                    .iter()
                    .any(|c| matches!(c, PathCommand::QuadraticTo { .. })),
                "subdivided QuadraticTo edges should be replaced by CubicTo: {commands:?}"
            );
        }
        other => panic!("expected PaintedPath, got {other:?}"),
    }

    // Straight-only shape with non-retracted auxiliary cubic handles MUST keep fla_cubic_editing_metadata!
    let mut straight_page = StreamWriter::new();
    straight_page.begin("CPicPage");
    straight_page.base_start();
    straight_page.begin("CPicLayer");
    straight_page.base_start();
    straight_page.begin("CPicFrame");
    straight_page.base_start();
    straight_page.base_end();
    straight_page.bytes.push(6);
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut straight_page.bytes, w);
    }
    straight_page.bytes.push(5);
    u32b(&mut straight_page.bytes, 0);
    u16b(&mut straight_page.bytes, 1);
    u32b(&mut straight_page.bytes, 0xff11_2233);
    straight_page.bytes.push(0);
    straight_page.bytes.push(0);
    u16b(&mut straight_page.bytes, 0);
    let s = 40 * 256;
    let sq = [[0, 0], [s, 0], [s, s], [0, s], [0, 0]];
    for (ei, w) in sq.windows(2).enumerate() {
        let [x0, y0] = w[0];
        let [x1, y1] = w[1];
        if ei == 0 {
            straight_page.bytes.push(0x80 | 0x40 | 0x20);
            straight_page.bytes.extend_from_slice(&[0, 0, 1]);
        } else {
            straight_page.bytes.push(0x20);
        }
        i32b(&mut straight_page.bytes, x1 - x0);
        i32b(&mut straight_page.bytes, y1 - y0);
        straight_page.bytes.push(0); // line_flags
    }
    straight_page.bytes.push(0);
    u32b(&mut straight_page.bytes, 1); // 1 auxiliary curve on straight-only shape
    for w in [0_i32, 0, 10, 20, 30, 20, 40, 0] {
        i32b(&mut straight_page.bytes, w);
    }
    straight_page.bytes.push(0);
    straight_page.bytes.push(0);
    straight_page.bytes.push(1);
    u16b(&mut straight_page.bytes, 1);
    straight_page.bytes.push(0);
    straight_page.base_end();
    straight_page.layer_tail_rev2("StraightAux");
    straight_page.base_end();
    straight_page.page_tail();

    let straight_path = tmp.0.join("cubic_straight_only.fla");
    write_cfb_fla(
        &straight_path,
        &[("Contents", &contents), ("Page 1", &straight_page.bytes)],
    );
    let rep_straight = inspect(&straight_path).unwrap();
    assert!(
        rep_straight
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_cubic_editing_metadata"),
        "straight-only shape with auxiliary handles must retain fla_cubic_editing_metadata"
    );
}

#[test]
fn oversized_repeated_painted_paths_deduplicate_into_shared_graphic_symbols_under_element_budget() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f(&[], &[]);

    // 255 keyframes * 400 fills = 102,000 > 100,000 MAX_AUTHORED_ELEMENTS!
    let mut single_frame_shape = Vec::new();
    single_frame_shape.push(2);
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut single_frame_shape, w);
    }
    single_frame_shape.push(2);
    u32b(&mut single_frame_shape, 0);
    u16b(&mut single_frame_shape, 400);
    for i in 0..400_u32 {
        u32b(
            &mut single_frame_shape,
            0xff00_0000 | ((i * 613) & 0x00ff_ffff),
        );
        single_frame_shape.push(0);
        single_frame_shape.push(0);
    }
    u16b(&mut single_frame_shape, 0);
    let mut prev = [0_i32, 0];
    for i in 0..400_i32 {
        let ox = (i % 20) * 5120 * 4;
        let oy = (i / 20) * 5120 * 4;
        let s = 5120 * 2;
        let corners = [
            [ox, oy],
            [ox + s, oy],
            [ox + s, oy + s],
            [ox, oy + s],
            [ox, oy],
        ];
        for (ei, w) in corners.windows(2).enumerate() {
            let [x0, y0] = w[0];
            let [x1, y1] = w[1];
            // 0x40 = has_styles (no 0x80 => u16 style indices!), 0x20 = end_mode 2, 0x02 = start_mode 2
            let flags = (if ei == 0 { 0x40 } else { 0 }) | 0x20 | 0x02;
            single_frame_shape.push(flags);
            if ei == 0 {
                u16b(&mut single_frame_shape, 0); // stroke = 0
                u16b(&mut single_frame_shape, 0); // fill_0 = 0
                u16b(&mut single_frame_shape, (i + 1) as u16); // fill_1 = i+1
            }
            i32b(&mut single_frame_shape, x0 - prev[0]);
            i32b(&mut single_frame_shape, y0 - prev[1]);
            i32b(&mut single_frame_shape, x1 - x0);
            i32b(&mut single_frame_shape, y1 - y0);
            prev = [x1, y1];
        }
    }
    single_frame_shape.push(0);

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    for _ in 0..255 {
        page.begin("CPicFrame");
        page.base_start();
        page.base_end();
        page.bytes.extend_from_slice(&single_frame_shape);
        page.bytes.push(1);
        u16b(&mut page.bytes, 1);
        page.bytes.push(0);
    }
    page.base_end();
    page.layer_tail_rev2("RepeatedVectorFrames");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("oversized_repeated_vectors.fla");
    write_cfb_fla(
        &fla_path,
        &[("Contents", &contents), ("Page 1", &page.bytes)],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_element_limit" && d.code != "fla_shape_approximated"),
        "repeated vector keyframes should deduplicate into shared Graphic symbol without shedding: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    assert!(
        proj.symbols
            .iter()
            .any(|s| s.id.starts_with("fla-shared-artwork-") && s.kind == SymbolKind::Graphic),
        "expected shared Graphic symbol for repeated vector artwork"
    );
}
