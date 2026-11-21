use super::*;

#[test]
fn illustrator_svg_uses_stylesheet_cascade_gradient_space_and_source_paint_order() {
    let svg = r##"<svg viewBox="0 0 40 20"><defs>
        <style>.tile {fill: #123456} .wash { fill: url(#sun) }</style>
        <linearGradient id="sun" gradientUnits="userSpaceOnUse" x1="2" x2="10" y1="4" y2="4">
          <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/>
        </linearGradient></defs>
        <rect width="40" height="20" class="tile" fill="red"/>
        <g id="Foreground" transform="translate(5 3)"><rect class="wash" width="8" height="8"/></g>
        <rect width="1" height="1" class="tile" style="fill:#abcdef"/></svg>"##;
    let (project, _) =
        import_svg_with_options(svg, "art", &VectorImportOptions::default()).unwrap();
    project.validate().unwrap();
    let layers = &project.symbol(&project.root).unwrap().timeline.layers;
    assert_eq!(layers.len(), 3);
    assert!(matches!(
        layers[0].keyframes[0].elements[0].kind,
        ElementKind::Path {
            fill: [18, 52, 86, 255],
            ..
        }
    ));
    let ElementKind::PaintedPath {
        fill_gradient: Some(g),
        ..
    } = &layers[1].keyframes[0].elements[0].kind
    else {
        panic!("missing gradient")
    };
    assert_eq!(g.sample(7., 7.), [255, 0, 0, 255]);
    assert_eq!(g.sample(15., 7.), [0, 0, 255, 255]);
    assert!(matches!(
        layers[2].keyframes[0].elements[0].kind,
        ElementKind::Path {
            fill: [171, 205, 239, 255],
            ..
        }
    ));
}

#[test]
fn pdf_reads_referenced_page_streams_in_order_and_restores_clip_after_q() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.6");
    let tree = doc.new_object_id();
    let first = doc.add_object(Stream::new(
        dictionary! {},
        b"1 0 0 rg 7 9 20 15 re f q 7 9 4 5 re W n 0 1 0 rg 7 9 10 10 re f".to_vec(),
    ));
    let second = doc.add_object(Stream::new(
        dictionary! {},
        b"8 0 0 8 7 9 cm /Sh0 sh Q 0 0 1 rg 7 9 1 1 re f".to_vec(),
    ));
    let sh = doc.add_object(dictionary!{
        "ShadingType"=>2,"ColorSpace"=>"DeviceRGB","Coords"=>vec![0.into(),0.into(),1.into(),0.into()],
        "Function"=>dictionary!{"FunctionType"=>2,"N"=>1,"C0"=>vec![0.into(),1.into(),0.into()],"C1"=>vec![1.into(),0.into(),0.into()]}
    });
    let page = doc.add_object(dictionary!{"Type"=>"Page","Parent"=>tree,"Contents"=>vec![Object::Reference(first),Object::Reference(second)],"Resources"=>dictionary!{"Shading"=>dictionary!{"Sh0"=>sh}}});
    doc.objects.insert(tree,dictionary!{"Type"=>"Pages","Count"=>1,"Kids"=>vec![Object::Reference(page)],"MediaBox"=>vec![7.into(),9.into(),27.into(),24.into()]}.into());
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>tree});
    doc.trailer.set("Root", root);
    doc.add_object(Stream::new(dictionary! {}, b") 0 0 100 100 re f".to_vec()));
    doc.compress();
    let mut bytes = vec![];
    doc.save_to(&mut bytes).unwrap();
    let (project, report) =
        import_pdf_ai_with_options(&bytes, "pages", &VectorImportOptions::default()).unwrap();
    project.validate().unwrap();
    assert_eq!((project.width, project.height), (20, 15));
    assert_eq!(report.statistics.elements, 4);
    let layers = &project.symbol(&project.root).unwrap().timeline.layers;
    assert_eq!(layers.len(), 4);
    assert!(layers[0].properties.mask.is_none() && layers[3].properties.mask.is_none());
    assert_eq!(layers[2].properties.mask.as_ref(), Some(&layers[1].id));
    // `/Sh0 sh` bakes `gstate.clip` directly as its `PaintedPath` geometry without a duplicate Mask layer,
    // followed by the post-`Q` unclipped path on `layers[3]`.
    let ElementKind::PaintedPath {
        commands: sh_cmds,
        fill_gradient: Some(paint),
        ..
    } = &layers[3].keyframes[0].elements[0].kind
    else {
        panic!("missing axial shading")
    };
    let sh_bounds = neoflash_core::path_bounds(sh_cmds).unwrap().unwrap();
    assert!((sh_bounds.x_max - sh_bounds.x_min - 4.0).abs() < 1e-6);
    assert!((sh_bounds.y_max - sh_bounds.y_min - 5.0).abs() < 1e-6);
    assert_eq!(paint.sample(0., 15.), [0, 255, 0, 255]);
    assert_eq!(paint.sample(8., 15.), [255, 0, 0, 255]);
}

