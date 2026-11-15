//! Synthetic regression tests for T-329: binary FLA partial-stream recovery
//! before an undecoded record and bounded large-document degradation with
//! located warnings and visible placeholders. Contains only synthetic bytes.

use neoflash_core::{ElementKind, LayerRole, MaskMode, Player, Project, SoundSync, sample_layer};
use neoflash_import::{ImportFormat, ImportReport, import_fxg_str, import_project, inspect};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-legacy-degrade-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
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
fn string(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn compact_ascii(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    for b in s.bytes() {
        u16b(out, u16::from(b));
    }
}

struct MfcWriter {
    bytes: Vec<u8>,
    classes: BTreeMap<&'static str, u16>,
    next_id: u16,
}
impl MfcWriter {
    fn new() -> Self {
        Self {
            bytes: vec![1],
            classes: BTreeMap::new(),
            next_id: 1,
        }
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(&class_id) = self.classes.get(class) {
            u16b(&mut self.bytes, 0x8000 | class_id);
        } else {
            u16b(&mut self.bytes, 0xffff);
            u16b(&mut self.bytes, 1);
            u16b(&mut self.bytes, class.len() as u16);
            self.bytes.extend_from_slice(class.as_bytes());
            self.classes.insert(class, self.next_id);
            self.next_id = self.next_id.checked_add(1).unwrap();
        }
        let obj_id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap();
        obj_id
    }
    fn base_start(&mut self) {
        self.bytes.extend_from_slice(&[1, 0]);
    }
    fn base_end(&mut self) {
        u16b(&mut self.bytes, 0);
        i32b(&mut self.bytes, i32::MIN);
        i32b(&mut self.bytes, i32::MIN);
    }
    fn empty_shape_record(&mut self) {
        self.bytes.push(2); // shape_revision = 2
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, v);
        }
        self.bytes.push(2); // data_revision = 2
        u32b(&mut self.bytes, 0); // metadata
        u16b(&mut self.bytes, 0); // fills
        u16b(&mut self.bytes, 0); // strokes
        self.bytes.push(0); // end edges
    }
    fn filled_rect_shape_record(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32, rgba: u32) {
        self.bytes.push(2);
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, v);
        }
        self.bytes.push(2); // data_revision = 2
        u32b(&mut self.bytes, 0);
        u16b(&mut self.bytes, 1); // 1 fill
        u32b(&mut self.bytes, rgba);
        u16b(&mut self.bytes, 0); // legacy plain fill
        u16b(&mut self.bytes, 0); // 0 strokes

        let x0 = x_px * 5120;
        let y0 = y_px * 5120;
        let dx = w_px * 5120;
        let dy = h_px * 5120;
        // 4 closed clockwise edges around (x0, y0)..(x0 + dx, y0 + dy) with fill_1 = 1
        // Edge 0: style change (0xc0) + i32 start (0x02) + i32 end (0x20)
        self.bytes.push(0xc0 | 0x02 | 0x20);
        self.bytes.extend_from_slice(&[0, 0, 1]); // stroke=0, fill_0=0, fill_1=1
        i32b(&mut self.bytes, x0);
        i32b(&mut self.bytes, y0);
        i32b(&mut self.bytes, dx);
        i32b(&mut self.bytes, 0);
        // Edge 1: contiguous start (0x00) + i32 end (0x20)
        self.bytes.push(0x20);
        i32b(&mut self.bytes, 0);
        i32b(&mut self.bytes, dy);
        // Edge 2: (-dx, 0)
        self.bytes.push(0x20);
        i32b(&mut self.bytes, -dx);
        i32b(&mut self.bytes, 0);
        // Edge 3: (0, -dy)
        self.bytes.push(0x20);
        i32b(&mut self.bytes, 0);
        i32b(&mut self.bytes, -dy);
        self.bytes.push(0); // end edges
    }
    fn symbol_instance_child(&mut self, library_id: u32, x_twips: i32, y_twips: i32) {
        self.begin("CPicSymbol");
        self.base_start();
        self.base_end();
        self.bytes.push(1); // CPicSymbol rev 1
        for v in [65536_i32, 0, 0, 65536, x_twips, y_twips] {
            i32b(&mut self.bytes, v);
        }
        u16b(&mut self.bytes, 0); // first_frame = 0
        u32b(&mut self.bytes, library_id);
    }
    fn sprite_instance_child(&mut self, library_id: u32, name: &str, x_twips: i32) {
        self.begin("CPicSprite");
        self.base_start();
        self.base_end();
        self.bytes.push(1); // CPicSymbol base rev 1
        for v in [65536_i32, 0, 0, 65536, x_twips, 0] {
            i32b(&mut self.bytes, v);
        }
        u16b(&mut self.bytes, 0); // first_frame = 0
        u32b(&mut self.bytes, library_id);
        self.bytes.push(1); // CPicSprite rev 1: instance name only
        string(&mut self.bytes, name);
    }
    fn bitmap_instance_child(&mut self, media_id: u16, x_twips: i32, y_twips: i32) {
        self.begin("CPicBitmap");
        self.base_start();
        self.base_end();
        self.bytes.push(1); // CPicBitmap rev 1
        for v in [65536_i32, 0, 0, 65536, x_twips, y_twips] {
            i32b(&mut self.bytes, v);
        }
        u16b(&mut self.bytes, media_id);
    }
    fn video_stream_child(&mut self, name: &str) {
        self.begin("CPicVideoStream");
        self.base_start();
        self.base_end();
        self.bytes.push(3); // CPicVideoStream rev 3
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, v);
        }
        for v in [0_i32, 160 * 20, 0, 120 * 20] {
            i32b(&mut self.bytes, v);
        }
        self.bytes.push(0);
        string(&mut self.bytes, "");
        string(&mut self.bytes, name);
        u32b(&mut self.bytes, 0); // media_present = 0
    }
    fn frame_tail_source(
        &mut self,
        duration: u16,
        sound_media_id: u16,
        label: &str,
        source: Option<&str>,
    ) {
        self.frame_tail_with_key_mode(duration, 0, sound_media_id, label, source);
    }
    fn frame_tail_with_key_mode(
        &mut self,
        duration: u16,
        key_mode: u16,
        sound_media_id: u16,
        label: &str,
        source: Option<&str>,
    ) {
        self.bytes.push(21); // CPicFrame rev 21
        u16b(&mut self.bytes, duration);
        u16b(&mut self.bytes, key_mode);
        u16b(&mut self.bytes, 0); // acceleration
        u16b(&mut self.bytes, sound_media_id);
        u16b(&mut self.bytes, 0); // 0 envelope points
        u16b(&mut self.bytes, 1); // loops = 1
        self.bytes.push(0); // Event sync
        u32b(&mut self.bytes, 0); // in_point
        u32b(&mut self.bytes, 0); // out_point
        u16b(&mut self.bytes, 0); // sound_zoom
        string(&mut self.bytes, label);
        u32b(&mut self.bytes, 0); // actions rev = 0
        u32b(&mut self.bytes, if source.is_some() { 1 } else { 2 });
        if let Some(code) = source {
            string(&mut self.bytes, code);
        }
        u32b(&mut self.bytes, 0); // rotation_mode
        u32b(&mut self.bytes, 0); // rotation_count
        u32b(&mut self.bytes, 0); // label_type
        u16b(&mut self.bytes, 0); // morph_shape = null
        u32b(&mut self.bytes, 0); // shape_tween_blend
        u16b(&mut self.bytes, 0); // sound_settings = null
        string(&mut self.bytes, "");
        u32b(&mut self.bytes, 0);
        u32b(&mut self.bytes, 0);
    }
    fn layer_tail(&mut self, name: &str) {
        self.layer_tail_typed(name, 0, 0);
    }
    fn layer_tail_typed(&mut self, name: &str, layer_type: u8, related_obj_id: u16) {
        self.bytes.push(8); // CPicLayer rev 8
        string(&mut self.bytes, name);
        self.bytes.extend_from_slice(&[1, 1, 0]);
        u32b(&mut self.bytes, 20);
        u32b(&mut self.bytes, 0xff336699);
        u32b(&mut self.bytes, 0); // outline = 0
        u32b(&mut self.bytes, 1); // height_multiplier = 1
        self.bytes.push(layer_type);
        u16b(&mut self.bytes, related_obj_id);
        u16b(&mut self.bytes, 0);
        self.bytes.push(1);
    }
    fn page_tail(&mut self) {
        self.bytes.push(3); // CPicPage rev 3
        u16b(&mut self.bytes, 0);
        u32b(&mut self.bytes, 0);
    }
}

