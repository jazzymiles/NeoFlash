use crate::*;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_DEPTH: usize = 64;
pub(crate) const MAX_NODES: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawItem<K = ElementKind> {
    pub element_id: Id,
    pub name: String,
    /// Root is []; this path includes the leaf itself.
    pub instance_path: Vec<Id>,
    pub layer_id: Id,
    /// Canvas-compatible affine matrix [a,b,c,d,tx,ty].
    pub transform: [f64; 6],
    pub alpha: f64,
    #[serde(default, skip_serializing_if = "crate::ColorTransform::is_identity")]
    pub color_transform: crate::ColorTransform,
    pub kind: K,
}

impl<A, B> PartialEq<DrawItem<B>> for DrawItem<A>
where
    A: PartialEq<B>,
{
    fn eq(&self, other: &DrawItem<B>) -> bool {
        self.element_id == other.element_id
            && self.name == other.name
            && self.instance_path == other.instance_path
            && self.layer_id == other.layer_id
            && self.transform == other.transform
            && self.alpha == other.alpha
            && self.color_transform == other.color_transform
            && self.kind == other.kind
    }
}
impl<K> DrawItem<K> {
    pub fn map_kind<T>(self, map: impl FnOnce(K) -> T) -> DrawItem<T> {
        DrawItem {
            element_id: self.element_id,
            name: self.name,
            instance_path: self.instance_path,
            layer_id: self.layer_id,
            transform: self.transform,
            alpha: self.alpha,
            color_transform: self.color_transform,
            kind: map(self.kind),
        }
    }
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
    pub masks: Vec<crate::authored_layers::LayerMaskPlan>,
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
    pub button_frame: Option<u32>,
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
    let (sy, cy) = (t.rotation + t.skew).to_radians().sin_cos();
    [
        c * t.scale_x,
        s * t.scale_x,
        -sy * t.scale_y,
        cy * t.scale_y,
        t.x,
        t.y,
    ]
}