#[test]
fn eps_imports_checked_preview_and_its_intrinsic_size() {
    use image::ImageEncoder;
    use std::io::Cursor;
    let mut tiff = Cursor::new(Vec::new());
    image::codecs::tiff::TiffEncoder::new(&mut tiff)
        .write_image(
            &[12, 34, 56, 78, 90, 12],
            2,
            1,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let ps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 6 3\n";
    let mut file = vec![0; 30];
    file[..4].copy_from_slice(eps::MAGIC);
    for (offset, value) in [
        (4, 30),
        (8, ps.len()),
        (20, 30 + ps.len()),
        (24, tiff.get_ref().len()),
    ] {
        file[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    file.extend_from_slice(ps);
    file.extend_from_slice(tiff.get_ref());
    let (project, report) =
        import_vector_bytes_with_options(&file, "preview", &VectorImportOptions::default())
            .unwrap();
    project.validate().unwrap();
    assert_eq!((project.width, project.height), (6, 3));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "eps-tiff-preview")
    );
    assert_eq!(
        project.resources[0].decode_image().unwrap().rgba.as_ref(),
        &[12, 34, 56, 255, 78, 90, 12, 255]
    );
    file[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        import_vector_bytes_with_options(&file, "invalid", &VectorImportOptions::default())
            .is_err()
    );
}

#[test]
fn pdf_imports_type3_radial_exponential_fn2_stitching_fn3_and_shading_patterns() {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.6");
    let tree = doc.new_object_id();

    // 1) Type 3 radial shading with focal offset + inner radius r0 > 0 + FunctionType 3 stitching (3 stops)
    let f_a = dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "N" => 1,
        "C0" => vec![1.into(), 0.into(), 0.into()],
        "C1" => vec![0.into(), 1.into(), 0.into()],
    };
    let f_b = dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "N" => 1,
        "C0" => vec![0.into(), 1.into(), 0.into()],
        "C1" => vec![0.into(), 0.into(), 1.into()],
    };
    let stitched = doc.add_object(dictionary! {
        "FunctionType" => 3,
        "Domain" => vec![0.into(), 1.into()],
        "Functions" => vec![f_a.into(), f_b.into()],
        "Bounds" => vec![Object::Real(0.5)],
        "Encode" => vec![0.into(), 1.into(), 0.into(), 1.into()],
    });
    let sh_radial = doc.add_object(dictionary! {
        "ShadingType" => 3,
        "ColorSpace" => "DeviceRGB",
        "Coords" => vec![60.into(), 50.into(), 10.into(), 50.into(), 50.into(), 40.into()],
        "Function" => Object::Reference(stitched),
    });

    // 2) Type 2 axial shading with exponential FunctionType 2 (N = 2.0) in DeviceCMYK via PatternType 2 (`scn`)
    let fn_exp = doc.add_object(dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "N" => Object::Real(2.0),
        "C0" => vec![0.into(), 0.into(), 0.into(), 0.into()], // white
        "C1" => vec![1.into(), 0.into(), 0.into(), 0.into()], // cyan (R=0, G=255, B=255)
    });
    let sh_exp = doc.add_object(dictionary! {
        "ShadingType" => 2,
        "ColorSpace" => "DeviceCMYK",
        "Coords" => vec![0.into(), 0.into(), 100.into(), 0.into()],
        "Function" => Object::Reference(fn_exp),
    });
    let pat0 = doc.add_object(dictionary! {
        "Type" => "Pattern",
        "PatternType" => 2,
        "Shading" => Object::Reference(sh_exp),
        "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 10.into(), 10.into()],
    });

    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"q 10 10 80 80 re W n /ShRad sh Q /Pattern cs /Pat0 scn 10 10 100 20 re f".to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => tree,
        "Contents" => Object::Reference(stream),
        "Resources" => dictionary! {
            "Shading" => dictionary! { "ShRad" => Object::Reference(sh_radial) },
            "Pattern" => dictionary! { "Pat0" => Object::Reference(pat0) },
        },
    });
    doc.objects.insert(
        tree,
        dictionary! {
            "Type" => "Pages",
            "Count" => 1,
            "Kids" => vec![Object::Reference(page)],
            "MediaBox" => vec![0.into(), 0.into(), 120.into(), 100.into()],
        }
        .into(),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    let (project, report) =
        import_pdf_ai_with_options(&bytes, "shadings", &VectorImportOptions::default()).unwrap();
    project.validate().unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "pdf-unsupported-operator")
    );

    let all_elements: Vec<&Element> = project
        .symbol(&project.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .collect();

    let radial_elem = all_elements
        .iter()
        .find(|e| e.name == "PDF radial shading")
        .expect("radial shading element");
    let ElementKind::PaintedPath {
        fill_gradient: Some(rad_grad),
        ..
    } = &radial_elem.kind
    else {
        panic!("expected radial PaintedPath");
    };
    assert_eq!(rad_grad.kind, GradientKind::Radial);
    assert!(
        (rad_grad.focal_point_ratio - (1.0 / 3.0)).abs() < 1e-4,
        "expected focal ratio dist/(r1-r0) = 10/30 ~= 0.3333, got {}",
        rad_grad.focal_point_ratio
    );
    // Inner radius r0/r1 = 10/40 = 0.25 -> first stop holds red, stitched midpoint green, outer blue
    assert_eq!(rad_grad.stops.first().unwrap().color, [255, 0, 0, 255]);
    assert_eq!(rad_grad.stops.last().unwrap().color, [0, 0, 255, 255]);
    assert!(rad_grad.stops.iter().any(|s| s.color == [0, 255, 0, 255]));

    let pat_elem = all_elements
        .iter()
        .find(|e| e.name.starts_with("PDF Path"))
        .expect("pattern-filled path element");
    let ElementKind::PaintedPath {
        fill_gradient: Some(exp_grad),
        ..
    } = &pat_elem.kind
    else {
        panic!("expected pattern-filled PaintedPath");
    };
    assert_eq!(exp_grad.kind, GradientKind::Linear);
    assert!(
        exp_grad.stops.len() > 2,
        "N=2.0 curve should sample multi-stop ramp"
    );
    // At midpoint x = 60 (10 + 50), t = 0.5, t^2 = 0.25 -> C = 0.25 -> R = round(0.75 * 255) = 191, G = 255, B = 255
    let mid_sample = exp_grad.sample(60.0, 80.0);
    assert!(
        (i16::from(mid_sample[0]) - 191).abs() <= 4 && mid_sample[1] == 255 && mid_sample[2] == 255,
        "expected exponential midpoint ~[191, 255, 255, 255], got {mid_sample:?}"
    );
}

