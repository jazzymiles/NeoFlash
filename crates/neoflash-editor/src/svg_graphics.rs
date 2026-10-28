//! SVG normalization uses the Rust usvg/resvg libraries. Solid artwork becomes
//! editable native paths; richer paint is rendered once, with its source retained.
use crate::graphic_import::PreparedGraphic;
use neoflash_core::*;
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

pub(crate) fn prepare_svg(source: ResourceUnit) -> Result<PreparedGraphic, String> {
    let bytes = source.decoded_bytes().map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "SVG must be UTF-8 XML.")?;
    let xml = usvg::roxmltree::Document::parse(text).map_err(|e| format!("SVG XML: {e}"))?;
    if xml.root_element().tag_name().name() != "svg" {
        return Err("The file has no SVG root element.".into());
    }
    let mut has_text = false;
    for node in xml.descendants().filter(|node| node.is_element()) {
        let tag = node.tag_name().name();
        if matches!(
            tag,
            "script" | "animate" | "animateMotion" | "animateTransform" | "set" | "foreignObject"
        ) {
            return Err(format!(
                "SVG <{tag}> is not static artwork. Export a static SVG without scripts, animation or embedded HTML."
            ));
        }
        has_text |= tag == "text";
        for attribute in node.attributes() {
            if attribute.name().starts_with("on") {
                return Err("SVG event scripts cannot be imported as static artwork.".into());
            }
            if attribute.name() == "href"
                && !attribute.value().starts_with('#')
                && !attribute.value().starts_with("data:")
            {
                return Err("SVG references an external file. Embed linked images in the SVG before importing.".into());
            }
        }
    }
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    if has_text {
        static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
        options.fontdb = FONTS
            .get_or_init(|| {
                let mut db = usvg::fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            })
            .clone();
    }
    let tree = usvg::Tree::from_str(text, &options).map_err(|e| format!("SVG: {e}"))?;
    let width = tree.size().width() as f64;
    let height = tree.size().height() as f64;
    if width > 1_000_000. || height > 1_000_000. {
        return Err("SVG dimensions exceed the supported artwork size.".into());
    }
    if tree.root().children().is_empty() {
        return Err("The SVG contains no supported visible artwork.".into());
    }
    let mut elements = Vec::new();
    let vector = collect_group(tree.root(), &mut elements, 0).is_ok()
        && !elements.is_empty()
        && elements.iter().all(|element| match &element.kind {
            ElementKind::Path { commands, .. } => {
                path_bounds(commands).ok().flatten().is_some_and(|b| {
                    b.x_min >= -0.001
                        && b.y_min >= -0.001
                        && b.x_max <= width + 0.001
                        && b.y_max <= height + 0.001
                })
            }
            _ => false,
        });
    let name = source.path.clone();
    if vector {
        return Ok(PreparedGraphic {
            name,
            width,
            height,
            elements,
            resources: vec![source],
            notice: has_text.then(|| {
                "SVG text was converted to editable outlines using installed fonts.".into()
            }),
        });
    }
    let scale = (4096. / width).min(4096. / height).min(1.);
    let pixels_w = (width * scale).ceil().max(1.) as u32;
    let pixels_h = (height * scale).ceil().max(1.) as u32;
    let mut pixels =
        tiny_skia::Pixmap::new(pixels_w, pixels_h).ok_or("SVG image allocation failed.")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            pixels_w as f32 / width as f32,
            pixels_h as f32 / height as f32,
        ),
        &mut pixels.as_mut(),
    );
    let png = pixels
        .encode_png()
        .map_err(|e| format!("SVG raster image: {e}"))?;
    let resource = ResourceUnit::from_bytes(
        new_id("resource"),
        format!("{}.png", source.path),
        "image/png",
        &png,
    )
    .map_err(|e| e.to_string())?;
    let element = Element {
        id: new_id("object"),
        name: name.clone(),
        transform: Transform {
            scale_x: width / pixels_w as f64,
            scale_y: height / pixels_h as f64,
            ..Default::default()
        },
        kind: ElementKind::Bitmap {
            resource_id: resource.id.clone(),
            width: pixels_w,
            height: pixels_h,
            smoothing: true,
        },
        scripts: vec![],
        accessibility: None,
    };
    Ok(PreparedGraphic {name,width,height,elements:vec![element],resources:vec![source,resource],
        notice:Some("SVG gradients, images, clipping or effects were imported as a rendered image; the original SVG is retained in Resource files.".into())})
}

