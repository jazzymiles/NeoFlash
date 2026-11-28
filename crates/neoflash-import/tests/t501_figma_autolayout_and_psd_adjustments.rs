//! Integration tests for T-501:
//! 1. Figma `GRADIENT_DIAMOND` & `GRADIENT_ANGULAR` imported as true scalable
//!    `VectorMeshPaint` (`Keyframe.vector_effects` -> `DrawItem.mesh_fill`) in both
//!    `.figma.json` and `.fig` Kiwi archives with zero bitmap resources and zero
//!    `figma-gradient-approximated` warnings.
//! 2. Deeper nested Figma Auto Layout reflow (`HORIZONTAL`, `VERTICAL`, `WRAP`, `HUG`,
//!    `FILL`, `SPACE_BETWEEN`, `BASELINE`, `minWidth`/`maxWidth`/`minHeight`/`maxHeight`,
//!    `itemReverseZIndex`, `strokesIncludedInLayout`, `ABSOLUTE` child constraints on parent
//!    resize, and `INSTANCE` text overrides growing `HUG` components) in `.figma.json` and `.fig`.
//! 3. Photoshop PSD `GdFl` (`Lnr `, `Rdl `, `Rflc`, `Angl`, `Dmnd`), `SoCo` (`0x0` full-canvas
//!    & `vmsk` v3 Bézier masks), and clipped + unclipped Adjustment Layers (`nvrt`, `brit`,
//!    `hue `/`hue2`, `bw  `/`blwh`, `expA`, `vibr`/`vibA`, `levl`, `curv`, `post`, `thrs`, `blnc`)
//!    preserving separate editable Timeline layers in `PsdLayerMapping::Automatic`.

use flate2::{Compression, write::DeflateEncoder};
use neoflash_core::{
    AuthoredScene, ElementKind, FillRule, FilterSpec, GradientKind, LayerRole, PathCommand,
};
use neoflash_import::{
    FigmaImportOptions, PsdImportOptions, PsdLayerMapping, import_figma_bytes,
    import_figma_clipboard, import_figma_json_str, import_psd_bytes,
};
use std::io::{Cursor, Write};

// ============================================================================
// Kiwi Binary Builders (`.fig` ZIP archive)
// ============================================================================

fn write_var_uint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn write_var_int(out: &mut Vec<u8>, v: i64) {
    let zigzag = ((v << 1) ^ (v >> 63)) as u64;
    write_var_uint(out, zigzag);
}

fn write_var_float(out: &mut Vec<u8>, f: f32) {
    if f == 0.0 {
        out.push(0);
        return;
    }
    let rotated = f.to_bits().rotate_left(9);
    out.extend_from_slice(&rotated.to_le_bytes());
}

fn write_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

fn wrap_fig_kiwi_zip(schema: &[u8], msg: &[u8]) -> Vec<u8> {
    let deflate = |raw: &[u8]| -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    };
    let c0 = deflate(schema);
    let c1 = deflate(msg);
    let mut canvas_fig = Vec::new();
    canvas_fig.extend_from_slice(b"fig-kiwi");
    canvas_fig.extend_from_slice(&15u32.to_le_bytes());
    canvas_fig.extend_from_slice(&(c0.len() as u32).to_le_bytes());
    canvas_fig.extend_from_slice(&c0);
    canvas_fig.extend_from_slice(&(c1.len() as u32).to_le_bytes());
    canvas_fig.extend_from_slice(&c1);

    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zip_buf);
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("meta.json", opts).unwrap();
        zw.write_all(br#"{"file_name":"T501 Kiwi Board"}"#).unwrap();
        zw.start_file("canvas.fig", opts).unwrap();
        zw.write_all(&canvas_fig).unwrap();
        zw.finish().unwrap();
    }
    zip_buf.into_inner()
}

// ============================================================================
// Synthetic PSD & Action Descriptor Builders
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

    fn encode_v16_block(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&16_u32.to_be_bytes());
        out.extend_from_slice(&self.encode_body());
        out
    }
}

fn rgb_desc(r: f64, g: f64, b: f64) -> DescBuilder {
    let mut d = DescBuilder::new("RGBC");
    d.doub("Rd  ", r).doub("Grn ", g).doub("Bl  ", b);
    d
}

fn build_gdfl_payload(
    grad_type: &str,
    angle_deg: f64,
    scale_pct: f64,
    reverse: bool,
    offset_pct: [f64; 2],
    color_stops: &[(i32, [u8; 3])],
    alpha_stops: &[(i32, f64)],
) -> Vec<u8> {
    let clrs: Vec<DescBuilder> = color_stops
        .iter()
        .map(|&(lctn_4096, [r, g, b])| {
            let mut st = DescBuilder::new("Clrt");
            let c = rgb_desc(f64::from(r), f64::from(g), f64::from(b));
            st.objc("Clr ", &c)
                .enumerated("Type", "Clry", "UsrS")
                .long("Lctn", lctn_4096)
                .long("Mdpn", 50);
            st
        })
        .collect();
    let trns: Vec<DescBuilder> = alpha_stops
        .iter()
        .map(|&(lctn_4096, opct_pct)| {
            let mut st = DescBuilder::new("TrnS");
            st.untf("Opct", b"#Prc", opct_pct)
                .long("Lctn", lctn_4096)
                .long("Mdpn", 50);
            st
        })
        .collect();
    let mut grad = DescBuilder::new("Grdn");
    grad.vlls_objc("Clrs", &clrs).vlls_objc("Trns", &trns);

    let mut ofst = DescBuilder::new("Pnt ");
    ofst.untf("Hrzn", b"#Prc", offset_pct[0])
        .untf("Vrtc", b"#Prc", offset_pct[1]);

    let mut gdfl = DescBuilder::new("gradientLayer");
    gdfl.untf("Angl", b"#Ang", angle_deg)
        .enumerated("Type", "GrdT", grad_type)
        .bool("Rvrs", reverse)
        .bool("Algn", true)
        .untf("Scl ", b"#Prc", scale_pct)
        .objc("Ofst", &ofst)
        .objc("Grad", &grad);
    gdfl.encode_v16_block()
}

fn build_soco_payload(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut soco = DescBuilder::new("solidColorLayer");
    let c = rgb_desc(f64::from(r), f64::from(g), f64::from(b));
    soco.objc("Clr ", &c);
    soco.encode_v16_block()
}

fn fixed_8_24(v: f64) -> [u8; 4] {
    let raw = (v * 16_777_216.0).round() as i32;
    raw.to_be_bytes()
}

/// Build a Photoshop version-3 `vmsk` block containing one closed 4-knot cubic Bézier diamond/capsule.
fn build_vmsk_v3_cubic_shape(canvas_w: f64, canvas_h: f64, fill_even_odd: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3_u32.to_be_bytes()); // version = 3
    out.extend_from_slice(&0_u32.to_be_bytes()); // flags = 0 (enabled, non-inverted)

    // Record 0: selector 6 (fill rule)
    let mut fill_rec = [0u8; 26];
    fill_rec[0..2].copy_from_slice(&6_u16.to_be_bytes());
    fill_rec[2..4].copy_from_slice(&u16::from(fill_even_odd).to_be_bytes());
    out.extend_from_slice(&fill_rec);

    // Record 1: selector 0 (closed subpath length = 4 knots)
    let mut len_rec = [0u8; 26];
    len_rec[0..2].copy_from_slice(&0_u16.to_be_bytes());
    len_rec[2..4].copy_from_slice(&4_u16.to_be_bytes());
    out.extend_from_slice(&len_rec);

    // 4 curved knots in pixel coordinates `(px, py)` normalized by `(canvas_w, canvas_h)`
    let knots = [
        // (preceding_x, preceding_y, anchor_x, anchor_y, leaving_x, leaving_y)
        (30.0, 10.0, 50.0, 10.0, 70.0, 10.0),
        (90.0, 30.0, 90.0, 50.0, 90.0, 70.0),
        (70.0, 90.0, 50.0, 90.0, 30.0, 90.0),
        (10.0, 70.0, 10.0, 50.0, 10.0, 30.0),
    ];
    for (px, py, ax, ay, lx, ly) in knots {
        let mut kr = [0u8; 26];
        kr[0..2].copy_from_slice(&1_u16.to_be_bytes()); // linked closed knot
        kr[2..6].copy_from_slice(&fixed_8_24(py / canvas_h));
        kr[6..10].copy_from_slice(&fixed_8_24(px / canvas_w));
        kr[10..14].copy_from_slice(&fixed_8_24(ay / canvas_h));
        kr[14..18].copy_from_slice(&fixed_8_24(ax / canvas_w));
        kr[18..22].copy_from_slice(&fixed_8_24(ly / canvas_h));
        kr[22..26].copy_from_slice(&fixed_8_24(lx / canvas_w));
        out.extend_from_slice(&kr);
    }
    out
}

fn push_tagged_block(extra: &mut Vec<u8>, key: &[u8; 4], payload: &[u8]) {
    extra.extend_from_slice(b"8BIM");
    extra.extend_from_slice(key);
    extra.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    extra.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        extra.push(0);
    }
}

struct SynthPsdLayer {
    name: String,
    rect: [i32; 4], // [top, left, bottom, right]
    blend: [u8; 4],
    opacity: u8,
    clipping: bool,
    visible: bool,
    noop_white_mask: bool,
    extra_blocks: Vec<u8>,
    rgba_fill: Option<[u8; 4]>,
}

impl SynthPsdLayer {
    fn pixel(name: &str, rect: [i32; 4], rgba: [u8; 4]) -> Self {
        Self {
            name: name.to_owned(),
            rect,
            blend: *b"norm",
            opacity: 255,
            clipping: false,
            visible: true,
            noop_white_mask: false,
            extra_blocks: Vec::new(),
            rgba_fill: Some(rgba),
        }
    }