#[test]
fn t414_svg_path_and_nesting_bounds_and_degenerate_gradient_fallback() {
    let mut huge_d = String::from("M 0 0");
    for i in 1..=40_000 {
        huge_d.push_str(&format!(" L {i} {}", i % 17));
    }
    let huge_svg = format!(
        r##"<svg viewBox="0 0 100 100"><path d="{huge_d}" stroke="#000" fill="none"/></svg>"##
    );
    let (huge_proj, _) =
        import_svg_with_options(&huge_svg, "huge", &VectorImportOptions::default()).unwrap();
    huge_proj.validate().unwrap();
    let huge_root = huge_proj.symbol(&huge_proj.root).unwrap();
    let ElementKind::Path { commands, .. } =
        &huge_root.timeline.layers[0].keyframes[0].elements[0].kind
    else {
        panic!("expected Path");
    };
    assert_eq!(commands.len(), 32_768);

    let mut shared_huge_d = String::from("M 0 0");
    for i in 1..=120_000 {
        shared_huge_d.push_str(&format!(" L {i} {}", i % 17));
    }
    assert_eq!(
        parse_svg_path_data(&shared_huge_d).len(),
        neoflash_core::MAX_PATH_COMMANDS + 1
    );

    let mut deep_svg = String::from(r#"<svg viewBox="0 0 100 100">"#);
    for _ in 0..140 {
        deep_svg.push_str("<g>");
    }
    deep_svg.push_str(r#"<rect width="10" height="10" fill="red"/>"#);
    for _ in 0..140 {
        deep_svg.push_str("</g>");
    }
    deep_svg.push_str("</svg>");
    let (deep_proj, deep_report) =
        import_svg_with_options(&deep_svg, "deep", &VectorImportOptions::default()).unwrap();
    deep_proj.validate().unwrap();
    assert!(
        deep_report
            .diagnostics
            .iter()
            .any(|d| d.code == "svg-nesting-limit-exceeded")
    );

    let deg_svg = r##"<svg viewBox="0 0 100 100"><defs>
        <linearGradient id="zero_lin" x1="5" y1="5" x2="5" y2="5">
          <stop offset="0" stop-color="#112233"/><stop offset="1" stop-color="#445566"/>
        </linearGradient>
        <radialGradient id="zero_rad" cx="0.5" cy="0.5" r="0">
          <stop offset="0" stop-color="#000000"/><stop offset="1" stop-color="#778899"/>
        </radialGradient>
        <linearGradient id="zero_scale" gradientTransform="scale(0)">
          <stop offset="0" stop-color="#000000"/><stop offset="1" stop-color="#aabbcc"/>
        </linearGradient>
        <linearGradient id="obb_horiz" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#00ff00"/>
        </linearGradient>
      </defs>
      <rect id="r1" x="0" y="0" width="20" height="20" fill="url(#zero_lin)"/>
      <rect id="r2" x="25" y="0" width="20" height="20" fill="url(#zero_rad)"/>
      <rect id="r3" x="50" y="0" width="20" height="20" fill="url(#zero_scale)"/>
      <line id="l1" x1="0" y1="40" x2="80" y2="40" stroke="url(#obb_horiz)" stroke-width="4"/>
    </svg>"##;
    let (deg_proj, deg_report) =
        import_svg_with_options(deg_svg, "deg", &VectorImportOptions::default()).unwrap();
    deg_proj.validate().unwrap();
    let approx_count = deg_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "svg-gradient-approximated")
        .count();
    assert_eq!(approx_count, 4);
    let els = &deg_proj.symbol(&deg_proj.root).unwrap().timeline.layers[0].keyframes[0].elements;
    assert!(matches!(
        els[0].kind,
        ElementKind::Path {
            fill: [0x44, 0x55, 0x66, 255],
            ..
        }
    ));
    assert!(matches!(
        els[1].kind,
        ElementKind::Path {
            fill: [0x77, 0x88, 0x99, 255],
            ..
        }
    ));
    assert!(matches!(
        els[2].kind,
        ElementKind::Path {
            fill: [0xaa, 0xbb, 0xcc, 255],
            ..
        }
    ));
    assert!(matches!(
        els[3].kind,
        ElementKind::Path {
            stroke: [0x00, 0xff, 0x00, 255],
            ..
        }
    ));
}

#[test]
fn t414_pdf_radial_focal_ratio_tube_shading_no_duplicate_mask_and_hex_octal_text() {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.6");
    let tree = doc.new_object_id();
    let fn_lin = doc.add_object(dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "N" => 1,
        "C0" => vec![1.into(), 0.into(), 0.into()],
        "C1" => vec![0.into(), 0.into(), 1.into()],
    });
    // r0 = 20, r1 = 40, dist = 15 => focal_point_ratio = 15 / (40 - 20) = 0.75
    let sh_cone = doc.add_object(dictionary! {
        "ShadingType" => 3,
        "ColorSpace" => "DeviceRGB",
        "Coords" => vec![65.into(), 50.into(), 20.into(), 50.into(), 50.into(), 40.into()],
        "Function" => Object::Reference(fn_lin),
    });
    // Equal-radius tube r0 == r1 == 15 along (10,20) -> (90,20)
    let sh_tube = doc.add_object(dictionary! {
        "ShadingType" => 3,
        "ColorSpace" => "DeviceRGB",
        "Coords" => vec![10.into(), 20.into(), 15.into(), 90.into(), 20.into(), 15.into()],
        "Function" => Object::Reference(fn_lin),
    });

    let content = br#"q 10 10 80 80 re W n /ShCone sh Q
q 10 5 80 30 re W n /ShTube sh Q
BT /F1 18 Tf 10 80 Td <48656C6C6F> Tj 0 -20 Td [<0048> -10 <0069>] TJ 0 -20 Td <FEFF041F0440> Tj 0 -20 Td (\320\241\nLine2\t\(ok\)) Tj ET"#;
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => tree,
        "Contents" => Object::Reference(stream),
        "Resources" => dictionary! {
            "Shading" => dictionary! {
                "ShCone" => Object::Reference(sh_cone),
                "ShTube" => Object::Reference(sh_tube),
            }
        }
    });
    doc.objects.insert(
        tree,
        dictionary! {
            "Type" => "Pages",
            "Count" => 1,
            "Kids" => vec![Object::Reference(page)],
            "MediaBox" => vec![0.into(), 0.into(), 120.into(), 100.into()],
        }
        .into(),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    let (project, report) =
        import_pdf_ai_with_options(&bytes, "t414_pdf", &VectorImportOptions::default()).unwrap();
    project.validate().unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "pdf-shading-approximated"),
        "equal-radius r0 == r1 shading should record approximation diagnostic"
    );

    let layers = &project.symbol(&project.root).unwrap().timeline.layers;
    assert!(
        layers.iter().all(|l| l.properties.role != LayerRole::Mask),
        "`W n /Sh* sh` must not emit duplicate Mask layers when `sh` bakes `gstate.clip`"
    );

    let els = &layers[0].keyframes[0].elements;
    let ElementKind::PaintedPath {
        fill_gradient: Some(cone_grad),
        ..
    } = &els[0].kind
    else {
        panic!("expected radial cone PaintedPath");
    };
    assert_eq!(cone_grad.kind, GradientKind::Radial);
    assert!(
        (cone_grad.focal_point_ratio - 0.75).abs() < 1e-4,
        "expected dist/(r1-r0) = 0.75, got {}",
        cone_grad.focal_point_ratio
    );

    let ElementKind::PaintedPath {
        fill_gradient: Some(tube_grad),
        ..
    } = &els[1].kind
    else {
        panic!("expected linear-approximated tube PaintedPath");
    };
    assert_eq!(tube_grad.kind, GradientKind::Linear);
    assert_eq!(tube_grad.stops.first().unwrap().ratio, 0);
    assert_eq!(tube_grad.stops.last().unwrap().ratio, 255);

    let texts: Vec<&str> = els
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Hello", "Hi", "Пр", "С\nLine2\t(ok)"]);
}

