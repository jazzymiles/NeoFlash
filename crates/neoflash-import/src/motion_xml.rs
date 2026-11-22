//! CS4+ `AnimationCore` and `fl.motion` `<Motion>` property-track sampling
//! shared by binary FLA (`CPicFrame.motion_xml`) and XFL (`MotionObjectXML` /
//! `motionObjectXML` / `DOMMotionObject`).
use neoflash_core::{
    ColorTransform, Element, ElementKind, FilterSpec, Keyframe, MAX_OBJECT_FILTERS, Transform,
    adjust_color_matrix, transform_matrix,
};
use roxmltree::Node;
use std::collections::BTreeMap;

pub(crate) const DOCUMENT_POSE_BUDGET: usize = 10_000;
pub(crate) const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_XML_NODES: u32 = 16_384;

const DEFAULT_VIEW_ANGLE_3D_DEG: f64 = 55.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionContext {
    pub pivot: Option<[f64; 2]>,
    pub stage_size: [f64; 2],
    pub view_angle_3d: Option<f64>,
    pub vanishing_point_3d: Option<[f64; 2]>,
}

impl Default for MotionContext {
    fn default() -> Self {
        Self {
            pivot: None,
            stage_size: [550.0, 400.0],
            view_angle_3d: None,
            vanishing_point_3d: None,
        }
    }
}

pub(crate) struct SampledMotion {
    pub start_element: Option<Element>,
    pub poses: Vec<Keyframe>,
    pub used_3d: bool,
    pub filter_approximated: bool,
}

#[derive(Clone, Debug)]
struct Control {
    time: f64,
    anchor: [f64; 2],
    next: [f64; 2],
    prev: [f64; 2],
    roving: bool,
    seg_ease: Option<SegmentEase>,
}

#[derive(Clone, Debug)]
enum SegmentEase {
    Flash(f64),
    Named(String, f64),
    CubicPoints(Vec<[f64; 2]>),
}

#[derive(Clone, Debug)]
enum TimeMapKind {
    Linear,
    Power(i32, f64),
    DualPower(i32, f64),
    Bounce(f64),
    BounceIn(f64),
    Spring(f64),
    SineWave(f64),
    SawtoothWave(f64),
    SquareWave(f64),
    DampedWave(f64),
    Back(f64),
    Elastic(f64),
    Custom(Vec<Control>),
}

#[derive(Clone, Debug)]
struct Track {
    controls: Vec<Control>,
    time_map: Option<TimeMapKind>,
}

#[derive(Clone, Debug)]
struct FilterCurveGroup {
    kind: String,
    tracks: BTreeMap<String, Track>,
    approximated: bool,
}

struct ParsedMotion {
    total: f64,
    tracks: BTreeMap<String, Track>,
    filters: Vec<FilterCurveGroup>,
    orient: bool,
    z_pivot_offset: f64,
    fallback_pivot: Option<[f64; 2]>,
    filter_approximated: bool,
}

fn number(s: &str) -> Option<f64> {
    s.trim()
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && n.abs() <= 1e12)
}

fn point(s: &str) -> Option<[f64; 2]> {
    let (x, y) = s.split_once(',')?;
    Some([number(x)?, number(y)?])
}

fn parse_rgba_hex(raw: &str) -> Option<[f64; 4]> {
    let trimmed = raw.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([
                f64::from((n >> 24) & 0xff),
                f64::from((n >> 16) & 0xff),
                f64::from((n >> 8) & 0xff),
                f64::from(n & 0xff),
            ]);
        }
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([
                f64::from((n >> 16) & 0xff),
                f64::from((n >> 8) & 0xff),
                f64::from(n & 0xff),
                255.0,
            ]);
        }
    }
    if let Some(hex) = trimmed.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([
                f64::from((n >> 16) & 0xff),
                f64::from((n >> 8) & 0xff),
                f64::from(n & 0xff),
                255.0,
            ]);
        }
        if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([
                f64::from((n >> 24) & 0xff),
                f64::from((n >> 16) & 0xff),
                f64::from((n >> 8) & 0xff),
                f64::from(n & 0xff),
            ]);
        }
    }
    None
}

fn bezier(p: [f64; 4], t: f64) -> f64 {
    let u = 1. - t;
    u * u * u * p[0] + 3. * u * u * t * p[1] + 3. * u * t * t * p[2] + t * t * t * p[3]
}

