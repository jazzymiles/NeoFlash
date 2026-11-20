use super::kiwi::{KiwiReader, decode_kiwi_scalar, parse_kiwi_schema};
use super::*;
use flate2::{Compression, write::DeflateEncoder};
use std::io::Write;
use std::path::PathBuf;

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
                        "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 0, "b": 1}}}}],
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
                            "style": {{"fontFamily": "Inter", "fontSize": 48, "lineHeightPx": 48, "fontWeight": 700, "textAlignHorizontal": "LEFT"}}
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
    let headline = root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes[0].elements)
        .find(|e| e.name == "Headline")
        .unwrap();
    let ElementKind::TextBox { format, .. } = &headline.kind else {
        panic!("editable headline expected")
    };
    assert!((1.2 * format.size + format.leading - 48.0).abs() < 1e-6);
    let field = format.field_options().unwrap();
    assert_eq!(
        (
            field.padding_left,
            field.padding_right,
            field.padding_top,
            field.padding_bottom
        ),
        (0., 0., 0., 0.)
    );
    let master = project
        .symbols
        .iter()
        .find(|s| s.name == "CTA_Button")
        .unwrap();
    assert_eq!(master.kind, SymbolKind::MovieClip);
    let background = &master.timeline.layers[0].keyframes[0].elements[0];
    assert!(background.visible);
    assert!(matches!(
        background.kind,
        ElementKind::Rectangle {
            fill: [255, 0, 255, 255],
            ..
        }
    ));
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

fn push_zstd_block_hdr(out: &mut Vec<u8>, last: bool, block_type: u8, block_size: usize) {
    let hdr = ((block_size as u32) << 3) | ((u32::from(block_type) & 0x03) << 1) | u32::from(last);
    let b = hdr.to_le_bytes();
    out.extend_from_slice(&[b[0], b[1], b[2]]);
}

/// Encodes `raw` into a valid RFC 8878 Zstandard frame (128 KiB raw blocks + 4-byte FCS + XXH64 checksum),
/// using `/opt/homebrew/bin/zstd` when available for full LZ77/Huffman/FSE compression and falling back
/// to deterministic RFC 8878 blocks otherwise.
fn encode_zstd_for_test(raw: &[u8]) -> Vec<u8> {
    for candidate in ["/opt/homebrew/bin/zstd", "/usr/local/bin/zstd", "zstd"] {
        if let Ok(mut child) = std::process::Command::new(candidate)
            .args(["-3", "-c", "--check"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            let writer = child.stdin.take().map(|mut stdin| {
                let owned = raw.to_vec();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(&owned);
                })
            });
            let out_res = child.wait_with_output();
            if let Some(handle) = writer {
                let _ = handle.join();
            }
            if let Ok(out) = out_res
                && out.status.success()
                && super::zstd::is_zstd_magic(&out.stdout)
            {
                return out.stdout;
            }
        }
    }

    let mut frame = Vec::new();
    frame.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]);
    // FHD: FCS_Field_Size_Flag=2 (4 bytes), Single_Segment_Flag=1, Content_Checksum_Flag=1 -> 0xA4
    frame.push(0xA4);
    frame.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    if raw.is_empty() {
        push_zstd_block_hdr(&mut frame, true, 0, 0);
    } else {
        let chunks: Vec<&[u8]> = raw.chunks(128 * 1024).collect();
        for (i, ch) in chunks.iter().enumerate() {
            let is_last = i + 1 == chunks.len();
            push_zstd_block_hdr(&mut frame, is_last, 0, ch.len());
            frame.extend_from_slice(ch);
        }
    }
    let cs = super::zstd::xxh64(raw, 0) as u32;
    frame.extend_from_slice(&cs.to_le_bytes());
    frame
}

#[test]
fn zstd_rfc8878_decompressor_covers_all_block_literal_sequence_checksum_and_bound_modes() {
    use super::zstd::{decompress_zstd, is_zstd_magic, xxh64};

    // 1. Canonical XXH64 reference vectors (seed = 0)
    assert_eq!(xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
    assert_eq!(xxh64(b"a", 0), 0xD24E_C4F1_A98C_6E5B);
    let stripe_64: Vec<u8> = (0u8..64).collect();
    assert_ne!(xxh64(&stripe_64, 0), xxh64(&stripe_64, 1));

    // 2. Skippable frame (`0x184D2A55`) + Multi-block Frame 1 (Raw + RLE + Compressed w/ 4-stream Huffman & RLE seqs)
    //    + Concatenated Frame 2 (Compressed w/ 1-stream Huffman -> Treeless Huffman + Repeat_Mode FSE + repeat-offset idx=3!)
    let mut stream = Vec::new();
    // Skippable frame with 5-byte payload
    stream.extend_from_slice(&[0x55, 0x2A, 0x4D, 0x18]);
    stream.extend_from_slice(&5u32.to_le_bytes());
    stream.extend_from_slice(b"SKIP!");
    assert!(is_zstd_magic(&stream));

    // Direct 4-bit Huffman tree mapping symbol 65 ('A') -> weight 1 (bit 0) and implied symbol 66 ('B') -> weight 1 (bit 1):
    // num_weights = 66 -> hdr = 127 + 66 = 193, followed by 33 bytes of 4-bit weights (w[0..65]=0, w[65]=1 -> last byte 0x01).
    let mut huf_tree = vec![193u8];
    huf_tree.extend(std::iter::repeat_n(0u8, 32));
    huf_tree.push(0x01); // w[64] = 0, w[65] = 1 -> implied w[66] = 1

    // Build Frame 1:
    // - Block 0 (Raw): "XYZ!" (4 bytes)
    // - Block 1 (RLE): 'Q' * 6 (6 bytes) -> history is "XYZ!QQQQQQ" (10 bytes)
    // - Block 2 (Compressed, last=true):
    //     4-stream Huffman literals (`lit_type=2, size_fmt=1`), regen_size = 8 ("ABBAAABB"):
    //       6-byte jump table [1,0, 1,0, 1,0] + streams [0x05 ("AB"), 0x06 ("BA"), 0x04 ("AA"), 0x07 ("BB")]
    //     Sequences (`num_seq = 1`): RLE modes (`0b01_01_01_00 = 0x54`):
    //       ll_sym = 8 (lit_len=8), of_sym = 3 (offset_value = 8 + extra(5) = 13 -> actual offset = 10 -> copies "XYZ!QQ" len 6!), ml_sym = 3 (match_len=6)
    //       extra bits read MSB-first: of_extra (3 bits) = 5 (`0b101`), ml_extra = 0, ll_extra = 0 -> sentinel 1 at bit 3 -> `0b0000_1101 = 0x0D`!
    let mut f1 = Vec::new();
    f1.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]);
    f1.push(0x04); // Single_Segment=0, Checksum=1, FCS=0; followed by Window_Descriptor
    f1.push(0x08); // Window_Descriptor -> 2 KB window

    // Block 0: Raw "XYZ!"
    push_zstd_block_hdr(&mut f1, false, 0, 4);
    f1.extend_from_slice(b"XYZ!");
    // Block 1: RLE 'Q' * 6
    push_zstd_block_hdr(&mut f1, false, 1, 6);
    f1.push(b'Q');

    // Block 2: Compressed
    let mut b2 = Vec::new();
    let mut huf_payload = huf_tree.clone();
    huf_payload.extend_from_slice(&[1, 0, 1, 0, 1, 0, 0x05, 0x06, 0x04, 0x07]);
    let regen_4s = 8u32;
    let comp_4s = huf_payload.len() as u32;
    // 3-byte Huffman header: lit_type=2, size_fmt=1 (4 streams), regen_size (10b), comp_size (10b)
    let h_4s = 2u32 | (1u32 << 2) | (regen_4s << 4) | (comp_4s << 14);
    b2.extend_from_slice(&[h_4s as u8, (h_4s >> 8) as u8, (h_4s >> 16) as u8]);
    b2.extend_from_slice(&huf_payload);
    // Sequences: 1 sequence, RLE modes (LL=8, OF=3, ML=3), bitstream [0x0D]
    b2.extend_from_slice(&[1, 0x54, 8, 3, 3, 0x0D]);

    push_zstd_block_hdr(&mut f1, true, 2, b2.len());
    f1.extend_from_slice(&b2);

    let expected_f1 = b"XYZ!QQQQQQABBAAABBQQABBA";
    let cs_f1 = xxh64(expected_f1, 0) as u32;
    f1.extend_from_slice(&cs_f1.to_le_bytes());
    stream.extend_from_slice(&f1);

    // Build Frame 2 testing:
    // - Block 0 (Compressed): 1-stream Huffman ("ABAB", regen=4) + 1 sequence with RLE modes (LL=4, OF=1, ML=0):
    //     of_sym=1, of_extra=0 -> offset_value = 2; with lit_len=4 > 0 -> idx = 1 -> rep[1] = 4! Copies "ABAB" (match_len=3 -> "ABA")!
    //     After Block 0: output = "ABABABA" (7 bytes), rep = [4, 1, 8].
    // - Block 1 (Compressed, last=true): Treeless Huffman (`lit_type=3, size_fmt=0`, regen=0? No, RLE/Raw or Treeless regen=4 "ABAB")
    //     Let's test Treeless (`lit_type=3, size_fmt=0`, regen=4, comp=1, stream=[0x15] -> "ABAB")
    //     + Repeat_Mode (`modes = 0xFC` reusing LL=4? Wait: let's test `lit_len == 0` repeat-offset `idx = 3 -> rep[0] - 1`!):
    //     In Block 1, use LL RLE=0 (`lit_len=0`), OF RLE=1 (`of_code=1, of_extra=1 -> offset_value=3; with lit_len==0 -> idx=3 -> rep[0]-1 = 4-1 = 3`!),
    //     ML Repeat_Mode (`mode=3` reusing ML=0 -> `match_len=3`), plus trailing 4 Treeless literals "ABAB"!
    let mut f2 = Vec::new();
    f2.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]);
    f2.push(0xA4); // FCS=4 bytes, Single_Segment=1, Checksum=1
    let expected_f2 = b"ABABABAABAABAB"; // 7 ("ABABABA") + 3 ("ABA" at offset 3 -> idx 4..7) + 4 ("ABAB") = 14 bytes!
    f2.extend_from_slice(&(expected_f2.len() as u32).to_le_bytes());

    // Frame 2 Block 0: 1-stream Huffman ("ABAB", stream [0x15]) + 1 seq (LL=4, OF=1, ML=0, of_extra=0 -> [0x02])
    let mut f2_b0 = Vec::new();
    let mut p_1s = huf_tree;
    p_1s.push(0x15);
    let h_1s = 2u32 | (4u32 << 4) | ((p_1s.len() as u32) << 14);
    f2_b0.extend_from_slice(&[h_1s as u8, (h_1s >> 8) as u8, (h_1s >> 16) as u8]);
    f2_b0.extend_from_slice(&p_1s);
    f2_b0.extend_from_slice(&[1, 0x54, 4, 1, 0, 0x02]); // sentinel bit 1, bit 0 = 0 (of_extra=0)
    push_zstd_block_hdr(&mut f2, false, 2, f2_b0.len());
    f2.extend_from_slice(&f2_b0);

    // Frame 2 Block 1: Treeless 1-stream Huffman (`lit_type=3, size_fmt=0`, regen=4, comp=1, payload=[0x15])
    // + 1 seq: LL=RLE(0), OF=RLE(1), ML=Repeat(mode 3) -> modes = (1<<6)|(1<<4)|(3<<2) = 0x5C, extra bits: of_extra=1 -> [0x03]
    let mut f2_b1 = Vec::new();
    let h_tl = 3u32 | (4u32 << 4) | (1u32 << 14);
    f2_b1.extend_from_slice(&[h_tl as u8, (h_tl >> 8) as u8, (h_tl >> 16) as u8, 0x15]);
    f2_b1.extend_from_slice(&[1, 0x5C, 0, 1, 0x03]); // sentinel bit 1, bit 0 = 1 (of_extra=1 -> offset_value=3!)
    push_zstd_block_hdr(&mut f2, true, 2, f2_b1.len());
    f2.extend_from_slice(&f2_b1);

    let cs_f2 = xxh64(expected_f2, 0) as u32;
    f2.extend_from_slice(&cs_f2.to_le_bytes());
    stream.extend_from_slice(&f2);

    let mut expected_all = Vec::new();
    expected_all.extend_from_slice(expected_f1);
    expected_all.extend_from_slice(expected_f2);

    let decoded = decompress_zstd(&stream, 1024 * 1024).expect("valid multi-frame RFC 8878 stream");
    assert_eq!(decoded, expected_all);

    // 3. Bounds check must reject output exceeding `max_output`
    let bound_err = decompress_zstd(&stream, expected_all.len() - 1)
        .expect_err("must enforce max_output bound");
    assert!(bound_err.contains("exceeds"));

    // 4. Corrupt XXH64 checksum must fail cleanly
    let mut bad_cs = stream.clone();
    let last_idx = bad_cs.len() - 1;
    bad_cs[last_idx] ^= 0xFF;
    let cs_err = decompress_zstd(&bad_cs, 1024 * 1024).expect_err("corrupt checksum must fail");
    assert!(cs_err.contains("checksum"));

    // 5. Cross-check against system `zstd` CLI across compression levels when installed
    let mut rich_payload = Vec::with_capacity(300_000);
    for i in 0..5_000u32 {
        rich_payload.extend_from_slice(
            format!("NeoFlash-Figma-v106-NodeChange-{i:04}-commandsBlob-vectorNetwork\n")
                .as_bytes(),
        );
    }
    let compressed = encode_zstd_for_test(&rich_payload);
    let roundtrip =
        decompress_zstd(&compressed, 10 * 1024 * 1024).expect("zstd roundtrip decompress");
    assert_eq!(roundtrip, rich_payload);
}

