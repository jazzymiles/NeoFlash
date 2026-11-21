//! Integration tests for T-418:
//! 1. Photoshop PSD (v1) & PSB (v2) editable Layer Styles (`lfx2` / `lmfx` Action
//!    Descriptors and legacy `lrFX` blocks) across raster, vector-shape (`vmsk`/`SoCo`),
//!    text (`TySh`), and group (`lsct`) layers, including `(180 - lagl)` shadow angle
//!    conversion, solid & gradient glows, bevel placements, Color Overlay (`SoFi`),
//!    CMYK/HSB/Gray colors, disabled switches, located `psd-layer-effect-approximated`
//!    warnings on unsupported effects, `<= 8` `FilterSpec` cap, and hostile/malformed
//!    descriptor bounds safety.
//! 2. Binary FLA sequential trailing-revision decoding (`CPicBitmap` rev 0/2,
//!    `CPicSymbol` rev 11/12, `CPicButton` rev 9 wide UTF-16, `CPicFrame` rev 29
//!    `poseMatrices`, `CPicTLFText`/`CPicThreeDObject`) and large-document bitmap
//!    downscaling (`fla_bitmap_downscaled`) across single-resource (16 MiB), aggregate
//!    resource (64 MiB), runtime RGBA (128 MiB), and encoded JSON (128 MiB) budgets
//!    while preserving rendered Stage dimensions and passing `Project::validate()`.

use neoflash_core::{ElementKind, FilterSpec, LayerRole};
use neoflash_import::{
    PsdImportOptions, PsdLayerMapping, import_project, import_psd_bytes, inspect,
    read_binary_timeline,
};
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
            "neoflash-t418-{}-{}",
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

// ============================================================================
// Synthetic PSD / PSB + Action Descriptor (`lfx2` / `lrFX`) Builders
// ============================================================================

fn push_desc_id(out: &mut Vec<u8>, id: &str) {
    if id.len() == 4 {
        out.extend_from_slice(&0_u32.to_be_bytes());
        out.extend_from_slice(id.as_bytes());
    } else {
        out.extend_from_slice(&(id.len() as u32).to_be_bytes());
        out.extend_from_slice(id.as_bytes());
    }
}

fn push_desc_unicode(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    out.extend_from_slice(&(units.len() as u32).to_be_bytes());
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
}

struct DescBuilder {
    class_id: String,
    items: Vec<u8>,
    count: u32,
}

impl DescBuilder {
    fn new(class_id: &str) -> Self {
        Self {
            class_id: class_id.to_owned(),
            items: Vec::new(),
            count: 0,
        }
    }

    fn bool(&mut self, key: &str, v: bool) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"bool");
        self.items.push(u8::from(v));
        self.count += 1;
        self
    }

    fn long(&mut self, key: &str, v: i32) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"long");
        self.items.extend_from_slice(&v.to_be_bytes());
        self.count += 1;
        self
    }

    fn doub(&mut self, key: &str, v: f64) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"doub");
        self.items.extend_from_slice(&v.to_bits().to_be_bytes());
        self.count += 1;
        self
    }

    fn untf(&mut self, key: &str, unit: &[u8; 4], v: f64) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"UntF");
        self.items.extend_from_slice(unit);
        self.items.extend_from_slice(&v.to_bits().to_be_bytes());
        self.count += 1;
        self
    }

    fn enumerated(&mut self, key: &str, type_id: &str, val_id: &str) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"enum");
        push_desc_id(&mut self.items, type_id);
        push_desc_id(&mut self.items, val_id);
        self.count += 1;
        self
    }

    fn text(&mut self, key: &str, s: &str) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"TEXT");
        push_desc_unicode(&mut self.items, s);
        self.count += 1;
        self
    }

    fn objc(&mut self, key: &str, child: &DescBuilder) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"Objc");
        self.items.extend_from_slice(&child.encode_body());
        self.count += 1;
        self
    }

    fn vlls_objc(&mut self, key: &str, children: &[DescBuilder]) -> &mut Self {
        push_desc_id(&mut self.items, key);
        self.items.extend_from_slice(b"VlLs");
        self.items
            .extend_from_slice(&(children.len() as u32).to_be_bytes());
        for c in children {
            self.items.extend_from_slice(b"Objc");
            self.items.extend_from_slice(&c.encode_body());
        }
        self.count += 1;
        self
    }

    fn encode_body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_desc_unicode(&mut out, "");
        push_desc_id(&mut out, &self.class_id);
        out.extend_from_slice(&self.count.to_be_bytes());
        out.extend_from_slice(&self.items);
        out
    }

    fn encode_lfx2_block(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0_u32.to_be_bytes()); // object effects version = 0
        out.extend_from_slice(&16_u32.to_be_bytes()); // descriptor version = 16
        out.extend_from_slice(&self.encode_body());
        out
    }
}

fn rgb_desc(r: f64, g: f64, b: f64) -> DescBuilder {
    let mut d = DescBuilder::new("RGBC");
    d.doub("Rd  ", r).doub("Grn ", g).doub("Bl  ", b);
    d
}

fn push_tagged_block(extra: &mut Vec<u8>, key: &[u8; 4], payload: &[u8], use_64bit_len: bool) {
    extra.extend_from_slice(b"8BIM");
    extra.extend_from_slice(key);
    if use_64bit_len {
        extra.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    } else {
        extra.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    }
    extra.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        extra.push(0);
    }
}

struct SyntheticPsdLayer {
    name: String,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    blend: [u8; 4],
    opacity: u8,
    visible: bool,
    extra_blocks: Vec<u8>,
    rgba_fill: Option<[u8; 4]>,
}

impl SyntheticPsdLayer {
    fn new(name: &str, rect: [i32; 4], fill: [u8; 4]) -> Self {
        Self {
            name: name.to_owned(),
            top: rect[0],
            left: rect[1],
            bottom: rect[2],
            right: rect[3],
            blend: *b"norm",
            opacity: 255,
            visible: true,
            extra_blocks: Vec::new(),
            rgba_fill: Some(fill),
        }
    }
}

