//! End-to-end integration tests for:
//! - `imports-external-libraries`: SWC catalog parsing, `library.swf` symbol/AS3/media import,
//!   `link_swc_into_project`, and multi-SWC dependency resolution & conflict handling.
//! - `imports-swf-inspection`: SWF `PlaceObject3` filter lists, blend modes, `cache_as_bitmap`,
//!   and classic tween / easing curve recovery across multi-frame `PlaceObject2` spans.
//! - `imports-swf-playback`: Deterministic AVM1 & AVM2 SWF bytecode player (`SwfBytecodePlayer`),
//!   display list stepping, clip/button event dispatch, audio event tracking, and corpus SWF playback.
//! - `imports-xfl` & `imports-binary-*`: XFL `BitmapFill` color sampling, `reflect`/`repeat`/`linearRGB`
//!   gradient synthesis, and 3D transform (`matrix3D`/`rotationX`/`rotationY`/`rotationZ` and binary
//!   `matrix_3d`) 2D projection.
//! - `imports-corpus`: Automated structural, visual (stage evaluation across frames), and behavioral
//!   comparison across the 21 corpus cases and companion SWF/FLA/XFL/SWC fixtures.

use image::{ExtendedColorType, ImageEncoder};
use neoflash_core::{
    BlendMode, Easing, ElementKind, FilterSpec, GradientKind, TweenKind, evaluate,
};
use neoflash_import::{
    ImportFormat, SwcConflictPolicy, SwcDependencyKind, SwfBytecodePlayer, SwfVmKind, export_fla,
    import_project, import_swc, import_swc_bytes, inspect, is_flash_builtin_definition,
    link_swc_into_project, normalize_catalog_id, play_swf_bytes, play_swf_path,
    resolve_swc_dependencies,
};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::write::SimpleFileOptions;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-import-cap-corpus-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct BitWriter {
    buf: u128,
    len: u32,
}
impl BitWriter {
    fn new() -> Self {
        Self { buf: 0, len: 0 }
    }
    fn push(&mut self, val: u64, n: u32) {
        let mask = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        self.buf = (self.buf << n) | u128::from(val & mask);
        self.len += n;
    }
    fn finish(mut self) -> Vec<u8> {
        let pad = (8 - (self.len % 8)) % 8;
        if pad > 0 {
            self.push(0, pad);
        }
        let total_bytes = (self.len / 8) as usize;
        let mut out = Vec::with_capacity(total_bytes);
        for i in (0..total_bytes).rev() {
            out.push(((self.buf >> (i * 8)) & 0xff) as u8);
        }
        out
    }
}

fn swf_tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
    if payload.len() < 0x3f {
        let header = (code << 6) | (payload.len() as u16);
        out.extend_from_slice(&header.to_le_bytes());
    } else {
        let header = (code << 6) | 0x3f;
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn build_uncompressed_swf(version: u8, frame_count: u16, tags: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut bw = BitWriter::new();
    bw.push(16, 5);
    bw.push(0, 16);
    bw.push(11000, 16);
    bw.push(0, 16);
    bw.push(8000, 16);
    body.extend_from_slice(&bw.finish());
    // Frame rate 24.0 (0x1800) + frame_count:
    body.extend_from_slice(&0x1800_u16.to_le_bytes());
    body.extend_from_slice(&frame_count.to_le_bytes());
    body.extend_from_slice(tags);
    swf_tag(&mut body, 0, &[]); // End tag

    let total_len = (8 + body.len()) as u32;
    let mut swf = vec![b'F', b'W', b'S', version];
    swf.extend_from_slice(&total_len.to_le_bytes());
    swf.extend_from_slice(&body);
    swf
}

/// Encode a SWF MATRIX with pixel translation `(tx_px, ty_px)` and optional scale `(sx, sy)`.
fn swf_matrix_bytes(sx: Option<f64>, sy: Option<f64>, tx_px: f64, ty_px: f64) -> Vec<u8> {
    let mut bw = BitWriter::new();
    if let (Some(a), Some(d)) = (sx, sy) {
        bw.push(1, 1); // HasScale = 1
        bw.push(20, 5); // nScaleBits = 20
        let a_fb = (a * 65536.0).round() as i32 as i64 as u64;
        let d_fb = (d * 65536.0).round() as i32 as i64 as u64;
        bw.push(a_fb, 20);
        bw.push(d_fb, 20);
    } else {
        bw.push(0, 1); // HasScale = 0
    }
    bw.push(0, 1); // HasRotate = 0
    let tx_twips = (tx_px * 20.0).round() as i32;
    let ty_twips = (ty_px * 20.0).round() as i32;
    bw.push(16, 5); // nTranslateBits = 16
    bw.push(tx_twips as i64 as u64, 16);
    bw.push(ty_twips as i64 as u64, 16);
    bw.finish()
}

/// Minimal `DefineShape` (tag 2) for character `shape_id`: a 20x20 red square.
fn define_red_square_shape_tag(shape_id: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&shape_id.to_le_bytes());
    p.push(0x00); // RECT nbits=0
    p.push(1); // 1 fill style
    p.push(0x00); // solid fill
    p.extend_from_slice(&[255, 0, 0]);
    p.push(0); // 0 line styles
    p.push(0x10); // NumFillBits=1, NumLineBits=0

    let mut bw = BitWriter::new();
    // StyleChangeRecord: TypeFlag=0 (1), NewStyles=0, LineStyle=0, FillStyle1=1, FillStyle0=0, MoveTo=1 -> 0_00101 = 5 (6 bits)
    bw.push(0b0_00101, 6);
    bw.push(0, 5); // MoveBits = 0
    bw.push(1, 1); // FillStyle1 = 1
    // Edge 1: dx = +400
    bw.push(0b11, 2);
    bw.push(8, 4); // nbits = 10
    bw.push(0, 1); // GeneralLineFlag = 0
    bw.push(0, 1); // VertLineFlag = 0 (horizontal)
    bw.push(400u64, 10);
    // Edge 2: dy = +400
    bw.push(0b11, 2);
    bw.push(8, 4);
    bw.push(0, 1);
    bw.push(1, 1); // vertical
    bw.push(400u64, 10);
    // Edge 3: dx = -400
    bw.push(0b11, 2);
    bw.push(8, 4);
    bw.push(0, 1);
    bw.push(0, 1);
    bw.push((-400i32) as i64 as u64, 10);
    // Edge 4: dy = -400
    bw.push(0b11, 2);
    bw.push(8, 4);
    bw.push(0, 1);
    bw.push(1, 1);
    bw.push((-400i32) as i64 as u64, 10);
    // EndShapeRecord: 6 zero bits
    bw.push(0, 6);
    p.extend_from_slice(&bw.finish());
    p
}

fn build_swc_zip_bytes(
    catalog_xml: &str,
    library_swf: &[u8],
    extra_files: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("catalog.xml", opts).unwrap();
        zip.write_all(catalog_xml.as_bytes()).unwrap();
        zip.start_file("library.swf", opts).unwrap();
        zip.write_all(library_swf).unwrap();
        for (name, bytes) in extra_files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn synthetic_solid_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        rgba.extend_from_slice(&color);
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    out
}

