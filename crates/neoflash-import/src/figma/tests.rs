
use super::kiwi::{KiwiReader, decode_kiwi_scalar, parse_kiwi_schema};
use super::*;
use flate2::{Compression, write::DeflateEncoder};
use std::io::Write;

fn tiny_png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 120, 40, 255]));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

#[test]
fn figma_json_decomposes_artboard_into_layers_components_masks_vectors_and_text() {
    let png_b64 = BASE64.encode(tiny_png_bytes());
    let figma_json = format!(
        r#"{{
              "name": "Hero Banner Design",
              "schemaVersion": 0,
              "images": {{
                "img_hero_1": "{png_b64}"
              }},
              "document": {{
                "id": "0:0",
                "name": "Document",
                "type": "DOCUMENT",
                "children": [
                  {{
                    "id": "0:1",
                    "name": "Landing Page",
                    "type": "CANVAS",
                    "backgroundColor": {{"r": 0.08, "g": 0.09, "b": 0.12, "a": 1.0}},
                    "children": [
                      {{
                        "id": "1:10",
                        "name": "CTA_Button",
                        "type": "COMPONENT",
                        "visible": false,
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 160, "height": 48}},
                        "children": [
                          {{
                            "id": "1:11",
                            "name": "Btn_BG",
                            "type": "RECTANGLE",
                            "cornerRadius": 12,
                            "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 160, "height": 48}},
                            "fills": [{{"type": "SOLID", "color": {{"r": 0.2, "g": 0.5, "b": 1.0, "a": 1.0}}}}]
                          }}
                        ]
                      }},
                      {{
                        "id": "2:1",
                        "name": "Main Composition",
                        "type": "FRAME",
                        "clipsContent": true,
                        "absoluteBoundingBox": {{"x": 100, "y": 50, "width": 1280, "height": 720}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.05, "g": 0.06, "b": 0.1, "a": 1.0}}}}],
                        "children": [
                          {{
                            "id": "2:2",
                            "name": "Photo Mask",
                            "type": "ELLIPSE",
                            "isMask": true,
                            "maskType": "VECTOR",
                            "absoluteBoundingBox": {{"x": 140, "y": 90, "width": 400, "height": 400}},
                            "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}]
                          }},
                          {{
                            "id": "2:3",
                            "name": "Hero Photo",
                            "type": "RECTANGLE",
                            "absoluteBoundingBox": {{"x": 140, "y": 90, "width": 400, "height": 400}},
                            "fills": [{{"type": "IMAGE", "imageRef": "img_hero_1"}}]
                          }},
                          {{
                            "id": "2:4",
                            "name": "Accent Ribbon",
                            "type": "VECTOR",
                            "blendMode": "SCREEN",
                            "absoluteBoundingBox": {{"x": 200, "y": 150, "width": 300, "height": 120}},
                            "fillGeometry": [{{"path": "M 0 0 L 300 0 L 260 120 L 0 120 Z", "windingRule": "NONZERO"}}],
                            "fills": [{{
                              "type": "GRADIENT_LINEAR",
                              "gradientStops": [
                                {{"position": 0.0, "color": {{"r": 1.0, "g": 0.3, "b": 0.2, "a": 1.0}}}},
                                {{"position": 1.0, "color": {{"r": 0.9, "g": 0.8, "b": 0.1, "a": 1.0}}}}
                              ]
                            }}],
                            "effects": [{{
                              "type": "DROP_SHADOW",
                              "visible": true,
                              "color": {{"r": 0, "g": 0, "b": 0, "a": 0.5}},
                              "offset": {{"x": 0, "y": 8}},
                              "radius": 16
                            }}]
                          }},
                          {{
                            "id": "2:5",
                            "name": "Headline",
                            "type": "TEXT",
                            "absoluteBoundingBox": {{"x": 600, "y": 180, "width": 520, "height": 80}},
                            "characters": "Motion Meets Design",
                            "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                            "style": {{"fontFamily": "Inter", "fontSize": 48, "fontWeight": 700, "textAlignHorizontal": "LEFT"}}
                          }},
                          {{
                            "id": "2:6",
                            "name": "Primary CTA",
                            "type": "INSTANCE",
                            "componentId": "1:10",
                            "absoluteBoundingBox": {{"x": 600, "y": 300, "width": 160, "height": 48}}
                          }}
                        ]
                      }}
                    ]
                  }}
                ]
              }}
            }}"#
    );

    let opts = FigmaImportOptions {
        import_hidden_layers: false,
        ..FigmaImportOptions::default()
    };
    let (project, report) =
        import_figma_json_str(&figma_json, "Hero Banner", &opts).expect("figma json import");
    project
        .validate()
        .expect("imported Figma project validates");
    assert!(report.can_import);
    assert_eq!(project.width, 1280);
    assert_eq!(project.height, 720);
    assert_eq!(project.resources.len(), 1);

    let root = project.symbol(&project.root).unwrap();
    // Frame Clip + Photo Mask + Hero Photo + Accent Ribbon + Headline + Primary CTA = 6 layers!
    assert_eq!(root.timeline.layers.len(), 6);
    assert_eq!(root.timeline.layers[1].properties.role, LayerRole::Mask);
    assert_eq!(
        root.timeline.layers[2].properties.mask.as_deref(),
        Some(root.timeline.layers[1].id.as_str())
    );
    assert!(
        project
            .symbols
            .iter()
            .any(|s| s.name == "CTA_Button" && s.kind == SymbolKind::MovieClip)
    );
}

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
    let bits = f.to_bits();
    let rotated = bits.rotate_left(9);
    out.extend_from_slice(&rotated.to_le_bytes());
}

