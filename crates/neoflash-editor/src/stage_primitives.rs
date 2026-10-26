//! Rectangle/ellipse geometry is stroked locally before its complete affine
//! transform is applied. A screen-width stroke cannot represent scaled or
//! nested nonuniform placements. Onion outlines are intentionally screen-space.

use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
    math::{Point, point},
    path::Path,
};
use neoflash_core::{DrawItem, ElementKind};

const MAX_ELLIPSE_SEGMENTS: usize = 4096;

#[derive(Clone, Copy)]
struct Primitive {
    width: f64,
    height: f64,
    stroke_width: f64,
    ellipse: bool,
}

impl Primitive {
    fn from_kind(kind: &ElementKind) -> Option<Self> {
        match kind {
            ElementKind::Rectangle {
                width,
                height,
                stroke_width,
                ..
            }
            | ElementKind::Ellipse {
                width,
                height,
                stroke_width,
                ..
            } => Some(Self {
                width: *width,
                height: *height,
                stroke_width: *stroke_width,
                ellipse: matches!(kind, ElementKind::Ellipse { .. }),
            }),
            _ => None,
        }
    }

    fn valid(self) -> bool {
        [self.width, self.height, self.stroke_width]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1e9).contains(v))
    }

    fn has_area(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

fn view_scale(matrix: [f64; 6], origin: Pos2, zoom: f32) -> Option<f64> {
    if !matrix.iter().all(|v| v.is_finite())
        || !origin.is_finite()
        || !zoom.is_finite()
        || zoom <= 0.0
    {
        return None;
    }
    // Frobenius norm bounds the largest singular value, including sheared
    // matrices produced by nested, differently rotated nonuniform placements.
    let scale = matrix[0].hypot(matrix[1]).hypot(matrix[2].hypot(matrix[3])) * zoom as f64;
    scale.is_finite().then_some(scale)
}

fn screen_point(local: Point, matrix: [f64; 6], origin: Pos2, zoom: f32) -> Option<Pos2> {
    let x = (matrix[0] * local.x as f64 + matrix[2] * local.y as f64 + matrix[4]) * zoom as f64
        + origin.x as f64;
    let y = (matrix[1] * local.x as f64 + matrix[3] * local.y as f64 + matrix[5]) * zoom as f64
        + origin.y as f64;
    let result = Pos2::new(x as f32, y as f32);
    result.is_finite().then_some(result)
}

/// Bound the transformed ellipse's sagitta, including the outer stroke radius.
/// At extreme zoom/size the segment cap bounds authoring cost; it does not
/// promise subpixel error for arbitrarily large offscreen ellipses.
fn ellipse_segments(primitive: Primitive, scale: f64, screen_tolerance: f64) -> usize {
    let radius = (primitive.width.max(primitive.height) + primitive.stroke_width) * 0.5;
    let screen_radius = radius * scale;
    if screen_radius <= screen_tolerance || screen_radius == 0.0 {
        return 16;
    }
    let half_angle = (1.0 - screen_tolerance / screen_radius)
        .clamp(-1.0, 1.0)
        .acos();
    let count = if half_angle > 0.0 {
        (std::f64::consts::PI / half_angle).ceil()
    } else {
        MAX_ELLIPSE_SEGMENTS as f64
    };
    let count = count.clamp(16.0, MAX_ELLIPSE_SEGMENTS as f64) as usize;
    count.div_ceil(4) * 4
}

fn contour(primitive: Primitive, scale: f64, screen_tolerance: f64) -> Vec<Point> {
    if primitive.width == 0.0 && primitive.height == 0.0 {
        return Vec::new();
    }
    if !primitive.has_area() {
        // A zero-radius ellipse or zero-width rectangle reduces to a line.
        // Keep its authored butt-capped stroke; it has no filled interior.
        return vec![
            point(0.0, 0.0),
            point(primitive.width as f32, primitive.height as f32),
        ];
    }
    if !primitive.ellipse {
        return vec![
            point(0.0, 0.0),
            point(primitive.width as f32, 0.0),
            point(primitive.width as f32, primitive.height as f32),
            point(0.0, primitive.height as f32),
        ];
    }
    let count = ellipse_segments(primitive, scale, screen_tolerance);
    (0..count)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / count as f64;
            point(
                (primitive.width * 0.5 * (1.0 + angle.cos())) as f32,
                (primitive.height * 0.5 * (1.0 + angle.sin())) as f32,
            )
        })
        .collect()
}