    fn adjustment_or_fill(name: &str, key: &[u8; 4], payload: &[u8], clipping: bool) -> Self {
        let mut extra_blocks = Vec::new();
        push_tagged_block(&mut extra_blocks, key, payload);
        Self {
            name: name.to_owned(),
            rect: [0, 0, 0, 0],
            blend: *b"norm",
            opacity: 255,
            clipping,
            visible: true,
            // Real Photoshop adjustment & fill layers carry a 20-byte default-255 0x0 layer mask!
            noop_white_mask: true,
            extra_blocks,
            rgba_fill: None,
        }
    }
}

fn build_psd_with_saved_composite(
    width: u32,
    height: u32,
    layers: &[SynthPsdLayer],
    composite_rgba_byte: u8,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&1_u16.to_be_bytes()); // PSD v1
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&4_u16.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&8_u16.to_be_bytes());
    out.extend_from_slice(&3_u16.to_be_bytes()); // RGB
    out.extend_from_slice(&0_u32.to_be_bytes()); // Color mode
    out.extend_from_slice(&0_u32.to_be_bytes()); // Image resources

    let mut layer_records = Vec::new();
    let mut channel_blobs = Vec::new();
    layer_records.extend_from_slice(&(layers.len() as i16).to_be_bytes());

    for l in layers {
        let [top, left, bottom, right] = l.rect;
        let lw = (right - left).max(0) as usize;
        let lh = (bottom - top).max(0) as usize;
        let plane_len = 2 + lw * lh;

        layer_records.extend_from_slice(&top.to_be_bytes());
        layer_records.extend_from_slice(&left.to_be_bytes());
        layer_records.extend_from_slice(&bottom.to_be_bytes());
        layer_records.extend_from_slice(&right.to_be_bytes());

        if let Some(fill) = l.rgba_fill {
            layer_records.extend_from_slice(&4_u16.to_be_bytes());
            for (ch_id, byte_val) in [
                (-1_i16, fill[3]),
                (0_i16, fill[0]),
                (1_i16, fill[1]),
                (2_i16, fill[2]),
            ] {
                layer_records.extend_from_slice(&ch_id.to_be_bytes());
                layer_records.extend_from_slice(&(plane_len as u32).to_be_bytes());
                channel_blobs.extend_from_slice(&0_u16.to_be_bytes());
                channel_blobs.extend(std::iter::repeat_n(byte_val, lw * lh));
            }
        } else {
            layer_records.extend_from_slice(&0_u16.to_be_bytes());
        }

        layer_records.extend_from_slice(b"8BIM");
        layer_records.extend_from_slice(&l.blend);
        layer_records.push(l.opacity);
        layer_records.push(u8::from(l.clipping));
        layer_records.push(if l.visible { 0 } else { 0x02 });
        layer_records.push(0);

        let mut extra = Vec::new();
        if l.noop_white_mask {
            extra.extend_from_slice(&20_u32.to_be_bytes());
            extra.extend_from_slice(&[0u8; 16]); // 0x0 bounds
            extra.push(255); // default_color = 255 (white)
            extra.push(0); // flags = 0
            extra.extend_from_slice(&[0u8; 2]); // pad
        } else {
            extra.extend_from_slice(&0_u32.to_be_bytes());
        }
        extra.extend_from_slice(&0_u32.to_be_bytes()); // blending ranges

        let name_bytes = l.name.as_bytes();
        let nlen = name_bytes.len().min(255);
        extra.push(nlen as u8);
        extra.extend_from_slice(&name_bytes[..nlen]);
        let pascal_pad = (4 - ((1 + nlen) % 4)) % 4;
        extra.extend(std::iter::repeat_n(0u8, pascal_pad));
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
    layer_and_mask.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
    layer_and_mask.extend_from_slice(&layer_info);

    out.extend_from_slice(&(layer_and_mask.len() as u32).to_be_bytes());
    out.extend_from_slice(&layer_and_mask);

    // Saved merged composite (raw compression = 0)
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend(std::iter::repeat_n(
        composite_rgba_byte,
        (width as usize) * (height as usize) * 4,
    ));
    out
}

// ============================================================================
// Deliverable 1 Tests: Figma Diamond & Angular True Vector Mesh Paints
// ============================================================================