fn write_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

#[test]
fn binary_fig_kiwi_zip_archive_decodes_into_layered_project() {
    // Build minimal Kiwi schema:
    // 0: GUID (STRUCT: sessionID uint, localID uint)
    // 1: ParentIndex (STRUCT: guid GUID, position string)
    // 2: Vector (STRUCT: x float, y float)
    // 3: Color (STRUCT: r float, g float, b float, a float)
    // 4: Paint (MESSAGE: 1=type string, 2=color Color)
    // 5: NodeChange (MESSAGE: 1=guid GUID, 2=parentIndex ParentIndex, 3=type string, 4=name string, 5=size Vector, 6=fillPaints Paint[])
    // 6: Message (MESSAGE: 1=nodeChanges NodeChange[])
    let mut schema = Vec::new();
    write_var_uint(&mut schema, 7);
    // 0: GUID
    write_cstr(&mut schema, "GUID");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "sessionID");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "localID");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 2);
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
    write_cstr(&mut schema, "x");
    write_var_int(&mut schema, -5);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "y");
    write_var_int(&mut schema, -5);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 3: Color
    write_cstr(&mut schema, "Color");
    schema.push(1);
    write_var_uint(&mut schema, 4);
    for (i, ch) in ["r", "g", "b", "a"].iter().enumerate() {
        write_cstr(&mut schema, ch);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 4: Paint
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
    // 5: NodeChange
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    write_var_uint(&mut schema, 6);
    for (tag, fname, tid, arr) in [
        (1u64, "guid", 0i64, 0u8),
        (2, "parentIndex", 1, 0),
        (3, "type", -6, 0),
        (4, "name", -6, 0),
        (5, "size", 2, 0),
        (6, "fillPaints", 4, 1),
    ] {
        write_cstr(&mut schema, fname);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 6: Message
    write_cstr(&mut schema, "Message");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "nodeChanges");
    write_var_int(&mut schema, 5);
    schema.push(1);
    write_var_uint(&mut schema, 1);

    // Encode root Message with 4 nodes: DOCUMENT(0:0) -> CANVAS(0:1) -> FRAME(1:1, 800x600) -> RECTANGLE(1:2) + ELLIPSE(1:3)
    let mut msg = Vec::new();
    write_var_uint(&mut msg, 1); // field 1 = nodeChanges
    write_var_uint(&mut msg, 5); // array len 5

    // Node 0: DOCUMENT 0:0
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "DOCUMENT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Doc");
    write_var_uint(&mut msg, 0);

    // Node 1: CANVAS 0:1 parent 0:0
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
    write_cstr(&mut msg, "Page 1");
    write_var_uint(&mut msg, 0);

    // Node 2: FRAME 1:1 parent 0:1 size 800x600
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
    write_var_float(&mut msg, 800.0);
    write_var_float(&mut msg, 600.0);
    write_var_uint(&mut msg, 0);

    // Node 3 & 4: RECTANGLE & ELLIPSE inside 1:1
    for (lid, pos_s, ntype, lname) in [
        (2u64, "a", "RECTANGLE", "Card BG"),
        (3u64, "b", "ELLIPSE", "Avatar Badge"),
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
        write_var_float(&mut msg, 120.0);
        write_var_float(&mut msg, 90.0);
        write_var_uint(&mut msg, 6); // fillPaints len 1
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, "SOLID");
        write_var_uint(&mut msg, 2);
        write_var_float(&mut msg, 0.25);
        write_var_float(&mut msg, 0.6);
        write_var_float(&mut msg, 0.95);
        write_var_float(&mut msg, 1.0);
        write_var_uint(&mut msg, 0); // end Paint
        write_var_uint(&mut msg, 0); // end NodeChange
    }
    write_var_uint(&mut msg, 0); // end Message

    let deflate = |raw: &[u8]| -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    };
    let c0 = deflate(&schema);
    let c1 = deflate(&msg);
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
        zw.write_all(br#"{"file_name":"UI Kit Board"}"#).unwrap();
        zw.start_file("canvas.fig", opts).unwrap();
        zw.write_all(&canvas_fig).unwrap();
        zw.finish().unwrap();
    }
    let fig_zip_bytes = zip_buf.into_inner();
    assert!(is_figma_bytes(&fig_zip_bytes));

    let (project, report) =
        import_figma_bytes(&fig_zip_bytes, "fallback", &FigmaImportOptions::default())
            .expect("binary .fig zip import");
    project.validate().expect("project validates");
    assert!(report.can_import);
    assert_eq!(project.name, "UI Kit Board");
    assert_eq!(project.width, 800);
    assert_eq!(project.height, 600);
    let root = project.symbol(&project.root).unwrap();
    assert_eq!(root.timeline.layers.len(), 2);
    assert_eq!(root.timeline.layers[0].name, "Card BG");
    assert_eq!(root.timeline.layers[1].name, "Avatar Badge");
}