#[test]
fn zstd_fig_kiwi_v106_handles_internal_canvas_overrides_blobs_corners_images_and_scenes() {
    // Build a realistic Figma Desktop v106 Kiwi schema exercising:
    // - `Internal Only Canvas` (`internalOnly: true`) holding master `SYMBOL` (`10:1` ButtonMaster)
    // - Two user-visible `CANVAS` pages (`0:1` Home Screen, `0:2` Settings Screen) -> `Project.scenes`
    // - `INSTANCE` (`2:2`) with `symbolID` + `symbolOverrides` overriding nested text & solid fill
    // - `ROUNDED_RECTANGLE` (`2:3`) with asymmetric Kiwi corners `[tl=4, tr=12, br=20, bl=28]` + oversized >4096px image fill
    // - `VECTOR` (`2:4`) with `blobBaseIndex = 1` + binary `commandsBlob` (M, L, Q, C, Z)
    // - `VECTOR` (`2:5`) with binary `vectorNetworkBlob` (4 vertices, cubic tangent, closed EVENODD region)
    // - Multi-run `TEXT` (`2:6`) with `characterStyleIDs` + `styleOverrideTable`
    let mut schema = Vec::new();
    write_var_uint(&mut schema, 15);

    // 0: GUID (STRUCT: sessionID uint, localID uint)
    write_cstr(&mut schema, "GUID");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["sessionID", "localID"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 1: ParentIndex (STRUCT: guid GUID, position string)
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
    // 2: Vector (STRUCT: x float, y float)
    write_cstr(&mut schema, "Vector");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["x", "y"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 3: Matrix (STRUCT: m00, m01, m02, m10, m11, m12 float)
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
    // 4: Color (STRUCT: r, g, b, a float)
    write_cstr(&mut schema, "Color");
    schema.push(1);
    write_var_uint(&mut schema, 4);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 5: Image (MESSAGE: 1=hash byte[])
    write_cstr(&mut schema, "Image");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "hash");
    write_var_int(&mut schema, -2);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    // 6: Paint (MESSAGE: 1=type string, 2=color Color, 3=image Image, 4=imageScaleMode string)
    write_cstr(&mut schema, "Paint");
    schema.push(2);
    write_var_uint(&mut schema, 4);
    for (tag, n, tid, arr) in [
        (1u64, "type", -6i64, 0u8),
        (2, "color", 4, 0),
        (3, "image", 5, 0),
        (4, "imageScaleMode", -6, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 7: Path (MESSAGE: 1=windingRule string, 2=commandsBlob uint)
    write_cstr(&mut schema, "Path");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "windingRule");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "commandsBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 8: VectorData (MESSAGE: 1=vectorNetworkBlob uint)
    write_cstr(&mut schema, "VectorData");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "vectorNetworkBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    // 9: FontName (STRUCT: family string, style string)
    write_cstr(&mut schema, "FontName");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["family", "style"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -6);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 10: StyleOverride (MESSAGE: 1=styleID uint, 2=fontName FontName, 3=fontSize float, 4=fillPaints Paint[])
    write_cstr(&mut schema, "StyleOverride");
    schema.push(2);
    write_var_uint(&mut schema, 4);
    for (tag, n, tid, arr) in [
        (1u64, "styleID", -4i64, 0u8),
        (2, "fontName", 9, 0),
        (3, "fontSize", -5, 0),
        (4, "fillPaints", 6, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 11: TextData (MESSAGE: 1=characters string, 2=characterStyleIDs uint[], 3=styleOverrideTable StyleOverride[])
    write_cstr(&mut schema, "TextData");
    schema.push(2);
    write_var_uint(&mut schema, 3);
    for (tag, n, tid, arr) in [
        (1u64, "characters", -6i64, 0u8),
        (2, "characterStyleIDs", -4, 1),
        (3, "styleOverrideTable", 10, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 12: SymbolData (MESSAGE: 1=symbolID GUID, 2=symbolOverrides NodeChange[]) -> forward ref to 13!
    write_cstr(&mut schema, "SymbolData");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "symbolID");
    write_var_int(&mut schema, 0);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "symbolOverrides");
    write_var_int(&mut schema, 13);
    schema.push(1);
    write_var_uint(&mut schema, 2);
    // 13: NodeChange (MESSAGE)
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    write_var_uint(&mut schema, 19);
    for (tag, n, tid, arr) in [
        (1u64, "guid", 0i64, 0u8),
        (2, "guidPath", 0, 1),
        (3, "parentIndex", 1, 0),
        (4, "type", -6, 0),
        (5, "name", -6, 0),
        (6, "internalOnly", -1, 0),
        (7, "size", 2, 0),
        (8, "transform", 3, 0),
        (9, "fillPaints", 6, 1),
        (10, "fillGeometry", 7, 1),
        (11, "vectorData", 8, 0),
        (12, "textData", 11, 0),
        (13, "fontName", 9, 0),
        (14, "fontSize", -5, 0),
        (15, "symbolData", 12, 0),
        (16, "rectangleTopLeftCornerRadius", -5, 0),
        (17, "rectangleTopRightCornerRadius", -5, 0),
        (18, "rectangleBottomRightCornerRadius", -5, 0),
        (19, "rectangleBottomLeftCornerRadius", -5, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 14: Blob (MESSAGE: 1=bytes byte[]) + Wait: let's add Blob as 14 and Message as 15 -> update def count!
    // Let's rewrite def count at top or make Blob struct/message:
    // Actually we wrote 15 defs (0..=14), so let's make Blob def 14 and Message def 15 (16 defs total)!
    schema.clear();
    write_var_uint(&mut schema, 17);
    // Re-emit 0..=13 cleanly:
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
    write_cstr(&mut schema, "Color");
    schema.push(1);
    write_var_uint(&mut schema, 4);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    write_cstr(&mut schema, "Image");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "hash");
    write_var_int(&mut schema, -2);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "Paint");
    schema.push(2);
    write_var_uint(&mut schema, 4);
    for (tag, n, tid, arr) in [
        (1u64, "type", -6i64, 0u8),
        (2, "color", 4, 0),
        (3, "image", 5, 0),
        (4, "imageScaleMode", -6, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    write_cstr(&mut schema, "Path");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "windingRule");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "commandsBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "VectorData");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "vectorNetworkBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "FontName");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["family", "style"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -6);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    write_cstr(&mut schema, "StyleOverride");
    schema.push(2);
    write_var_uint(&mut schema, 4);
    for (tag, n, tid, arr) in [
        (1u64, "styleID", -4i64, 0u8),
        (2, "fontName", 9, 0),
        (3, "fontSize", -5, 0),
        (4, "fillPaints", 6, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    write_cstr(&mut schema, "TextData");
    schema.push(2);
    write_var_uint(&mut schema, 3);
    for (tag, n, tid, arr) in [
        (1u64, "characters", -6i64, 0u8),
        (2, "characterStyleIDs", -4, 1),
        (3, "styleOverrideTable", 10, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    write_cstr(&mut schema, "SymbolData");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "symbolID");
    write_var_int(&mut schema, 0);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "symbolOverrides");
    write_var_int(&mut schema, 13);
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    write_var_uint(&mut schema, 26);
    for (tag, n, tid, arr) in [
        (1u64, "guid", 0i64, 0u8),
        (2, "guidPath", 0, 1),
        (3, "parentIndex", 1, 0),
        (4, "type", -6, 0),
        (5, "name", -6, 0),
        (6, "internalOnly", -1, 0),
        (7, "size", 2, 0),
        (8, "transform", 3, 0),
        (9, "fillPaints", 6, 1),
        (10, "fillGeometry", 7, 1),
        (11, "vectorData", 8, 0),
        (12, "textData", 11, 0),
        (13, "fontName", 9, 0),
        (14, "fontSize", -5, 0),
        (15, "symbolData", 12, 0),
        (16, "rectangleTopLeftCornerRadius", -5, 0),
        (17, "rectangleTopRightCornerRadius", -5, 0),
        (18, "rectangleBottomRightCornerRadius", -5, 0),
        (19, "rectangleBottomLeftCornerRadius", -5, 0),
        (20, "strokePaints", 6, 1),
        (21, "strokeGeometry", 7, 1),
        (22, "componentPropAssignments", 16, 1),
        (23, "parameterConsumptionMap", 16, 0),
        (24, "derivedSymbolData", 13, 1),
        (25, "derivedTextData", -6, 0),
        (26, "textAutoResize", -6, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 14: Blob (MESSAGE: 1=bytes byte[])
    write_cstr(&mut schema, "Blob");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "bytes");
    write_var_int(&mut schema, -2);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    // 15: Message (MESSAGE: 1=nodeChanges NodeChange[], 2=blobs Blob[], 3=blobBaseIndex uint, 99=unusedExtra string)
    write_cstr(&mut schema, "Message");
    schema.push(2);
    write_var_uint(&mut schema, 4);
    for (tag, n, tid, arr) in [
        (1u64, "nodeChanges", 13i64, 1u8),
        (2, "blobs", 14, 1),
        (3, "blobBaseIndex", -4, 0),
        (99, "unusedExtra", -6, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }

    // The test schema uses one tagged message for the assignment and binding envelopes.
    write_cstr(&mut schema, "Property");
    schema.push(2);
    let property_fields = [
        ("defID", 0, 0),
        ("value", 16, 0),
        ("textValue", 11, 0),
        ("entries", 16, 1),
        ("variableField", -6, 0),
        ("variableData", 16, 0),
        ("propRefValue", 16, 0),
        ("defId", 0, 0),
        ("boolValue", -1, 0),
        ("varValue", 16, 0),
        ("symbolIdValue", 0, 0),
    ];
    write_var_uint(&mut schema, property_fields.len() as u64);
    for (index, (name, ty, array)) in property_fields.into_iter().enumerate() {
        write_cstr(&mut schema, name);
        write_var_int(&mut schema, ty);
        schema.push(array);
        write_var_uint(&mut schema, index as u64 + 1);
    }

    let write_guid = |m: &mut Vec<u8>, s: u64, l: u64| {
        write_var_uint(m, s);
        write_var_uint(m, l);
    };
    let write_parent = |m: &mut Vec<u8>, ps: u64, pl: u64, pos: &str| {
        write_var_uint(m, 3);
        write_guid(m, ps, pl);
        write_cstr(m, pos);
    };
    let write_matrix = |m: &mut Vec<u8>, tx: f32, ty: f32| {
        write_var_uint(m, 8);
        for v in [1.0f32, 0.0, tx, 0.0, 1.0, ty] {
            write_var_float(m, v);
        }
    };
    let write_paint = |m: &mut Vec<u8>, tag: u64, rgba: [f32; 4]| {
        write_var_uint(m, tag);
        write_var_uint(m, 1); // 1 paint
        write_var_uint(m, 1);
        write_cstr(m, "SOLID");
        write_var_uint(m, 2);
        for c in rgba {
            write_var_float(m, c);
        }
        write_var_uint(m, 0);
    };

    let write_solid = |m: &mut Vec<u8>, rgba| write_paint(m, 9, rgba);
    let write_geometry = |m: &mut Vec<u8>, tag: u64| {
        write_var_uint(m, tag);
        write_var_uint(m, 1); // one path
        write_var_uint(m, 1);
        write_cstr(m, "NONZERO");
        write_var_uint(m, 2);
        write_var_uint(m, 1); // commandsBlob = 1
        write_var_uint(m, 0);
    };

    let write_outline = |m: &mut Vec<u8>| write_geometry(m, 10);
    let write_binding = |m: &mut Vec<u8>, def_id, field| {
        m.extend_from_slice(&[23, 4, 1, 5]); // parameterConsumptionMap.entries[0].variableField
        write_cstr(m, field);
        m.extend_from_slice(&[6, 2, 7, 8]); // variableData.value.propRefValue.defId
        write_guid(m, 5, def_id);
        m.extend_from_slice(&[0, 0, 0, 0, 0]);
    };
    let write_label_assignment = |m: &mut Vec<u8>, label: &str| {
        write_var_uint(m, 1);
        write_guid(m, 5, 42);
        m.extend_from_slice(&[2, 3, 1]); // value.textValue.characters
        write_cstr(m, label);
        m.extend_from_slice(&[0, 0, 0]);
    };

    // Binary `commandsBlob` (blob ID = 1 with `blobBaseIndex = 1` -> `blobs[0]`):
    // M(10, 20), L(90, 20), Q(100, 50, 90, 80), C(60, 90, 30, 90, 10, 80), Z
    let mut cmd_blob = Vec::new();
    cmd_blob.push(1); // MoveTo
    for f in [10.0f32, 20.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(2); // LineTo
    for f in [90.0f32, 20.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(3); // QuadTo
    for f in [100.0f32, 50.0, 90.0, 80.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(4); // CubicTo
    for f in [60.0f32, 90.0, 30.0, 90.0, 10.0, 80.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(0); // Close

    // Binary `vectorNetworkBlob` (blob ID = 2 with `blobBaseIndex = 1` -> `blobs[1]`):
    // 3 vertices, 3 segments (1 cubic + 2 linear), 1 closed EVENODD region loop
    let mut vn_blob = Vec::new();
    vn_blob.extend_from_slice(&3u32.to_le_bytes()); // v_count
    vn_blob.extend_from_slice(&3u32.to_le_bytes()); // s_count
    vn_blob.extend_from_slice(&1u32.to_le_bytes()); // r_count
    // Vertices: (style_u32, x_f32, y_f32)
    for (vx, vy) in [(0.0f32, 0.0f32), (80.0, 0.0), (40.0, 70.0)] {
        vn_blob.extend_from_slice(&0u32.to_le_bytes());
        vn_blob.extend_from_slice(&vx.to_le_bytes());
        vn_blob.extend_from_slice(&vy.to_le_bytes());
    }
    // Segments: (style_u32, start_v, dx1, dy1, end_v, dx2, dy2)
    for (sv, dx1, dy1, ev, dx2, dy2) in [
        (0u32, 20.0f32, -15.0f32, 1u32, -20.0f32, -15.0f32),
        (1, 0.0, 0.0, 2, 0.0, 0.0),
        (2, 0.0, 0.0, 0, 0.0, 0.0),
    ] {
        vn_blob.extend_from_slice(&0u32.to_le_bytes());
        vn_blob.extend_from_slice(&sv.to_le_bytes());
        vn_blob.extend_from_slice(&dx1.to_le_bytes());
        vn_blob.extend_from_slice(&dy1.to_le_bytes());
        vn_blob.extend_from_slice(&ev.to_le_bytes());
        vn_blob.extend_from_slice(&dx2.to_le_bytes());
        vn_blob.extend_from_slice(&dy2.to_le_bytes());
    }
    // Region 0: style_and_rule = 0 (EVENODD -> bit 0 == 0), loop_count = 1, seg_count = 3, [0, 1, 2]
    vn_blob.extend_from_slice(&0u32.to_le_bytes());
    vn_blob.extend_from_slice(&1u32.to_le_bytes());
    vn_blob.extend_from_slice(&3u32.to_le_bytes());
    for si in [0u32, 1, 2] {
        vn_blob.extend_from_slice(&si.to_le_bytes());
    }

    let img_sha_bytes: [u8; 20] = [
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15,
    ];
    let img_sha_hex: String = img_sha_bytes.iter().map(|b| format!("{b:02x}")).collect();

    let mut msg = Vec::new();
    // Test skipping unused field tag 99 before/after main fields
    write_var_uint(&mut msg, 99);
    write_cstr(&mut msg, "ignored-metadata");
    // blobBaseIndex = 1
    write_var_uint(&mut msg, 3);
    write_var_uint(&mut msg, 1);

    // nodeChanges:
    // 0: DOCUMENT (0:0)
    // 1: Internal Only Canvas (0:99, internalOnly=true) — put FIRST to prove it is never chosen as Stage!
    // 2: Master SYMBOL (10:1, parent 0:99, "ButtonMaster", 180x48)
    // 3: Master child RECTANGLE (10:2, parent 10:1, "Bg")
    // 4: Master child TEXT (10:3, parent 10:1, "Label", "Click")
    // 5: User Page 1 CANVAS (0:1, "Home Screen")
    // 6: Artboard FRAME (2:1, parent 0:1, "Hero Screen", 1440x900)
    // 7: Overridden INSTANCE (2:2, parent 2:1, symbolID=10:1, overrides 10:2 fill & 10:3 text="Submit Now")
    // 8: Asymmetric ROUNDED_RECTANGLE (2:3, parent 2:1, corners 4/12/20/28 + oversized 4200x64 image fill)
    // 9: VECTOR with commandsBlob (2:4, parent 2:1)
    // 10: VECTOR with vectorNetworkBlob (2:5, parent 2:1)
    // 11: Multi-run TEXT (2:6, parent 2:1, "Hello World" with bold accent run)
    // 12: User Page 2 CANVAS (0:2, "Settings Screen") + child RECTANGLE (3:1, parent 0:2) + a stroke-only vector (3:2)
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 19);

    // 0: DOCUMENT 0:0
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 0);
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "DOCUMENT");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Document");
    write_var_uint(&mut msg, 0);

    // 1: Internal Only Canvas 0:99 (position "!" — sorts BEFORE user pages!)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 99);
    write_parent(&mut msg, 0, 0, "!");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Internal Only Canvas");
    write_var_uint(&mut msg, 6);
    msg.push(1); // internalOnly = true
    write_var_uint(&mut msg, 0);

    // 2: Master SYMBOL 10:1 inside 0:99
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 1);
    write_parent(&mut msg, 0, 99, "!");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "SYMBOL");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "ButtonMaster");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 180.0);
    write_var_float(&mut msg, 48.0);
    write_matrix(&mut msg, 0.0, 0.0);
    write_solid(&mut msg, [0.1, 0.1, 0.1, 1.0]);
    write_outline(&mut msg);
    write_var_uint(&mut msg, 0);

    // 3: Master child RECTANGLE 10:2
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 2);
    write_parent(&mut msg, 10, 1, "a");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "RECTANGLE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "BtnBg");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 180.0);
    write_var_float(&mut msg, 48.0);
    write_matrix(&mut msg, 0.0, 0.0);
    write_solid(&mut msg, [0.2, 0.3, 0.8, 1.0]);
    write_binding(&mut msg, 43, "VISIBLE");
    write_var_uint(&mut msg, 0);

    // 4: Master child TEXT 10:3
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 3);
    write_parent(&mut msg, 10, 1, "b");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "TEXT");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "BtnLabel");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 160.0);
    write_var_float(&mut msg, 24.0);
    write_matrix(&mut msg, 10.0, 12.0);
    write_solid(&mut msg, [1.0, 1.0, 1.0, 1.0]);
    write_var_uint(&mut msg, 12); // textData
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Click");
    write_var_uint(&mut msg, 0); // end textData
    write_var_uint(&mut msg, 13); // fontName
    write_cstr(&mut msg, "Inter");
    write_cstr(&mut msg, "Medium");
    write_var_uint(&mut msg, 14); // fontSize
    write_var_float(&mut msg, 16.0);
    write_binding(&mut msg, 42, "TEXT_DATA");
    write_var_uint(&mut msg, 0);

    // 5: User Page 1 CANVAS 0:1
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 1);
    write_parent(&mut msg, 0, 0, "a");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Home Screen");
    write_var_uint(&mut msg, 0);

    // 6: Artboard FRAME 2:1 (1440x900)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 1);
    write_parent(&mut msg, 0, 1, "a");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "FRAME");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Hero Screen");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 1440.0);
    write_var_float(&mut msg, 900.0);
    write_matrix(&mut msg, 0.0, 0.0);
    write_solid(&mut msg, [0.1, 0.1, 0.1, 1.0]);
    write_outline(&mut msg);
    write_var_uint(&mut msg, 0);

    // 7: INSTANCE 2:2 referencing 10:1 with symbolOverrides for 10:2 (green fill) & 10:3 ("Submit Now")
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 2);
    write_parent(&mut msg, 2, 1, "a");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "INSTANCE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Submit CTA");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 180.0);
    write_var_float(&mut msg, 48.0);
    write_matrix(&mut msg, 60.0, 80.0);
    write_var_uint(&mut msg, 15); // symbolData
    write_var_uint(&mut msg, 1); // symbolID = 10:1
    write_guid(&mut msg, 10, 1);
    write_var_uint(&mut msg, 2); // symbolOverrides len = 2
    write_var_uint(&mut msg, 2);
    // Override 0: guidPath=[10:2], green fill
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 2);
    write_solid(&mut msg, [0.1, 0.85, 0.4, 1.0]);
    write_var_uint(&mut msg, 0);
    // Override 1: guidPath=[10:3], textData.characters = "Submit Now"
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 3);
    write_var_uint(&mut msg, 12);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Submit Now");
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0); // end symbolData
    write_var_uint(&mut msg, 24); // derivedSymbolData, one text box in resized instance space
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 3);
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 126.0);
    write_var_float(&mut msg, 32.0);
    write_matrix(&mut msg, 18.0, 7.0);
    write_var_uint(&mut msg, 25);
    write_cstr(&mut msg, "opaque glyph payload");
    write_var_uint(&mut msg, 0);
    msg.extend_from_slice(&[22, 1]); // componentPropAssignments
    write_label_assignment(&mut msg, "Superseded by direct override");
    write_var_uint(&mut msg, 0); // end INSTANCE

    // 8: Asymmetric ROUNDED_RECTANGLE 2:3 with corners [tl=4, tr=12, br=20, bl=28] + image fill
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 3);
    write_parent(&mut msg, 2, 1, "b");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "ROUNDED_RECTANGLE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Asym Card");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 320.0);
    write_var_float(&mut msg, 200.0);
    write_matrix(&mut msg, 300.0, 80.0);
    write_var_uint(&mut msg, 9); // fillPaints: SOLID + IMAGE (2 fills!)
    write_var_uint(&mut msg, 2);
    // Fill 0: SOLID
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "SOLID");
    write_var_uint(&mut msg, 2);
    for c in [0.15f32, 0.18, 0.25, 1.0] {
        write_var_float(&mut msg, c);
    }
    write_var_uint(&mut msg, 0);
    // Fill 1: IMAGE referencing `img_sha_bytes`
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "IMAGE");
    write_var_uint(&mut msg, 3); // Image message
    write_var_uint(&mut msg, 1); // hash byte[]
    write_var_uint(&mut msg, img_sha_bytes.len() as u64);
    msg.extend_from_slice(&img_sha_bytes);
    write_var_uint(&mut msg, 0); // end Image
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "FILL");
    write_var_uint(&mut msg, 0); // end Paint 1
    write_var_uint(&mut msg, 16);
    write_var_float(&mut msg, 4.0); // TL
    write_var_uint(&mut msg, 17);
    write_var_float(&mut msg, 12.0); // TR
    write_var_uint(&mut msg, 18);
    write_var_float(&mut msg, 20.0); // BR
    write_var_uint(&mut msg, 19);
    write_var_float(&mut msg, 28.0); // BL
    write_var_uint(&mut msg, 0);

    // 9: VECTOR 2:4 with `commandsBlob = 1` (`blobBaseIndex = 1` -> `blobs[0]`)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 4);
    write_parent(&mut msg, 2, 1, "c");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "VECTOR");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Blob Badge");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 100.0);
    write_var_float(&mut msg, 100.0);
    write_matrix(&mut msg, 680.0, 80.0);
    write_solid(&mut msg, [0.95, 0.45, 0.2, 1.0]);
    write_outline(&mut msg);
    write_var_uint(&mut msg, 0);

    // 10: VECTOR 2:5 with `vectorNetworkBlob = 2` (`blobBaseIndex = 1` -> `blobs[1]`)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 5);
    write_parent(&mut msg, 2, 1, "d");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "VECTOR");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Network Crest");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 80.0);
    write_var_float(&mut msg, 70.0);
    write_matrix(&mut msg, 820.0, 80.0);
    write_solid(&mut msg, [0.5, 0.8, 1.0, 1.0]);
    write_var_uint(&mut msg, 11); // vectorData
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2); // vectorNetworkBlob = 2
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);

    // 11: Multi-run TEXT 2:6 ("Hello World", "World" styled Bold 24px gold via styleID=7)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 6);
    write_parent(&mut msg, 2, 1, "e");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "TEXT");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Rich Heading");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 360.0);
    write_var_float(&mut msg, 48.0);
    write_matrix(&mut msg, 60.0, 320.0);
    write_solid(&mut msg, [0.9, 0.92, 0.96, 1.0]);
    write_var_uint(&mut msg, 12); // textData
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Hello World");
    write_var_uint(&mut msg, 2); // characterStyleIDs: 6 zeros ("Hello ") + 5 sevens ("World")
    write_var_uint(&mut msg, 11);
    for sid in [0u64, 0, 0, 0, 0, 0, 7, 7, 7, 7, 7] {
        write_var_uint(&mut msg, sid);
    }
    write_var_uint(&mut msg, 3); // styleOverrideTable len=1
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1); // styleID = 7
    write_var_uint(&mut msg, 7);
    write_var_uint(&mut msg, 2); // fontName = Inter Bold
    write_cstr(&mut msg, "Inter");
    write_cstr(&mut msg, "Bold Italic");
    write_var_uint(&mut msg, 3); // fontSize = 24
    write_var_float(&mut msg, 24.0);
    write_var_uint(&mut msg, 4); // fillPaints
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "SOLID");
    write_var_uint(&mut msg, 2);
    for c in [1.0f32, 0.85, 0.2, 1.0] {
        write_var_float(&mut msg, c);
    }
    write_var_uint(&mut msg, 0); // end Paint
    write_var_uint(&mut msg, 0); // end StyleOverride
    write_var_uint(&mut msg, 0); // end TextData
    write_var_uint(&mut msg, 13);
    write_cstr(&mut msg, "Inter");
    write_cstr(&mut msg, "Regular");
    write_var_uint(&mut msg, 14);
    write_var_float(&mut msg, 20.0);
    write_var_uint(&mut msg, 0);

    // 12: User Page 2 CANVAS 0:2 ("Settings Screen")
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 2);
    write_parent(&mut msg, 0, 0, "b");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Settings Screen");
    write_var_uint(&mut msg, 0);

    // 13: Settings Frame 3:1 inside 0:2
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 3, 1);
    write_parent(&mut msg, 0, 2, "a");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "RECTANGLE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Settings Card");
    write_var_uint(&mut msg, 7);
    write_var_float(&mut msg, 400.0);
    write_var_float(&mut msg, 300.0);
    write_matrix(&mut msg, 20.0, 20.0);
    write_solid(&mut msg, [0.2, 0.25, 0.35, 1.0]);
    write_var_uint(&mut msg, 0);

    // Stroke-only icon with both a cached stroke outline and an editable network centerline.
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 3, 2);
    write_parent(&mut msg, 0, 2, "b");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "VECTOR");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Stroked icon");
    write_paint(&mut msg, 20, [0.2, 0.4, 0.6, 1.0]);
    write_geometry(&mut msg, 21);
    write_var_uint(&mut msg, 11); // vectorData
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 2); // vectorNetworkBlob
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);

    // Second instance binds the same master label solely through its public text property.
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 2, 7);
    write_parent(&mut msg, 2, 1, "f");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "INSTANCE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "File menu");
    msg.extend_from_slice(&[15, 1]); // symbolData.symbolID
    write_guid(&mut msg, 10, 1);
    write_var_uint(&mut msg, 0);
    msg.extend_from_slice(&[22, 3]); // componentPropAssignments
    write_label_assignment(&mut msg, "File");
    write_var_uint(&mut msg, 1); // defID
    write_guid(&mut msg, 5, 43);
    msg.extend_from_slice(&[2, 9, 0, 0, 0]); // value.boolValue = false
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 5, 44);
    msg.extend_from_slice(&[10, 2, 11]); // varValue.value.symbolIdValue
    write_guid(&mut msg, 10, 5);
    msg.extend_from_slice(&[0, 0, 0]);
    write_var_uint(&mut msg, 0);

    // The swapped glyph is on the internal page and only reachable via the property assignment.
    for (id, name, color) in [
        (4, "Default glyph", [0.9, 0.1, 0.1, 1.0]),
        (5, "Alternate glyph", [0.1, 0.2, 0.9, 1.0]),
    ] {
        write_var_uint(&mut msg, 1);
        write_guid(&mut msg, 10, id);
        write_parent(&mut msg, 0, 99, name);
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, "SYMBOL");
        write_var_uint(&mut msg, 5);
        write_cstr(&mut msg, name);
        write_solid(&mut msg, color);
        write_var_uint(&mut msg, 0);
    }
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 10, 6);
    write_parent(&mut msg, 10, 1, "c");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "INSTANCE");
    write_var_uint(&mut msg, 5);
    write_cstr(&mut msg, "Glyph");
    msg.extend_from_slice(&[15, 1]); // symbolData.symbolID
    write_guid(&mut msg, 10, 4);
    write_var_uint(&mut msg, 0);
    write_binding(&mut msg, 44, "OVERRIDDEN_SYMBOL_ID");
    write_var_uint(&mut msg, 0);

    // Message field 2: `blobs` = [cmd_blob, vn_blob]
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 2);
    for blob in [&cmd_blob, &vn_blob] {
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, blob.len() as u64);
        msg.extend_from_slice(blob);
        write_var_uint(&mut msg, 0);
    }
    write_var_uint(&mut msg, 0); // end Message

    // Chunk 0 = Deflate schema, Chunk 1 = Zstandard v106 message!
    let c0 = {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&schema).unwrap();
        enc.finish().unwrap()
    };
    let c1 = encode_zstd_for_test(&msg);
    let mut canvas_fig = Vec::new();
    canvas_fig.extend_from_slice(b"fig-kiwi");
    canvas_fig.extend_from_slice(&106u32.to_le_bytes());
    canvas_fig.extend_from_slice(&(c0.len() as u32).to_le_bytes());
    canvas_fig.extend_from_slice(&c0);
    canvas_fig.extend_from_slice(&(c1.len() as u32).to_le_bytes());
    canvas_fig.extend_from_slice(&c1);

    // Oversized 4200x32 PNG (> 4096px NeoFlash bitmap limit!) + unreferenced ZIP image entry
    let wide_img = image::RgbaImage::from_pixel(4200, 32, image::Rgba([40, 140, 255, 255]));
    let mut wide_png = Vec::new();
    wide_img
        .write_to(&mut Cursor::new(&mut wide_png), image::ImageFormat::Png)
        .unwrap();

    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zip_buf);
        let zopts = zip::write::SimpleFileOptions::default();
        zw.start_file("meta.json", zopts).unwrap();
        zw.write_all(br#"{"file_name":"Desktop v106 Design"}"#)
            .unwrap();
        zw.start_file("canvas.fig", zopts).unwrap();
        zw.write_all(&canvas_fig).unwrap();
        zw.start_file(format!("images/{img_sha_hex}"), zopts)
            .unwrap();
        zw.write_all(&wide_png).unwrap();
        // Unreferenced corrupt/unused entry must NOT be eagerly loaded!
        zw.start_file("images/deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", zopts)
            .unwrap();
        zw.write_all(b"not-referenced-so-never-decoded").unwrap();
        zw.finish().unwrap();
    }
    let fig_bytes = zip_buf.into_inner();

    let (project, report) =
        import_figma_bytes(&fig_bytes, "fallback", &FigmaImportOptions::default())
            .expect("v106 zstd .fig import");
    project.validate().expect("v106 imported project validates");
    assert!(report.can_import);
    assert_eq!(report.version.as_deref(), Some("fig-kiwi v106 (zstd)"));
    assert_eq!(project.name, "Desktop v106 Design");
    assert_eq!(project.width, 1440);
    assert_eq!(project.height, 900);

    // 1. Multi-page scenes: 2 user scenes ("Home Screen" + "Settings Screen"), NEVER "Internal Only Canvas"!
    assert_eq!(project.scenes.len(), 2);
    assert_eq!(project.scenes[0], project.root);
    for sid in &project.scenes {
        let s = project.symbol(sid).unwrap();
        assert_ne!(s.name, "Internal Only Canvas");
    }
    assert_eq!(
        project.symbol(&project.scenes[0]).unwrap().name,
        "Home Screen"
    );
    assert_eq!(
        project.symbol(&project.scenes[1]).unwrap().name,
        "Settings Screen"
    );

    // 2. Lazy image loading + automatic >4096px downscaling: only 1 resource loaded, width <= 4096!
    assert_eq!(project.resources.len(), 1);
    let decoded_res = project.resources[0].decode_image().unwrap();
    assert_eq!(decoded_res.width, 4096);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "figma-image-downscaled")
    );

    // 3. Master component from Internal Only Canvas + specialized override variant symbol!
    assert!(
        project
            .symbols
            .iter()
            .any(|s| s.name == "ButtonMaster" && s.folder == "Figma Components")
    );
    let has_override_text = project.symbols.iter().any(|s| {
        s.folder == "Figma Components"
            && s.timeline.layers.iter().any(|l| {
                l.keyframes.iter().any(|k| {
                    k.elements.iter().any(|el| match &el.kind {
                        ElementKind::TextBox {
                            text,
                            width,
                            height,
                            ..
                        } => text == "Submit Now" && *width == 126.0 && *height == 32.0,
                        _ => false,
                    })
                })
            })
    });
    assert!(
        has_override_text,
        "symbolOverrides text variant must be materialized"
    );

    let variant = project
        .symbols
        .iter()
        .find(|s| s.name == "ButtonMaster Variant")
        .unwrap();
    let background = &variant.timeline.layers[0].keyframes[0].elements;
    assert!(matches!(
        background.as_slice(),
        [Element {
            kind: ElementKind::Path { .. },
            ..
        }]
    ));

    let file_instance = &project
        .symbol(&project.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "File menu")
        .unwrap()
        .keyframes[0]
        .elements[0];
    let ElementKind::Instance { symbol_id, .. } = &file_instance.kind else {
        panic!("Expected an editable component instance")
    };
    assert!(
        project
            .symbol(symbol_id)
            .unwrap()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes[0].elements)
            .any(|e| matches!(&e.kind, ElementKind::TextBox { text, .. } if text == "File"))
    );
    assert!(
        !project
            .to_json()
            .unwrap()
            .contains("Superseded by direct override")
    );

    // 4. Asymmetric corner radii [tl=4, tr=12, br=20, bl=28] -> NeoFlash [tl=4, tr=12, bl=28, br=20]
    //    AND rounded bitmap fill preserved via `AuthoredBitmapFill`!
    let home = project.symbol(&project.root).unwrap();
    let asym_layer = home
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Asym Card")
        .expect("Asym Card layer");
    assert_eq!(asym_layer.keyframes[0].bitmap_fills.len(), 1);
    let rect_el = asym_layer.keyframes[0]
        .elements
        .iter()
        .find(|e| matches!(e.kind, ElementKind::Rectangle { .. }))
        .expect("Asym Card base rectangle");
    assert_eq!(
        rect_el.transform.primitive,
        Some(PrimitiveOptions::Rectangle(RectangleOptions {
            corner_radii: [4.0, 12.0, 28.0, 20.0],
            corners_locked: false,
        }))
    );

    // 5. Binary `commandsBlob` & `vectorNetworkBlob` decoded into non-empty vector `Path`s!
    let blob_layer = home
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Blob Badge")
        .unwrap();
    assert!(matches!(
        &blob_layer.keyframes[0].elements[0].kind,
        ElementKind::Path { commands, .. } if commands.len() >= 5
    ));
    let vn_layer = home
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Network Crest")
        .unwrap();
    assert!(matches!(
        &vn_layer.keyframes[0].elements[0].kind,
        ElementKind::Path { commands, fill_rule: FillRule::EvenOdd, .. } if commands.len() >= 4
    ));

    let icon_layer = project
        .symbol(&project.scenes[1])
        .unwrap()
        .timeline
        .layers
        .iter()
        .find(|layer| layer.name == "Stroked icon")
        .unwrap();
    assert!(matches!(&icon_layer.keyframes[0].elements[0].kind,
        ElementKind::Path { commands, stroke, fill, stroke_width, .. }
        if *stroke == [51, 102, 153, 255] && fill[3] == 0 && *stroke_width == 1.0
           && matches!(commands.as_slice(), [PathCommand::MoveTo { x: 0.0, y: 0.0 },
               PathCommand::CubicTo { x: 80.0, y: 0.0, .. }, ..])
    ));

    // 6. Multi-run `TEXT` decoded into `TextBox` with bold italic gold run!
    let text_layer = home
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Rich Heading")
        .unwrap();
    match &text_layer.keyframes[0].elements[0].kind {
        ElementKind::TextBox { text, format, .. } => {
            assert_eq!(text, "Hello World");
            let opts = format.field_options().expect("multi-run field_options");
            assert!(opts.format_runs.iter().any(|r| {
                r.format.bold
                    && r.format.italic
                    && (r.format.size - 24.0).abs() < 1e-6
                    && r.format.color == 0xFF_D9_33
            }));
        }
        other => panic!("expected multi-run Text/TextBox, got {other:?}"),
    }

    // 7. Full `.nflash` JSON round-trip + scene flattening round-trip!
    let serialized = project.to_json().expect(".nflash JSON serialization");
    let reloaded = Project::from_json(&serialized).expect(".nflash JSON deserialization");
    reloaded.validate().expect("reloaded .nflash validates");
    let (flattened, ranges) = neoflash_core::flatten_scenes_with_ranges(&reloaded);
    flattened
        .validate()
        .expect("flattened multi-scene project validates");
    assert_eq!(ranges.len(), 2);

    // 8. Binary `.fig` Kiwi fast page inspection + selective 1-page-per-layer mapping (`T-391`)!
    let bin_summary = inspect_figma_bytes(&fig_bytes, "fallback").expect("binary .fig inspect");
    assert_eq!(bin_summary.name, "Desktop v106 Design");
    assert_eq!(bin_summary.version_label, "fig-kiwi v106 (zstd)");
    assert_eq!(
        bin_summary
            .pages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Home Screen", "Settings Screen"],
        "Internal Only Canvas must be excluded from binary .fig page summary"
    );

    let (layered_proj, _) = import_figma_bytes(
        &fig_bytes,
        "fallback",
        &FigmaImportOptions {
            selected_page_ids: Some(vec![
                bin_summary.pages[1].id.clone(),
                bin_summary.pages[0].id.clone(),
            ]),
            page_mapping: FigmaPageMapping::Layers,
            ..FigmaImportOptions::default()
        },
    )
    .expect("binary .fig 1-page-per-layer selective import");
    layered_proj
        .validate()
        .expect("binary .fig layered validates");
    assert!(layered_proj.scenes.is_empty());
    let layered_root = layered_proj.symbol(&layered_proj.root).unwrap();
    assert_eq!(
        layered_root
            .timeline
            .layers
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Settings Screen", "Home Screen"]
    );

    // Figma also emits fillGeometry on frames containing editable artwork. The
    // frame's path is its own background, underneath the text, vectors and instances.
    let artboard = &layered_root.timeline.layers[1].keyframes[0].elements[0];
    let ElementKind::Instance { symbol_id, .. } = &artboard.kind else {
        panic!(
            "Expected layered frame with its contents, got {:?}",
            artboard.kind
        );
    };
    let frame_layers = &layered_proj.symbol(symbol_id).unwrap().timeline.layers;
    let file_slot = &frame_layers
        .iter()
        .find(|l| l.name == "File menu")
        .unwrap()
        .keyframes[0]
        .elements[0];
    let ElementKind::Instance { symbol_id, .. } = &file_slot.kind else {
        panic!("File slot must resolve")
    };
    let file_layers = &layered_proj.symbol(symbol_id).unwrap().timeline.layers;
    assert!(
        !file_layers
            .iter()
            .find(|l| l.name == "BtnBg")
            .unwrap()
            .visible
    );
    let glyph_slot = &file_layers
        .iter()
        .find(|l| l.name == "Glyph")
        .unwrap()
        .keyframes[0]
        .elements[0];
    let ElementKind::Instance { symbol_id, .. } = &glyph_slot.kind else {
        panic!("Glyph slot must resolve")
    };
    assert_eq!(
        layered_proj.symbol(symbol_id).unwrap().name,
        "Alternate glyph"
    );

    assert_eq!(frame_layers[0].name, "Background");
    assert!(matches!(
        frame_layers[0].keyframes[0].elements[0].kind,
        ElementKind::Path { .. }
    ));
    assert!(
        frame_layers
            .iter()
            .any(|l| l.keyframes[0].elements.iter().any(|el| {
                matches!(&el.kind, ElementKind::TextBox { text, .. } if text == "Hello World")
            }))
    );

    // Selecting ONLY "Settings Screen" skips the 5000px image on "Home Screen" and `ButtonMaster`!
    let (single_page_proj, _) = import_figma_bytes(
        &fig_bytes,
        "fallback",
        &FigmaImportOptions {
            selected_page_ids: Some(vec![bin_summary.pages[1].id.clone()]),
            page_mapping: FigmaPageMapping::Layers,
            ..FigmaImportOptions::default()
        },
    )
    .expect("binary .fig single-page selective import");
    single_page_proj.validate().unwrap();
    assert_eq!(
        single_page_proj
            .symbol(&single_page_proj.root)
            .unwrap()
            .timeline
            .layers
            .len(),
        1
    );
    assert!(
        single_page_proj.resources.is_empty(),
        "unselected page image must not be decoded or imported"
    );
    assert!(
        !single_page_proj
            .symbols
            .iter()
            .any(|s| s.name == "ButtonMaster"),
        "unreferenced Internal Only Canvas master must not be materialized"
    );
}