fn zlib_compress_test(data: &[u8]) -> Vec<u8> {
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write as _;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn png_up_filter_rows(raw: &[u8], cols: usize) -> Vec<u8> {
    assert_eq!(raw.len() % cols, 0);
    let mut out = Vec::with_capacity((raw.len() / cols) * (cols + 1));
    let mut prev = vec![0u8; cols];
    for row in raw.chunks_exact(cols) {
        out.push(2); // PNG Up filter
        for c in 0..cols {
            out.push(row[c].wrapping_sub(prev[c]));
        }
        prev.copy_from_slice(row);
    }
    out
}

/// Build a genuine PDF 1.5 byte buffer using ONLY `/Type /ObjStm` compressed objects and a
/// PNG-Up-predicted `/Type /XRef` cross-reference stream (zero classic `xref` table), matching
/// Adobe Illustrator CC's `com.adobe.pdf` clipboard output.
fn build_illustrator_pdf15_clipboard_fixture() -> Vec<u8> {
    // Object ID layout:
    //  1 (in ObjStm 20): /Catalog
    //  2 (in ObjStm 20): /Pages
    //  3 (in ObjStm 20): /Page (/MediaBox [0 0 960 600], /ArtBox [120 330 420 510])
    //  4 (in ObjStm 20): Page /Resources
    //  5 (in ObjStm 20): /ExtGState /GS1 (ca=0.5, CA=0.8, BM=/Multiply)
    //  6 (in ObjStm 20): /FunctionType 3 multi-stop stitching function (Red -> Gold -> Blue)
    //  7 (in ObjStm 20): /ShadingType 2 axial shading -> Function 6
    //  8 (in ObjStm 20): /ShadingType 3 radial shading -> Function 6
    //  9 (in ObjStm 20): /PatternType 2 shading pattern /P1 -> Shading 7
    // 10 (in ObjStm 20): /Font /Type0 /F1 -> ToUnicode 15
    // 11 (stream): Page /Contents (FlateDecode)
    // 12 (stream): /Subtype /Form /Fm1 (FlateDecode)
    // 13 (stream): /Subtype /Image /Im1 (2x2 RGB FlateDecode, /SMask 14 0 R)
    // 14 (stream): /Subtype /Image SMask (2x2 DeviceGray FlateDecode)
    // 15 (stream): /ToUnicode CMap for Cyrillic "Привет, мир!"
    // 16 (stream): /FunctionType 4 PostScript tint transform for /Separation /SpotCoral
    // 20 (stream): /Type /ObjStm containing objects 1..=10
    // 21 (stream): /Type /XRef with FlateDecode + Predictor 12 (PNG Up), Columns 5
    let objstm_entries: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 960 600] /CropBox [0 0 960 600] /ArtBox [120 330 420 510] /Resources 4 0 R /Contents 11 0 R >>".into(),
        ),
        (
            4,
            "<< /ColorSpace << /CSspot [/Separation /SpotCoral /DeviceCMYK 16 0 R] >> /ExtGState << /GS1 5 0 R >> /Shading << /Sh2 7 0 R /Sh3 8 0 R >> /Pattern << /P1 9 0 R >> /Font << /F1 10 0 R >> /XObject << /Fm1 12 0 R /Im1 13 0 R >> >>".into(),
        ),
        (
            5,
            "<< /Type /ExtGState /ca 0.5 /CA 0.8 /BM /Multiply >>".into(),
        ),
        (
            6,
            "<< /FunctionType 3 /Domain [0 1] /Bounds [0.5] /Encode [0 1 0 1] /Functions [ << /FunctionType 2 /Domain [0 1] /N 1 /C0 [1 0 0] /C1 [1 0.8 0] >> << /FunctionType 2 /Domain [0 1] /N 1 /C0 [1 0.8 0] /C1 [0 0.3 1] >> ] >>".into(),
        ),
        (
            7,
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [130 480 230 480] /Extend [true true] /Function 6 0 R >>".into(),
        ),
        (
            8,
            "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [280 460 2 280 460 35] /Extend [true true] /Function 6 0 R >>".into(),
        ),
        (
            9,
            "<< /Type /Pattern /PatternType 2 /Shading 7 0 R /Matrix [1 0 0 1 0 0] >>".into(),
        ),
        (
            10,
            "<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Inter-Bold /Encoding /Identity-H /ToUnicode 15 0 R /DescendantFonts [ << /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Inter-Bold /DW 600 >> ] >>".into(),
        ),
    ];

    let mut header_part = String::new();
    let mut body_part = String::new();
    for (oid, body) in &objstm_entries {
        let rel_offset = body_part.len();
        header_part.push_str(&format!("{oid} {rel_offset} "));
        body_part.push_str(body);
        body_part.push('\n');
    }
    let first_offset = header_part.len();
    let mut objstm_raw = header_part.into_bytes();
    objstm_raw.extend_from_slice(body_part.as_bytes());
    let objstm_zlib = zlib_compress_test(&objstm_raw);

    // Page content stream (in PDF coordinates; ArtBox is [120, 330, 420, 510]):
    // 1. EvenOdd compound shape with hole + CMYK stroke + SpotCoral Separation fill + dash
    // 2. Pattern /P1 multi-stop linear gradient fill (`CS /Pattern /P1 scn ... f`)
    // 3. Clipped radial `/Sh3 sh` + `/GS1 gs` (`/ca 0.5 /BM /Multiply`) + clipped path (`W n ... Q`)
    // 4. `/Fm1 Do` Form XObject
    // 5. Two `/Im1 Do` RGBA `/SMask` Image XObject placements (proving deduplication)
    // 6. `BT .. ET` `/F1` Cyrillic ToUnicode `"Привет, мир!"`
    let page_ops = "\
q /CSspot cs 0.85 scn 0.9 0.1 0.0 0.0 K 3 w 1 J 1 j 6 M [4 2] 0 d \
125 430 70 70 re 145 450 30 30 re B* Q \
q /Pattern cs /P1 scn 130 380 100 40 re f Q \
q 245 425 70 70 re W n /Sh3 sh Q \
q 330 430 60 60 re W n /GS1 gs 0.2 0.8 0.4 rg 325 425 70 70 re f Q \
q 1 0 0 1 130 340 cm /Fm1 Do Q \
q 24 0 0 24 220 340 cm /Im1 Do Q \
q 24 0 0 24 260 340 cm /Im1 Do Q \
BT /F1 18 Tf 0.1 0.1 0.15 rg 1 0 0 1 300 355 Tm <000100020003000400050006000700080009000A000B000C> Tj ET\n";
    let page_zlib = zlib_compress_test(page_ops.as_bytes());

    let form_ops = "0.95 0.35 0.15 rg 0 0 m 30 0 l 30 25 l 0 25 l h f\n";
    let form_zlib = zlib_compress_test(form_ops.as_bytes());

    // 2x2 RGB pixels + 2x2 Grayscale SMask alpha ([255, 128, 64, 0])
    let img_rgb_zlib = zlib_compress_test(&[
        255, 0, 0, 0, 255, 0, // row 0
        0, 0, 255, 255, 255, 0, // row 1
    ]);
    let img_alpha_zlib = zlib_compress_test(&[255, 128, 64, 0]);

    // Cyrillic "Привет, мир!" UTF-16BE:
    // П=041F р=0440 и=0438 в=0432 е=0435 т=0442 ,=002C ' '=0020 м=043C и=0438 р=0440 !=0021
    let cmap_text = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