#[test]
fn figma_diamond_and_angular_gradients_import_as_true_vector_mesh_in_json_and_kiwi() {
    let figma_json = r#"{
      "name": "Diamond & Angular Vector Meshes",
      "schemaVersion": 0,
      "document": {
        "id": "0:0", "name": "Document", "type": "DOCUMENT",
        "children": [{
          "id": "0:1", "name": "Page 1", "type": "CANVAS",
          "children": [
            {
              "id": "9:1", "name": "GemComponent", "type": "COMPONENT",
              "visible": false,
              "absoluteBoundingBox": {"x": 0, "y": 0, "width": 120, "height": 120},
              "children": [{
                "id": "9:2", "name": "GemFacet", "type": "RECTANGLE",
                "cornerRadius": 16,
                "absoluteBoundingBox": {"x": 0, "y": 0, "width": 120, "height": 120},
                "fills": [{
                  "type": "GRADIENT_DIAMOND",
                  "gradientHandlePositions": [
                    {"x": 0.5, "y": 0.5},
                    {"x": 1.0, "y": 0.5},
                    {"x": 0.5, "y": 1.0}
                  ],
                  "gradientStops": [
                    {"position": 0.0, "color": {"r": 1.0, "g": 0.9, "b": 0.2, "a": 1.0}},
                    {"position": 0.5, "color": {"r": 1.0, "g": 0.2, "b": 0.5, "a": 1.0}},
                    {"position": 1.0, "color": {"r": 0.1, "g": 0.1, "b": 0.4, "a": 1.0}}
                  ]
                }]
              }]
            },
            {
              "id": "1:1", "name": "StageFrame", "type": "FRAME",
              "absoluteBoundingBox": {"x": 0, "y": 0, "width": 600, "height": 400},
              "children": [
                {
                  "id": "1:2", "name": "DiamondCard", "type": "RECTANGLE",
                  "absoluteBoundingBox": {"x": 20, "y": 20, "width": 200, "height": 160},
                  "fills": [{
                    "type": "GRADIENT_DIAMOND",
                    "gradientTransform": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    "gradientStops": [
                      {"position": 0.0, "color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}},
                      {"position": 0.5, "color": {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0}},
                      {"position": 1.0, "color": {"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0}}
                    ]
                  }]
                },
                {
                  "id": "1:3", "name": "AngularWheel", "type": "ELLIPSE",
                  "absoluteBoundingBox": {"x": 240, "y": 20, "width": 160, "height": 160},
                  "fills": [{
                    "type": "GRADIENT_ANGULAR",
                    "gradientHandlePositions": [
                      {"x": 0.5, "y": 0.5},
                      {"x": 1.0, "y": 0.5},
                      {"x": 0.5, "y": 1.0}
                    ],
                    "gradientStops": [
                      {"position": 0.0, "color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}},
                      {"position": 0.33, "color": {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0}},
                      {"position": 0.66, "color": {"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0}},
                      {"position": 1.0, "color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}}
                    ]
                  }],
                  "strokes": [{
                    "type": "GRADIENT_DIAMOND",
                    "gradientStops": [
                      {"position": 0.0, "color": {"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0}},
                      {"position": 1.0, "color": {"r": 0.2, "g": 0.2, "b": 0.2, "a": 1.0}}
                    ]
                  }],
                  "strokeWeight": 4.0
                },
                {
                  "id": "1:4", "name": "GemInstance", "type": "INSTANCE",
                  "componentId": "9:1",
                  "absoluteBoundingBox": {"x": 430, "y": 40, "width": 120, "height": 120}
                }
              ]
            }
          ]
        }]
      }
    }"#;

    let opts = FigmaImportOptions {
        import_hidden_layers: false,
        ..FigmaImportOptions::default()
    };
    let (project, report) =
        import_figma_json_str(figma_json, "Meshes", &opts).expect("figma mesh import");
    project.validate().expect("mesh project validates");
    assert!(
        project.resources.is_empty(),
        "diamond and angular vector meshes must not allocate bitmap resources"
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "figma-gradient-approximated"),
        "unexpected figma-gradient-approximated diagnostic: {:?}",
        report.diagnostics
    );
    assert!(
        project.format_version >= 16,
        "vector_mesh requires format >= 16"
    );

    let root = project.symbol(&project.root).unwrap();
    let diamond_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "DiamondCard")
        .expect("DiamondCard layer");
    let diamond_el = &diamond_layer.keyframes[0].elements[0];
    assert!(matches!(diamond_el.kind, ElementKind::Path { .. }));
    let diamond_fx = diamond_layer.keyframes[0]
        .vector_effects(&diamond_el.id)
        .expect("DiamondCard vector_effects");
    let diamond_mesh = diamond_fx
        .vector_mesh
        .as_ref()
        .expect("DiamondCard vector_mesh");
    assert!(
        diamond_mesh.triangles.len() >= 20,
        "expected 4-quadrant multi-stop diamond triangles, got {}",
        diamond_mesh.triangles.len()
    );
    let bounds = diamond_mesh.bounds().expect("diamond bounds");
    assert!(
        bounds[0] <= 0.0 && bounds[1] <= 0.0 && bounds[2] >= 200.0 && bounds[3] >= 160.0,
        "diamond mesh must cover 200x160 shape bounds, got {bounds:?}"
    );

    let angular_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "AngularWheel")
        .expect("AngularWheel layer");
    assert!(
        angular_layer.keyframes[0].vector_effects.len() >= 2,
        "both AngularWheel fill and Diamond stroke outline should carry vector_mesh"
    );

    // Component symbol also carries vector_mesh on its keyframe
    let gem_sym = project
        .symbols
        .iter()
        .find(|s| s.name == "GemComponent")
        .expect("GemComponent symbol");
    assert!(
        gem_sym
            .timeline
            .layers
            .iter()
            .any(|l| !l.keyframes[0].vector_effects.is_empty()),
        "component symbol must preserve vector_effects"
    );

    // Authored stage evaluation must emit DrawItems with `mesh_fill: Some(...)`
    let evaluated = AuthoredScene::evaluate_editable_stage(&project, &project.root, 0, false, true);
    let mesh_draws: usize = evaluated
        .draws
        .iter()
        .filter(|d| d.item.mesh_fill.is_some())
        .count();
    assert!(
        mesh_draws >= 3,
        "expected at least 3 evaluated DrawItems with mesh_fill, got {mesh_draws}"
    );

    // Clipboard paste path must also preserve vector_effects without bitmaps
    let clip_out = import_figma_clipboard(figma_json, None).expect("clipboard");
    clip_out
        .project
        .validate()
        .expect("clipboard project validates");
    assert!(clip_out.project.resources.is_empty());
    assert!(
        !clip_out
            .report
            .diagnostics
            .iter()
            .any(|d| d.code == "figma-gradient-approximated")
    );

    // Also verify binary `.fig` Kiwi decoding of `GRADIENT_DIAMOND` & `GRADIENT_ANGULAR`
    let mut schema = Vec::new();
    write_var_uint(&mut schema, 9);
    // 0: GUID
    write_cstr(&mut schema, "GUID");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["sessionID", "localID"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 1: ParentIndex
    write_cstr(&mut schema, "ParentIndex");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "guid");
    write_var_int(&mut schema, 0);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "position");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 2: Vector
    write_cstr(&mut schema, "Vector");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["x", "y"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 3: Matrix
    write_cstr(&mut schema, "Matrix");
    schema.push(1);
    write_var_uint(&mut schema, 6);
    for (i, n) in ["m00", "m01", "m02", "m10", "m11", "m12"]
        .iter()
        .enumerate()
    {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 4: Color
    write_cstr(&mut schema, "Color");
    schema.push(1);
    write_var_uint(&mut schema, 4);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 5: ColorStop (STRUCT: color Color, position float)
    write_cstr(&mut schema, "ColorStop");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "color");
    write_var_int(&mut schema, 4);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "position");
    write_var_int(&mut schema, -5);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 6: Paint (MESSAGE: 1=type string, 2=stops ColorStop[], 3=transform Matrix)
    write_cstr(&mut schema, "Paint");
    schema.push(2);
    write_var_uint(&mut schema, 3);
    for (tag, n, tid, arr) in [
        (1u64, "type", -6i64, 0u8),
        (2, "stops", 5, 1),
        (3, "transform", 3, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 7: NodeChange
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    write_var_uint(&mut schema, 7);
    for (tag, n, tid, arr) in [
        (1u64, "guid", 0i64, 0u8),
        (2, "parentIndex", 1, 0),
        (3, "type", -6, 0),
        (4, "name", -6, 0),
        (5, "size", 2, 0),
        (6, "transform", 3, 0),
        (7, "fillPaints", 6, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 8: Message
    write_cstr(&mut schema, "Message");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "nodeChanges");
    write_var_int(&mut schema, 7);
    schema.push(1);
    write_var_uint(&mut schema, 1);

    let mut msg = Vec::new();
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 5); // 5 nodes: DOC, CANVAS, FRAME, DiamondRect, AngularEllipse

    // DOC 0:0
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "DOCUMENT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Doc");
    write_var_uint(&mut msg, 0);
    // CANVAS 0:1
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_cstr(&mut msg, "!");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Page");
    write_var_uint(&mut msg, 0);
    // FRAME 1:1
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "!");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "FRAME");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Board");
    write_var_uint(&mut msg, 5);
    write_var_float(&mut msg, 400.0);
    write_var_float(&mut msg, 200.0);
    write_var_uint(&mut msg, 0);

    for (lid, pos_s, ntype, lname, gtype, tx) in [
        (
            2u64,
            "a",
            "RECTANGLE",
            "KiwiDiamond",
            "GRADIENT_DIAMOND",
            10.0f32,
        ),
        (
            3u64,
            "b",
            "ELLIPSE",
            "KiwiAngular",
            "GRADIENT_ANGULAR",
            210.0f32,
        ),
    ] {
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, lid);
        write_var_uint(&mut msg, 2);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, pos_s);
        write_var_uint(&mut msg, 3);
        write_cstr(&mut msg, ntype);
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, lname);
        write_var_uint(&mut msg, 5);
        write_var_float(&mut msg, 160.0);
        write_var_float(&mut msg, 160.0);
        write_var_uint(&mut msg, 6);
        for f in [1.0f32, 0.0, tx, 0.0, 1.0, 20.0] {
            write_var_float(&mut msg, f);
        }
        write_var_uint(&mut msg, 7);
        write_var_uint(&mut msg, 1); // 1 Paint
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, gtype);
        write_var_uint(&mut msg, 2); // 2 stops
        write_var_uint(&mut msg, 2);
        for (c, pos) in [
            ([1.0f32, 0.2, 0.1, 1.0], 0.0f32),
            ([0.1, 0.4, 1.0, 1.0], 1.0),
        ] {
            for ch in c {
                write_var_float(&mut msg, ch);
            }
            write_var_float(&mut msg, pos);
        }
        write_var_uint(&mut msg, 3); // Paint.transform: center at (0.5, 0.5)
        for f in [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0] {
            write_var_float(&mut msg, f);
        }
        write_var_uint(&mut msg, 0); // end Paint
        write_var_uint(&mut msg, 0); // end NodeChange
    }
    write_var_uint(&mut msg, 0); // end Message

    let fig_bytes = wrap_fig_kiwi_zip(&schema, &msg);
    let (kiwi_proj, kiwi_rep) =
        import_figma_bytes(&fig_bytes, "kiwi-mesh", &FigmaImportOptions::default())
            .expect("kiwi mesh import");
    kiwi_proj.validate().expect("kiwi mesh project validates");
    assert!(kiwi_proj.resources.is_empty());
    assert!(
        !kiwi_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "figma-gradient-approximated")
    );
    let kiwi_eval =
        AuthoredScene::evaluate_editable_stage(&kiwi_proj, &kiwi_proj.root, 0, false, true);
    assert_eq!(
        kiwi_eval
            .draws
            .iter()
            .filter(|d| d.item.mesh_fill.is_some())
            .count(),
        2
    );
}

// ============================================================================
// Deliverable 2 Tests: Deeper Nested Figma Auto Layout Reflow
// ============================================================================

