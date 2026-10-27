//! Native font measurements and paint for the shared Rust text layout.
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Shape, Vec2};
use neoflash_core::{ElementKind, TextFieldState, TextFormat, TextLayout, layout_text};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct CachedLayout {
    kind: ElementKind,
    family: FontFamily,
    ppp: u32,
    layout: Arc<TextLayout>,
    bytes: usize,
}
#[derive(Default)]
struct LayoutCache {
    entries: VecDeque<CachedLayout>,
    bytes: usize,
}
const CACHE_BYTES: usize = 8 * 1024 * 1024;

impl crate::app::Editor {
    pub(crate) fn text_box_properties(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        kind: &mut ElementKind,
    ) {
        let ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } = kind
        else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label("Text box");
            ui.label("Width");
            ui.add(egui::DragValue::new(width).range(0.0..=10000.));
            ui.label("Height");
            ui.add(egui::DragValue::new(height).range(0.0..=10000.));
            ui.checkbox(multiline, "Multiline");
            ui.checkbox(word_wrap, "Word wrap");
        });
        ui.add(
            egui::TextEdit::multiline(text)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("text_box_font")
                .selected_text(&format.font)
                .width(180.)
                .show_ui(ui, |ui| {
                    ui.text_edit_singleline(&mut self.font_filter);
                    for family in &self.fonts.families {
                        if self.font_filter.is_empty()
                            || family
                                .to_lowercase()
                                .contains(&self.font_filter.to_lowercase())
                        {
                            ui.selectable_value(&mut format.font, family.clone(), family);
                        }
                    }
                });
            ui.label("Size");
            let size_changed = ui
                .add(egui::DragValue::new(&mut format.size).range(0.1..=10000.))
                .changed();
            ui.checkbox(&mut format.bold, "Bold");
            ui.checkbox(&mut format.italic, "Italic");
            let mut rgb = [
                (format.color >> 16) as u8,
                (format.color >> 8) as u8,
                format.color as u8,
            ];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                format.color = u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
            }
            ui.label("Leading");
            let min_leading = (-format.size * 1.2 + 0.1).max(-10000.);
            if size_changed && format.leading < min_leading {
                format.leading = min_leading;
            }
            ui.add(egui::DragValue::new(&mut format.leading).range(min_leading..=10000.));
            egui::ComboBox::from_id_salt("text_box_align")
                .selected_text(format!("{:?}", format.align))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (neoflash_core::TextAlign::Left, "Left"),
                        (neoflash_core::TextAlign::Center, "Center"),
                        (neoflash_core::TextAlign::Right, "Right"),
                    ] {
                        ui.selectable_value(&mut format.align, value, label);
                    }
                });
        });
        if let Err(error) = self
            .fonts
            .load_style(ctx, &format.font, format.bold, format.italic)
        {
            ui.colored_label(crate::theme::UiPalette::current(ctx).error_text, error);
        }
    }
}

fn family(p: &egui::Painter, format: &TextFormat) -> FontFamily {
    let named = crate::fonts::styled_family(&format.font, format.bold, format.italic);
    if p.ctx().fonts(|fonts| fonts.families().contains(&named)) {
        named
    } else {
        FontFamily::Proportional
    }
}