#[test]
#[ignore = "reads local unversioned Figma corpus and design-system .fig files when present"]
fn real_world_figma_corpus_imports_and_validates() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let worktree_root = manifest_dir.parent().and_then(Path::parent).unwrap();
    let common_root = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(worktree_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| worktree_root.to_path_buf());

    // Exercise the owner's local import fixtures without embedding their names or document text.
    let mut files = Vec::new();
    for dir in [
        common_root.join("interface/figma"),
        common_root.join("target/figma-corpus"),
        common_root.join("tests"),
    ] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut dir_files: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && {
                        let low = p
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_ascii_lowercase();
                        low.ends_with(".fig") || low.ends_with(".figma.json")
                    }
                })
                .collect();
            dir_files.sort();
            files.extend(dir_files);
        }
    }

    // 1. Byte-for-byte Zstd cross-check on any discovered `.fig` ZIP archive whose `canvas.fig`
    //    chunk 1 uses Zstandard compression.
    for (idx, fig_path) in files.iter().enumerate() {
        let is_fig_zip = fig_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fig"));
        if !is_fig_zip {
            continue;
        }
        let Ok(file) = File::open(fig_path) else {
            continue;
        };
        let Ok(mut za) = ZipArchive::new(file) else {
            continue;
        };
        let mut canvas_bytes = Vec::new();
        let Ok(mut canvas_entry) = za.by_name("canvas.fig") else {
            continue;
        };
        if canvas_entry.read_to_end(&mut canvas_bytes).is_err() || canvas_bytes.len() < 20 {
            continue;
        }
        let c0_len = u32::from_le_bytes(canvas_bytes[12..16].try_into().unwrap()) as usize;
        let c1_off = 16 + c0_len;
        if canvas_bytes.len() < c1_off + 4 {
            continue;
        }
        let c1_len =
            u32::from_le_bytes(canvas_bytes[c1_off..c1_off + 4].try_into().unwrap()) as usize;
        if canvas_bytes.len() < c1_off + 4 + c1_len {
            continue;
        }
        let c1_compressed = &canvas_bytes[c1_off + 4..c1_off + 4 + c1_len];
        if !super::zstd::is_zstd_magic(c1_compressed) {
            continue;
        }
        let rust_decompressed = super::zstd::decompress_zstd(c1_compressed, MAX_FIGMA_DECODE_BYTES)
            .unwrap_or_else(|e| {
                panic!("pure-Rust Zstd decode failed on corpus archive #{idx}: {e}")
            });
        assert!(!rust_decompressed.is_empty());

        if let Ok(mut child) = std::process::Command::new("/opt/homebrew/bin/zstd")
            .args(["-d", "-c"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            let writer = child.stdin.take().map(|mut stdin| {
                let owned = c1_compressed.to_vec();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(&owned);
                })
            });
            let cli_res = child.wait_with_output();
            if let Some(handle) = writer {
                let _ = handle.join();
            }
            if let Ok(cli_out) = cli_res
                && cli_out.status.success()
            {
                assert_eq!(
                    rust_decompressed, cli_out.stdout,
                    "pure-Rust Zstd output must match system zstd byte-for-byte on corpus archive #{idx}"
                );
            }
        }
    }

    // 2. Exercise both a selective layer import and opening the entire document as scenes.

    let mut imported_count = 0usize;
    let mut total_symbols = 0usize;
    let mut total_layers = 0usize;
    let mut total_elements = 0usize;
    let mut total_inspect_ms = 0.0f64;
    let mut max_inspect_ms = 0.0f64;
    let mut total_selective_ms = 0.0f64;
    let mut max_selective_ms = 0.0f64;
    let mut total_full_ms = 0.0f64;

    for path in &files {
        let t_ins = std::time::Instant::now();
        let summary = inspect_figma_path(path).unwrap_or_else(|e| {
            panic!("corpus file #{imported_count} failed inspect_figma_path: {e}")
        });
        let ins_ms = t_ins.elapsed().as_secs_f64() * 1000.0;
        total_inspect_ms += ins_ms;
        max_inspect_ms = max_inspect_ms.max(ins_ms);
        assert!(
            !summary.pages.is_empty(),
            "corpus file #{imported_count} summary had 0 user pages"
        );

        // Benchmark importing the first visible, non-empty page into a single layer.
        let first_page_id = summary
            .pages
            .iter()
            .find(|p| p.visible && p.node_count > 0)
            .or_else(|| summary.pages.iter().find(|p| p.node_count > 0))
            .unwrap_or(&summary.pages[0])
            .id
            .clone();
        let sel_opts = FigmaImportOptions {
            import_hidden_layers: false,
            selected_page_ids: Some(vec![first_page_id]),
            page_mapping: FigmaPageMapping::Layers,
            ..FigmaImportOptions::default()
        };
        let t_sel = std::time::Instant::now();
        let (sel_proj, sel_rep) = import_figma_path(path, &sel_opts).unwrap_or_else(|e| {
            panic!("corpus file #{imported_count} failed selective import_figma_path: {e}")
        });
        let sel_ms = t_sel.elapsed().as_secs_f64() * 1000.0;
        total_selective_ms += sel_ms;
        max_selective_ms = max_selective_ms.max(sel_ms);
        assert!(sel_rep.can_import);
        sel_proj
            .validate()
            .unwrap_or_else(|e| panic!("corpus file #{imported_count} selective validate(): {e}"));
        let sel_root = sel_proj.symbol(&sel_proj.root).unwrap();
        assert_eq!(sel_root.timeline.layers.len(), 1);
        assert!(sel_proj.scenes.is_empty());

        let t_full = std::time::Instant::now();
        let (project, report) = import_figma_path(path, &FigmaImportOptions::default())
            .unwrap_or_else(|e| {
                panic!("corpus file #{imported_count} failed import_figma_path: {e}")
            });
        total_full_ms += t_full.elapsed().as_secs_f64() * 1000.0;
        assert!(
            report.can_import,
            "corpus file #{imported_count} can_import was false"
        );
        project
            .validate()
            .unwrap_or_else(|e| panic!("corpus file #{imported_count} failed validate(): {e}"));
        let json = project
            .to_json()
            .unwrap_or_else(|e| panic!("corpus file #{imported_count} failed to_json(): {e}"));
        assert!(json.len() <= neoflash_core::MAX_PROJECT_JSON_BYTES);

        // Also verify `crate::inspect` (File -> Open container probe in `source.rs`) succeeds!
        let inspected = crate::inspect(path)
            .unwrap_or_else(|e| panic!("corpus file #{imported_count} failed inspect(): {e}"));
        assert!(inspected.can_import);

        total_symbols += project.symbols.len();
        total_layers += report.statistics.layers;
        total_elements += report.statistics.elements;
        imported_count += 1;
    }
    println!(
        "Verified {imported_count} real Figma document(s): {total_symbols} symbols, {total_layers} layers, {total_elements} elements | inspect total={total_inspect_ms:.1}ms (max={max_inspect_ms:.1}ms) | selective-1page total={total_selective_ms:.1}ms (max={max_selective_ms:.1}ms) | full-all-pages total={total_full_ms:.1}ms"
    );
}