#[test]
fn figma_nested_auto_layout_reflow_handles_hug_fill_wrap_baseline_constraints_reverse_z_and_overrides()
 {
    // Notice none of the Auto Layout children specify `absoluteBoundingBox.x/y` or `relativeTransform`
    // (except the `ABSOLUTE` pinned badge inside the initial 600x100 root frame), so the Auto Layout
    // engine must compute all nested sizes, wraps, alignments, overrides, Z-orders, and constraint shifts!
    let figma_json = r#"{
      "name": "Nested Auto Layout Dashboard",
      "schemaVersion": 0,
      "document": {
        "id": "0:0", "name": "Doc", "type": "DOCUMENT",
        "children": [{
          "id": "0:1", "name": "Page 1", "type": "CANVAS",
          "children": [
            {
              "id": "10:1", "name": "PillButton", "type": "COMPONENT",
              "visible": false,
              "layoutMode": "HORIZONTAL",
              "layoutSizingHorizontal": "HUG",
              "layoutSizingVertical": "HUG",
              "paddingLeft": 12, "paddingRight": 12, "paddingTop": 8, "paddingBottom": 8,
              "itemSpacing": 6,
              "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.5, "b": 1.0, "a": 1.0}}],
              "children": [{
                "id": "10:2", "name": "BtnText", "type": "TEXT",
                "characters": "Go",
                "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1, "a": 1}}],
                "style": {"fontFamily": "Inter", "fontSize": 16, "textAutoResize": "WIDTH_AND_HEIGHT"}
              }]
            },
            {
              "id": "1:1", "name": "RootDashboard", "type": "FRAME",
              "size": {"x": 600, "y": 100},
              "layoutMode": "VERTICAL",
              "layoutSizingHorizontal": "FIXED",
              "layoutSizingVertical": "HUG",
              "paddingLeft": 20, "paddingRight": 20, "paddingTop": 20, "paddingBottom": 20,
              "itemSpacing": 16,
              "fills": [{"type": "SOLID", "color": {"r": 0.08, "g": 0.09, "b": 0.12, "a": 1.0}}],
              "children": [
                {
                  "id": "1:2", "name": "HeaderBar", "type": "FRAME",
                  "size": {"x": 100, "y": 20},
                  "layoutMode": "HORIZONTAL",
                  "layoutSizingHorizontal": "FILL",
                  "layoutSizingVertical": "HUG",
                  "primaryAxisAlignItems": "SPACE_BETWEEN",
                  "counterAxisAlignItems": "BASELINE",
                  "paddingLeft": 10, "paddingRight": 10, "paddingTop": 8, "paddingBottom": 8,
                  "itemSpacing": 12,
                  "children": [
                    {
                      "id": "1:3", "name": "BigTitle", "type": "TEXT",
                      "characters": "Overview",
                      "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1, "a": 1}}],
                      "style": {"fontFamily": "Inter", "fontSize": 30, "lineHeightPx": 36, "textAutoResize": "WIDTH_AND_HEIGHT"}
                    },
                    {
                      "id": "1:4", "name": "SmallSub", "type": "TEXT",
                      "characters": "v2.0",
                      "fills": [{"type": "SOLID", "color": {"r": 0.7, "g": 0.7, "b": 0.8, "a": 1}}],
                      "style": {"fontFamily": "Inter", "fontSize": 14, "lineHeightPx": 18, "textAutoResize": "WIDTH_AND_HEIGHT"}
                    },
                    {
                      "id": "1:5", "name": "ActionCTA", "type": "INSTANCE",
                      "componentId": "10:1",
                      "layoutSizingHorizontal": "HUG",
                      "layoutSizingVertical": "HUG",
                      "overrides": [
                        {"id": "10:2", "overriddenFields": ["characters"], "characters": "Upgrade Enterprise Plan"}
                      ]
                    }
                  ]
                },
                {
                  "id": "1:6", "name": "WrapGrid", "type": "FRAME",
                  "size": {"x": 100, "y": 10},
                  "layoutMode": "HORIZONTAL",
                  "layoutWrap": "WRAP",
                  "layoutSizingHorizontal": "FILL",
                  "layoutSizingVertical": "HUG",
                  "itemSpacing": 16,
                  "counterAxisSpacing": 12,
                  "children": [
                    {"id": "1:7", "name": "Card1", "type": "RECTANGLE", "size": {"x": 170, "y": 60}, "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.3, "b": 0.4, "a": 1}}]},
                    {"id": "1:8", "name": "Card2", "type": "RECTANGLE", "size": {"x": 170, "y": 60}, "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.3, "b": 0.4, "a": 1}}]},
                    {"id": "1:9", "name": "Card3", "type": "RECTANGLE", "size": {"x": 170, "y": 60}, "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.3, "b": 0.4, "a": 1}}]},
                    {"id": "1:10", "name": "Card4", "type": "RECTANGLE", "size": {"x": 170, "y": 60}, "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.3, "b": 0.4, "a": 1}}]}
                  ]
                },
                {
                  "id": "1:11", "name": "SplitRow", "type": "FRAME",
                  "size": {"x": 100, "y": 80},
                  "layoutMode": "HORIZONTAL",
                  "layoutSizingHorizontal": "FILL",
                  "layoutSizingVertical": "FIXED",
                  "itemSpacing": 10,
                  "itemReverseZIndex": true,
                  "strokesIncludedInLayout": true,
                  "children": [
                    {
                      "id": "1:12", "name": "SidebarCol", "type": "RECTANGLE",
                      "size": {"x": 50, "y": 20},
                      "layoutGrow": 1,
                      "layoutAlign": "STRETCH",
                      "maxWidth": 150,
                      "strokeWeight": 4,
                      "strokeAlign": "OUTSIDE",
                      "strokes": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1, "a": 1}}],
                      "fills": [{"type": "SOLID", "color": {"r": 0.4, "g": 0.2, "b": 0.6, "a": 1}}]
                    },
                    {
                      "id": "1:13", "name": "MainCol", "type": "RECTANGLE",
                      "size": {"x": 50, "y": 20},
                      "layoutGrow": 1,
                      "layoutAlign": "STRETCH",
                      "minWidth": 200,
                      "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.6, "b": 0.4, "a": 1}}]
                    }
                  ]
                },
                {
                  "id": "1:14", "name": "FloatingCornerBadge", "type": "RECTANGLE",
                  "layoutPositioning": "ABSOLUTE",
                  "relativeTransform": [[1, 0, 556], [0, 1, 72]],
                  "size": {"x": 32, "y": 16},
                  "constraints": {"horizontal": "RIGHT", "vertical": "BOTTOM"},
                  "fills": [{"type": "SOLID", "color": {"r": 1.0, "g": 0.4, "b": 0.1, "a": 1.0}}]
                }
              ]
            }
          ]
        }]
      }
    }"#;

    let opts = FigmaImportOptions {
        import_hidden_layers: false,
        ..FigmaImportOptions::default()
    };
    let (project, _report) =
        import_figma_json_str(figma_json, "AutoLayout", &opts).expect("autolayout import");
    project.validate().expect("autolayout project validates");

    // RootDashboard width stays 600, and HUG height expands well past initial 100px
    // (20 top + ~52 header + 16 + 132 wrap grid + 16 + 80 split row + 20 bottom > 300px)
    assert_eq!(project.width, 600);
    assert!(
        project.height >= 310,
        "expected HUG RootDashboard height >= 310, got {}",
        project.height
    );

    let root = project.symbol(&project.root).unwrap();
    let find_el = |name: &str| {
        root.timeline
            .layers
            .iter()
            .enumerate()
            .find_map(|(li, l)| {
                l.keyframes
                    .first()?
                    .elements
                    .iter()
                    .find(|e| e.name == name)
                    .map(|e| (li, e))
            })
            .unwrap_or_else(|| panic!("missing element {name}"))
    };

    // 1. Header SPACE_BETWEEN + BASELINE + Overridden HUG Instance:
    let (_, big_title) = find_el("BigTitle");
    let (_, small_sub) = find_el("SmallSub");
    let (_, action_cta) = find_el("ActionCTA");
    assert!(
        (big_title.transform.x - 30.0).abs() < 1.0,
        "BigTitle x = {} (expected 20 root pad + 10 header pad = 30)",
        big_title.transform.x
    );
    // SmallSub (14px) must sit lower than BigTitle (30px) to align baselines!
    assert!(
        small_sub.transform.y > big_title.transform.y + 8.0,
        "BASELINE alignment must shift 14px SmallSub y ({}) below 30px BigTitle y ({})",
        small_sub.transform.y,
        big_title.transform.y
    );
    // Master PillButton ("Go") was ~42px wide; overridden "Upgrade Enterprise Plan" grows > 180px,
    // and SPACE_BETWEEN right-aligns ActionCTA against x = 600 - 20 - 10 = 570!
    let ElementKind::Instance { symbol_id, .. } = &action_cta.kind else {
        panic!("expected ActionCTA instance");
    };
    let cta_sym = project.symbol(symbol_id).expect("specialized CTA symbol");
    let cta_bg = &cta_sym.timeline.layers[0].keyframes[0].elements[0];
    let ElementKind::Rectangle { width: cta_w, .. } = cta_bg.kind else {
        panic!("expected CTA background rectangle");
    };
    assert!(
        cta_w > 160.0,
        "text override must expand HUG PillButton width (>160px), got {cta_w}"
    );
    assert!(
        ((action_cta.transform.x + cta_w) - 570.0).abs() < 2.0,
        "SPACE_BETWEEN must place ActionCTA right edge at 570, got {}",
        action_cta.transform.x + cta_w
    );

    // 2. WrapGrid (`560` wide): Card1, Card2, Card3 on row 1 (`x = 20, 206, 392`), Card4 wraps to row 2 (`x = 20, y = row1_y + 72`)!
    let (_, card1) = find_el("Card1");
    let (_, card2) = find_el("Card2");
    let (_, card3) = find_el("Card3");
    let (_, card4) = find_el("Card4");
    assert!((card1.transform.x - 20.0).abs() < 1.0);
    assert!((card2.transform.x - 206.0).abs() < 1.0);
    assert!((card3.transform.x - 392.0).abs() < 1.0);
    assert!((card4.transform.x - 20.0).abs() < 1.0);
    assert!(
        (card4.transform.y - (card1.transform.y + 72.0)).abs() < 1.0,
        "Card4 must wrap to second row (+60 height + 12 counterAxisSpacing = +72), got {} vs {}",
        card4.transform.y,
        card1.transform.y
    );

    // 3. SplitRow: `strokesIncludedInLayout` + `maxWidth: 150` + `STRETCH` + `itemReverseZIndex: true`:
    let (sidebar_li, sidebar) = find_el("SidebarCol");
    let (main_li, main_col) = find_el("MainCol");
    // itemReverseZIndex puts first flow item (SidebarCol) ABOVE second flow item (MainCol)!
    assert!(
        sidebar_li > main_li,
        "itemReverseZIndex must place SidebarCol layer ({sidebar_li}) above MainCol layer ({main_li})"
    );
    // SidebarCol has 4px OUTSIDE stroke included in layout -> x = 20 + 4 = 24, inner width clamped to maxWidth = 150, stretched height = 80 - 8 = 72!
    assert!(
        (sidebar.transform.x - 24.0).abs() < 1.0,
        "strokesIncludedInLayout must offset OUTSIDE stroke by +4px (20 + 4 = 24), got {}",
        sidebar.transform.x
    );
    let (sb_w, sb_h) = match &sidebar.kind {
        ElementKind::Rectangle { width, height, .. } => (*width, *height),
        ElementKind::Path { commands, .. } => {
            let b = neoflash_core::path_bounds(commands).unwrap().unwrap();
            (b.x_max - b.x_min, b.y_max - b.y_min)
        }
        other => panic!("unexpected kind for SidebarCol: {other:?}"),
    };
    assert!((sb_w - 150.0).abs() < 1.0, "SidebarCol width = {sb_w}");
    assert!((sb_h - 72.0).abs() < 1.0, "SidebarCol height = {sb_h}");
    // Remaining primary width in 560px SplitRow: 560 - (150 + 8) - 10 gap = 392px for MainCol!
    let ElementKind::Rectangle {
        width: main_w,
        height: main_h,
        ..
    } = main_col.kind
    else {
        panic!("expected Rectangle for MainCol");
    };
    assert!((main_w - 392.0).abs() < 1.0, "MainCol width = {main_w}");
    assert!(
        (main_h - 80.0).abs() < 1.0,
        "MainCol stretched height = {main_h}"
    );

    // 4. ABSOLUTE child with RIGHT + BOTTOM constraints:
    // Originally at (556, 72) with size (32, 16) inside 600x100 -> right margin = 12, bottom margin = 12.
    // After RootDashboard grew to `project.height`, badge y must equal `project.height - 12 - 16`!
    let (_, badge) = find_el("FloatingCornerBadge");
    assert!((badge.transform.x - 556.0).abs() < 1.0);
    assert!(
        (badge.transform.y - (f64::from(project.height) - 28.0)).abs() < 1.5,
        "BOTTOM-constrained ABSOLUTE badge must move with HUG parent height ({} vs expected {})",
        badge.transform.y,
        f64::from(project.height) - 28.0
    );

    // 5. Verify binary `.fig` Kiwi Auto Layout (`stackMode`, `stackWrap`, `stackSpacing`,
    //    `stackCounterSpacing`, `stackHorizontalPadding`, `stackVerticalPadding`, `stackChildPrimaryGrow`)
    let mut schema = Vec::new();
    write_var_uint(&mut schema, 7);
    write_cstr(&mut schema, "GUID");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["sessionID", "localID"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    write_cstr(&mut schema, "ParentIndex");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "guid");
    write_var_int(&mut schema, 0);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "position");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "Vector");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["x", "y"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    write_cstr(&mut schema, "Color");
    schema.push(1);
    write_var_uint(&mut schema, 4);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    write_cstr(&mut schema, "Paint");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "type");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "color");
    write_var_int(&mut schema, 3);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    write_var_uint(&mut schema, 14);
    for (tag, n, tid, arr) in [
        (1u64, "guid", 0i64, 0u8),
        (2, "parentIndex", 1, 0),
        (3, "type", -6, 0),
        (4, "name", -6, 0),
        (5, "size", 2, 0),
        (6, "fillPaints", 4, 1),
        (7, "stackMode", -6, 0),
        (8, "stackSpacing", -5, 0),
        (9, "stackHorizontalPadding", -5, 0),
        (10, "stackVerticalPadding", -5, 0),
        (11, "stackCounterSizing", -6, 0),
        (12, "stackWrap", -6, 0),
        (13, "stackCounterSpacing", -5, 0),
        (14, "stackChildPrimaryGrow", -5, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    write_cstr(&mut schema, "Message");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "nodeChanges");
    write_var_int(&mut schema, 5);
    schema.push(1);
    write_var_uint(&mut schema, 1);

    let mut msg = Vec::new();
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 6); // DOC, CANVAS, StackFrame(300px wide, WRAP, HUG height), 3 items of 120x40
    // DOC
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "DOCUMENT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Doc");
    write_var_uint(&mut msg, 0);
    // CANVAS
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_cstr(&mut msg, "!");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Page");
    write_var_uint(&mut msg, 0);
    // StackFrame 1:1 (width 300, initial height 20, HORIZONTAL + WRAP + pad 10 + gap 10 + counterGap 8 + HUG counter)
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "!");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "FRAME");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "KiwiStack");
    write_var_uint(&mut msg, 5);
    write_var_float(&mut msg, 300.0);
    write_var_float(&mut msg, 20.0);
    write_var_uint(&mut msg, 7);
    write_cstr(&mut msg, "HORIZONTAL");
    write_var_uint(&mut msg, 8);
    write_var_float(&mut msg, 10.0);
    write_var_uint(&mut msg, 9);
    write_var_float(&mut msg, 10.0);
    write_var_uint(&mut msg, 10);
    write_var_float(&mut msg, 10.0);
    write_var_uint(&mut msg, 11);
    write_cstr(&mut msg, "RESIZE_TO_FIT_WITH_IMPLICIT_SIZE");
    write_var_uint(&mut msg, 12);
    write_cstr(&mut msg, "WRAP");
    write_var_uint(&mut msg, 13);
    write_var_float(&mut msg, 8.0);
    write_var_uint(&mut msg, 0);

    for (lid, pos_s, iname) in [
        (2u64, "a", "KItem1"),
        (3, "b", "KItem2"),
        (4, "c", "KItem3"),
    ] {
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, lid);
        write_var_uint(&mut msg, 2);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, pos_s);
        write_var_uint(&mut msg, 3);
        write_cstr(&mut msg, "RECTANGLE");
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, iname);
        write_var_uint(&mut msg, 5);
        write_var_float(&mut msg, 120.0);
        write_var_float(&mut msg, 40.0);
        write_var_uint(&mut msg, 6);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, "SOLID");
        write_var_uint(&mut msg, 2);
        for c in [0.3f32, 0.7, 0.9, 1.0] {
            write_var_float(&mut msg, c);
        }
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 0);
    }
    write_var_uint(&mut msg, 0);

    let kiwi_bytes = wrap_fig_kiwi_zip(&schema, &msg);
    let (kp, _) = import_figma_bytes(&kiwi_bytes, "kiwi-stack", &FigmaImportOptions::default())
        .expect("kiwi autolayout");
    kp.validate().expect("kiwi autolayout validates");
    // Available width = 300 - 20 = 280 -> KItem1 (120) + gap (10) + KItem2 (120) = 250 fits on row 1;
    // KItem3 wraps to row 2 -> HUG height = 10 + 40 + 8 + 40 + 10 = 108!
    assert_eq!(kp.width, 300);
    assert_eq!(kp.height, 108);
    let kroot = kp.symbol(&kp.root).unwrap();
    let ki3 = kroot
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "KItem3")
        .unwrap();
    assert!((ki3.keyframes[0].elements[0].transform.x - 10.0).abs() < 1.0);
    assert!((ki3.keyframes[0].elements[0].transform.y - 58.0).abs() < 1.0);
}

