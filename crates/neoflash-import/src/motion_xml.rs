//! The same AnimationCore record accompanies CS4 binary motion frames and XFL
//! MotionObjectXML. Sample its 2D tracks at the authored movie frames.
use neoflash_core::{ElementKind, Keyframe, transform_matrix};
use roxmltree::Node;
use std::collections::BTreeMap;

pub(crate) const DOCUMENT_POSE_BUDGET: usize = 10_000;
pub(crate) const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_XML_NODES: u32 = 16_384;

struct Control {
    time: f64,
    anchor: [f64; 2],
    next: [f64; 2],
    prev: [f64; 2],
}

fn number(s: &str) -> Option<f64> {
    s.parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && n.abs() <= 1e12)
}

fn point(s: &str) -> Option<[f64; 2]> {
    let (x, y) = s.split_once(',')?;
    Some([number(x)?, number(y)?])
}

fn bezier(p: [f64; 4], t: f64) -> f64 {
    let u = 1. - t;
    u * u * u * p[0] + 3. * u * u * t * p[1] + 3. * u * t * t * p[2] + t * t * t * p[3]
}

fn value(controls: &[Control], time: f64) -> f64 {
    let upper = controls.partition_point(|k| k.time <= time);
    if upper == 0 {
        return controls[0].anchor[1];
    }
    let a = &controls[upper - 1];
    let Some(b) = controls.get(upper) else {
        return a.anchor[1];
    };
    if time == a.time {
        return a.anchor[1];
    }
    let domain = [a.time, a.time + a.next[0], b.time + b.prev[0], b.time];
    if a.next == a.anchor && b.prev == b.anchor {
        return a.anchor[1] + (b.anchor[1] - a.anchor[1]) * ((time - a.time) / (b.time - a.time));
    }
    let (mut low, mut high) = (0., 1.);
    for _ in 0..38 {
        let mid = (low + high) * 0.5;
        if bezier(domain, mid) < time {
            low = mid;
        } else {
            high = mid;
        }
    }
    bezier(
        [a.anchor[1], a.next[1], b.prev[1], b.anchor[1]],
        (low + high) * 0.5,
    )
}

fn read_controls(n: Node<'_, '_>, total: f64) -> Result<Vec<Control>, &'static str> {
    let mut points = Vec::new();
    for k in n.children().filter(Node::is_element) {
        if !k.has_tag_name("Keyframe") || matches!(k.attribute("roving"), Some("1" | "true")) {
            return Err("unsupported motion key or roving timing");
        }
        let c = Control {
            time: k
                .attribute("timevalue")
                .and_then(number)
                .ok_or("invalid motion key time")?,
            anchor: k
                .attribute("anchor")
                .and_then(point)
                .ok_or("invalid motion anchor")?,
            next: k
                .attribute("next")
                .and_then(point)
                .ok_or("invalid outgoing motion handle")?,
            prev: k
                .attribute("previous")
                .and_then(point)
                .ok_or("invalid incoming motion handle")?,
        };
        if c.anchor[0] != 0. || c.time < 0. || c.time > total {
            return Err("motion key is outside its span");
        }
        if points.last().is_some_and(|p: &Control| {
            p.time >= c.time
                || p.next[0] < 0.
                || c.prev[0] > 0.
                || p.time + p.next[0] > c.time + c.prev[0]
        }) {
            return Err("nonmonotone motion handles");
        }
        points.push(c);
    }
    if points.first().is_some_and(|k: &Control| k.time != 0.) {
        return Err("motion track has no initial key");
    }
    Ok(points)
}

struct Motion<'a> {
    total: f64,
    tracks: BTreeMap<&'a str, Vec<Control>>,
    orient: bool,
}