fn build_psd_or_psb(
    version: u16,
    width: u32,
    height: u32,
    image_resources: &[u8],
    layers: &[SyntheticPsdLayer],
) -> Vec<u8> {
    let mut out = Vec::new();
    // 1. File Header (26 bytes)
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&4_u16.to_be_bytes()); // RGBA channels
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&8_u16.to_be_bytes()); // 8-bit depth
    out.extend_from_slice(&3_u16.to_be_bytes()); // RGB color mode

    // 2. Color Mode Data (0)
    out.extend_from_slice(&0_u32.to_be_bytes());

    // 3. Image Resources
    out.extend_from_slice(&(image_resources.len() as u32).to_be_bytes());
    out.extend_from_slice(image_resources);

    // 4. Layer and Mask Information
    let mut layer_records = Vec::new();
    let mut channel_blobs = Vec::new();
    layer_records.extend_from_slice(&(layers.len() as i16).to_be_bytes());

    for l in layers {
        let lw = (l.right - l.left).max(0) as usize;
        let lh = (l.bottom - l.top).max(0) as usize;
        let plane_len = 2 + lw * lh; // 2-byte compression=0 + raw plane

        layer_records.extend_from_slice(&l.top.to_be_bytes());
        layer_records.extend_from_slice(&l.left.to_be_bytes());
        layer_records.extend_from_slice(&l.bottom.to_be_bytes());
        layer_records.extend_from_slice(&l.right.to_be_bytes());
        if let Some(fill) = l.rgba_fill {
            layer_records.extend_from_slice(&4_u16.to_be_bytes()); // channels: -1, 0, 1, 2
            for (ch_id, fill_byte) in [
                (-1_i16, fill[3]),
                (0_i16, fill[0]),
                (1_i16, fill[1]),
                (2_i16, fill[2]),
            ] {
                layer_records.extend_from_slice(&ch_id.to_be_bytes());
                if version == 2 {
                    layer_records.extend_from_slice(&(plane_len as u64).to_be_bytes());
                } else {
                    layer_records.extend_from_slice(&(plane_len as u32).to_be_bytes());
                }
                channel_blobs.extend_from_slice(&0_u16.to_be_bytes()); // raw compression
                channel_blobs.extend(std::iter::repeat_n(fill_byte, lw * lh));
            }
        } else {
            layer_records.extend_from_slice(&0_u16.to_be_bytes());
        }
        layer_records.extend_from_slice(b"8BIM");
        layer_records.extend_from_slice(&l.blend);
        layer_records.push(l.opacity);
        layer_records.push(0); // clipping = base
        layer_records.push(if l.visible { 0 } else { 0x02 }); // bit 1 = invisible
        layer_records.push(0);

        let mut extra = Vec::new();
        extra.extend_from_slice(&0_u32.to_be_bytes()); // mask data len
        extra.extend_from_slice(&0_u32.to_be_bytes()); // blending ranges len
        // Pascal string padded to 4 bytes
        let name_bytes = l.name.as_bytes();
        let nlen = name_bytes.len().min(255);
        extra.push(nlen as u8);
        extra.extend_from_slice(&name_bytes[..nlen]);
        while !(1 + nlen + (extra.len() - 8 - 1 - nlen)).is_multiple_of(4) {
            extra.push(0);
        }
        extra.extend_from_slice(&l.extra_blocks);

        layer_records.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        layer_records.extend_from_slice(&extra);
    }

    let mut layer_info = Vec::new();
    layer_info.extend_from_slice(&layer_records);
    layer_info.extend_from_slice(&channel_blobs);
    if !layer_info.len().is_multiple_of(2) {
        layer_info.push(0);
    }

    let mut layer_and_mask = Vec::new();
    if version == 2 {
        layer_and_mask.extend_from_slice(&(layer_info.len() as u64).to_be_bytes());
    } else {
        layer_and_mask.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
    }
    layer_and_mask.extend_from_slice(&layer_info);
    layer_and_mask.extend_from_slice(&0_u32.to_be_bytes()); // global layer mask len = 0

    if version == 2 {
        out.extend_from_slice(&(layer_and_mask.len() as u64).to_be_bytes());
    } else {
        out.extend_from_slice(&(layer_and_mask.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(&layer_and_mask);

    // 5. Merged Composite Image Data (raw compression = 0)
    out.extend_from_slice(&0_u16.to_be_bytes());
    let total_px = (width as usize) * (height as usize);
    out.extend(std::iter::repeat_n(200_u8, total_px * 4));
    out
}

fn build_global_angle_image_resource(angle: i32) -> Vec<u8> {
    let mut res = Vec::new();
    res.extend_from_slice(b"8BIM");
    res.extend_from_slice(&1037_u16.to_be_bytes());
    res.extend_from_slice(&0_u16.to_be_bytes()); // empty Pascal name (2 bytes)
    res.extend_from_slice(&4_u32.to_be_bytes());
    res.extend_from_slice(&angle.to_be_bytes());
    res
}

fn build_minimal_tysh_payload(text: &str) -> Vec<u8> {
    let mut text_desc = DescBuilder::new("TxLr");
    text_desc.text("Txt ", text);
    let mut b = Vec::new();
    b.extend_from_slice(&1_u16.to_be_bytes()); // TySh version 1
    for m in [1.0_f64, 0.0, 0.0, 1.0, 12.0, 24.0] {
        b.extend_from_slice(&m.to_bits().to_be_bytes());
    }
    b.extend_from_slice(&50_u16.to_be_bytes()); // text version
    b.extend_from_slice(&16_u32.to_be_bytes()); // descriptor version 16
    b.extend_from_slice(&text_desc.encode_body());
    b
}

#[test]
fn psd_lfx2_drop_inner_shadow_glow_gradient_bevel_and_color_overlay_on_all_layer_types() {
    // 1. Raster layer with DrSh (local angle 120 -> 60 deg, Scl 200% -> distance 20) + IrSh (global angle 30 -> 150 deg, inner: true)
    let mut raster = SyntheticPsdLayer::new("HeroRaster", [10, 20, 50, 60], [240, 80, 40, 255]);
    let mut drsh = DescBuilder::new("DrSh");
    drsh.bool("enab", true)
        .bool("uglg", false)
        .untf("lagl", b"#Ang", 120.0)
        .untf("Dstn", b"#Pxl", 10.0)
        .untf("blur", b"#Pxl", 8.0)
        .untf("Ckmt", b"#Prc", 50.0)
        .untf("Opct", b"#Prc", 75.0)
        .bool("knko", true)
        .objc("Clr ", &rgb_desc(17.0, 34.0, 51.0));
    let mut irsh = DescBuilder::new("IrSh");
    irsh.bool("enab", true)
        .bool("uglg", true)
        .untf("Dstn", b"#Pxl", 4.0)
        .untf("blur", b"#Pxl", 6.0)
        .untf("Opct", b"#Prc", 80.0)
        .objc("Clr ", &rgb_desc(0.0, 0.0, 0.0));
    let mut raster_fx = DescBuilder::new("Lefx");
    raster_fx
        .bool("masterFXSwitch", true)
        .untf("Scl ", b"#Prc", 200.0)
        .untf("lagl", b"#Ang", 30.0)
        .objc("DrSh", &drsh)
        .objc("IrSh", &irsh);
    push_tagged_block(
        &mut raster.extra_blocks,
        b"lfx2",
        &raster_fx.encode_lfx2_block(),
        false,
    );

    // 2. Vector-shape layer (`vmsk` + `SoCo` with 0 raster channels) with Gradient Outer Glow (`Grad`) + Solid Inner Glow
    let mut vector_layer =
        SyntheticPsdLayer::new("VectorBadge", [5, 5, 45, 45], [30, 160, 220, 255]);
    vector_layer.rgba_fill = None;
    let mut soco_desc = DescBuilder::new("null");
    soco_desc.objc("Clr ", &rgb_desc(30.0, 160.0, 220.0));
    let mut soco_bytes = 16_u32.to_be_bytes().to_vec();
    soco_bytes.extend_from_slice(&soco_desc.encode_body());
    push_tagged_block(&mut vector_layer.extra_blocks, b"SoCo", &soco_bytes, false);
    push_tagged_block(&mut vector_layer.extra_blocks, b"vmsk", &[0u8; 8], false);

    let mut stop0 = DescBuilder::new("Clrt");
    stop0
        .long("Lctn", 0)
        .objc("Clr ", &rgb_desc(255.0, 220.0, 0.0));
    let mut stop1 = DescBuilder::new("Clrt");
    stop1
        .long("Lctn", 4096)
        .objc("Clr ", &rgb_desc(255.0, 40.0, 120.0));
    let mut trns0 = DescBuilder::new("TrnS");
    trns0.long("Lctn", 0).untf("Opct", b"#Prc", 100.0);
    let mut trns1 = DescBuilder::new("TrnS");
    trns1.long("Lctn", 4096).untf("Opct", b"#Prc", 25.0);
    let mut grad = DescBuilder::new("Grdn");
    grad.vlls_objc("Clrs", &[stop0, stop1])
        .vlls_objc("Trns", &[trns0, trns1]);

    let mut orgl = DescBuilder::new("OrGl");
    orgl.bool("enab", true)
        .untf("blur", b"#Pxl", 12.0)
        .untf("Ckmt", b"#Prc", 20.0)
        .untf("Opct", b"#Prc", 90.0)
        .objc("Grad", &grad);
    let mut irgl = DescBuilder::new("IrGl");
    irgl.bool("enab", true)
        .untf("blur", b"#Pxl", 5.0)
        .untf("Opct", b"#Prc", 60.0)
        .objc("Clr ", &rgb_desc(255.0, 255.0, 255.0));
    let mut vec_fx = DescBuilder::new("Lefx");
    vec_fx
        .bool("masterFXSwitch", true)
        .objc("OrGl", &orgl)
        .objc("IrGl", &irgl);
    push_tagged_block(
        &mut vector_layer.extra_blocks,
        b"lfx2",
        &vec_fx.encode_lfx2_block(),
        false,
    );

    // 3. Text layer (`TySh`) with Inner Bevel (`InrB`) + Normal Color Overlay (`SoFi`)
    let mut text_layer =
        SyntheticPsdLayer::new("Headline", [12, 24, 44, 140], [255, 255, 255, 255]);
    push_tagged_block(
        &mut text_layer.extra_blocks,
        b"TySh",
        &build_minimal_tysh_payload("Styled Title"),
        false,
    );
    let mut ebbl = DescBuilder::new("ebbl");
    ebbl.bool("enab", true)
        .enumerated("bvlS", "BESl", "InrB")
        .bool("uglg", false)
        .untf("lagl", b"#Ang", 135.0)
        .untf("blur", b"#Pxl", 7.0)
        .untf("sftn", b"#Pxl", 2.0)
        .untf("srfs", b"#Prc", 150.0)
        .untf("hglO", b"#Prc", 80.0)
        .objc("hglC", &rgb_desc(255.0, 255.0, 255.0))
        .untf("sdwO", b"#Prc", 70.0)
        .objc("sdwC", &rgb_desc(16.0, 32.0, 64.0));
    let mut sofi = DescBuilder::new("SoFi");
    sofi.bool("enab", true)
        .enumerated("Md  ", "BlnM", "Nrml")
        .untf("Opct", b"#Prc", 50.0)
        .objc("Clr ", &rgb_desc(255.0, 0.0, 128.0));
    let mut text_fx = DescBuilder::new("Lefx");
    text_fx
        .bool("masterFXSwitch", true)
        .objc("ebbl", &ebbl)
        .objc("SoFi", &sofi);
    push_tagged_block(
        &mut text_layer.extra_blocks,
        b"lfx2",
        &text_fx.encode_lfx2_block(),
        false,
    );

    // 4. Folder group (`lsct` = 3 end, child raster, `lsct` = 1 open folder with Outer/Full Bevel + SoFi)
    let mut folder_end = SyntheticPsdLayer::new("</Layer group>", [0, 0, 0, 0], [0, 0, 0, 0]);
    push_tagged_block(
        &mut folder_end.extra_blocks,
        b"lsct",
        &3_u32.to_be_bytes(),
        false,
    );
    let folder_child = SyntheticPsdLayer::new("FolderChild", [0, 0, 16, 16], [100, 200, 100, 255]);
    let mut folder_open = SyntheticPsdLayer::new("StyledGroup", [0, 0, 0, 0], [0, 0, 0, 0]);
    push_tagged_block(
        &mut folder_open.extra_blocks,
        b"lsct",
        &1_u32.to_be_bytes(),
        false,
    );
    let mut bvl_outer = DescBuilder::new("ebbl");
    bvl_outer
        .bool("enab", true)
        .enumerated("bvlS", "BESl", "OtrB")
        .untf("blur", b"#Pxl", 4.0);
    let mut bvl_full = DescBuilder::new("ebbl");
    bvl_full
        .bool("enab", true)
        .enumerated("bvlS", "BESl", "Embs")
        .untf("blur", b"#Pxl", 6.0);
    let mut grp_fx = DescBuilder::new("Lefx");
    grp_fx
        .bool("masterFXSwitch", true)
        .vlls_objc("bevelEmbossMulti", &[bvl_outer, bvl_full])
        .objc("SoFi", &sofi);
    push_tagged_block(
        &mut folder_open.extra_blocks,
        b"lfx2",
        &grp_fx.encode_lfx2_block(),
        false,
    );

    let psd_bytes = build_psd_or_psb(
        1,
        160,
        120,
        &build_global_angle_resource_135(),
        &[
            raster,
            vector_layer,
            text_layer,
            folder_end,
            folder_child,
            folder_open,
        ],
    );

    // Verify TimelineLayers mode
    let (project, report) = import_psd_bytes(
        &psd_bytes,
        "styles_all_layers",
        &PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            ..PsdImportOptions::default()
        },
    )
    .expect("import PSD with lfx2 styles");
    project.validate().expect("validated PSD project");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "psd-layer-effect-approximated"),
        "all effects here are natively reconstructable: {:?}",
        report.diagnostics
    );

    let root = project.symbol(&project.root).unwrap();

    // Check Raster layer DrSh + IrSh:
    let hero_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "HeroRaster")
        .unwrap();
    let hero_el = &hero_layer.keyframes[0].elements[0];
    assert_eq!(hero_el.filters.len(), 2);
    match &hero_el.filters[0] {
        FilterSpec::DropShadow {
            distance,
            angle,
            color,
            alpha,
            knockout,
            inner,
            ..
        } => {
            assert!(
                (angle - 60.0).abs() < 1e-6,
                "expected 180 - 120 = 60 deg, got {angle}"
            );
            assert!(
                (distance - 20.0).abs() < 1e-6,
                "expected 10 * 200% = 20 px, got {distance}"
            );
            assert_eq!(*color, 0x112233);
            assert!((alpha - 0.75).abs() < 1e-6);
            assert!(*knockout);
            assert!(!*inner);
        }
        other => panic!("expected DropShadow, got {other:?}"),
    }
    match &hero_el.filters[1] {
        FilterSpec::DropShadow {
            distance,
            angle,
            inner,
            ..
        } => {
            assert!(
                (angle - 150.0).abs() < 1e-6,
                "expected 180 - 30 = 150 deg, got {angle}"
            );
            assert!(
                (distance - 8.0).abs() < 1e-6,
                "expected 4 * 200% = 8 px, got {distance}"
            );
            assert!(*inner, "IrSh must map to DropShadow with inner: true");
        }
        other => panic!("expected inner DropShadow, got {other:?}"),
    }

    // Check VectorBadge layer GradientGlow + Inner Glow:
    let vec_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "VectorBadge")
        .unwrap();
    let vec_el = &vec_layer.keyframes[0].elements[0];
    assert!(matches!(vec_el.kind, ElementKind::Rectangle { .. }));
    assert_eq!(vec_el.filters.len(), 2);
    assert!(matches!(
        &vec_el.filters[0],
        FilterSpec::GradientGlow { placement, colors, ratios, .. }
            if placement == "outer" && colors.len() == 2 && ratios == &[0, 255]
    ));
    assert!(matches!(
        &vec_el.filters[1],
        FilterSpec::Glow {
            inner: true,
            color: 0xffffff,
            ..
        }
    ));

    // Check Headline TextBox layer Inner Bevel + SoFi ColorTransform tint:
    let txt_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Headline")
        .unwrap();
    let txt_el = &txt_layer.keyframes[0].elements[0];
    assert!(matches!(txt_el.kind, ElementKind::Text { .. }));
    assert!(matches!(
        &txt_el.filters[0],
        FilterSpec::Bevel { placement, angle, .. }
            if placement == "inner" && (angle - 45.0).abs() < 1e-6
    ));
    let ct = txt_el.transform.color_transform;
    assert!((ct.multipliers[0] - 0.5).abs() < 1e-6);
    assert!((ct.offsets[0] - 127.5).abs() < 1e-6);
    assert!((ct.offsets[2] - 64.0).abs() < 1e-6);

    // Check StyledGroup Folder layer filters (`outer`, `full`, and `ColorMatrix` for `SoFi`):
    let grp_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "StyledGroup")
        .unwrap();
    assert_eq!(grp_layer.properties.role, LayerRole::Folder);
    assert_eq!(grp_layer.properties.filters.len(), 3);
    assert!(matches!(
        &grp_layer.properties.filters[0],
        FilterSpec::Bevel { placement, .. } if placement == "outer"
    ));
    assert!(matches!(
        &grp_layer.properties.filters[1],
        FilterSpec::Bevel { placement, .. } if placement == "full"
    ));
    assert!(matches!(
        &grp_layer.properties.filters[2],
        FilterSpec::ColorMatrix { .. }
    ));

    // Verify MovieClipSymbols, Keyframes, and Automatic modes also preserve filters and pass validate():
    for mode in [
        PsdLayerMapping::Automatic,
        PsdLayerMapping::MovieClipSymbols,
        PsdLayerMapping::Keyframes,
    ] {
        let (p, _) = import_psd_bytes(
            &psd_bytes,
            "styles_modes",
            &PsdImportOptions {
                layer_mapping: mode,
                ..PsdImportOptions::default()
            },
        )
        .unwrap();
        p.validate().unwrap();
    }
}