fn tessellate(
    points: &[Point],
    closed: bool,
    stroke: Option<f32>,
    tolerance: f32,
    ellipse: bool,
) -> Result<VertexBuffers<Point, u32>, String> {
    let mut mesh = VertexBuffers::new();
    if points.len() < 2 || stroke.is_some_and(|width| width <= 0.0) {
        return Ok(mesh);
    }
    let mut builder = Path::builder();
    builder.begin(points[0]);
    for p in &points[1..] {
        builder.line_to(*p);
    }
    builder.end(closed);
    let path = builder.build();
    if let Some(width) = stroke {
        StrokeTessellator::new()
            .tessellate_path(
                &path,
                &StrokeOptions::default()
                    .with_line_width(width)
                    .with_line_cap(LineCap::Butt)
                    // Rectangles match Canvas's default joins/miter limit. The
                    // smooth ellipse is sampled as a polygon; round joins avoid
                    // artificial spikes between its short approximation segments.
                    .with_line_join(if ellipse {
                        LineJoin::Round
                    } else {
                        LineJoin::Miter
                    })
                    .with_miter_limit(10.0)
                    .with_tolerance(tolerance),
                &mut BuffersBuilder::new(&mut mesh, |vertex: StrokeVertex| vertex.position()),
            )
            .map_err(|error| format!("Primitive stroke tessellation: {error:?}"))?;
    } else if closed {
        FillTessellator::new()
            .tessellate_path(
                &path,
                &FillOptions::default().with_tolerance(tolerance),
                &mut BuffersBuilder::new(&mut mesh, |vertex: FillVertex| vertex.position()),
            )
            .map_err(|error| format!("Primitive fill tessellation: {error:?}"))?;
    }
    Ok(mesh)
}

fn transformed_mesh(
    geometry: VertexBuffers<Point, u32>,
    matrix: [f64; 6],
    origin: Pos2,
    zoom: f32,
    color: Color32,
) -> Option<egui::epaint::Mesh> {
    let mut mesh = egui::epaint::Mesh::default();
    for vertex in geometry.vertices {
        mesh.colored_vertex(screen_point(vertex, matrix, origin, zoom)?, color);
    }
    mesh.indices = geometry.indices;
    Some(mesh)
}

fn outline_shape(
    points: &[Point],
    closed: bool,
    matrix: [f64; 6],
    origin: Pos2,
    zoom: f32,
    color: Color32,
) -> Option<Shape> {
    let points = points
        .iter()
        .map(|point| screen_point(*point, matrix, origin, zoom))
        .collect::<Option<Vec<_>>>()?;
    let stroke = Stroke::new(1.0_f32, color);
    Some(if closed {
        Shape::closed_line(points, stroke)
    } else {
        Shape::line(points, stroke)
    })
}