fn eval_custom_ease_points(pts: &[[f64; 2]], u: f64) -> f64 {
    if pts.len() < 4 || !(pts.len() - 1).is_multiple_of(3) {
        return u;
    }
    if u <= 0.0 {
        return pts[0][1];
    }
    if u >= 1.0 {
        return pts.last().unwrap()[1];
    }
    let seg_count = (pts.len() - 1) / 3;
    for m in 0..seg_count {
        let p0 = pts[3 * m];
        let p1 = pts[3 * m + 1];
        let p2 = pts[3 * m + 2];
        let p3 = pts[3 * m + 3];
        if u <= p3[0] + 1e-9 || m + 1 == seg_count {
            if p3[0] <= p0[0] + 1e-9 {
                return p3[1];
            }
            let c1x = p1[0].clamp(p0[0], p3[0]);
            let c2x = p2[0].clamp(p0[0], p3[0]);
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if bezier([p0[0], c1x, c2x, p3[0]], mid) < u {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return bezier([p0[1], p1[1], p2[1], p3[1]], 0.5 * (lo + hi));
        }
    }
    u
}

fn apply_segment_ease(ease: &SegmentEase, u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    match ease {
        SegmentEase::Flash(accel) => {
            let a = (accel / 100.0).clamp(-1.0, 1.0);
            if a == 0.0 {
                u
            } else if a > 0.0 {
                u * (1.0 - a) + (1.0 - (1.0 - u) * (1.0 - u)) * a
            } else {
                let k = -a;
                u * (1.0 - k) + (u * u) * k
            }
        }
        SegmentEase::Named(method, intensity) => {
            let m = method.to_ascii_lowercase();
            let strength = intensity.clamp(-100.0, 100.0);
            if m.contains("bounce") {
                apply_time_map(&TimeMapKind::BounceIn(4.0), u)
            } else if m.contains("elastic") || m.contains("spring") {
                apply_time_map(&TimeMapKind::Elastic(strength), u)
            } else if m.contains("back") {
                apply_time_map(&TimeMapKind::Back(strength), u)
            } else if m.contains("sine") {
                let s = if m.contains("in") && !m.contains("out") {
                    -100.0
                } else {
                    100.0
                };
                let curve = if s < 0.0 {
                    1.0 - (u * std::f64::consts::FRAC_PI_2).cos()
                } else if m.contains("inout") || m.contains("in_out") || m.contains("in-out") {
                    0.5 * (1.0 - (std::f64::consts::PI * u).cos())
                } else {
                    (u * std::f64::consts::FRAC_PI_2).sin()
                };
                let blend = (strength.abs() / 100.0).clamp(0.0, 1.0);
                let w = if strength == 0.0 { 1.0 } else { blend };
                w * curve + (1.0 - w) * u
            } else {
                let power = if m.contains("quint") {
                    5
                } else if m.contains("quart") {
                    4
                } else if m.contains("cubic") {
                    3
                } else {
                    2
                };
                if m.contains("inout") || m.contains("in_out") || m.contains("in-out") {
                    let s = if strength == 0.0 {
                        -100.0
                    } else {
                        -strength.abs()
                    };
                    apply_time_map(&TimeMapKind::DualPower(power, s), u)
                } else if m.contains("in") {
                    let s = if strength == 0.0 {
                        -100.0
                    } else {
                        -strength.abs()
                    };
                    apply_time_map(&TimeMapKind::Power(power, s), u)
                } else {
                    let s = if strength == 0.0 {
                        100.0
                    } else {
                        strength.abs()
                    };
                    apply_time_map(&TimeMapKind::Power(power, s), u)
                }
            }
        }
        SegmentEase::CubicPoints(pts) => eval_custom_ease_points(pts, u),
    }
}

fn bounce_arc(n: usize, r: f64) -> (usize, f64) {
    let mut harmonic = 0.0;
    for i in 1..=n {
        harmonic += 1.0 / (i as f64);
    }
    let t0 = 1.0 / harmonic;
    let mut k = 0usize;
    let mut start = 0.0;
    let mut end = t0;
    while end < r && k + 1 < n {
        k += 1;
        start = end;
        end += t0 / ((k + 1) as f64);
    }
    let width = t0 / ((k + 1) as f64);
    (
        k,
        if width > 1e-12 {
            ((r - start) / width).clamp(0.0, 1.0)
        } else {
            0.0
        },
    )
}

fn bounce_ease(n: usize, r: f64) -> f64 {
    let (k, u) = bounce_arc(n, r);
    4.0 * (-(k as f64)).exp() * u * (1.0 - u)
}

fn bounce_in_ease(n: usize, r: f64) -> f64 {
    let (k, u) = bounce_arc(n, r);
    if k == 0 {
        u * u
    } else {
        1.0 - 4.0 * (-(k as f64)).exp() * u * (1.0 - u)
    }
}

fn apply_time_map(tm: &TimeMapKind, r: f64) -> f64 {
    let r = r.clamp(0.0, 1.0);
    match tm {
        TimeMapKind::Linear => r,
        TimeMapKind::Power(power, strength) => {
            if *strength == 0.0 {
                return r;
            }
            let curve = if *strength > 0.0 {
                1.0 - (1.0 - r).powi(*power)
            } else {
                r.powi(*power)
            };
            let a = (strength.abs() / 100.0).min(1.0);
            a * curve + (1.0 - a) * r
        }
        TimeMapKind::DualPower(power, strength) => {
            if *strength == 0.0 {
                return r;
            }
            let p = |x: f64| x.abs().powi(*power);
            let curve = if *strength > 0.0 {
                if r < 0.5 {
                    0.5 - p(2.0 * r - 1.0) * 0.5
                } else {
                    0.5 + p(2.0 * r - 1.0) * 0.5
                }
            } else if r < 0.5 {
                p(2.0 * r) * 0.5
            } else {
                1.0 - p(2.0 * r - 2.0) * 0.5
            };
            let a = (strength.abs() / 100.0).min(1.0);
            a * curve + (1.0 - a) * r
        }
        TimeMapKind::Bounce(strength) => {
            let n = (strength.trunc().abs() as usize).min(100);
            if n == 0 {
                r
            } else if *strength > 0.0 {
                bounce_ease(n, r)
            } else {
                bounce_ease(n, 1.0 - r)
            }
        }
        TimeMapKind::BounceIn(strength) => {
            let n = (strength.trunc().abs() as usize).min(100);
            if n == 0 {
                r
            } else if *strength > 0.0 {
                bounce_in_ease(n, r)
            } else {
                1.0 - bounce_in_ease(n, 1.0 - r)
            }
        }
        TimeMapKind::Spring(strength) => {
            let n = strength.trunc() as i32;
            if n < 1 {
                return r;
            }
            let nf = f64::from(n.min(100));
            let startup = 0.4 / nf;
            let cycle = (1.0 - startup) / nf;
            if r < startup {
                (r * std::f64::consts::FRAC_PI_2 / startup).sin()
            } else {
                let t = r - startup;
                0.7 + 0.3 * (-t / cycle).exp() * (2.0 * std::f64::consts::PI / cycle * t).cos()
            }
        }
        TimeMapKind::SineWave(strength) => {
            let waves = (strength.trunc() as i32).clamp(0, 100);
            if waves == 0 {
                r
            } else {
                0.5 * (1.0
                    + (r * std::f64::consts::PI * f64::from(waves) - std::f64::consts::FRAC_PI_2)
                        .sin())
            }
        }
        TimeMapKind::SawtoothWave(strength) => {
            let waves = f64::from((strength.trunc() as i32).clamp(1, 100));
            let v = (r * waves).rem_euclid(2.0);
            if v > 1.0 { 2.0 - v } else { v }
        }
        TimeMapKind::SquareWave(strength) => {
            let waves = f64::from((strength.trunc() as i32).clamp(2, 100));
            if (r * waves).rem_euclid(2.0) <= 1.0 {
                0.0
            } else {
                1.0
            }
        }
        TimeMapKind::DampedWave(strength) => {
            let waves = (strength.trunc() as i32).clamp(0, 100);
            if waves == 0 {
                r
            } else {
                let wf = f64::from(waves);
                -0.5 * (r * std::f64::consts::PI * wf * 2.0).cos() * (-r * wf).exp()
            }
        }
        TimeMapKind::Back(strength) => {
            let a = if *strength == 0.0 {
                1.0
            } else {
                (strength.abs() / 100.0).min(1.0)
            };
            let s = 1.70158;
            let curve = if *strength >= 0.0 {
                let u = r - 1.0;
                u * u * ((s + 1.0) * u + s) + 1.0
            } else {
                r * r * ((s + 1.0) * r - s)
            };
            a * curve + (1.0 - a) * r
        }
        TimeMapKind::Elastic(strength) => {
            if r <= 0.0 || r >= 1.0 {
                return r;
            }
            let a = if *strength == 0.0 {
                1.0
            } else {
                (strength.abs() / 100.0).min(1.0)
            };
            let c4 = (2.0 * std::f64::consts::PI) / 3.0;
            let curve = if *strength >= 0.0 {
                2.0_f64.powf(-10.0 * r) * ((r * 10.0 - 0.75) * c4).sin() + 1.0
            } else {
                -(2.0_f64.powf(10.0 * r - 10.0) * ((r * 10.0 - 10.75) * c4).sin())
            };
            a * curve + (1.0 - a) * r
        }
        TimeMapKind::Custom(controls) => {
            if controls.is_empty() {
                return r;
            }
            let first = controls[0].time;
            let last = controls.last().unwrap().time;
            if last <= first {
                return r;
            }
            raw_value(controls, first + (last - first) * r).clamp(0.0, 1.0)
        }
    }
}

fn raw_value(controls: &[Control], time: f64) -> f64 {
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
    if let Some(ref seg_ease) = a.seg_ease {
        let u = ((time - a.time) / (b.time - a.time)).clamp(0.0, 1.0);
        let v = apply_segment_ease(seg_ease, u);
        return a.anchor[1] + (b.anchor[1] - a.anchor[1]) * v;
    }
    if a.next == a.anchor && b.prev == b.anchor {
        return a.anchor[1] + (b.anchor[1] - a.anchor[1]) * ((time - a.time) / (b.time - a.time));
    }
    let c1_t = (a.time + a.next[0]).clamp(a.time, b.time);
    let c2_t = (b.time + b.prev[0]).clamp(a.time, b.time);
    let domain = [a.time, c1_t, c2_t, b.time];
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

fn eval_track(track: &Track, time: f64) -> f64 {
    let controls = &track.controls;
    if controls.is_empty() {
        return 0.0;
    }
    match &track.time_map {
        None | Some(TimeMapKind::Linear) => raw_value(controls, time),
        Some(TimeMapKind::Power(_, s) | TimeMapKind::DualPower(_, s)) if *s == 0.0 => {
            raw_value(controls, time)
        }
        Some(tm) => {
            if controls.len() == 1 {
                return controls[0].anchor[1];
            }
            let first = controls[0].time;
            let last = controls.last().unwrap().time;
            if time <= first || last <= first {
                return controls[0].anchor[1];
            }
            let ratio = apply_time_map(tm, ((time - first) / (last - first)).min(1.0));
            let eased = (first + (last - first) * ratio).clamp(first, last);
            raw_value(controls, eased)
        }
    }
}

enum ParsedPropertyControls {
    Numeric(Vec<Control>),
    Rgba([Vec<Control>; 4]),
}

fn read_property_controls(
    n: Node<'_, '_>,
    total: f64,
) -> Result<ParsedPropertyControls, &'static str> {
    let mut points = Vec::new();
    let mut color_keys: Vec<(f64, [f64; 4], bool)> = Vec::new();
    let child_elements: Vec<_> = n.children().filter(Node::is_element).collect();
    if child_elements.is_empty()
        && let Some(raw_val) = n.attribute("value")
    {
        if let Some(rgba) = parse_rgba_hex(raw_val) {
            color_keys.push((0.0, rgba, false));
        } else if let Some(v) = number(raw_val) {
            points.push(Control {
                time: 0.0,
                anchor: [0.0, v],
                next: [0.0, v],
                prev: [0.0, v],
                roving: false,
                seg_ease: None,
            });
        } else {
            return Err("invalid motion anchor");
        }
    }
    for k in child_elements {
        if !k.has_tag_name("Keyframe") {
            return Err("unsupported motion key or roving timing");
        }
        let roving = matches!(k.attribute("roving"), Some("1" | "true"));
        let time = k
            .attribute("timevalue")
            .and_then(number)
            .ok_or("invalid motion key time")?;
        if time < 0. || time > total {
            return Err("motion key is outside its span");
        }
        if let Some(anchor_attr) = k.attribute("anchor") {
            let anchor = point(anchor_attr).ok_or("invalid motion anchor")?;
            let next = match k.attribute("next") {
                Some(s) => point(s).ok_or("invalid outgoing motion handle")?,
                None => [0.0, anchor[1]],
            };
            let prev = match k.attribute("previous") {
                Some(s) => point(s).ok_or("invalid incoming motion handle")?,
                None => [0.0, anchor[1]],
            };
            if anchor[0] != 0. {
                return Err("motion key is outside its span");
            }
            if points
                .last()
                .is_some_and(|p: &Control| p.time >= time || p.next[0] < 0. || prev[0] > 0.)
            {
                return Err("nonmonotone motion handles");
            }
            points.push(Control {
                time,
                anchor,
                next,
                prev,
                roving,
                seg_ease: None,
            });
        } else if let Some(raw_val) = k.attribute("value") {
            if let Some(rgba) = parse_rgba_hex(raw_val) {
                if color_keys.last().is_some_and(|prev| prev.0 >= time) {
                    return Err("nonmonotone motion handles");
                }
                color_keys.push((time, rgba, roving));
            } else if let Some(v) = number(raw_val) {
                if points.last().is_some_and(|p: &Control| p.time >= time) {
                    return Err("nonmonotone motion handles");
                }
                points.push(Control {
                    time,
                    anchor: [0.0, v],
                    next: [0.0, v],
                    prev: [0.0, v],
                    roving,
                    seg_ease: None,
                });
            } else {
                return Err("invalid motion anchor");
            }
        } else {
            return Err("invalid motion anchor");
        }
    }
    if !color_keys.is_empty() {
        if color_keys[0].0 != 0. {
            return Err("motion track has no initial key");
        }
        let channels = std::array::from_fn(|ch| {
            color_keys
                .iter()
                .map(|&(t, rgba, roving)| Control {
                    time: t,
                    anchor: [0.0, rgba[ch]],
                    next: [0.0, rgba[ch]],
                    prev: [0.0, rgba[ch]],
                    roving,
                    seg_ease: None,
                })
                .collect()
        });
        return Ok(ParsedPropertyControls::Rgba(channels));
    }
    if points.first().is_some_and(|k: &Control| k.time != 0.) {
        return Err("motion track has no initial key");
    }
    Ok(ParsedPropertyControls::Numeric(points))
}

fn unpack_packed_rgb_controls(points: &[Control]) -> [Vec<Control>; 4] {
    std::array::from_fn(|ch| {
        points
            .iter()
            .map(|c| {
                let packed = c.anchor[1].round().clamp(0.0, 16_777_215.0) as u32;
                let comp = match ch {
                    0 => f64::from((packed >> 16) & 0xff),
                    1 => f64::from((packed >> 8) & 0xff),
                    2 => f64::from(packed & 0xff),
                    _ => 255.0,
                };
                Control {
                    time: c.time,
                    anchor: [0.0, comp],
                    next: [0.0, comp],
                    prev: [0.0, comp],
                    roving: c.roving,
                    seg_ease: c.seg_ease.clone(),
                }
            })
            .collect()
    })
}

fn redistribute_roving_times(tracks: &mut BTreeMap<String, Track>) {
    // First handle synchronized 2D/3D spatial roving keyframes across Motion_X/Motion_Y/Motion_Z.
    let spatial_ids = ["Motion_X", "Motion_Y", "Motion_Z"];
    let ref_len = spatial_ids
        .iter()
        .filter_map(|id| tracks.get(*id))
        .find(|t| {
            t.controls.len() >= 3 && t.controls[1..t.controls.len() - 1].iter().any(|c| c.roving)
        })
        .map(|t| t.controls.len());

    if let Some(len) = ref_len {
        let all_aligned = spatial_ids
            .iter()
            .filter_map(|id| tracks.get(*id))
            .all(|t| t.controls.len() == 1 || t.controls.len() == len);
        if all_aligned {
            let get_coord = |tracks: &BTreeMap<String, Track>, id: &str, idx: usize| -> [f64; 3] {
                match tracks.get(id) {
                    Some(t) if t.controls.len() == len => [
                        t.controls[idx].anchor[1],
                        t.controls[idx].next[1],
                        t.controls[idx].prev[1],
                    ],
                    Some(t) if !t.controls.is_empty() => [t.controls[0].anchor[1]; 3],
                    _ => [0.0; 3],
                }
            };
            let mut roving_flags = vec![false; len];
            let mut times = vec![0.0; len];
            for idx in 0..len {
                for id in spatial_ids {
                    if let Some(t) = tracks.get(id)
                        && t.controls.len() == len
                    {
                        times[idx] = t.controls[idx].time;
                        if idx > 0 && idx + 1 < len && t.controls[idx].roving {
                            roving_flags[idx] = true;
                        }
                    }
                }
            }
            let mut left = 0usize;
            while left + 1 < len {
                if !roving_flags[left + 1] {
                    left += 1;
                    continue;
                }
                let mut right = left + 1;
                while right + 1 < len && roving_flags[right] {
                    right += 1;
                }
                let t_start = times[left];
                let t_end = times[right];
                let mut seg_lengths = Vec::with_capacity(right - left);
                for k in left..right {
                    let xa = get_coord(tracks, "Motion_X", k);
                    let xb = get_coord(tracks, "Motion_X", k + 1);
                    let ya = get_coord(tracks, "Motion_Y", k);
                    let yb = get_coord(tracks, "Motion_Y", k + 1);
                    let za = get_coord(tracks, "Motion_Z", k);
                    let zb = get_coord(tracks, "Motion_Z", k + 1);
                    let mut dist = 0.0;
                    let mut prev_pt = [xa[0], ya[0], za[0]];
                    for step in 1..=16 {
                        let s = f64::from(step) / 16.0;
                        let pt = [
                            bezier([xa[0], xa[1], xb[2], xb[0]], s),
                            bezier([ya[0], ya[1], yb[2], yb[0]], s),
                            bezier([za[0], za[1], zb[2], zb[0]], s),
                        ];
                        let dx = pt[0] - prev_pt[0];
                        let dy = pt[1] - prev_pt[1];
                        let dz = pt[2] - prev_pt[2];
                        dist += (dx * dx + dy * dy + dz * dz).sqrt();
                        prev_pt = pt;
                    }
                    seg_lengths.push(dist);
                }
                let total_dist: f64 = seg_lengths.iter().sum();
                let old_times = times.clone();
                let mut accum = 0.0;
                for (offset, seg_d) in seg_lengths.iter().enumerate().take(right - left - 1) {
                    accum += *seg_d;
                    let frac = if total_dist > 1e-9 {
                        accum / total_dist
                    } else {
                        (offset + 1) as f64 / (right - left) as f64
                    };
                    times[left + 1 + offset] = t_start + (t_end - t_start) * frac;
                }
                for id in spatial_ids {
                    if let Some(track) = tracks.get_mut(id)
                        && track.controls.len() == len
                    {
                        for k in left..right {
                            let old_dt = (old_times[k + 1] - old_times[k]).max(1e-9);
                            let new_dt = (times[k + 1] - times[k]).max(0.0);
                            let scale = new_dt / old_dt;
                            track.controls[k].next[0] =
                                (track.controls[k].next[0] * scale).clamp(0.0, new_dt);
                            track.controls[k + 1].prev[0] =
                                (track.controls[k + 1].prev[0] * scale).clamp(-new_dt, 0.0);
                        }
                        for (k, &new_t) in times.iter().enumerate().take(right).skip(left + 1) {
                            track.controls[k].time = new_t;
                            track.controls[k].roving = false;
                        }
                    }
                }
                left = right;
            }
        }
    }

    // Fallback 1D roving redistribution for any remaining track with interior roving keys.
    for track in tracks.values_mut() {
        let len = track.controls.len();
        if len < 3 || !track.controls[1..len - 1].iter().any(|c| c.roving) {
            continue;
        }
        let mut left = 0usize;
        while left + 1 < len {
            if !(left + 1 < len - 1 && track.controls[left + 1].roving) {
                left += 1;
                continue;
            }
            let mut right = left + 1;
            while right + 1 < len && right < len - 1 && track.controls[right].roving {
                right += 1;
            }
            let t_start = track.controls[left].time;
            let t_end = track.controls[right].time;
            let old_times: Vec<f64> = track.controls.iter().map(|c| c.time).collect();
            let segs: Vec<f64> = (left..right)
                .map(|k| (track.controls[k + 1].anchor[1] - track.controls[k].anchor[1]).abs())
                .collect();
            let total_d: f64 = segs.iter().sum();
            let mut accum = 0.0;
            for (offset, d) in segs.iter().enumerate().take(right - left - 1) {
                accum += *d;
                let frac = if total_d > 1e-9 {
                    accum / total_d
                } else {
                    (offset + 1) as f64 / (right - left) as f64
                };
                track.controls[left + 1 + offset].time = t_start + (t_end - t_start) * frac;
                track.controls[left + 1 + offset].roving = false;
            }
            for k in left..right {
                let old_dt = (old_times[k + 1] - old_times[k]).max(1e-9);
                let new_dt = (track.controls[k + 1].time - track.controls[k].time).max(0.0);
                let scale = new_dt / old_dt;
                track.controls[k].next[0] = (track.controls[k].next[0] * scale).clamp(0.0, new_dt);
                track.controls[k + 1].prev[0] =
                    (track.controls[k + 1].prev[0] * scale).clamp(-new_dt, 0.0);
            }
            left = right;
        }
    }
}

fn is_filter_property_id(id: &str) -> Option<(&str, &str)> {
    let (prefix, rest) = id.split_once('_')?;
    if matches!(
        prefix,
        "Blur"
            | "Glow"
            | "DropShadow"
            | "Bevel"
            | "GradientGlow"
            | "GradientBevel"
            | "AdjColor"
            | "AdjustColor"
    ) {
        Some((prefix, rest))
    } else {
        None
    }
}

fn is_known_motion_property_id(id: &str) -> bool {
    matches!(
        id,
        "Motion_X"
            | "Motion_Y"
            | "Motion_Z"
            | "Translation_Z"
            | "Depth"
            | "Rotation_X"
            | "Rotation_Y"
            | "Rotation_Z"
            | "Scale_X"
            | "Scale_Y"
            | "Scale_Z"
            | "Skew_X"
            | "Skew_Y"
            | "Alpha_Amount"
            | "Brightness_Amount"
            | "Tint_Color"
            | "Tint_Amount"
            | "AdvClr_R_Pct"
            | "AdvClr_G_Pct"
            | "AdvClr_B_Pct"
            | "AdvClr_A_Pct"
            | "AdvClr_R_Offset"
            | "AdvClr_G_Offset"
            | "AdvClr_B_Offset"
            | "AdvClr_A_Offset"
    )
}

fn insert_property_into_map(
    map: &mut BTreeMap<String, Track>,
    key_name: &str,
    parsed: ParsedPropertyControls,
    time_map: Option<TimeMapKind>,
) -> Result<(), &'static str> {
    match parsed {
        ParsedPropertyControls::Numeric(points) => {
            if points.is_empty() {
                return Ok(());
            }
            if key_name == "Tint_Color"
                || key_name == "Color"
                || key_name == "ShadowColor"
                || key_name == "HilightColor"
            {
                let chans = unpack_packed_rgb_controls(&points);
                for (suffix, c_pts) in ["r", "g", "b", "a"].into_iter().zip(chans) {
                    let full = format!("{key_name}.{suffix}");
                    if map
                        .insert(
                            full,
                            Track {
                                controls: c_pts,
                                time_map: time_map.clone(),
                            },
                        )
                        .is_some()
                    {
                        return Err("duplicate motion track");
                    }
                }
            } else if map
                .insert(
                    key_name.to_owned(),
                    Track {
                        controls: points,
                        time_map,
                    },
                )
                .is_some()
            {
                return Err("duplicate motion track");
            }
        }
        ParsedPropertyControls::Rgba(chans) => {
            for (suffix, c_pts) in ["r", "g", "b", "a"].into_iter().zip(chans) {
                let full = format!("{key_name}.{suffix}");
                if map
                    .insert(
                        full,
                        Track {
                            controls: c_pts,
                            time_map: time_map.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err("duplicate motion track");
                }
            }
        }
    }
    Ok(())
}

fn parse_time_map_node(n: Node<'_, '_>, total: f64) -> Result<TimeMapKind, &'static str> {
    let strength = n.attribute("strength").and_then(number).unwrap_or(0.0);
    let tm_type = n.attribute("type").unwrap_or("Linear");
    if tm_type == "Custom" {
        let mut custom_controls = Vec::new();
        for desc in n.descendants().filter(Node::is_element) {
            if desc.has_tag_name("Property")
                && let ParsedPropertyControls::Numeric(pts) = read_property_controls(desc, total)?
                && !pts.is_empty()
            {
                custom_controls = pts;
                break;
            }
        }
        if custom_controls.is_empty()
            && n.children()
                .any(|c| c.is_element() && c.has_tag_name("Keyframe"))
            && let ParsedPropertyControls::Numeric(pts) = read_property_controls(n, total)?
        {
            custom_controls = pts;
        }
        if custom_controls.is_empty() {
            if strength == 0.0 {
                return Ok(TimeMapKind::Linear);
            }
            return Err("nonlinear or custom motion time map");
        }
        return Ok(TimeMapKind::Custom(custom_controls));
    }
    if strength == 0.0 || tm_type == "Linear" {
        return Ok(TimeMapKind::Linear);
    }
    match tm_type {
        "Quadratic" => Ok(TimeMapKind::Power(2, strength)),
        "Cubic" => Ok(TimeMapKind::Power(3, strength)),
        "Quartic" => Ok(TimeMapKind::Power(4, strength)),
        "Quintic" => Ok(TimeMapKind::Power(5, strength)),
        "DualQuadratic" => Ok(TimeMapKind::DualPower(2, strength)),
        "DualCubic" => Ok(TimeMapKind::DualPower(3, strength)),
        "DualQuartic" => Ok(TimeMapKind::DualPower(4, strength)),
        "DualQuintic" => Ok(TimeMapKind::DualPower(5, strength)),
        "Bounce" => Ok(TimeMapKind::Bounce(strength)),
        "BounceIn" => Ok(TimeMapKind::BounceIn(strength)),
        "Spring" => Ok(TimeMapKind::Spring(strength)),
        "Sine" | "SineWave" => Ok(TimeMapKind::SineWave(strength)),
        "SawtoothWave" => Ok(TimeMapKind::SawtoothWave(strength)),
        "SquareWave" => Ok(TimeMapKind::SquareWave(strength)),
        "DampedWave" => Ok(TimeMapKind::DampedWave(strength)),
        "Back" => Ok(TimeMapKind::Back(strength)),
        "Elastic" => Ok(TimeMapKind::Elastic(strength)),
        _ => Err("nonlinear or custom motion time map"),
    }
}