fn build_global_angle_resource_135() -> Vec<u8> {
    build_global_angle_image_resource(135)
}

#[test]
fn psb_v2_color_spaces_disabled_switches_unsupported_effects_and_eight_filter_cap() {
    // Layer 1: CMYK + HSB + Grayscale colors in PSB v2 + 64-bit LMsk block
    let mut color_layer =
        SyntheticPsdLayer::new("ColorSpaces", [0, 0, 24, 24], [120, 90, 200, 255]);
    push_tagged_block(&mut color_layer.extra_blocks, b"LMsk", &[0u8; 12], true);

    let mut cmyk = DescBuilder::new("CMYC");
    cmyk.doub("Cyn ", 100.0)
        .doub("Mgnt", 0.0)
        .doub("Ylw ", 0.0)
        .doub("Blck", 0.0);
    let mut hsbc = DescBuilder::new("HSBC");
    hsbc.untf("H   ", b"#Ang", 120.0)
        .doub("Strt", 100.0)
        .doub("Brgh", 100.0);
    let mut gray = DescBuilder::new("Grsc");
    gray.doub("Gry ", 25.0); // 25% ink -> ~75% brightness (~191)

    let mut fx_colors = DescBuilder::new("Lefx");
    let mut d_cmyk = DescBuilder::new("DrSh");
    d_cmyk.bool("enab", true).objc("Clr ", &cmyk);
    let mut g_hsb = DescBuilder::new("OrGl");
    g_hsb.bool("enab", true).objc("Clr ", &hsbc);
    let mut g_gray = DescBuilder::new("IrGl");
    g_gray.bool("enab", true).objc("Clr ", &gray);
    let mut d_disabled = DescBuilder::new("IrSh");
    d_disabled.bool("enab", false); // disabled individual effect -> ignored
    fx_colors
        .bool("masterFXSwitch", true)
        .objc("DrSh", &d_cmyk)
        .objc("OrGl", &g_hsb)
        .objc("IrGl", &g_gray)
        .objc("IrSh", &d_disabled);
    push_tagged_block(
        &mut color_layer.extra_blocks,
        b"lfx2",
        &fx_colors.encode_lfx2_block(),
        false,
    );

    // Layer 2: masterFXSwitch = false -> all effects ignored
    let mut master_off = SyntheticPsdLayer::new("MasterOff", [0, 0, 20, 20], [50, 50, 50, 255]);
    let mut fx_off = DescBuilder::new("Lefx");
    let mut d_active = DescBuilder::new("DrSh");
    d_active.bool("enab", true);
    let mut chfx_off = DescBuilder::new("ChFX");
    chfx_off.bool("enab", true);
    fx_off
        .bool("masterFXSwitch", false)
        .objc("DrSh", &d_active)
        .objc("ChFX", &chfx_off);
    push_tagged_block(
        &mut master_off.extra_blocks,
        b"lfx2",
        &fx_off.encode_lfx2_block(),
        false,
    );

    // Layer 3: Satin (`ChFX`) + Inside Stroke (`FrFX` `InsF`) + Pattern Overlay (`patternFill`) + Multiply `SoFi`
    let mut approx_layer =
        SyntheticPsdLayer::new("SatinAndPattern", [0, 0, 20, 20], [200, 100, 50, 255]);
    let mut chfx = DescBuilder::new("ChFX");
    chfx.bool("enab", true);
    let mut frfx = DescBuilder::new("FrFX");
    frfx.bool("enab", true)
        .enumerated("Styl", "FStl", "InsF")
        .untf("Sz  ", b"#Pxl", 3.0);
    let mut pat = DescBuilder::new("patternFill");
    pat.bool("enab", true);
    let mut mult_sofi = DescBuilder::new("SoFi");
    mult_sofi
        .bool("enab", true)
        .enumerated("Md  ", "BlnM", "Mltp")
        .untf("Opct", b"#Prc", 60.0);
    let mut fx_approx = DescBuilder::new("Lefx");
    fx_approx
        .bool("masterFXSwitch", true)
        .objc("ChFX", &chfx)
        .objc("FrFX", &frfx)
        .objc("patternFill", &pat)
        .objc("SoFi", &mult_sofi);
    push_tagged_block(
        &mut approx_layer.extra_blocks,
        b"lfx2",
        &fx_approx.encode_lfx2_block(),
        false,
    );

    // Layer 4: 11 active stacked `dropShadowMulti` effects -> capped at 8 with located warning
    let mut multi_layer =
        SyntheticPsdLayer::new("ElevenShadows", [0, 0, 20, 20], [80, 120, 240, 255]);
    let mut shadows = Vec::new();
    for idx in 0..11 {
        let mut s = DescBuilder::new("DrSh");
        s.bool("enab", true)
            .bool("uglg", false)
            .untf("lagl", b"#Ang", 90.0)
            .untf("Dstn", b"#Pxl", f64::from(idx + 1))
            .untf("blur", b"#Pxl", 4.0);
        shadows.push(s);
    }
    let mut fx_multi = DescBuilder::new("Lefx");
    fx_multi
        .bool("masterFXSwitch", true)
        .vlls_objc("dropShadowMulti", &shadows);
    push_tagged_block(
        &mut multi_layer.extra_blocks,
        b"lfx2",
        &fx_multi.encode_lfx2_block(),
        false,
    );

    let psb_bytes = build_psd_or_psb(
        2,
        80,
        80,
        &[],
        &[color_layer, master_off, approx_layer, multi_layer],
    );
    let (project, report) = import_psd_bytes(
        &psb_bytes,
        "psb_styles",
        &PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            ..PsdImportOptions::default()
        },
    )
    .expect("import PSB v2");
    project.validate().expect("PSB v2 project validates");

    let root = project.symbol(&project.root).unwrap();

    // Check ColorSpaces layer: 3 active filters (IrSh disabled was skipped)
    let cs_el = &root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "ColorSpaces")
        .unwrap()
        .keyframes[0]
        .elements[0];
    assert_eq!(cs_el.filters.len(), 3);
    assert!(matches!(
        cs_el.filters[0],
        FilterSpec::DropShadow {
            color: 0x00ffff,
            ..
        }
    ));
    assert!(matches!(
        cs_el.filters[1],
        FilterSpec::Glow {
            color: 0x00ff00,
            ..
        }
    ));
    assert!(matches!(
        cs_el.filters[2],
        FilterSpec::Glow { color, inner: true, .. } if color == 0xbfbfbf
    ));

    // Check MasterOff layer: 0 filters
    let off_el = &root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "MasterOff")
        .unwrap()
        .keyframes[0]
        .elements[0];
    assert!(off_el.filters.is_empty());

    // Check ElevenShadows layer: clamped to exactly 8 filters
    let multi_el = &root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "ElevenShadows")
        .unwrap()
        .keyframes[0]
        .elements[0];
    assert_eq!(multi_el.filters.len(), 8);

    // Check located `psd-layer-effect-approximated` warnings on SatinAndPattern and ElevenShadows
    let approx_diags: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "psd-layer-effect-approximated")
        .collect();
    assert!(
        approx_diags
            .iter()
            .any(|d| d.location == "psb_styles::SatinAndPattern" && d.message.contains("Satin")),
        "expected Satin warning on SatinAndPattern: {approx_diags:?}"
    );
    assert!(
        approx_diags
            .iter()
            .any(|d| d.location == "psb_styles::SatinAndPattern"
                && d.message.contains("Pattern Overlay")),
        "expected Pattern Overlay warning on SatinAndPattern: {approx_diags:?}"
    );
    assert!(
        approx_diags
            .iter()
            .any(|d| d.location == "psb_styles::ElevenShadows" && d.message.contains("8-filter")),
        "expected >8 filter cap warning on ElevenShadows: {approx_diags:?}"
    );
}