#[test]
fn swc_external_libraries_catalog_linkage_and_dependency_resolution() {
    assert_eq!(normalize_catalog_id("com.example:Hero"), "com.example.Hero");
    assert_eq!(
        normalize_catalog_id("ui/widgets:ButtonSkin"),
        "ui.widgets.ButtonSkin"
    );
    assert!(is_flash_builtin_definition("flash.display.MovieClip"));
    assert!(is_flash_builtin_definition("Object"));
    assert!(!is_flash_builtin_definition("com.example.Hero"));
    assert_eq!(
        SwcDependencyKind::from_catalog_type("i"),
        SwcDependencyKind::Inheritance
    );

    // Build a library.swf containing a shape (id 1), a sprite (id 2), and SymbolClass (tag 76)
    // binding sprite 2 to "com.example.HeroView":
    let mut tags = Vec::new();
    // FileAttributes (tag 69): AS3 flag = 0x08
    swf_tag(&mut tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    let shape1 = define_red_square_shape_tag(1);
    swf_tag(&mut tags, 2, &shape1);
    // DefineSprite (tag 39): id=2, frame_count=1, PlaceObject2(depth=1, char=1), ShowFrame, End
    let mut sprite2 = Vec::new();
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    sprite2.extend_from_slice(&1_u16.to_le_bytes());
    let mut po2 = vec![0x06]; // HasMatrix | HasCharacter
    po2.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po2.extend_from_slice(&1_u16.to_le_bytes()); // char 1
    po2.extend_from_slice(&swf_matrix_bytes(None, None, 10.0, 20.0));
    swf_tag(&mut sprite2, 26, &po2);
    swf_tag(&mut sprite2, 1, &[]);
    swf_tag(&mut sprite2, 0, &[]);
    swf_tag(&mut tags, 39, &sprite2);

    // SymbolClass (tag 76): 1 entry: tag_id=2 -> "com.example.HeroView\0"
    let mut sym_cls = Vec::new();
    sym_cls.extend_from_slice(&1_u16.to_le_bytes());
    sym_cls.extend_from_slice(&2_u16.to_le_bytes());
    sym_cls.extend_from_slice(b"com.example.HeroView\0");
    swf_tag(&mut tags, 76, &sym_cls);
    swf_tag(&mut tags, 1, &[]); // ShowFrame

    let lib_swf = build_uncompressed_swf(10, 1, &tags);
    let icon_png = synthetic_solid_png(4, 4, [20, 180, 240, 255]);

    let catalog_a = r#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions>
    <swc version="1.2"/>
    <flex version="4.6.0" build="23201" minimumSupportedVersion="4.0.0"/>
    <compiler name="ActionScript Compiler" version="2.0.0" build="354200"/>
  </versions>
  <features>
    <feature-script-deps/>
    <feature-components/>
    <feature-files/>
  </features>
  <components>
    <component className="com.example:HeroView" name="HeroView" uri="http://example.com/ui" icon="assets/icon.png" preview="assets/preview.png" tooltip="Hero component"/>
  </components>
  <libraries>
    <library path="library.swf">
      <script name="com/example/HeroView" mod="1700000200" signatureChecksum="12345">
        <def id="com.example:HeroView"/>
        <dep id="flash.display:MovieClip" type="i"/>
        <dep id="com.core:BaseView" type="i"/>
      </script>
      <script name="com/example/HeroController" mod="1700000200">
        <def id="com.example:HeroController"/>
        <dep id="com.example:HeroView" type="s"/>
        <dep id="com.core:EventBus" type="e"/>
      </script>
      <keep-as3-metadata>
        <metadata name="Bindable"/>
        <metadata name="Event"/>
      </keep-as3-metadata>
      <digests>
        <digest type="SHA-256" signed="false" value="deadbeefcafebabe"/>
      </digests>
    </library>
  </libraries>
  <files>
    <file path="assets/icon.png" mod="1700000100"/>
  </files>
</swc>"#;

    let swc_a_bytes = build_swc_zip_bytes(catalog_a, &lib_swf, &[("assets/icon.png", &icon_png)]);
    let temp = TempDir::new();
    let swc_a_path = temp.0.join("hero_ui.swc");
    fs::write(&swc_a_path, &swc_a_bytes).unwrap();

    // Verify `import_swc` and parsed catalog metadata:
    let swc_lib_a = import_swc(&swc_a_path).unwrap();
    let parsed_cat = &swc_lib_a.catalog;
    assert_eq!(parsed_cat.versions.swc_version.as_deref(), Some("1.2"));
    assert_eq!(parsed_cat.versions.flex_version.as_deref(), Some("4.6.0"));
    assert!(parsed_cat.features.script_deps);
    assert!(parsed_cat.features.components);
    assert!(parsed_cat.features.files);
    assert_eq!(parsed_cat.components.len(), 1);
    assert_eq!(parsed_cat.components[0].class_name, "com.example.HeroView");
    assert_eq!(parsed_cat.libraries.len(), 1);
    assert_eq!(parsed_cat.libraries[0].scripts.len(), 2);
    assert_eq!(
        parsed_cat.libraries[0].keep_as3_metadata,
        vec!["Bindable", "Event"]
    );
    assert_eq!(
        parsed_cat.libraries[0].digests,
        vec![("SHA-256".to_owned(), "deadbeefcafebabe".to_owned())]
    );

    // Verify top-level `inspect` and `import_project` on `.swc`:
    let inspect_rep = inspect(&swc_a_path).unwrap();
    assert_eq!(inspect_rep.format, ImportFormat::Swc);
    assert!(inspect_rep.can_import, "{:?}", inspect_rep.diagnostics);
    let swc_info = inspect_rep.swc.as_ref().unwrap();
    assert_eq!(swc_info.script_count, 2);
    assert_eq!(
        swc_info.definitions,
        vec!["com.example.HeroController", "com.example.HeroView"]
    );
    assert_eq!(
        swc_info.unresolved_external_dependencies,
        vec!["com.core.BaseView", "com.core.EventBus"]
    );

    let proj_from_swc = import_project(&swc_a_path).unwrap();
    proj_from_swc.validate().unwrap();
    assert!(
        proj_from_swc
            .symbols
            .iter()
            .any(|s| s.class_name.as_deref() == Some("com.example.HeroView")),
        "expected HeroView symbol with class linkage in imported SWC project"
    );
    assert!(
        proj_from_swc
            .sources
            .iter()
            .any(|s| s.path.ends_with("com/example/HeroController.as")),
        "expected synthesized AS3 stub for HeroController, got: {:?}",
        proj_from_swc
            .sources
            .iter()
            .map(|s| &s.path)
            .collect::<Vec<_>>()
    );
    assert!(
        proj_from_swc
            .resources
            .iter()
            .any(|r| r.path.contains("icon.png") && r.media_type == "image/png"),
        "expected bundled assets/icon.png imported as ResourceUnit"
    );

    // Link SWC into an existing project:
    let mut host_project = neoflash_core::Project::demo();
    let link_rep = link_swc_into_project(
        &mut host_project,
        &swc_lib_a,
        SwcConflictPolicy::PreferNewer,
    )
    .unwrap();
    host_project.validate().unwrap();
    assert!(link_rep.linked_symbols >= 1);
    assert!(link_rep.linked_sources >= 2);
    assert!(link_rep.linked_resources >= 1);
    assert!(
        link_rep
            .linked_definitions
            .contains(&"com.example.HeroView".to_owned())
    );

    // Multi-SWC topological dependency sorting & conflict policies:
    // Library B defines `com.core.BaseView` (mod=100) and `com.core.EventBus` (mod=100).
    // Library C also defines `com.core.EventBus` (mod=200, newer) and `com.core.Logger`.
    let catalog_b = r#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions><swc version="1.2"/></versions>
  <features><feature-script-deps/></features>
  <libraries>
    <library path="library.swf">
      <script name="com/core/BaseView" mod="100">
        <def id="com.core:BaseView"/>
        <dep id="com.core:Logger" type="n"/>
      </script>
      <script name="com/core/EventBus" mod="100">
        <def id="com.core:EventBus"/>
      </script>
    </library>
  </libraries>
</swc>"#;
    let catalog_c = r#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions><swc version="1.2"/></versions>
  <features><feature-script-deps/></features>
  <libraries>
    <library path="library.swf">
      <script name="com/core/Logger" mod="150">
        <def id="com.core:Logger"/>
      </script>
      <script name="com/core/EventBus" mod="200">
        <def id="com.core:EventBus"/>
        <dep id="com.missing:ExternalPlugin" type="e"/>
      </script>
    </library>
  </libraries>
</swc>"#;
    let swc_lib_b =
        import_swc_bytes("core_b", &build_swc_zip_bytes(catalog_b, &lib_swf, &[])).unwrap();
    let swc_lib_c =
        import_swc_bytes("core_c", &build_swc_zip_bytes(catalog_c, &lib_swf, &[])).unwrap();

    // 1. PreferNewer: C (defines Logger, newer EventBus) -> B (depends on Logger, defines BaseView) -> A (depends on BaseView & EventBus)
    let mut proj_newer = neoflash_core::Project::demo();
    let res_newer = resolve_swc_dependencies(
        &mut proj_newer,
        &[swc_lib_a.clone(), swc_lib_b.clone(), swc_lib_c.clone()],
        SwcConflictPolicy::PreferNewer,
    )
    .unwrap();
    proj_newer.validate().unwrap();
    assert_eq!(
        res_newer.library_load_order,
        vec!["core_c", "core_b", "hero_ui"]
    );
    assert_eq!(res_newer.conflicts.len(), 1);
    assert_eq!(res_newer.conflicts[0].definition, "com.core.EventBus");
    assert_eq!(res_newer.conflicts[0].winner, "core_c");
    assert_eq!(
        res_newer.unresolved_dependencies,
        vec!["com.missing.ExternalPlugin".to_owned()]
    );

    // 2. PreferExisting: first loaded library (`core_b` when loaded before `core_c` if no dep order, or `core_c` in topo order) wins:
    let mut proj_existing = neoflash_core::Project::demo();
    let res_existing = resolve_swc_dependencies(
        &mut proj_existing,
        &[swc_lib_b.clone(), swc_lib_c.clone()],
        SwcConflictPolicy::PreferExisting,
    )
    .unwrap();
    assert_eq!(res_existing.conflicts.len(), 1);
    assert_eq!(
        res_existing.conflicts[0].winner,
        res_existing.conflicts[0].existing_library
    );

    // 3. ErrorOnConflict: returns Err(ImportError::Blocked) on duplicate definition:
    let mut proj_err = neoflash_core::Project::demo();
    assert!(
        resolve_swc_dependencies(
            &mut proj_err,
            &[swc_lib_b, swc_lib_c],
            SwcConflictPolicy::ErrorOnConflict,
        )
        .is_err()
    );
}

#[test]
fn swf_inspection_recovers_filters_blend_modes_cache_as_bitmap_and_classic_tweens() {
    let mut tags = Vec::new();
    // DefineShape 1 (red square)
    let shape1 = define_red_square_shape_tag(1);
    swf_tag(&mut tags, 2, &shape1);

    // DefineSprite 2 wrapping shape 1 so it can be placed as an instance on root:
    let mut sprite2 = Vec::new();
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    sprite2.extend_from_slice(&1_u16.to_le_bytes());
    let mut po2_inner = vec![0x06];
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sprite2, 26, &po2_inner);
    swf_tag(&mut sprite2, 1, &[]);
    swf_tag(&mut sprite2, 0, &[]);
    swf_tag(&mut tags, 39, &sprite2);

    // Frame 0:
    // - PlaceObject3 (tag 70) at depth 1: char 2, name "filtered_mc", Matrix(10, 20),
    //   HasFilterList | HasBlendMode | HasCacheAsBitmap (flags2 = 0x07),
    //   2 filters (DropShadow id=0 + Blur id=1), blend_mode = 3 (Multiply), bitmap_cache = 1
    let mut po3 = Vec::new();
    po3.push(0x26); // flags1: HasName (0x20) | HasMatrix (0x04) | HasCharacter (0x02)
    po3.push(0x07); // flags2: HasCacheAsBitmap (0x04) | HasBlendMode (0x02) | HasFilterList (0x01)
    po3.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po3.extend_from_slice(&2_u16.to_le_bytes()); // char 2
    po3.extend_from_slice(&swf_matrix_bytes(None, None, 10.0, 20.0));
    po3.extend_from_slice(b"filtered_mc\0");
    // FilterList: 2 filters
    po3.push(2);
    // Filter 0: DropShadow (id=0): RGBA(0,0,0,128), blurX=8.0, blurY=8.0, angle=0.785398rad (~45deg), distance=6.0, strength=1.0 (0x0100), flags=0x81 (inner + passes=1)
    po3.push(0);
    po3.extend_from_slice(&[0, 0, 0, 128]);
    po3.extend_from_slice(&((8.0 * 65536.0) as i32).to_le_bytes());
    po3.extend_from_slice(&((8.0 * 65536.0) as i32).to_le_bytes());
    po3.extend_from_slice(&((std::f64::consts::FRAC_PI_4 * 65536.0) as i32).to_le_bytes());
    po3.extend_from_slice(&((6.0 * 65536.0) as i32).to_le_bytes());
    po3.extend_from_slice(&0x0100_i16.to_le_bytes());
    po3.push(0x81);
    // Filter 1: Blur (id=1): blurX=4.0, blurY=6.0, flags=2<<3 (passes=2)
    po3.push(1);
    po3.extend_from_slice(&((4.0 * 65536.0) as i32).to_le_bytes());
    po3.extend_from_slice(&((6.0 * 65536.0) as i32).to_le_bytes());
    po3.push(2 << 3);
    // BlendMode = 3 (Multiply), BitmapCache = 1
    po3.push(3);
    po3.push(1);
    swf_tag(&mut tags, 70, &po3);

    // Depth 2: PlaceObject2 (tag 26) for char 2 ("tween_mc") animated over 5 frames (t = 0.0, 0.25, 0.5, 0.75, 1.0)
    // with EaseIn quadratic progress (u = t^2) from x=0 to x=160:
    // f=0: x=0, f=1: x=10, f=2: x=40, f=3: x=90, f=4: x=160
    for (frame_idx, x_pos) in [0.0, 10.0, 40.0, 90.0, 160.0].into_iter().enumerate() {
        let mut po2 = Vec::new();
        if frame_idx == 0 {
            po2.push(0x26); // HasName | HasMatrix | HasCharacter
            po2.extend_from_slice(&2_u16.to_le_bytes()); // depth 2
            po2.extend_from_slice(&2_u16.to_le_bytes()); // char 2
            po2.extend_from_slice(&swf_matrix_bytes(None, None, x_pos, 50.0));
            po2.extend_from_slice(b"tween_mc\0");
        } else {
            po2.push(0x05); // HasMatrix | Move
            po2.extend_from_slice(&2_u16.to_le_bytes()); // depth 2
            po2.extend_from_slice(&swf_matrix_bytes(None, None, x_pos, 50.0));
        }
        swf_tag(&mut tags, 26, &po2);
        swf_tag(&mut tags, 1, &[]); // ShowFrame
    }

    let swf_bytes = build_uncompressed_swf(9, 5, &tags);
    let temp = TempDir::new();
    let swf_path = temp.0.join("filters_and_tween.swf");
    fs::write(&swf_path, &swf_bytes).unwrap();

    let project = import_project(&swf_path).unwrap();
    project.validate().unwrap();

    let root = project.symbol(&project.root).unwrap();
    // Layer for depth 1 (`filtered_mc`):
    let filtered_el = root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .find(|e| e.name == "filtered_mc")
        .expect("expected filtered_mc element");
    assert_eq!(filtered_el.blend_mode, BlendMode::Multiply);
    assert!(filtered_el.cache_as_bitmap);
    assert_eq!(filtered_el.filters.len(), 2);
    assert!(matches!(
        filtered_el.filters[0],
        FilterSpec::DropShadow {
            blur_x,
            blur_y,
            distance,
            inner: true,
            ..
        } if (blur_x - 8.0).abs() < 1e-3 && (blur_y - 8.0).abs() < 1e-3 && (distance - 6.0).abs() < 1e-3
    ));
    assert!(matches!(
        filtered_el.filters[1],
        FilterSpec::Blur {
            blur_x,
            blur_y,
            quality: 2,
        } if (blur_x - 4.0).abs() < 1e-3 && (blur_y - 6.0).abs() < 1e-3
    ));

    // Layer for depth 2 (`tween_mc`): 5 frame snapshots collapsed into 2 keyframes (frame 0 -> frame 4) with Classic tween!
    let tween_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| {
            l.keyframes
                .iter()
                .any(|k| k.elements.iter().any(|e| e.name == "tween_mc"))
        })
        .expect("expected tween_mc layer");
    assert_eq!(
        tween_layer.keyframes.len(),
        2,
        "expected 5-frame classic tween span collapsed into start + end keyframes, got: {:?}",
        tween_layer
            .keyframes
            .iter()
            .map(|k| (k.frame, k.tween))
            .collect::<Vec<_>>()
    );
    assert_eq!(tween_layer.keyframes[0].frame, 0);
    assert_eq!(tween_layer.keyframes[1].frame, 4);
    assert_eq!(tween_layer.keyframes[0].tween_kind, TweenKind::Classic);
    assert!(
        matches!(
            tween_layer.keyframes[0].tween,
            Some(Easing::EaseIn | Easing::Flash { .. })
        ),
        "expected EaseIn or negative Flash acceleration on recovered tween, got: {:?}",
        tween_layer.keyframes[0].tween
    );
    // Verify evaluated midpoint at frame 2 matches x=40.0 within 1.5px:
    let mid_draw = evaluate(&project, &project.root, 2);
    assert!(
        mid_draw
            .iter()
            .any(|d| (d.transform[4] - 40.0).abs() <= 1.5 && (d.transform[5] - 50.0).abs() <= 0.5),
        "evaluated midpoint did not match x=40: {mid_draw:?}"
    );
}

