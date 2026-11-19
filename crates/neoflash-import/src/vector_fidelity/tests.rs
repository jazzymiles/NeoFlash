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