#[test]
fn t391_inspect_figma_bytes_and_page_selective_layers_mapping() {
    let png_used_b64 = BASE64.encode(tiny_png_bytes());
    let png_unused_b64 = BASE64.encode(tiny_png_bytes());
    let figma_json = format!(
        r#"{{
          "name": "Multi Page Product Kit",
          "schemaVersion": 0,
          "images": {{
            "img_used": "{png_used_b64}",
            "img_unused_internal": "{png_unused_b64}"
          }},
          "document": {{
            "id": "0:0",
            "name": "Document",
            "type": "DOCUMENT",
            "children": [
              {{
                "id": "0:99",
                "name": "Internal Only Canvas",
                "type": "CANVAS",
                "internalOnly": true,
                "children": [
                  {{
                    "id": "90:1",
                    "name": "BadgeMaster",
                    "type": "COMPONENT",
                    "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 80, "height": 32}},
                    "children": [
                      {{
                        "id": "90:2",
                        "name": "BadgeBg",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 80, "height": 32}},
                        "fills": [{{"type": "IMAGE", "imageRef": "img_used"}}]
                      }}
                    ]
                  }},
                  {{
                    "id": "99:1",
                    "name": "UnusedHeavyMaster",
                    "type": "COMPONENT",
                    "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 400, "height": 400}},
                    "children": [
                      {{
                        "id": "99:2",
                        "name": "UnusedHeavyImg",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 400, "height": 400}},
                        "fills": [{{"type": "IMAGE", "imageRef": "img_unused_internal"}}]
                      }}
                    ]
                  }}
                ]
              }},
              {{
                "id": "0:1",
                "name": "Cover",
                "type": "CANVAS",
                "children": [
                  {{
                    "id": "1:1",
                    "name": "Cover Artboard",
                    "type": "FRAME",
                    "clipsContent": true,
                    "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 1200, "height": 630}},
                    "fills": [{{"type": "SOLID", "color": {{"r": 0.1, "g": 0.12, "b": 0.18, "a": 1.0}}}}],
                    "fillGeometry": [{{"path": "M0 0H1200V630H0Z"}}],
                    "children": [
                      {{
                        "id": "1:2",
                        "name": "Cover Title",
                        "type": "TEXT",
                        "characters": "Design Kit v3",
                        "absoluteBoundingBox": {{"x": 40, "y": 60, "width": 400, "height": 48}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                        "style": {{"fontFamily": "Inter", "fontSize": 36, "fontWeight": 700}}
                      }},
                      {{
                        "id": "1:4",
                        "type": "BOOLEAN_OPERATION",
                        "name": "Cutout",
                        "fillGeometry": [{{"path": "M0 0H40V40H0Z M10 10V30H30V10Z", "windingRule": "EVENODD"}}],
                        "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 0, "b": 0}}}}],
                        "children": [{{"id": "1:5", "type": "RECTANGLE", "name": "Operand"}}]
                      }},
                      {{
                        "id": "1:3",
                        "name": "Hidden Draft Note",
                        "type": "RECTANGLE",
                        "visible": false,
                        "absoluteBoundingBox": {{"x": 40, "y": 140, "width": 100, "height": 40}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 0, "b": 0, "a": 1}}}}]
                      }}
                    ]
                  }}
                ]
              }},
              {{
                "id": "0:2",
                "name": "Dashboard",
                "type": "CANVAS",
                "children": [
                  {{
                    "id": "2:1",
                    "name": "Dash Frame",
                    "type": "FRAME",
                    "clipsContent": true,
                    "absoluteBoundingBox": {{"x": 50, "y": 20, "width": 1440, "height": 900}},
                    "children": [
                      {{
                        "id": "2:2",
                        "name": "Badge Instance",
                        "type": "INSTANCE",
                        "componentId": "90:1",
                        "absoluteBoundingBox": {{"x": 70, "y": 40, "width": 80, "height": 32}}
                      }}
                    ]
                  }}
                ]
              }},
              {{
                "id": "0:3",
                "name": "Settings",
                "type": "CANVAS",
                "children": [
                  {{
                    "id": "3:1",
                    "name": "Settings Modal",
                    "type": "FRAME",
                    "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 640, "height": 480}},
                    "children": [
                      {{
                        "id": "3:2",
                        "name": "Avatar Mask",
                        "type": "ELLIPSE",
                        "isMask": true,
                        "maskType": "VECTOR",
                        "absoluteBoundingBox": {{"x": 20, "y": 20, "width": 64, "height": 64}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}]
                      }},
                      {{
                        "id": "3:3",
                        "name": "Avatar Fill",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": {{"x": 20, "y": 20, "width": 64, "height": 64}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.2, "g": 0.6, "b": 1.0, "a": 1}}}}]
                      }}
                    ]
                  }}
                ]
              }},
              {{
                "id": "0:4",
                "name": "Unselected Scratchpad",
                "type": "CANVAS",
                "visible": false,
                "children": [
                  {{
                    "id": "4:1",
                    "name": "Scratch Box",
                    "type": "RECTANGLE",
                    "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 200, "height": 200}},
                    "fills": [{{"type": "SOLID", "color": {{"r": 0.5, "g": 0.5, "b": 0.5, "a": 1}}}}]
                  }}
                ]
              }}
            ]
          }}
        }}"#
    );

    let summary =
        inspect_figma_bytes(figma_json.as_bytes(), "fallback").expect("inspect_figma_bytes");
    assert_eq!(summary.name, "Multi Page Product Kit");
    assert_eq!(
        summary.pages.len(),
        4,
        "Internal Only Canvas must be excluded"
    );
    assert_eq!(summary.pages[0].id, "0:1");
    assert_eq!(summary.pages[0].name, "Cover");
    assert!(summary.pages[0].visible);
    assert_eq!(summary.pages[0].top_frame_count, 1);
    assert_eq!(summary.pages[0].node_count, 5);
    assert_eq!(summary.pages[3].id, "0:4");
    assert!(!summary.pages[3].visible);

    // Select 3 pages in custom order (`Dashboard`, `Cover`, `Settings`), skipping `0:4` (`Unselected Scratchpad`)!
    let opts = FigmaImportOptions {
        import_hidden_layers: false,
        selected_page_ids: Some(vec!["0:2".into(), "0:1".into(), "0:3".into()]),
        page_mapping: FigmaPageMapping::Layers,
        ..FigmaImportOptions::default()
    };
    let (project, report) =
        import_figma_bytes(figma_json.as_bytes(), "Kit", &opts).expect("3-page Layers import");
    assert!(report.can_import);
    project.validate().expect("3-page Layers project validates");
    assert!(
        project.scenes.is_empty(),
        "Layers mapping must place all selected pages on root timeline without extra scenes"
    );
    // Stage dimensions come from the first selected page (`0:2` Dashboard = 1440x900).
    assert_eq!(project.width, 1440);
    assert_eq!(project.height, 900);

    let root = project.symbol(&project.root).unwrap();
    let layer_names: Vec<&str> = root
        .timeline
        .layers
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(layer_names, vec!["Dashboard", "Cover", "Settings"]);

    let cover_el = &root.timeline.layers[1].keyframes[0].elements[0];
    let ElementKind::Instance { symbol_id, .. } = &cover_el.kind else {
        panic!("Expected cover frame to retain its children");
    };
    let cover = &project.symbol(symbol_id).unwrap().timeline.layers;
    assert_eq!(
        cover.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Background", "Frame Clip", "Cover Title", "Cutout"]
    );
    assert!(matches!(
        &cover[3].keyframes[0].elements[..],
        [Element {
            kind: ElementKind::Path {
                fill_rule: FillRule::EvenOdd,
                ..
            },
            ..
        }]
    ));
    assert_eq!(cover[3].properties.mask.as_ref(), Some(&cover[1].id));

    // Referenced master `BadgeMaster` (`90:1`) is materialized, while `UnusedHeavyMaster` (`99:1`)
    // and unselected `Scratch Box` (`4:1`) are NOT materialized, and only 1 image resource is imported!
    assert!(project.symbols.iter().any(|s| s.name == "BadgeMaster"));
    assert!(
        !project
            .symbols
            .iter()
            .any(|s| s.name == "UnusedHeavyMaster")
    );
    assert!(!project.symbols.iter().any(|s| s.name.contains("Scratch")));
    assert_eq!(project.resources.len(), 1);
}