#[test]
fn swf_bytecode_player_executes_avm1_avm2_button_events_and_corpus_swfs() {
    let mut tags = Vec::new();
    let shape1 = define_red_square_shape_tag(1);
    swf_tag(&mut tags, 2, &shape1);

    // DefineSprite 2 ("box"):
    let mut sprite2 = Vec::new();
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    sprite2.extend_from_slice(&1_u16.to_le_bytes());
    let mut po2_inner = vec![0x06];
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sprite2, 26, &po2_inner);
    swf_tag(&mut sprite2, 1, &[]);
    swf_tag(&mut sprite2, 0, &[]);
    swf_tag(&mut tags, 39, &sprite2);

    // DefineButton2 (tag 34) id=3 with button cond action tracing "btn_clicked":
    let mut btn3 = Vec::new();
    btn3.extend_from_slice(&3_u16.to_le_bytes());
    btn3.push(0); // flags
    let mat_id = swf_matrix_bytes(None, None, 0.0, 0.0);
    let rec_len = 1 + 2 + 2 + mat_id.len() + 1 + 1;
    let action_offset = (2 + rec_len) as u16;
    btn3.extend_from_slice(&action_offset.to_le_bytes());
    btn3.push(0x0f); // Up|Over|Down|Hit
    btn3.extend_from_slice(&1_u16.to_le_bytes());
    btn3.extend_from_slice(&1_u16.to_le_bytes());
    btn3.extend_from_slice(&mat_id);
    btn3.push(0x00); // CXFORMWITHALPHA empty
    btn3.push(0x00); // end button records
    let mut btn_act = Vec::new();
    btn_act.extend_from_slice(&0_u16.to_le_bytes());
    btn_act.extend_from_slice(&0x0008_u16.to_le_bytes());
    btn_act.push(0x96);
    let s_btn = b"\x00btn_clicked\0";
    btn_act.extend_from_slice(&(s_btn.len() as u16).to_le_bytes());
    btn_act.extend_from_slice(s_btn);
    btn_act.push(0x26);
    btn_act.push(0x00);
    btn3.extend_from_slice(&btn_act);
    swf_tag(&mut tags, 34, &btn3);

    // Place sprite 2 at depth 1 ("box") with spec-accurate ClipEvent flags for
    // Load (0x80), EnterFrame (0x40), Unload (0x20), and Press (0x2000).
    let mut po2_box = Vec::new();
    po2_box.push(0xA6); // HasClipActions (0x80) | HasName (0x20) | HasMatrix (0x04) | HasCharacter (0x02)
    po2_box.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po2_box.extend_from_slice(&2_u16.to_le_bytes()); // char 2
    po2_box.extend_from_slice(&swf_matrix_bytes(None, None, 25.0, 40.0));
    po2_box.extend_from_slice(b"box\0");
    po2_box.extend_from_slice(&0_u16.to_le_bytes());
    po2_box.extend_from_slice(&(0x80_u32 | 0x40 | 0x20 | 0x2000).to_le_bytes());
    let clip_trace_action = |message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x26);
        action.push(0x00);
        action
    };
    for (event_flag, message) in [
        (0x80_u32, "box_load"),
        (0x40_u32, "box_enter"),
        (0x20_u32, "box_unload"),
        (0x2000_u32, "box_press"),
    ] {
        let action = clip_trace_action(message);
        po2_box.extend_from_slice(&event_flag.to_le_bytes());
        po2_box.extend_from_slice(&(action.len() as u32).to_le_bytes());
        po2_box.extend_from_slice(&action);
    }
    po2_box.extend_from_slice(&0_u32.to_le_bytes());
    swf_tag(&mut tags, 26, &po2_box);

    // Place button 3 at depth 2 ("btn"):
    let mut po2_btn = Vec::new();
    po2_btn.push(0x26);
    po2_btn.extend_from_slice(&2_u16.to_le_bytes());
    po2_btn.extend_from_slice(&3_u16.to_le_bytes());
    po2_btn.extend_from_slice(&swf_matrix_bytes(None, None, 100.0, 100.0));
    po2_btn.extend_from_slice(b"btn\0");
    swf_tag(&mut tags, 26, &po2_btn);

    // StartSound (tag 15): sound_id=10, flags=0x00 (Start/Event)
    swf_tag(&mut tags, 15, &[10, 0, 0]);

    // DoAction (tag 12) on frame 0:
    // Push "box", Push 0 (_x), GetProperty (0x22), Push 5.0, Add2 (0x47), Trace (0x26), Stop (0x07)
    let mut do_act = Vec::new();
    let push_str_and_int = |buf: &mut Vec<u8>, s: &str, prop_idx: i32| {
        buf.push(0x96);
        let mut payload = Vec::new();
        payload.push(0x00);
        payload.extend_from_slice(s.as_bytes());
        payload.push(0);
        payload.push(0x07); // integer
        payload.extend_from_slice(&prop_idx.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        buf.extend_from_slice(&payload);
    };
    push_str_and_int(&mut do_act, "box", 0); // _x of "box" (25)
    do_act.push(0x22); // ActionGetProperty
    do_act.push(0x96);
    do_act.extend_from_slice(&5_u16.to_le_bytes());
    do_act.push(0x07);
    do_act.extend_from_slice(&5_i32.to_le_bytes());
    do_act.push(0x47); // ActionAdd2 -> 30
    do_act.push(0x26); // ActionTrace -> traces "30"
    do_act.push(0x07); // ActionStop
    do_act.push(0x00);
    swf_tag(&mut tags, 12, &do_act);
    swf_tag(&mut tags, 1, &[]); // ShowFrame

    // Frame 1 removes the clip, which must dispatch ClipEventUnload (0x20).
    swf_tag(&mut tags, 28, &1_u16.to_le_bytes());
    swf_tag(&mut tags, 1, &[]);

    let avm1_swf = build_uncompressed_swf(7, 2, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&avm1_swf).unwrap();
    assert_eq!(player.vm_kind(), SwfVmKind::Avm1);
    let state0 = player.step_frame();
    assert_eq!(state0.frame, 0);
    assert_eq!(state0.traces, vec!["box_load", "box_enter", "30"]);
    assert_eq!(state0.display_list.len(), 2);
    assert_eq!(state0.audio_events.len(), 1);
    assert_eq!(state0.audio_events[0].sound_id, 10);

    // Dispatch button click on placed button:
    player.trigger_pointer_click();
    assert_eq!(
        player.trace_output(),
        &[
            "box_load".to_owned(),
            "box_enter".to_owned(),
            "30".to_owned(),
            "box_press".to_owned(),
            "btn_clicked".to_owned()
        ]
    );

    let state1 = player.seek_to_frame(1);
    assert_eq!(state1.traces, vec!["box_unload"]);

    // 2. Real Ruffle AVM1 corpus SWF (`avm1-frame-labels/test.swf`):
    let corpus =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus/upstream/ruffle");
    let labels_swf = corpus.join("avm1-frame-labels/test.swf");
    let labels_expected: Vec<String> =
        fs::read_to_string(corpus.join("avm1-frame-labels/output.txt"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
    let labels_res = play_swf_path(&labels_swf, 10).unwrap();
    assert_eq!(labels_res.vm_kind, SwfVmKind::Avm1);
    assert_eq!(
        labels_res.trace_output, labels_expected,
        "AVM1 bytecode player trace output on avm1-frame-labels/test.swf must match Ruffle's output.txt!"
    );

    // 3. Synthetic AVM2 SWF with `DoABC2` executing `trace("avm2_hello")` and `trace(10 + 32)`:
    let mut avm2_tags = Vec::new();
    swf_tag(&mut avm2_tags, 69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes AS3
    let mut abc = Vec::new();
    abc.extend_from_slice(&0_u32.to_le_bytes()); // flags
    abc.push(0); // empty name
    abc.extend_from_slice(&16_u16.to_le_bytes()); // minor=16
    abc.extend_from_slice(&46_u16.to_le_bytes()); // major=46
    abc.push(1); // int_count = 1
    abc.push(1); // uint_count = 1
    abc.push(1); // double_count = 1
    abc.push(4); // string_count = 4 (idx 1 = "", idx 2 = "trace", idx 3 = "avm2_hello")
    abc.push(0); // ""
    abc.push(5);
    abc.extend_from_slice(b"trace");
    abc.push(10);
    abc.extend_from_slice(b"avm2_hello");
    abc.push(2); // namespace_count = 2
    abc.push(0x16); // CONSTANT_PackageNamespace
    abc.push(1); // name = ""
    abc.push(1); // ns_set_count = 1
    abc.push(2); // multiname_count = 2 (idx 1 = QName(ns=1, name=2 "trace"))
    abc.push(0x07); // CONSTANT_QName
    abc.push(1); // ns = 1
    abc.push(2); // name = 2 ("trace")
    abc.push(1); // method_count = 1
    abc.push(0);
    abc.push(0);
    abc.push(0);
    abc.push(0);
    abc.push(0); // metadata_count = 0
    abc.push(0); // class_count = 0
    abc.push(1); // script_count = 1 (init=0, trait_count=0)
    abc.push(0);
    abc.push(0);
    abc.push(1); // method_body_count = 1
    abc.push(0); // method 0
    abc.push(4); // max_stack
    abc.push(1); // local_count
    abc.push(0); // init_scope_depth
    abc.push(1); // max_scope_depth
    let code = [
        0x5D, 0x01, 0x2C, 0x03, 0x4F, 0x01, 0x01, 0x5D, 0x01, 0x24, 10, 0x24, 32, 0xA0, 0x4F, 0x01,
        0x01, 0x47,
    ];
    abc.push(code.len() as u8);
    abc.extend_from_slice(&code);
    abc.push(0); // exception_count = 0
    abc.push(0); // trait_count = 0
    swf_tag(&mut avm2_tags, 82, &abc);
    swf_tag(&mut avm2_tags, 1, &[]); // ShowFrame

    let avm2_swf = build_uncompressed_swf(10, 1, &avm2_tags);
    let avm2_res = play_swf_bytes(&avm2_swf, 1).unwrap();
    assert_eq!(avm2_res.vm_kind, SwfVmKind::Avm2);
    assert_eq!(avm2_res.trace_output, vec!["avm2_hello", "42"]);

    // 4. Real Ruffle AVM2 corpus SWFs execute cleanly and produce expected constructor/frame traces:
    let avm2_btn_res =
        play_swf_path(&corpus.join("avm2-button-frame-scripts/test.swf"), 2).unwrap();
    assert_eq!(avm2_btn_res.vm_kind, SwfVmKind::Avm2);
    assert!(
        avm2_btn_res
            .trace_output
            .iter()
            .any(|l| l.contains("Main framescript 1")),
        "expected 'Main framescript 1' in AVM2 playback traces, got: {:?}",
        avm2_btn_res.trace_output
    );
}

#[test]
fn xfl_bitmap_fill_sampling_gradient_spread_and_3d_transform_projection() {
    let temp = TempDir::new();
    fs::create_dir_all(temp.0.join("LIBRARY")).unwrap();

    // Write a solid bright green PNG (4x4, RGBA = [20, 220, 60, 255]):
    let green_png = synthetic_solid_png(4, 4, [20, 220, 60, 255]);
    fs::write(temp.0.join("LIBRARY/tile.png"), &green_png).unwrap();

    fs::write(
        temp.0.join("LIBRARY/Card.xml"),
        r#"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="Card">
  <timeline>
    <DOMTimeline name="Card">
      <layers>
        <DOMLayer name="Layer 1">
          <frames>
            <DOMFrame index="0">
              <elements>
                <DOMRectangleObject objectWidth="50" objectHeight="50"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"#,
    )
    .unwrap();

    fs::write(
        temp.0.join("DOMDocument.xml"),
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="400" height="300" frameRate="24">
  <media>
    <DOMBitmapItem name="tile.png" href="tile.png"/>
  </media>
  <symbols>
    <Include href="Card.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="Art">
          <frames>
            <DOMFrame index="0">
              <elements>
                <DOMShape>
                  <fills>
                    <FillStyle index="1">
                      <BitmapFill bitmapPath="tile.png"/>
                    </FillStyle>
                    <FillStyle index="2">
                      <LinearGradient spreadMethod="reflect" interpolationMethod="linearRGB">
                        <matrix><Matrix a="0.05" b="0" c="0" d="0.05" tx="50" ty="50"/></matrix>
                        <GradientEntry color="#FF0000" ratio="0"/>
                        <GradientEntry color="#0000FF" ratio="1"/>
                      </LinearGradient>
                    </FillStyle>
                  </fills>
                  <edges>
                    <Edge fillStyle1="1" edges="!0 0|400 0|400 400|0 400|0 0"/>
                    <Edge fillStyle1="2" edges="!600 0|1000 0|1000 400|600 400|600 0"/>
                  </edges>
                </DOMShape>
                <DOMSymbolInstance libraryItemName="Card" name="card3d" rotationX="60" rotationZ="30">
                  <matrix><Matrix tx="120" ty="80"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##,
    )
    .unwrap();

    let project = import_project(&temp.0).unwrap();
    project.validate().unwrap();
    let root = project.symbol(&project.root).unwrap();
    let elems = &root.timeline.layers[0].keyframes[0].elements;

    // 1. BitmapFill sampled color [20, 220, 60, 255] from LIBRARY/tile.png:
    let ElementKind::PaintedPath { fill, .. } = &elems[0].kind else {
        panic!("expected PaintedPath for BitmapFill shape");
    };
    assert_eq!(*fill, [20, 220, 60, 255]);

    // 2. LinearGradient with spreadMethod="reflect" and interpolationMethod="linearRGB" has synthesized stops:
    let ElementKind::PaintedPath {
        fill_gradient: Some(grad),
        ..
    } = &elems[1].kind
    else {
        panic!("expected PaintedPath with gradient");
    };
    assert_eq!(grad.kind, GradientKind::Linear);
    assert!(
        grad.stops.len() >= 5,
        "expected linearRGB + reflect synthesized stops, got: {:?}",
        grad.stops
    );
    assert_eq!(
        grad.stops.first().unwrap().color,
        grad.stops.last().unwrap().color
    );

    // 3. 3D instance rotationX="60" (cos(60deg)=0.5 foreshortens scale_y) and rotationZ="30":
    let card_inst = elems.iter().find(|e| e.name == "card3d").unwrap();
    assert!((card_inst.transform.rotation - 30.0).abs() < 1e-3);
    assert!((card_inst.transform.scale_y - 0.5).abs() < 1e-2);
}

#[test]
fn corpus_structural_visual_and_behavioral_comparison_harness() {
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus");
    let manifest_text = fs::read_to_string(corpus_dir.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    let cases = manifest["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 21);

    let temp = TempDir::new();

    for case in cases {
        let id = case["id"].as_str().unwrap();
        let rel_path = case["path"].as_str().unwrap();
        let source_path = corpus_dir.join(rel_path);
        let expected = &case["expected"];

        let report = inspect(&source_path).unwrap();
        assert_eq!(
            report.can_import,
            expected["can_import"].as_bool().unwrap(),
            "case {id}"
        );
        let diag_codes: BTreeSet<&str> =
            report.diagnostics.iter().map(|d| d.code.as_str()).collect();
        for req in expected["required_diagnostics"].as_array().unwrap() {
            let req_code = req.as_str().unwrap();
            assert!(
                diag_codes.contains(req_code),
                "case {id} missing required diagnostic {req_code}; got {diag_codes:?}"
            );
        }

        let project = import_project(&source_path).unwrap();
        project.validate().unwrap();

        // Check statistics match manifest expectations:
        let exp_stats = &expected["statistics"];
        assert_eq!(
            report.statistics.symbols,
            exp_stats["symbols"].as_u64().unwrap() as usize,
            "case {id} symbols"
        );
        assert_eq!(
            report.statistics.layers,
            exp_stats["layers"].as_u64().unwrap() as usize,
            "case {id} layers"
        );
        assert_eq!(
            report.statistics.keyframes,
            exp_stats["keyframes"].as_u64().unwrap() as usize,
            "case {id} keyframes"
        );
        assert_eq!(
            report.statistics.elements,
            exp_stats["elements"].as_u64().unwrap() as usize,
            "case {id} elements"
        );
        assert_eq!(
            report.statistics.scripts,
            exp_stats["scripts"].as_u64().unwrap() as usize,
            "case {id} scripts"
        );

        // Visual evaluation check across frames 0..min(frame_count, 10):
        let root_sym = project.symbol(&project.root).unwrap();
        let sample_frames = root_sym.timeline.frame_count.min(10);
        for f in 0..sample_frames {
            let draw_items = evaluate(&project, &project.root, f);
            for item in &draw_items {
                assert!(
                    item.transform.iter().all(|v| v.is_finite()),
                    "case {id} frame {f} produced non-finite draw transform"
                );
            }
        }

        // Round-trip export_fla -> import_project visual & structural comparison:
        let roundtrip_fla = temp.0.join(format!("{id}_roundtrip.fla"));
        export_fla(&project, &roundtrip_fla).unwrap();
        let reimported = import_project(&roundtrip_fla).unwrap();
        reimported.validate().unwrap();
        assert_eq!(
            (reimported.width, reimported.height, reimported.fps),
            (project.width, project.height, project.fps),
            "case {id} stage dimensions/fps roundtrip mismatch"
        );
        let draw0_orig = evaluate(&project, &project.root, 0);
        let draw0_rt = evaluate(&reimported, &reimported.root, 0);
        assert_eq!(
            draw0_orig.len(),
            draw0_rt.len(),
            "case {id} frame 0 draw item count mismatch after FLA roundtrip"
        );
        for (a, b) in draw0_orig.iter().zip(draw0_rt.iter()) {
            let kind_tag = |k: &ElementKind| match k {
                ElementKind::Rectangle { .. } => "rect",
                ElementKind::Ellipse { .. } => "ellipse",
                ElementKind::Path { .. } => "path",
                ElementKind::PaintedPath { .. } => "painted_path",
                ElementKind::TextBox { .. } | ElementKind::Text { .. } => "text",
                ElementKind::Bitmap { .. } => "bitmap",
                ElementKind::Video { .. } => "video",
                ElementKind::Instance { .. } => "instance",
            };
            // Primitive Rect/Ellipse export as DOMShape PaintedPath/Path in XFL, so vector kinds are compatible:
            let is_vec = |t: &str| matches!(t, "rect" | "ellipse" | "path" | "painted_path");
            let ta = kind_tag(&a.kind);
            let tb = kind_tag(&b.kind);
            assert!(
                ta == tb || (is_vec(ta) && is_vec(tb)),
                "case {id} draw item kind mismatch: {ta} vs {tb}"
            );
        }
    }
}

#[test]
fn t204_swf_bytecode_player_display_list_projection_seek_reset_masks_filters_and_avm1_avm2() {
    let temp = TempDir::new();

    // --- 1. Bounded AVM1 SWF fixture (3 frames, mask clip_depth, PlaceObject3 filter+blend, nested sprite, SetProperty) ---
    let mut tags = Vec::new();
    let shape1 = define_red_square_shape_tag(1);
    swf_tag(&mut tags, 2, &shape1);

    // DefineSprite 2 (2 frames: frame 0 at (0,0), frame 1 moves inner shape to (15,20)):
    let mut sprite2 = Vec::new();
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    let mut po2_inner_f0 = vec![0x06]; // HasMatrix | HasCharacter
    po2_inner_f0.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner_f0.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner_f0.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sprite2, 26, &po2_inner_f0);
    swf_tag(&mut sprite2, 1, &[]); // ShowFrame 0
    let mut po2_inner_f1 = vec![0x05]; // HasMatrix | Move
    po2_inner_f1.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner_f1.extend_from_slice(&swf_matrix_bytes(None, None, 15.0, 20.0));
    swf_tag(&mut sprite2, 26, &po2_inner_f1);
    swf_tag(&mut sprite2, 1, &[]); // ShowFrame 1
    swf_tag(&mut sprite2, 0, &[]); // End
    swf_tag(&mut tags, 39, &sprite2);

    // Frame 0: PlaceObject2 mask shape 1 at depth 1 with clip_depth = 2
    let mut po2_mask = vec![0x46]; // HasClipDepth (0x40) | HasMatrix (0x04) | HasCharacter (0x02)
    po2_mask.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po2_mask.extend_from_slice(&1_u16.to_le_bytes()); // char 1
    po2_mask.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    po2_mask.extend_from_slice(&2_u16.to_le_bytes()); // clip_depth = 2
    swf_tag(&mut tags, 26, &po2_mask);

    // Frame 0: PlaceObject3 (tag 70) sprite 2 ("hero") at depth 2 with BlurFilter and BlendMode::Multiply
    let mut po3_hero = Vec::new();
    po3_hero.push(0x26); // flags1: HasName (0x20) | HasMatrix (0x04) | HasCharacter (0x02)
    po3_hero.push(0x03); // flags2: HasBlendMode (0x02) | HasFilterList (0x01)
    po3_hero.extend_from_slice(&2_u16.to_le_bytes()); // depth 2
    po3_hero.extend_from_slice(&2_u16.to_le_bytes()); // char 2
    po3_hero.extend_from_slice(&swf_matrix_bytes(None, None, 20.0, 30.0));
    po3_hero.extend_from_slice(b"hero\0");
    // FilterList: 1 filter = BlurFilter (id = 1), blur_x = 4.0 (FIXED 16.16), blur_y = 6.0 (FIXED 16.16), flags = 1 << 3
    po3_hero.push(1); // count = 1
    po3_hero.push(1); // BlurFilter
    po3_hero.extend_from_slice(&((4i32 << 16).to_le_bytes()));
    po3_hero.extend_from_slice(&((6i32 << 16).to_le_bytes()));
    po3_hero.push(1 << 3); // passes = 1
    po3_hero.push(3); // BlendMode::Multiply
    swf_tag(&mut tags, 70, &po3_hero);

    let push_set_prop = |buf: &mut Vec<u8>, target: &str, prop_idx: i32, val: i32| {
        buf.push(0x96); // ActionPush
        let mut p = Vec::new();
        p.push(0x00);
        p.extend_from_slice(target.as_bytes());
        p.push(0);
        p.push(0x07);
        p.extend_from_slice(&prop_idx.to_le_bytes());
        p.push(0x07);
        p.extend_from_slice(&val.to_le_bytes());
        buf.extend_from_slice(&(p.len() as u16).to_le_bytes());
        buf.extend_from_slice(&p);
        buf.push(0x23); // ActionSetProperty
    };
    let push_trace = |buf: &mut Vec<u8>, msg: &str| {
        buf.push(0x96);
        let mut p = Vec::new();
        p.push(0x00);
        p.extend_from_slice(msg.as_bytes());
        p.push(0);
        buf.extend_from_slice(&(p.len() as u16).to_le_bytes());
        buf.extend_from_slice(&p);
        buf.push(0x26); // ActionTrace
    };

    // Frame 0 DoAction: hero._x = 60, hero._y = 80, hero._xscale = 150, hero._alpha = 50, trace("avm1_f0")
    let mut act_f0 = Vec::new();
    push_set_prop(&mut act_f0, "hero", 0, 60); // _x = 60
    push_set_prop(&mut act_f0, "hero", 1, 80); // _y = 80
    push_set_prop(&mut act_f0, "hero", 2, 150); // _xscale = 150
    push_set_prop(&mut act_f0, "hero", 6, 50); // _alpha = 50
    push_trace(&mut act_f0, "avm1_f0");
    act_f0.push(0x00);
    swf_tag(&mut tags, 12, &act_f0);
    swf_tag(&mut tags, 1, &[]); // ShowFrame 0

    // Frame 1 DoAction: hero._rotation = 90, hero._visible = 0, trace("avm1_f1")
    let mut act_f1 = Vec::new();
    push_set_prop(&mut act_f1, "hero", 10, 90); // _rotation = 90
    push_set_prop(&mut act_f1, "hero", 7, 0); // _visible = false
    push_trace(&mut act_f1, "avm1_f1");
    act_f1.push(0x00);
    swf_tag(&mut tags, 12, &act_f1);
    swf_tag(&mut tags, 1, &[]); // ShowFrame 1

    // Frame 2 DoAction: hero._visible = 1, hero._x = 120, hero._alpha = 100, unsupported opcode 0xFE, trace("avm1_f2")
    let mut act_f2 = Vec::new();
    push_set_prop(&mut act_f2, "hero", 7, 1); // _visible = true
    push_set_prop(&mut act_f2, "hero", 0, 120); // _x = 120
    push_set_prop(&mut act_f2, "hero", 6, 100); // _alpha = 100
    act_f2.push(0xFE); // unsupported AVM1 short opcode -> explicit diagnostic
    push_trace(&mut act_f2, "avm1_f2");
    act_f2.push(0x00);
    swf_tag(&mut tags, 12, &act_f2);
    swf_tag(&mut tags, 1, &[]); // ShowFrame 2

    let avm1_bytes = build_uncompressed_swf(8, 3, &tags);
    let avm1_path = temp.0.join("t204_avm1.swf");
    fs::write(&avm1_path, &avm1_bytes).unwrap();

    let project = import_project(&avm1_path).unwrap();
    project.validate().unwrap();
    let project_snapshot = project.clone();

    let mut player = SwfBytecodePlayer::from_bytes(&avm1_bytes).unwrap();
    let state0 = player.seek_to_frame(0);
    assert_eq!(state0.frame, 0);
    assert_eq!(state0.traces, vec!["avm1_f0"]);
    let hero0 = state0
        .display_list
        .iter()
        .find(|e| e.name == "hero")
        .unwrap();
    assert!((hero0.x - 60.0).abs() < 1e-3);
    assert!((hero0.y - 80.0).abs() < 1e-3);
    assert!((hero0.scale_x - 150.0).abs() < 1e-3);
    assert!((hero0.alpha - 50.0).abs() < 1e-3);
    assert!(hero0.visible);
    assert_eq!(hero0.blend_mode, BlendMode::Multiply);
    assert_eq!(hero0.filters.len(), 1);
    assert_eq!(hero0.current_frame, 0);
    assert_eq!(hero0.children.len(), 1);
    assert!((hero0.children[0].x - 0.0).abs() < 1e-3);

    let scene0 = player.evaluate_stage_scene(&project, &project.root, 0, true);
    assert_eq!(
        scene0.draws.len(),
        1,
        "expected 1 visible masked hero draw at frame 0"
    );
    assert!(
        !scene0.clips.is_empty(),
        "expected clip_depth mask clips at frame 0"
    );
    assert!(
        scene0.has_blend_modes(),
        "expected Multiply blend mode at frame 0"
    );
    assert!((scene0.draws[0].item.transform[4] - 60.0).abs() < 1e-3);
    assert!((scene0.draws[0].item.transform[5] - 80.0).abs() < 1e-3);
    assert!((scene0.draws[0].item.transform[0] - 1.5).abs() < 1e-3);
    assert!((scene0.draws[0].item.alpha - 0.5).abs() < 1e-3);

    // Step/seek to frame 1: hero is hidden (_visible = false), nested sprite stepped to frame 1 (15, 20)
    let state1 = player.seek_to_frame(1);
    assert_eq!(state1.frame, 1);
    let hero1 = state1
        .display_list
        .iter()
        .find(|e| e.name == "hero")
        .unwrap();
    assert!(!hero1.visible);
    assert!((hero1.rotation - 90.0).abs() < 1e-3);
    assert_eq!(hero1.current_frame, 1);
    assert!((hero1.children[0].x - 15.0).abs() < 1e-3);
    assert!((hero1.children[0].y - 20.0).abs() < 1e-3);
    let scene1 = player.evaluate_stage_scene(&project, &project.root, 1, true);
    assert!(
        scene1.draws.is_empty(),
        "expected 0 visible draws when hero._visible is false at frame 1"
    );

    // Seek to frame 2: hero is visible again at x=120 + nested sprite (frame 0 on loop = 0,0), unsupported diagnostic recorded
    let state2 = player.seek_to_frame(2);
    assert_eq!(state2.frame, 2);
    assert!(
        state2
            .unsupported_diagnostics
            .iter()
            .any(|d| d.contains("0xFE")),
        "expected explicit unsupported opcode diagnostic at frame 2, got: {:?}",
        state2.unsupported_diagnostics
    );
    let scene2 = player.evaluate_stage_scene(&project, &project.root, 2, true);
    assert_eq!(scene2.draws.len(), 1);
    assert!((scene2.draws[0].item.transform[4] - 120.0).abs() < 1e-3);
    assert!((scene2.draws[0].item.alpha - 1.0).abs() < 1e-3);

    // Determinism of seek_to_frame(0) / reset() and immutability of authored Project:
    let state0_again = player.seek_to_frame(0);
    assert_eq!(state0_again.display_list, state0.display_list);
    let scene0_again = player.evaluate_stage_scene(&project, &project.root, 0, true);
    assert_eq!(scene0_again.draws.len(), scene0.draws.len());
    assert_eq!(
        scene0_again.draws[0].item.transform,
        scene0.draws[0].item.transform
    );
    assert_eq!(
        project, project_snapshot,
        "authored Project must remain 100% untouched"
    );

    // --- 2. Bounded AVM2 SWF fixture (2 frames, DoABC2 mutating child x/y/alpha/visible) ---
    let mut avm2_tags = Vec::new();
    swf_tag(&mut avm2_tags, 69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes AS3
    swf_tag(&mut avm2_tags, 2, &shape1);
    let mut po2_box = vec![0x26]; // HasName | HasMatrix | HasCharacter
    po2_box.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po2_box.extend_from_slice(&1_u16.to_le_bytes()); // char 1
    po2_box.extend_from_slice(&swf_matrix_bytes(None, None, 10.0, 20.0));
    po2_box.extend_from_slice(b"box\0");
    swf_tag(&mut avm2_tags, 26, &po2_box);

    // DoABC2 on frame 0: findpropstrict("box"), getproperty("box"), pushbyte(95), setproperty("x"), trace("avm2_f0")
    let mut abc = Vec::new();
    abc.extend_from_slice(&0_u32.to_le_bytes());
    abc.push(0);
    abc.extend_from_slice(&16_u16.to_le_bytes());
    abc.extend_from_slice(&46_u16.to_le_bytes());
    abc.push(1); // int_count
    abc.push(1); // uint_count
    abc.push(1); // double_count
    abc.push(6); // string_count = 6 (1="", 2="trace", 3="avm2_f0", 4="box", 5="x")
    abc.push(0);
    abc.push(5);
    abc.extend_from_slice(b"trace");
    abc.push(7);
    abc.extend_from_slice(b"avm2_f0");
    abc.push(3);
    abc.extend_from_slice(b"box");
    abc.push(1);
    abc.extend_from_slice(b"x");
    abc.push(2); // namespace_count
    abc.push(0x16);
    abc.push(1);
    abc.push(1); // ns_set_count
    abc.push(4); // multiname_count = 4 (1="trace", 2="box", 3="x")
    abc.extend_from_slice(&[0x07, 1, 2]); // mn 1: trace
    abc.extend_from_slice(&[0x07, 1, 4]); // mn 2: box
    abc.extend_from_slice(&[0x07, 1, 5]); // mn 3: x
    abc.extend_from_slice(&[1, 0, 0, 0, 0]); // 1 method
    abc.push(0); // metadata
    abc.push(0); // class
    abc.extend_from_slice(&[1, 0, 0]); // 1 script
    abc.extend_from_slice(&[1, 0, 4, 1, 0, 1]); // 1 body
    let avm2_code = [
        0x5D, 0x02, // findpropstrict "box"
        0x66, 0x02, // getproperty "box" -> Value::Object("box")
        0x24, 95, // pushbyte 95
        0x61, 0x03, // setproperty "x" -> box.x = 95
        0x5D, 0x01, // findpropstrict "trace"
        0x2C, 0x03, // pushstring "avm2_f0"
        0x4F, 0x01, 0x01, // callpropvoid "trace", 1
        0x47, // returnvoid
    ];
    abc.push(avm2_code.len() as u8);
    abc.extend_from_slice(&avm2_code);
    abc.push(0);
    abc.push(0);
    swf_tag(&mut avm2_tags, 82, &abc);
    swf_tag(&mut avm2_tags, 1, &[]); // ShowFrame 0

    // Frame 1: Move box via PlaceObject2 to (140, 55)
    let mut po2_move = vec![0x05]; // HasMatrix | Move
    po2_move.extend_from_slice(&1_u16.to_le_bytes());
    po2_move.extend_from_slice(&swf_matrix_bytes(None, None, 140.0, 55.0));
    swf_tag(&mut avm2_tags, 26, &po2_move);
    swf_tag(&mut avm2_tags, 1, &[]); // ShowFrame 1

    let avm2_bytes = build_uncompressed_swf(10, 2, &avm2_tags);
    let avm2_path = temp.0.join("t204_avm2.swf");
    fs::write(&avm2_path, &avm2_bytes).unwrap();
    let avm2_project = import_project(&avm2_path).unwrap();
    let mut avm2_player = SwfBytecodePlayer::from_bytes(&avm2_bytes).unwrap();

    let avm2_s0 = avm2_player.seek_to_frame(0);
    assert_eq!(avm2_s0.traces, vec!["avm2_f0"]);
    let avm2_scene0 = avm2_player.evaluate_stage_scene(&avm2_project, &avm2_project.root, 0, true);
    assert_eq!(avm2_scene0.draws.len(), 1);
    assert!((avm2_scene0.draws[0].item.transform[4] - 95.0).abs() < 1e-3);
    assert!((avm2_scene0.draws[0].item.transform[5] - 20.0).abs() < 1e-3);

    let _avm2_s1 = avm2_player.seek_to_frame(1);
    let avm2_scene1 = avm2_player.evaluate_stage_scene(&avm2_project, &avm2_project.root, 1, true);
    assert!((avm2_scene1.draws[0].item.transform[4] - 140.0).abs() < 1e-3);
    assert!((avm2_scene1.draws[0].item.transform[5] - 55.0).abs() < 1e-3);
}

#[test]
fn t208_swf_runtime_keeps_same_sprite_instances_and_grandchildren_independent() {
    let temp = TempDir::new();
    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_red_square_shape_tag(1));

    // Inner sprite 3: the leaf shape moves from x=0 to x=30 on its second frame.
    let mut inner = Vec::new();
    inner.extend_from_slice(&3_u16.to_le_bytes());
    inner.extend_from_slice(&2_u16.to_le_bytes());
    let mut inner_place = vec![0x26]; // HasName | HasMatrix | HasCharacter
    inner_place.extend_from_slice(&1_u16.to_le_bytes());
    inner_place.extend_from_slice(&1_u16.to_le_bytes());
    inner_place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    inner_place.extend_from_slice(b"leaf\0");
    swf_tag(&mut inner, 26, &inner_place);
    swf_tag(&mut inner, 1, &[]);
    let mut inner_move = vec![0x05]; // HasMatrix | Move
    inner_move.extend_from_slice(&1_u16.to_le_bytes());
    inner_move.extend_from_slice(&swf_matrix_bytes(None, None, 30.0, 0.0));
    swf_tag(&mut inner, 26, &inner_move);
    swf_tag(&mut inner, 1, &[]);
    swf_tag(&mut inner, 0, &[]);
    swf_tag(&mut tags, 39, &inner);

    // Outer sprite 2 keeps the inner sprite alive for two frames.
    let mut outer = Vec::new();
    outer.extend_from_slice(&2_u16.to_le_bytes());
    outer.extend_from_slice(&2_u16.to_le_bytes());
    let mut outer_place = vec![0x26];
    outer_place.extend_from_slice(&1_u16.to_le_bytes());
    outer_place.extend_from_slice(&3_u16.to_le_bytes());
    outer_place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    outer_place.extend_from_slice(b"inner\0");
    swf_tag(&mut outer, 26, &outer_place);
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 0, &[]);
    swf_tag(&mut tags, 39, &outer);

    // Stagger identical outer instances by one root frame. At root frame 1 the first instance and
    // its inner child are on frame 1 while the newly placed second instance remains on frame 0.
    let place_outer = |depth: u16, name: &str, x: f64| {
        let mut place = vec![0x26];
        place.extend_from_slice(&depth.to_le_bytes());
        place.extend_from_slice(&2_u16.to_le_bytes());
        place.extend_from_slice(&swf_matrix_bytes(None, None, x, 0.0));
        place.extend_from_slice(name.as_bytes());
        place.push(0);
        place
    };
    swf_tag(&mut tags, 26, &place_outer(1, "first", 100.0));
    swf_tag(&mut tags, 1, &[]);
    swf_tag(&mut tags, 26, &place_outer(2, "second", 200.0));
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 2, &tags);
    let path = temp.0.join("t208-nested-instances.swf");
    fs::write(&path, &bytes).unwrap();
    let project = import_project(&path).unwrap();
    project.validate().unwrap();
    let authored_snapshot = project.clone();

    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let state = player.seek_to_frame(1);
    let first = state
        .display_list
        .iter()
        .find(|entry| entry.name == "first")
        .unwrap();
    let second = state
        .display_list
        .iter()
        .find(|entry| entry.name == "second")
        .unwrap();
    assert_eq!((first.current_frame, second.current_frame), (1, 0));
    assert_eq!(first.children.len(), 1);
    assert_eq!(second.children.len(), 1);
    assert_eq!(
        (
            first.children[0].current_frame,
            second.children[0].current_frame
        ),
        (1, 0)
    );
    assert_eq!(first.children[0].children.len(), 1);
    assert_eq!(second.children[0].children.len(), 1);
    assert!((first.children[0].children[0].x - 30.0).abs() < 1e-3);
    assert!((second.children[0].children[0].x - 0.0).abs() < 1e-3);

    let overlay = player.project_with_display_list_overlay(&project, &project.root, 1);
    let instance_symbol = |symbol_id: &str, frame: u32, name: &str| {
        overlay
            .symbol(symbol_id)
            .unwrap()
            .timeline
            .layers
            .iter()
            .filter_map(|layer| layer.key_at(frame))
            .flat_map(|keyframe| &keyframe.elements)
            .find_map(|element| {
                if element.name == name {
                    match &element.kind {
                        ElementKind::Instance { symbol_id, .. } => Some(symbol_id.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .unwrap()
    };
    let first_outer = instance_symbol(&project.root, 1, "first");
    let second_outer = instance_symbol(&project.root, 1, "second");
    assert_ne!(first_outer, second_outer);
    assert_ne!(first_outer, "swf-sym-2");
    assert_ne!(second_outer, "swf-sym-2");
    let first_inner = instance_symbol(&first_outer, 1, "inner");
    let second_inner = instance_symbol(&second_outer, 0, "inner");
    assert_ne!(first_inner, second_inner);
    assert_ne!(first_inner, "swf-sym-3");
    assert_ne!(second_inner, "swf-sym-3");

    let scene = player.evaluate_stage_scene(&project, &project.root, 1, true);
    let mut x_positions: Vec<f64> = scene
        .draws
        .iter()
        .map(|draw| draw.item.transform[4])
        .collect();
    x_positions.sort_by(f64::total_cmp);
    assert_eq!(x_positions.len(), 2, "unexpected scene: {scene:?}");
    assert!((x_positions[0] - 130.0).abs() < 1e-3, "{x_positions:?}");
    assert!((x_positions[1] - 200.0).abs() < 1e-3, "{x_positions:?}");
    assert_eq!(project, authored_snapshot);
}

#[test]
fn t208_nested_clip_events_and_frame_scripts_keep_instance_context() {
    let trace_action = |message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x26);
        action
    };
    let property_action = |property: i32, value: i32, message: &str, stop: bool| {
        let mut action = vec![0x96];
        let mut push = vec![0x00, 0x00]; // empty target string means the current clip
        push.push(0x07);
        push.extend_from_slice(&property.to_le_bytes());
        push.push(0x07);
        push.extend_from_slice(&value.to_le_bytes());
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x23); // ActionSetProperty
        action.extend_from_slice(&trace_action(message));
        if stop {
            action.push(0x07); // ActionStop must stop this nested instance, not the root.
        }
        action.push(0x00);
        action
    };
    let this_member_action = |member: &str, value: i32, message: &str, stop: bool| {
        let mut action = vec![0x96];
        let mut this_push = vec![0x00];
        this_push.extend_from_slice(b"this\0");
        action.extend_from_slice(&(this_push.len() as u16).to_le_bytes());
        action.extend_from_slice(&this_push);
        action.push(0x1C); // ActionGetVariable -> current MovieClip
        action.push(0x96);
        let mut value_push = vec![0x00];
        value_push.extend_from_slice(member.as_bytes());
        value_push.push(0);
        value_push.push(0x07);
        value_push.extend_from_slice(&value.to_le_bytes());
        action.extend_from_slice(&(value_push.len() as u16).to_le_bytes());
        action.extend_from_slice(&value_push);
        action.push(0x4F); // ActionSetMember
        action.extend_from_slice(&trace_action(message));
        if stop {
            action.push(0x07);
        }
        action.push(0x00);
        action
    };
    let detached_unload_action = || {
        let mut action = this_member_action("_x", 44, "nested_unload", false);
        action.pop(); // replace ActionEnd with a read-back of this._x
        action.push(0x96);
        action.extend_from_slice(&6_u16.to_le_bytes());
        action.extend_from_slice(b"\x00this\0");
        action.push(0x1C);
        action.push(0x96);
        action.extend_from_slice(&4_u16.to_le_bytes());
        action.extend_from_slice(b"\x00_x\0");
        action.push(0x4E); // ActionGetMember
        action.push(0x26);
        action.push(0x07); // must stop only the detached instance
        action.push(0x00);
        action
    };

    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_red_square_shape_tag(1));

    // Inner sprite 3 has its own frame script. Its unnamed top-level parent exercises the stable
    // `depthN/inner` target path used when a PlaceObject record has no instance name.
    let mut inner = Vec::new();
    inner.extend_from_slice(&3_u16.to_le_bytes());
    inner.extend_from_slice(&2_u16.to_le_bytes());
    let mut leaf = vec![0x06];
    leaf.extend_from_slice(&1_u16.to_le_bytes());
    leaf.extend_from_slice(&1_u16.to_le_bytes());
    leaf.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut inner, 26, &leaf);
    swf_tag(
        &mut inner,
        12,
        &this_member_action("_x", 13, "nested_frame", true),
    );
    swf_tag(&mut inner, 1, &[]);
    swf_tag(&mut inner, 1, &[]);
    swf_tag(&mut inner, 0, &[]);
    swf_tag(&mut tags, 39, &inner);

    // Outer sprite 2 places the inner sprite with Load, EnterFrame, Release and Unload handlers,
    // then removes it on its second frame.
    let mut outer = Vec::new();
    outer.extend_from_slice(&2_u16.to_le_bytes());
    outer.extend_from_slice(&2_u16.to_le_bytes());
    let mut place_inner = vec![0xA6];
    place_inner.extend_from_slice(&1_u16.to_le_bytes());
    place_inner.extend_from_slice(&3_u16.to_le_bytes());
    place_inner.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    place_inner.extend_from_slice(b"inner\0");
    place_inner.extend_from_slice(&0_u16.to_le_bytes());
    let all_flags = 0x80_u32 | 0x40 | 0x20 | 0x1000;
    place_inner.extend_from_slice(&all_flags.to_le_bytes());
    for (flag, action) in [
        (0x80_u32, property_action(1, 11, "nested_load", false)),
        (0x40_u32, property_action(6, 22, "nested_enter", false)),
        (0x1000_u32, property_action(0, 33, "nested_click", false)),
        (0x20_u32, detached_unload_action()),
    ] {
        place_inner.extend_from_slice(&flag.to_le_bytes());
        place_inner.extend_from_slice(&(action.len() as u32).to_le_bytes());
        place_inner.extend_from_slice(&action);
    }
    place_inner.extend_from_slice(&0_u32.to_le_bytes());
    swf_tag(&mut outer, 26, &place_inner);
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 28, &1_u16.to_le_bytes());
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 0, &[]);
    swf_tag(&mut tags, 39, &outer);

    let mut place_outer = vec![0x06];
    place_outer.extend_from_slice(&1_u16.to_le_bytes());
    place_outer.extend_from_slice(&2_u16.to_le_bytes());
    place_outer.extend_from_slice(&swf_matrix_bytes(None, None, 100.0, 0.0));
    swf_tag(&mut tags, 26, &place_outer);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 1, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let first = player.step_frame();
    assert!(first.playing, "nested ActionStop must not stop the root");
    let outer = &first.display_list[0];
    assert!(outer.playing, "nested ActionStop must not stop its parent");
    let inner = &outer.children[0];
    assert!((inner.x - 13.0).abs() < 1e-3);
    assert!((inner.y - 11.0).abs() < 1e-3);
    assert!((inner.alpha - 22.0).abs() < 1e-3);
    assert!(!inner.playing);
    assert_eq!(
        first.traces,
        vec!["nested_load", "nested_enter", "nested_frame"]
    );

    player.trigger_pointer_click();
    let clicked = player.display_list();
    assert!((clicked[0].children[0].x - 33.0).abs() < 1e-3);
    assert_eq!(player.trace_output().last().unwrap(), "nested_click");

    let removed = player.step_frame();
    assert!(removed.display_list[0].children.is_empty());
    assert!(removed.playing);
    assert!(removed.display_list[0].playing);
    assert_eq!(removed.traces, vec!["nested_unload", "44"]);
}