// ============================================================================
// Deliverable 3A Tests: PSD `GdFl` & `SoCo` Fill Layers + `vmsk` v3 Vector Masks
// ============================================================================

#[test]
fn psd_gdfl_gradient_fill_and_soco_solid_fill_with_vmsk_preserve_editable_vector_layers() {
    // 1. Full-canvas 0x0 `SoCo` layer
    let soco_full = SynthPsdLayer::adjustment_or_fill(
        "FullCanvasSolid",
        b"SoCo",
        &build_soco_payload(24, 48, 96),
        false,
    );
    // 2. `SoCo` with version-3 `vmsk` cubic Bézier contour (`EvenOdd`)
    let mut soco_masked = SynthPsdLayer::adjustment_or_fill(
        "BezierBadgeSolid",
        b"SoCo",
        &build_soco_payload(240, 80, 40),
        false,
    );
    push_tagged_block(
        &mut soco_masked.extra_blocks,
        b"vmsk",
        &build_vmsk_v3_cubic_shape(100.0, 100.0, true),
    );
    // 3. `GdFl` Linear, Radial, Reflected
    let gdfl_linear = SynthPsdLayer::adjustment_or_fill(
        "LinearSky",
        b"GdFl",
        &build_gdfl_payload(
            "Lnr ",
            45.0,
            100.0,
            false,
            [0.0, 0.0],
            &[(0, [255, 0, 0]), (4096, [0, 0, 255])],
            &[(0, 100.0), (4096, 80.0)],
        ),
        false,
    );
    let gdfl_reflected = SynthPsdLayer::adjustment_or_fill(
        "ReflectedHorizon",
        b"GdFl",
        &build_gdfl_payload(
            "Rflc",
            90.0,
            120.0,
            true,
            [10.0, -5.0],
            &[(0, [20, 200, 120]), (4096, [250, 220, 40])],
            &[(0, 100.0), (4096, 100.0)],
        ),
        false,
    );
    let gdfl_radial = SynthPsdLayer::adjustment_or_fill(
        "RadialGlowFill",
        b"GdFl",
        &build_gdfl_payload(
            "Rdl ",
            0.0,
            80.0,
            false,
            [0.0, 0.0],
            &[(0, [255, 255, 255]), (4096, [0, 0, 0])],
            &[(0, 100.0), (4096, 0.0)],
        ),
        false,
    );
    // 4. `GdFl` Diamond (clipped to `vmsk` Bézier contour!) & Angular
    let mut gdfl_diamond = SynthPsdLayer::adjustment_or_fill(
        "DiamondMeshFill",
        b"GdFl",
        &build_gdfl_payload(
            "Dmnd",
            30.0,
            100.0,
            false,
            [0.0, 0.0],
            &[
                (0, [255, 220, 40]),
                (2048, [240, 40, 120]),
                (4096, [20, 10, 80]),
            ],
            &[(0, 100.0), (4096, 100.0)],
        ),
        false,
    );
    push_tagged_block(
        &mut gdfl_diamond.extra_blocks,
        b"vmsk",
        &build_vmsk_v3_cubic_shape(100.0, 100.0, false),
    );
    let gdfl_angular = SynthPsdLayer::adjustment_or_fill(
        "AngularSweepFill",
        b"GdFl",
        &build_gdfl_payload(
            "Angl",
            0.0,
            100.0,
            false,
            [0.0, 0.0],
            &[
                (0, [255, 0, 0]),
                (1365, [0, 255, 0]),
                (2730, [0, 0, 255]),
                (4096, [255, 0, 0]),
            ],
            &[(0, 100.0), (4096, 100.0)],
        ),
        false,
    );

    let psd_bytes = build_psd_with_saved_composite(
        100,
        100,
        &[
            soco_full,
            soco_masked,
            gdfl_linear,
            gdfl_reflected,
            gdfl_radial,
            gdfl_diamond,
            gdfl_angular,
        ],
        200,
    );

    // Import with `PsdLayerMapping::Automatic` — must NOT collapse to merged composite!
    let (project, report) =
        import_psd_bytes(&psd_bytes, "VectorFills.psd", &PsdImportOptions::default())
            .expect("PSD vector fills import");
    project.validate().expect("PSD vector fills validate");
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-merged-appearance"),
        "SoCo/GdFl/vmsk layers must NOT collapse to composite: {:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-vector-mask-approximated"
                || d.code == "psd-layer-bitmap-too-large"),
        "unexpected diagnostic: {:?}",
        report.diagnostics
    );
    assert!(
        project.resources.is_empty(),
        "vector fill layers must allocate zero bitmap resources"
    );

    let root = project.symbol(&project.root).unwrap();
    assert_eq!(root.timeline.layers.len(), 7);

    // Layer 0: FullCanvasSolid -> 100x100 Rectangle
    match &root.timeline.layers[0].keyframes[0].elements[0].kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            ..
        } => {
            assert_eq!((*width, *height), (100.0, 100.0));
            assert_eq!(*fill, [24, 48, 96, 255]);
        }
        other => panic!("expected full-canvas Rectangle for 0x0 SoCo, got {other:?}"),
    }

    // Layer 1: BezierBadgeSolid -> Path with CubicTo & EvenOdd
    match &root.timeline.layers[1].keyframes[0].elements[0].kind {
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            ..
        } => {
            assert_eq!(*fill_rule, FillRule::EvenOdd);
            assert_eq!(*fill, [240, 80, 40, 255]);
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, PathCommand::CubicTo { .. }))
            );
        }
        other => panic!("expected cubic Path for vmsk SoCo, got {other:?}"),
    }

    // Layer 2..=4: Linear, Reflected, Radial GradientPaint
    for (idx, expected_kind, min_stops) in [
        (2usize, GradientKind::Linear, 2usize),
        (3, GradientKind::Linear, 3), // Reflected mirrors stops around center!
        (4, GradientKind::Radial, 2),
    ] {
        match &root.timeline.layers[idx].keyframes[0].elements[0].kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(gp),
                ..
            } => {
                assert_eq!(gp.kind, expected_kind);
                assert!(gp.stops.len() >= min_stops);
            }
            other => {
                panic!("expected PaintedPath with GradientPaint at layer {idx}, got {other:?}")
            }
        }
    }
    // Verify Reflected gradient symmetry: first and last stop match!
    if let ElementKind::PaintedPath {
        fill_gradient: Some(rflc),
        ..
    } = &root.timeline.layers[3].keyframes[0].elements[0].kind
    {
        assert_eq!(
            rflc.stops.first().unwrap().color,
            rflc.stops.last().unwrap().color
        );
    }

    // Layer 5 & 6: DiamondMeshFill (with vmsk CubicTo) & AngularSweepFill -> VectorMeshPaint!
    for idx in [5usize, 6] {
        let kf = &root.timeline.layers[idx].keyframes[0];
        let el = &kf.elements[0];
        let vfx = kf
            .vector_effects(&el.id)
            .expect("Dmnd/Angl GdFl must attach ElementVectorEffects");
        assert!(
            vfx.vector_mesh
                .as_ref()
                .is_some_and(|m| m.triangles.len() >= 12)
        );
    }
    let stage = AuthoredScene::evaluate_editable_stage(&project, &project.root, 0, false, true);
    assert_eq!(
        stage
            .draws
            .iter()
            .filter(|d| d.item.mesh_fill.is_some())
            .count(),
        2
    );

    // Also verify MovieClipSymbols and Keyframes mapping modes validate cleanly with vector_effects
    for mapping in [
        PsdLayerMapping::MovieClipSymbols,
        PsdLayerMapping::Keyframes,
    ] {
        let (mp, _) = import_psd_bytes(
            &psd_bytes,
            "VectorFills.psd",
            &PsdImportOptions {
                layer_mapping: mapping,
                ..PsdImportOptions::default()
            },
        )
        .unwrap();
        mp.validate()
            .expect("MovieClipSymbols/Keyframes GdFl validates");
    }
}