#[test]
fn dense_page_preserves_named_layers_and_editable_translated_folders() {
    use serde_json::json;

    let leaf = |name: &str| {
        json!({
            "id": name, "name": name, "type": "RECTANGLE", "size":{"x":4,"y":5},
            "relativeTransform": [[1,0,2],[0,1,3]],
            "fills": [{"type":"SOLID","color":{"r":0.2,"g":0.3,"b":0.4}}]
        })
    };
    let mut children: Vec<_> = (0..100).map(|n| leaf(&format!("Tile {n}"))).collect();
    children.push(json!({
        "id":"folder", "name":"Translations", "type":"FRAME", "visible":false, "locked":true,
        "relativeTransform":[[1,0,20],[0,1,30]], "children":[leaf("Editable child")]
    }));
    children.push(json!({
        "id":"rotation", "name":"Rotation", "type":"FRAME",
        "relativeTransform":[[0,-1,20],[1,0,30]], "children":[leaf("Rotated child")]
    }));
    let file = json!({"document":{"type":"DOCUMENT", "children":[
      {"id":"first", "type":"CANVAS", "name":"Empty first", "children":[]}, {
        "id":"page", "type":"CANVAS", "name":"Page", "children": [{
            "id":"art", "name":"Art", "type":"FRAME", "size":{"x":100,"y":100},
            "children":children
        }]
    }, {"id":"last", "type":"CANVAS", "name":"Empty last", "children":[]}]}})
    .to_string();
    let (project, _) =
        import_figma_json_str(&file, "Page", &FigmaImportOptions::default()).unwrap();
    let scenes: Vec<_> = project
        .scenes
        .iter()
        .map(|id| project.symbol(id).unwrap())
        .collect();
    assert_eq!(
        scenes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Empty first", "Page", "Empty last"]
    );
    assert!(scenes.iter().all(|s| s.timeline.frame_count == 1));
    let layers = &scenes[1].timeline.layers;
    for (i, layer) in layers.iter().take(100).enumerate() {
        assert_eq!(layer.name, format!("Tile {i}"));
        assert_eq!(layer.keyframes[0].elements.len(), 1);
    }
    let folder = layers.iter().find(|l| l.name == "Translations").unwrap();
    assert_eq!(folder.properties.role, LayerRole::Folder);
    assert!(!folder.visible && folder.locked);
    let child = layers.iter().find(|l| l.name == "Editable child").unwrap();
    assert_eq!(child.properties.parent.as_ref(), Some(&folder.id));
    let tf = child.keyframes[0].elements[0].transform;
    assert_eq!((tf.x, tf.y), (22., 33.));
    let rotated = layers.iter().find(|l| l.name == "Rotation").unwrap();
    let element = &rotated.keyframes[0].elements[0];
    assert!(matches!(element.kind, ElementKind::Instance { .. }));
    assert_eq!(element.transform.rotation, 90.);
}

