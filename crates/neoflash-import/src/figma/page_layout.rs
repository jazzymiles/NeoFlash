//! Fits the full bounds of each selected Figma page on the shared project stage.
use super::{FigmaNode, Layer};
use neoflash_core::{PathBounds, affine_multiply};

const MAX_STAGE_SIDE: f64 = 8192.0;
const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn local_matrix(node: &FigmaNode) -> [f64; 6] {
    node.relative_transform
        .filter(|matrix| matrix.iter().all(|v| v.is_finite()))
        .unwrap_or_else(|| {
            let (sin, cos) = node.rotation_deg.to_radians().sin_cos();
            [cos, sin, -sin, cos, node.x, node.y]
        })
}

fn include_node(
    bounds: &mut Option<PathBounds>,
    node: &FigmaNode,
    parent: [f64; 6],
    hidden: bool,
    top: bool,
) {
    if !node.visible && !hidden {
        return;
    }
    let matrix = affine_multiply(parent, local_matrix(node));
    let visible_bounds = top
        || node.clips_content
        || node.children.is_empty()
        || !node.fills.is_empty()
        || !node.strokes.is_empty()
        || !node.effects.is_empty();
    if visible_bounds {
        include_box(bounds, matrix, node.width, node.height);
    }
    if !node.clips_content {
        for child in &node.children {
            include_node(bounds, child, matrix, hidden, false);
        }
    }
}

fn include_box(bounds: &mut Option<PathBounds>, matrix: [f64; 6], width: f64, height: f64) {
    let [a, b, c, d, x, y] = matrix;
    let w = width.max(0.0);
    let h = height.max(0.0);
    for (u, v) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
        let px = a * u + c * v + x;
        let py = b * u + d * v + y;
        if px.is_finite() && py.is_finite() {
            let bb = bounds.get_or_insert(PathBounds {
                x_min: px,
                x_max: px,
                y_min: py,
                y_max: py,
            });
            bb.x_min = bb.x_min.min(px);
            bb.y_min = bb.y_min.min(py);
            bb.x_max = bb.x_max.max(px);
            bb.y_max = bb.y_max.max(py);
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct PageLayout {
    pub origin: [f64; 2],
    pub width: f64,
    pub height: f64,
}

impl PageLayout {
    pub fn measure(page: &FigmaNode, hidden: bool) -> Self {
        let mut bounds = None;
        for child in &page.children {
            include_node(&mut bounds, child, IDENTITY, hidden, true);
        }
        bounds.map_or_else(Self::default, |bb| Self {
            origin: [bb.x_min, bb.y_min],
            width: bb.width(),
            height: bb.height(),
        })
    }

    pub fn scale(self) -> f64 {
        MAX_STAGE_SIDE / self.width.max(self.height).max(MAX_STAGE_SIDE)
    }

    pub fn stage_extent(self) -> (u32, u32) {
        let scale = self.scale();
        (
            (self.width * scale).ceil().min(MAX_STAGE_SIDE) as u32,
            (self.height * scale).ceil().min(MAX_STAGE_SIDE) as u32,
        )
    }

    /// A plain, stage-sized frame can expose its children directly on the scene timeline.
    pub fn fills_stage(self, node: &FigmaNode, stage: (u32, u32), resize: bool) -> bool {
        let [a, b, c, d, x, y] = local_matrix(node);
        let scale = if resize { self.scale() } else { 1.0 };
        (a - 1.0).abs() < 1e-6
            && (d - 1.0).abs() < 1e-6
            && b.abs() < 1e-6
            && c.abs() < 1e-6
            && (x - self.origin[0]).abs() < 1e-6
            && (y - self.origin[1]).abs() < 1e-6
            && (node.width * scale).ceil() as u32 == stage.0
            && (node.height * scale).ceil() as u32 == stage.1
    }

    pub fn fit_elements(self, layers: &mut [Layer]) {
        let scale = self.scale();
        if scale == 1.0 {
            return;
        }
        for element in layers
            .iter_mut()
            .flat_map(|layer| &mut layer.keyframes)
            .flat_map(|key| &mut key.elements)
        {
            element.transform.x *= scale;
            element.transform.y *= scale;
            element.transform.scale_x *= scale;
            element.transform.scale_y *= scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FigmaImportOptions, FigmaPageMapping, import_figma_json_str};
    use neoflash_core::transform_matrix;

    #[test]
    fn all_selected_pages_fit_the_stage_including_distant_and_rotated_artwork() {
        let json = r#"{"document":{"type":"DOCUMENT","children":[
            {"id":"small","name":"Card","type":"CANVAS","children":[
                {"id":"frame","name":"Small design","type":"FRAME",
                 "absoluteBoundingBox":{"x":100,"y":200,"width":800,"height":320},
                 "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]}]},
            {"id":"wide","name":"Board","type":"CANVAS","children":[
                {"id":"left","name":"Left","type":"RECTANGLE","size":{"x":20,"y":20},
                 "relativeTransform":[[1,0,-15000],[0,1,0]],
                 "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]},
                {"id":"far","name":"Far","type":"RECTANGLE","size":{"x":860,"y":100},
                 "relativeTransform":[[0,-1,10000],[1,0,-300]],
                 "fills":[{"type":"SOLID","color":{"r":0,"g":1,"b":0}}]}]}
        ]}}"#;
        for page_mapping in [FigmaPageMapping::Scenes, FigmaPageMapping::Layers] {
            let (project, report) = import_figma_json_str(
                json,
                "Board",
                &FigmaImportOptions {
                    page_mapping,
                    ..FigmaImportOptions::default()
                },
            )
            .unwrap();
            project.validate().unwrap();
            assert_eq!((project.width, project.height), (8192, 320));
            assert_eq!(
                report
                    .diagnostics
                    .iter()
                    .filter(|d| d.code == "figma-page-fitted-to-stage")
                    .count(),
                1
            );
            let scene = project.scene_ids().last().unwrap().clone();
            let symbol = project.symbol(&scene).unwrap();
            let far = symbol
                .timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .find(|e| e.name == "Far")
                .unwrap();
            let [a, b, c, d, x, y] = transform_matrix(far.transform);
            assert!((x - 8192.0).abs() < 1e-6 && y.abs() < 1e-6);
            assert!(a.abs() < 1e-6 && d.abs() < 1e-6);
            let scale = 8192.0 / 25000.0;
            assert!((b - scale).abs() < 1e-6 && (c + scale).abs() < 1e-6);
        }
    }
}