12 beginbfchar
<0001> <041F>
<0002> <0440>
<0003> <0438>
<0004> <0432>
<0005> <0435>
<0006> <0442>
<0007> <002C>
<0008> <0020>
<0009> <043C>
<000A> <0438>
<000B> <0440>
<000C> <0021>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end\n";
    let cmap_zlib = zlib_compress_test(cmap_text.as_bytes());

    // Type 4 PostScript tint transform: tint -> [0.1*t, 0.8*t, 0.6*t, 0.0]
    let fn4_ps = b"{ dup 0.1 mul exch dup 0.8 mul exch 0.6 mul 0.0 }";
    let fn4_zlib = zlib_compress_test(fn4_ps);

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n");

    let mut file_offsets = std::collections::HashMap::<u32, usize>::new();
    let push_stream_obj = |buf: &mut Vec<u8>,
                           offsets: &mut std::collections::HashMap<u32, usize>,
                           oid: u32,
                           dict_extra: &str,
                           zlib_bytes: &[u8]| {
        offsets.insert(oid, buf.len());
        let header = format!(
            "{oid} 0 obj\n<< {dict_extra} /Filter /FlateDecode /Length {} >>\nstream\n",
            zlib_bytes.len()
        );
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(zlib_bytes);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
    };

    push_stream_obj(&mut pdf, &mut file_offsets, 11, "", &page_zlib);
    push_stream_obj(
        &mut pdf,
        &mut file_offsets,
        12,
        "/Type /XObject /Subtype /Form /FormType 1 /BBox [0 0 30 25] /Matrix [1 0 0 1 0 0]",
        &form_zlib,
    );
    push_stream_obj(
        &mut pdf,
        &mut file_offsets,
        13,
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 14 0 R",
        &img_rgb_zlib,
    );
    push_stream_obj(
        &mut pdf,
        &mut file_offsets,
        14,
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
        &img_alpha_zlib,
    );
    push_stream_obj(&mut pdf, &mut file_offsets, 15, "", &cmap_zlib);
    push_stream_obj(
        &mut pdf,
        &mut file_offsets,
        16,
        "/FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1 0 1]",
        &fn4_zlib,
    );
    push_stream_obj(
        &mut pdf,
        &mut file_offsets,
        20,
        &format!(
            "/Type /ObjStm /N {} /First {first_offset}",
            objstm_entries.len()
        ),
        &objstm_zlib,
    );

    let xref_offset = pdf.len();
    file_offsets.insert(21, xref_offset);

    // Build /Type /XRef stream for IDs 0..=21 with /W [1 3 1] (5 bytes/row) + PNG Up Predictor 12!
    let total_ids = 22u32;
    let mut xref_raw = Vec::with_capacity((total_ids as usize) * 5);
    for oid in 0..total_ids {
        if oid == 0 {
            xref_raw.extend_from_slice(&[0, 0, 0, 0, 255]);
        } else if (1..=10).contains(&oid) {
            let idx_in_objstm = (oid - 1) as u8;
            // Type 2: compressed in ObjStm 20 at index `idx_in_objstm`
            xref_raw.extend_from_slice(&[2, 0, 0, 20, idx_in_objstm]);
        } else if let Some(&off) = file_offsets.get(&oid) {
            let b0 = ((off >> 16) & 0xFF) as u8;
            let b1 = ((off >> 8) & 0xFF) as u8;
            let b2 = (off & 0xFF) as u8;
            xref_raw.extend_from_slice(&[1, b0, b1, b2, 0]);
        } else {
            xref_raw.extend_from_slice(&[0, 0, 0, 0, 0]);
        }
    }
    let xref_predicted = png_up_filter_rows(&xref_raw, 5);
    let xref_zlib = zlib_compress_test(&xref_predicted);
    let xref_header = format!(
        "21 0 obj\n<< /Type /XRef /Size {total_ids} /Root 1 0 R /W [1 3 1] \
         /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 5 >> /Length {} >>\nstream\n",
        xref_zlib.len()
    );
    pdf.extend_from_slice(xref_header.as_bytes());
    pdf.extend_from_slice(&xref_zlib);
    pdf.extend_from_slice(
        format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    pdf
}