fn read_motion<'a>(root: Node<'a, '_>) -> Result<Motion<'a>, &'static str> {
    if !root.has_tag_name("AnimationCore") || root.attribute("Version") != Some("1") {
        return Err("unknown motion data version");
    }
    let total = root
        .attribute("duration")
        .and_then(number)
        .filter(|n| *n > 0.)
        .ok_or("invalid motion duration")?;
    root.attribute("TimeScale")
        .and_then(number)
        .filter(|n| *n > 0.)
        .ok_or("invalid motion time scale")?;
    if root.range().len() > MAX_XML_BYTES
        || root.descendants().take(MAX_XML_NODES as usize + 1).count() > MAX_XML_NODES as usize
    {
        return Err("motion XML exceeds the sampling limit");
    }
    let mut tracks = BTreeMap::new();
    let mut orient = false;
    let mut disabled_end = 0;
    for n in root.descendants().filter(Node::is_element) {
        // Disabled properties do not participate in the authored motion, even
        // when their saved controls use a later Animate-only representation.
        if n.range().start < disabled_end {
            continue;
        }
        if n.tag_name().namespace() != root.tag_name().namespace() {
            return Err("unknown motion XML namespace");
        }
        match n.tag_name().name() {
            "Settings" => {
                orient = matches!(n.attribute("orientToPath"), Some("1" | "true"));
                if n.attribute("xformPtZOffsetPixels")
                    .and_then(number)
                    .unwrap_or(0.)
                    != 0.
                {
                    return Err("3D motion pivot");
                }
            }
            "TimeMap"
                if matches!(n.attribute("type"), Some("Quadratic" | "Linear"))
                    && n.attribute("strength").and_then(number) == Some(0.)
                    && n.children().all(|e| !e.is_element()) => {}
            "TimeMap" => return Err("nonlinear or custom motion time map"),
            "Property" => {
                if matches!(n.attribute("enabled"), Some("0" | "false")) {
                    disabled_end = n.range().end;
                    continue;
                }
                let id = n.attribute("id").ok_or("motion property without a name")?;
                let points = read_controls(n, total)?;
                if points.is_empty() {
                    continue;
                }
                if matches!(
                    id,
                    "Motion_X" | "Motion_Y" | "Rotation_Z" | "Scale_X" | "Scale_Y"
                ) {
                    if tracks.insert(id, points).is_some() {
                        return Err("duplicate motion track");
                    }
                } else if !matches!(
                    id,
                    "Skew_X" | "Skew_Y" | "Motion_Z" | "Rotation_X" | "Rotation_Y" | "Scale_Z"
                ) {
                    return Err("unknown enabled motion property");
                } else if points.iter().any(|p| {
                    [p.anchor[1], p.next[1], p.prev[1]]
                        .iter()
                        .any(|v| (*v - points[0].anchor[1]).abs() > 1e-9)
                }) {
                    return Err("animated skew, color, filter or 3D track");
                }
            }
            "AnimationCore" | "metadata" | "names" | "name" | "PropertyContainer" | "Keyframe" => {}
            _ => return Err("unknown motion XML element"),
        }
    }
    if tracks.is_empty() {
        return Err("no supported 2D motion tracks");
    }
    Ok(Motion {
        total,
        tracks,
        orient,
    })
}