fn read_animation_core(root: Node<'_, '_>) -> Result<ParsedMotion, &'static str> {
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

    for desc in root.descendants().filter(Node::is_element) {
        if desc.tag_name().namespace() != root.tag_name().namespace() {
            return Err("unknown motion XML namespace");
        }
    }

    let mut time_maps = Vec::new();
    for tm_node in root
        .children()
        .filter(|c| c.is_element() && c.has_tag_name("TimeMap"))
    {
        time_maps.push(parse_time_map_node(tm_node, total)?);
    }

    let mut tracks = BTreeMap::new();
    let mut filters = Vec::new();
    let mut filter_approximated = false;
    let mut orient = false;
    let mut z_pivot_offset = 0.0;
    let mut skip_end = 0usize;

    for n in root.descendants().filter(Node::is_element) {
        if n.range().start < skip_end {
            continue;
        }
        if n.tag_name().namespace() != root.tag_name().namespace() {
            return Err("unknown motion XML namespace");
        }
        match n.tag_name().name() {
            "Settings" => {
                orient = matches!(n.attribute("orientToPath"), Some("1" | "true"));
                z_pivot_offset = n
                    .attribute("xformPtZOffsetPixels")
                    .and_then(number)
                    .unwrap_or(0.0);
            }
            "TimeMap" => {
                if n.parent() != Some(root) {
                    let _ = parse_time_map_node(n, total)?;
                }
                skip_end = n.range().end;
            }
            "PropertyContainer" if n.attribute("id") == Some("Filters") => {
                skip_end = n.range().end;
                for fc in n.children().filter(Node::is_element) {
                    if !fc.has_tag_name("PropertyContainer") {
                        continue;
                    }
                    if matches!(fc.attribute("enabled"), Some("0" | "false")) {
                        continue;
                    }
                    let mut group_tracks = BTreeMap::new();
                    let mut group_kind = fc
                        .attribute("id")
                        .and_then(|id| id.strip_suffix("_Filter"))
                        .unwrap_or("")
                        .to_owned();
                    for prop in fc
                        .descendants()
                        .filter(|e| e.is_element() && e.has_tag_name("Property"))
                    {
                        if matches!(prop.attribute("enabled"), Some("0" | "false")) {
                            continue;
                        }
                        let pid = prop
                            .attribute("id")
                            .ok_or("motion property without a name")?;
                        let ignore_tm =
                            matches!(prop.attribute("ignoreTimeMap"), Some("1" | "true"));
                        let tm_idx = prop
                            .attribute("TimeMapIndex")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        let tm = if ignore_tm {
                            None
                        } else {
                            time_maps.get(tm_idx).cloned()
                        };
                        let parsed = read_property_controls(prop, total)?;
                        if let Some((prefix, sub_name)) = is_filter_property_id(pid) {
                            if group_kind.is_empty() {
                                group_kind = prefix.to_owned();
                            }
                            insert_property_into_map(&mut group_tracks, sub_name, parsed, tm)?;
                        } else {
                            filter_approximated = true;
                        }
                    }
                    if !group_tracks.is_empty() {
                        let approx =
                            matches!(group_kind.as_str(), "GradientGlow" | "GradientBevel" | "");
                        if approx {
                            filter_approximated = true;
                        }
                        filters.push(FilterCurveGroup {
                            kind: group_kind,
                            tracks: group_tracks,
                            approximated: approx,
                        });
                    }
                }
            }
            "Property" => {
                if matches!(n.attribute("enabled"), Some("0" | "false")) {
                    skip_end = n.range().end;
                    continue;
                }
                let id = n.attribute("id").ok_or("motion property without a name")?;
                let ignore_tm = matches!(n.attribute("ignoreTimeMap"), Some("1" | "true"));
                let tm_idx = n
                    .attribute("TimeMapIndex")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let tm = if ignore_tm {
                    None
                } else {
                    time_maps.get(tm_idx).cloned()
                };
                let parsed = read_property_controls(n, total)?;
                let mapped_id = match id {
                    "Translation_Z" | "Depth" => "Motion_Z",
                    other => other,
                };
                if is_known_motion_property_id(id) {
                    insert_property_into_map(&mut tracks, mapped_id, parsed, tm)?;
                } else if let Some((prefix, sub_name)) = is_filter_property_id(id) {
                    let approx = matches!(prefix, "GradientGlow" | "GradientBevel");
                    if approx {
                        filter_approximated = true;
                    }
                    if let Some(existing) = filters.iter_mut().find(|g| g.kind == prefix) {
                        insert_property_into_map(&mut existing.tracks, sub_name, parsed, tm)?;
                    } else {
                        let mut group_tracks = BTreeMap::new();
                        insert_property_into_map(&mut group_tracks, sub_name, parsed, tm)?;
                        filters.push(FilterCurveGroup {
                            kind: prefix.to_owned(),
                            tracks: group_tracks,
                            approximated: approx,
                        });
                    }
                } else {
                    return Err("unknown enabled motion property");
                }
            }
            "AnimationCore" | "metadata" | "names" | "name" | "PropertyContainer" | "Keyframe" => {}
            _ => return Err("unknown motion XML element"),
        }
    }

    redistribute_roving_times(&mut tracks);

    if tracks.is_empty() && filters.is_empty() {
        return Err("no supported 2D motion tracks");
    }

    Ok(ParsedMotion {
        total,
        tracks,
        filters,
        orient,
        z_pivot_offset,
        fallback_pivot: None,
        filter_approximated,
    })
}

