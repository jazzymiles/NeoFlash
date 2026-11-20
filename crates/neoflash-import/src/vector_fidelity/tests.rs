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
        b"1 0 0 rg 7 9 20 15 re f q 7 9 4 5 re W n".to_vec(),
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
    assert_eq!(report.statistics.elements, 3);
    let layers = &project.symbol(&project.root).unwrap().timeline.layers;
    assert_eq!(layers.len(), 4);
    assert!(layers[0].properties.mask.is_none() && layers[3].properties.mask.is_none());
    assert_eq!(layers[2].properties.mask.as_ref(), Some(&layers[1].id));
    let ElementKind::PaintedPath {
        fill_gradient: Some(paint),
        ..
    } = &layers[2].keyframes[0].elements[0].kind
    else {
        panic!("missing axial shading")
    };
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
        rad_grad.focal_point_ratio > 0.2 && rad_grad.focal_point_ratio < 0.3,
        "expected focal ratio ~0.25, got {}",
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