#[test]
fn t208_clip_event_identity_survives_duplicate_names_and_rename() {
    let trace_bytes = |message: &str, action: &mut Vec<u8>| {
        action.push(0x96);
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x26);
    };
    let set_numeric = |property: i32, value: i32, message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00, 0x00, 0x07];
        push.extend_from_slice(&property.to_le_bytes());
        push.push(0x07);
        push.extend_from_slice(&value.to_le_bytes());
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x23);
        trace_bytes(message, &mut action);
        action.push(0x00);
        action
    };
    let rename = |value: &str, message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00, 0x00, 0x07];
        push.extend_from_slice(&13_i32.to_le_bytes());
        push.push(0x00);
        push.extend_from_slice(value.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x23);
        trace_bytes(message, &mut action);
        action.push(0x00);
        action
    };
    let this_identity_action = || {
        let push_string = |action: &mut Vec<u8>, value: &str| {
            action.push(0x96);
            let mut payload = vec![0x00];
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
            action.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            action.extend_from_slice(&payload);
        };
        let push_int = |action: &mut Vec<u8>, value: i32| {
            action.push(0x96);
            action.extend_from_slice(&5_u16.to_le_bytes());
            action.push(0x07);
            action.extend_from_slice(&value.to_le_bytes());
        };
        let push_this = |action: &mut Vec<u8>| {
            push_string(action, "this");
            action.push(0x1C);
        };
        let set_member = |action: &mut Vec<u8>, member: &str, value: i32| {
            push_this(action);
            push_string(action, member);
            push_int(action, value);
            action.push(0x4F);
        };
        let trace_member = |action: &mut Vec<u8>, member: &str| {
            push_this(action);
            push_string(action, member);
            action.extend_from_slice(&[0x4E, 0x26]);
        };

        let mut action = Vec::new();
        set_member(&mut action, "_x", 22);
        set_member(&mut action, "custom", 77);
        push_this(&mut action);
        push_string(&mut action, "customMethod");
        let mut method_body = Vec::new();
        push_string(&mut method_body, "method_exact");
        method_body.extend_from_slice(&[0x26, 0x00]);
        let mut function_header = vec![0x00];
        function_header.extend_from_slice(&0_u16.to_le_bytes());
        function_header.extend_from_slice(&(method_body.len() as u16).to_le_bytes());
        action.push(0x9B);
        action.extend_from_slice(&(function_header.len() as u16).to_le_bytes());
        action.extend_from_slice(&function_header);
        action.extend_from_slice(&method_body);
        action.push(0x4F);
        trace_member(&mut action, "_x");
        trace_member(&mut action, "custom");
        trace_member(&mut action, "_name");
        push_int(&mut action, 0);
        push_this(&mut action);
        push_string(&mut action, "customMethod");
        action.extend_from_slice(&[0x52, 0x17]);
        push_int(&mut action, 0);
        push_this(&mut action);
        push_string(&mut action, "stop");
        action.extend_from_slice(&[0x52, 0x17, 0x00]);
        action
    };
    let place_with_handlers = |depth: u16, name: &str, handlers: Vec<(u32, Vec<u8>)>| -> Vec<u8> {
        let mut place = vec![0xA6];
        place.extend_from_slice(&depth.to_le_bytes());
        place.extend_from_slice(&2_u16.to_le_bytes());
        place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
        place.extend_from_slice(name.as_bytes());
        place.push(0);
        place.extend_from_slice(&0_u16.to_le_bytes());
        let all_flags = handlers.iter().fold(0_u32, |flags, (flag, _)| flags | flag);
        place.extend_from_slice(&all_flags.to_le_bytes());
        for (flag, action) in handlers {
            place.extend_from_slice(&flag.to_le_bytes());
            place.extend_from_slice(&(action.len() as u32).to_le_bytes());
            place.extend_from_slice(&action);
        }
        place.extend_from_slice(&0_u32.to_le_bytes());
        place
    };

    let mut tags = Vec::new();
    let mut sprite = Vec::new();
    sprite.extend_from_slice(&2_u16.to_le_bytes());
    sprite.extend_from_slice(&1_u16.to_le_bytes());
    swf_tag(&mut sprite, 1, &[]);
    swf_tag(&mut sprite, 0, &[]);
    swf_tag(&mut tags, 39, &sprite);
    swf_tag(
        &mut tags,
        26,
        &place_with_handlers(2, "depth2", vec![(0x80, this_identity_action())]),
    );
    swf_tag(
        &mut tags,
        26,
        &place_with_handlers(
            1,
            "depth2",
            vec![
                (0x80, rename("renamed", "renamed_first")),
                (0x80, set_numeric(0, 11, "queued_after_rename")),
            ],
        ),
    );
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 1, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let state = player.step_frame();
    assert_eq!(
        state.traces,
        vec![
            "22",
            "77",
            "depth2",
            "method_exact",
            "renamed_first",
            "queued_after_rename"
        ]
    );
    assert_eq!(state.display_list[0].name, "renamed");
    assert_eq!(state.display_list[1].name, "depth2");
    assert!((state.display_list[0].x - 11.0).abs() < 1e-3);
    assert!((state.display_list[1].x - 22.0).abs() < 1e-3);
    assert!(state.display_list[0].playing);
    assert!(!state.display_list[1].playing);
}

