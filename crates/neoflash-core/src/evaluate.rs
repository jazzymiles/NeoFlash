use crate::*;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_DEPTH: usize = 64;
pub(crate) const MAX_NODES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawItem {
    pub element_id: Id,
    pub name: String,
    /// Root is []; this path includes the leaf itself.
    pub instance_path: Vec<Id>,
    pub layer_id: Id,
    /// Canvas-compatible affine matrix [a,b,c,d,tx,ty].
    pub transform: [f64; 6],
    pub alpha: f64,
    pub kind: ElementKind,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub path: Vec<Id>,
    pub name: String,
    pub symbol_id: Option<Id>,
    pub frame: u32,
    pub independent: bool,
    pub scripts: Vec<ObjectScript>,
    pub transform: Transform,
    pub accessibility: Option<Accessibility>,
    pub world: [f64; 6],
    pub alpha: f64,
    /// Local script property; inherited visibility must not change readback.
    pub visible: bool,
    pub effective_visible: bool,
    pub kind: Option<ElementKind>,
}

#[derive(Default)]
pub(crate) struct Evaluation {
    pub items: Vec<DrawItem>,
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DisplayPatch {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub scale_x: Option<f64>,
    pub scale_y: Option<f64>,
    pub rotation: Option<f64>,
    pub alpha: Option<f64>,
    pub visible: Option<bool>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub text_color: Option<u32>,
}

impl DisplayPatch {
    pub fn apply(&self, value: &mut Transform) {
        if let Some(v) = self.x {
            value.x = v;
        }
        if let Some(v) = self.y {
            value.y = v;
        }
        if let Some(v) = self.scale_x {
            value.scale_x = v;
        }
        if let Some(v) = self.scale_y {
            value.scale_y = v;
        }
        if let Some(v) = self.rotation {
            value.rotation = v;
        }
        if let Some(v) = self.alpha {
            value.alpha = v;
        }
    }
}

pub fn affine_multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

pub fn transform_matrix(t: Transform) -> [f64; 6] {
    let (s, c) = t.rotation.to_radians().sin_cos();
    [
        c * t.scale_x,
        s * t.scale_x,
        -s * t.scale_y,
        c * t.scale_y,
        t.x,
        t.y,
    ]
}

/// Evaluate a CSS-style timing function; cubic Bézier solves x before reading y.
pub fn easing(t: f64, ease: Easing) -> f64 {
    let t = t.clamp(0., 1.);
    match ease {
        Easing::Linear => t,
        Easing::EaseIn => t * t,
        Easing::EaseOut => 1. - (1. - t) * (1. - t),
        Easing::EaseInOut => {
            if t < 0.5 {
                2. * t * t
            } else {
                1. - (-2. * t + 2.).powi(2) / 2.
            }
        }
        Easing::CubicBezier { x1, y1, x2, y2 } => {
            if t == 0. || t == 1. {
                return t;
            }
            let bez = |u: f64, p1: f64, p2: f64| {
                3. * (1. - u).powi(2) * u * p1 + 3. * (1. - u) * u * u * p2 + u.powi(3)
            };
            let (mut low, mut high) = (0., 1.);
            for _ in 0..40 {
                let mid = (low + high) / 2.;
                if bez(mid, x1, x2) < t {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            bez((low + high) / 2., y1, y2)
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn color_lerp(a: Color, b: Color, t: f64) -> Color {
    std::array::from_fn(|i| lerp(a[i] as f64, b[i] as f64, t).clamp(0., 255.).round() as u8)
}

fn interpolate(a: &Element, b: &Element, t: f64) -> Element {
    let mut e = a.clone();
    let (x, y) = (a.transform, b.transform);
    e.transform = Transform {
        x: lerp(x.x, y.x, t),
        y: lerp(x.y, y.y, t),
        scale_x: lerp(x.scale_x, y.scale_x, t),
        scale_y: lerp(x.scale_y, y.scale_y, t),
        rotation: lerp(x.rotation, y.rotation, t),
        alpha: lerp(x.alpha, y.alpha, t),
    };
    match (&a.kind, &b.kind, &mut e.kind) {
        (
            ElementKind::Rectangle {
                width: aw,
                height: ah,
                fill: af,
                stroke: as_,
                stroke_width: az,
            },
            ElementKind::Rectangle {
                width: bw,
                height: bh,
                fill: bf,
                stroke: bs,
                stroke_width: bz,
            },
            ElementKind::Rectangle {
                width,
                height,
                fill,
                stroke,
                stroke_width,
            },
        )
        | (
            ElementKind::Ellipse {
                width: aw,
                height: ah,
                fill: af,
                stroke: as_,
                stroke_width: az,
            },
            ElementKind::Ellipse {
                width: bw,
                height: bh,
                fill: bf,
                stroke: bs,
                stroke_width: bz,
            },
            ElementKind::Ellipse {
                width,
                height,
                fill,
                stroke,
                stroke_width,
            },
        ) => {
            *width = lerp(*aw, *bw, t);
            *height = lerp(*ah, *bh, t);
            *fill = color_lerp(*af, *bf, t);
            *stroke = color_lerp(*as_, *bs, t);
            *stroke_width = lerp(*az, *bz, t);
        }
        (
            ElementKind::Text {
                font_size: a,
                color: ac,
                ..
            },
            ElementKind::Text {
                font_size: b,
                color: bc,
                ..
            },
            ElementKind::Text {
                font_size, color, ..
            },
        ) => {
            *font_size = lerp(*a, *b, t);
            *color = color_lerp(*ac, *bc, t);
        }
        (
            ElementKind::Path {
                commands: a,
                fill_rule: ar,
                fill: af,
                stroke: as_,
                stroke_width: aw,
            },
            ElementKind::Path {
                commands: b,
                fill_rule: br,
                fill: bf,
                stroke: bs,
                stroke_width: bw,
            },
            ElementKind::Path {
                commands,
                fill,
                stroke,
                stroke_width,
                ..
            },
        ) if ar == br && paths_have_same_topology(a, b) => {
            *commands = crate::geometry::interpolate_commands(a, b, t);
            *fill = color_lerp(*af, *bf, t);
            *stroke = color_lerp(*as_, *bs, t);
            *stroke_width = lerp(*aw, *bw, t).max(0.);
        }
        _ => {}
    }
    e
}

pub fn sample_layer(layer: &Layer, frame: u32) -> Vec<Element> {
    let Some(key) = layer.key_at(frame) else {
        return vec![];
    };
    let Some(ease) = key.tween else {
        return key.elements.clone();
    };
    let Some(next) = layer
        .keyframes
        .iter()
        .filter(|k| k.frame > key.frame)
        .min_by_key(|k| k.frame)
    else {
        return key.elements.clone();
    };
    let t = easing(
        (frame - key.frame) as f64 / (next.frame - key.frame) as f64,
        ease,
    );
    key.elements
        .iter()
        .map(|e| {
            next.elements
                .iter()
                .find(|n| n.id == e.id)
                .map_or_else(|| e.clone(), |n| interpolate(e, n, t))
        })
        .collect()
}

/// Earliest key in the current continuous placement span (stable IDs survive keys).
fn placement_start(layer: &Layer, frame: u32, id: &str) -> u32 {
    let mut keys: Vec<_> = layer
        .keyframes
        .iter()
        .filter(|k| k.frame <= frame)
        .collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.frame));
    let mut start = frame;
    for key in keys {
        if !key.elements.iter().any(|e| e.id == id) {
            break;
        }
        start = key.frame;
    }
    start
}

pub fn evaluate(project: &Project, symbol_id: &str, frame: u32) -> Vec<DrawItem> {
    snapshot(project, symbol_id, frame, &|_| None, &|_| None).items
}

pub(crate) fn snapshot(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    clock: &dyn Fn(&[Id]) -> Option<u32>,
    patch: &dyn Fn(&[Id]) -> Option<DisplayPatch>,
) -> Evaluation {
    let mut result = Evaluation::default();
    let Some(root) = project.symbol(symbol_id) else {
        return result;
    };
    let frame = frame.min(root.timeline.frame_count.saturating_sub(1));
    let root_patch = patch(&[]).unwrap_or_default();
    let visible = root_patch.visible.unwrap_or(true);
    let mut transform = Transform::default();
    root_patch.apply(&mut transform);
    let world = transform_matrix(transform);
    result.nodes.push(Node {
        path: vec![],
        name: root_patch.name.unwrap_or_else(|| root.name.clone()),
        symbol_id: Some(root.id.clone()),
        frame,
        independent: true,
        scripts: vec![],
        transform,
        accessibility: None,
        world,
        alpha: transform.alpha,
        visible,
        effective_visible: visible,
        kind: None,
    });
    let mut walker = Walker {
        project,
        clock,
        patch,
        result: &mut result,
    };
    walker.walk(
        root,
        frame,
        &[],
        InheritedDisplay {
            matrix: world,
            alpha: transform.alpha,
            visible,
        },
        0,
    );
    result
}

struct Walker<'a> {
    project: &'a Project,
    clock: &'a dyn Fn(&[Id]) -> Option<u32>,
    patch: &'a dyn Fn(&[Id]) -> Option<DisplayPatch>,
    result: &'a mut Evaluation,
}

#[derive(Clone, Copy)]
struct InheritedDisplay {
    matrix: [f64; 6],
    alpha: f64,
    visible: bool,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        symbol: &Symbol,
        frame: u32,
        parent: &[Id],
        inherited: InheritedDisplay,
        depth: usize,
    ) {
        if depth >= MAX_DEPTH || self.result.nodes.len() >= MAX_NODES {
            return;
        }
        // Layer zero is the backmost layer; list order is painter order everywhere.
        for layer in &symbol.timeline.layers {
            if !layer.visible {
                continue;
            }
            for mut e in sample_layer(layer, frame) {
                if self.result.nodes.len() >= MAX_NODES {
                    return;
                }
                let mut path = parent.to_vec();
                path.push(e.id.clone());
                let patch = (self.patch)(&path);
                let visible = patch.as_ref().and_then(|p| p.visible).unwrap_or(true);
                if let Some(patch) = patch {
                    patch.apply(&mut e.transform);
                    if let ElementKind::Text { text, color, .. } = &mut e.kind {
                        if let Some(value) = patch.text {
                            *text = value;
                        }
                        if let Some(value) = patch.text_color {
                            color[0] = (value >> 16) as u8;
                            color[1] = (value >> 8) as u8;
                            color[2] = value as u8;
                        }
                    }
                    if let Some(name) = patch.name {
                        e.name = name;
                    }
                }
                let world = affine_multiply(inherited.matrix, transform_matrix(e.transform));
                let opacity = inherited.alpha * e.transform.alpha.clamp(0., 1.);
                let effective_visible = inherited.visible && visible;
                let mut node = Node {
                    path: path.clone(),
                    name: e.name.clone(),
                    symbol_id: None,
                    frame: 0,
                    independent: false,
                    scripts: e.scripts.clone(),
                    transform: e.transform,
                    accessibility: e.accessibility.clone(),
                    world,
                    alpha: opacity,
                    visible,
                    effective_visible,
                    kind: None,
                };
                if let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &e.kind
                {
                    let Some(child) = self.project.symbol(symbol_id) else {
                        continue;
                    };
                    let count = child.timeline.frame_count.max(1);
                    let age = frame.saturating_sub(placement_start(layer, frame, &e.id));
                    let child_frame = match playback {
                        InstancePlayback::Independent => {
                            (self.clock)(&path).unwrap_or(age % count) % count
                        }
                        InstancePlayback::Loop => age % count,
                        InstancePlayback::PlayOnce => age.min(count - 1),
                        InstancePlayback::SingleFrame(f) => (*f).min(count - 1),
                    };
                    node.symbol_id = Some(child.id.clone());
                    node.frame = child_frame;
                    node.independent = matches!(playback, InstancePlayback::Independent);
                    self.result.nodes.push(node);
                    // Invisible clips still exist and advance their timelines.
                    self.walk(
                        child,
                        child_frame,
                        &path,
                        InheritedDisplay {
                            matrix: world,
                            alpha: opacity,
                            visible: effective_visible,
                        },
                        depth + 1,
                    );
                } else {
                    node.kind = Some(e.kind.clone());
                    self.result.nodes.push(node);
                    if effective_visible {
                        self.result.items.push(DrawItem {
                            element_id: e.id,
                            name: e.name,
                            instance_path: path,
                            layer_id: layer.id.clone(),
                            transform: world,
                            alpha: opacity,
                            kind: e.kind,
                        });
                    }
                }
            }
        }
    }
}