#[test]
fn t415_illustrator_pdf15_xref_objstm_artbox_cmyk_separation_evenodd_fn3_masks_extgstate_form_smask_image_and_cyrillic_tounicode()
 {
    let pdf_bytes = build_illustrator_pdf15_clipboard_fixture();
    assert!(
        !pdf_bytes.windows(5).any(|w| w == b"\nxref"),
        "fixture must use pure PDF 1.5 /Type /XRef + /ObjStm with zero classic xref table"
    );

    let out = import_pdf_clipboard(&pdf_bytes, "illustrator-clipboard.pdf")
        .expect("PDF 1.5 /XRef + /ObjStm clipboard import must succeed");
    out.project.validate().unwrap();

    // 1. `/ArtBox [120 330 420 510]` inside `/MediaBox [0 0 960 600]` -> Stage `[120, 90, 420, 270]` (300 x 180)!
    assert_eq!(out.project.width, 300);
    assert_eq!(out.project.height, 180);
    assert!((out.stage_bounds[0] - 120.0).abs() < 1e-6);
    assert!((out.stage_bounds[1] - 90.0).abs() < 1e-6);
    assert!((out.stage_bounds[2] - 420.0).abs() < 1e-6);
    assert!((out.stage_bounds[3] - 270.0).abs() < 1e-6);

    let root_sym = out.project.symbol(&out.project.root).unwrap();
    let all_root_els: Vec<&Element> = root_sym
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .collect();

    // 2. EvenOdd compound path with hole (`B*`) + SpotCoral `/Separation` Type 4 tint fill!
    let even_odd_fill = all_root_els
        .iter()
        .find(|el| {
            matches!(
                &el.kind,
                ElementKind::Path {
                    fill_rule: FillRule::EvenOdd,
                    fill,
                    ..
                } if fill[3] == 255 && fill[0] > 200 && fill[1] < 120
            )
        })
        .expect("expected EvenOdd SpotCoral Separation compound path");
    if let ElementKind::Path { commands, .. } = &even_odd_fill.kind {
        let move_count = commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count();
        assert!(move_count >= 2, "compound hole must retain both subpaths");
    }

    // 3. Multi-stop `/FunctionType 3` gradients via BOTH `/Pattern` (`scn`) AND `/Sh3 sh`!
    let linear_grads: Vec<&GradientPaint> = all_root_els
        .iter()
        .filter_map(|el| match &el.kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } if g.kind == GradientKind::Linear => Some(g),
            _ => None,
        })
        .collect();
    assert!(
        linear_grads.iter().any(|g| g.stops.len() >= 3),
        "Pattern /P1 -> Type 2 /FunctionType 3 stitching gradient must have >= 3 stops"
    );
    let radial_grads: Vec<&GradientPaint> = all_root_els
        .iter()
        .filter_map(|el| match &el.kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } if g.kind == GradientKind::Radial => Some(g),
            _ => None,
        })
        .collect();
    assert!(
        radial_grads.iter().any(|g| g.stops.len() >= 3),
        "/Sh3 sh Type 3 radial /FunctionType 3 stitching gradient must have >= 3 stops"
    );

    // 4. `q ... W n /GS1 gs ... Q` -> wrapped `Clipped Group` MovieClip symbol with `LayerRole::Mask`
    //    AND `/ExtGState` `/ca 0.5` + `/BM /Multiply` inside!
    let clipped_sym = out
        .project
        .symbols
        .iter()
        .find(|s| {
            s.id != out.project.root
                && s.timeline
                    .layers
                    .iter()
                    .any(|l| l.properties.role == LayerRole::Mask)
        })
        .expect("W n clipping path must produce a masked MovieClip symbol");
    let has_multiply_half_alpha = clipped_sym
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.role == LayerRole::Normal)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|e| {
            e.blend_mode == BlendMode::Multiply
                && matches!(e.kind, ElementKind::Path { fill, .. } if (120..=135).contains(&fill[3]))
        });
    assert!(
        has_multiply_half_alpha,
        "clipped element must preserve /ExtGState /BM /Multiply and /ca 0.5"
    );

    // 5. `/Fm1 Do` Form XObject -> nested `MovieClip` symbol + `ElementKind::Instance`!
    assert!(
        all_root_els
            .iter()
            .any(|e| e.name.contains("Fm1") && matches!(e.kind, ElementKind::Instance { .. })),
        "/Fm1 Do must import as nested MovieClip Instance"
    );

    // 6. `/Im1 Do` RGBA `/SMask` Image XObject (invoked twice) -> 2 `Bitmap` elements, 1 deduplicated `ResourceUnit`!
    let bitmaps: Vec<&Element> = all_root_els
        .iter()
        .copied()
        .filter(|e| matches!(e.kind, ElementKind::Bitmap { .. }))
        .collect();
    assert_eq!(
        bitmaps.len(),
        2,
        "two /Im1 Do invocations must place 2 Bitmap elements"
    );
    assert_eq!(
        out.project.resources.len(),
        1,
        "identical /Im1 Image XObject must deduplicate to 1 lossless PNG ResourceUnit"
    );
    let decoded_png = image::load_from_memory(&out.project.resources[0].decoded_bytes().unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded_png.dimensions(), (2, 2));
    assert_eq!(decoded_png.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(decoded_png.get_pixel(1, 0).0, [0, 255, 0, 128]);
    assert_eq!(decoded_png.get_pixel(0, 1).0, [0, 0, 255, 64]);
    assert_eq!(decoded_png.get_pixel(1, 1).0, [255, 255, 0, 0]);

    // 7. `BT .. ET` `/ToUnicode` Cyrillic text -> editable `ElementKind::TextBox`!
    let cyrillic_tb = all_root_els
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::TextBox { text, format, .. } if text == "Привет, мир!" => {
                Some((text.as_str(), format))
            }
            _ => None,
        })
        .expect("Cyrillic /ToUnicode BT..ET text must import as editable TextBox");
    assert_eq!(cyrillic_tb.1.font, "Inter");
    assert!(cyrillic_tb.1.bold);
}

