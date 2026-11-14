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
    write_var_uint(&mut schema, 16);
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
    let write_solid = |m: &mut Vec<u8>, rgba: [f32; 4]| {
        write_var_uint(m, 9);
        write_var_uint(m, 1); // 1 paint
        write_var_uint(m, 1);
        write_cstr(m, "SOLID");
        write_var_uint(m, 2);
        for c in rgba {
            write_var_float(m, c);
        }
        write_var_uint(m, 0);
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

    // nodeChanges (13 nodes):
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
    // 12: User Page 2 CANVAS (0:2, "Settings Screen") + child RECTANGLE (3:1, parent 0:2) -> 14 nodes!
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 14);

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
    write_var_uint(&mut msg, 10); // fillGeometry len=1
    write_var_uint(&mut msg, 1);
    write_var_uint(&mut msg, 1);
    write_cstr(&mut msg, "NONZERO");
    write_var_uint(&mut msg, 2);
    write_var_uint(&mut msg, 1); // commandsBlob = 1
    write_var_uint(&mut msg, 0);
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
                        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                            text == "Submit Now"
                        }
                        _ => false,
                    })
                })
            })
    });
    assert!(
        has_override_text,
        "symbolOverrides text variant must be materialized"
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

    // 1. Byte-for-byte Zstd cross-check on `interface/figma/Interface test.fig` chunk 1 when present
    let interface_fig = common_root.join("interface/figma/Interface test.fig");
    if interface_fig.is_file() {
        let mut za = ZipArchive::new(File::open(&interface_fig).unwrap()).unwrap();
        let mut canvas_bytes = Vec::new();
        za.by_name("canvas.fig")
            .unwrap()
            .read_to_end(&mut canvas_bytes)
            .unwrap();
        let c0_len = u32::from_le_bytes(canvas_bytes[12..16].try_into().unwrap()) as usize;
        let c1_off = 16 + c0_len;
        let c1_len =
            u32::from_le_bytes(canvas_bytes[c1_off..c1_off + 4].try_into().unwrap()) as usize;
        let c1_compressed = &canvas_bytes[c1_off + 4..c1_off + 4 + c1_len];
        assert!(super::zstd::is_zstd_magic(c1_compressed));
        let rust_decompressed = super::zstd::decompress_zstd(c1_compressed, MAX_FIGMA_DECODE_BYTES)
            .expect("pure-Rust Zstd decode on Interface test.fig chunk 1");
        assert!(rust_decompressed.len() > 1_000_000);

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
                    "pure-Rust Zstd output must match system zstd byte-for-byte"
                );
            }
        }
    }

    // 2. Import and validate EVERY `.fig` / `.figma.json` across `interface/figma` + `target/figma-corpus`
    //    without printing any private filenames or document text!
    let mut files = Vec::new();
    for dir in [
        common_root.join("interface/figma"),
        common_root.join("target/figma-corpus"),
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

    let mut imported_count = 0usize;
    let mut total_symbols = 0usize;
    let mut total_layers = 0usize;
    let mut total_elements = 0usize;
    for path in &files {
        let (project, report) = import_figma_path(path, &FigmaImportOptions::default())
            .unwrap_or_else(|e| {
                panic!("corpus file #{imported_count} failed import_figma_path: {e}")
            });
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
        "Verified {imported_count} real Figma document(s): {total_symbols} symbols, {total_layers} layers, {total_elements} elements"
    );
}