fn parse_fl_motion_tweens(tweens_node: Node<'_, '_>) -> Result<Option<SegmentEase>, &'static str> {
    let mut found = None;
    for ch in tweens_node.children().filter(Node::is_element) {
        match ch.tag_name().name() {
            "SimpleEase" => {
                if ch.children().any(|c| c.is_element()) {
                    return Err("unknown motion XML element");
                }
                let raw = ch.attribute("ease").and_then(number).unwrap_or(0.0);
                let scaled = if raw.abs() <= 1.0 { raw * 100.0 } else { raw };
                if found.is_none() {
                    found = Some(SegmentEase::Flash(scaled.clamp(-100.0, 100.0)));
                }
            }
            "Ease" => {
                if ch.children().any(|c| c.is_element()) {
                    return Err("unknown motion XML element");
                }
                let method = ch.attribute("method").unwrap_or("Quadratic").to_owned();
                let intensity = ch
                    .attribute("intensity")
                    .or_else(|| ch.attribute("strength"))
                    .and_then(number)
                    .map(|v| {
                        if v.abs() <= 1.0 && v != 0.0 {
                            v * 100.0
                        } else {
                            v
                        }
                    })
                    .unwrap_or(100.0);
                if found.is_none() {
                    found = Some(SegmentEase::Named(method, intensity));
                }
            }
            "CustomEase" => {
                let mut pts = Vec::new();
                for p in ch.children().filter(Node::is_element) {
                    if !p.has_tag_name("Point") || p.children().any(|c| c.is_element()) {
                        return Err("unknown motion XML element");
                    }
                    if let (Some(x), Some(y)) = (
                        p.attribute("x").unwrap_or("0").parse::<f64>().ok(),
                        p.attribute("y").unwrap_or("0").parse::<f64>().ok(),
                    ) {
                        pts.push([x, y]);
                    }
                }
                if pts.len() >= 4 && (pts.len() - 1).is_multiple_of(3) && found.is_none() {
                    found = Some(SegmentEase::CubicPoints(pts));
                }
            }
            _ => return Err("unknown motion XML element"),
        }
    }
    Ok(found)
}