struct SymbolSpec<'a> {
    stream_name: &'a str,
    item_name: &'a str,
    library_id: u32,
    symbol_type: u8,
    bytes: &'a [u8],
}

enum MediaEntrySpec<'a> {
    LosslessBitmap {
        stream_name: &'a str,
        item_name: &'a str,
        media_id: u16,
        bytes: &'a [u8],
    },
    PcmSound {
        stream_name: &'a str,
        item_name: &'a str,
        media_id: u16,
        sound_flags: u8,
        sample_count: u32,
        bytes: &'a [u8],
    },
}

fn native_argb_bitmap(width: u16, height: u16, rgba: [u8; 4]) -> Vec<u8> {
    let stride = width.checked_mul(4).unwrap();
    let mut out = Vec::with_capacity(28 + usize::from(stride) * usize::from(height));
    out.push(3); // revision 3
    out.push(5); // 32-bit ARGB
    u16b(&mut out, stride);
    u16b(&mut out, width);
    u16b(&mut out, height);
    for v in [0_i32, i32::from(width) * 20, 0_i32, i32::from(height) * 20] {
        i32b(&mut out, v);
    }
    out.push(1); // alpha = true
    out.push(0); // compressed = false
    let argb = [rgba[3], rgba[0], rgba[1], rgba[2]];
    for _ in 0..(usize::from(width) * usize::from(height)) {
        out.extend_from_slice(&argb);
    }
    out
}

fn build_contents_bytes(
    scene_stream_name: &str,
    symbols: &[SymbolSpec<'_>],
    media: &[MediaEntrySpec<'_>],
    trailing_suffix_bytes: &[u8],
) -> Vec<u8> {
    let mut c = vec![0x3f]; // Flash 8
    u32b(&mut c, 1);
    u32b(&mut c, 0);
    c.push(0);
    c.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut c, 0);
    }
    // Scene 1: CDocumentPage class = 1, object = 2
    u16b(&mut c, 0xffff);
    u16b(&mut c, 1);
    u16b(&mut c, "CDocumentPage".len() as u16);
    c.extend_from_slice(b"CDocumentPage");
    c.push(4); // CDocumentPage rev 4 (compact ASCII stream name)
    compact_ascii(&mut c, scene_stream_name);
    string(&mut c, "Scene 1");
    u32b(&mut c, 1); // scene library_id
    c.push(2); // MovieClip
    u16b(&mut c, 0); // end scenes
    u16b(&mut c, 1); // count = 1
    u16b(&mut c, 0);
    u16b(&mut c, 2); // selected_scene = object 2

    // Library symbols reuse CDocumentPage class 1 -> 0x8001
    let mut next_ref = 3_u16;
    for sym in symbols {
        u16b(&mut c, 0x8001);
        c.push(4);
        compact_ascii(&mut c, sym.stream_name);
        string(&mut c, sym.item_name);
        u32b(&mut c, sym.library_id);
        c.push(sym.symbol_type);
        next_ref = next_ref.checked_add(1).unwrap();
    }
    u16b(&mut c, 0); // end symbols
    u16b(&mut c, symbols.len() as u16);

    let mut bits_class: Option<u16> = None;
    let mut sound_class: Option<u16> = None;
    for item in media {
        match item {
            MediaEntrySpec::LosslessBitmap {
                stream_name,
                item_name,
                media_id,
                ..
            } => {
                if let Some(cid) = bits_class {
                    u16b(&mut c, 0x8000 | cid);
                } else {
                    u16b(&mut c, 0xffff);
                    u16b(&mut c, 1);
                    u16b(&mut c, "CMediaBits".len() as u16);
                    c.extend_from_slice(b"CMediaBits");
                    bits_class = Some(next_ref);
                    next_ref = next_ref.checked_add(1).unwrap();
                }
                next_ref = next_ref.checked_add(1).unwrap();
                c.push(1); // CMedia base rev 1
                compact_ascii(&mut c, stream_name);
                string(&mut c, item_name);
                u16b(&mut c, *media_id);
                c.push(3); // CMediaBits sub rev 3
                c.extend_from_slice(&[0, 0]);
                c.push(1); // smoothing
                i32b(&mut c, 0); // native raster
            }
            MediaEntrySpec::PcmSound {
                stream_name,
                item_name,
                media_id,
                sound_flags,
                sample_count,
                bytes,
            } => {
                if let Some(cid) = sound_class {
                    u16b(&mut c, 0x8000 | cid);
                } else {
                    u16b(&mut c, 0xffff);
                    u16b(&mut c, 1);
                    u16b(&mut c, "CMediaSound".len() as u16);
                    c.extend_from_slice(b"CMediaSound");
                    sound_class = Some(next_ref);
                    next_ref = next_ref.checked_add(1).unwrap();
                }
                let _ = next_ref;
                next_ref = next_ref.saturating_add(1);
                c.push(6); // CMedia base rev 6
                compact_utf16(&mut c, stream_name);
                string(&mut c, item_name);
                u16b(&mut c, *media_id);
                string(&mut c, "");
                u32b(&mut c, 0);
                u32b(&mut c, 3); // library_metadata rev 3
                u32b(&mut c, 0);
                u32b(&mut c, 0);
                u32b(&mut c, 0);
                u32b(&mut c, 0);
                c.push(8); // CMediaSound sub rev 8
                u16b(&mut c, u16::from(*sound_flags));
                u32b(&mut c, *sample_count);
                u16b(&mut c, 0);
                u16b(&mut c, 0);
                u32b(&mut c, bytes.len() as u32);
                c.push(0);
                u16b(&mut c, 0);
                u32b(&mut c, 0);
            }
        }
    }
    u16b(&mut c, 0); // end media
    u16b(&mut c, media.len() as u16);

    // Document settings (550x400 px, 24 fps)
    u16b(&mut c, 2);
    c.push(0);
    for word in [0_i32, 11_000, 0, 8_000] {
        i32b(&mut c, word);
    }
    u32b(&mut c, 20 * 65536);
    c.extend_from_slice(&[0; 5]);
    c.extend_from_slice(&[0; 16]);
    c.extend_from_slice(&[0; 4]);
    u32b(&mut c, 65536);
    u16b(&mut c, 0);
    u16b(&mut c, 0);
    u32b(&mut c, 0);
    u32b(&mut c, 0xffffffff);
    u32b(&mut c, 24 * 65536);
    c.extend_from_slice(&[0; 3]);
    u32b(&mut c, 20);
    c.extend_from_slice(trailing_suffix_bytes);
    c
}