#[test]
fn psd_legacy_lrfx_fallback_and_non_duplication_when_lfx2_present() {
    fn build_lrfx_shadow_entry(
        angle_deg: i32,
        dist_px: i32,
        blur_px: i32,
        rgb: [u8; 3],
    ) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&2_u32.to_be_bytes()); // version 2
        s.extend_from_slice(&(blur_px << 16).to_be_bytes()); // 16.16 blur
        s.extend_from_slice(&(100_i32 << 16).to_be_bytes()); // 16.16 intensity
        s.extend_from_slice(&(angle_deg << 16).to_be_bytes()); // 16.16 angle
        s.extend_from_slice(&(dist_px << 16).to_be_bytes()); // 16.16 distance
        // 10-byte color: space=0 (RGB), R, G, B u16s, pad
        s.extend_from_slice(&0_u16.to_be_bytes());
        s.extend_from_slice(&(u16::from(rgb[0]) * 257).to_be_bytes());
        s.extend_from_slice(&(u16::from(rgb[1]) * 257).to_be_bytes());
        s.extend_from_slice(&(u16::from(rgb[2]) * 257).to_be_bytes());
        s.extend_from_slice(&0_u16.to_be_bytes());
        s.extend_from_slice(b"8BIMnorm");
        s.push(1); // enabled
        s.push(0); // use_global_angle = false
        s.push(204); // opacity (~80%)
        s.extend_from_slice(&[0u8; 10]); // native color
        s
    }

    fn build_lrfx_glow_entry(blur_px: i32, rgb: [u8; 3]) -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&2_u32.to_be_bytes());
        g.extend_from_slice(&(blur_px << 16).to_be_bytes());
        g.extend_from_slice(&(100_i32 << 16).to_be_bytes());
        g.extend_from_slice(&0_u16.to_be_bytes());
        g.extend_from_slice(&(u16::from(rgb[0]) * 257).to_be_bytes());
        g.extend_from_slice(&(u16::from(rgb[1]) * 257).to_be_bytes());
        g.extend_from_slice(&(u16::from(rgb[2]) * 257).to_be_bytes());
        g.extend_from_slice(&0_u16.to_be_bytes());
        g.extend_from_slice(b"8BIMscrn");
        g.push(1); // enabled
        g.push(255); // opacity
        g
    }

    fn build_lrfx_bevel_entry(angle_deg: i32, blur_px: i32, style: u8) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&2_u32.to_be_bytes());
        b.extend_from_slice(&(angle_deg << 16).to_be_bytes());
        b.extend_from_slice(&(4_i32 << 16).to_be_bytes()); // strength/depth
        b.extend_from_slice(&(blur_px << 16).to_be_bytes());
        b.extend_from_slice(b"8BIMscrn");
        b.extend_from_slice(b"8BIMmul ");
        // highlight white
        b.extend_from_slice(&0_u16.to_be_bytes());
        for _ in 0..3 {
            b.extend_from_slice(&0xffff_u16.to_be_bytes());
        }
        b.extend_from_slice(&0_u16.to_be_bytes());
        // shadow black
        b.extend_from_slice(&[0u8; 10]);
        b.push(style); // 2 = inner bevel
        b.push(204); // highlight opacity
        b.push(191); // shadow opacity
        b.push(1); // enabled
        b.push(0); // use_global_angle = false
        b.push(1); // up
        b
    }

    fn build_lrfx_sofi_entry(rgb: [u8; 3], opacity: u8) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&2_u32.to_be_bytes());
        f.extend_from_slice(b"8BIMnorm");
        f.extend_from_slice(&0_u16.to_be_bytes());
        f.extend_from_slice(&(u16::from(rgb[0]) * 257).to_be_bytes());
        f.extend_from_slice(&(u16::from(rgb[1]) * 257).to_be_bytes());
        f.extend_from_slice(&(u16::from(rgb[2]) * 257).to_be_bytes());
        f.extend_from_slice(&0_u16.to_be_bytes());
        f.push(opacity);
        f.push(1); // enabled
        f.extend_from_slice(&[0u8; 10]);
        f
    }

    let mut lrfx = Vec::new();
    lrfx.extend_from_slice(&0_u16.to_be_bytes()); // version 0
    lrfx.extend_from_slice(&6_u16.to_be_bytes()); // 6 entries
    let cmns = [0, 0, 0, 0, 1, 0, 0]; // version 0, visible = 1
    for (sig, body) in [
        (b"cmnS", cmns.to_vec()),
        (b"dsdw", build_lrfx_shadow_entry(150, 9, 6, [10, 20, 30])),
        (b"isdw", build_lrfx_shadow_entry(150, 3, 4, [40, 50, 60])),
        (b"oglw", build_lrfx_glow_entry(10, [255, 200, 0])),
        (b"bevl", build_lrfx_bevel_entry(120, 5, 2)),
        (b"sofi", build_lrfx_sofi_entry([0, 128, 255], 128)),
    ] {
        lrfx.extend_from_slice(b"8BIM");
        lrfx.extend_from_slice(sig);
        lrfx.extend_from_slice(&(body.len() as u32).to_be_bytes());
        lrfx.extend_from_slice(&body);
        if !body.len().is_multiple_of(2) {
            lrfx.push(0);
        }
    }

    // Layer 1: Legacy lrFX only
    let mut legacy_only =
        SyntheticPsdLayer::new("LegacyOnly", [0, 0, 32, 32], [220, 220, 220, 255]);
    push_tagged_block(&mut legacy_only.extra_blocks, b"lrFX", &lrfx, false);

    // Layer 2: Both lfx2 (1 DropShadow) and lrFX (4 filters) -> must NOT duplicate!
    let mut dual_layer =
        SyntheticPsdLayer::new("DualLfx2AndLrFx", [0, 0, 32, 32], [180, 180, 180, 255]);
    let mut d = DescBuilder::new("DrSh");
    d.bool("enab", true)
        .bool("uglg", false)
        .untf("lagl", b"#Ang", 100.0)
        .untf("Dstn", b"#Pxl", 5.0);
    let mut fx = DescBuilder::new("Lefx");
    fx.bool("masterFXSwitch", true).objc("DrSh", &d);
    push_tagged_block(&mut dual_layer.extra_blocks, b"lrFX", &lrfx, false);
    push_tagged_block(
        &mut dual_layer.extra_blocks,
        b"lfx2",
        &fx.encode_lfx2_block(),
        false,
    );

    let psd = build_psd_or_psb(1, 64, 64, &[], &[legacy_only, dual_layer]);
    let (project, _) = import_psd_bytes(
        &psd,
        "legacy_lrfx",
        &PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            ..PsdImportOptions::default()
        },
    )
    .unwrap();
    project.validate().unwrap();

    let root = project.symbol(&project.root).unwrap();
    let leg_el = &root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "LegacyOnly")
        .unwrap()
        .keyframes[0]
        .elements[0];
    assert_eq!(leg_el.filters.len(), 4);
    assert!(matches!(
        &leg_el.filters[0],
        FilterSpec::DropShadow { angle, distance, inner: false, .. }
            if (angle - 30.0).abs() < 1e-6 && (distance - 9.0).abs() < 1e-6
    ));
    assert!(matches!(
        &leg_el.filters[1],
        FilterSpec::DropShadow { inner: true, .. }
    ));
    assert!(matches!(
        &leg_el.filters[2],
        FilterSpec::Glow { inner: false, .. }
    ));
    assert!(matches!(
        &leg_el.filters[3],
        FilterSpec::Bevel { placement, angle, .. }
            if placement == "inner" && (angle - 60.0).abs() < 1e-6
    ));
    assert!(!leg_el.transform.color_transform.is_identity());

    let dual_el = &root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "DualLfx2AndLrFx")
        .unwrap()
        .keyframes[0]
        .elements[0];
    assert_eq!(
        dual_el.filters.len(),
        1,
        "lfx2 must take precedence over lrFX without duplicating filters"
    );
}

