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
            ElementKind::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Hello", "Hi", "Пр", "С\nLine2\t(ok)"]);
}