fn build_cfb(
    contents_stream_name: &str,
    contents: &[u8],
    scene_stream_name: &str,
    scene_bytes: &[u8],
    symbols: &[SymbolSpec<'_>],
    media: &[MediaEntrySpec<'_>],
) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb.create_stream(contents_stream_name)
        .unwrap()
        .write_all(contents)
        .unwrap();
    cfb.create_stream(scene_stream_name)
        .unwrap()
        .write_all(scene_bytes)
        .unwrap();
    for sym in symbols {
        cfb.create_stream(sym.stream_name)
            .unwrap()
            .write_all(sym.bytes)
            .unwrap();
    }
    for m in media {
        let (name, bytes) = match m {
            MediaEntrySpec::LosslessBitmap {
                stream_name, bytes, ..
            } => (*stream_name, *bytes),
            MediaEntrySpec::PcmSound {
                stream_name, bytes, ..
            } => (*stream_name, *bytes),
        };
        cfb.create_stream(name).unwrap().write_all(bytes).unwrap();
    }
    cfb.into_inner().into_inner()
}

fn import_fla_checked(path: &std::path::Path) -> (Project, ImportReport) {
    let report = inspect(path).unwrap();
    assert_eq!(report.format, ImportFormat::LegacyBinaryFla);
    assert!(
        report.can_import,
        "expected can_import=true: {:?}",
        report.diagnostics
    );
    let project = import_project(path).unwrap();
    (project, report)
}

fn maybe_verify_html5_export(project_json: &str) {
    let bin_name = format!("neoflash-export{}", std::env::consts::EXE_SUFFIX);
    let export_bin = std::env::var_os("NEOFLASH_EXPORT_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            let profile_dir = exe.parent()?.parent()?;
            Some(profile_dir.join(&bin_name))
        });
    let Some(bin) = export_bin.filter(|p| p.is_file()) else {
        assert!(
            std::env::var_os("NEOFLASH_REQUIRE_HTML5_EXPORT").is_none(),
            "NEOFLASH_REQUIRE_HTML5_EXPORT=1 set, but neoflash-export binary was not found"
        );
        return;
    };
    let dir = TempDir::new();
    let nflash_path = dir.write("degraded.nflash", project_json.as_bytes());
    let rt_dir = dir.0.join("runtime");
    let out_dir = dir.0.join("html5-out");
    fs::create_dir_all(&rt_dir).unwrap();
    fs::write(rt_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    fs::write(rt_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let output = std::process::Command::new(&bin)
        .arg(&nflash_path)
        .arg("--output")
        .arg(&out_dir)
        .arg("--runtime")
        .arg(&rt_dir)
        .output()
        .expect("spawn neoflash-export");
    assert!(
        output.status.success(),
        "neoflash-export HTML5 export failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out_dir.join("index.html").is_file(),
        "expected exported index.html in {}",
        out_dir.display()
    );
}

fn assert_valid_and_roundtrips_nflash(project: &Project) {
    project
        .validate()
        .unwrap_or_else(|e| panic!("imported project failed validate(): {e}"));
    let json = project.to_json().expect("serialize .nflash JSON");
    let reopened = Project::from_json(&json).expect("deserialize .nflash JSON");
    reopened
        .validate()
        .expect("reopened .nflash project must validate");
    assert_eq!(&reopened, project, ".nflash save/reopen must be lossless");
    let player = Player::new(&reopened);
    let _ = player.evaluate(&reopened);
    maybe_verify_html5_export(&json);
}

#[test]
fn binary_classic_symbol_swap_keeps_motion_and_resets_child_lifetimes() {
    let dir = TempDir::new();
    let child = |rgba| {
        let mut stream = MfcWriter::new();
        stream.begin("CPicPage");
        stream.base_start();
        stream.begin("CPicLayer");
        stream.base_start();
        stream.begin("CPicFrame");
        stream.base_start();
        stream.base_end();
        stream.filled_rect_shape_record(0, 0, 20, 20, rgba);
        stream.frame_tail_source(8, 0, "", None);
        stream.base_end();
        stream.layer_tail("Artwork");
        stream.base_end();
        stream.page_tail();
        stream.bytes
    };
    let red = child(0xff0000ff);
    let blue = child(0xffff0000);
    let symbols = [
        SymbolSpec {
            stream_name: "Symbol 1",
            item_name: "Red clip",
            library_id: 10,
            symbol_type: 2,
            bytes: &red,
        },
        SymbolSpec {
            stream_name: "Symbol 2",
            item_name: "Blue clip",
            library_id: 11,
            symbol_type: 2,
            bytes: &blue,
        },
    ];
    let mut page = MfcWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    // Flash may encode a zero-duration lead record with an empty shape, label
    // and absent actions. Those envelopes do not constitute lost content.
    for (duration, mode, placement) in [
        (0, 0, None),
        (4, 1, Some((10, "beforeSwap", 0))),
        (3, 0, Some((11, "afterSwap", 1600))),
        (1, 0, None),
        (2, 0, Some((11, "afterSwap", 1600))),
    ] {
        page.begin("CPicFrame");
        page.base_start();
        if let Some((library, name, x)) = placement {
            page.sprite_instance_child(library, name, x);
        }
        page.base_end();
        page.empty_shape_record();
        page.frame_tail_with_key_mode(duration, mode, 0, "", None);
    }
    page.base_end();
    page.layer_tail("Classic swap");
    page.base_end();
    page.page_tail();
    let contents = build_contents_bytes("Page 1", &symbols, &[], &[]);
    let bytes = build_cfb("Contents", &contents, "Page 1", &page.bytes, &symbols, &[]);
    let path = dir.write("classic-symbol-swap.fla", &bytes);
    let (project, report) = import_fla_checked(&path);
    assert_valid_and_roundtrips_nflash(&project);
    assert_eq!(
        import_project(&path).unwrap(),
        project,
        "stable imported IDs"
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| { d.code != "fla_tween_dropped" && d.code != "fla_frame_duration" }),
        "{:?}",
        report.diagnostics
    );

    let timeline = &project.symbol(&project.root).unwrap().timeline;
    assert_eq!(timeline.frame_count, 10);
    let layer = &timeline.layers[0];
    assert_eq!(
        layer.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>(),
        [0, 4, 7, 8]
    );
    let start = &layer.keyframes[0].elements[0];
    let end = &layer.keyframes[1].elements[0];
    let later = &layer.keyframes[3].elements[0];
    let symbol_id = |element: &neoflash_core::Element| match &element.kind {
        ElementKind::Instance { symbol_id, .. } => symbol_id.clone(),
        _ => panic!("expected imported library instance"),
    };
    let red_id = symbol_id(start);
    let blue_id = symbol_id(end);
    assert_ne!(red_id, blue_id);
    assert_eq!(project.symbol(&red_id).unwrap().name, "Red clip");
    assert_eq!(project.symbol(&blue_id).unwrap().name, "Blue clip");
    assert_eq!((&*start.name, &*end.name), ("beforeSwap", "afterSwap"));
    assert_eq!(
        start.id, end.id,
        "explicit tween connects the placement pose"
    );
    assert_ne!(end.id, later.id, "a blank breaks placement continuity");
    assert!(layer.keyframes[0].tween.is_some());
    let middle = sample_layer(layer, 2);
    assert_eq!(symbol_id(&middle[0]), red_id);
    assert_eq!(middle[0].name, "beforeSwap");
    assert!((middle[0].transform.x - 40.0).abs() < 1e-8);
    assert_eq!(symbol_id(&sample_layer(layer, 4)[0]), blue_id);

    let placement = vec![start.id.clone()];
    let reentry = vec![later.id.clone()];
    let node = |player: &Player, path: &[String]| {
        player
            .display_nodes(&project)
            .into_iter()
            .find(|n| n.path == path)
            .unwrap()
    };
    let mut player = Player::new(&project);
    let first_lifetime = player.lifetime_id(&placement).unwrap();
    player.tick(&project);
    player.tick(&project);
    assert_eq!(player.frame(), 2);
    assert_eq!(node(&player, &placement).frame, 2);
    assert_eq!(player.lifetime_id(&placement), Some(first_lifetime));
    player.tick(&project);
    player.tick(&project);
    let swapped = node(&player, &placement);
    assert_eq!(player.frame(), 4);
    assert_eq!(swapped.symbol_id.as_deref(), Some(blue_id.as_str()));
    assert_eq!(swapped.name, "afterSwap");
    assert_eq!(
        swapped.frame, 0,
        "replacement symbol begins with a fresh child clock"
    );
    let swapped_lifetime = player.lifetime_id(&placement).unwrap();
    assert_ne!(swapped_lifetime, first_lifetime);
    player.tick(&project);
    assert_eq!(node(&player, &placement).frame, 1);
    player.tick(&project);
    player.tick(&project);
    assert_eq!(player.frame(), 7);
    assert_eq!(player.lifetime_id(&placement), None);
    player.tick(&project);
    assert_eq!(player.frame(), 8);
    assert_eq!(node(&player, &reentry).frame, 0);
    assert_ne!(player.lifetime_id(&reentry), Some(swapped_lifetime));
}

#[test]
fn partial_streams_preserve_completed_layers_frames_shapes_media_and_scripts_before_stop() {
    let dir = TempDir::new();

    // Symbol 1: complete 1-frame MovieClip with a filled rectangle.
    let mut sym1 = MfcWriter::new();
    sym1.begin("CPicPage");
    sym1.base_start();
    sym1.begin("CPicLayer");
    sym1.base_start();
    sym1.begin("CPicFrame");
    sym1.base_start();
    sym1.base_end();
    sym1.filled_rect_shape_record(0, 0, 40, 30, 0xff22aa55);
    sym1.frame_tail_source(1, 0, "", None);
    sym1.base_end();
    sym1.layer_tail("sym_layer");
    sym1.base_end();
    sym1.page_tail();

    // Symbol 2: stops immediately before decoding a root CPicPage.
    let sym2_bytes = [1_u8, 0xff, 0xff];

    // Media 1: 2x2 lossless ARGB bitmap; Media 2: 4-sample 16-bit 44.1kHz mono PCM sound.
    let bitmap_bytes = native_argb_bitmap(2, 2, [20, 140, 220, 255]);
    let pcm_bytes: [u8; 8] = [0x00, 0x10, 0x00, 0x20, 0x00, 0xe0, 0x00, 0xf0];

    // Scene 1 ("Page 1"):
    // - Layer 1 (complete) has Frame 0 (duration 3: Symbol 1 instance + Bitmap 1 + Sound 2 + source script)
    //   followed by Frame 1 (duration 2: filled shape + compiled actions with 1 complete `stop()` command
    //   before an unknown command kind stops the stream!).
    let mut page = MfcWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();

    // Frame 0 (complete)
    page.begin("CPicFrame");
    page.base_start();
    page.symbol_instance_child(10, 400, 600);
    page.bitmap_instance_child(1, 1200, 800);
    page.base_end();
    page.empty_shape_record();
    page.frame_tail_source(3, 2, "intro", Some("var initialized = 1;\n"));

    // Frame 1 (stops mid-compiled-actions after complete `stop();`)
    page.begin("CPicFrame");
    page.base_start();
    page.base_end();
    page.filled_rect_shape_record(10, 15, 50, 25, 0xffcc4422);
    page.bytes.push(21); // CPicFrame rev 21
    u16b(&mut page.bytes, 2); // duration = 2
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // no sound
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    string(&mut page.bytes, "halted_frame");
    u32b(&mut page.bytes, 0); // actions rev = 0
    u32b(&mut page.bytes, 0); // mode = 0 (Compiled)
    u32b(&mut page.bytes, 0); // list_revision = 0
    u32b(&mut page.bytes, 2); // 2 commands declared
    u32b(&mut page.bytes, 2); // command 0: complete `stop()`
    u32b(&mut page.bytes, 999); // command 1: unknown compiled command -> sequential stop!

    let symbols = [
        SymbolSpec {
            stream_name: "Symbol 1",
            item_name: "HeroClip",
            library_id: 10,
            symbol_type: 2,
            bytes: &sym1.bytes,
        },
        SymbolSpec {
            stream_name: "Symbol 2",
            item_name: "TruncatedClip",
            library_id: 11,
            symbol_type: 2,
            bytes: &sym2_bytes,
        },
    ];
    let media = [
        MediaEntrySpec::LosslessBitmap {
            stream_name: "Media 1",
            item_name: "Sprite.png",
            media_id: 1,
            bytes: &bitmap_bytes,
        },
        MediaEntrySpec::PcmSound {
            stream_name: "Media 2",
            item_name: "Cue.wav",
            media_id: 2,
            sound_flags: 0x0e, // PCM, 44.1kHz, 16-bit, mono
            sample_count: 4,
            bytes: &pcm_bytes,
        },
    ];
    // Append an unsupported Contents extension record byte (`99`) after valid settings
    // and use lowercase CFB stream names (`contents`, `page 1`) to test case-insensitive role matching.
    let contents = build_contents_bytes("Page 1", &symbols, &media, &[99]);
    let fla_bytes = build_cfb(
        "contents",
        &contents,
        "page 1",
        &page.bytes,
        &symbols,
        &media,
    );
    let fla_path = dir.write("partial_stream.fla", &fla_bytes);

    let (imported_project, imported_report) = import_fla_checked(&fla_path);
    assert_valid_and_roundtrips_nflash(&imported_project);

    let codes: Vec<&str> = imported_report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.contains(&"fla_contents_partial"), "{codes:?}");
    assert!(
        codes.contains(&"fla_action_command_unsupported"),
        "{codes:?}"
    );
    assert!(codes.contains(&"fla_truncated_record"), "{codes:?}");
    assert!(codes.contains(&"fla_layer_partial"), "{codes:?}");
    assert!(codes.contains(&"fla_actions_partial"), "{codes:?}");

    // Verify PNG bitmap resource and WAV sound resource both survived.
    assert!(
        imported_project
            .resources
            .iter()
            .any(|r| r.id == "legacy-bitmap-1" && r.media_type == "image/png")
    );
    assert!(
        imported_project
            .resources
            .iter()
            .any(|r| r.id == "legacy-sound-2" && r.media_type == "audio/wav")
    );

    // Verify Symbol 1 kept its filled shape and Symbol 2 degraded to a valid 1-frame symbol
    // without shifting `fla-symbol-*` IDs.
    let hero_sym = imported_project
        .symbols
        .iter()
        .find(|s| s.name == "HeroClip")
        .expect("HeroClip symbol preserved");
    assert_eq!(hero_sym.id, "fla-symbol-4");
    assert_eq!(hero_sym.timeline.layers[0].keyframes[0].elements.len(), 1);
    let truncated_sym = imported_project
        .symbols
        .iter()
        .find(|s| s.name == "TruncatedClip")
        .expect("TruncatedClip symbol preserved as empty 1-frame timeline");
    assert_eq!(truncated_sym.id, "fla-symbol-5");
    assert_eq!(truncated_sym.timeline.frame_count, 1);

    // Verify root scene preserved BOTH Frame 0 (duration 3, Symbol 1 + Bitmap 1 + Sound 2 + script)
    // AND Frame 1 (duration 2, filled shape + translated `stop();` before the undecoded command)!
    let root = imported_project.symbol(&imported_project.root).unwrap();
    assert_eq!(root.timeline.frame_count, 5);
    let root_layer = &root.timeline.layers[0];
    assert_eq!(root_layer.keyframes.len(), 2);

    let kf0 = &root_layer.keyframes[0];
    assert_eq!(kf0.frame, 0);
    assert_eq!(kf0.label, "intro");
    assert!(kf0.elements.iter().any(
        |e| matches!(&e.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == &hero_sym.id)
    ));
    assert!(
        kf0.elements
            .iter()
            .any(|e| matches!(&e.kind, ElementKind::Bitmap { resource_id, .. } if resource_id == "legacy-bitmap-1"))
    );
    let sound = kf0.sound.as_ref().expect("frame 0 sound preserved");
    assert_eq!(sound.resource, "legacy-sound-2");
    assert_eq!(sound.sync, SoundSync::Event);

    let kf1 = &root_layer.keyframes[1];
    assert_eq!(kf1.frame, 3);
    assert_eq!(kf1.label, "halted_frame");
    assert_eq!(kf1.elements.len(), 1, "frame 1 shape before stop preserved");

    // Verify both Frame 0's source script and Frame 1's recovered `stop();` script survived.
    assert_eq!(root.timeline.scripts.len(), 2);
    assert!(
        root.timeline.scripts[0]
            .script
            .source
            .contains("initialized = 1")
    );
    assert!(
        root.timeline.scripts[1]
            .script
            .source
            .starts_with("stop();\n"),
        "unexpected partial script: {}",
        root.timeline.scripts[1].script.source
    );

    // Also verify partial CPicShape group recovery (`fla_group_partial`): child 0 survives when
    // the enclosing group stops before its own tail completes.
    let mut group_page = MfcWriter::new();
    group_page.begin("CPicPage");
    group_page.base_start();
    group_page.begin("CPicLayer");
    group_page.base_start();
    group_page.begin("CPicFrame");
    group_page.base_start();
    group_page.begin("CPicShape"); // group
    group_page.base_start();
    // Child 0: complete filled rectangle
    group_page.begin("CPicShape");
    group_page.base_start();
    group_page.base_end();
    group_page.filled_rect_shape_record(5, 5, 20, 20, 0xff1188ee);
    // Child 1: unknown class tag stops the group mid-children!
    u16b(&mut group_page.bytes, 0xffff);
    u16b(&mut group_page.bytes, 1);
    u16b(&mut group_page.bytes, "CPicUnknownFuture".len() as u16);
    group_page.bytes.extend_from_slice(b"CPicUnknownFuture");

    let group_contents = build_contents_bytes("Page 1", &[], &[], &[]);
    let group_fla = build_cfb(
        "Contents",
        &group_contents,
        "Page 1",
        &group_page.bytes,
        &[],
        &[],
    );
    let (group_project, group_report) =
        import_fla_checked(&dir.write("partial_group.fla", &group_fla));
    assert_valid_and_roundtrips_nflash(&group_project);
    let group_codes: Vec<&str> = group_report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        group_codes.contains(&"fla_group_partial"),
        "{group_codes:?}"
    );
    let group_root = group_project.symbol(&group_project.root).unwrap();
    assert_eq!(
        group_root.timeline.layers[0].keyframes[0].elements.len(),
        1,
        "complete group child before stream stop must survive"
    );
}