/// `fill` and `stroke_color` already include opacity/onion tint. Normal strokes
/// use authored local width; onion outlines use one logical screen point,
/// independently of document scale, nesting, zoom, or reflection.
pub(crate) fn paint_primitive(
    p: &egui::Painter,
    item: &DrawItem,
    origin: Pos2,
    zoom: f32,
    fill: Color32,
    stroke_color: Color32,
    outlines: bool,
) {
    let Some(primitive) = Primitive::from_kind(&item.kind) else {
        return;
    };
    let Some(scale) = view_scale(item.transform, origin, zoom) else {
        return;
    };
    if !primitive.valid() {
        return;
    }
    let screen_tolerance = 0.25 / p.ctx().pixels_per_point().max(0.1) as f64;
    let points = contour(primitive, scale, screen_tolerance);
    if points.len() < 2 {
        return;
    }
    let closed = primitive.has_area();
    if outlines {
        if stroke_color.a() > 0
            && let Some(shape) =
                outline_shape(&points, closed, item.transform, origin, zoom, stroke_color)
        {
            p.add(shape);
        }
        return;
    }
    let local_tolerance = (screen_tolerance / scale.max(1e-12)).clamp(0.0001, 1.0) as f32;
    for (color, stroke) in [
        (fill, None),
        (stroke_color, Some(primitive.stroke_width as f32)),
    ] {
        if color.a() == 0
            || (stroke.is_none() && !closed)
            || stroke.is_some_and(|width| width <= 0.0)
        {
            continue;
        }
        match tessellate(&points, closed, stroke, local_tolerance, primitive.ellipse) {
            Ok(geometry) => {
                if let Some(mesh) = transformed_mesh(geometry, item.transform, origin, zoom, color)
                {
                    p.add(Shape::mesh(mesh));
                }
            }
            Err(message) => {
                p.text(
                    origin,
                    egui::Align2::LEFT_TOP,
                    message,
                    egui::FontId::monospace(12.0),
                    Color32::DARK_RED,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Transform, affine_multiply, transform_matrix};

    fn area(mesh: &egui::epaint::Mesh) -> f64 {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let a = mesh.vertices[triangle[0] as usize].pos;
                let b = mesh.vertices[triangle[1] as usize].pos;
                let c = mesh.vertices[triangle[2] as usize].pos;
                (((b.x - a.x) as f64 * (c.y - a.y) as f64)
                    - ((b.y - a.y) as f64 * (c.x - a.x) as f64))
                    .abs()
                    * 0.5
            })
            .sum()
    }

    fn bounds(mesh: &egui::epaint::Mesh) -> egui::Rect {
        mesh.vertices
            .iter()
            .fold(egui::Rect::NOTHING, |rect, vertex| {
                rect.union(egui::Rect::from_min_max(vertex.pos, vertex.pos))
            })
    }

    fn close(a: f64, b: f64, tolerance: f64) {
        assert!(
            (a - b).abs() <= tolerance,
            "{a} != {b}; tolerance {tolerance}"
        );
    }

    fn matrix() -> [f64; 6] {
        affine_multiply(
            transform_matrix(Transform {
                x: 125.,
                y: -30.,
                scale_x: -2.,
                scale_y: 0.7,
                rotation: 37.,
                ..Default::default()
            }),
            transform_matrix(Transform {
                x: 18.,
                y: 41.,
                scale_x: 1.3,
                scale_y: 0.9,
                rotation: -22.,
                ..Default::default()
            }),
        )
    }

    fn mesh(primitive: Primitive, matrix: [f64; 6], zoom: f32, stroke: bool) -> egui::epaint::Mesh {
        let scale = view_scale(matrix, Pos2::ZERO, zoom).unwrap();
        let points = contour(primitive, scale, 0.005);
        let geometry = tessellate(
            &points,
            primitive.has_area(),
            stroke.then_some(primitive.stroke_width as f32),
            0.001,
            primitive.ellipse,
        )
        .unwrap();
        transformed_mesh(geometry, matrix, Pos2::new(25., -10.), zoom, Color32::WHITE).unwrap()
    }

    #[test]
    fn rectangle_stroke_is_locally_mitered_before_nested_affine() {
        let primitive = Primitive {
            width: 80.,
            height: 40.,
            stroke_width: 8.,
            ellipse: false,
        };
        let matrix = matrix();
        let zoom = 1.7;
        let fill = mesh(primitive, matrix, zoom, false);
        let stroke = mesh(primitive, matrix, zoom, true);
        let determinant =
            (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() * (zoom as f64).powi(2);
        close(
            area(&fill),
            primitive.width * primitive.height * determinant,
            0.02,
        );
        close(
            area(&stroke),
            2. * primitive.stroke_width * (primitive.width + primitive.height) * determinant,
            0.03,
        );
        let mut expected = egui::Rect::NOTHING;
        for local in [
            point(-4., -4.),
            point(84., -4.),
            point(84., 44.),
            point(-4., 44.),
        ] {
            let p = screen_point(local, matrix, Pos2::new(25., -10.), zoom).unwrap();
            expected.extend_with(p);
        }
        let actual = bounds(&stroke);
        close(actual.min.x as f64, expected.min.x as f64, 0.0001);
        close(actual.min.y as f64, expected.min.y as f64, 0.0001);
        close(actual.max.x as f64, expected.max.x as f64, 0.0001);
        close(actual.max.y as f64, expected.max.y as f64, 0.0001);
    }

    #[test]
    fn ellipse_stroke_bounds_follow_affine_minkowski_sum_and_area() {
        let primitive = Primitive {
            width: 80.,
            height: 40.,
            stroke_width: 4.,
            ellipse: true,
        };
        let matrix = matrix();
        let zoom = 1.3;
        let fill = mesh(primitive, matrix, zoom, false);
        let stroke = mesh(primitive, matrix, zoom, true);
        let determinant =
            (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() * (zoom as f64).powi(2);
        close(
            area(&fill),
            std::f64::consts::PI * 40. * 20. * determinant,
            2.0,
        );
        // Numerical integration is independent of the tessellation's polygon.
        let steps = 8192;
        let perimeter: f64 = (0..steps)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64 + 0.5) / steps as f64;
                (40. * a.sin()).hypot(20. * a.cos()) * std::f64::consts::TAU / steps as f64
            })
            .sum();
        close(
            area(&stroke),
            perimeter * primitive.stroke_width * determinant,
            1.0,
        );
        let center = screen_point(point(40., 20.), matrix, Pos2::new(25., -10.), zoom).unwrap();
        let rx = ((matrix[0] * 40.).hypot(matrix[2] * 20.) + 2. * matrix[0].hypot(matrix[2]))
            * zoom as f64;
        let ry = ((matrix[1] * 40.).hypot(matrix[3] * 20.) + 2. * matrix[1].hypot(matrix[3]))
            * zoom as f64;
        let actual = bounds(&stroke);
        close(actual.min.x as f64, center.x as f64 - rx, 0.04);
        close(actual.max.x as f64, center.x as f64 + rx, 0.04);
        close(actual.min.y as f64, center.y as f64 - ry, 0.04);
        close(actual.max.y as f64, center.y as f64 + ry, 0.04);
    }

    #[test]
    fn ellipse_detail_tracks_screen_size_and_has_a_cost_bound() {
        let primitive = Primitive {
            width: 80.,
            height: 40.,
            stroke_width: 4.,
            ellipse: true,
        };
        let small = ellipse_segments(primitive, 1., 0.25);
        let large = ellipse_segments(primitive, 20., 0.25);
        assert!(large > small);
        for scale in [1., 20.] {
            let count = ellipse_segments(primitive, scale, 0.25);
            let sagitta = 42. * scale * (1. - (std::f64::consts::PI / count as f64).cos());
            assert!(sagitta <= 0.25);
            assert_eq!(count % 4, 0);
        }
        assert_eq!(
            ellipse_segments(primitive, 1e100, 0.25),
            MAX_ELLIPSE_SEGMENTS
        );
    }

    #[test]
    fn onion_outline_width_is_one_screen_point_after_affine() {
        let primitive = Primitive {
            width: 80.,
            height: 40.,
            stroke_width: 30.,
            ellipse: false,
        };
        let points = contour(primitive, 1., 0.25);
        for zoom in [0.25, 8.] {
            let shape =
                outline_shape(&points, true, matrix(), Pos2::ZERO, zoom, Color32::WHITE).unwrap();
            let Shape::Path(path) = shape else {
                panic!("expected screen-space path")
            };
            assert_eq!(path.stroke.width, 1.);
            assert!(path.closed);
            assert!(path.points.iter().all(|p| p.is_finite()));
            let expected = screen_point(points[1], matrix(), Pos2::ZERO, zoom).unwrap();
            assert_eq!(path.points[1], expected);
        }
    }

    #[test]
    fn zero_dimensions_have_no_fill_but_finite_butt_strokes() {
        for ellipse in [false, true] {
            let line = Primitive {
                width: 0.,
                height: 40.,
                stroke_width: 6.,
                ellipse,
            };
            let fill = mesh(line, matrix(), 1., false);
            let stroke = mesh(line, matrix(), 1., true);
            assert!(fill.indices.is_empty());
            assert!(stroke.vertices.iter().all(|v| v.pos.is_finite()));
            let determinant = (matrix()[0] * matrix()[3] - matrix()[1] * matrix()[2]).abs();
            close(area(&stroke), 40. * 6. * determinant, 0.005);
            let point = Primitive { height: 0., ..line };
            assert!(contour(point, 1., 0.25).is_empty());
            assert!(mesh(point, matrix(), 1., true).indices.is_empty());
        }
    }

    #[test]
    fn invalid_or_unrepresentable_coordinates_never_enter_meshes() {
        let primitive = Primitive {
            width: f64::NAN,
            height: 40.,
            stroke_width: 6.,
            ellipse: false,
        };
        assert!(!primitive.valid());
        assert!(
            !Primitive {
                width: -1.,
                ..primitive
            }
            .valid()
        );
        assert!(view_scale([f64::INFINITY; 6], Pos2::ZERO, 1.).is_none());
        assert!(view_scale(matrix(), Pos2::ZERO, f32::NAN).is_none());
        assert!(view_scale(matrix(), Pos2::ZERO, 0.).is_none());
        assert!(screen_point(point(1., 1.), [1., 0., 0., 1., 1e100, 0.], Pos2::ZERO, 1.).is_none());
        let collapsed = [0., 0., 0., 0., 20., 30.];
        let mesh = mesh(
            Primitive {
                width: 20.,
                height: 30.,
                stroke_width: 2.,
                ellipse: true,
            },
            collapsed,
            1.,
            true,
        );
        assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
        close(area(&mesh), 0., 0.);
    }

    #[test]
    fn painter_emits_transformed_fill_stroke_and_screen_space_onion() {
        let context = egui::Context::default();
        let item = DrawItem {
            element_id: "primitive".into(),
            name: "Primitive".into(),
            instance_path: vec!["nested".into(), "primitive".into()],
            layer_id: "layer".into(),
            transform: matrix(),
            alpha: 1.,
            kind: ElementKind::Rectangle {
                width: 80.,
                height: 40.,
                fill: [0, 255, 0, 255],
                stroke: [0, 0, 255, 255],
                stroke_width: 8.,
            },
        };
        let paint = |outlines| {
            context.run(egui::RawInput::default(), |context| {
                let painter = context.layer_painter(egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::new("primitive-test"),
                ));
                paint_primitive(
                    &painter,
                    &item,
                    Pos2::ZERO,
                    2.,
                    Color32::GREEN,
                    Color32::BLUE,
                    outlines,
                );
            })
        };
        let output = paint(false);
        assert_eq!(output.shapes.len(), 2);
        for (index, clipped) in output.shapes.iter().enumerate() {
            let Shape::Mesh(mesh) = &clipped.shape else {
                panic!("expected fill/stroke mesh")
            };
            assert!(!mesh.indices.is_empty());
            assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
            assert!(mesh.vertices.iter().all(|v| v.color
                == if index == 0 {
                    Color32::GREEN
                } else {
                    Color32::BLUE
                }));
        }
        let output = paint(true);
        assert_eq!(output.shapes.len(), 1);
        let Shape::Path(path) = &output.shapes[0].shape else {
            panic!("expected onion contour")
        };
        assert_eq!(path.stroke.width, 1.);
        assert_eq!(path.fill, Color32::TRANSPARENT);
    }
}