/// The first pose is already present in the caller. Return only subsequent
/// poses, preserving instance identity and leaving frame labels and sound on
/// their original key. The budget belongs to the whole imported document.
pub(crate) fn poses(
    root: Node<'_, '_>,
    key: &Keyframe,
    duration: u32,
    pivot: Option<[f64; 2]>,
    budget: &mut usize,
) -> Result<Vec<Keyframe>, &'static str> {
    let count = duration.saturating_sub(1) as usize;
    if count == 0 || count > *budget || count > 4095 {
        return Err("motion span duration exceeds the available sampling range");
    }
    let [object] = key.elements.as_slice() else {
        return Err("motion span needs one placed instance");
    };
    if !matches!(object.kind, ElementKind::Instance { .. })
        || !object.scripts.is_empty()
        || !key.bitmap_fills.is_empty()
        || !key.asset_warps.is_empty()
        || !key.appearances.is_empty()
        || !key.vector_effects.is_empty()
        || !key.element_parents.is_empty()
        || !key.instance_overrides.is_empty()
        || !key.extended_accessibility.is_empty()
        || !key.filters.is_empty()
        || !key.ik_armatures.is_empty()
        || !key.property_expressions.is_empty()
        || key.camera.is_some()
        || key.video.is_some()
        || key.tween.is_some()
    {
        return Err("motion span uses unsupported placement effects or object scripts");
    }
    // Account for long instance names, filters and accessibility metadata as well
    // as the number of poses; repeated keyframes must fit the shared import budget.
    let size = serde_json::to_vec(object)
        .map_err(|_| "unserializable motion placement")?
        .len();
    let cost = count.saturating_mul(size.div_ceil(2048).max(1));
    if cost > *budget {
        return Err("motion poses exceed the shared document sampling budget");
    }
    let Motion {
        total,
        tracks,
        orient,
    } = read_motion(root)?;
    let sample =
        |id: &str, time: f64, fallback: f64| tracks.get(id).map_or(fallback, |c| value(c, time));
    let sx0 = sample("Scale_X", 0., 100.);
    let sy0 = sample("Scale_Y", 0., 100.);
    if sx0.abs() < 1e-9 || sy0.abs() < 1e-9 {
        return Err("singular initial motion scale");
    }
    let step = total / f64::from(duration);
    let pos = |t: f64| [sample("Motion_X", t, 0.), sample("Motion_Y", t, 0.)];
    let origin = pos(0.);
    let tangent = |t: f64| {
        let a = pos((t - step / 1024.).max(0.));
        let b = pos((t + step / 1024.).min(total - step));
        let d = [b[0] - a[0], b[1] - a[1]];
        (d[0].hypot(d[1]) > 1e-9).then(|| d[1].atan2(d[0]).to_degrees())
    };
    let mut heading = tangent(0.).unwrap_or(0.);
    let initial_heading = heading;
    let rotation0 = sample("Rotation_Z", 0., 0.);
    let initial = object.transform;
    let base = transform_matrix(initial);
    let mut poses = Vec::with_capacity(count);
    for frame in 1..duration {
        let t = step * f64::from(frame);
        let xy = pos(t);
        let mut element = object.clone();
        let tr = &mut element.transform;
        if orient && let Some(direction) = tangent(t) {
            heading = direction;
        }
        tr.rotation += sample("Rotation_Z", t, 0.) - rotation0
            + if orient {
                heading - initial_heading
            } else {
                0.
            };
        tr.scale_x *= sample("Scale_X", t, 100.) / sx0;
        tr.scale_y *= sample("Scale_Y", t, 100.) / sy0;
        tr.x += xy[0] - origin[0];
        tr.y += xy[1] - origin[1];
        if (tr.rotation - initial.rotation).abs() > 1e-8
            || (tr.scale_x - initial.scale_x).abs() > 1e-8
            || (tr.scale_y - initial.scale_y).abs() > 1e-8
        {
            let [px, py] = pivot.ok_or("unresolved rotation or scale pivot")?;
            let current = transform_matrix(*tr);
            tr.x += (base[0] - current[0]) * px + (base[2] - current[2]) * py;
            tr.y += (base[1] - current[1]) * px + (base[3] - current[3]) * py;
        }
        if ![tr.x, tr.y, tr.rotation, tr.scale_x, tr.scale_y]
            .iter()
            .all(|v| v.is_finite())
            || tr.x.abs() > 1e12
            || tr.y.abs() > 1e12
            || tr.rotation.abs() > 1e9
            || tr.scale_x.abs() > 1e4
            || tr.scale_y.abs() > 1e4
        {
            return Err("motion pose exceeds the document transform limits");
        }
        poses.push(Keyframe {
            frame: key
                .frame
                .checked_add(frame)
                .ok_or("motion frame overflow")?,
            elements: vec![element],
            ..Default::default()
        });
    }
    *budget -= cost;
    Ok(poses)
}
