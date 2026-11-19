//! Represent an image-filled vector as masked footage in interchange formats that have no bitmap brush.
use crate::{ExportError, sha256_hex};
use neoflash_core::{
    AuthoredBitmapFill, Element, ElementKind, FillRule, InstancePlayback, Keyframe, Layer,
    LayerProperties, LayerRole, PathBounds, PathCommand, Project, ResourceUnit, Symbol, SymbolKind,
    Timeline, Transform, invert_affine_matrix, path_bounds,
};
use std::collections::{BTreeMap, HashMap};

fn layer(id: String, name: &str, element: Element, properties: LayerProperties) -> Layer {
    Layer {
        id,
        name: name.into(),
        visible: true,
        locked: false,
        properties,
        keyframes: vec![Keyframe {
            elements: vec![element],
            ..Default::default()
        }],
    }
}

fn inside_image(bounds: PathBounds, matrix: [f64; 6], w: u32, h: u32) -> bool {
    let Some([a, b, c, d, tx, ty]) = invert_affine_matrix(matrix) else {
        return false;
    };
    [bounds.x_min, bounds.x_max].into_iter().all(|x| {
        [bounds.y_min, bounds.y_max].into_iter().all(|y| {
            let u = a * x + c * y + tx;
            let v = b * x + d * y + ty;
            u >= -1e-7 && v >= -1e-7 && u <= f64::from(w) + 1e-7 && v <= f64::from(h) + 1e-7
        })
    })
}

fn stroke_only(
    kind: &ElementKind,
    commands: &[PathCommand],
    fill_rule: FillRule,
) -> Option<ElementKind> {
    let (color, width, gradient) = match kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => (*stroke, *stroke_width, None),
        ElementKind::PaintedPath {
            stroke,
            stroke_width,
            stroke_gradient,
            ..
        } => (*stroke, *stroke_width, stroke_gradient.clone()),
        _ => return None,
    };
    (width > 0. && (color[3] > 0 || gradient.is_some())).then(|| ElementKind::PaintedPath {
        commands: commands.to_owned(),
        fill_rule,
        fill: [0; 4],
        fill_gradient: None,
        stroke: color,
        stroke_width: width,
        stroke_gradient: gradient,
    })
}

struct FillLayers<'a> {
    project: &'a Project,
    prefix: String,
    symbols: BTreeMap<String, Symbol>,
    image_sizes: HashMap<String, (u32, u32)>,
    assets: Vec<ResourceUnit>,
}