// ============================================================================
// Deliverable 3B Tests: PSD Clipped & Unclipped Adjustment Layers
// ============================================================================

#[test]
fn psd_clipped_and_unclipped_adjustment_layers_preserve_separate_editable_timeline_layers() {
    // 1. Test `FilterSpec` Adjustment Layers (`nvrt`, `brit`, `hue2`, `bw  `/`blwh`) — clipped & unclipped!
    let base_photo = SynthPsdLayer::pixel("BasePhoto", [0, 0, 16, 16], [200, 100, 50, 255]);
    // Clipped Invert (`nvrt`) -> attaches exact `[-1,0,0,0,255,...]` ColorMatrix to `BasePhoto`!
    let clipped_invert = SynthPsdLayer::adjustment_or_fill("ClippedInvert", b"nvrt", &[], true);

    let mid_card = SynthPsdLayer::pixel("MidCard", [2, 2, 14, 14], [80, 160, 220, 255]);

    // Unclipped Brightness/Contrast (`brit` v16 descriptor) -> wraps `BasePhoto` + `MidCard` in a Folder!
    let mut brit_desc = DescBuilder::new("BrgC");
    brit_desc.long("Brgh", 25).long("Cntr", 15);
    let unclipped_brit = SynthPsdLayer::adjustment_or_fill(
        "GlobalBrightness",
        b"brit",
        &brit_desc.encode_v16_block(),
        false,
    );

    // Unclipped Hue/Saturation (`hue2` binary v2: master hue=+30, sat=+20, light=-10) -> outer Folder!
    let mut hue2_bytes = Vec::new();
    hue2_bytes.extend_from_slice(&2_u16.to_be_bytes()); // version = 2
    hue2_bytes.push(0); // colorize = 0
    hue2_bytes.push(0); // pad
    // colorize defaults (ignored when colorize == 0)
    for v in [210_i16, 25, 0] {
        hue2_bytes.extend_from_slice(&v.to_be_bytes());
    }
    // master tuple: [hue_deg, sat, light]
    for v in [30_i16, 20, -10] {
        hue2_bytes.extend_from_slice(&v.to_be_bytes());
    }
    let unclipped_hue2 =
        SynthPsdLayer::adjustment_or_fill("GlobalHueSat", b"hue2", &hue2_bytes, false);

    // Hidden Black & White (`bw  `) -> Folder stays visible so underlying layers aren't hidden,
    // while its `FilterSpec::ColorMatrix` has `is_enabled() == false`!
    let mut hidden_bw = SynthPsdLayer::adjustment_or_fill("HiddenBW", b"bw  ", &[], false);
    hidden_bw.visible = false;

    // Layer ABOVE all adjustment layers — must NOT be wrapped inside any adjustment folder!
    let top_badge = SynthPsdLayer::pixel("TopUnadjustedBadge", [0, 0, 8, 8], [255, 255, 255, 255]);

    let psd_filter_adj = build_psd_with_saved_composite(
        16,
        16,
        &[
            base_photo,
            clipped_invert,
            mid_card,
            unclipped_brit,
            unclipped_hue2,
            hidden_bw,
            top_badge,
        ],
        128,
    );

    let (proj_f, rep_f) = import_psd_bytes(
        &psd_filter_adj,
        "FilterAdjustments.psd",
        &PsdImportOptions::default(),
    )
    .expect("PSD filter adjustments import");
    proj_f.validate().expect("PSD filter adjustments validate");
    assert!(
        !rep_f
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-merged-appearance"
                || d.code == "psd-adjustment-layer-approximated"),
        "supported adjustment layers must not collapse or emit approximation warnings: {:?}",
        rep_f.diagnostics
    );

    let root_f = proj_f.symbol(&proj_f.root).unwrap();
    fn find_layer<'a>(p: &'a neoflash_core::Project, name: &str) -> &'a neoflash_core::Layer {
        p.symbol(&p.root)
            .unwrap()
            .timeline
            .layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("missing layer {name}"))
    }

    let base_l = find_layer(&proj_f, "BasePhoto");
    assert_eq!(
        base_l.properties.role,
        LayerRole::Normal,
        "clipped adjustment must not turn BasePhoto into an untargeted Mask layer"
    );
    let expected_invert = [
        -1.0, 0.0, 0.0, 0.0, 255.0, 0.0, -1.0, 0.0, 0.0, 255.0, 0.0, 0.0, -1.0, 0.0, 255.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
    ];
    assert_eq!(
        base_l.keyframes[0].elements[0].filters,
        vec![FilterSpec::ColorMatrix {
            matrix: expected_invert
        }]
    );

    // Verify nested Folder chain: BasePhoto & MidCard -> GlobalBrightness -> GlobalHueSat -> HiddenBW
    let brit_folder = find_layer(&proj_f, "GlobalBrightness");
    let hue_folder = find_layer(&proj_f, "GlobalHueSat");
    let bw_folder = find_layer(&proj_f, "HiddenBW");
    let top_l = find_layer(&proj_f, "TopUnadjustedBadge");
    assert_eq!(brit_folder.properties.role, LayerRole::Folder);
    assert_eq!(brit_folder.properties.filters.len(), 1);
    assert_eq!(
        base_l.properties.parent.as_deref(),
        Some(brit_folder.id.as_str())
    );
    assert_eq!(
        brit_folder.properties.parent.as_deref(),
        Some(hue_folder.id.as_str())
    );
    assert_eq!(
        hue_folder.properties.parent.as_deref(),
        Some(bw_folder.id.as_str())
    );
    assert!(bw_folder.visible);
    assert!(!bw_folder.properties.filters[0].is_enabled());
    assert!(
        top_l.properties.parent.is_none(),
        "TopUnadjustedBadge sits above adjustments and must not be wrapped"
    );
    assert!(
        !root_f
            .timeline
            .layers
            .iter()
            .any(|l| l.name == "ClippedInvert")
    );

    // 2. Test LUT & Tonal Adjustment Layers (`levl`, `curv` v1 & v4, `post`, `thrs`, `blnc`, `expA`, `vibr`)
    //    both clipped and unclipped, verifying exact decoded RGBA pixel output!
    let decode_layer_px = |p: &neoflash_core::Project, lname: &str| -> [u8; 4] {
        let l = find_layer(p, lname);
        match &l.keyframes[0].elements[0].kind {
            ElementKind::Bitmap { resource_id, .. } => {
                let res = p.resources.iter().find(|r| &r.id == resource_id).unwrap();
                let img = res.decode_image().unwrap();
                [img.rgba[0], img.rgba[1], img.rgba[2], img.rgba[3]]
            }
            ElementKind::Rectangle { fill, .. } => *fill,
            other => panic!("unexpected element kind on {lname}: {other:?}"),
        }
    };

    // Layer A: Clipped Levels (`in_black=50, in_white=200, out_black=0, out_white=255, gamma=100`)
    // Input `[125, 50, 200, 255]` -> `[(125-50)/150 * 255 = 128, 0, 255, 255]`!
    let mut levl_bytes = Vec::new();
    levl_bytes.extend_from_slice(&2_u16.to_be_bytes());
    for (ib, iw, ob, ow, gam) in [
        (50_u16, 200_u16, 0_u16, 255_u16, 100_u16),
        (0, 255, 0, 255, 100),
        (0, 255, 0, 255, 100),
        (0, 255, 0, 255, 100),
    ] {
        for w in [ib, iw, ob, ow, gam] {
            levl_bytes.extend_from_slice(&w.to_be_bytes());
        }
    }
    let px_levels = SynthPsdLayer::pixel("LevelsTarget", [0, 0, 4, 4], [125, 50, 200, 255]);
    let adj_levels = SynthPsdLayer::adjustment_or_fill("LevelsClipped", b"levl", &levl_bytes, true);

    // Layer B: Clipped Curves (`version = 4`, master bitmask = 1, 3 points `(0->0), (128->200), (255->255)`)
    let mut curv_bytes = Vec::new();
    curv_bytes.extend_from_slice(&4_u16.to_be_bytes());
    curv_bytes.extend_from_slice(&1_u32.to_be_bytes()); // master channel
    curv_bytes.extend_from_slice(&3_u16.to_be_bytes()); // 3 points (output, input)
    for (out_v, in_v) in [(0_u16, 0_u16), (200, 128), (255, 255)] {
        curv_bytes.extend_from_slice(&out_v.to_be_bytes());
        curv_bytes.extend_from_slice(&in_v.to_be_bytes());
    }
    let px_curves = SynthPsdLayer::pixel("CurvesTarget", [0, 0, 4, 4], [128, 0, 255, 255]);
    let adj_curves = SynthPsdLayer::adjustment_or_fill("CurvesClipped", b"curv", &curv_bytes, true);

    // Layer C: Clipped Posterize (`levels = 2`) -> `[60, 190, 127, 255]` becomes `[0, 255, 0, 255]`!
    let mut post_bytes = Vec::new();
    post_bytes.extend_from_slice(&2_u16.to_be_bytes());
    post_bytes.extend_from_slice(&0_u16.to_be_bytes());
    let px_post = SynthPsdLayer::pixel("PosterizeTarget", [0, 0, 4, 4], [60, 190, 127, 255]);
    let adj_post =
        SynthPsdLayer::adjustment_or_fill("PosterizeClipped", b"post", &post_bytes, true);

    // Layer D: Clipped Threshold (`level = 128`) -> luma of `[200, 200, 200]` >= 128 -> `[255, 255, 255, 255]`!
    let mut thrs_bytes = Vec::new();
    thrs_bytes.extend_from_slice(&128_u16.to_be_bytes());
    thrs_bytes.extend_from_slice(&0_u16.to_be_bytes());
    let px_thrs = SynthPsdLayer::pixel("ThresholdTarget", [0, 0, 4, 4], [90, 90, 90, 255]);
    let adj_thrs =
        SynthPsdLayer::adjustment_or_fill("ThresholdClipped", b"thrs", &thrs_bytes, true);

    // Layer E: Clipped Color Balance (`midtones = [+80, 0, -80]`, preserve luminosity = 0)
    let mut blnc_bytes = Vec::new();
    for v in [0_i16, 0, 0, 80, 0, -80, 0, 0, 0] {
        blnc_bytes.extend_from_slice(&v.to_be_bytes());
    }
    blnc_bytes.extend_from_slice(&0_u16.to_be_bytes());
    let px_blnc = SynthPsdLayer::pixel("BalanceTarget", [0, 0, 4, 4], [128, 128, 128, 255]);
    let adj_blnc = SynthPsdLayer::adjustment_or_fill("BalanceClipped", b"blnc", &blnc_bytes, true);

    // Layer F: Clipped Exposure (`exposure = +1.0` -> 2x multiplier!) & Vibrance (`vibr` +80)
    let mut exp_bytes = Vec::new();
    exp_bytes.extend_from_slice(&1_u16.to_be_bytes());
    exp_bytes.extend_from_slice(&1.0_f32.to_be_bytes());
    exp_bytes.extend_from_slice(&0.0_f32.to_be_bytes());
    exp_bytes.extend_from_slice(&1.0_f32.to_be_bytes());
    let px_exp = SynthPsdLayer::pixel("ExposureTarget", [0, 0, 4, 4], [64, 80, 100, 255]);
    let adj_exp = SynthPsdLayer::adjustment_or_fill("ExposureClipped", b"expA", &exp_bytes, true);

    let mut vibr_desc = DescBuilder::new("vibrance");
    vibr_desc.long("vibrance", 80).long("Strt", 20);
    let adj_vibr = SynthPsdLayer::adjustment_or_fill(
        "VibranceClipped",
        b"vibr",
        &vibr_desc.encode_v16_block(),
        true,
    );

    let psd_lut = build_psd_with_saved_composite(
        8,
        8,
        &[
            px_levels, adj_levels, px_curves, adj_curves, px_post, adj_post, px_thrs, adj_thrs,
            px_blnc, adj_blnc, px_exp, adj_exp, adj_vibr,
        ],
        100,
    );

    let (proj_l, rep_l) =
        import_psd_bytes(&psd_lut, "LutAdjustments.psd", &PsdImportOptions::default())
            .expect("PSD LUT adjustments import");
    proj_l.validate().expect("PSD LUT adjustments validate");
    assert!(
        !rep_l
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-merged-appearance"
                || d.code == "psd-adjustment-layer-approximated"),
        "unexpected diagnostic: {:?}",
        rep_l.diagnostics
    );
    assert_eq!(
        proj_l.symbol(&proj_l.root).unwrap().timeline.layers.len(),
        6
    );

    let lv_px = decode_layer_px(&proj_l, "LevelsTarget");
    assert!((i32::from(lv_px[0]) - 128).abs() <= 1 && lv_px[1] == 0 && lv_px[2] == 255);
    assert_eq!(decode_layer_px(&proj_l, "CurvesTarget"), [200, 0, 255, 255]);
    assert_eq!(
        decode_layer_px(&proj_l, "PosterizeTarget"),
        [0, 255, 0, 255]
    );
    assert_eq!(decode_layer_px(&proj_l, "ThresholdTarget"), [0, 0, 0, 255]);
    let bl_px = decode_layer_px(&proj_l, "BalanceTarget");
    assert!(
        bl_px[0] > 160 && bl_px[2] < 95,
        "Color Balance +cyan_red/-yellow_blue must warm midtones, got {bl_px:?}"
    );
    let ex_px = decode_layer_px(&proj_l, "ExposureTarget");
    assert!(
        ex_px[2] > 200 && (ex_px[2] - ex_px[0]) > 72,
        "Exposure +1EV + Vibrance must brighten and boost chroma, got {ex_px:?}"
    );

    // 3. Unclipped LUT adjustment (`post` Posterize 2 levels) across multiple underlying pixel + `SoCo` layers
    //    must adjust underlying layers while leaving layers ABOVE it untouched, preserving separate editable layers!
    let bg_solid = SynthPsdLayer::adjustment_or_fill(
        "BgSolid",
        b"SoCo",
        &build_soco_payload(40, 210, 90),
        false,
    );
    let mid_px = SynthPsdLayer::pixel("MidSprite", [0, 0, 4, 4], [200, 50, 180, 255]);
    let unclipped_post =
        SynthPsdLayer::adjustment_or_fill("UnclippedPosterize", b"post", &post_bytes, false);
    let above_px = SynthPsdLayer::pixel("AboveUntouched", [0, 0, 4, 4], [100, 150, 200, 255]);

    let psd_unclipped_lut =
        build_psd_with_saved_composite(8, 8, &[bg_solid, mid_px, unclipped_post, above_px], 90);
    let (proj_u, rep_u) = import_psd_bytes(
        &psd_unclipped_lut,
        "UnclippedLut.psd",
        &PsdImportOptions::default(),
    )
    .expect("unclipped LUT import");
    proj_u.validate().expect("unclipped LUT validates");
    assert!(
        !rep_u
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-merged-appearance")
    );
    assert_eq!(
        proj_u.symbol(&proj_u.root).unwrap().timeline.layers.len(),
        3
    );
    assert_eq!(decode_layer_px(&proj_u, "BgSolid"), [0, 255, 0, 255]);
    assert_eq!(decode_layer_px(&proj_u, "MidSprite"), [255, 0, 255, 255]);
    assert_eq!(
        decode_layer_px(&proj_u, "AboveUntouched"),
        [100, 150, 200, 255]
    );

    // 4. Malformed adjustment payload (`curv` with 2 garbage bytes) emits `psd-adjustment-layer-approximated` safely!
    let bad_curv = SynthPsdLayer::adjustment_or_fill("BrokenCurves", b"curv", &[0xFF, 0x00], false);
    let psd_bad = build_psd_with_saved_composite(
        8,
        8,
        &[
            SynthPsdLayer::pixel("Base", [0, 0, 4, 4], [10, 20, 30, 255]),
            bad_curv,
        ],
        80,
    );
    let (proj_bad, rep_bad) = import_psd_bytes(
        &psd_bad,
        "Broken.psd",
        &PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            ..PsdImportOptions::default()
        },
    )
    .expect("malformed adjustment must not fail container import");
    proj_bad
        .validate()
        .expect("broken adjustment project validates");
    assert!(
        rep_bad
            .diagnostics
            .iter()
            .any(|d| d.code == "psd-adjustment-layer-approximated")
    );
}