#[test]
fn t208_avm1_timeline_opcodes_keep_exact_this_identity_through_depth_name_collisions() {
    let push_string = |action: &mut Vec<u8>, value: &str| {
        action.push(0x96);
        let mut payload = vec![0x00];
        payload.extend_from_slice(value.as_bytes());
        payload.push(0);
        action.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        action.extend_from_slice(&payload);
    };
    let push_int = |action: &mut Vec<u8>, value: i32| {
        action.push(0x96);
        action.extend_from_slice(&5_u16.to_le_bytes());
        action.push(0x07);
        action.extend_from_slice(&value.to_le_bytes());
    };
    let push_this = |action: &mut Vec<u8>| {
        push_string(action, "this");
        action.push(0x1C);
    };
    let trace_this_frame = |action: &mut Vec<u8>| {
        push_this(action);
        push_int(action, 4);
        action.extend_from_slice(&[0x22, 0x26]);
    };
    let call_unqualified_goto = |name: &str, frame: i32| {
        let mut action = Vec::new();
        push_int(&mut action, frame);
        push_int(&mut action, 1);
        push_string(&mut action, name);
        action.extend_from_slice(&[0x3D, 0x17]);
        trace_this_frame(&mut action);
        action.push(0x00);
        action
    };
    let place_with_handlers = |depth: u16, handlers: Vec<Vec<u8>>| {
        let mut place = vec![0xA6];
        place.extend_from_slice(&depth.to_le_bytes());
        place.extend_from_slice(&2_u16.to_le_bytes());
        place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
        place.extend_from_slice(b"depth2\0");
        place.extend_from_slice(&0_u16.to_le_bytes());
        place.extend_from_slice(&0x80_u32.to_le_bytes());
        for action in handlers {
            place.extend_from_slice(&0x80_u32.to_le_bytes());
            place.extend_from_slice(&(action.len() as u32).to_le_bytes());
            place.extend_from_slice(&action);
        }
        place.extend_from_slice(&0_u32.to_le_bytes());
        place
    };

    let mut set_target_this_then_goto_frame = Vec::new();
    push_this(&mut set_target_this_then_goto_frame);
    set_target_this_then_goto_frame.push(0x20); // ActionSetTarget2
    set_target_this_then_goto_frame.extend_from_slice(&[0x81, 0x02, 0x00, 0x01, 0x00]);
    trace_this_frame(&mut set_target_this_then_goto_frame);
    set_target_this_then_goto_frame.push(0x00);

    let mut goto_label = vec![0x8C];
    goto_label.extend_from_slice(&6_u16.to_le_bytes());
    goto_label.extend_from_slice(b"third\0");
    trace_this_frame(&mut goto_label);
    goto_label.push(0x00);

    let handlers = vec![
        set_target_this_then_goto_frame,
        goto_label,
        call_unqualified_goto("gotoAndStop", 1),
        call_unqualified_goto("gotoAndPlay", 2),
    ];

    let mut tags = Vec::new();
    let mut sprite = Vec::new();
    sprite.extend_from_slice(&2_u16.to_le_bytes());
    sprite.extend_from_slice(&3_u16.to_le_bytes());
    for label in ["first", "second", "third"] {
        let mut payload = label.as_bytes().to_vec();
        payload.push(0);
        swf_tag(&mut sprite, 43, &payload);
        swf_tag(&mut sprite, 1, &[]);
    }
    swf_tag(&mut sprite, 0, &[]);
    swf_tag(&mut tags, 39, &sprite);

    // Both instances are authored as `depth2`. Internal runtime target `depth2` must mean the
    // actual depth-2 slot, while an authored string lookup still selects the earlier named slot.
    swf_tag(&mut tags, 26, &place_with_handlers(2, handlers));
    swf_tag(&mut tags, 26, &place_with_handlers(1, Vec::new()));
    let mut authored_set_target = Vec::new();
    push_string(&mut authored_set_target, "depth2");
    authored_set_target.extend_from_slice(&[0x20, 0x07, 0x00]);
    swf_tag(&mut tags, 12, &authored_set_target);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 1, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let state = player.step_frame();
    assert_eq!(state.traces, vec!["2", "3", "1", "2"]);
    assert_eq!(state.display_list[0].depth, 1);
    assert_eq!(state.display_list[0].current_frame, 0);
    assert!(!state.display_list[0].playing);
    assert_eq!(state.display_list[1].depth, 2);
    assert_eq!(state.display_list[1].current_frame, 1);
    assert!(state.display_list[1].playing);
}