#[test]
fn psd_malformed_hostile_lfx2_and_lrfx_never_panic_or_oom() {
    let mut hostile_payloads: Vec<(&str, [u8; 4], Vec<u8>)> = Vec::new();

    // 1. Huge UTF-16BE char_count = u32::MAX in `TEXT`
    let mut bad_utf16 = Vec::new();
    bad_utf16.extend_from_slice(&0_u32.to_be_bytes());
    bad_utf16.extend_from_slice(&16_u32.to_be_bytes());
    bad_utf16.extend_from_slice(&1_u32.to_be_bytes()); // class name len = 1
    bad_utf16.extend_from_slice(&0_u16.to_be_bytes());
    push_desc_id(&mut bad_utf16, "Lefx");
    bad_utf16.extend_from_slice(&1_u32.to_be_bytes()); // 1 item
    push_desc_id(&mut bad_utf16, "Nm  ");
    bad_utf16.extend_from_slice(b"TEXT");
    bad_utf16.extend_from_slice(&u32::MAX.to_be_bytes()); // hostile u32::MAX UTF-16 count!
    hostile_payloads.push(("HugeUtf16", *b"lfx2", bad_utf16));

    // 2. Huge VlLs list count = u32::MAX
    let mut bad_vlls = Vec::new();
    bad_vlls.extend_from_slice(&0_u32.to_be_bytes());
    bad_vlls.extend_from_slice(&16_u32.to_be_bytes());
    bad_vlls.extend_from_slice(&1_u32.to_be_bytes());
    bad_vlls.extend_from_slice(&0_u16.to_be_bytes());
    push_desc_id(&mut bad_vlls, "Lefx");
    bad_vlls.extend_from_slice(&1_u32.to_be_bytes());
    push_desc_id(&mut bad_vlls, "DrSh");
    bad_vlls.extend_from_slice(b"VlLs");
    bad_vlls.extend_from_slice(&u32::MAX.to_be_bytes()); // hostile u32::MAX VlLs count!
    hostile_payloads.push(("HugeVlLs", *b"lfx2", bad_vlls));

    // 3. Huge Objc item_count = u32::MAX + unpaired UTF-16 surrogate class name
    let mut bad_objc = Vec::new();
    bad_objc.extend_from_slice(&0_u32.to_be_bytes());
    bad_objc.extend_from_slice(&16_u32.to_be_bytes());
    bad_objc.extend_from_slice(&2_u32.to_be_bytes());
    bad_objc.extend_from_slice(&0xD800_u16.to_be_bytes()); // lone high surrogate
    bad_objc.extend_from_slice(&0_u16.to_be_bytes());
    push_desc_id(&mut bad_objc, "Lefx");
    bad_objc.extend_from_slice(&u32::MAX.to_be_bytes()); // hostile u32::MAX item_count!
    hostile_payloads.push(("HugeObjcSurrogate", *b"lfx2", bad_objc));

    // 4. Deeply nested Objc > MAX_DESC_DEPTH (24 levels deep)
    let mut nested = DescBuilder::new("Leaf");
    nested.bool("enab", true);
    for _ in 0..24 {
        let mut parent = DescBuilder::new("Wrap");
        parent.objc("Chld", &nested);
        nested = parent;
    }
    hostile_payloads.push(("DeepNested", *b"lfx2", nested.encode_lfx2_block()));

    // 5. Truncated lrFX claiming 50 entries and oversized subblock len
    let mut bad_lrfx = Vec::new();
    bad_lrfx.extend_from_slice(&0_u16.to_be_bytes());
    bad_lrfx.extend_from_slice(&50_u16.to_be_bytes());
    bad_lrfx.extend_from_slice(b"8BIMdsdw");
    bad_lrfx.extend_from_slice(&0x7fff_ffff_u32.to_be_bytes());
    hostile_payloads.push(("TruncatedLrFx", *b"lrFX", bad_lrfx));

    // 6. Malformed `luni` claiming u32::MAX UTF-16 characters
    hostile_payloads.push(("HugeLuni", *b"luni", u32::MAX.to_be_bytes().to_vec()));

    let mut layers = Vec::new();
    for (name, key, payload) in &hostile_payloads {
        let mut l = SyntheticPsdLayer::new(name, [0, 0, 16, 16], [100, 150, 200, 255]);
        push_tagged_block(&mut l.extra_blocks, key, payload, false);
        layers.push(l);
    }

    let psd = build_psd_or_psb(1, 32, 32, &[], &layers);
    let (project, report) = import_psd_bytes(
        &psd,
        "hostile_psd",
        &PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            ..PsdImportOptions::default()
        },
    )
    .expect("hostile descriptors must degrade gracefully without failing container");
    project.validate().unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-layer-effect-approximated"),
        "expected malformed lfx2/lrFX approximation diagnostic"
    );
}