#[test]
fn figma_page_limits_locate_partial_and_skipped_pages_for_both_mappings() {
    use serde_json::json;

    let mut master = json!({
        "id": "m0", "type": "COMPONENT", "name": "Base",
        "children": [{"id": "leaf", "type": "RECTANGLE",
            "fills": [{"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0}}]}]
    });
    let mut masters = Vec::new();
    for level in 1..=10 {
        masters.push(master);
        let pair: Vec<_> = (0..2).map(|i| json!({
            "id": format!("m{level}-{i}"), "type": "INSTANCE", "componentId": format!("m{}", level - 1),
        })).collect();
        master = json!({"id": format!("m{level}"), "type": "COMPONENT", "name": format!("Pair {level}"), "children": pair});
    }
    masters.push(master);
    let page = |id: &str, count: usize| {
        let children: Vec<_> = (0..count)
            .map(|i| {
                json!({
                    "id": format!("{id}-{i}"), "type": "INSTANCE", "componentId": "m10",
                })
            })
            .collect();
        json!({"id": id, "type": "CANVAS", "name": id, "children": children})
    };
    let file = json!({"document": {"type": "DOCUMENT", "children": [
        {"id": "masters", "type": "CANVAS", "internalOnly": true, "children": masters},
        page("First", 1), page("Large", 60), page("Last", 1),
    ]}})
    .to_string();
    for mapping in [FigmaPageMapping::Layers, FigmaPageMapping::Scenes] {
        let options = FigmaImportOptions {
            selected_page_ids: Some(vec!["First".into(), "Large".into(), "Last".into()]),
            page_mapping: mapping,
            ..Default::default()
        };
        let (project, report) = import_figma_json_str(&file, "Page limits", &options).unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "figma-page-truncated" && d.location == "Large (Large)")
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "figma-page-skipped" && d.location == "Last (Last)"),
            mapping == FigmaPageMapping::Layers
        );
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.location == "First (First)")
        );
        let pages = if mapping == FigmaPageMapping::Layers {
            project.symbol(&project.root).unwrap().timeline.layers.len()
        } else {
            project.scenes.len()
        };
        assert_eq!(
            pages,
            if mapping == FigmaPageMapping::Layers {
                2
            } else {
                3
            }
        );
    }
}

#[test]
fn figma_image_crop_transform_inverts_uv_matrix_to_exact_bitmap_fill() {
    let img = image::RgbaImage::from_pixel(200, 100, image::Rgba([40, 160, 240, 255]));
    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .unwrap();
    let png_b64 = BASE64.encode(&png_bytes);

    let uv_t = [0.5_f64, 0.1, 0.25, -0.1, 0.5, 0.2];
    let file = serde_json::json!({
        "name": "Image Crop Fidelity",
        "images": { "img_crop": png_b64 },
        "document": {
            "id": "0:0",
            "type": "DOCUMENT",
            "children": [{
                "id": "0:1",
                "type": "CANVAS",
                "name": "Page 1",
                "children": [
                    {
                        "id": "1:1",
                        "name": "Cropped Photo",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 20.0, "y": 30.0, "width": 100.0, "height": 50.0 },
                        "fills": [{
                            "type": "IMAGE",
                            "imageRef": "img_crop",
                            "scaleMode": "CROP",
                            "imageTransform": [
                                [uv_t[0], uv_t[1], uv_t[2]],
                                [uv_t[3], uv_t[4], uv_t[5]]
                            ]
                        }]
                    },
                    {
                        "id": "1:2",
                        "name": "Retina Tile",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 140.0, "y": 30.0, "width": 100.0, "height": 50.0 },
                        "fills": [{
                            "type": "IMAGE",
                            "imageRef": "img_crop",
                            "scaleMode": "TILE",
                            "scalingFactor": 0.5
                        }]
                    }
                ]
            }]
        }
    })
    .to_string();

    let (project, report) =
        import_figma_json_str(&file, "Crop Test", &FigmaImportOptions::default()).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.severity != crate::Severity::Error),
        "diagnostics: {:?}",
        report.diagnostics
    );
    project.validate().expect("valid cropped project");

    let root = project.symbol(&project.root).unwrap();
    let crop_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Cropped Photo")
        .expect("Cropped Photo layer");
    let crop_kf = &crop_layer.keyframes[0];
    assert_eq!(crop_kf.elements.len(), 1);
    assert!(
        matches!(crop_kf.elements[0].kind, ElementKind::Path { .. }),
        "cropped rectangle must use clipped Path + AuthoredBitmapFill instead of uncropped Bitmap"
    );
    assert_eq!(crop_kf.bitmap_fills.len(), 1);
    let bf = &crop_kf.bitmap_fills[0];
    assert_eq!(bf.element_id, crop_kf.elements[0].id);
    assert!(!bf.repeat);

    let [a, b, c, d, tx, ty] = bf.matrix;
    for (lx, ly) in [(0.0, 0.0), (100.0, 0.0), (0.0, 50.0), (73.5, 29.25)] {
        let xn = lx / 100.0;
        let yn = ly / 50.0;
        let u = uv_t[0] * xn + uv_t[1] * yn + uv_t[2];
        let v = uv_t[3] * xn + uv_t[4] * yn + uv_t[5];
        let px = u * 200.0;
        let py = v * 100.0;
        let rx = a * px + c * py + tx;
        let ry = b * px + d * py + ty;
        assert!(
            (rx - lx).abs() < 1e-6 && (ry - ly).abs() < 1e-6,
            "expected ({lx}, {ly}), got ({rx}, {ry})"
        );
    }

    let tile_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Retina Tile")
        .expect("Retina Tile layer");
    let tile_bf = &tile_layer.keyframes[0].bitmap_fills[0];
    assert!(tile_bf.repeat);
    assert!((tile_bf.matrix[0] - 0.5).abs() < 1e-6);
    assert!((tile_bf.matrix[3] - 0.5).abs() < 1e-6);
}

#[test]
fn figma_layer_and_background_blur_preserve_editable_blur_filters_and_skip_zero_or_hidden() {
    let file = serde_json::json!({
        "name": "Blur Fidelity",
        "document": {
            "id": "0:0",
            "type": "DOCUMENT",
            "children": [{
                "id": "0:1",
                "type": "CANVAS",
                "name": "Page 1",
                "children": [
                    {
                        "id": "1:1",
                        "name": "Soft Badge",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 10.0, "y": 10.0, "width": 80.0, "height": 40.0 },
                        "fills": [{ "type": "SOLID", "color": { "r": 0.2, "g": 0.6, "b": 1.0, "a": 1.0 } }],
                        "effects": [{ "type": "LAYER_BLUR", "visible": true, "radius": 12.5 }]
                    },
                    {
                        "id": "1:2",
                        "name": "Glass Card",
                        "type": "FRAME",
                        "absoluteBoundingBox": { "x": 110.0, "y": 10.0, "width": 160.0, "height": 90.0 },
                        "fills": [{ "type": "SOLID", "opacity": 0.35, "color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } }],
                        "effects": [{ "type": "BACKGROUND_BLUR", "visible": true, "radius": 18.0 }],
                        "children": [{
                            "id": "1:3",
                            "name": "Inner Pill",
                            "type": "RECTANGLE",
                            "absoluteBoundingBox": { "x": 126.0, "y": 26.0, "width": 60.0, "height": 24.0 },
                            "fills": [{ "type": "SOLID", "color": { "r": 0.9, "g": 0.3, "b": 0.4, "a": 1.0 } }]
                        }]
                    },
                    {
                        "id": "1:4",
                        "name": "Empty Backdrop Overlay",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 10.0, "y": 120.0, "width": 120.0, "height": 60.0 },
                        "fills": [],
                        "strokes": [],
                        "effects": [{ "type": "BACKGROUND_BLUR", "visible": true, "radius": 8.0 }]
                    },
                    {
                        "id": "1:5",
                        "name": "Inactive Blurs",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 150.0, "y": 120.0, "width": 80.0, "height": 40.0 },
                        "fills": [{ "type": "SOLID", "color": { "r": 0.4, "g": 0.8, "b": 0.4, "a": 1.0 } }],
                        "effects": [
                            { "type": "LAYER_BLUR", "visible": false, "radius": 20.0 },
                            { "type": "BACKGROUND_BLUR", "visible": true, "radius": 0.0 }
                        ]
                    }
                ]
            }]
        }
    })
    .to_string();

    for layer_mapping in [
        FigmaLayerMapping::TimelineLayers,
        FigmaLayerMapping::MovieClipSymbols,
    ] {
        let opts = FigmaImportOptions {
            layer_mapping,
            ..FigmaImportOptions::default()
        };
        let (project, report) = import_figma_json_str(&file, "Blur Test", &opts).unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| d.severity != crate::Severity::Error)
        );
        project.validate().expect("valid blur project");

        let root = project.symbol(&project.root).unwrap();
        let find_el = |name: &str| -> &Element {
            let layer = root
                .timeline
                .layers
                .iter()
                .find(|l| l.name == name)
                .unwrap_or_else(|| panic!("missing layer {name}"));
            &layer.keyframes[0].elements[0]
        };

        let badge = find_el("Soft Badge");
        assert!(badge.cache_as_bitmap);
        assert_eq!(
            badge.filters,
            vec![FilterSpec::Blur {
                blur_x: 12.5,
                blur_y: 12.5,
                quality: 2,
            }]
        );

        let glass = find_el("Glass Card");
        assert!(matches!(glass.kind, ElementKind::Instance { .. }));
        assert!(glass.cache_as_bitmap);
        assert_eq!(
            glass.filters,
            vec![FilterSpec::Blur {
                blur_x: 18.0,
                blur_y: 18.0,
                quality: 2,
            }]
        );

        let overlay = find_el("Empty Backdrop Overlay");
        assert!(overlay.cache_as_bitmap);
        assert_eq!(
            overlay.filters,
            vec![FilterSpec::Blur {
                blur_x: 8.0,
                blur_y: 8.0,
                quality: 2,
            }]
        );

        let inactive = find_el("Inactive Blurs");
        assert!(inactive.filters.is_empty());
        assert!(!inactive.cache_as_bitmap);

        let bg_warnings: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == "figma-background-blur-approximated")
            .collect();
        assert_eq!(
            bg_warnings.len(),
            2,
            "expected warnings only for the two active radius>0 BACKGROUND_BLUR nodes"
        );
        assert!(bg_warnings[0].message.contains("backdrop"));
    }
}