#[test]
fn t208_swf_replacement_and_rewind_dispatch_complete_lifecycle() {
    let trace_action = |message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.extend_from_slice(&[0x26, 0x00]);
        action
    };
    let rename_and_trace = |name: &str, message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00, 0x00, 0x07];
        push.extend_from_slice(&13_i32.to_le_bytes());
        push.push(0x00);
        push.extend_from_slice(name.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x23);
        let mut trace = trace_action(message);
        trace.pop();
        action.extend_from_slice(&trace);
        action.push(0x00);
        action
    };
    let clip_records = |payload: &mut Vec<u8>, handlers: Vec<(u32, Vec<u8>)>| {
        payload.extend_from_slice(&0_u16.to_le_bytes());
        let all_flags = handlers.iter().fold(0_u32, |flags, (flag, _)| flags | flag);
        payload.extend_from_slice(&all_flags.to_le_bytes());
        for (flag, action) in handlers {
            payload.extend_from_slice(&flag.to_le_bytes());
            payload.extend_from_slice(&(action.len() as u32).to_le_bytes());
            payload.extend_from_slice(&action);
        }
        payload.extend_from_slice(&0_u32.to_le_bytes());
    };

    let mut tags = Vec::new();
    for character_id in [2_u16, 4] {
        let mut sprite = Vec::new();
        sprite.extend_from_slice(&character_id.to_le_bytes());
        sprite.extend_from_slice(&1_u16.to_le_bytes());
        swf_tag(&mut sprite, 1, &[]);
        swf_tag(&mut sprite, 0, &[]);
        swf_tag(&mut tags, 39, &sprite);
    }

    let mut old_place = vec![0xA6];
    old_place.extend_from_slice(&1_u16.to_le_bytes());
    old_place.extend_from_slice(&2_u16.to_le_bytes());
    old_place.extend_from_slice(&swf_matrix_bytes(None, None, 5.0, 0.0));
    old_place.extend_from_slice(b"old\0");
    clip_records(
        &mut old_place,
        vec![
            (0x80, rename_and_trace("mutated", "old_load")),
            (0x40, trace_action("old_enter")),
            (0x20, trace_action("old_unload")),
        ],
    );
    swf_tag(&mut tags, 26, &old_place);
    swf_tag(&mut tags, 1, &[]);

    // Move+HasCharacter is a replacement lifecycle even when the character ID is unchanged.
    let mut move_replace = vec![0x83];
    move_replace.extend_from_slice(&1_u16.to_le_bytes());
    move_replace.extend_from_slice(&2_u16.to_le_bytes());
    clip_records(
        &mut move_replace,
        vec![
            (0x80, trace_action("move_load")),
            (0x20, trace_action("move_unload")),
        ],
    );
    swf_tag(&mut tags, 26, &move_replace);
    swf_tag(&mut tags, 1, &[]);

    // A non-Move placement at an occupied depth also replaces and unloads the prior tree.
    let mut occupied_replace = vec![0x82];
    occupied_replace.extend_from_slice(&1_u16.to_le_bytes());
    occupied_replace.extend_from_slice(&4_u16.to_le_bytes());
    clip_records(
        &mut occupied_replace,
        vec![
            (0x80, trace_action("occupied_load")),
            (0x20, trace_action("occupied_unload")),
        ],
    );
    swf_tag(&mut tags, 26, &occupied_replace);
    swf_tag(&mut tags, 1, &[]);

    let goto_zero_and_stop = [0x81, 0x02, 0x00, 0x00, 0x00, 0x07, 0x00];
    swf_tag(&mut tags, 12, &goto_zero_and_stop);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 4, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let old = player.step_frame();
    assert_eq!(old.traces, vec!["old_load", "old_enter"]);
    assert_eq!(old.display_list[0].name, "mutated");

    let moved = player.step_frame();
    assert_eq!(moved.traces, vec!["old_unload", "move_load"]);
    assert_eq!(moved.display_list[0].character_id, 2);
    assert_eq!(moved.display_list[0].name, "");
    assert!((moved.display_list[0].x - 5.0).abs() < 1e-3);
    assert!(moved.playing && moved.display_list[0].playing);

    let occupied = player.step_frame();
    assert_eq!(occupied.traces, vec!["move_unload", "occupied_load"]);
    assert_eq!(occupied.display_list[0].character_id, 4);

    let rewound = player.step_frame();
    assert_eq!(rewound.frame, 0);
    assert_eq!(
        rewound.traces,
        vec!["occupied_unload", "old_load", "old_enter"]
    );
    assert_eq!(rewound.display_list[0].character_id, 2);
}