// ============================================================================
// Synthetic Binary `.fla` Builders & Tests
// ============================================================================

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn mfc_utf16(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    assert!(units.len() < 255);
    out.extend_from_slice(&[0xff, 0xfe, 0xff]);
    out.push(units.len() as u8);
    for u in units {
        u16b(out, u);
    }
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    out.push(s.len() as u8);
    for b in s.bytes() {
        u16b(out, u16::from(b));
    }
}
fn mfc_class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xffff);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());
}

struct PageWriter {
    bytes: Vec<u8>,
    classes: BTreeMap<&'static str, u16>,
    next: u16,
}
impl PageWriter {
    fn new() -> Self {
        Self {
            bytes: vec![1], // timeline stream header byte
            classes: BTreeMap::new(),
            next: 1,
        }
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(id) = self.classes.get(class).copied() {
            u16b(&mut self.bytes, 0x8000 | id);
        } else {
            mfc_class(&mut self.bytes, class);
            self.classes.insert(class, self.next);
            self.next += 1;
        }
        let id = self.next;
        self.next += 1;
        id
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
        for n in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, n);
        }
        self.bytes.push(5);
        self.bytes.extend_from_slice(&[0; 9]);
        u32b(&mut self.bytes, 0);
    }
}

fn push_minimal_contents_scene_and_symbol(contents: &mut Vec<u8>, with_symbol: bool) {
    contents.push(0x3f);
    u32b(contents, 1); // compatibility_marker
    u32b(contents, 0); // ole_count
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(contents, 0);
    }
    mfc_class(contents, "CDocumentPage"); // ref 1 (class), ref 2 (Scene 1)
    push_cdocument_page_rev5(contents, "Page 1", "Scene 1", 0, 2);
    u16b(contents, 0); // end scenes
    u16b(contents, 1); // next_scene_id
    u16b(contents, 1); // next_item_tag
    u16b(contents, 2); // selected_scene = ref 2
    if with_symbol {
        u16b(contents, 0x8001); // CDocumentPage class ref 1 -> ref 3 (Symbol 1)
        push_cdocument_page_rev5(contents, "Symbol 1", "ClipSym", 100, 2);
        u16b(contents, 0); // end symbols
        u16b(contents, 101); // library_counter
    } else {
        u16b(contents, 0); // end symbols
        u16b(contents, 1); // library_counter
    }
}

fn push_cdocument_page_rev5(
    contents: &mut Vec<u8>,
    stream_name: &str,
    item_name: &str,
    library_id: u32,
    symbol_type: u32,
) {
    contents.push(5); // descriptor rev 5 (ASCII stream_name)
    mfc_str(contents, stream_name);
    mfc_str(contents, item_name);
    u32b(contents, library_id);
    contents.push(symbol_type as u8);
    u32b(contents, 3); // library_metadata rev 3
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
}

fn push_cmedia_bits_descriptor(
    contents: &mut Vec<u8>,
    first_bitmap_class_id: &mut Option<u16>,
    next_ref: &mut u16,
    stream_name: &str,
    item_name: &str,
    media_id: u16,
    encoded_byte_count: i32,
) {
    if let Some(cid) = *first_bitmap_class_id {
        u16b(contents, 0x8000 | cid);
    } else {
        mfc_class(contents, "CMediaBits");
        *first_bitmap_class_id = Some(*next_ref);
        *next_ref += 1;
    }
    *next_ref += 1; // object ref
    contents.push(6); // descriptor rev 6 (UTF-16 stream_name)
    compact_utf16(contents, stream_name);
    mfc_str(contents, item_name);
    u16b(contents, media_id);
    mfc_str(contents, "");
    u32b(contents, 0);
    u32b(contents, 3); // library_metadata rev 3
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    // bitmap_descriptor rev 3
    contents.push(3);
    contents.push(if encoded_byte_count > 0 { 0 } else { 1 }); // compression
    contents.push(80); // quality
    contents.push(1); // smoothing = true
    i32b(contents, encoded_byte_count);
}