pub(crate) fn layout(p: &egui::Painter, kind: &ElementKind) -> Result<Arc<TextLayout>, String> {
    let ElementKind::TextBox { format, .. } = kind else {
        return Err("Expected text box".into());
    };
    let family = family(p, format);
    let ppp = p.ctx().pixels_per_point().to_bits();
    let cache = p.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<LayoutCache>>>(egui::Id::new(
            "native-text-layouts",
        ))
        .clone()
    });
    {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(index) = cache
            .entries
            .iter()
            .position(|entry| entry.kind == *kind && entry.family == family && entry.ppp == ppp)
        {
            let entry = cache.entries.remove(index).expect("cached layout index");
            let result = entry.layout.clone();
            cache.entries.push_back(entry);
            return Ok(result);
        }
    }
    let field = TextFieldState::from_kind(kind).ok_or("Expected text box")?;
    let result = Arc::new(
        layout_text(&field, |segments, format| {
            Ok(segments
                .iter()
                .map(|text| {
                    p.layout_no_wrap(
                        text.clone(),
                        FontId::new(format.size as f32, family.clone()),
                        Color32::WHITE,
                    )
                    .size()
                    .x as f64
                })
                .collect())
        })
        .map_err(|error| error.to_string())?,
    );
    let bytes = field.text.len()
        + result
            .lines
            .iter()
            .map(|line| line.text.len() + 80)
            .sum::<usize>();
    if bytes <= CACHE_BYTES {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        while cache.entries.len() >= 32 || cache.bytes + bytes > CACHE_BYTES {
            let Some(entry) = cache.entries.pop_front() else {
                break;
            };
            cache.bytes -= entry.bytes;
        }
        cache.bytes += bytes;
        cache.entries.push_back(CachedLayout {
            kind: kind.clone(),
            family,
            ppp,
            layout: result.clone(),
            bytes,
        });
    }
    Ok(result)
}

pub(crate) fn paint(
    p: &egui::Painter,
    kind: &ElementKind,
    color: Color32,
    screen: impl Fn(Pos2) -> Pos2,
) -> Result<(), String> {
    let ElementKind::TextBox {
        format,
        width,
        height,
        ..
    } = kind
    else {
        return Err("Expected text box".into());
    };
    let result = layout(p, kind)?;
    let family = family(p, format);
    let clip = Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32));
    for line in &result.lines {
        if line.y >= clip.max.y as f64 || line.y + line.height <= 0. {
            continue;
        }
        let galley = p.layout_no_wrap(
            line.text.clone(),
            FontId::new(format.size as f32, family.clone()),
            color,
        );
        let shape = Shape::Text(egui::epaint::TextShape::new(
            Pos2::new(line.x as f32, line.y as f32),
            galley,
            color,
        ));
        for primitive in p.ctx().tessellate(
            vec![egui::epaint::ClippedShape {
                clip_rect: Rect::EVERYTHING,
                shape,
            }],
            p.ctx().pixels_per_point(),
        ) {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                let mut mesh = clip_mesh(mesh, clip);
                for vertex in &mut mesh.vertices {
                    vertex.pos = screen(vertex.pos);
                }
                p.add(Shape::mesh(mesh));
            }
        }
    }
    Ok(())
}

// Clip in field coordinates before the affine transform, including rotated boxes.
fn clip_mesh(mesh: egui::Mesh, rect: Rect) -> egui::Mesh {
    let mut output = egui::Mesh::with_texture(mesh.texture_id);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let mut polygon: Vec<_> = triangle
            .iter()
            .map(|index| mesh.vertices[*index as usize])
            .collect();
        for (axis, edge, lower) in [
            (0, rect.min.x, true),
            (0, rect.max.x, false),
            (1, rect.min.y, true),
            (1, rect.max.y, false),
        ] {
            if polygon.is_empty() {
                break;
            }
            let old = std::mem::take(&mut polygon);
            let inside = |vertex: &egui::epaint::Vertex| {
                if lower {
                    vertex.pos[axis] >= edge
                } else {
                    vertex.pos[axis] <= edge
                }
            };
            let mut previous = *old.last().expect("nonempty polygon");
            for current in old {
                if inside(&previous) != inside(&current) {
                    let t = (edge - previous.pos[axis]) / (current.pos[axis] - previous.pos[axis]);
                    let a = previous.color.to_array();
                    let b = current.color.to_array();
                    let c = std::array::from_fn::<_, 4, _>(|i| {
                        (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8
                    });
                    polygon.push(egui::epaint::Vertex {
                        pos: previous.pos.lerp(current.pos, t),
                        uv: previous.uv.lerp(current.uv, t),
                        color: Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]),
                    });
                }
                if inside(&current) {
                    polygon.push(current);
                }
                previous = current;
            }
        }
        if polygon.len() >= 3 {
            let start = output.vertices.len() as u32;
            for index in 1..polygon.len() as u32 - 1 {
                output
                    .indices
                    .extend_from_slice(&[start, start + index, start + index + 1]);
            }
            output.vertices.extend(polygon);
        }
    }
    output
}
