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

    // Place sprite 2 at depth 1 ("box") with ClipEventLoad (0x01) tracing "box_load":
    let mut po2_box = Vec::new();
    po2_box.push(0xA6); // HasClipActions (0x80) | HasName (0x20) | HasMatrix (0x04) | HasCharacter (0x02)
    po2_box.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    po2_box.extend_from_slice(&2_u16.to_le_bytes()); // char 2
    po2_box.extend_from_slice(&swf_matrix_bytes(None, None, 25.0, 40.0));
    po2_box.extend_from_slice(b"box\0");
    po2_box.extend_from_slice(&0_u16.to_le_bytes());
    po2_box.extend_from_slice(&1_u32.to_le_bytes());
    po2_box.extend_from_slice(&1_u32.to_le_bytes()); // ClipEventLoad
    let mut load_act = Vec::new();
    load_act.push(0x96);
    let s_load = b"\x00box_load\0";
    load_act.extend_from_slice(&(s_load.len() as u16).to_le_bytes());
    load_act.extend_from_slice(s_load);
    load_act.push(0x26);
    load_act.push(0x00);
    po2_box.extend_from_slice(&(load_act.len() as u32).to_le_bytes());
    po2_box.extend_from_slice(&load_act);
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

    let avm1_swf = build_uncompressed_swf(7, 2, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&avm1_swf).unwrap();
    assert_eq!(player.vm_kind(), SwfVmKind::Avm1);
    let state0 = player.step_frame();
    assert_eq!(state0.frame, 0);
    assert_eq!(state0.traces, vec!["box_load", "30"]);
    assert_eq!(state0.display_list.len(), 2);
    assert_eq!(state0.audio_events.len(), 1);
    assert_eq!(state0.audio_events[0].sound_id, 10);

    // Dispatch button click on placed button:
    player.trigger_pointer_click();
    assert_eq!(
        player.trace_output(),
        &[
            "box_load".to_owned(),
            "30".to_owned(),
            "btn_clicked".to_owned()
        ]
    );

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