fn push_contents_settings(contents: &mut Vec<u8>, media_counter: u16) {
    u16b(contents, 0); // end media list
    u16b(contents, media_counter);
    u16b(contents, 0); // ruler_unit: u16
    contents.push(0); // grid_display_mode: u8
    for word in [0_i32, 11000, 0, 8000] {
        i32b(contents, word);
    }
    u32b(contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    i32b(contents, 65536);
    u16b(contents, 0);
    u16b(contents, 0);
    u32b(contents, 0xffffffff);
    u32b(contents, 0);
    u32b(contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    i32b(contents, 20);
}

fn build_native_argb_media_stream(
    width: u16,
    height: u16,
    compressed: bool,
    noisy_alpha: bool,
) -> Vec<u8> {
    let stride = (width as usize) * 4;
    let mut raw = Vec::with_capacity(stride * (height as usize));
    let mut state = 0x1234_5678_9abc_def0_u64;
    for y in 0..height as usize {
        for x in 0..width as usize {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let a = if noisy_alpha {
                ((state >> 56) as u8).max(32)
            } else {
                255
            };
            let r = (((state >> 40) as u8).min(a))
                .min(((x * 7 + y * 3) & 0xff) as u8)
                .min(a);
            let g = ((state >> 24) as u8).min(a);
            let b = ((state >> 8) as u8).min(a);
            raw.extend_from_slice(&[a, r, g, b]);
        }
    }
    let mut out = Vec::new();
    out.push(3); // revision 3
    out.push(5); // format 5 (32-bit ARGB)
    u16b(&mut out, stride as u16);
    u16b(&mut out, width);
    u16b(&mut out, height);
    for b in [0_i32, i32::from(width) * 20, 0, i32::from(height) * 20] {
        i32b(&mut out, b);
    }
    out.push(u8::from(noisy_alpha)); // alpha present
    out.push(u8::from(compressed));
    if compressed {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&raw).unwrap();
        let z = enc.finish().unwrap();
        for chunk in z.chunks(32_768) {
            u16b(&mut out, chunk.len() as u16);
            out.extend_from_slice(chunk);
        }
        u16b(&mut out, 0);
    } else {
        out.extend_from_slice(&raw);
    }
    out
}

#[test]
fn fla_timeline_and_contents_trailing_revisions_decode_completely() {
    let tmp = TempDir::new();

    // Build Symbol Page 2 (empty 1-frame clip so Library ID 100 resolves)
    let mut sym_w = PageWriter::new();
    sym_w.begin("CPicPage");
    sym_w.base_start();
    sym_w.begin("CPicLayer");
    sym_w.base_start();
    sym_w.begin("CPicFrame");
    sym_w.base_start();
    sym_w.base_end();
    sym_w.empty_shape();
    sym_w.bytes.push(1); // frame rev 1
    u16b(&mut sym_w.bytes, 1); // duration 1
    sym_w.bytes.push(0); // key_mode 0 (1 byte for rev <= 2)
    sym_w.base_end();
    sym_w.bytes.push(2); // layer rev 2
    mfc_str(&mut sym_w.bytes, "SymLayer");
    sym_w.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
    sym_w.base_end();
    sym_w.bytes.push(1); // page rev 1
    u16b(&mut sym_w.bytes, 1);

    // Build Scene Page 1 with newly supported trailing revisions:
    let mut w = PageWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();

    // Child 1: CPicBitmap rev 0 inline raster (2x2 RGB24)
    w.begin("CPicBitmap");
    w.base_start();
    w.base_end();
    w.bytes.push(0); // CPicBitmap rev 0!
    for m in [65536_i32, 0, 0, 65536, 100, 200] {
        i32b(&mut w.bytes, m);
    }
    let inline_raster = build_native_argb_media_stream(2, 2, false, false);
    w.bytes.extend_from_slice(&inline_raster);

    // Child 2: CPicSymbol rev 12 with timelineEffect (rev >= 11) + persistentData (rev >= 12)
    w.begin("CPicSymbol");
    w.base_start();
    w.base_end();
    w.bytes.push(12); // CPicSymbol rev 12!
    for m in [65536_i32, 0, 0, 65536, 400, 200] {
        i32b(&mut w.bytes, m);
    }
    u16b(&mut w.bytes, 0); // first_frame
    u16b(&mut w.bytes, 0); // loop_mode
    w.bytes.push(1); // color_transform rev 1 -> 8 i16s + effect_type/percent/rgba
    for mv in [256_i16, 0, 256, 0, 256, 0, 256, 0] {
        u16b(&mut w.bytes, mv as u16);
    }
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    mfc_str(&mut w.bytes, ""); // legacy_name (rev >= 6)
    u32b(&mut w.bytes, 100); // library_id = 100
    w.bytes.push(1); // timelineEffect present!
    mfc_str(&mut w.bytes, "DropShadowEffect");
    mfc_str(&mut w.bytes, "Effects");
    mfc_str(&mut w.bytes, "graphic");
    mfc_str(&mut w.bytes, "DropShadow.jsfl");
    u32b(&mut w.bytes, 0); // 0 effect params
    w.bytes.push(1); // persistentData present!
    u32b(&mut w.bytes, 1); // header
    u32b(&mut w.bytes, 1); // 1 entry
    w.bytes.push(0); // kind 0 (i32)
    mfc_str(&mut w.bytes, "customKey");
    i32b(&mut w.bytes, 42);

    // Child 3: CPicButton (base rev 7 + Button ext rev 9) with historical wide UTF-16 instance name & accessibility
    w.begin("CPicButton");
    w.base_start();
    w.base_end();
    w.bytes.push(7); // CPicSymbol base rev 7
    for m in [65536_i32, 0, 0, 65536, 800, 200] {
        i32b(&mut w.bytes, m);
    }
    u16b(&mut w.bytes, 0); // first_frame
    u16b(&mut w.bytes, 0); // loop_mode
    w.bytes.push(1); // color_transform rev 1
    for mv in [256_i16, 0, 256, 0, 256, 0, 256, 0] {
        u16b(&mut w.bytes, mv as u16);
    }
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    mfc_str(&mut w.bytes, ""); // legacy_name
    u32b(&mut w.bytes, 100); // library_id = 100
    w.bytes.push(9); // CPicButton extension rev 9!
    // Actions (container 19): rev = 0, mode = 2 (empty)
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 2);
    w.bytes.push(0); // track_as_menu
    // Historical byte-swapped wide string: byte_len = 8 ("Play" in BE u16s)
    u32b(&mut w.bytes, 8);
    for ch in "Play".encode_utf16() {
        w.bytes.extend_from_slice(&ch.to_be_bytes());
    }
    // Historical wide accessibility (rev 1)
    u32b(&mut w.bytes, 1);
    u32b(&mut w.bytes, 0); // flags = enabled
    u32b(&mut w.bytes, 14); // "PlayBtn" (7 u16s = 14 bytes LE)
    for ch in "PlayBtn".encode_utf16() {
        u16b(&mut w.bytes, ch);
    }
    u32b(&mut w.bytes, 0); // description byte_len = 0
    u32b(&mut w.bytes, 0); // shortcut byte_len = 0
    u32b(&mut w.bytes, 0); // force_simple
    u32b(&mut w.bytes, 0); // auto_labeling

    // Child 4: CPicTLFText (rev 1) + Child 5: CPicThreeDObject (rev 1) -> must NOT stop stream!
    w.begin("CPicTLFText");
    w.base_start();
    w.base_end();
    w.bytes.push(1);
    for m in [65536_i32, 0, 0, 65536, 1200, 200] {
        i32b(&mut w.bytes, m);
    }
    for b in [0_i32, 2000, 0, 600] {
        i32b(&mut w.bytes, b);
    }
    mfc_str(&mut w.bytes, "fl.text.TLFTextField");
    mfc_utf16(&mut w.bytes, "<TextFlow>Hello</TextFlow>");
    w.bytes.push(1); // tlfRevision
    mfc_str(&mut w.bytes, ""); // localizationStringId
    mfc_str(&mut w.bytes, ""); // variableName

    w.begin("CPicThreeDObject");
    w.base_start();
    w.base_end();
    w.bytes.push(1);
    for m in [65536_i32, 0, 0, 65536, 1600, 200] {
        i32b(&mut w.bytes, m);
    }
    for b in [0_i32, 1000, 0, 1000] {
        i32b(&mut w.bytes, b);
    }
    mfc_str(&mut w.bytes, "ThreeD");
    mfc_str(&mut w.bytes, "");

    w.base_end(); // end CPicFrame children
    w.empty_shape();
    // CPicFrame rev 29 with CS4 poseMatrices (key_mode = 4 IK pose)!
    w.bytes.push(29);
    u16b(&mut w.bytes, 1); // duration = 1
    u16b(&mut w.bytes, 4); // key_mode = 4 (IK pose)
    u16b(&mut w.bytes, 0); // acceleration
    u16b(&mut w.bytes, 0); // sound_media_id = 0
    u16b(&mut w.bytes, 0); // sound_envelope_count = 0
    u16b(&mut w.bytes, 1); // sound_loop_count = 1
    w.bytes.push(0); // sound_sync = 0
    u32b(&mut w.bytes, 0); // sound_in_point
    u32b(&mut w.bytes, 0); // sound_out_point
    u16b(&mut w.bytes, 0); // sound_zoom
    mfc_utf16(&mut w.bytes, "PoseFrame"); // label
    // actions (rev 0, mode 2 = empty)
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 2);
    u32b(&mut w.bytes, 0); // rotation_mode
    u32b(&mut w.bytes, 0); // rotation_count
    u32b(&mut w.bytes, 0); // label_type
    u16b(&mut w.bytes, 0); // morph_shape = null
    u32b(&mut w.bytes, 0); // shape_tween_blend
    u16b(&mut w.bytes, 0); // sound_settings = null
    mfc_str(&mut w.bytes, ""); // extension
    u32b(&mut w.bytes, 0); // reserved
    u32b(&mut w.bytes, 0); // sound_effect
    u32b(&mut w.bytes, 0); // bookmark
    u32b(&mut w.bytes, 1); // single_ease_curve
    u32b(&mut w.bytes, 0); // custom_ease_present
    // poseMatrices (rev >= 29 && kind == 4): bone_count = 1, 1 * duration(1) matrix of 24 bytes
    u32b(&mut w.bytes, 1);
    for m in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut w.bytes, m);
    }

    w.base_end(); // end CPicLayer children
    w.bytes.push(2); // layer rev 2
    mfc_str(&mut w.bytes, "MainLayer");
    w.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);

    w.base_end(); // end CPicPage children
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);

    let mut contents = Vec::new();
    push_minimal_contents_scene_and_symbol(&mut contents, true);
    push_contents_settings(&mut contents, 1);

    let mut cfb_buf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb_buf
        .create_stream("Contents")
        .unwrap()
        .write_all(&contents)
        .unwrap();
    cfb_buf
        .create_stream("Page 1")
        .unwrap()
        .write_all(&w.bytes)
        .unwrap();
    cfb_buf
        .create_stream("Symbol 1")
        .unwrap()
        .write_all(&sym_w.bytes)
        .unwrap();

    let fla_path = tmp.0.join("revisions.fla");
    fs::write(&fla_path, cfb_buf.into_inner().into_inner()).unwrap();

    let doc = read_binary_timeline(&fla_path).unwrap();
    for s in &doc.inspection.streams {
        if s.role == neoflash_import::BinaryStreamRole::Contents {
            continue;
        }
        assert!(
            s.stop_code.is_none(),
            "stream #{} stopped unexpectedly: {:?} {:?}",
            s.index,
            s.stop_code,
            s.stop_message
        );
    }

    let report = inspect(&fla_path).unwrap();
    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();

    let root = project.symbol(&project.root).unwrap();
    let kf = &root.timeline.layers[0].keyframes[0];
    assert_eq!(kf.label, "PoseFrame");
    assert_eq!(kf.elements.len(), 5);
    assert!(matches!(
        kf.elements[0].kind,
        ElementKind::Bitmap {
            width: 2,
            height: 2,
            ..
        }
    ));
    assert_eq!(kf.elements[2].name, "Play");
    assert_eq!(
        kf.elements[2]
            .accessibility
            .as_ref()
            .map(|a| a.name.as_str()),
        Some("PlayBtn")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_layer_animation")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_text_approximated")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_object_placeholder")
    );
}