fn collect_group(group: &usvg::Group, output: &mut Vec<Element>, depth: usize) -> Result<(), ()> {
    if depth > 64
        || output.len() > 10_000
        || group.opacity().get() != 1.
        || group.blend_mode() != usvg::BlendMode::Normal
        || group.clip_path().is_some()
        || group.mask().is_some()
        || !group.filters().is_empty()
    {
        return Err(());
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_group(group, output, depth + 1)?,
            usvg::Node::Text(text) => collect_group(text.flattened(), output, depth + 1)?,
            usvg::Node::Image(_) => return Err(()),
            usvg::Node::Path(path) => {
                if !path.is_visible() {
                    continue;
                }
                let mut fill = None;
                if let Some(paint) = path.fill() {
                    let color = solid(paint.paint(), paint.opacity().get())?;
                    let rule = match paint.rule() {
                        usvg::FillRule::NonZero => FillRule::NonZero,
                        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    };
                    fill = Some(path_element(path, path.data().clone(), color, rule)?);
                }
                let mut stroke = None;
                if let Some(paint) = path.stroke() {
                    let color = solid(paint.paint(), paint.opacity().get())?;
                    let style = paint.to_tiny_skia();
                    let data = if let Some(dash) = &style.dash {
                        path.data().dash(dash, 1.).ok_or(())?
                    } else {
                        path.data().clone()
                    };
                    let outline = data.stroke(&style, 1.).ok_or(())?;
                    stroke = Some(path_element(path, outline, color, FillRule::NonZero)?);
                }
                if path.paint_order() == usvg::PaintOrder::FillAndStroke {
                    output.extend(fill);
                    output.extend(stroke);
                } else {
                    output.extend(stroke);
                    output.extend(fill);
                }
            }
        }
    }
    Ok(())
}

fn solid(paint: &usvg::Paint, opacity: f32) -> Result<Color, ()> {
    let usvg::Paint::Color(color) = paint else {
        return Err(());
    };
    Ok([
        color.red,
        color.green,
        color.blue,
        (opacity * 255.).round().clamp(0., 255.) as u8,
    ])
}

fn path_element(
    source: &usvg::Path,
    path: tiny_skia::Path,
    fill: Color,
    fill_rule: FillRule,
) -> Result<Element, ()> {
    let path = path.transform(source.abs_transform()).ok_or(())?;
    let mut commands = Vec::new();
    for segment in path.segments() {
        use tiny_skia::PathSegment::*;
        commands.push(match segment {
            MoveTo(p) => PathCommand::MoveTo {
                x: p.x as f64,
                y: p.y as f64,
            },
            LineTo(p) => PathCommand::LineTo {
                x: p.x as f64,
                y: p.y as f64,
            },
            QuadTo(c, p) => PathCommand::QuadraticTo {
                cx: c.x as f64,
                cy: c.y as f64,
                x: p.x as f64,
                y: p.y as f64,
            },
            CubicTo(a, b, p) => PathCommand::CubicTo {
                cx1: a.x as f64,
                cy1: a.y as f64,
                cx2: b.x as f64,
                cy2: b.y as f64,
                x: p.x as f64,
                y: p.y as f64,
            },
            Close => PathCommand::Close,
        });
        if commands.len() > MAX_PATH_COMMANDS {
            return Err(());
        }
    }
    validate_path_commands(&commands).map_err(|_| ())?;
    Ok(Element {
        id: new_id("object"),
        name: if source.id().is_empty() {
            "SVG path".into()
        } else {
            source.id().to_owned()
        },
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke: [0; 4],
            stroke_width: 0.,
        },
        scripts: vec![],
        accessibility: None,
    })
}