/// Evaluate a CSS-style timing function; cubic Bézier solves x before reading y.
pub fn easing(t: f64, ease: Easing) -> f64 {
    let t = t.clamp(0., 1.);
    match ease {
        Easing::Linear => t,
        Easing::Flash { acceleration } => {
            let amount = (acceleration / 100.).clamp(-1., 1.);
            if amount < 0. {
                t + (-amount) * (t * t - t)
            } else {
                t + amount * ((1. - (1. - t).powi(2)) - t)
            }
        }
        Easing::EaseIn => t * t,
        Easing::EaseOut => 1. - (1. - t) * (1. - t),
        Easing::EaseInOut => {
            if t < 0.5 {
                2. * t * t
            } else {
                1. - (-2. * t + 2.).powi(2) / 2.
            }
        }
        Easing::Bounce => {
            crate::as_easing::ease("Bounce", "easeOut", t, 0.0, 1.0, 1.0, f64::NAN, f64::NAN)
                .unwrap_or(t)
        }
        Easing::Elastic => {
            crate::as_easing::ease("Elastic", "easeOut", t, 0.0, 1.0, 1.0, f64::NAN, f64::NAN)
                .unwrap_or(t)
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
    let pivot = match (x.pivot, y.pivot) {
        (Some(pa), Some(pb)) => Some([lerp(pa[0], pb[0], t), lerp(pa[1], pb[1], t)]),
        (pa, _) => pa,
    };
    let primitive = match (x.primitive, y.primitive) {
        (Some(PrimitiveOptions::Rectangle(ra)), Some(PrimitiveOptions::Rectangle(rb))) => {
            let r = RectangleOptions {
                corner_radii: std::array::from_fn(|i| {
                    lerp(ra.corner_radii[i], rb.corner_radii[i], t)
                }),
                corners_locked: ra.corners_locked,
            };
            (!r.is_default()).then_some(PrimitiveOptions::Rectangle(r))
        }
        (Some(PrimitiveOptions::Oval(oa)), Some(PrimitiveOptions::Oval(ob))) => {
            let o = OvalOptions {
                start_angle: lerp(oa.start_angle, ob.start_angle, t),
                end_angle: lerp(oa.end_angle, ob.end_angle, t),
                inner_radius: lerp(oa.inner_radius, ob.inner_radius, t),
                close_path: oa.close_path,
            };
            (!o.is_default()).then_some(PrimitiveOptions::Oval(o))
        }
        (pa, _) => pa,
    };
    e.transform = Transform {
        x: lerp(x.x, y.x, t),
        y: lerp(x.y, y.y, t),
        scale_x: lerp(x.scale_x, y.scale_x, t),
        scale_y: lerp(x.scale_y, y.scale_y, t),
        rotation: lerp(x.rotation, y.rotation, t),
        alpha: lerp(x.alpha, y.alpha, t),
        skew: lerp(x.skew, y.skew, t),
        color_transform: crate::ColorTransform {
            multipliers: std::array::from_fn(|i| {
                lerp(
                    x.color_transform.multipliers[i],
                    y.color_transform.multipliers[i],
                    t,
                )
            }),
            offsets: std::array::from_fn(|i| {
                lerp(
                    x.color_transform.offsets[i],
                    y.color_transform.offsets[i],
                    t,
                )
            }),
        },
        pivot,
        primitive,
    };
    match (&a.kind, &b.kind, &mut e.kind) {
        (
            ElementKind::Video {
                width: aw,
                height: ah,
            },
            ElementKind::Video {
                width: bw,
                height: bh,
            },
            ElementKind::Video { width, height },
        ) => {
            *width = lerp(*aw, *bw, t);
            *height = lerp(*ah, *bh, t);
        }
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
    if key.keyed_properties.is_some()
        || (key.tween.is_some() && key.tween_kind == TweenKind::Motion)
    {
        return sample_motion(layer, frame);
    }
    let Some(ease) = key.tween else {
        return key.elements.clone();
    };
    let Some(next) = next_key(layer, key.frame) else {
        return key.elements.clone();
    };
    let t = easing(
        (frame - key.frame) as f64 / (next.frame - key.frame) as f64,
        ease,
    );
    if key.tween_kind == TweenKind::Shape {
        return crate::shape_tween::shape_tween_elements(&key.elements, &next.elements, t);
    }
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

fn next_key(layer: &Layer, frame: u32) -> Option<&Keyframe> {
    layer
        .keyframes
        .iter()
        .filter(|k| k.frame > frame)
        .min_by_key(|k| k.frame)
}

/// The keys of the motion tween span containing `frame`: its start key, then
/// its ◆ property keys in frame order, and the frame where the span ends.
pub fn motion_span(layer: &Layer, frame: u32) -> Option<(&Keyframe, Vec<&Keyframe>, Option<u32>)> {
    let mut keys: Vec<_> = layer.keyframes.iter().collect();
    keys.sort_by_key(|k| k.frame);
    let start = keys
        .iter()
        .rposition(|k| k.frame <= frame && k.keyed_properties.is_none())?;
    let first = keys[start];
    if first.tween.is_none() || first.tween_kind != TweenKind::Motion {
        return None;
    }
    let mut properties = vec![];
    let mut end = layer.properties.end_frame;
    for key in &keys[start + 1..] {
        if key.keyed_properties.is_none() {
            end = Some(key.frame);
            break;
        }
        properties.push(*key);
    }
    Some((first, properties, end))
}

pub fn property_value(transform: &Transform, property: TweenProperty) -> f64 {
    match property {
        TweenProperty::X => transform.x,
        TweenProperty::Y => transform.y,
        TweenProperty::ScaleX => transform.scale_x,
        TweenProperty::ScaleY => transform.scale_y,
        TweenProperty::Rotation => transform.rotation,
        TweenProperty::Skew => transform.skew,
        TweenProperty::Alpha => transform.alpha,
        TweenProperty::Color => 0.,
    }
}

fn set_property(
    transform: &mut Transform,
    property: TweenProperty,
    a: &Transform,
    b: &Transform,
    t: f64,
) {
    let value = |p| lerp(property_value(a, p), property_value(b, p), t);
    match property {
        TweenProperty::X => transform.x = value(property),
        TweenProperty::Y => transform.y = value(property),
        TweenProperty::ScaleX => transform.scale_x = value(property),
        TweenProperty::ScaleY => transform.scale_y = value(property),
        TweenProperty::Rotation => transform.rotation = value(property),
        TweenProperty::Skew => transform.skew = value(property),
        TweenProperty::Alpha => transform.alpha = value(property),
        TweenProperty::Color => {
            let (x, y) = (a.color_transform, b.color_transform);
            transform.color_transform = crate::ColorTransform {
                multipliers: std::array::from_fn(|i| lerp(x.multipliers[i], y.multipliers[i], t)),
                offsets: std::array::from_fn(|i| lerp(x.offsets[i], y.offsets[i], t)),
            };
        }
    }
}

pub fn effective_property_ease(start: &Keyframe, property: TweenProperty) -> Easing {
    start
        .property_eases
        .iter()
        .find(|(p, _)| *p == property)
        .map(|(_, ease)| *ease)
        .unwrap_or_else(|| start.tween.unwrap_or(Easing::Linear))
}

/// Motion tween: every property has its own track through the ◆ keys that key
/// it. Each property uses its override in `start.property_eases` (or the span
/// `start.tween` ease) to remap time over the span; after the last property key
/// a track holds its value.
fn sample_motion(layer: &Layer, frame: u32) -> Vec<Element> {
    let Some((start, properties, end)) = motion_span(layer, frame) else {
        return layer
            .key_at(frame)
            .map(|k| k.elements.clone())
            .unwrap_or_default();
    };
    let last = properties.last().map_or(start.frame, |k| k.frame);
    let span_end = end.unwrap_or(last).max(last).max(start.frame + 1);
    let length = (span_end - start.frame) as f64;
    let progress = ((frame - start.frame) as f64 / length).min(1.);
    let x_ease = effective_property_ease(start, TweenProperty::X);
    let y_ease = effective_property_ease(start, TweenProperty::Y);
    start
        .elements
        .iter()
        .map(|element| {
            let mut out = element.clone();
            out.transform = motion_pose(start, &properties, element, progress, length);
            if start.orient_to_path {
                // Orient to path: turn with the direction of travel, relative to the
                // direction at the span start (the authored rotation stays the base).
                if x_ease == y_ease {
                    let xy_time = start.frame as f64 + easing(progress, x_ease) * length;
                    let heading = |at: f64| {
                        let a = motion_pose_at_time(start, &properties, element, at);
                        let b = motion_pose_at_time(start, &properties, element, at + 0.5);
                        let (dx, dy) = (b.x - a.x, b.y - a.y);
                        (dx.hypot(dy) > 1e-9).then(|| dy.atan2(dx).to_degrees())
                    };
                    let initial = heading(start.frame as f64);
                    let now = heading(xy_time).or_else(|| heading(xy_time - 0.5));
                    if let (Some(initial), Some(now)) = (initial, now) {
                        out.transform.rotation += now - initial;
                    }
                } else {
                    let dp = (0.5 / length).min(0.25);
                    let heading_p = |p: f64| {
                        let p1 = (p + dp).min(1.0);
                        let p0 = if p1 - p > 1e-6 { p } else { (p1 - dp).max(0.0) };
                        let a = motion_pose(start, &properties, element, p0, length);
                        let b = motion_pose(start, &properties, element, p1, length);
                        let (dx, dy) = (b.x - a.x, b.y - a.y);
                        (dx.hypot(dy) > 1e-9).then(|| dy.atan2(dx).to_degrees())
                    };
                    if let (Some(initial), Some(now)) = (heading_p(0.0), heading_p(progress)) {
                        out.transform.rotation += now - initial;
                    }
                }
            }
            out
        })
        .collect()
}

fn interpolate_motion_property(
    transform: &mut Transform,
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    property: TweenProperty,
    time: f64,
) {
    let mut previous = (start.frame as f64, element.transform);
    let mut next = None;
    for key in properties {
        if !key
            .keyed_properties
            .as_ref()
            .is_some_and(|keyed| keyed.contains(&property))
        {
            continue;
        }
        let Some(keyed) = key.elements.iter().find(|e| e.id == element.id) else {
            continue;
        };
        if key.frame as f64 <= time {
            previous = (key.frame as f64, keyed.transform);
        } else {
            next = Some((key.frame as f64, keyed.transform));
            break;
        }
    }
    let (to, t) = match next {
        Some((frame, to)) => (
            to,
            ((time - previous.0) / (frame - previous.0)).clamp(0., 1.),
        ),
        None => (previous.1, 0.),
    };
    set_property(transform, property, &previous.1, &to, t);
}

/// Pose of one motion-tweened element at normalized `progress` (`0..=1`) over `length`:
/// every property applies its effective ease and interpolates between the ◆ keys that key it.
fn motion_pose(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    progress: f64,
    length: f64,
) -> Transform {
    let mut transform = element.transform;
    for property in TweenProperty::ALL {
        let ease = effective_property_ease(start, property);
        let time = start.frame as f64 + easing(progress, ease) * length;
        interpolate_motion_property(
            &mut transform,
            start,
            properties,
            element,
            property,
            time,
        );
    }
    transform
}

/// Pose of one motion-tweened element at an explicit fractional frame `time`.
fn motion_pose_at_time(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    time: f64,
) -> Transform {
    let mut transform = element.transform;
    for property in TweenProperty::ALL {
        interpolate_motion_property(
            &mut transform,
            start,
            properties,
            element,
            property,
            time,
        );
    }
    transform
}

/// Sample a layer within its timeline: classic tweens (and motion tweens with
/// `snap_to_guide`) on a layer guided by a motion guide move their objects'
/// registration points along the guide's first stroke.
pub fn sample_layer_in(timeline: &Timeline, layer: &Layer, frame: u32) -> Vec<Element> {
    let mut elements = sample_layer(layer, frame);
    let Some(guide) = layer
        .properties
        .motion_guide
        .as_ref()
        .and_then(|id| timeline.layers.iter().find(|l| &l.id == id))
    else {
        return elements;
    };
    if let Some((start, properties, end)) = motion_span(layer, frame)
        && start.snap_to_guide
        && let Some(path) = guide_polyline(guide, start.frame)
    {
        let last = properties.last().map_or(start.frame, |k| k.frame);
        let span_end = end.unwrap_or(last).max(last).max(start.frame + 1);
        let length = (span_end - start.frame) as f64;
        let progress = ((frame - start.frame) as f64 / length).min(1.);
        let x_ease = effective_property_ease(start, TweenProperty::X);
        let time = start.frame as f64 + easing(progress, x_ease) * length;
        for element in &mut elements {
            let Some(a) = start.elements.iter().find(|e| e.id == element.id) else {
                continue;
            };
            let s0 = path.nearest([a.transform.x, a.transform.y]);
            let mut waypoints = vec![(start.frame as f64, s0)];
            for key in &properties {
                if key.keyed_properties.as_ref().is_some_and(|kp| {
                    kp.contains(&TweenProperty::X) || kp.contains(&TweenProperty::Y)
                }) {
                    let pose = motion_pose_at_time(start, &properties, a, key.frame as f64);
                    waypoints.push((key.frame as f64, path.nearest([pose.x, pose.y])));
                }
            }
            let mut previous = waypoints[0];
            let mut next = None;
            for &wp in &waypoints[1..] {
                if wp.0 <= time {
                    previous = wp;
                } else {
                    next = Some(wp);
                    break;
                }
            }
            let s = match next {
                Some((wf, ws)) => {
                    let u = ((time - previous.0) / (wf - previous.0)).clamp(0., 1.);
                    lerp(previous.1, ws, u)
                }
                None => previous.1,
            };
            let [x, y] = path.point_at(s);
            element.transform.x = x;
            element.transform.y = y;
            if start.orient_to_path {
                let base = motion_pose(start, &properties, a, progress, length).rotation;
                element.transform.rotation = base + path.tangent_at(s) - path.tangent_at(s0);
            }
        }
        return elements;
    }
    let Some(key) = layer.key_at(frame) else {
        return elements;
    };
    let (Some(ease), TweenKind::Classic, None) = (key.tween, key.tween_kind, &key.keyed_properties)
    else {
        return elements;
    };
    let Some(next) = next_key(layer, key.frame) else {
        return elements;
    };
    let Some(path) = guide_polyline(guide, key.frame) else {
        return elements;
    };
    let t = easing(
        (frame - key.frame) as f64 / (next.frame - key.frame) as f64,
        ease,
    );
    for element in &mut elements {
        let (Some(a), Some(b)) = (
            key.elements.iter().find(|e| e.id == element.id),
            next.elements.iter().find(|e| e.id == element.id),
        ) else {
            continue;
        };
        let s0 = path.nearest([a.transform.x, a.transform.y]);
        let s1 = path.nearest([b.transform.x, b.transform.y]);
        let s = lerp(s0, s1, t);
        let [x, y] = path.point_at(s);
        element.transform.x = x;
        element.transform.y = y;
        if key.orient_to_path {
            // Flash semantics: the object keeps its start pose relative to the path and turns
            // with the path tangent.
            element.transform.rotation =
                a.transform.rotation + path.tangent_at(s) - path.tangent_at(s0);
        }
    }
    elements
}

/// A guide stroke flattened in timeline coordinates with cumulative lengths.
pub struct GuidePath {
    points: Vec<[f64; 2]>,
    lengths: Vec<f64>,
}

impl GuidePath {
    pub fn length(&self) -> f64 {
        self.lengths.last().copied().unwrap_or(0.)
    }

    /// Arc length of the point on the path nearest to `p`.
    pub fn nearest(&self, p: [f64; 2]) -> f64 {
        let mut best = (f64::INFINITY, 0.);
        for i in 1..self.points.len() {
            let (a, b) = (self.points[i - 1], self.points[i]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            let u = if len2 > 0. {
                (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0., 1.)
            } else {
                0.
            };
            let q = [a[0] + d[0] * u, a[1] + d[1] * u];
            let dist = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
            if dist < best.0 {
                best = (dist, lerp(self.lengths[i - 1], self.lengths[i], u));
            }
        }
        best.1
    }

    /// Direction of the path at arc length `s`, in degrees (0 = +x, clockwise with y down).
    /// Uses the segment containing `s`; zero-length segments look ahead, then behind.
    pub fn tangent_at(&self, s: f64) -> f64 {
        let s = s.clamp(0., self.length());
        let i = self
            .lengths
            .partition_point(|l| *l < s)
            .clamp(1, self.points.len() - 1);
        let segment = |i: usize| {
            let (a, b) = (self.points[i - 1], self.points[i]);
            [b[0] - a[0], b[1] - a[1]]
        };
        let d = (i..self.points.len())
            .chain((1..i).rev())
            .map(segment)
            .find(|d| d[0] != 0. || d[1] != 0.)
            .unwrap_or([1., 0.]);
        d[1].atan2(d[0]).to_degrees()
    }

    pub fn point_at(&self, s: f64) -> [f64; 2] {
        let s = s.clamp(0., self.length());
        let i = self
            .lengths
            .partition_point(|l| *l < s)
            .clamp(1, self.points.len() - 1);
        let (l0, l1) = (self.lengths[i - 1], self.lengths[i]);
        let u = if l1 > l0 { (s - l0) / (l1 - l0) } else { 0. };
        let (a, b) = (self.points[i - 1], self.points[i]);
        [lerp(a[0], b[0], u), lerp(a[1], b[1], u)]
    }
}

/// The first stroke on a guide layer at `frame`, in its timeline's coordinates.
pub fn guide_polyline(guide: &Layer, frame: u32) -> Option<GuidePath> {
    let key = guide.key_at(frame)?;
    key.elements.iter().find_map(|element| {
        let commands = match &element.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                commands
            }
            _ => return None,
        };
        let contour = crate::geometry::flatten_path(commands, Default::default())
            .ok()?
            .into_iter()
            .next()?;
        let m = transform_matrix(element.transform);
        let mut points: Vec<[f64; 2]> = contour
            .points
            .iter()
            .map(|p| {
                [
                    m[0] * p[0] + m[2] * p[1] + m[4],
                    m[1] * p[0] + m[3] * p[1] + m[5],
                ]
            })
            .collect();
        if contour.closed
            && let Some(first) = points.first().copied()
        {
            points.push(first);
        }
        if points.len() < 2 {
            return None;
        }
        let mut lengths = vec![0.];
        for i in 1..points.len() {
            let d = (points[i][0] - points[i - 1][0]).hypot(points[i][1] - points[i - 1][1]);
            lengths.push(lengths[i - 1] + d);
        }
        Some(GuidePath { points, lengths })
    })
}

/// Earliest key in the current continuous placement span (stable IDs survive keys, and
/// `sync_symbols` motion spans chain continuously across consecutive keyframes of the same
/// graphic symbol).
pub(crate) fn placement_start(layer: &Layer, frame: u32, id: &str) -> u32 {
    let mut keys: Vec<_> = layer
        .keyframes
        .iter()
        .filter(|k| k.frame <= frame)
        .collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.frame));
    let target_symbol = keys
        .iter()
        .find_map(|k| k.elements.iter().find(|e| e.id == id))
        .and_then(|e| match &e.kind {
            ElementKind::Instance { symbol_id, .. } => Some(symbol_id.as_str()),
            _ => None,
        });
    let mut current_id = id;
    let mut seen_current = false;
    let mut start = frame;
    for key in keys {
        if key.elements.iter().any(|e| e.id == current_id) {
            seen_current = true;
            start = key.frame;
            continue;
        }
        if seen_current
            && motion_span(layer, key.frame).is_some_and(|(s, _, _)| s.sync_symbols)
            && let Some(sym) = target_symbol
            && let Some(matched) = key.elements.iter().find(|e| {
                matches!(&e.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == sym)
            })
        {
            current_id = &matched.id;
            start = key.frame;
            continue;
        }
        break;
    }
    start
}

pub fn evaluate(project: &Project, symbol_id: &str, frame: u32) -> Vec<DrawItem> {
    snapshot_mode(project, symbol_id, frame, &|_| None, &|_| None, true).items
}

pub(crate) fn snapshot(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    clock: &dyn Fn(&[Id]) -> Option<u32>,
    patch: &dyn Fn(&[Id]) -> Option<DisplayPatch>,
) -> Evaluation {
    snapshot_mode(project, symbol_id, frame, clock, patch, false)
}

pub(crate) fn snapshot_mode(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    clock: &dyn Fn(&[Id]) -> Option<u32>,
    patch: &dyn Fn(&[Id]) -> Option<DisplayPatch>,
    include_guides: bool,
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
        include_guides,
    };
    walker.walk(
        root,
        frame,
        &[],
        InheritedDisplay {
            matrix: world,
            alpha: transform.alpha,
            visible,
            color_transform: transform.color_transform.with_alpha(transform.alpha),
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
    include_guides: bool,
}

#[derive(Clone, Copy)]
struct InheritedDisplay {
    matrix: [f64; 6],
    alpha: f64,
    visible: bool,
    color_transform: crate::ColorTransform,
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
        self.result
            .masks
            .extend(crate::authored_layers::layer_mask_plans(
                &symbol.timeline,
                frame,
                parent,
            ));
        // Layer zero is the backmost layer; list order is painter order everywhere.
        for layer in &symbol.timeline.layers {
            if !layer_effectively_visible(&symbol.timeline, layer)
                || layer.properties.role == LayerRole::Folder
                || (!self.include_guides && layer.properties.role == LayerRole::Guide)
            {
                continue;
            }
            for mut e in sample_layer_in(&symbol.timeline, layer, frame) {
                if self.result.nodes.len() >= MAX_NODES {
                    return;
                }
                let mut path = parent.to_vec();
                path.push(e.id.clone());
                let patch = (self.patch)(&path);
                let visible = patch.as_ref().and_then(|p| p.visible).unwrap_or(true);
                let button_frame = patch.as_ref().and_then(|p| p.button_frame).unwrap_or(0);
                if let Some(patch) = patch {
                    patch.apply(&mut e.transform);
                    match &mut e.kind {
                        ElementKind::Text { text, color, .. } => {
                            if let Some(value) = patch.text {
                                *text = value;
                            }
                            if let Some(value) = patch.text_color {
                                color[0] = (value >> 16) as u8;
                                color[1] = (value >> 8) as u8;
                                color[2] = value as u8;
                            }
                        }
                        ElementKind::TextBox { text, format, .. } => {
                            if let Some(value) = patch.text {
                                *text = value;
                            }
                            if let Some(value) = patch.text_color {
                                format.color = value;
                            }
                        }
                        _ => {}
                    }
                    if let Some(name) = patch.name {
                        e.name = name;
                    }
                }
                let world = affine_multiply(inherited.matrix, transform_matrix(e.transform));
                let opacity = inherited.alpha * e.transform.alpha.clamp(0., 1.);
                let color_transform = e
                    .transform
                    .color_transform
                    .with_alpha(e.transform.alpha)
                    .then(inherited.color_transform);
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
                    let child_frame = if child.kind == SymbolKind::Button {
                        button_frame.min(2).min(count - 1)
                    } else {
                        match playback {
                            InstancePlayback::Independent => {
                                (self.clock)(&path).unwrap_or(age % count) % count
                            }
                            InstancePlayback::Loop => age % count,
                            InstancePlayback::LoopFrom(start) => age.saturating_add(*start) % count,
                            InstancePlayback::PlayOnce => age.min(count - 1),
                            InstancePlayback::PlayOnceFrom(start) => {
                                age.saturating_add(*start).min(count - 1)
                            }
                            InstancePlayback::SingleFrame(f) => (*f).min(count - 1),
                        }
                    };
                    node.symbol_id = Some(child.id.clone());
                    node.frame = child_frame;
                    node.independent = child.kind != SymbolKind::Button
                        && matches!(playback, InstancePlayback::Independent);
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
                            color_transform,
                        },
                        depth + 1,
                    );
                } else {
                    let kind = e.parametric_path_kind().unwrap_or(e.kind);
                    node.kind = Some(kind.clone());
                    self.result.nodes.push(node);
                    if effective_visible {
                        self.result.items.push(DrawItem {
                            element_id: e.id,
                            name: e.name,
                            instance_path: path,
                            layer_id: layer.id.clone(),
                            transform: world,
                            alpha: opacity,
                            color_transform,
                            kind,
                        });
                    }
                }
            }
        }
    }
}