#[test]
fn large_document_vector_budgets_emit_located_placeholders_and_split_strokes_cleanly() {
    let dir = TempDir::new();

    // Shape 1: 20,001 disconnected closed triangles on fill 1 -> 60,003 edges (<= 100,000 geometry items)
    // which assemble into 100,005 PathCommands (> MAX_PATH_COMMANDS = 100,000), triggering a located
    // `fla_shape_approximated` warning and bounding-box Rectangle placeholder!
    // Shape 2: 50,001 disconnected stroke segments on stroke 1 -> 50,001 edges -> 100,002 PathCommands,
    // split losslessly into 2 PaintedPath elements (<= 100,000 commands each)!
    // Shape 3: 100,001 edges -> exceeds geometry::MAX_ITEMS, triggering `fla_shape_skipped` + placeholder
    // while keeping Shapes 1 and 2 intact!
    let mut page = MfcWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();

    // Child 0: Shape 1 (fill > MAX_PATH_COMMANDS)
    page.begin("CPicShape");
    page.base_start();
    page.base_end();
    page.bytes.push(2);
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page.bytes, v);
    }
    page.bytes.push(2);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1); // 1 fill
    u32b(&mut page.bytes, 0xffdd3344);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // 0 strokes
    for i in 0..20_001_i32 {
        let first = i == 0;
        // Start of triangle i is offset by (+5120, 0) from previous triangle's end (which returned to start).
        page.bytes
            .push((if first { 0xc0 } else { 0 }) | 0x01 | 0x10);
        if first {
            page.bytes.extend_from_slice(&[0, 0, 1]);
        }
        u16b(&mut page.bytes, 5120_i16 as u16);
        u16b(&mut page.bytes, 0);
        u16b(&mut page.bytes, 2560_i16 as u16);
        u16b(&mut page.bytes, 0);
        // Edge 2 of triangle: (0, +2560)
        page.bytes.push(0x10);
        u16b(&mut page.bytes, 0);
        u16b(&mut page.bytes, 2560_i16 as u16);
        // Edge 3 of triangle: (-2560, -2560) back to triangle start
        page.bytes.push(0x10);
        u16b(&mut page.bytes, (-2560_i16) as u16);
        u16b(&mut page.bytes, (-2560_i16) as u16);
    }
    page.bytes.push(0);

    // Child 1: Shape 2 (stroke > MAX_PATH_COMMANDS split into parts)
    page.begin("CPicShape");
    page.base_start();
    page.base_end();
    page.bytes.push(2);
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page.bytes, v);
    }
    page.bytes.push(2);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // 0 fills
    u16b(&mut page.bytes, 1); // 1 stroke
    u32b(&mut page.bytes, 0xff1144cc);
    u16b(&mut page.bytes, 20); // 1 px stroke
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    for i in 0..50_001_i32 {
        let first = i == 0;
        // Disconnected 1px segments so each edge emits MoveTo + LineTo (100,002 commands total).
        page.bytes
            .push((if first { 0xc0 } else { 0 }) | 0x01 | 0x10);
        if first {
            page.bytes.extend_from_slice(&[1, 0, 0]);
        }
        u16b(&mut page.bytes, 256_i16 as u16);
        u16b(&mut page.bytes, 0);
        u16b(&mut page.bytes, 256_i16 as u16);
        u16b(&mut page.bytes, 0);
    }
    page.bytes.push(0);

    // Child 2: Shape 3 (exceeds 100,000 geometry items -> stops stream with `fla_shape_limit`,
    // emitting a located placeholder without discarding Shapes 1 and 2!)
    page.begin("CPicShape");
    page.base_start();
    page.base_end();
    page.bytes.push(2);
    for v in [65536_i32, 0, 0, 65536, 400, 600] {
        i32b(&mut page.bytes, v);
    }
    page.bytes.push(2);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    u32b(&mut page.bytes, 0xff00aa00);
    u16b(&mut page.bytes, 20);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    for i in 0..100_001_i32 {
        let first = i == 0;
        page.bytes.push((if first { 0xc0 } else { 0 }) | 0x10);
        if first {
            page.bytes.extend_from_slice(&[1, 0, 0]);
        }
        u16b(&mut page.bytes, 256_i16 as u16);
        u16b(&mut page.bytes, 0);
    }
    page.bytes.push(0);

    let contents = build_contents_bytes("Page 1", &[], &[], &[]);
    let fla_bytes = build_cfb("Contents", &contents, "Page 1", &page.bytes, &[], &[]);
    let (vec_project, vec_report) =
        import_fla_checked(&dir.write("vector_budgets.fla", &fla_bytes));
    assert_valid_and_roundtrips_nflash(&vec_project);

    let codes: Vec<&str> = vec_report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.contains(&"fla_shape_approximated"), "{codes:?}");
    assert!(codes.contains(&"fla_shape_limit"), "{codes:?}");
    assert!(codes.contains(&"fla_object_placeholder"), "{codes:?}");
    assert!(codes.contains(&"fla_frame_partial"), "{codes:?}");
    assert!(codes.contains(&"fla_layer_partial"), "{codes:?}");

    let root = vec_project.symbol(&vec_project.root).unwrap();
    let elems = &root.timeline.layers[0].keyframes[0].elements;
    // 3 CPicShape children on Frame 0:
    // - elems[0] -> group symbol containing 1 bounding-box Rectangle (fill > MAX_PATH_COMMANDS)
    // - elems[1] -> group symbol containing 2 split PaintedPath stroke chunks (100_000 + 3 commands)
    // - elems[2] -> located PLACEHOLDER instance for Child 2 (exceeded 100,000 geometry items)
    assert_eq!(elems.len(), 3);
    let shape1_sym = match &elems[0].kind {
        ElementKind::Instance { symbol_id, .. } => vec_project.symbol(symbol_id).unwrap(),
        other => panic!("expected group instance for Shape 1, got {other:?}"),
    };
    let shape1_elems = &shape1_sym.timeline.layers[0].keyframes[0].elements;
    assert_eq!(shape1_elems.len(), 1);
    assert!(matches!(
        shape1_elems[0].kind,
        ElementKind::Rectangle { width, height, .. } if width > 10_000.0 && height > 0.0
    ));

    let shape2_sym = match &elems[1].kind {
        ElementKind::Instance { symbol_id, .. } => vec_project.symbol(symbol_id).unwrap(),
        other => panic!("expected group instance for Shape 2, got {other:?}"),
    };
    let shape2_elems = &shape2_sym.timeline.layers[0].keyframes[0].elements;
    assert_eq!(shape2_elems.len(), 2);
    assert!(matches!(
        &shape2_elems[0].kind,
        ElementKind::PaintedPath { commands, .. } if commands.len() == 100_000
    ));
    assert!(matches!(
        &shape2_elems[1].kind,
        ElementKind::PaintedPath { commands, .. } if !commands.is_empty() && commands.len() <= 10
    ));
    assert!(matches!(
        &elems[2].kind,
        ElementKind::Instance { symbol_id, .. } if symbol_id == "fla-unsupported-object"
    ));
}