fn validate_fl_motion_tree(root: Node<'_, '_>) -> Result<(), &'static str> {
    for desc in root.descendants().filter(Node::is_element) {
        if desc.tag_name().namespace() != root.tag_name().namespace() {
            return Err("unknown motion XML namespace");
        }
    }
    for ch in root.children().filter(Node::is_element) {
        match ch.tag_name().name() {
            "source" => {
                for src in ch.children().filter(Node::is_element) {
                    if !src.has_tag_name("Source") {
                        return Err("unknown motion XML element");
                    }
                    for sc in src.children().filter(Node::is_element) {
                        match sc.tag_name().name() {
                            "dimensions" => {
                                if sc.children().filter(Node::is_element).any(|r| {
                                    !r.has_tag_name("GeomRectangle")
                                        || r.children().any(|c| c.is_element())
                                }) {
                                    return Err("unknown motion XML element");
                                }
                            }
                            "transformationPoint" => {
                                if sc.children().filter(Node::is_element).any(|p| {
                                    !p.has_tag_name("Point") || p.children().any(|c| c.is_element())
                                }) {
                                    return Err("unknown motion XML element");
                                }
                            }
                            "color" => {
                                if sc.children().filter(Node::is_element).any(|c| {
                                    !c.has_tag_name("Color") || c.children().any(|d| d.is_element())
                                }) {
                                    return Err("unknown motion XML element");
                                }
                            }
                            _ => return Err("unknown motion XML element"),
                        }
                    }
                }
            }
            "tweens" => {}
            "TimeMap" => {
                for tc in ch.descendants().skip(1).filter(Node::is_element) {
                    if !matches!(
                        tc.tag_name().name(),
                        "Property" | "PropertyContainer" | "Keyframe"
                    ) {
                        return Err("unknown motion XML element");
                    }
                }
            }
            "Keyframe" => {
                for kc in ch.children().filter(Node::is_element) {
                    match kc.tag_name().name() {
                        "tweens" => {}
                        "color" => {
                            if kc.children().filter(Node::is_element).any(|c| {
                                !c.has_tag_name("Color") || c.children().any(|d| d.is_element())
                            }) {
                                return Err("unknown motion XML element");
                            }
                        }
                        "filters" => {
                            if kc.children().filter(Node::is_element).any(|f| {
                                !matches!(
                                    f.tag_name().name(),
                                    "BlurFilter"
                                        | "GlowFilter"
                                        | "DropShadowFilter"
                                        | "BevelFilter"
                                        | "GradientGlowFilter"
                                        | "GradientBevelFilter"
                                        | "AdjustColorFilter"
                                ) || f.children().any(|c| c.is_element())
                            }) {
                                return Err("unknown motion XML element");
                            }
                        }
                        _ => return Err("unknown motion XML element"),
                    }
                }
            }
            _ => return Err("unknown motion XML element"),
        }
    }
    Ok(())
}