#[test]
fn t415_illustrator_aicb_and_binary_eps_vectors_strokes_colors_clip_and_security_caps() {
    // 1. ASCII AICB / PostScript Level 3 EPS with HiResBoundingBox, %AI5_ArtSize, compound *u/*U
    //    EvenOdd hole, AICB XA/XR, CMYK k/K, dashed stroke, gsave..clip..grestore, and shfill!
    let aicb_eps = "\
%!PS-Adobe-3.0 EPSF-3.0
%%Creator: Adobe Illustrator(R) 28.0
%%BoundingBox: 80 220 321 400
%%HiResBoundingBox: 80.5 220.0 320.5 400.0
%AI5_ArtSize: 960 600
%%EndComments
1 XR
*u
0.95 0.25 0.15 XA
0.1 0.8 0.2 0.0 K
2.5 w 1 J 1 j 5 M [6 3] 0 d
85 310 m 175 310 l 175 390 l 85 390 l h
105 330 m 155 330 l 155 370 l 105 370 l h
B*
*U
0 XR
gsave
190 230 110 80 rectclip
0.2 0.6 0.95 rg
180 220 m 220 320 260 320 310 220 c 310 300 v 190 300 y closepath fill
<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [195 240 295 240] \
   /Function << /FunctionType 2 /Domain [0 1] /N 1 /C0 [1 0.9 0] /C1 [0.9 0 0.5] >> >> shfill
grestore
showpage
%%EOF
";
    let eps_out = import_eps_clipboard(aicb_eps.as_bytes(), "aicb-clipboard.eps")
        .expect("AICB / Level 3 EPS vector clipboard must parse");
    eps_out.project.validate().unwrap();
    // HiResBoundingBox [80.5, 220, 320.5, 400] on %AI5_ArtSize 960x600 -> Stage [80.5, 200.0, 320.5, 380.0] (240 x 180)
    assert_eq!(eps_out.project.width, 240);
    assert_eq!(eps_out.project.height, 180);
    assert!((eps_out.stage_bounds[0] - 80.5).abs() < 1e-6);
    assert!((eps_out.stage_bounds[1] - 200.0).abs() < 1e-6);
    assert!((eps_out.stage_bounds[2] - 320.5).abs() < 1e-6);
    assert!((eps_out.stage_bounds[3] - 380.0).abs() < 1e-6);
    assert!(
        eps_out.project.resources.is_empty(),
        "vector EPS must NOT create raster preview resources"
    );

    let root_els: Vec<&Element> = eps_out
        .project
        .symbol(&eps_out.project.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .collect();
    assert!(
        root_els.iter().any(|e| matches!(
            e.kind,
            ElementKind::Path {
                fill_rule: FillRule::EvenOdd,
                ..
            }
        )),
        "AICB 1 XR + *u..*U B* must import EvenOdd compound hole"
    );
    assert!(
        eps_out.project.symbols.iter().any(|s| {
            s.id != eps_out.project.root
                && s.timeline
                    .layers
                    .iter()
                    .any(|l| l.properties.role == LayerRole::Mask)
        }),
        "gsave..rectclip..grestore must preserve clipping mask symbol"
    );
    assert!(
        eps_out
            .project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .any(|e| matches!(
                e.kind,
                ElementKind::PaintedPath {
                    fill_gradient: Some(_),
                    ..
                }
            )),
        "Level 3 shfill must import as vector PaintedPath gradient"
    );

    // 2. DOS Binary EPS (`0xC5D0D3C6`) containing BOTH PostScript vectors AND an embedded TIFF preview:
    //    must decode editable vector paths and NEVER fall back to the raster TIFF preview!
    let ps_sec = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 10 20 110 80\n1 0 0 rg 10 20 m 110 20 l 110 80 l 10 80 l closepath fill\n%%EOF\n";
    let mut tiff_preview = Vec::new();
    {
        let preview_img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 255, 0, 255]));
        preview_img
            .write_to(
                &mut std::io::Cursor::new(&mut tiff_preview),
                image::ImageFormat::Tiff,
            )
            .unwrap();
    }
    let ps_offset = 30u32;
    let ps_len = ps_sec.len() as u32;
    let tiff_offset = ps_offset + ps_len;
    let tiff_len = tiff_preview.len() as u32;
    let mut dos_eps = Vec::with_capacity((tiff_offset + tiff_len) as usize);
    dos_eps.extend_from_slice(&[0xC5, 0xD0, 0xD3, 0xC6]);
    dos_eps.extend_from_slice(&ps_offset.to_le_bytes());
    dos_eps.extend_from_slice(&ps_len.to_le_bytes());
    dos_eps.extend_from_slice(&0u32.to_le_bytes()); // WMF offset
    dos_eps.extend_from_slice(&0u32.to_le_bytes()); // WMF len
    dos_eps.extend_from_slice(&tiff_offset.to_le_bytes());
    dos_eps.extend_from_slice(&tiff_len.to_le_bytes());
    dos_eps.extend_from_slice(&0xFFFFu16.to_le_bytes());
    dos_eps.extend_from_slice(ps_sec);
    dos_eps.extend_from_slice(&tiff_preview);

    let bin_out = import_eps_clipboard(&dos_eps, "dos-binary.eps").unwrap();
    bin_out.project.validate().unwrap();
    assert!(
        bin_out.project.resources.is_empty(),
        "DOS binary EPS with vector paths must NOT fall back to embedded TIFF preview"
    );
    assert!(
        bin_out
            .project
            .symbol(&bin_out.project.root)
            .unwrap()
            .timeline
            .layers[0]
            .keyframes[0]
            .elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::Path { .. })),
        "DOS binary EPS must produce editable vector Path"
    );

    // 3. Security caps: >32,768 path commands, >64 gsave stack, and >32 /Form recursion rejected cleanly!
    let mut huge_eps = String::from("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 100\n0 0 m\n");
    for i in 0..32_800 {
        huge_eps.push_str(&format!("{} {} l\n", i % 90, (i / 90) % 90));
    }
    huge_eps.push_str("f\n%%EOF\n");
    assert!(
        import_eps_clipboard(huge_eps.as_bytes(), "bomb.eps").is_err(),
        "EPS exceeding 32,768 path commands must be rejected"
    );

    let mut deep_gsave = String::from("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 100\n");
    for _ in 0..70 {
        deep_gsave.push_str("gsave\n");
    }
    deep_gsave.push_str("0 0 10 10 rectfill\n%%EOF\n");
    assert!(
        import_eps_clipboard(deep_gsave.as_bytes(), "deep-gsave.eps").is_err(),
        "EPS exceeding 64 gsave depth must be rejected"
    );

    // Cyclic / deep Form XObject recursion (>32) in PDF rejected cleanly!
    {
        use lopdf::{Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let tree = doc.new_object_id();
        let fm_id = doc.new_object_id();
        let fm_res = dictionary! {
            "XObject" => dictionary! { "FmLoop" => Object::Reference(fm_id) }
        };
        doc.objects.insert(
            fm_id,
            Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
                    "Resources" => fm_res.clone(),
                },
                b"/FmLoop Do".to_vec(),
            )
            .into(),
        );
        let contents = doc.add_object(Stream::new(dictionary! {}, b"/FmLoop Do".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => tree,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => fm_res,
            "Contents" => contents,
        });
        doc.objects.insert(
            tree,
            dictionary! {
                "Type" => "Pages",
                "Count" => 1,
                "Kids" => vec![Object::Reference(page)],
            }
            .into(),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
        doc.trailer.set("Root", root);
        let mut loop_pdf = Vec::new();
        doc.save_to(&mut loop_pdf).unwrap();
        assert!(
            import_pdf_clipboard(&loop_pdf, "loop.pdf").is_err(),
            "self-referential /Form XObject recursion must be rejected"
        );
    }

    // 4. 36 flat sibling `/Subtype /Form` XObjects sharing ONE `/Resources` dictionary (`O(K)` lazy
    //    memoization — zero `K!` explosion and zero false `>32` depth error!), PLUS 34-deep nested
    //    `/Form` chain rejected cleanly!
    {
        use lopdf::{Dictionary, Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let tree = doc.new_object_id();
        let shared_res_id = doc.new_object_id();
        let mut xobj_dict = Dictionary::new();
        let mut page_ops = String::new();
        for i in 0..36 {
            let fm_id = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 12.into(), 12.into()],
                    "Resources" => Object::Reference(shared_res_id),
                },
                b"0.2 0.5 0.9 rg 0 0 10 10 re f\n".to_vec(),
            ));
            xobj_dict.set(format!("Fm{i}"), Object::Reference(fm_id));
            page_ops.push_str(&format!("q 1 0 0 1 {} 10 cm /Fm{i} Do Q\n", i * 14));
        }
        doc.objects
            .insert(shared_res_id, dictionary! { "XObject" => xobj_dict }.into());
        let contents = doc.add_object(Stream::new(dictionary! {}, page_ops.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => tree,
            "MediaBox" => vec![0.into(), 0.into(), 600.into(), 100.into()],
            "Resources" => Object::Reference(shared_res_id),
            "Contents" => contents,
        });
        doc.objects.insert(
            tree,
            dictionary! {
                "Type" => "Pages",
                "Count" => 1,
                "Kids" => vec![Object::Reference(page)],
            }
            .into(),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
        doc.trailer.set("Root", root);
        let mut many_forms_pdf = Vec::new();
        doc.save_to(&mut many_forms_pdf).unwrap();

        let siblings_out = import_pdf_clipboard(&many_forms_pdf, "siblings36.pdf")
            .expect("36 flat sibling /Form XObjects sharing one /Resources must succeed in O(K)");
        siblings_out.project.validate().unwrap();
        let placed_count = siblings_out
            .project
            .symbol(&siblings_out.project.root)
            .unwrap()
            .timeline
            .layers[0]
            .keyframes[0]
            .elements
            .len();
        assert_eq!(placed_count, 36);

        // 34-deep nested chain Fm0 -> Fm1 -> ... -> Fm33 must fail MAX_PDF_FORM_DEPTH (32)!
        let mut deep_doc = Document::with_version("1.5");
        let dtree = deep_doc.new_object_id();
        let mut prev_fm = deep_doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            },
            b"0 0 10 10 re f\n".to_vec(),
        ));
        for depth in 0..34 {
            let name = format!("Chain{depth}");
            prev_fm = deep_doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                    "Resources" => dictionary! {
                        "XObject" => dictionary! { name.clone() => Object::Reference(prev_fm) }
                    },
                },
                format!("/{name} Do\n").into_bytes(),
            ));
        }
        let dcontents =
            deep_doc.add_object(Stream::new(dictionary! {}, b"/TopChain Do\n".to_vec()));
        let dpage = deep_doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => dtree,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! {
                "XObject" => dictionary! { "TopChain" => Object::Reference(prev_fm) }
            },
            "Contents" => dcontents,
        });
        deep_doc.objects.insert(
            dtree,
            dictionary! {
                "Type" => "Pages",
                "Count" => 1,
                "Kids" => vec![Object::Reference(dpage)],
            }
            .into(),
        );
        let droot = deep_doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => dtree });
        deep_doc.trailer.set("Root", droot);
        let mut deep_pdf = Vec::new();
        deep_doc.save_to(&mut deep_pdf).unwrap();
        assert!(
            import_pdf_clipboard(&deep_pdf, "deep-chain.pdf").is_err(),
            "34-deep nested /Form chain must be rejected"
        );
    }

    // 5. Cumulative >64 MiB multi-/ObjStm decompression bomb (4 x 20 MiB = 80 MiB total, each <64 MiB)
    //    rejected cleanly with ZERO unbounded `lopdf::Document::load_mem` fallback!
    {
        let mut chunk_raw = String::from("30 0 ");
        chunk_raw.push_str("<< /Dummy (x) >>\n");
        let pad_target = 20 * 1024 * 1024;
        let first_off = 5usize;
        let mut full_chunk = chunk_raw.into_bytes();
        full_chunk.resize(pad_target, b' ');
        let compressed_20m = zlib_compress_test(&full_chunk);
        assert!(
            compressed_20m.len() < 128 * 1024,
            "20 MiB space-padded ObjStm compresses to <128 KiB"
        );

        let mut bomb_pdf = Vec::new();
        bomb_pdf.extend_from_slice(b"%PDF-1.5\n");
        let mut offs = [0usize; 9];
        offs[1] = bomb_pdf.len();
        bomb_pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offs[2] = bomb_pdf.len();
        bomb_pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offs[3] = bomb_pdf.len();
        bomb_pdf.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>\nendobj\n",
        );
        offs[4] = bomb_pdf.len();
        bomb_pdf.extend_from_slice(
            b"4 0 obj\n<< /Length 14 >>\nstream\n0 0 10 10 re f\nendstream\nendobj\n",
        );
        for (oid, slot) in offs.iter_mut().enumerate().take(9).skip(5) {
            *slot = bomb_pdf.len();
            bomb_pdf.extend_from_slice(
                format!(
                    "{oid} 0 obj\n<< /Type /ObjStm /N 1 /First {first_off} /Filter /FlateDecode /Length {} >>\nstream\n",
                    compressed_20m.len()
                )
                .as_bytes(),
            );
            bomb_pdf.extend_from_slice(&compressed_20m);
            bomb_pdf.extend_from_slice(b"\nendstream\nendobj\n");
        }
        let xref_off = bomb_pdf.len();
        bomb_pdf.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
        for &off in &offs[1..=8] {
            bomb_pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        bomb_pdf.extend_from_slice(
            format!("trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref_off}\n%%EOF\n")
                .as_bytes(),
        );

        assert!(
            import_pdf_clipboard(&bomb_pdf, "objstm-bomb.pdf").is_err(),
            "4 x 20 MiB = 80 MiB cumulative /ObjStm decompression bomb must be rejected without lopdf fallback"
        );
    }

    // 6. EPS inline `colorimage` (bitmap-only EPS without TIFF preview!) + rotated PDF `/Rotate 90` bounds!
    {
        let bmp_only_eps = "\
%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: 10 20 50 40
%AI5_ArtSize: 200 100
gsave
10 20 translate
40 20 scale
/picstr 6 string def
2 2 8 [2 0 0 -2 0 2]
{ currentfile picstr readhexstring pop } false 3 colorimage
ff000000ff00
0000ffffff00
grestore
showpage
%%EOF
";
        let eps_bmp = import_eps_clipboard(bmp_only_eps.as_bytes(), "bitmap-only.eps")
            .expect("bitmap-only colorimage EPS must decode without needing a TIFF preview");
        eps_bmp.project.validate().unwrap();
        assert_eq!(eps_bmp.project.resources.len(), 1);
        let root_els = &eps_bmp
            .project
            .symbol(&eps_bmp.project.root)
            .unwrap()
            .timeline
            .layers[0]
            .keyframes[0]
            .elements;
        assert!(matches!(
            root_els[0].kind,
            ElementKind::Bitmap {
                width: 2,
                height: 2,
                ..
            }
        ));

        // Oversized 5000x5000 inline `colorimage` rejected cleanly!
        let oversized_eps_img = "\
%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: 0 0 100 100
5000 5000 8 [5000 0 0 -5000 0 5000]
{ currentfile picstr readhexstring pop } false 3 colorimage
000000
%%EOF
";
        assert!(import_eps_clipboard(oversized_eps_img.as_bytes(), "oversized-img.eps").is_err());

        // Rotated PDF `/Rotate 90` Stage bounds check:
        use lopdf::{Document, Object, Stream, dictionary};
        let mut rdoc = Document::with_version("1.5");
        let rtree = rdoc.new_object_id();
        let rcontents =
            rdoc.add_object(Stream::new(dictionary! {}, b"20 30 40 50 re f\n".to_vec()));
        let rpage = rdoc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => rtree,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 100.into()],
            "ArtBox" => vec![20.into(), 30.into(), 60.into(), 80.into()],
            "Rotate" => 90,
            "Contents" => rcontents,
        });
        rdoc.objects.insert(
            rtree,
            dictionary! {
                "Type" => "Pages",
                "Count" => 1,
                "Kids" => vec![Object::Reference(rpage)],
            }
            .into(),
        );
        let rroot = rdoc.add_object(dictionary! { "Type" => "Catalog", "Pages" => rtree });
        rdoc.trailer.set("Root", rroot);
        let mut rbytes = Vec::new();
        rdoc.save_to(&mut rbytes).unwrap();
        let rout = import_pdf_clipboard(&rbytes, "rot90.pdf").unwrap();
        // With /Rotate 90 on MediaBox [0,0,200,100], ArtBox [20,30,60,80] has Stage size (50, 40) at [30, 20, 80, 60]
        assert_eq!(rout.project.width, 50);
        assert_eq!(rout.project.height, 40);
        assert_eq!(rout.stage_bounds, [30.0, 20.0, 80.0, 60.0]);
    }
}