#[allow(clippy::too_many_lines)]
fn build_rich_figma_clipboard_kiwi_fixture(
    use_zstd: bool,
    origin_x: f32,
    origin_y: f32,
    embed_image_in_blob: bool,
) -> (Vec<u8>, Vec<u8>) {
    // Rich binary `fig-kiwi` schema covering:
    // 0: GUID, 1: ParentIndex, 2: Vector, 3: Matrix, 4: Color, 5: ColorStop,
    // 6: Image (hash + dataBlob), 7: Paint (SOLID / GRADIENT_LINEAR / IMAGE + transform),
    // 8: Path (windingRule + commandsBlob), 9: FontName, 10: Number (value + units),
    // 11: TextData (characters), 12: SymbolOverride, 13: SymbolData, 14: Effect,
    // 15: NodeChange, 16: Blob, 17: Message.
    let mut schema = Vec::new();
    write_var_uint(&mut schema, 18);

    // 0: GUID (STRUCT)
    write_cstr(&mut schema, "GUID");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["sessionID", "localID"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 1: ParentIndex (STRUCT)
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
    // 2: Vector (STRUCT)
    write_cstr(&mut schema, "Vector");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["x", "y"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 3: Matrix (STRUCT)
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
    // 4: Color (STRUCT)
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
    // 6: Image (MESSAGE: 1=hash byte[], 2=dataBlob uint)
    write_cstr(&mut schema, "Image");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "hash");
    write_var_int(&mut schema, -2);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "dataBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 7: Paint (MESSAGE: 1=type, 2=color, 3=stops ColorStop[], 4=transform Matrix, 5=image Image, 6=imageScaleMode, 7=opacity)
    write_cstr(&mut schema, "Paint");
    schema.push(2);
    write_var_uint(&mut schema, 7);
    for (tag, n, tid, arr) in [
        (1u64, "type", -6i64, 0u8),
        (2, "color", 4, 0),
        (3, "stops", 5, 1),
        (4, "transform", 3, 0),
        (5, "image", 6, 0),
        (6, "imageScaleMode", -6, 0),
        (7, "opacity", -5, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 8: Path (MESSAGE: 1=windingRule, 2=commandsBlob)
    write_cstr(&mut schema, "Path");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "windingRule");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "commandsBlob");
    write_var_int(&mut schema, -4);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 9: FontName (STRUCT: family, style)
    write_cstr(&mut schema, "FontName");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    for (i, n) in ["family", "style"].iter().enumerate() {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, -6);
        schema.push(0);
        write_var_uint(&mut schema, (i + 1) as u64);
    }
    // 10: Number (STRUCT: value float, units string)
    write_cstr(&mut schema, "Number");
    schema.push(1);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "value");
    write_var_int(&mut schema, -5);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "units");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 2);
    // 11: TextData (MESSAGE: 1=characters)
    write_cstr(&mut schema, "TextData");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "characters");
    write_var_int(&mut schema, -6);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    // 12: SymbolOverride (MESSAGE: 1=guidPath GUID[], 2=textData TextData, 3=fillPaints Paint[])
    write_cstr(&mut schema, "SymbolOverride");
    schema.push(2);
    write_var_uint(&mut schema, 3);
    for (tag, n, tid, arr) in [
        (1u64, "guidPath", 0i64, 1u8),
        (2, "textData", 11, 0),
        (3, "fillPaints", 7, 1),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 13: SymbolData (MESSAGE: 1=symbolID GUID, 2=symbolOverrides SymbolOverride[])
    write_cstr(&mut schema, "SymbolData");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "symbolID");
    write_var_int(&mut schema, 0);
    schema.push(0);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "symbolOverrides");
    write_var_int(&mut schema, 12);
    schema.push(1);
    write_var_uint(&mut schema, 2);
    // 14: Effect (MESSAGE: 1=type, 2=visible, 3=radius, 4=color, 5=offset Vector)
    write_cstr(&mut schema, "Effect");
    schema.push(2);
    write_var_uint(&mut schema, 5);
    for (tag, n, tid, arr) in [
        (1u64, "type", -6i64, 0u8),
        (2, "visible", -1, 0),
        (3, "radius", -5, 0),
        (4, "color", 4, 0),
        (5, "offset", 2, 0),
    ] {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 15: NodeChange (MESSAGE)
    write_cstr(&mut schema, "NodeChange");
    schema.push(2);
    let nc_fields = [
        (1u64, "guid", 0i64, 0u8),
        (2, "parentIndex", 1, 0),
        (3, "type", -6, 0),
        (4, "name", -6, 0),
        (5, "internalOnly", -1, 0),
        (6, "size", 2, 0),
        (7, "transform", 3, 0),
        (8, "fillPaints", 7, 1),
        (9, "strokePaints", 7, 1),
        (10, "strokeWeight", -5, 0),
        (11, "fillGeometry", 8, 1),
        (12, "cornerRadius", -5, 0),
        (13, "rectangleTopLeftCornerRadius", -5, 0),
        (14, "rectangleTopRightCornerRadius", -5, 0),
        (15, "rectangleBottomRightCornerRadius", -5, 0),
        (16, "rectangleBottomLeftCornerRadius", -5, 0),
        (17, "effects", 14, 1),
        (18, "textData", 11, 0),
        (19, "fontName", 9, 0),
        (20, "fontSize", -5, 0),
        (21, "textAlignHorizontal", -6, 0),
        (22, "letterSpacing", 10, 0),
        (23, "lineHeight", 10, 0),
        (24, "symbolData", 13, 0),
        (25, "clipsContent", -1, 0),
        (26, "opacity", -5, 0),
        (27, "blendMode", -6, 0),
    ];
    write_var_uint(&mut schema, nc_fields.len() as u64);
    for (tag, n, tid, arr) in nc_fields {
        write_cstr(&mut schema, n);
        write_var_int(&mut schema, tid);
        schema.push(arr);
        write_var_uint(&mut schema, tag);
    }
    // 16: Blob (MESSAGE: 1=bytes byte[])
    write_cstr(&mut schema, "Blob");
    schema.push(2);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "bytes");
    write_var_int(&mut schema, -2);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    // 17: Message (MESSAGE: 1=nodeChanges NodeChange[], 2=blobs Blob[])
    write_cstr(&mut schema, "Message");
    schema.push(2);
    write_var_uint(&mut schema, 2);
    write_cstr(&mut schema, "nodeChanges");
    write_var_int(&mut schema, 15);
    schema.push(1);
    write_var_uint(&mut schema, 1);
    write_cstr(&mut schema, "blobs");
    write_var_int(&mut schema, 16);
    schema.push(1);
    write_var_uint(&mut schema, 2);

    // Blob 0: Bezier commandsBlob (0..120 x 0..80): M(0,0) -> L(120,0) -> C(120,50, 80,80, 40,80) -> L(0,80) -> Z
    let mut cmd_blob = Vec::new();
    cmd_blob.push(1);
    for f in [0.0f32, 0.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(2);
    for f in [120.0f32, 0.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(4);
    for f in [120.0f32, 50.0, 80.0, 80.0, 40.0, 80.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(2);
    for f in [0.0f32, 80.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(0);

    let png = tiny_png_bytes();
    let write_guid = |m: &mut Vec<u8>, s: u64, l: u64| {
        write_var_uint(m, s);
        write_var_uint(m, l);
    };
    let write_parent = |m: &mut Vec<u8>, ps: u64, pl: u64, pos: &str| {
        write_var_uint(m, 2);
        write_guid(m, ps, pl);
        write_cstr(m, pos);
    };
    let write_matrix = |m: &mut Vec<u8>, tx: f32, ty: f32| {
        write_var_uint(m, 7);
        for v in [1.0f32, 0.0, tx, 0.0, 1.0, ty] {
            write_var_float(m, v);
        }
    };
    let write_solid = |m: &mut Vec<u8>, tag: u64, rgba: [f32; 4]| {
        write_var_uint(m, tag);
        write_var_uint(m, 1);
        write_var_uint(m, 1);
        write_cstr(m, "SOLID");
        write_var_uint(m, 2);
        for c in rgba {
            write_var_float(m, c);
        }
        write_var_uint(m, 0);
    };

    // Nodes:
    // 0: DOCUMENT (0:0)
    // 1: Internal Only Canvas (0:99)
    // 2: Master COMPONENT (9:1, "ChipMaster", 140x40) inside 0:99
    // 3: Master child VECTOR (9:2, "ChipIcon", 20x20) inside 9:1
    // 4: Master child TEXT (9:3, "ChipLabel", "Tag") inside 9:1
    // 5: User Page (0:1, "Page 1")
    // 6: Copied VECTOR (1:1, at (origin_x, origin_y), 120x80) with Bezier commandsBlob + GRADIENT_LINEAR + 3px stroke
    // 7: Copied ROUNDED_RECTANGLE (1:2, at (origin_x + 140, origin_y), 160x90) with corners [6,12,18,24] + LAYER_BLUR(10) + IMAGE crop
    // 8: Copied TEXT (1:3, at (origin_x, origin_y + 110), 220x44) "Vector Paste" Inter Bold Italic 28px tracking 1.5 leading 34
    // 9: Copied INSTANCE (1:4, at (origin_x + 160, origin_y + 110), 140x40) referencing 9:1 with override "Live Pro"
    let mut msg = Vec::new();
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 10);

    // 0: DOCUMENT
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 0);
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "DOCUMENT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Clipboard Doc");
    write_var_uint(&mut msg, 0);

    // 1: Internal Only Canvas
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 99);
    write_parent(&mut msg, 0, 0, "!");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Internal Only Canvas");
    write_var_uint(&mut msg, 5);
    msg.push(1);
    write_var_uint(&mut msg, 0);

    // 2: Master COMPONENT 9:1
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 9, 1);
    write_parent(&mut msg, 0, 99, "a");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "COMPONENT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "ChipMaster");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 140.0);
    write_var_float(&mut msg, 40.0);
    write_matrix(&mut msg, 0.0, 0.0);
    write_solid(&mut msg, 8, [0.12, 0.16, 0.28, 1.0]);
    write_var_uint(&mut msg, 25);
    msg.push(1); // clipsContent = true
    write_var_uint(&mut msg, 0);

    // 3: Master child VECTOR 9:2
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 9, 2);
    write_parent(&mut msg, 9, 1, "a");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "VECTOR");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "ChipIcon");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 120.0);
    write_var_float(&mut msg, 80.0);
    write_matrix(&mut msg, 8.0, 6.0);
    write_solid(&mut msg, 8, [0.3, 0.75, 1.0, 1.0]);
    write_var_uint(&mut msg, 11);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "NONZERO");
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0); // commandsBlob = 0
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);

    // 4: Master child TEXT 9:3
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 9, 3);
    write_parent(&mut msg, 9, 1, "b");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "TEXT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "ChipLabel");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 100.0);
    write_var_float(&mut msg, 24.0);
    write_matrix(&mut msg, 32.0, 8.0);
    write_solid(&mut msg, 8, [1.0, 1.0, 1.0, 1.0]);
    write_var_uint(&mut msg, 18);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Tag");
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 19);
    write_cstr(&mut msg, "Inter");
    write_cstr(&mut msg, "SemiBold");
    write_var_uint(&mut msg, 20);
    write_var_float(&mut msg, 14.0);
    write_var_uint(&mut msg, 0);

    // 5: User Page 0:1
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 0, 1);
    write_parent(&mut msg, 0, 0, "a");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "CANVAS");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Page 1");
    write_var_uint(&mut msg, 0);

    // 6: Copied VECTOR 1:1 with Bezier commandsBlob + GRADIENT_LINEAR + 3px stroke
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 1, 1);
    write_parent(&mut msg, 0, 1, "a");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "VECTOR");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Hero Curve");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 120.0);
    write_var_float(&mut msg, 80.0);
    write_matrix(&mut msg, origin_x, origin_y);
    // fillPaints: GRADIENT_LINEAR (2 stops)
    write_var_uint(&mut msg, 8);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "GRADIENT_LINEAR");
    write_var_uint(&mut msg, 3);
    write_var_uint(&mut msg, 2);
    for (rgba, pos) in [
        ([1.0f32, 0.25, 0.4, 1.0], 0.0f32),
        ([0.2, 0.6, 1.0, 1.0], 1.0),
    ] {
        for c in rgba {
            write_var_float(&mut msg, c);
        }
        write_var_float(&mut msg, pos);
    }
    write_var_uint(&mut msg, 0); // end fill Paint
    // strokePaints: SOLID + strokeWeight = 3.5
    write_solid(&mut msg, 9, [0.95, 0.95, 1.0, 1.0]);
    write_var_uint(&mut msg, 10);
    write_var_float(&mut msg, 3.5);
    write_var_uint(&mut msg, 11);
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "NONZERO");
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 0); // commandsBlob = 0
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0);

    // 7: Copied ROUNDED_RECTANGLE 1:2 (160x90) at (origin_x + 140, origin_y) with corners [6,12,18,24], Blur(10), DropShadow, and IMAGE crop
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 1, 2);
    write_parent(&mut msg, 0, 1, "b");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "ROUNDED_RECTANGLE");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Blurred Card");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 160.0);
    write_var_float(&mut msg, 90.0);
    write_matrix(&mut msg, origin_x + 140.0, origin_y);
    // fillPaints: SOLID + cropped IMAGE
    write_var_uint(&mut msg, 8);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "SOLID");
    write_var_uint(&mut msg, 2);
    for c in [0.18f32, 0.22, 0.34, 1.0] {
        write_var_float(&mut msg, c);
    }
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "IMAGE");
    write_var_uint(&mut msg, 4); // UV crop transform
    for v in [0.5f32, 0.0, 0.25, 0.0, 0.5, 0.25] {
        write_var_float(&mut msg, v);
    }
    write_var_uint(&mut msg, 5); // Image
    write_var_uint(&mut msg, 1); // hash
    let fake_hash = [0x42u8; 20];
    write_var_uint(&mut msg, fake_hash.len() as u64);
    msg.extend_from_slice(&fake_hash);
    if embed_image_in_blob {
        write_var_uint(&mut msg, 2); // dataBlob = 1
        write_var_uint(&mut msg, 1);
    }
    write_var_uint(&mut msg, 0); // end Image
    write_var_uint(&mut msg, 6);
    write_cstr(&mut msg, "CROP");
    write_var_uint(&mut msg, 0); // end IMAGE Paint
    write_var_uint(&mut msg, 13);
    write_var_float(&mut msg, 6.0);
    write_var_uint(&mut msg, 14);
    write_var_float(&mut msg, 12.0);
    write_var_uint(&mut msg, 15);
    write_var_float(&mut msg, 18.0);
    write_var_uint(&mut msg, 16);
    write_var_float(&mut msg, 24.0);
    // effects: LAYER_BLUR(10.0) + DROP_SHADOW(radius=8, offset=(0,4))
    write_var_uint(&mut msg, 17);
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "LAYER_BLUR");
    write_var_uint(&mut msg, 2);
    msg.push(1);
    write_var_uint(&mut msg, 3);
    write_var_float(&mut msg, 10.0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "DROP_SHADOW");
    write_var_uint(&mut msg, 2);
    msg.push(1);
    write_var_uint(&mut msg, 3);
    write_var_float(&mut msg, 8.0);
    write_var_uint(&mut msg, 4);
    for c in [0.0f32, 0.0, 0.0, 0.5] {
        write_var_float(&mut msg, c);
    }
    write_var_uint(&mut msg, 5);
    write_var_float(&mut msg, 0.0);
    write_var_float(&mut msg, 4.0);
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 26);
    write_var_float(&mut msg, 0.85);
    write_var_uint(&mut msg, 27);
    write_cstr(&mut msg, "SCREEN");
    write_var_uint(&mut msg, 0);

    // 8: Copied TEXT 1:3 (220x44) at (origin_x, origin_y + 110)
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 1, 3);
    write_parent(&mut msg, 0, 1, "c");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "TEXT");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Title Copy");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 220.0);
    write_var_float(&mut msg, 44.0);
    write_matrix(&mut msg, origin_x, origin_y + 110.0);
    write_solid(&mut msg, 8, [0.98, 0.96, 0.9, 1.0]);
    write_var_uint(&mut msg, 18);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Vector Paste");
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 19);
    write_cstr(&mut msg, "Inter");
    write_cstr(&mut msg, "Bold Italic");
    write_var_uint(&mut msg, 20);
    write_var_float(&mut msg, 28.0);
    write_var_uint(&mut msg, 21);
    write_cstr(&mut msg, "CENTER");
    write_var_uint(&mut msg, 22);
    write_var_float(&mut msg, 1.5);
    write_cstr(&mut msg, "PIXELS");
    write_var_uint(&mut msg, 23);
    write_var_float(&mut msg, 36.0);
    write_cstr(&mut msg, "PIXELS");
    write_var_uint(&mut msg, 0);

    // 9: Copied INSTANCE 1:4 (140x40) at (origin_x + 160, origin_y + 110) referencing 9:1 with override on 9:3 -> "Live Pro"
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 1, 4);
    write_parent(&mut msg, 0, 1, "d");
    write_var_uint(&mut msg, 3);
    write_cstr(&mut msg, "INSTANCE");
    write_var_uint(&mut msg, 4);
    write_cstr(&mut msg, "Chip Instance");
    write_var_uint(&mut msg, 6);
    write_var_float(&mut msg, 140.0);
    write_var_float(&mut msg, 40.0);
    write_matrix(&mut msg, origin_x + 160.0, origin_y + 110.0);
    write_var_uint(&mut msg, 24); // symbolData
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 9, 1);
    write_var_uint(&mut msg, 2); // symbolOverrides len = 1
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1); // guidPath = [9:3]
    write_var_uint(&mut msg, 1);
    write_guid(&mut msg, 9, 3);
    write_var_uint(&mut msg, 2); // textData.characters = "Live Pro"
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "Live Pro");
    write_var_uint(&mut msg, 0);
    write_var_uint(&mut msg, 0); // end override
    write_var_uint(&mut msg, 0); // end symbolData
    write_var_uint(&mut msg, 0); // end INSTANCE

    // blobs: [cmd_blob, optional png]
    write_var_uint(&mut msg, 2);
    if embed_image_in_blob {
        write_var_uint(&mut msg, 2);
        for b in [&cmd_blob, &png] {
            write_var_uint(&mut msg, 1);
            write_var_uint(&mut msg, b.len() as u64);
            msg.extend_from_slice(b);
            write_var_uint(&mut msg, 0);
        }
    } else {
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, cmd_blob.len() as u64);
        msg.extend_from_slice(&cmd_blob);
        write_var_uint(&mut msg, 0);
    }
    write_var_uint(&mut msg, 0); // end Message

    let deflate = |raw: &[u8]| -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    };
    let c0 = deflate(&schema);
    let c1 = if use_zstd {
        encode_zstd_for_test(&msg)
    } else {
        deflate(&msg)
    };
    let mut kiwi = Vec::new();
    kiwi.extend_from_slice(b"fig-kiwi");
    kiwi.extend_from_slice(&106u32.to_le_bytes());
    kiwi.extend_from_slice(&(c0.len() as u32).to_le_bytes());
    kiwi.extend_from_slice(&c0);
    kiwi.extend_from_slice(&(c1.len() as u32).to_le_bytes());
    kiwi.extend_from_slice(&c1);
    (kiwi, png)
}