fn build_single_scene_bitmap_fla(media_streams: &[(String, Vec<u8>, i32)]) -> Vec<u8> {
    let mut contents = Vec::new();
    push_minimal_contents_scene_and_symbol(&mut contents, false);
    let mut first_bmp_cls = None;
    let mut next_ref = 3_u16;
    for (idx, (stream_name, _, enc_len)) in media_streams.iter().enumerate() {
        let mid = (idx + 1) as u16;
        push_cmedia_bits_descriptor(
            &mut contents,
            &mut first_bmp_cls,
            &mut next_ref,
            stream_name,
            &format!("Bitmap{mid}"),
            mid,
            *enc_len,
        );
    }
    push_contents_settings(&mut contents, (media_streams.len() + 1) as u16);

    // Scene Page 1 placing every bitmap on Stage
    let mut w = PageWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    for idx in 0..media_streams.len() {
        let mid = (idx + 1) as u16;
        w.begin("CPicBitmap");
        w.base_start();
        w.base_end();
        w.bytes.push(1); // CPicBitmap rev 1
        for m in [65536_i32, 0, 0, 65536, (idx as i32) * 400 + 800, 1200] {
            i32b(&mut w.bytes, m);
        }
        u16b(&mut w.bytes, mid);
    }
    w.base_end();
    w.empty_shape();
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);
    w.bytes.push(0);
    w.base_end();
    w.bytes.push(2);
    mfc_str(&mut w.bytes, "Bitmaps");
    w.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
    w.base_end();
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);

    let mut cfb_buf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb_buf
        .create_stream("Contents")
        .unwrap()
        .write_all(&contents)
        .unwrap();
    cfb_buf
        .create_stream("Page 1")
        .unwrap()
        .write_all(&w.bytes)
        .unwrap();
    for (stream_name, bytes, _) in media_streams {
        cfb_buf
            .create_stream(stream_name)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    cfb_buf.into_inner().into_inner()
}

#[test]
fn fla_oversized_single_resource_bitmap_downscales_cleanly_within_16mib() {
    let tmp = TempDir::new();

    // Incompressible 2200 x 2200 translucent ARGB bitmap whose PNG exceeds 16 MiB single-resource cap
    let noisy_argb = build_native_argb_media_stream(2200, 2200, true, true);
    let fla_16m = tmp.0.join("oversized_single_16mib.fla");
    fs::write(
        &fla_16m,
        build_single_scene_bitmap_fla(&[("Media 1".into(), noisy_argb, 0)]),
    )
    .unwrap();

    let rep_16m = inspect(&fla_16m).unwrap();
    let proj_16m = import_project(&fla_16m).unwrap();
    proj_16m.validate().unwrap();
    assert!(
        rep_16m
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_bitmap_downscaled" && d.location == "media 1")
    );
    assert!(
        rep_16m
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_bitmap_skipped")
    );
    let el_16m =
        &proj_16m.symbol(&proj_16m.root).unwrap().timeline.layers[0].keyframes[0].elements[0];
    match &el_16m.kind {
        ElementKind::Bitmap { width, height, .. } => {
            assert!(*width < 2200 && *height < 2200);
            let rendered_w = f64::from(*width) * el_16m.transform.scale_x;
            assert!((rendered_w - 2200.0).abs() < 2.0);
        }
        other => panic!("expected downscaled Stage Bitmap, got {other:?}"),
    }
}

#[test]
fn fla_oversized_multi_bitmap_runtime_rgba_budget_downscales_before_shedding() {
    let tmp = TempDir::new();

    // 9 distinct 2048 x 2048 solid ARGB bitmaps: compressed stream total is < 300 KiB,
    // while decoded runtime RGBA is 9 * 16 MiB = 144 MiB > 128 MiB MAX_BITMAP_BYTES!
    let mut streams = Vec::new();
    for idx in 0..9 {
        let stride = 2048 * 4;
        let pixel = [255_u8, (idx as u8) * 25 + 10, 80, 160];
        let raw = pixel.repeat(2048 * 2048);
        let mut out = Vec::new();
        out.push(3); // revision 3
        out.push(5); // format 5 (32-bit ARGB)
        u16b(&mut out, stride as u16);
        u16b(&mut out, 2048);
        u16b(&mut out, 2048);
        for b in [0_i32, 2048 * 20, 0, 2048 * 20] {
            i32b(&mut out, b);
        }
        out.push(0); // opaque
        out.push(1); // zlib compressed
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&raw).unwrap();
        let z = enc.finish().unwrap();
        for chunk in z.chunks(32_768) {
            u16b(&mut out, chunk.len() as u16);
            out.extend_from_slice(chunk);
        }
        u16b(&mut out, 0);
        streams.push((format!("Media {}", idx + 1), out, 0));
    }

    let fla_path = tmp.0.join("oversized_rgba_budget.fla");
    fs::write(&fla_path, build_single_scene_bitmap_fla(&streams)).unwrap();

    let rep = inspect(&fla_path).unwrap();
    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();

    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_bitmap_downscaled"),
        "expected fla_bitmap_downscaled diagnostic: {:?}",
        rep.diagnostics
    );
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_bitmap_skipped"),
        "all 9 Stage bitmaps must stay visible without shedding to placeholders: {:?}",
        rep.diagnostics
    );
    let elems = &proj.symbol(&proj.root).unwrap().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elems.len(), 9);
    for el in elems {
        match &el.kind {
            ElementKind::Bitmap { width, height, .. } => {
                let rendered_w = f64::from(*width) * el.transform.scale_x;
                let rendered_h = f64::from(*height) * el.transform.scale_y;
                assert!((rendered_w - 2048.0).abs() < 1.0);
                assert!((rendered_h - 2048.0).abs() < 1.0);
            }
            other => panic!("expected visible Stage Bitmap, got {other:?}"),
        }
    }
}