// ============================================================================
// Adversarial Audit Regression Tests (`[P0]` & `[P2]`)
// ============================================================================

#[test]
fn t501_adversarial_cyclic_and_diamond_dag_reflow_extreme_floats_and_huge_vmsk_bounds() {
    use std::time::{Duration, Instant};

    // 1a. Cyclic `INSTANCE` (`C1` containing two `INSTANCE` children of `C1` with `symbol_overrides`)
    let cyclic_json = r#"{
      "name": "Cyclic Instance Bomb",
      "schemaVersion": 0,
      "document": {
        "id": "0:0", "name": "Doc", "type": "DOCUMENT",
        "children": [{
          "id": "0:1", "name": "Page 1", "type": "CANVAS",
          "children": [
            {
              "id": "90:1", "name": "C1", "type": "COMPONENT",
              "layoutMode": "HORIZONTAL",
              "layoutSizingHorizontal": "HUG",
              "layoutSizingVertical": "HUG",
              "paddingLeft": 8, "paddingRight": 8, "paddingTop": 4, "paddingBottom": 4,
              "itemSpacing": 6,
              "children": [
                {
                  "id": "90:2", "name": "Label", "type": "TEXT",
                  "characters": "Base",
                  "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1, "a": 1}}],
                  "style": {"fontFamily": "Inter", "fontSize": 14, "textAutoResize": "WIDTH_AND_HEIGHT"}
                },
                {
                  "id": "90:3", "name": "SelfA", "type": "INSTANCE",
                  "componentId": "90:1",
                  "layoutSizingHorizontal": "HUG",
                  "layoutSizingVertical": "HUG",
                  "overrides": [{"id": "90:2", "characters": "Loop Alpha"}]
                },
                {
                  "id": "90:4", "name": "SelfB", "type": "INSTANCE",
                  "componentId": "90:1",
                  "layoutSizingHorizontal": "HUG",
                  "layoutSizingVertical": "HUG",
                  "overrides": [{"id": "90:2", "characters": "Loop Beta"}]
                }
              ]
            },
            {
              "id": "1:1", "name": "Stage", "type": "FRAME",
              "size": {"x": 400, "y": 200},
              "layoutMode": "VERTICAL",
              "layoutSizingHorizontal": "FIXED",
              "layoutSizingVertical": "HUG",
              "children": [{
                "id": "1:2", "name": "TopC1", "type": "INSTANCE",
                "componentId": "90:1",
                "layoutSizingHorizontal": "HUG",
                "layoutSizingVertical": "HUG",
                "overrides": [{"id": "90:2", "characters": "Root Cycle"}]
              }]
            }
          ]
        }]
      }
    }"#;

    let t_cyc = Instant::now();
    let (cyc_proj, _) =
        import_figma_json_str(cyclic_json, "Cyclic", &FigmaImportOptions::default())
            .expect("cyclic instance import must succeed cleanly");
    let cyc_elapsed = t_cyc.elapsed();
    cyc_proj.validate().expect("cyclic project must validate");
    assert!(
        cyc_elapsed < Duration::from_secs(5),
        "cyclic instance reflow took {cyc_elapsed:?} (must finish < 5s)"
    );

    // 1b. 32-level `C1 -> 2x C2 -> ... -> C32` diamond DAG with `symbol_overrides`
    let mut comps_json = Vec::new();
    for k in (1..=32).rev() {
        let cid = format!("80:{k}");
        let tid = format!("81:{k}");
        if k == 32 {
            comps_json.push(format!(
                r#"{{
                  "id": "{cid}", "name": "C{k}", "type": "COMPONENT", "visible": false,
                  "layoutMode": "HORIZONTAL", "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                  "paddingLeft": 2, "paddingRight": 2, "itemSpacing": 2,
                  "children": [{{
                    "id": "{tid}", "name": "LeafText", "type": "TEXT", "characters": "Leaf",
                    "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                    "style": {{"fontFamily": "Inter", "fontSize": 12, "textAutoResize": "WIDTH_AND_HEIGHT"}}
                  }}]
                }}"#
            ));
        } else {
            let next_cid = format!("80:{}", k + 1);
            let next_tid = format!("81:{}", k + 1);
            let ia = format!("82:{k}");
            let ib = format!("83:{k}");
            comps_json.push(format!(
                r#"{{
                  "id": "{cid}", "name": "C{k}", "type": "COMPONENT", "visible": false,
                  "layoutMode": "HORIZONTAL", "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                  "paddingLeft": 2, "paddingRight": 2, "itemSpacing": 2,
                  "children": [
                    {{
                      "id": "{tid}", "name": "Hdr{k}", "type": "TEXT", "characters": "L{k}",
                      "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                      "style": {{"fontFamily": "Inter", "fontSize": 12, "textAutoResize": "WIDTH_AND_HEIGHT"}}
                    }},
                    {{
                      "id": "{ia}", "name": "Left{k}", "type": "INSTANCE", "componentId": "{next_cid}",
                      "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                      "overrides": [{{"id": "{next_tid}", "characters": "BranchA_{k}"}}]
                    }},
                    {{
                      "id": "{ib}", "name": "Right{k}", "type": "INSTANCE", "componentId": "{next_cid}",
                      "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                      "overrides": [{{"id": "{next_tid}", "characters": "BranchB_{k}"}}]
                    }}
                  ]
                }}"#
            ));
        }
    }
    let dag_json = format!(
        r#"{{
          "name": "32-Level Diamond DAG",
          "schemaVersion": 0,
          "document": {{
            "id": "0:0", "name": "Doc", "type": "DOCUMENT",
            "children": [{{
              "id": "0:1", "name": "Page 1", "type": "CANVAS",
              "children": [
                {},
                {{
                  "id": "2:1", "name": "Stage", "type": "FRAME",
                  "size": {{"x": 800, "y": 400}},
                  "layoutMode": "HORIZONTAL",
                  "layoutSizingHorizontal": "HUG",
                  "layoutSizingVertical": "HUG",
                  "children": [{{
                    "id": "2:2", "name": "RootDAG", "type": "INSTANCE",
                    "componentId": "80:1",
                    "layoutSizingHorizontal": "HUG",
                    "layoutSizingVertical": "HUG",
                    "overrides": [{{"id": "81:1", "characters": "TopOverride"}}]
                  }}]
                }}
              ]
            }}]
          }}
        }}"#,
        comps_json.join(",")
    );

    let t_dag = Instant::now();
    let (dag_proj, _) = import_figma_json_str(
        &dag_json,
        "DiamondDAG",
        &FigmaImportOptions {
            import_hidden_layers: false,
            ..FigmaImportOptions::default()
        },
    )
    .expect("32-level diamond DAG must import without hang");
    let dag_elapsed = t_dag.elapsed();
    dag_proj
        .validate()
        .expect("32-level diamond DAG must validate");
    assert!(
        dag_elapsed < Duration::from_secs(5),
        "32-level diamond DAG took {dag_elapsed:?} (must finish < 5s)"
    );

    // 1c. Extreme `1e308` Auto Layout padding / spacing / min / max in `.figma.json` -> finite & valid!
    let extreme_floats_json = r#"{
      "name": "Extreme Auto Layout Floats",
      "schemaVersion": 0,
      "document": {
        "id": "0:0", "name": "Doc", "type": "DOCUMENT",
        "children": [{
          "id": "0:1", "name": "Page 1", "type": "CANVAS",
          "children": [{
            "id": "1:1", "name": "OverflowFrame", "type": "FRAME",
            "size": {"x": 1e308, "y": 1e308},
            "layoutMode": "HORIZONTAL",
            "layoutWrap": "WRAP",
            "layoutSizingHorizontal": "HUG",
            "layoutSizingVertical": "HUG",
            "paddingLeft": 1e308,
            "paddingRight": 1e308,
            "paddingTop": 1e308,
            "paddingBottom": 1e308,
            "itemSpacing": 1e308,
            "counterAxisSpacing": 1e308,
            "minWidth": -500.0,
            "maxWidth": 1e308,
            "minHeight": 1e308,
            "maxHeight": 1e308,
            "children": [
              {"id": "1:2", "name": "ChildA", "type": "RECTANGLE", "size": {"x": 40, "y": 40}, "layoutGrow": 1e308, "fills": [{"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0, "a": 1}}]},
              {"id": "1:3", "name": "ChildB", "type": "RECTANGLE", "size": {"x": 40, "y": 40}, "fills": [{"type": "SOLID", "color": {"r": 0, "g": 1, "b": 0, "a": 1}}]}
            ]
          }]
        }]
      }
    }"#;

    let (ext_proj, _) = import_figma_json_str(
        extreme_floats_json,
        "ExtremeFloats",
        &FigmaImportOptions::default(),
    )
    .expect("extreme 1e308 auto layout floats must import cleanly");
    ext_proj
        .validate()
        .expect("clamped auto layout floats must produce finite coordinates passing validate()");
    for sym in &ext_proj.symbols {
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    assert!(
                        el.transform.x.is_finite() && el.transform.y.is_finite(),
                        "non-finite transform on {}: {:?}",
                        el.name,
                        el.transform
                    );
                }
            }
        }
    }

    // 2. `[P2]` Corrupt multi-megabyte `vmsk` block (`150_000` knot records > `MAX_PATH_COMMANDS`)
    //    must abort in-loop immediately without unbounded command/knot allocation!
    let mut huge_vmsk = Vec::with_capacity(8 + 150_000 * 26);
    huge_vmsk.extend_from_slice(&3_u32.to_be_bytes());
    huge_vmsk.extend_from_slice(&0_u32.to_be_bytes());
    let mut knot_rec = [0u8; 26];
    knot_rec[0..2].copy_from_slice(&1_u16.to_be_bytes()); // knot selector 1
    for _ in 0..150_000 {
        huge_vmsk.extend_from_slice(&knot_rec);
    }
    let t_vmsk = Instant::now();
    assert!(
        neoflash_import::psd::parse_vmsk_block(&huge_vmsk, 200, 200).is_none(),
        "oversized vmsk stream must abort early in-loop"
    );
    assert!(
        t_vmsk.elapsed() < Duration::from_secs(5),
        "oversized vmsk rejection took {:?} (expected immediate in-loop abort)",
        t_vmsk.elapsed()
    );
}