#[test]
fn t413_figma_clipboard_html_raw_and_escaped_decodes_zstd_and_deflate_at_exact_1to1_scale() {
    for (use_zstd, escaped_html, origin_x, origin_y, embed_blob) in [
        (true, false, 120.0f32, 80.0f32, true),
        (false, true, 18_400.0f32, -9_200.0f32, false),
        (true, true, 240_000.0f32, -150_000.0f32, true),
    ] {
        let (kiwi_bytes, png_bytes) =
            build_rich_figma_clipboard_kiwi_fixture(use_zstd, origin_x, origin_y, embed_blob);
        let meta_json = br#"{"fileKey":"AbCdEf123","pasteID":987654,"dataType":"scene"}"#;
        let meta_b64 = BASE64.encode(meta_json);
        let raw_kiwi_b64 = BASE64.encode(&kiwi_bytes);
        let png_b64 = BASE64.encode(&png_bytes);

        let html = if escaped_html {
            // Browser-escaped `&lt;!--(figma)...(/figma)--&gt;` with wrapped lines + HTML entity `&#61;`
            let wrapped: String = raw_kiwi_b64
                .as_bytes()
                .chunks(64)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect::<Vec<_>>()
                .join("\n  ")
                .replace('=', "&#61;");
            format!(
                "<meta charset=\"utf-8\"><span data-metadata=\"&lt;!--(figmeta){meta_b64}(/figmeta)--&gt;\"></span><span data-buffer=\"&lt;!--(figma)\n  {wrapped}\n(/figma)--&gt;\"></span><img src=\"data:image/png;base64,{png_b64}\">"
            )
        } else {
            format!(
                "<meta charset='utf-8'><span data-metadata=\"<!--(figmeta){meta_b64}(/figmeta)-->\"></span><span data-buffer=\"<!--(figma){raw_kiwi_b64}(/figma)-->\"></span>"
            )
        };

        assert!(contains_figma_clipboard_marker(&html));
        let extracted = extract_figma_clipboard_kiwi(&html).expect("extract figma clipboard");
        assert_eq!(extracted.kiwi_bytes, kiwi_bytes);
        let meta = extracted.meta.as_ref().expect("figmeta parsed");
        assert_eq!(meta.file_key.as_deref(), Some("AbCdEf123"));
        assert_eq!(meta.paste_id.as_deref(), Some("987654"));
        assert_eq!(meta.data_type.as_deref(), Some("scene"));
        if escaped_html {
            assert_eq!(extracted.inline_images.len(), 1);
        }

        let output = import_figma_clipboard(&html, None).expect("import figma clipboard 1:1");
        output
            .project
            .validate()
            .expect("clipboard project validates");
        assert!(
            !output
                .report
                .diagnostics
                .iter()
                .any(|d| d.code == "figma-page-fitted-to-stage"),
            "clipboard import must NEVER auto-downscale"
        );

        let fb = output.figma_bounds.expect("figma_bounds measured");
        assert!((fb.x_min - f64::from(origin_x)).abs() < 1e-3);
        assert!((fb.y_min - f64::from(origin_y)).abs() < 1e-3);
        assert!((fb.width() - 300.0).abs() < 1e-3);
        assert!((fb.height() - 154.0).abs() < 1e-3);

        let root = output.project.symbol(&output.project.root).unwrap();
        assert_eq!(root.timeline.layers.len(), 1);
        let kf = &root.timeline.layers[0].keyframes[0];

        // 1. Editable Bezier VECTOR with linear gradient + 3.5px stroke at exact 1:1 scale
        let curve = kf
            .elements
            .iter()
            .find(|e| e.name == "Hero Curve")
            .expect("Hero Curve vector");
        assert!((curve.transform.scale_x - 1.0).abs() < 1e-6);
        assert!((curve.transform.scale_y - 1.0).abs() < 1e-6);
        match &curve.kind {
            ElementKind::PaintedPath {
                commands,
                fill_gradient,
                stroke_width,
                ..
            } => {
                assert!((*stroke_width - 3.5).abs() < 1e-6);
                assert!(fill_gradient.is_some());
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, PathCommand::CubicTo { .. }))
                );
            }
            other => panic!("expected PaintedPath for gradient vector, got {other:?}"),
        }

        // 2. Rounded rectangle with asymmetric corners [6, 12, 24, 18] (NF [tl, tr, bl, br]), Blur + DropShadow, SCREEN blend, 0.85 alpha, and UV-cropped bitmap fill
        let card_els: Vec<&Element> = kf
            .elements
            .iter()
            .filter(|e| e.name == "Blurred Card")
            .collect();
        assert_eq!(card_els.len(), 2, "solid backdrop + cropped image fill");
        assert!(matches!(
            card_els[0].transform.primitive,
            Some(PrimitiveOptions::Rectangle(RectangleOptions {
                corner_radii: [6.0, 12.0, 24.0, 18.0],
                corners_locked: false,
            }))
        ));
        assert_eq!(card_els[1].blend_mode, BlendMode::Screen);
        assert!((card_els[1].transform.alpha - 0.85).abs() < 1e-5);
        assert_eq!(card_els[1].filters.len(), 2);
        assert!(card_els[1].filters.iter().any(
            |f| matches!(f, FilterSpec::Blur { blur_x, .. } if (*blur_x - 10.0).abs() < 1e-6)
        ));
        assert!(
            card_els[1]
                .filters
                .iter()
                .any(|f| matches!(f, FilterSpec::DropShadow { .. }))
        );
        assert_eq!(kf.bitmap_fills.len(), 1);
        assert_eq!(output.project.resources.len(), 1);

        // 3. Editable TEXT with font family, bold, italic, size 28, tracking 1.5, leading, CENTER align
        let title = kf
            .elements
            .iter()
            .find(|e| e.name == "Title Copy")
            .expect("Title Copy text");
        match &title.kind {
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => {
                assert_eq!(text, "Vector Paste");
                assert!((*width - 220.0).abs() < 1e-6);
                assert!((*height - 44.0).abs() < 1e-6);
                assert_eq!(format.font, "Inter");
                assert!((format.size - 28.0).abs() < 1e-6);
                assert!(format.bold && format.italic);
                assert_eq!(format.align, TextAlign::Center);
                assert!((format.letter_spacing - 1.5).abs() < 1e-6);
                assert!((1.2 * format.size + format.leading - 36.0).abs() < 1e-5);
            }
            other => panic!("expected editable TextBox, got {other:?}"),
        }

        // 4. Nested INSTANCE resolved from `Internal Only Canvas` COMPONENT + override "Live Pro" + Frame Clip mask
        let inst = kf
            .elements
            .iter()
            .find(|e| e.name == "Chip_Instance")
            .expect("Chip_Instance");
        let ElementKind::Instance { symbol_id, .. } = &inst.kind else {
            panic!("expected Instance");
        };
        let variant_sym = output.project.symbol(symbol_id).expect("variant symbol");
        assert!(
            variant_sym
                .timeline
                .layers
                .iter()
                .any(|l| l.properties.role == LayerRole::Mask && l.name == "Frame Clip"),
            "clipsContent mask preserved on component symbol"
        );
        let has_live_pro = variant_sym
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .any(|el| matches!(&el.kind, ElementKind::TextBox { text, .. } if text == "Live Pro"));
        assert!(
            has_live_pro,
            "Internal Only Canvas override text materialized"
        );
    }

    assert!(!contains_figma_clipboard_marker(
        "<p>Hello (figma) world (/figma)</p>"
    ));
    assert!(extract_figma_clipboard_kiwi("<!--(figma)bm90LWZpZ21h(/figma)-->").is_none());
}