#[test]
fn t259_figma_hardening_handles_forward_components_text_masks_arcs_and_cyclic_kiwi_structs() {
    // 1. Forward-referenced INSTANCE before COMPONENT + TEXT mask + >360° arc + 20 gradient stops + oversized corner radii
    let stops_json: String = (0..20)
        .map(|i| {
            let p = f64::from(i) / 19.0;
            format!(
                r#"{{"position": {p:.4}, "color": {{"r": {p:.4}, "g": 0.4, "b": 0.8, "a": 1.0}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let figma_json = format!(
        r#"{{
              "name": "Seam Torture",
              "schemaVersion": 0,
              "document": {{
                "id": "0:0", "name": "Doc", "type": "DOCUMENT",
                "children": [{{
                  "id": "0:1", "name": "Page 1", "type": "CANVAS",
                  "children": [
                    {{
                      "id": "2:1", "name": "Board", "type": "FRAME",
                      "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 600, "height": 400}},
                      "children": [
                        {{
                          "id": "2:2", "name": "Forward CTA", "type": "INSTANCE",
                          "componentId": "9:99",
                          "absoluteBoundingBox": {{"x": 20, "y": 20, "width": 120, "height": 40}}
                        }},
                        {{
                          "id": "2:3", "name": "Text Mask", "type": "TEXT",
                          "isMask": true, "maskType": "VECTOR",
                          "characters": "CLIP",
                          "absoluteBoundingBox": {{"x": 40, "y": 80, "width": 200, "height": 60}},
                          "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                          "style": {{"fontFamily": "Inter", "fontSize": 32}}
                        }},
                        {{
                          "id": "2:4", "name": "Masked Arc", "type": "ELLIPSE",
                          "arcData": {{"startingAngle": -3.14159, "endingAngle": 9.42477, "innerRadius": 1.5}},
                          "absoluteBoundingBox": {{"x": 50, "y": 90, "width": 100, "height": 100}},
                          "fills": [{{"type": "GRADIENT_LINEAR", "gradientStops": [{stops_json}]}}]
                        }},
                        {{
                          "id": "2:5", "name": "Huge Radius Pill", "type": "RECTANGLE",
                          "cornerRadius": 9999.0,
                          "absoluteBoundingBox": {{"x": 10, "y": 200, "width": 80, "height": 30}},
                          "fills": [{{"type": "SOLID", "color": {{"r": 0.3, "g": 0.9, "b": 0.5, "a": 1.0}}}}]
                        }}
                      ]
                    }},
                    {{
                      "id": "9:99", "name": "Late_Defined_Component", "type": "COMPONENT",
                      "absoluteBoundingBox": {{"x": 700, "y": 0, "width": 120, "height": 40}},
                      "children": [{{
                        "id": "9:100", "name": "Inner Pill", "type": "RECTANGLE",
                        "cornerRadius": 20.0,
                        "absoluteBoundingBox": {{"x": 700, "y": 0, "width": 120, "height": 40}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.9, "g": 0.2, "b": 0.4, "a": 1.0}}}}]
                      }}]
                    }}
                  ]
                }}]
              }}
            }}"#
    );
    let opts = FigmaImportOptions::default();
    let (project, report) = import_figma_json_str(&figma_json, "Seam Torture", &opts)
        .expect("torture figma json must import and validate");
    project.validate().expect("validated");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "figma-mask-text-bounds-approximated")
    );

    // 2. Cyclic Kiwi Struct definition must fail cleanly without stack overflow
    let mut cyc_schema = Vec::new();
    write_var_uint(&mut cyc_schema, 1);
    write_cstr(&mut cyc_schema, "SelfCycle");
    cyc_schema.push(1); // Struct
    write_var_uint(&mut cyc_schema, 1);
    write_cstr(&mut cyc_schema, "self");
    write_var_int(&mut cyc_schema, 0); // type_id 0 -> SelfCycle!
    cyc_schema.push(0);
    write_var_uint(&mut cyc_schema, 1);
    let defs = parse_kiwi_schema(&cyc_schema).unwrap();
    let mut mr = KiwiReader::new(&[0; 16]);
    let mut budget = MAX_KIWI_VALUES;
    let mut active = Vec::new();
    let err = decode_kiwi_scalar(&mut mr, &defs, 0, 0, &mut budget, &mut active)
        .expect_err("cyclic Kiwi struct must error");
    assert!(err.contains("Cyclic Kiwi struct"));

    // 3. Path-traversal ZIP entry inside .fig archive must be rejected
    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zip_buf);
        let zopts = zip::write::SimpleFileOptions::default();
        zw.start_file("../escape.fig", zopts).unwrap();
        zw.write_all(b"fig-kiwi").unwrap();
        zw.finish().unwrap();
    }
    assert!(import_figma_bytes(&zip_buf.into_inner(), "bad", &opts).is_err());
}