impl FillLayers<'_> {
    fn picture(
        &mut self,
        id: &str,
        fill: &AuthoredBitmapFill,
        bounds: PathBounds,
    ) -> Result<Element, ExportError> {
        let (w, h) = if let Some(&size) = self.image_sizes.get(&fill.resource_id) {
            size
        } else {
            let image = self
                .project
                .resource(&fill.resource_id)
                .ok_or_else(|| {
                    ExportError::Options(format!("Missing image fill '{}'", fill.resource_id))
                })?
                .decode_image()
                .map_err(|e| ExportError::Options(e.to_string()))?;
            self.image_sizes
                .insert(fill.resource_id.clone(), (image.width, image.height));
            (image.width, image.height)
        };
        let (resource_id, width, height, transform) = if let Some(transform) =
            Transform::from_matrix(fill.matrix)
            && inside_image(bounds, fill.matrix, w, h)
        {
            (fill.resource_id.clone(), w, h, transform)
        } else {
            let (png, width, height) =
                crate::video::render_local_brush(self.project, fill, bounds)?;
            let asset_id = format!("{id}-tile");
            self.assets.push(
                ResourceUnit::from_bytes(&asset_id, format!("{asset_id}.png"), "image/png", &png)
                    .map_err(|e| ExportError::Options(e.to_string()))?,
            );
            let transform = Transform {
                x: bounds.x_min,
                y: bounds.y_min,
                scale_x: bounds.width() / f64::from(width),
                scale_y: bounds.height() / f64::from(height),
                ..Default::default()
            };
            (asset_id, width, height, transform)
        };
        Ok(Element {
            id: format!("{id}-image"),
            name: "Image".into(),
            transform,
            kind: ElementKind::Bitmap {
                resource_id,
                width,
                height,
                smoothing: fill.smoothing,
            },
            ..Default::default()
        })
    }

    fn symbol_for(
        &mut self,
        shape: &Element,
        fill: &AuthoredBitmapFill,
    ) -> Result<Option<String>, ExportError> {
        let key = serde_json::to_vec(&(
            &shape.kind,
            shape.transform.primitive,
            &fill.resource_id,
            fill.matrix,
            fill.repeat,
            fill.smoothing,
        ))
        .map_err(|e| ExportError::Options(e.to_string()))?;
        let id = format!("{}{}", self.prefix, sha256_hex(&key));
        if self.symbols.contains_key(&id) {
            return Ok(Some(id));
        }
        let Some((commands, rule)) = shape.to_path_commands() else {
            return Ok(None);
        };
        let Some(bounds) = path_bounds(&commands)
            .map_err(|e| ExportError::Options(e.to_string()))?
            .filter(|b| b.width() > 0. && b.height() > 0.)
        else {
            return Ok(None);
        };
        let picture = self.picture(&id, fill, bounds)?;
        let stroke = stroke_only(&shape.kind, &commands, rule);
        let outline = Element {
            id: format!("{id}-outline"),
            name: "Outline".into(),
            kind: ElementKind::Path {
                commands,
                fill_rule: rule,
                fill: [255; 4],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            ..Default::default()
        };
        let mask = format!("{id}-mask");
        let mut layers = vec![
            layer(
                format!("{id}-content"),
                "Image",
                picture,
                LayerProperties {
                    mask: Some(mask.clone()),
                    ..Default::default()
                },
            ),
            layer(
                mask,
                "Outline",
                outline,
                LayerProperties {
                    role: LayerRole::Mask,
                    ..Default::default()
                },
            ),
        ];
        if let Some(kind) = stroke {
            layers.push(layer(
                format!("{id}-stroke"),
                "Stroke",
                Element {
                    id: format!("{id}-line"),
                    name: "Stroke".into(),
                    kind,
                    ..Default::default()
                },
                Default::default(),
            ));
        }
        self.symbols.insert(
            id.clone(),
            Symbol {
                id: id.clone(),
                name: format!("Image fill · {}", shape.name),
                kind: SymbolKind::Graphic,
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                timeline: Timeline {
                    frame_count: 1,
                    layers,
                    scripts: vec![],
                    guides: vec![],
                },
            },
        );
        Ok(Some(id))
    }
}

pub(crate) fn prepare_image_fill_layers(project: &mut Project) -> Result<usize, ExportError> {
    let mut prefix = String::from("nf-image-fill-");
    while project.symbols.iter().any(|s| s.id.starts_with(&prefix))
        || project.resources.iter().any(|r| r.id.starts_with(&prefix))
    {
        prefix.push('_');
    }
    let mut fills = FillLayers {
        project,
        prefix,
        symbols: BTreeMap::new(),
        image_sizes: HashMap::new(),
        assets: vec![],
    };
    let mut replacements = Vec::new();
    for (si, symbol) in project.symbols.iter().enumerate() {
        for (li, layer) in symbol.timeline.layers.iter().enumerate() {
            for (ki, frame) in layer.keyframes.iter().enumerate() {
                for fill in &frame.bitmap_fills {
                    if let Some((ei, shape)) = frame
                        .elements
                        .iter()
                        .enumerate()
                        .find(|(_, el)| el.id == fill.element_id)
                        && let Some(id) = fills.symbol_for(shape, fill)?
                    {
                        replacements.push((si, li, ki, ei, id));
                    }
                }
            }
        }
    }
    let count = replacements.len();
    let FillLayers {
        symbols, assets, ..
    } = fills;
    for (si, li, ki, ei, id) in replacements {
        let frame = &mut project.symbols[si].timeline.layers[li].keyframes[ki];
        let element = &mut frame.elements[ei];
        element.kind = ElementKind::Instance {
            symbol_id: id,
            playback: InstancePlayback::SingleFrame(0),
        };
        element.transform.primitive = None;
        frame.bitmap_fills.retain(|f| f.element_id != element.id);
    }
    project.resources.extend(assets);
    project.symbols.extend(symbols.into_values());
    Ok(count)
}