#[test]
fn large_document_media_symbol_and_fxg_budgets_degrade_with_located_placeholders() {
    let dir = TempDir::new();

    // 1. Media budget: 1 oversized bitmap (5000x1 > 4096 max dimension) + 257 distinct valid 2x2 bitmaps
    //    all placed on stage (> 256 placed bitmap buffer budget).
    let oversized_bmp = native_argb_bitmap(5000, 1, [200, 50, 50, 255]);
    let mut small_bitmaps = Vec::with_capacity(257);
    for i in 0..257_u16 {
        small_bitmaps.push((
            format!("Media {}", i + 2),
            format!("Tile_{i}.png"),
            native_argb_bitmap(2, 2, [(i & 0xff) as u8, ((i >> 1) & 0xff) as u8, 90, 255]),
        ));
    }

    let mut media_specs = Vec::with_capacity(258);
    media_specs.push(MediaEntrySpec::LosslessBitmap {
        stream_name: "Media 1",
        item_name: "Banner5000.png",
        media_id: 1,
        bytes: &oversized_bmp,
    });
    for (idx, (sname, iname, bytes)) in small_bitmaps.iter().enumerate() {
        media_specs.push(MediaEntrySpec::LosslessBitmap {
            stream_name: sname,
            item_name: iname,
            media_id: (idx + 2) as u16,
            bytes,
        });
    }

    let mut page = MfcWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    // Place oversized Media 1 at (120px, 80px) = (2400, 1600) twips, plus all 257 valid bitmaps.
    page.bitmap_instance_child(1, 2400, 1600);
    for i in 0..257_u16 {
        page.bitmap_instance_child(i + 2, i32::from(i) * 40, 200);
    }
    page.base_end();
    page.empty_shape_record();
    page.frame_tail_source(1, 0, "", None);
    page.base_end();
    page.layer_tail("bitmaps");
    page.base_end();
    page.page_tail();

    let contents = build_contents_bytes("Page 1", &[], &media_specs, &[]);
    let fla_bytes = build_cfb(
        "Contents",
        &contents,
        "Page 1",
        &page.bytes,
        &[],
        &media_specs,
    );
    let (media_project, media_report) =
        import_fla_checked(&dir.write("bitmap_budgets.fla", &fla_bytes));
    assert_valid_and_roundtrips_nflash(&media_project);

    let media_codes: Vec<&str> = media_report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        media_codes.contains(&"fla_bitmap_skipped"),
        "{media_codes:?}"
    );
    assert!(
        !media_codes.contains(&"fla_source_bitmap_missing"),
        "skipped oversized bitmap should emit fla_bitmap_skipped, not missing: {media_codes:?}"
    );
    let bmp_root = media_project.symbol(&media_project.root).unwrap();
    let placed_elems = &bmp_root.timeline.layers[0].keyframes[0].elements;
    assert_eq!(placed_elems.len(), 258);
    // First element (oversized Media 1) preserved its (120, 80) stage coordinates as a placeholder!
    assert_eq!(
        (placed_elems[0].transform.x, placed_elems[0].transform.y),
        (120.0, 80.0)
    );
    assert!(matches!(placed_elems[0].kind, ElementKind::Instance { .. }));
    let bitmap_elem_count = placed_elems
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::Bitmap { .. }))
        .count();
    assert_eq!(bitmap_elem_count, 256);
    assert!(matches!(
        placed_elems[257].kind,
        ElementKind::Instance { .. }
    ));

    // 2. Symbol cycle + >100,000 expanded scene nodes budget:
    //    - Symbol 1 & Symbol 2 reference each other (cycle -> `fla_symbol_nesting_limit`)
    //    - Symbol 3 contains 320 shapes; Scene 1 places 320 instances of Symbol 3
    //      (320 * 321 + 1 = 102,721 > 100,000 expanded nodes -> `fla_expanded_node_limit`!)
    let build_single_instance_symbol = |target_lib_id: u32| -> Vec<u8> {
        let mut w = MfcWriter::new();
        w.begin("CPicPage");
        w.base_start();
        w.begin("CPicLayer");
        w.base_start();
        w.begin("CPicFrame");
        w.base_start();
        w.symbol_instance_child(target_lib_id, 0, 0);
        w.base_end();
        w.empty_shape_record();
        w.frame_tail_source(1, 0, "", None);
        w.base_end();
        w.layer_tail("cyc");
        w.base_end();
        w.page_tail();
        w.bytes
    };
    let cyc_a = build_single_instance_symbol(21);
    let cyc_b = build_single_instance_symbol(20);

    let mut leaf_fan = MfcWriter::new();
    leaf_fan.begin("CPicPage");
    leaf_fan.base_start();
    leaf_fan.begin("CPicLayer");
    leaf_fan.base_start();
    leaf_fan.begin("CPicFrame");
    leaf_fan.base_start();
    for i in 0..320_i32 {
        leaf_fan.begin("CPicShape");
        leaf_fan.base_start();
        leaf_fan.base_end();
        leaf_fan.filled_rect_shape_record(i * 2, 0, 1, 1, 0xff4488cc);
    }
    leaf_fan.base_end();
    leaf_fan.empty_shape_record();
    leaf_fan.frame_tail_source(1, 0, "", None);
    leaf_fan.base_end();
    leaf_fan.layer_tail("fan_leaf");
    leaf_fan.base_end();
    leaf_fan.page_tail();

    let mut fan_scene = MfcWriter::new();
    fan_scene.begin("CPicPage");
    fan_scene.base_start();
    fan_scene.begin("CPicLayer");
    fan_scene.base_start();
    fan_scene.begin("CPicFrame");
    fan_scene.base_start();
    fan_scene.symbol_instance_child(20, 0, 0);
    for i in 0..320_i32 {
        fan_scene.symbol_instance_child(22, i * 40, 40);
    }
    fan_scene.base_end();
    fan_scene.empty_shape_record();
    fan_scene.frame_tail_source(1, 0, "", None);
    fan_scene.base_end();
    fan_scene.layer_tail("fan_root");
    fan_scene.base_end();
    fan_scene.page_tail();

    let fan_symbols = [
        SymbolSpec {
            stream_name: "Symbol 1",
            item_name: "CycleA",
            library_id: 20,
            symbol_type: 2,
            bytes: &cyc_a,
        },
        SymbolSpec {
            stream_name: "Symbol 2",
            item_name: "CycleB",
            library_id: 21,
            symbol_type: 2,
            bytes: &cyc_b,
        },
        SymbolSpec {
            stream_name: "Symbol 3",
            item_name: "DenseTile320",
            library_id: 22,
            symbol_type: 2,
            bytes: &leaf_fan.bytes,
        },
    ];
    let fan_contents = build_contents_bytes("Page 1", &fan_symbols, &[], &[]);
    let fan_fla = build_cfb(
        "Contents",
        &fan_contents,
        "Page 1",
        &fan_scene.bytes,
        &fan_symbols,
        &[],
    );
    let (fan_project, fan_report) = import_fla_checked(&dir.write("expanded_nodes.fla", &fan_fla));
    assert_valid_and_roundtrips_nflash(&fan_project);
    let fan_codes: Vec<&str> = fan_report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        fan_codes.contains(&"fla_symbol_nesting_limit"),
        "{fan_codes:?}"
    );
    assert!(
        fan_codes.contains(&"fla_expanded_node_limit"),
        "{fan_codes:?}"
    );

    // 3. FXG `<Path>` > 100,000 commands degrades to a located bounding-box Rectangle placeholder
    //    while preserving sibling vector elements!
    let mut path_data = String::with_capacity(1_300_000);
    path_data.push_str("M 10 20");
    for i in 1..=100_001_u32 {
        use std::fmt::Write as _;
        let x = 10 + (i % 250);
        let y = 20 + (i % 180);
        let _ = write!(path_data, " L {x} {y}");
    }
    let fxg = format!(
        r##"<?xml version="1.0" encoding="utf-8"?>
<Graphic version="2.0" viewWidth="400" viewHeight="300" xmlns="http://ns.adobe.com/fxg/2008">
  <Group>
    <Path data="{path_data}">
      <fill><SolidColor color="#3366CC"/></fill>
    </Path>
    <Rect x="300" y="40" width="50" height="60">
      <fill><SolidColor color="#22AA55"/></fill>
    </Rect>
  </Group>
</Graphic>"##
    );
    let (fxg_project, fxg_report) = import_fxg_str(&fxg, "oversized_path.fxg").unwrap();
    assert_valid_and_roundtrips_nflash(&fxg_project);
    assert!(
        fxg_report
            .diagnostics
            .iter()
            .any(|d| d.code == "fxg-path-approximated")
    );
    let fxg_root = fxg_project.symbol(&fxg_project.root).unwrap();
    let fxg_elems = &fxg_root.timeline.layers[0].keyframes[0].elements;
    assert_eq!(
        fxg_elems.len(),
        2,
        "placeholder + sibling Rect both preserved"
    );
    assert!(matches!(
        fxg_elems[0].kind,
        ElementKind::Rectangle { width, height, .. } if width > 200.0 && height > 150.0
    ));
}