#[test]
fn t208_run_frames_keeps_advancing_playing_grandchildren() {
    let trace_action = |message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.extend_from_slice(&[0x26, 0x00]);
        action
    };
    let place_named = |depth: u16, character: u16, name: &str| {
        let mut place = vec![0x26];
        place.extend_from_slice(&depth.to_le_bytes());
        place.extend_from_slice(&character.to_le_bytes());
        place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
        place.extend_from_slice(name.as_bytes());
        place.push(0);
        place
    };

    let mut tags = Vec::new();
    let mut grandchild = Vec::new();
    grandchild.extend_from_slice(&3_u16.to_le_bytes());
    grandchild.extend_from_slice(&2_u16.to_le_bytes());
    swf_tag(&mut grandchild, 1, &[]);
    swf_tag(&mut grandchild, 12, &trace_action("grandchild_advanced"));
    swf_tag(&mut grandchild, 1, &[]);
    swf_tag(&mut grandchild, 0, &[]);
    swf_tag(&mut tags, 39, &grandchild);

    let mut parent = Vec::new();
    parent.extend_from_slice(&2_u16.to_le_bytes());
    parent.extend_from_slice(&1_u16.to_le_bytes());
    swf_tag(&mut parent, 26, &place_named(1, 3, "grandchild"));
    swf_tag(&mut parent, 12, &[0x07, 0x00]); // stop only the parent
    swf_tag(&mut parent, 1, &[]);
    swf_tag(&mut parent, 0, &[]);
    swf_tag(&mut tags, 39, &parent);

    swf_tag(&mut tags, 26, &place_named(1, 2, "parent"));
    swf_tag(&mut tags, 12, &[0x07, 0x00]); // stop the root
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 1, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let playback = player.run_frames(2);
    assert_eq!(playback.frames_executed.len(), 2);
    assert_eq!(playback.trace_output, vec!["grandchild_advanced"]);
    let nested = &playback.frames_executed[1].display_list[0].children[0];
    assert_eq!(nested.current_frame, 1);
    assert!(!playback.frames_executed[1].playing);
    assert!(!playback.frames_executed[1].display_list[0].playing);
    assert!(nested.playing);
}

#[test]
fn t208_natural_root_loop_rebuilds_display_list_once() {
    let trace_action = |message: &str| {
        let mut action = vec![0x96];
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.extend_from_slice(&[0x26, 0x00]);
        action
    };

    let mut tags = Vec::new();
    let mut sprite = Vec::new();
    sprite.extend_from_slice(&2_u16.to_le_bytes());
    sprite.extend_from_slice(&1_u16.to_le_bytes());
    swf_tag(&mut sprite, 1, &[]);
    swf_tag(&mut sprite, 0, &[]);
    swf_tag(&mut tags, 39, &sprite);

    swf_tag(&mut tags, 1, &[]); // frame 0 is intentionally empty
    let mut transient = vec![0xA6];
    transient.extend_from_slice(&1_u16.to_le_bytes());
    transient.extend_from_slice(&2_u16.to_le_bytes());
    transient.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    transient.extend_from_slice(b"transient\0");
    transient.extend_from_slice(&0_u16.to_le_bytes());
    transient.extend_from_slice(&(0x80_u32 | 0x20).to_le_bytes());
    for (flag, action) in [
        (0x80_u32, trace_action("natural_load")),
        (0x20_u32, trace_action("natural_unload")),
    ] {
        transient.extend_from_slice(&flag.to_le_bytes());
        transient.extend_from_slice(&(action.len() as u32).to_le_bytes());
        transient.extend_from_slice(&action);
    }
    transient.extend_from_slice(&0_u32.to_le_bytes());
    swf_tag(&mut tags, 26, &transient);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 2, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let first = player.step_frame();
    assert_eq!(first.frame, 0);
    assert!(first.display_list.is_empty());
    let last = player.step_frame();
    assert_eq!(last.frame, 1);
    assert_eq!(last.traces, vec!["natural_load"]);
    assert_eq!(last.display_list.len(), 1);

    let looped = player.step_frame();
    assert_eq!(looped.frame, 0);
    assert_eq!(looped.traces, vec!["natural_unload"]);
    assert!(looped.display_list.is_empty());
    assert_eq!(
        player.trace_output(),
        &["natural_load".to_owned(), "natural_unload".to_owned()]
    );
}

#[test]
fn t208_natural_loop_honors_pointer_and_unload_root_goto_overrides() {
    let root_goto = |frame: i32, playing: bool| {
        let push_int = |action: &mut Vec<u8>, value: i32| {
            action.push(0x96);
            action.extend_from_slice(&5_u16.to_le_bytes());
            action.push(0x07);
            action.extend_from_slice(&value.to_le_bytes());
        };
        let push_string = |action: &mut Vec<u8>, value: &str| {
            action.push(0x96);
            let mut payload = vec![0x00];
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
            action.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            action.extend_from_slice(&payload);
        };
        let mut action = Vec::new();
        push_int(&mut action, frame);
        push_int(&mut action, 1);
        push_string(&mut action, "_root");
        action.push(0x1C);
        push_string(
            &mut action,
            if playing {
                "gotoAndPlay"
            } else {
                "gotoAndStop"
            },
        );
        action.extend_from_slice(&[0x52, 0x17, 0x00]);
        action
    };
    let build = |event_flag: u32, playing: bool| {
        let mut tags = Vec::new();
        for character_id in [2_u16, 3] {
            let mut sprite = Vec::new();
            sprite.extend_from_slice(&character_id.to_le_bytes());
            sprite.extend_from_slice(&1_u16.to_le_bytes());
            swf_tag(&mut sprite, 1, &[]);
            swf_tag(&mut sprite, 0, &[]);
            swf_tag(&mut tags, 39, &sprite);
        }
        let mut marker = vec![0x26];
        marker.extend_from_slice(&1_u16.to_le_bytes());
        marker.extend_from_slice(&2_u16.to_le_bytes());
        marker.extend_from_slice(&swf_matrix_bytes(None, None, 10.0, 0.0));
        marker.extend_from_slice(b"marker\0");
        swf_tag(&mut tags, 26, &marker);
        swf_tag(&mut tags, 1, &[]);

        let mut move_mid = vec![0x05];
        move_mid.extend_from_slice(&1_u16.to_le_bytes());
        move_mid.extend_from_slice(&swf_matrix_bytes(None, None, 20.0, 0.0));
        swf_tag(&mut tags, 26, &move_mid);
        swf_tag(&mut tags, 1, &[]);

        let mut move_last = vec![0x05];
        move_last.extend_from_slice(&1_u16.to_le_bytes());
        move_last.extend_from_slice(&swf_matrix_bytes(None, None, 30.0, 0.0));
        swf_tag(&mut tags, 26, &move_last);
        let mut trigger = vec![0xA6];
        trigger.extend_from_slice(&2_u16.to_le_bytes());
        trigger.extend_from_slice(&3_u16.to_le_bytes());
        trigger.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
        trigger.extend_from_slice(b"trigger\0");
        trigger.extend_from_slice(&0_u16.to_le_bytes());
        trigger.extend_from_slice(&event_flag.to_le_bytes());
        let action = root_goto(2, playing);
        trigger.extend_from_slice(&event_flag.to_le_bytes());
        trigger.extend_from_slice(&(action.len() as u32).to_le_bytes());
        trigger.extend_from_slice(&action);
        trigger.extend_from_slice(&0_u32.to_le_bytes());
        swf_tag(&mut tags, 26, &trigger);
        swf_tag(&mut tags, 1, &[]);
        build_uncompressed_swf(8, 3, &tags)
    };

    for (event_flag, playing) in [(0x1000_u32, false), (0x20, true)] {
        let bytes = build(event_flag, playing);
        let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
        let _ = player.step_frame();
        let _ = player.step_frame();
        let last = player.step_frame();
        assert_eq!(last.frame, 2);
        assert_eq!(last.display_list.len(), 2);
        if event_flag == 0x1000 {
            player.trigger_pointer_click();
        }
        let redirected = player.step_frame();
        assert_eq!(redirected.frame, 1);
        assert_eq!(redirected.playing, playing);
        assert_eq!(redirected.display_list.len(), 1);
        assert_eq!(redirected.display_list[0].name, "marker");
        assert!((redirected.display_list[0].x - 20.0).abs() < 1e-3);
    }
}

#[test]
fn t208_swf_goto_presents_destination_display_list_and_keeps_diagnostics() {
    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_red_square_shape_tag(1));

    let mut place = vec![0x06]; // HasMatrix | HasCharacter
    place.extend_from_slice(&1_u16.to_le_bytes());
    place.extend_from_slice(&1_u16.to_le_bytes());
    place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut tags, 26, &place);

    let trace_action = |message: &str, action: &mut Vec<u8>| {
        action.push(0x96);
        let mut push = vec![0x00];
        push.extend_from_slice(message.as_bytes());
        push.push(0);
        action.extend_from_slice(&(push.len() as u16).to_le_bytes());
        action.extend_from_slice(&push);
        action.push(0x26);
    };
    let mut frame0_action = Vec::new();
    trace_action("before_goto", &mut frame0_action);
    frame0_action.push(0x7E); // source-frame diagnostic must survive the jump and later seeks
    frame0_action.push(0x81); // ActionGotoFrame
    frame0_action.extend_from_slice(&2_u16.to_le_bytes());
    frame0_action.extend_from_slice(&2_u16.to_le_bytes()); // zero-based frame 2
    frame0_action.push(0x00);
    swf_tag(&mut tags, 12, &frame0_action);
    swf_tag(&mut tags, 1, &[]);

    let mut move_mid = vec![0x05];
    move_mid.extend_from_slice(&1_u16.to_le_bytes());
    move_mid.extend_from_slice(&swf_matrix_bytes(None, None, 10.0, 0.0));
    swf_tag(&mut tags, 26, &move_mid);
    swf_tag(&mut tags, 1, &[]);

    let mut move_destination = vec![0x05];
    move_destination.extend_from_slice(&1_u16.to_le_bytes());
    move_destination.extend_from_slice(&swf_matrix_bytes(None, None, 50.0, 0.0));
    swf_tag(&mut tags, 26, &move_destination);
    let mut frame2_action = Vec::new();
    trace_action("after_goto", &mut frame2_action);
    frame2_action.push(0x7F); // unsupported short action: must remain in destination diagnostics
    frame2_action.push(0x07); // ActionStop
    frame2_action.push(0x00);
    swf_tag(&mut tags, 12, &frame2_action);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 3, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    let state = player.step_frame();
    assert_eq!(state.frame, 2);
    assert_eq!(player.presented_frame(), 2);
    assert_eq!(state.traces, vec!["before_goto", "after_goto"]);
    assert_eq!(state.display_list.len(), 1);
    assert!((state.display_list[0].x - 50.0).abs() < 1e-3);
    assert!(
        state
            .unsupported_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("0x7F")),
        "{:?}",
        state.unsupported_diagnostics
    );
    assert!(
        state
            .unsupported_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("0x7E"))
    );

    let seek_state = player.seek_to_frame(0);
    assert_eq!(seek_state.frame, 2);
    assert_eq!(player.presented_frame(), 2);
    assert!((seek_state.display_list[0].x - 50.0).abs() < 1e-3);
    assert!(
        seek_state
            .unsupported_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("0x7F"))
    );

    let replay_state = player.seek_to_frame(1);
    assert_eq!(replay_state.frame, 1);
    assert!((replay_state.display_list[0].x - 10.0).abs() < 1e-3);
    assert!(
        replay_state
            .unsupported_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("0x7E")),
        "{:?}",
        replay_state.unsupported_diagnostics
    );
}