fn read_fl_motion(root: Node<'_, '_>, span_duration: u32) -> Result<ParsedMotion, &'static str> {
    if root.range().len() > MAX_XML_BYTES
        || root.descendants().take(MAX_XML_NODES as usize + 1).count() > MAX_XML_NODES as usize
    {
        return Err("motion XML exceeds the sampling limit");
    }
    validate_fl_motion_tree(root)?;
    let dur_frames = root
        .attribute("duration")
        .and_then(number)
        .unwrap_or_else(|| f64::from(span_duration))
        .max(2.0);
    let total = (dur_frames - 1.0).max(1.0);

    let mut fallback_pivot = None;
    let mut orient = matches!(root.attribute("orientToPath"), Some("1" | "true"));
    let mut global_ease: Option<SegmentEase> = None;
    let mut time_map: Option<TimeMapKind> = None;
    let mut filter_approximated = false;

    if let Some(src_wrap) = root
        .children()
        .find(|c| c.is_element() && c.has_tag_name("source"))
        && let Some(src) = src_wrap
            .children()
            .find(|c| c.is_element() && c.has_tag_name("Source"))
    {
        if matches!(src.attribute("orientToPath"), Some("1" | "true")) {
            orient = true;
        }
        let mut dims = [0.0, 0.0, 0.0, 0.0];
        if let Some(dim_wrap) = src
            .children()
            .find(|c| c.is_element() && c.has_tag_name("dimensions"))
            && let Some(rect) = dim_wrap.children().find(Node::is_element)
        {
            dims = [
                rect.attribute("left").and_then(number).unwrap_or(0.0),
                rect.attribute("top").and_then(number).unwrap_or(0.0),
                rect.attribute("width").and_then(number).unwrap_or(0.0),
                rect.attribute("height").and_then(number).unwrap_or(0.0),
            ];
        }
        if let Some(tp_wrap) = src
            .children()
            .find(|c| c.is_element() && c.has_tag_name("transformationPoint"))
            && let Some(pt) = tp_wrap
                .children()
                .find(|c| c.is_element() && c.has_tag_name("Point"))
            && let (Some(px), Some(py)) = (
                pt.attribute("x").and_then(number),
                pt.attribute("y").and_then(number),
            )
        {
            if dims[2] > 0.0 || dims[3] > 0.0 {
                fallback_pivot = Some([dims[0] + px * dims[2], dims[1] + py * dims[3]]);
            } else {
                fallback_pivot = Some([px, py]);
            }
        }
    }

    for ch in root.children().filter(Node::is_element) {
        if ch.has_tag_name("tweens") {
            global_ease = parse_fl_motion_tweens(ch)?;
        } else if ch.has_tag_name("TimeMap") {
            time_map = Some(parse_time_map_node(ch, total)?);
        }
    }

    let mut tracks: BTreeMap<String, Track> = BTreeMap::new();
    let push_kf_val = |tracks: &mut BTreeMap<String, Track>,
                       name: &str,
                       time: f64,
                       val: f64,
                       roving: bool,
                       seg_ease: Option<SegmentEase>,
                       tm: Option<TimeMapKind>| {
        let entry = tracks.entry(name.to_owned()).or_insert_with(|| Track {
            controls: Vec::new(),
            time_map: tm,
        });
        entry.controls.push(Control {
            time,
            anchor: [0.0, val],
            next: [0.0, val],
            prev: [0.0, val],
            roving,
            seg_ease,
        });
    };

    let kf_nodes: Vec<_> = root
        .children()
        .filter(|c| c.is_element() && c.has_tag_name("Keyframe"))
        .collect();
    if kf_nodes.is_empty() {
        return Err("no supported 2D motion tracks");
    }

    for kf in kf_nodes {
        let idx = kf
            .attribute("index")
            .and_then(number)
            .ok_or("invalid motion key time")?;
        if idx < 0.0 || idx > total {
            return Err("motion key is outside its span");
        }
        let roving = matches!(kf.attribute("roving"), Some("1" | "true"));
        if matches!(kf.attribute("orientToPath"), Some("1" | "true")) {
            orient = true;
        }
        let kf_ease = match kf
            .children()
            .find(|c| c.is_element() && c.has_tag_name("tweens"))
        {
            Some(tw) => parse_fl_motion_tweens(tw)?,
            None => None,
        }
        .or_else(|| global_ease.clone());

        for (attr, track_id, scale) in [
            ("x", "Motion_X", 1.0),
            ("y", "Motion_Y", 1.0),
            ("z", "Motion_Z", 1.0),
            ("scaleX", "Scale_X", 100.0),
            ("scaleY", "Scale_Y", 100.0),
            ("scaleZ", "Scale_Z", 100.0),
            ("skewX", "Skew_X", 1.0),
            ("skewY", "Skew_Y", 1.0),
            ("rotation", "Rotation_Z", 1.0),
            ("rotationZ", "Rotation_Z", 1.0),
            ("rotationX", "Rotation_X", 1.0),
            ("rotationY", "Rotation_Y", 1.0),
        ] {
            if let Some(raw) = kf.attribute(attr) {
                let v = number(raw).ok_or("invalid motion anchor")? * scale;
                push_kf_val(
                    &mut tracks,
                    track_id,
                    idx,
                    v,
                    roving && matches!(track_id, "Motion_X" | "Motion_Y" | "Motion_Z"),
                    kf_ease.clone(),
                    time_map.clone(),
                );
            }
        }

        if let Some(col_wrap) = kf
            .children()
            .find(|c| c.is_element() && c.has_tag_name("color"))
            && let Some(col) = col_wrap
                .children()
                .find(|c| c.is_element() && c.has_tag_name("Color"))
        {
            if col.attribute("redMultiplier").is_some()
                || col.attribute("greenMultiplier").is_some()
                || col.attribute("blueMultiplier").is_some()
                || col.attribute("redOffset").is_some()
                || col.attribute("greenOffset").is_some()
                || col.attribute("blueOffset").is_some()
                || col.attribute("alphaOffset").is_some()
            {
                for (attr, tid, def, mul) in [
                    ("redMultiplier", "AdvClr_R_Pct", 1.0, 100.0),
                    ("greenMultiplier", "AdvClr_G_Pct", 1.0, 100.0),
                    ("blueMultiplier", "AdvClr_B_Pct", 1.0, 100.0),
                    ("alphaMultiplier", "AdvClr_A_Pct", 1.0, 100.0),
                    ("redOffset", "AdvClr_R_Offset", 0.0, 1.0),
                    ("greenOffset", "AdvClr_G_Offset", 0.0, 1.0),
                    ("blueOffset", "AdvClr_B_Offset", 0.0, 1.0),
                    ("alphaOffset", "AdvClr_A_Offset", 0.0, 1.0),
                ] {
                    let v = col.attribute(attr).and_then(number).unwrap_or(def) * mul;
                    push_kf_val(
                        &mut tracks,
                        tid,
                        idx,
                        v,
                        false,
                        kf_ease.clone(),
                        time_map.clone(),
                    );
                }
            } else if col.attribute("tintMultiplier").is_some()
                || col.attribute("tintColor").is_some()
            {
                let amt = col
                    .attribute("tintMultiplier")
                    .and_then(number)
                    .map(|v| if v.abs() <= 1.0 { v * 100.0 } else { v })
                    .unwrap_or(0.0);
                let rgba = col
                    .attribute("tintColor")
                    .and_then(parse_rgba_hex)
                    .unwrap_or([0.0, 0.0, 0.0, 255.0]);
                push_kf_val(
                    &mut tracks,
                    "Tint_Amount",
                    idx,
                    amt,
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
                push_kf_val(
                    &mut tracks,
                    "Tint_Color.r",
                    idx,
                    rgba[0],
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
                push_kf_val(
                    &mut tracks,
                    "Tint_Color.g",
                    idx,
                    rgba[1],
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
                push_kf_val(
                    &mut tracks,
                    "Tint_Color.b",
                    idx,
                    rgba[2],
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
            } else if let Some(br_raw) = col.attribute("brightness").and_then(number) {
                let br = if br_raw.abs() <= 1.0 {
                    br_raw * 100.0
                } else {
                    br_raw
                };
                push_kf_val(
                    &mut tracks,
                    "Brightness_Amount",
                    idx,
                    br,
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
            } else if let Some(am_raw) = col.attribute("alphaMultiplier").and_then(number) {
                let am = if (0.0..=1.0).contains(&am_raw) {
                    am_raw * 100.0
                } else {
                    am_raw
                };
                push_kf_val(
                    &mut tracks,
                    "Alpha_Amount",
                    idx,
                    am,
                    false,
                    kf_ease.clone(),
                    time_map.clone(),
                );
            }
        }

        if kf
            .children()
            .any(|c| c.is_element() && c.has_tag_name("filters"))
        {
            filter_approximated = true;
        }
    }

    for (tid, track) in &mut tracks {
        track.controls.sort_by(|a, b| a.time.total_cmp(&b.time));
        if track.controls.windows(2).any(|w| w[0].time >= w[1].time) {
            return Err("nonmonotone motion handles");
        }
        if track.controls.first().is_some_and(|c| c.time > 0.0) {
            let neutral = match tid.as_str() {
                "Scale_X" | "Scale_Y" | "Scale_Z" | "Alpha_Amount" | "AdvClr_R_Pct"
                | "AdvClr_G_Pct" | "AdvClr_B_Pct" | "AdvClr_A_Pct" => 100.0,
                _ => 0.0,
            };
            track.controls.insert(
                0,
                Control {
                    time: 0.0,
                    anchor: [0.0, neutral],
                    next: [0.0, neutral],
                    prev: [0.0, neutral],
                    roving: false,
                    seg_ease: global_ease.clone(),
                },
            );
        }
    }

    redistribute_roving_times(&mut tracks);

    Ok(ParsedMotion {
        total,
        tracks,
        filters: Vec::new(),
        orient,
        z_pivot_offset: 0.0,
        fallback_pivot,
        filter_approximated,
    })
}

fn read_motion(node: Node<'_, '_>, duration: u32) -> Result<ParsedMotion, &'static str> {
    let target = match node.tag_name().name() {
        "MotionObjectXML" | "motionObjectXML" | "DOMMotionObject" => {
            let elems: Vec<_> = node.children().filter(Node::is_element).collect();
            if elems.is_empty() {
                return Err("missing AnimationCore");
            }
            if elems.len() > 1 {
                return Err("unknown motion XML element");
            }
            let child = elems[0];
            if !child.has_tag_name("AnimationCore") && !child.has_tag_name("Motion") {
                return Err("missing AnimationCore");
            }
            child
        }
        _ => node,
    };
    if target.has_tag_name("AnimationCore") {
        read_animation_core(target)
    } else if target.has_tag_name("Motion") {
        read_fl_motion(target, duration)
    } else {
        Err("unknown motion data version")
    }
}

fn rotation_3d_matrix(rx_deg: f64, ry_deg: f64, rz_deg: f64) -> [[f64; 3]; 3] {
    let (sx, cx) = rx_deg.to_radians().sin_cos();
    let (sy, cy) = ry_deg.to_radians().sin_cos();
    let (sz, cz) = rz_deg.to_radians().sin_cos();
    let mx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let my = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let mz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| -> [[f64; 3]; 3] {
        std::array::from_fn(|r| {
            std::array::from_fn(|c| a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c])
        })
    };
    mul(mz, mul(my, mx))
}

#[allow(clippy::too_many_arguments)]
fn project_3d_pose_to_2d(
    world_pivot: [f64; 2],
    local_pivot: [f64; 2],
    z_pivot_offset: f64,
    motion_z: f64,
    scale_x: f64,
    scale_y: f64,
    skew_x_deg: f64,
    skew_y_deg: f64,
    rx_deg: f64,
    ry_deg: f64,
    rz_deg: f64,
    ctx: &MotionContext,
) -> Option<Transform> {
    let angle = ctx
        .view_angle_3d
        .filter(|a| a.is_finite() && *a > 0.0 && *a < 180.0)
        .unwrap_or(DEFAULT_VIEW_ANGLE_3D_DEG);
    let stage_w = ctx.stage_size[0].max(1.0);
    let stage_h = ctx.stage_size[1].max(1.0);
    let focal = (stage_w * 0.5) / (angle.to_radians() * 0.5).tan();
    let center = ctx
        .vanishing_point_3d
        .unwrap_or([stage_w * 0.5, stage_h * 0.5]);

    let (ssy, csy) = skew_y_deg.to_radians().sin_cos();
    let (ssx, csx) = skew_x_deg.to_radians().sin_cos();
    let l00 = scale_x * csy;
    let l10 = scale_x * ssy;
    let l01 = -scale_y * ssx;
    let l11 = scale_y * csx;

    let r = rotation_3d_matrix(rx_deg, ry_deg, rz_deg);
    // Offset from 3D pivot (px, py, pz) to local plane (px, py, 0) is (0, 0, -pz)
    let p0_x = world_pivot[0] - r[0][2] * z_pivot_offset;
    let p0_y = world_pivot[1] - r[1][2] * z_pivot_offset;
    let p0_z = motion_z + z_pivot_offset - r[2][2] * z_pivot_offset;

    let depth = (focal + p0_z).max(focal * 0.05);
    let k = focal / depth;
    let vx = p0_x - center[0];
    let vy = p0_y - center[1];

    // Columns of R * [L; 0]
    let cu = [
        r[0][0] * l00 + r[0][1] * l10,
        r[1][0] * l00 + r[1][1] * l10,
        r[2][0] * l00 + r[2][1] * l10,
    ];
    let cv = [
        r[0][0] * l01 + r[0][1] * l11,
        r[1][0] * l01 + r[1][1] * l11,
        r[2][0] * l01 + r[2][1] * l11,
    ];

    let mut a = k * (cu[0] - vx * cu[2] / depth);
    let mut b = k * (cu[1] - vy * cu[2] / depth);
    let mut c = k * (cv[0] - vx * cv[2] / depth);
    let mut d = k * (cv[1] - vy * cv[2] / depth);

    // Prevent edge-on 90-degree singularity so Transform::from_matrix stays valid.
    if a.hypot(b) < 1e-2 {
        let dir = rz_deg.to_radians();
        a = 1e-2 * dir.cos();
        b = 1e-2 * dir.sin();
    }
    if c.hypot(d) < 1e-2 {
        let dir = rz_deg.to_radians();
        c = -1e-2 * dir.sin();
        d = 1e-2 * dir.cos();
    }
    if (a * d - b * c).abs() < 1e-4 {
        let len_u = a.hypot(b).max(1e-2);
        c += (-b / len_u) * 1e-2;
        d += (a / len_u) * 1e-2;
    }

    let sx_piv = center[0] + vx * k;
    let sy_piv = center[1] + vy * k;
    let tx = sx_piv - a * local_pivot[0] - c * local_pivot[1];
    let ty = sy_piv - b * local_pivot[0] - d * local_pivot[1];

    Transform::from_matrix([a, b, c, d, tx, ty])
}

fn evaluate_color_tracks(
    tracks: &BTreeMap<String, Track>,
    t: f64,
    base_alpha: f64,
    base_ct: ColorTransform,
) -> Option<(f64, ColorTransform)> {
    let sample = |id: &str, fallback: f64| tracks.get(id).map_or(fallback, |tr| eval_track(tr, t));
    let has_adv = [
        "AdvClr_R_Pct",
        "AdvClr_G_Pct",
        "AdvClr_B_Pct",
        "AdvClr_A_Pct",
        "AdvClr_R_Offset",
        "AdvClr_G_Offset",
        "AdvClr_B_Offset",
        "AdvClr_A_Offset",
    ]
    .iter()
    .any(|k| tracks.contains_key(*k));
    let has_tint = tracks.contains_key("Tint_Amount") || tracks.contains_key("Tint_Color.r");
    let has_bright = tracks.contains_key("Brightness_Amount");
    let has_alpha = tracks.contains_key("Alpha_Amount");

    if !has_adv && !has_tint && !has_bright && !has_alpha {
        return None;
    }

    let (rm, gm, bm, am, ro, go, bo, ao) = if has_adv {
        (
            sample("AdvClr_R_Pct", 100.0) / 100.0,
            sample("AdvClr_G_Pct", 100.0) / 100.0,
            sample("AdvClr_B_Pct", 100.0) / 100.0,
            sample("AdvClr_A_Pct", 100.0) / 100.0,
            sample("AdvClr_R_Offset", 0.0),
            sample("AdvClr_G_Offset", 0.0),
            sample("AdvClr_B_Offset", 0.0),
            sample("AdvClr_A_Offset", 0.0),
        )
    } else if has_tint {
        let amt = (sample("Tint_Amount", 100.0) / 100.0).clamp(0.0, 1.0);
        let m = 1.0 - amt;
        let am_cur = if has_alpha {
            sample("Alpha_Amount", 100.0) / 100.0
        } else {
            base_alpha
        };
        (
            m,
            m,
            m,
            am_cur,
            sample("Tint_Color.r", 0.0).clamp(0.0, 255.0) * amt,
            sample("Tint_Color.g", 0.0).clamp(0.0, 255.0) * amt,
            sample("Tint_Color.b", 0.0).clamp(0.0, 255.0) * amt,
            base_ct.offsets[3],
        )
    } else if has_bright {
        let br = (sample("Brightness_Amount", 0.0) / 100.0).clamp(-1.0, 1.0);
        let m = 1.0 - br.abs();
        let off = if br > 0.0 { 255.0 * br } else { 0.0 };
        let am_cur = if has_alpha {
            sample("Alpha_Amount", 100.0) / 100.0
        } else {
            base_alpha
        };
        (m, m, m, am_cur, off, off, off, base_ct.offsets[3])
    } else {
        (
            base_ct.multipliers[0],
            base_ct.multipliers[1],
            base_ct.multipliers[2],
            sample("Alpha_Amount", 100.0) / 100.0,
            base_ct.offsets[0],
            base_ct.offsets[1],
            base_ct.offsets[2],
            base_ct.offsets[3],
        )
    };

    let (alpha_out, mult_a) = if (0.0..=1.0).contains(&am) {
        (am, 1.0)
    } else {
        (1.0, am.clamp(-10_000.0, 10_000.0))
    };

    Some((
        alpha_out,
        ColorTransform {
            multipliers: [
                rm.clamp(-10_000.0, 10_000.0),
                gm.clamp(-10_000.0, 10_000.0),
                bm.clamp(-10_000.0, 10_000.0),
                mult_a,
            ],
            offsets: [
                ro.clamp(-255.0, 255.0),
                go.clamp(-255.0, 255.0),
                bo.clamp(-255.0, 255.0),
                ao.clamp(-255.0, 255.0),
            ],
        },
    ))
}

fn evaluate_filter_groups(
    groups: &[FilterCurveGroup],
    t: f64,
    base_filters: &[FilterSpec],
) -> Vec<FilterSpec> {
    if groups.is_empty() {
        return base_filters.to_vec();
    }
    let mut out = base_filters.to_vec();
    let mut used = vec![false; out.len()];

    for grp in groups {
        let num = |k: &str, fb: f64| grp.tracks.get(k).map_or(fb, |tr| eval_track(tr, t));
        let flag = |k: &str, fb: bool| grp.tracks.get(k).map_or(fb, |tr| eval_track(tr, t) != 0.0);
        let rgb_alpha = |k: &str, fb_col: u32, fb_alpha: f64| -> (u32, f64) {
            let rk = format!("{k}.r");
            if grp.tracks.contains_key(&rk) {
                let r = num(&rk, f64::from((fb_col >> 16) & 0xff))
                    .round()
                    .clamp(0.0, 255.0) as u32;
                let g = num(&format!("{k}.g"), f64::from((fb_col >> 8) & 0xff))
                    .round()
                    .clamp(0.0, 255.0) as u32;
                let b = num(&format!("{k}.b"), f64::from(fb_col & 0xff))
                    .round()
                    .clamp(0.0, 255.0) as u32;
                let a = (num(&format!("{k}.a"), fb_alpha * 255.0) / 255.0).clamp(0.0, 1.0);
                ((r << 16) | (g << 8) | b, a)
            } else {
                (fb_col, fb_alpha)
            }
        };

        let push_or_replace =
            |out: &mut Vec<FilterSpec>, used: &mut Vec<bool>, idx: Option<usize>, f: FilterSpec| {
                if let Some(i) = idx {
                    let keep_en = out[i].is_enabled();
                    out[i] = f.with_enabled(keep_en);
                    used[i] = true;
                } else if out.len() < MAX_OBJECT_FILTERS {
                    out.push(f);
                    used.push(true);
                }
            };

        match grp.kind.as_str() {
            "Blur" => {
                let idx = out
                    .iter()
                    .enumerate()
                    .position(|(i, f)| !used[i] && matches!(f, FilterSpec::Blur { .. }));
                let base_b = idx.and_then(|i| match &out[i] {
                    FilterSpec::Blur { blur_x, blur_y, .. } => {
                        Some((*blur_x, *blur_y, out[i].quality_level()))
                    }
                    _ => None,
                });
                let f = FilterSpec::Blur {
                    blur_x: num("BlurX", base_b.map_or(5.0, |v| v.0)).clamp(0.0, 255.0),
                    blur_y: num("BlurY", base_b.map_or(5.0, |v| v.1)).clamp(0.0, 255.0),
                    quality: num("Quality", f64::from(base_b.map_or(1, |v| v.2)))
                        .round()
                        .clamp(1.0, 15.0) as u8,
                };
                push_or_replace(&mut out, &mut used, idx, f);
            }
            "Glow" => {
                let idx = out
                    .iter()
                    .enumerate()
                    .position(|(i, f)| !used[i] && matches!(f, FilterSpec::Glow { .. }));
                let (def_col, def_a, def_bx, def_by, def_s, def_q, def_in, def_ko) =
                    match idx.map(|i| (&out[i], out[i].quality_level())) {
                        Some((
                            FilterSpec::Glow {
                                color,
                                alpha,
                                blur_x,
                                blur_y,
                                strength,
                                inner,
                                knockout,
                                ..
                            },
                            ql,
                        )) => (
                            *color,
                            *alpha,
                            *blur_x,
                            *blur_y,
                            *strength * 100.0,
                            ql,
                            *inner,
                            *knockout,
                        ),
                        _ => (0xff0000, 1.0, 5.0, 5.0, 100.0, 1, false, false),
                    };
                let (color, alpha) = rgb_alpha("Color", def_col, def_a);
                let f = FilterSpec::Glow {
                    color,
                    alpha,
                    blur_x: num("BlurX", def_bx).clamp(0.0, 255.0),
                    blur_y: num("BlurY", def_by).clamp(0.0, 255.0),
                    strength: (num("Strength", def_s) / 100.0).clamp(0.0, 255.0),
                    quality: num("Quality", f64::from(def_q)).round().clamp(1.0, 15.0) as u8,
                    inner: flag("InnerGlow", def_in),
                    knockout: flag("Knockout", def_ko),
                };
                push_or_replace(&mut out, &mut used, idx, f);
            }
            "DropShadow" => {
                let idx = out
                    .iter()
                    .enumerate()
                    .position(|(i, f)| !used[i] && matches!(f, FilterSpec::DropShadow { .. }));
                let (
                    def_col,
                    def_a,
                    def_bx,
                    def_by,
                    def_ang,
                    def_dist,
                    def_s,
                    def_q,
                    def_in,
                    def_ko,
                    def_ho,
                ) = match idx.map(|i| (&out[i], out[i].quality_level())) {
                    Some((
                        FilterSpec::DropShadow {
                            color,
                            alpha,
                            blur_x,
                            blur_y,
                            angle,
                            distance,
                            strength,
                            inner,
                            knockout,
                            hide_object,
                            ..
                        },
                        ql,
                    )) => (
                        *color,
                        *alpha,
                        *blur_x,
                        *blur_y,
                        *angle,
                        *distance,
                        *strength * 100.0,
                        ql,
                        *inner,
                        *knockout,
                        *hide_object,
                    ),
                    _ => (
                        0x000000, 1.0, 5.0, 5.0, 45.0, 5.0, 100.0, 1, false, false, false,
                    ),
                };
                let (color, alpha) = rgb_alpha("Color", def_col, def_a);
                let f = FilterSpec::DropShadow {
                    distance: num("Distance", def_dist).clamp(-255.0, 255.0),
                    angle: num("Angle", def_ang).clamp(-360.0, 360.0),
                    color,
                    alpha,
                    blur_x: num("BlurX", def_bx).clamp(0.0, 255.0),
                    blur_y: num("BlurY", def_by).clamp(0.0, 255.0),
                    strength: (num("Strength", def_s) / 100.0).clamp(0.0, 255.0),
                    quality: num("Quality", f64::from(def_q)).round().clamp(1.0, 15.0) as u8,
                    inner: flag("InnerShadow", def_in),
                    knockout: flag("Knockout", def_ko),
                    hide_object: flag("HideObject", def_ho),
                };
                push_or_replace(&mut out, &mut used, idx, f);
            }
            "Bevel" => {
                let idx = out
                    .iter()
                    .enumerate()
                    .position(|(i, f)| !used[i] && matches!(f, FilterSpec::Bevel { .. }));
                let (
                    def_hc,
                    def_ha,
                    def_sc,
                    def_sa,
                    def_bx,
                    def_by,
                    def_ang,
                    def_dist,
                    def_s,
                    def_q,
                    def_pl,
                    def_ko,
                ) = match idx.map(|i| (&out[i], out[i].quality_level())) {
                    Some((
                        FilterSpec::Bevel {
                            highlight_color,
                            highlight_alpha,
                            shadow_color,
                            shadow_alpha,
                            blur_x,
                            blur_y,
                            angle,
                            distance,
                            strength,
                            placement,
                            knockout,
                            ..
                        },
                        ql,
                    )) => (
                        *highlight_color,
                        *highlight_alpha,
                        *shadow_color,
                        *shadow_alpha,
                        *blur_x,
                        *blur_y,
                        *angle,
                        *distance,
                        *strength * 100.0,
                        ql,
                        placement.clone(),
                        *knockout,
                    ),
                    _ => (
                        0xffffff,
                        1.0,
                        0x000000,
                        1.0,
                        5.0,
                        5.0,
                        45.0,
                        5.0,
                        100.0,
                        1,
                        "inner".to_owned(),
                        false,
                    ),
                };
                let (highlight_color, highlight_alpha) = rgb_alpha("HilightColor", def_hc, def_ha);
                let (shadow_color, shadow_alpha) = rgb_alpha("ShadowColor", def_sc, def_sa);
                let f = FilterSpec::Bevel {
                    distance: num("Distance", def_dist).clamp(-255.0, 255.0),
                    angle: num("Angle", def_ang).clamp(-360.0, 360.0),
                    highlight_color,
                    highlight_alpha,
                    shadow_color,
                    shadow_alpha,
                    blur_x: num("BlurX", def_bx).clamp(0.0, 255.0),
                    blur_y: num("BlurY", def_by).clamp(0.0, 255.0),
                    strength: (num("Strength", def_s) / 100.0).clamp(0.0, 255.0),
                    quality: num("Quality", f64::from(def_q)).round().clamp(1.0, 15.0) as u8,
                    placement: def_pl,
                    knockout: flag("Knockout", def_ko),
                };
                push_or_replace(&mut out, &mut used, idx, f);
            }
            "AdjColor" | "AdjustColor" => {
                let idx = out
                    .iter()
                    .enumerate()
                    .position(|(i, f)| !used[i] && matches!(f, FilterSpec::ColorMatrix { .. }));
                let b = num("Brightness", 0.0).clamp(-100.0, 100.0);
                let c = num("Contrast", 0.0).clamp(-100.0, 100.0);
                let s = num("Saturation", 0.0).clamp(-100.0, 100.0);
                let h = num("Hue", 0.0).clamp(-180.0, 180.0);
                let f = FilterSpec::ColorMatrix {
                    matrix: adjust_color_matrix(b, c, s, h),
                };
                push_or_replace(&mut out, &mut used, idx, f);
            }
            _ => {}
        }
    }
    out
}

/// Sample CS4+ `AnimationCore` or `fl.motion` `<Motion>` tracks across `duration` frames.
/// The first pose is already present in `key`; `start_element` is `Some` only when frame 0's
/// color/3D/filter track overrides the stored element on `key`.
pub(crate) fn sample_poses(
    root: Node<'_, '_>,
    key: &Keyframe,
    duration: u32,
    mut ctx: MotionContext,
    budget: &mut usize,
) -> Result<SampledMotion, &'static str> {
    let count = duration.saturating_sub(1) as usize;
    if count == 0 || count > *budget || count > 4095 {
        return Err("motion span duration exceeds the available sampling range");
    }
    let [object] = key.elements.as_slice() else {
        return Err("motion span needs one placed instance");
    };
    if !matches!(
        object.kind,
        ElementKind::Instance { .. }
            | ElementKind::Bitmap { .. }
            | ElementKind::TextBox { .. }
            | ElementKind::Text { .. }
            | ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
    ) || !object.scripts.is_empty()
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
    let size = serde_json::to_vec(object)
        .map_err(|_| "unserializable motion placement")?
        .len();
    let cost = count.saturating_mul(size.div_ceil(2048).max(1));
    if cost > *budget {
        return Err("motion poses exceed the shared document sampling budget");
    }

    let ParsedMotion {
        total,
        tracks,
        filters,
        orient,
        z_pivot_offset,
        fallback_pivot,
        filter_approximated,
    } = read_motion(root, duration)?;

    if ctx.pivot.is_none() {
        ctx.pivot = fallback_pivot;
    }

    let sample = |id: &str, time: f64, fallback: f64| {
        tracks.get(id).map_or(fallback, |tr| eval_track(tr, time))
    };

    let sx0 = sample("Scale_X", 0., 100.);
    let sy0 = sample("Scale_Y", 0., 100.);
    let sz0 = sample("Scale_Z", 0., 100.);
    if sx0.abs() < 1e-9 || sy0.abs() < 1e-9 || sz0.abs() < 1e-9 {
        return Err("singular initial motion scale");
    }

    let track_has_nonzero = |id: &str, neutral: f64| -> bool {
        tracks.get(id).is_some_and(|t| {
            t.controls.iter().any(|c| {
                (c.anchor[1] - neutral).abs() > 1e-7
                    || (c.next[1] - neutral).abs() > 1e-7
                    || (c.prev[1] - neutral).abs() > 1e-7
            })
        })
    };

    let used_3d = z_pivot_offset.abs() > 1e-7
        || track_has_nonzero("Rotation_X", 0.0)
        || track_has_nonzero("Rotation_Y", 0.0)
        || track_has_nonzero("Motion_Z", 0.0)
        || track_has_nonzero("Scale_Z", 100.0);

    let step = if root.has_tag_name("Motion")
        || root
            .children()
            .any(|c| c.is_element() && c.has_tag_name("Motion"))
    {
        total / f64::from(duration.saturating_sub(1).max(1))
    } else {
        total / f64::from(duration)
    };

    let pos = |t: f64| [sample("Motion_X", t, 0.), sample("Motion_Y", t, 0.)];
    let origin = pos(0.);
    let max_tangent_t = (total - step).max(0.0);
    let tangent = |t: f64| {
        let a = pos((t - step / 1024.).max(0.));
        let b = pos((t + step / 1024.).min(max_tangent_t.max(step / 512.)));
        let d = [b[0] - a[0], b[1] - a[1]];
        (d[0].hypot(d[1]) > 1e-9).then(|| d[1].atan2(d[0]).to_degrees())
    };

    let mut heading = tangent(0.).unwrap_or(0.);
    let initial_heading = heading;
    let rotation0 = sample("Rotation_Z", 0., 0.);
    let skew_x0 = sample("Skew_X", 0., 0.);
    let skew_y0 = sample("Skew_Y", 0., 0.);
    let initial = object.transform;
    let base = transform_matrix(initial);

    let eval_element_at = |frame: u32, heading_state: &mut f64| -> Result<Element, &'static str> {
        let t = step * f64::from(frame);
        let xy = pos(t);
        let mut element = object.clone();
        if orient
            && frame > 0
            && let Some(direction) = tangent(t)
        {
            *heading_state = direction;
        }
        let orient_delta = if orient && frame > 0 {
            *heading_state - initial_heading
        } else {
            0.0
        };
        let drot = (sample("Rotation_Z", t, 0.) - rotation0) + orient_delta;
        let dsx = sample("Skew_X", t, 0.) - skew_x0;
        let dsy = sample("Skew_Y", t, 0.) - skew_y0;
        let scale_x = initial.scale_x * (sample("Scale_X", t, 100.) / sx0);
        let scale_y = initial.scale_y * (sample("Scale_Y", t, 100.) / sy0);

        if used_3d {
            let [px, py] = ctx.pivot.unwrap_or([0.0, 0.0]);
            let world_piv = [
                base[0] * px + base[2] * py + base[4] + (xy[0] - origin[0]),
                base[1] * px + base[3] * py + base[5] + (xy[1] - origin[1]),
            ];
            let rx = sample("Rotation_X", t, 0.);
            let ry = sample("Rotation_Y", t, 0.);
            let rz = initial.rotation + drot;
            let mz = sample("Motion_Z", t, 0.);
            let sz_ratio = sample("Scale_Z", t, 100.) / sz0;
            let mut projected = project_3d_pose_to_2d(
                world_piv,
                [px, py],
                z_pivot_offset * sz_ratio,
                mz,
                scale_x,
                scale_y,
                initial.skew + dsx,
                dsy,
                rx,
                ry,
                rz,
                &ctx,
            )
            .ok_or("motion pose exceeds the document transform limits")?;
            projected.alpha = element.transform.alpha;
            projected.color_transform = element.transform.color_transform;
            element.transform = projected;
        } else {
            let tr = &mut element.transform;
            tr.rotation += drot + dsy;
            tr.skew += dsx - dsy;
            tr.scale_x = scale_x;
            tr.scale_y = scale_y;
            tr.x += xy[0] - origin[0];
            tr.y += xy[1] - origin[1];
            if (tr.rotation - initial.rotation).abs() > 1e-8
                || (tr.skew - initial.skew).abs() > 1e-8
                || (tr.scale_x - initial.scale_x).abs() > 1e-8
                || (tr.scale_y - initial.scale_y).abs() > 1e-8
            {
                let [px, py] = ctx.pivot.ok_or("unresolved rotation or scale pivot")?;
                let current = transform_matrix(*tr);
                tr.x += (base[0] - current[0]) * px + (base[2] - current[2]) * py;
                tr.y += (base[1] - current[1]) * px + (base[3] - current[3]) * py;
            }
        }

        if let Some((alpha_out, ct_out)) =
            evaluate_color_tracks(&tracks, t, initial.alpha, initial.color_transform)
        {
            element.transform.alpha = alpha_out;
            element.transform.color_transform = ct_out;
        }
        if !filters.is_empty() {
            element.filters = evaluate_filter_groups(&filters, t, &object.filters);
        }

        let tr = &element.transform;
        if ![tr.x, tr.y, tr.rotation, tr.skew, tr.scale_x, tr.scale_y]
            .iter()
            .all(|v| v.is_finite())
            || tr.x.abs() > 1e12
            || tr.y.abs() > 1e12
            || tr.rotation.abs() > 1e9
            || tr.skew.abs() > 1e9
            || tr.scale_x.abs() > 1e4
            || tr.scale_y.abs() > 1e4
        {
            return Err("motion pose exceeds the document transform limits");
        }
        Ok(element)
    };

    let start_element = if used_3d
        || !filters.is_empty()
        || evaluate_color_tracks(&tracks, 0.0, initial.alpha, initial.color_transform).is_some()
    {
        let mut h0 = heading;
        let el0 = eval_element_at(0, &mut h0)?;
        (el0 != *object).then_some(el0)
    } else {
        None
    };

    let mut poses = Vec::with_capacity(count);
    for frame in 1..duration {
        let element = eval_element_at(frame, &mut heading)?;
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
    Ok(SampledMotion {
        start_element,
        poses,
        used_3d,
        filter_approximated: filter_approximated || filters.iter().any(|g| g.approximated),
    })
}

#[allow(dead_code)]
pub(crate) fn poses(
    root: Node<'_, '_>,
    key: &Keyframe,
    duration: u32,
    pivot: Option<[f64; 2]>,
    budget: &mut usize,
) -> Result<Vec<Keyframe>, &'static str> {
    sample_poses(
        root,
        key,
        duration,
        MotionContext {
            pivot,
            ..MotionContext::default()
        },
        budget,
    )
    .map(|res| res.poses)
}