#[test]
fn binary_fla_folder_artwork_and_nested_mask_layers_degrade_and_validate() {
    let dir = TempDir::new();

    // Symbol 1 (`InnerBadMask`, library_id = 30): has a Mask layer containing a Video viewport
    // plus a masked child layer, exercising nested symbol mask fixpoint propagation.
    let mut inner = MfcWriter::new();
    inner.begin("CPicPage");
    inner.base_start();
    let inner_mask_id = inner.begin("CPicLayer");
    inner.base_start();
    inner.begin("CPicFrame");
    inner.base_start();
    inner.video_stream_child("vid_mask");
    inner.base_end();
    inner.empty_shape_record();
    inner.frame_tail_source(1, 0, "", None);
    inner.base_end();
    inner.layer_tail_typed("InnerVideoMask", 4, 0);

    inner.begin("CPicLayer");
    inner.base_start();
    inner.begin("CPicFrame");
    inner.base_start();
    inner.base_end();
    inner.filled_rect_shape_record(0, 0, 40, 40, 0xff3366cc);
    inner.frame_tail_source(1, 0, "", None);
    inner.base_end();
    inner.layer_tail_typed("InnerMaskedArt", 0, inner_mask_id);
    inner.base_end();
    inner.page_tail();

    // Root scene with:
    // 1) Childless Folder layer containing filled shape artwork -> demoted to Normal, keeping artwork.
    // 2) Parent Folder layer containing stray artwork + a child Normal layer -> stays Folder, clears stray artwork.
    // 3) Bitmap Mask layer + masked Normal layer -> promoted to MaskMode::Alpha (format >= 9).
    // 4) Nested-symbol Mask layer placing `InnerBadMask` + masked Normal layer -> demoted to Normal.
    let mut scene = MfcWriter::new();
    scene.begin("CPicPage");
    scene.base_start();

    // Layer A: childless Folder with artwork
    scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.base_end();
    scene.filled_rect_shape_record(4, 4, 24, 24, 0xff22aa55);
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("ChildlessFolderWithArt", 3, 0);

    // Layer B: parent Folder with stray artwork + Layer C child referencing Layer B
    let parent_folder_id = scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.base_end();
    scene.filled_rect_shape_record(30, 4, 24, 24, 0xffcc4422);
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("ParentFolderWithStrayArt", 3, 0);

    scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.base_end();
    scene.filled_rect_shape_record(60, 4, 24, 24, 0xff4488ee);
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("FolderChildLayer", 0, parent_folder_id);

    // Layer D: Geometry Mask placing valid bitmap (media_id = 1) + Layer E masked child
    let bmp_mask_id = scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.bitmap_instance_child(1, 0, 0);
    scene.base_end();
    scene.empty_shape_record();
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("BitmapMaskLayer", 4, 0);

    scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.base_end();
    scene.filled_rect_shape_record(0, 0, 32, 32, 0xffffaa00);
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("BitmapMaskedChild", 0, bmp_mask_id);

    // Layer F: Mask placing `InnerBadMask` (library_id = 30) + Layer G masked child
    let bad_outer_mask_id = scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.symbol_instance_child(30, 0, 0);
    scene.base_end();
    scene.empty_shape_record();
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("NestedVideoMaskLayer", 4, 0);

    scene.begin("CPicLayer");
    scene.base_start();
    scene.begin("CPicFrame");
    scene.base_start();
    scene.base_end();
    scene.filled_rect_shape_record(0, 0, 50, 50, 0xff9933cc);
    scene.frame_tail_source(1, 0, "", None);
    scene.base_end();
    scene.layer_tail_typed("NestedVideoMaskedChild", 0, bad_outer_mask_id);

    scene.base_end();
    scene.page_tail();

    let bmp_bytes = native_argb_bitmap(4, 4, [180, 180, 180, 200]);
    let symbols = [SymbolSpec {
        stream_name: "Symbol 1",
        item_name: "InnerBadMask",
        library_id: 30,
        symbol_type: 2,
        bytes: &inner.bytes,
    }];
    let media = [MediaEntrySpec::LosslessBitmap {
        stream_name: "Media 1",
        item_name: "MaskAlphaBmp",
        media_id: 1,
        bytes: &bmp_bytes,
    }];
    let contents = build_contents_bytes("Page 1", &symbols, &media, &[]);
    let fla = build_cfb(
        "Contents",
        &contents,
        "Page 1",
        &scene.bytes,
        &symbols,
        &media,
    );
    let (project, report) = import_fla_checked(&dir.write("folder_and_masks.fla", &fla));
    assert_valid_and_roundtrips_nflash(&project);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"fla_layer_type"), "{codes:?}");
    assert!(codes.contains(&"fla_mask_layer_approximated"), "{codes:?}");

    let root = project.symbol(&project.root).unwrap();
    let by_name = |name: &str| {
        root.timeline
            .layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("missing layer {name}"))
    };

    let childless_folder = by_name("ChildlessFolderWithArt");
    assert_eq!(childless_folder.properties.role, LayerRole::Normal);
    assert!(!childless_folder.properties.collapsed);
    assert_eq!(childless_folder.keyframes[0].elements.len(), 1);

    let parent_folder = by_name("ParentFolderWithStrayArt");
    assert_eq!(parent_folder.properties.role, LayerRole::Folder);
    assert!(parent_folder.keyframes[0].elements.is_empty());
    let folder_child = by_name("FolderChildLayer");
    assert_eq!(
        folder_child.properties.parent.as_deref(),
        Some(parent_folder.id.as_str())
    );
    assert_eq!(folder_child.keyframes[0].elements.len(), 1);

    let bmp_mask = by_name("BitmapMaskLayer");
    assert_eq!(bmp_mask.properties.role, LayerRole::Mask);
    assert_eq!(bmp_mask.properties.mask_mode, MaskMode::Alpha);
    assert!(project.format_version >= 9);

    let outer_bad_mask = by_name("NestedVideoMaskLayer");
    assert_eq!(outer_bad_mask.properties.role, LayerRole::Normal);
    assert_eq!(outer_bad_mask.properties.mask_mode, MaskMode::Geometry);
    assert!(!outer_bad_mask.properties.mask_invert);
    assert!(by_name("NestedVideoMaskedChild").properties.mask.is_none());

    let inner_sym = project
        .symbols
        .iter()
        .find(|s| s.name == "InnerBadMask")
        .unwrap();
    let inner_mask = inner_sym
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "InnerVideoMask")
        .unwrap();
    assert_eq!(inner_mask.properties.role, LayerRole::Normal);
    assert_eq!(inner_mask.properties.mask_mode, MaskMode::Geometry);
    assert!(!inner_mask.properties.mask_invert);
}