#[test]
fn t230_swf_stage_overlay_evaluates_without_cloning_project_and_matches_overlay() {
    let temp = TempDir::new();
    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_red_square_shape_tag(1));

    // Inner sprite 3 steps leaf x from 0 -> 25 across two frames.
    let mut inner = Vec::new();
    inner.extend_from_slice(&3_u16.to_le_bytes());
    inner.extend_from_slice(&2_u16.to_le_bytes());
    let mut inner_place = vec![0x26];
    inner_place.extend_from_slice(&1_u16.to_le_bytes());
    inner_place.extend_from_slice(&1_u16.to_le_bytes());
    inner_place.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    inner_place.extend_from_slice(b"leaf\0");
    swf_tag(&mut inner, 26, &inner_place);
    swf_tag(&mut inner, 1, &[]);
    let mut inner_move = vec![0x05];
    inner_move.extend_from_slice(&1_u16.to_le_bytes());
    inner_move.extend_from_slice(&swf_matrix_bytes(None, None, 25.0, 10.0));
    swf_tag(&mut inner, 26, &inner_move);
    swf_tag(&mut inner, 1, &[]);
    swf_tag(&mut inner, 0, &[]);
    swf_tag(&mut tags, 39, &inner);

    // Outer sprite 2 hosts inner sprite 3 across two frames.
    let mut outer = Vec::new();
    outer.extend_from_slice(&2_u16.to_le_bytes());
    outer.extend_from_slice(&2_u16.to_le_bytes());
    let mut outer_place = vec![0x26];
    outer_place.extend_from_slice(&1_u16.to_le_bytes());
    outer_place.extend_from_slice(&3_u16.to_le_bytes());
    outer_place.extend_from_slice(&swf_matrix_bytes(None, None, 5.0, 5.0));
    outer_place.extend_from_slice(b"inner\0");
    swf_tag(&mut outer, 26, &outer_place);
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 1, &[]);
    swf_tag(&mut outer, 0, &[]);
    swf_tag(&mut tags, 39, &outer);

    // Root places a clip-depth mask at depth 1 and two staggered outer sprite instances at depths 2 and 3.
    let mut place_mask = vec![0x66]; // HasClipDepth | HasName | HasMatrix | HasCharacter
    place_mask.extend_from_slice(&1_u16.to_le_bytes());
    place_mask.extend_from_slice(&1_u16.to_le_bytes());
    place_mask.extend_from_slice(&swf_matrix_bytes(Some(8.0), Some(8.0), 0.0, 0.0));
    place_mask.extend_from_slice(b"mask\0");
    place_mask.extend_from_slice(&4_u16.to_le_bytes());
    swf_tag(&mut tags, 26, &place_mask);

    let place_outer = |depth: u16, name: &str, x: f64, y: f64| {
        let mut p = vec![0x26];
        p.extend_from_slice(&depth.to_le_bytes());
        p.extend_from_slice(&2_u16.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, x, y));
        p.extend_from_slice(name.as_bytes());
        p.push(0);
        p
    };
    swf_tag(&mut tags, 26, &place_outer(2, "alpha", 40.0, 20.0));
    swf_tag(&mut tags, 1, &[]);
    swf_tag(&mut tags, 26, &place_outer(3, "beta", 90.0, 35.0));
    swf_tag(&mut tags, 1, &[]);
    let mut move_alpha = vec![0x05];
    move_alpha.extend_from_slice(&2_u16.to_le_bytes());
    move_alpha.extend_from_slice(&swf_matrix_bytes(None, None, 65.0, 20.0));
    swf_tag(&mut tags, 26, &move_alpha);
    swf_tag(&mut tags, 1, &[]);

    let bytes = build_uncompressed_swf(8, 3, &tags);
    let path = temp.0.join("t230-zero-clone-overlay.swf");
    fs::write(&path, &bytes).unwrap();
    let mut project = import_project(&path).unwrap();

    // Attach heavy untouched payload (8 MiB embedded resource + 128 untouched library symbols
    // carrying dense vector elements) so full-project cloning per frame would copy thousands of
    // symbols/elements while symbol-override evaluation touches only the active display-list symbols.
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "heavy-bg-media",
            "HeavyMedia.bin",
            "application/octet-stream",
            &vec![0x5A_u8; 8 * 1024 * 1024],
        )
        .unwrap(),
    );
    let leaf_timeline = project.symbol("swf-sym-1").unwrap().timeline.clone();
    let mut seq = 0usize;
    for idx in 0..128 {
        let mut timeline = leaf_timeline.clone();
        timeline.remap_internal_ids(
            &mut |prefix: &str| {
                seq += 1;
                format!("untouched-{idx}-{prefix}-{seq}")
            },
            None,
            None,
        );
        project.symbols.push(neoflash_core::Symbol {
            id: format!("untouched-sym-{idx}"),
            name: format!("Untouched {idx}"),
            folder: String::new(),
            kind: neoflash_core::SymbolKind::Graphic,
            timeline,
            class_name: None,
            linkage_id: None,
        });
    }
    project.validate().unwrap();
    let heavy_resource_ptr = project.resources.last().unwrap().data_base64.as_ptr();
    let symbols_len_before = project.symbols.len();

    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    for &frame in &[0_u32, 1, 2, 0, 1] {
        let _ = player.seek_to_frame(frame);
        let overrides = player.build_display_list_symbol_overrides(&project, &project.root, frame);
        // Only root + active nested sprite instances (<= 5 symbols) are materialized, never the 128 untouched ones.
        assert!(
            overrides.len() <= 5,
            "expected minimal transient overrides at frame {frame}, got {}",
            overrides.len()
        );
        for show_masking in [true, false] {
            let fast_scene =
                player.evaluate_stage_scene(&project, &project.root, frame, show_masking);
            let reference_overlay =
                player.project_with_display_list_overlay(&project, &project.root, frame);
            let reference_scene = neoflash_core::evaluate_scene(
                &reference_overlay,
                &project.root,
                frame,
                true,
                show_masking,
            );
            assert_eq!(
                format!("{fast_scene:?}"),
                format!("{reference_scene:?}"),
                "transient override scene must match overlay scene at frame {frame} (masking={show_masking})"
            );
            let authoring_fast = neoflash_core::evaluate_scene_with_symbol_overrides(
                &project,
                &overrides,
                &project.root,
                frame,
                false,
                show_masking,
            );
            let authoring_ref = neoflash_core::evaluate_scene(
                &reference_overlay,
                &project.root,
                frame,
                false,
                show_masking,
            );
            assert_eq!(
                format!("{authoring_fast:?}"),
                format!("{authoring_ref:?}"),
                "authoring-mode override scene must match overlay scene at frame {frame}"
            );
        }
    }

    // 120 consecutive Stage evaluations must keep the authored Project & 32 MiB buffer untouched and fast.
    let start = std::time::Instant::now();
    for step in 0..120_u32 {
        let f = step % 3;
        let _ = player.seek_to_frame(f);
        let scene = player.evaluate_stage_scene(&project, &project.root, f, true);
        assert!(!scene.draws.is_empty());
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1500,
        "120 zero-clone SWF Stage evaluations took unexpectedly long: {elapsed:?}"
    );
    assert_eq!(project.symbols.len(), symbols_len_before);
    assert_eq!(
        project.resources.last().unwrap().data_base64.as_ptr(),
        heavy_resource_ptr
    );
}

#[test]
fn t241_avm1_and_avm2_decompiler_and_player_opcode_conformance() {
    let temp = TempDir::new();

    // 1. Hand-assembled AVM2 DoABC2 with negative pushshort (0x25) and comparison branches
    //    (iflt 0x15, ifle 0x16, ifgt 0x17, ifge 0x18) to verify signed pushshort decoding
    //    in player.rs and non-inverted <, <=, >, >= decompilation in avm2.rs.
    let mut abc = Vec::new();
    abc.extend_from_slice(&16_u16.to_le_bytes());
    abc.extend_from_slice(&46_u16.to_le_bytes());
    abc.push(1); // ints: 1 (empty)
    abc.push(1); // uints: 1 (empty)
    abc.push(1); // doubles: 1 (empty)
    // strings: 1="", 2="MainTimeline", 3="flash.display", 4="MovieClip", 5="trace", 6="neg-ok"
    let strings = [
        "",
        "MainTimeline",
        "flash.display",
        "MovieClip",
        "trace",
        "neg-ok",
    ];
    abc.push((strings.len() + 1) as u8);
    for s in &strings {
        abc.push(s.len() as u8);
        abc.extend_from_slice(s.as_bytes());
    }
    // namespaces: 1=(pub ""), 2=(pub "flash.display")
    abc.push(3);
    abc.extend_from_slice(&[0x16, 1, 0x16, 3]);
    abc.push(1); // ns_sets
    // multinames: 1=QName(1,"MainTimeline"), 2=QName(2,"MovieClip"), 3=QName(1,"trace")
    abc.push(4);
    abc.extend_from_slice(&[0x07, 1, 2, 0x07, 2, 4, 0x07, 1, 5]);
    // methods: 0=iinit, 1=cinit, 2=script_init
    abc.push(3);
    for _ in 0..3 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    abc.push(0); // metadata
    abc.push(1); // 1 class (MainTimeline extends flash.display.MovieClip)
    abc.extend_from_slice(&[1, 2, 0x01, 0, 0, 0]); // iinit=0, 0 traits
    abc.extend_from_slice(&[1, 0]); // cinit=1, 0 traits
    abc.push(1); // 1 script
    abc.extend_from_slice(&[2, 1, 1, 0x04, 1, 0]); // init=2, 1 class trait
    // bodies:
    abc.push(3);
    // body 0 (iinit): getlocal_0, pushscope, findpropstrict(trace), pushshort(-42), pushbyte(0),
    //                 ifge (+6 -> skip), pushstring("bad"), jump(+4), pushstring("neg-ok"), callpropvoid(trace, 1), returnvoid
    // Wait: let's test pushshort(-42) directly passed to trace(pushshort(-42)) AND iflt / ifge!
    // In u30/s30, -42 as i32 is 0xFFFFFFD6 -> 5-byte u30: [0xD6, 0xFF, 0xFF, 0xFF, 0x0F]
    let iinit_code: Vec<u8> = vec![
        0xD0, // getlocal_0
        0x30, // pushscope
        0x5D, 3, // findpropstrict trace
        0x25, 0xD6, 0xFF, 0xFF, 0xFF, 0x0F, // pushshort -42
        0x4F, 3, 1, // callpropvoid trace (1 arg)
        0x25, 0xD6, 0xFF, 0xFF, 0xFF, 0x0F, // pushshort -42
        0x24, 0, // pushbyte 0
        0x15, 1, 0, 0,    // iflt +1 (skips 1-byte returnvoid to findpropstrict trace("neg-ok"))
        0x47, // returnvoid (not taken because -42 < 0)
        0x5D, 3, // findpropstrict trace
        0x2C, 6, // pushstring "neg-ok"
        0x4F, 3, 1,    // callpropvoid trace (1 arg)
        0x47, // returnvoid
    ];
    abc.extend_from_slice(&[0, 4, 2, 1, 3, iinit_code.len() as u8]);
    abc.extend_from_slice(&iinit_code);
    abc.extend_from_slice(&[0, 0]);
    // body 1 (cinit)
    abc.extend_from_slice(&[1, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    // body 2 (script_init)
    abc.extend_from_slice(&[2, 1, 1, 1, 2, 1, 0x47, 0, 0]);

    let mut do_abc = vec![1, 0, 0, 0];
    do_abc.extend_from_slice(b"frame\0");
    do_abc.extend_from_slice(&abc);

    let mut tags = Vec::new();
    swf_tag(&mut tags, 69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes (AS3=1)
    swf_tag(&mut tags, 82, &do_abc);
    // SymbolClass: 1 entry (0 -> "MainTimeline")
    let mut sym_cls = vec![1, 0, 0, 0];
    sym_cls.extend_from_slice(b"MainTimeline\0");
    swf_tag(&mut tags, 76, &sym_cls);
    swf_tag(&mut tags, 1, &[]);

    let swf_bytes = build_uncompressed_swf(10, 1, &tags);
    let swf_path = temp.0.join("t241-avm2-pushshort-iflt.swf");
    fs::write(&swf_path, &swf_bytes).unwrap();

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();
    let _ = player.seek_to_frame(0);
    assert_eq!(
        player.trace_output(),
        &["-42".to_owned(), "neg-ok".to_owned()],
        "pushshort(-42) must decode as signed -42 and satisfy -42 < 0 in iflt"
    );

    let imported = import_project(&swf_path).unwrap();
    let all_src: String = imported
        .sources
        .iter()
        .map(|s| s.source.as_str())
        .chain(imported.symbols.iter().flat_map(|sym| {
            sym.timeline
                .scripts
                .iter()
                .map(|fs| fs.script.source.as_str())
        }))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        all_src.contains("-42") && (all_src.contains("< 0") || all_src.contains(">= 0")),
        "decompiled AVM2 source must preserve signed -42 and comparison direction: {all_src}"
    );
}
