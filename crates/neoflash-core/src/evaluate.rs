use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
    /// Derived local-path cache hint. It is intentionally absent from serialized snapshots and
    /// equality so renderer preparation never changes the public evaluation contract.
    #[serde(skip)]
    pub path_fingerprint: Option<PathFingerprint>,
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
            path_fingerprint: None,
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
    pub blend_mode: BlendMode,
    pub element_blend_mode: BlendMode,
    pub cache_as_bitmap: bool,
    pub filters: Vec<FilterSpec>,
    pub kind: Option<ElementKind>,
}

#[derive(Default)]
pub(crate) struct Evaluation {
    pub items: Vec<DrawItem>,
    pub nodes: Vec<Node>,
    pub masks: Vec<crate::authored_layers::LayerMaskPlan>,
    pub blend_groups: Vec<crate::authored_layers::AuthoredBlendGroupPlan>,
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

fn zero_dummy_filter(spec: &FilterSpec) -> FilterSpec {
    let quality = spec.quality_level();
    match spec {
        FilterSpec::Blur { .. } => FilterSpec::Blur {
            blur_x: 0.0,
            blur_y: 0.0,
            quality,
        },
        FilterSpec::Glow {
            color,
            inner,
            knockout,
            ..
        } => FilterSpec::Glow {
            color: *color,
            alpha: 0.0,
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 0.0,
            quality,
            inner: *inner,
            knockout: *knockout,
        },
        FilterSpec::DropShadow {
            angle,
            color,
            inner,
            knockout,
            hide_object,
            ..
        } => FilterSpec::DropShadow {
            distance: 0.0,
            angle: *angle,
            color: *color,
            alpha: 0.0,
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 0.0,
            quality,
            inner: *inner,
            knockout: *knockout,
            hide_object: *hide_object,
        },
        FilterSpec::Bevel {
            angle,
            highlight_color,
            shadow_color,
            placement,
            knockout,
            ..
        } => FilterSpec::Bevel {
            distance: 0.0,
            angle: *angle,
            highlight_color: *highlight_color,
            highlight_alpha: 0.0,
            shadow_color: *shadow_color,
            shadow_alpha: 0.0,
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 0.0,
            quality,
            placement: placement.clone(),
            knockout: *knockout,
        },
        FilterSpec::GradientGlow {
            angle,
            colors,
            alphas,
            ratios,
            placement,
            knockout,
            ..
        } => FilterSpec::GradientGlow {
            distance: 0.0,
            angle: *angle,
            colors: colors.clone(),
            alphas: vec![0.0; alphas.len()],
            ratios: ratios.clone(),
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 0.0,
            quality,
            placement: placement.clone(),
            knockout: *knockout,
        },
        FilterSpec::GradientBevel {
            angle,
            colors,
            alphas,
            ratios,
            placement,
            knockout,
            ..
        } => FilterSpec::GradientBevel {
            distance: 0.0,
            angle: *angle,
            colors: colors.clone(),
            alphas: vec![0.0; alphas.len()],
            ratios: ratios.clone(),
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 0.0,
            quality,
            placement: placement.clone(),
            knockout: *knockout,
        },
        FilterSpec::ColorMatrix { .. } => FilterSpec::ColorMatrix {
            matrix: crate::IDENTITY_COLOR_MATRIX,
        },
        FilterSpec::Convolution {
            matrix_x,
            matrix_y,
            preserve_alpha,
            clamp,
            color,
            alpha,
            ..
        } => {
            let mx = (*matrix_x).max(1);
            let my = (*matrix_y).max(1);
            let mut matrix = vec![0.0; (mx * my) as usize];
            let center = ((my / 2) * mx + (mx / 2)) as usize;
            if let Some(cell) = matrix.get_mut(center) {
                *cell = 1.0;
            }
            FilterSpec::Convolution {
                matrix_x: mx,
                matrix_y: my,
                matrix,
                divisor: 1.0,
                bias: 0.0,
                preserve_alpha: *preserve_alpha,
                clamp: *clamp,
                color: *color,
                alpha: *alpha,
            }
        }
    }
}

fn sample_gradient_stop_at(
    colors: &[u32],
    alphas: &[f64],
    ratios: &[u8],
    ratio: f64,
) -> (u32, f64) {
    let n = colors.len().min(alphas.len()).min(ratios.len());
    if n == 0 {
        return (0x000000, 0.0);
    }
    if ratio <= ratios[0] as f64 || n == 1 {
        return (colors[0], alphas[0]);
    }
    if ratio >= ratios[n - 1] as f64 {
        return (colors[n - 1], alphas[n - 1]);
    }
    for i in 0..(n - 1) {
        let r0 = ratios[i] as f64;
        let r1 = ratios[i + 1] as f64;
        if ratio <= r1 {
            let local_t = if (r1 - r0).abs() <= 1e-9 {
                0.0
            } else {
                ((ratio - r0) / (r1 - r0)).clamp(0.0, 1.0)
            };
            let ch = |c: u32, shift: u32| ((c >> shift) & 0xff) as f64;
            let r = lerp(ch(colors[i], 16), ch(colors[i + 1], 16), local_t)
                .round()
                .clamp(0.0, 255.0) as u32;
            let g = lerp(ch(colors[i], 8), ch(colors[i + 1], 8), local_t)
                .round()
                .clamp(0.0, 255.0) as u32;
            let b = lerp(ch(colors[i], 0), ch(colors[i + 1], 0), local_t)
                .round()
                .clamp(0.0, 255.0) as u32;
            let a = lerp(alphas[i], alphas[i + 1], local_t).clamp(0.0, 1.0);
            return ((r << 16) | (g << 8) | b, a);
        }
    }
    (colors[n - 1], alphas[n - 1])
}

fn interpolate_gradient_stops(
    ac: &[u32],
    aa: &[f64],
    ar: &[u8],
    bc: &[u32],
    ba: &[f64],
    br: &[u8],
    t: f64,
) -> (Vec<u32>, Vec<f64>, Vec<u8>) {
    let lerp_rgb = |ca: u32, cb: u32| -> u32 {
        let ch = |c: u32, shift: u32| ((c >> shift) & 0xff) as f64;
        let r = lerp(ch(ca, 16), ch(cb, 16), t).round().clamp(0.0, 255.0) as u32;
        let g = lerp(ch(ca, 8), ch(cb, 8), t).round().clamp(0.0, 255.0) as u32;
        let bl = lerp(ch(ca, 0), ch(cb, 0), t).round().clamp(0.0, 255.0) as u32;
        (r << 16) | (g << 8) | bl
    };
    let na = ac.len().min(aa.len()).min(ar.len());
    let nb = bc.len().min(ba.len()).min(br.len());
    if na == nb && na > 0 {
        let colors = (0..na).map(|i| lerp_rgb(ac[i], bc[i])).collect();
        let alphas = (0..na)
            .map(|i| lerp(aa[i], ba[i], t).clamp(0.0, 1.0))
            .collect();
        let mut ratios: Vec<u8> = (0..na)
            .map(|i| {
                lerp(ar[i] as f64, br[i] as f64, t)
                    .round()
                    .clamp(0.0, 255.0) as u8
            })
            .collect();
        for i in 1..ratios.len() {
            ratios[i] = ratios[i].max(ratios[i - 1]);
        }
        return (colors, alphas, ratios);
    }
    let count = na.max(nb).clamp(2, 16);
    let mut colors = Vec::with_capacity(count);
    let mut alphas = Vec::with_capacity(count);
    let mut ratios = Vec::with_capacity(count);
    for i in 0..count {
        let frac = i as f64 / (count - 1) as f64;
        let ra = if na > 0 {
            let idx_f = frac * (na - 1) as f64;
            let i0 = idx_f.floor() as usize;
            let i1 = (i0 + 1).min(na - 1);
            lerp(ar[i0] as f64, ar[i1] as f64, idx_f - i0 as f64)
        } else {
            frac * 255.0
        };
        let rb = if nb > 0 {
            let idx_f = frac * (nb - 1) as f64;
            let i0 = idx_f.floor() as usize;
            let i1 = (i0 + 1).min(nb - 1);
            lerp(br[i0] as f64, br[i1] as f64, idx_f - i0 as f64)
        } else {
            frac * 255.0
        };
        let r = lerp(ra, rb, t).round().clamp(0.0, 255.0) as u8;
        let (ca, alpha_a) = sample_gradient_stop_at(ac, aa, ar, r as f64);
        let (cb, alpha_b) = sample_gradient_stop_at(bc, ba, br, r as f64);
        colors.push(lerp_rgb(ca, cb));
        alphas.push(lerp(alpha_a, alpha_b, t).clamp(0.0, 1.0));
        ratios.push(r);
    }
    for i in 1..ratios.len() {
        ratios[i] = ratios[i].max(ratios[i - 1]);
    }
    (colors, alphas, ratios)
}

fn interpolate_filter(a: &FilterSpec, b: &FilterSpec, t: f64) -> Option<FilterSpec> {
    let a_enabled = a.is_enabled();
    let b_enabled = b.is_enabled();
    if !a_enabled && !b_enabled {
        let a_en = a.clone().with_enabled(true);
        let b_en = b.clone().with_enabled(true);
        return interpolate_filter(&a_en, &b_en, t).map(|f| f.with_enabled(false));
    }
    if !a_enabled && b_enabled {
        let dummy = zero_dummy_filter(b);
        return interpolate_filter(&dummy, b, t);
    }
    if a_enabled && !b_enabled {
        let dummy = zero_dummy_filter(a);
        return interpolate_filter(a, &dummy, t);
    }
    let lerp_rgb = |ca: u32, cb: u32| -> u32 {
        let ch = |c: u32, shift: u32| ((c >> shift) & 0xff) as f64;
        let r = lerp(ch(ca, 16), ch(cb, 16), t).round().clamp(0.0, 255.0) as u32;
        let g = lerp(ch(ca, 8), ch(cb, 8), t).round().clamp(0.0, 255.0) as u32;
        let bl = lerp(ch(ca, 0), ch(cb, 0), t).round().clamp(0.0, 255.0) as u32;
        (r << 16) | (g << 8) | bl
    };
    match (a, b) {
        (
            FilterSpec::Blur {
                blur_x: ax,
                blur_y: ay,
                quality,
            },
            FilterSpec::Blur {
                blur_x: bx,
                blur_y: by,
                ..
            },
        ) => Some(FilterSpec::Blur {
            blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
            blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
            quality: *quality,
        }),
        (
            FilterSpec::Glow {
                color: ac,
                alpha: aa,
                blur_x: ax,
                blur_y: ay,
                strength: ast,
                quality,
                inner,
                knockout,
            },
            FilterSpec::Glow {
                color: bc,
                alpha: ba,
                blur_x: bx,
                blur_y: by,
                strength: bst,
                ..
            },
        ) => Some(FilterSpec::Glow {
            color: lerp_rgb(*ac, *bc),
            alpha: lerp(*aa, *ba, t).clamp(0.0, 1.0),
            blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
            blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
            strength: lerp(*ast, *bst, t).clamp(0.0, 255.0),
            quality: *quality,
            inner: *inner,
            knockout: *knockout,
        }),
        (
            FilterSpec::DropShadow {
                distance: ad,
                angle: aang,
                color: ac,
                alpha: aa,
                blur_x: ax,
                blur_y: ay,
                strength: ast,
                quality,
                inner,
                knockout,
                hide_object,
            },
            FilterSpec::DropShadow {
                distance: bd,
                angle: bang,
                color: bc,
                alpha: ba,
                blur_x: bx,
                blur_y: by,
                strength: bst,
                ..
            },
        ) => Some(FilterSpec::DropShadow {
            distance: lerp(*ad, *bd, t),
            angle: lerp(*aang, *bang, t),
            color: lerp_rgb(*ac, *bc),
            alpha: lerp(*aa, *ba, t).clamp(0.0, 1.0),
            blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
            blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
            strength: lerp(*ast, *bst, t).clamp(0.0, 255.0),
            quality: *quality,
            inner: *inner,
            knockout: *knockout,
            hide_object: *hide_object,
        }),
        (FilterSpec::ColorMatrix { matrix: am }, FilterSpec::ColorMatrix { matrix: bm }) => {
            Some(FilterSpec::ColorMatrix {
                matrix: std::array::from_fn(|i| lerp(am[i], bm[i], t)),
            })
        }
        (
            FilterSpec::Bevel {
                distance: ad,
                angle: aang,
                highlight_color: ahc,
                highlight_alpha: aha,
                shadow_color: asc,
                shadow_alpha: asa,
                blur_x: ax,
                blur_y: ay,
                strength: ast,
                quality,
                placement,
                knockout,
            },
            FilterSpec::Bevel {
                distance: bd,
                angle: bang,
                highlight_color: bhc,
                highlight_alpha: bha,
                shadow_color: bsc,
                shadow_alpha: bsa,
                blur_x: bx,
                blur_y: by,
                strength: bst,
                ..
            },
        ) => Some(FilterSpec::Bevel {
            distance: lerp(*ad, *bd, t),
            angle: lerp(*aang, *bang, t),
            highlight_color: lerp_rgb(*ahc, *bhc),
            highlight_alpha: lerp(*aha, *bha, t).clamp(0.0, 1.0),
            shadow_color: lerp_rgb(*asc, *bsc),
            shadow_alpha: lerp(*asa, *bsa, t).clamp(0.0, 1.0),
            blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
            blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
            strength: lerp(*ast, *bst, t).clamp(0.0, 255.0),
            quality: *quality,
            placement: placement.clone(),
            knockout: *knockout,
        }),
        (
            FilterSpec::GradientGlow {
                distance: ad,
                angle: aang,
                colors: ac,
                alphas: aa,
                ratios: ar,
                blur_x: ax,
                blur_y: ay,
                strength: ast,
                quality,
                placement,
                knockout,
            },
            FilterSpec::GradientGlow {
                distance: bd,
                angle: bang,
                colors: bc,
                alphas: ba,
                ratios: br,
                blur_x: bx,
                blur_y: by,
                strength: bst,
                ..
            },
        ) => {
            let (colors, alphas, ratios) = interpolate_gradient_stops(ac, aa, ar, bc, ba, br, t);
            Some(FilterSpec::GradientGlow {
                distance: lerp(*ad, *bd, t),
                angle: lerp(*aang, *bang, t),
                colors,
                alphas,
                ratios,
                blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
                blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
                strength: lerp(*ast, *bst, t).clamp(0.0, 255.0),
                quality: *quality,
                placement: placement.clone(),
                knockout: *knockout,
            })
        }
        (
            FilterSpec::GradientBevel {
                distance: ad,
                angle: aang,
                colors: ac,
                alphas: aa,
                ratios: ar,
                blur_x: ax,
                blur_y: ay,
                strength: ast,
                quality,
                placement,
                knockout,
            },
            FilterSpec::GradientBevel {
                distance: bd,
                angle: bang,
                colors: bc,
                alphas: ba,
                ratios: br,
                blur_x: bx,
                blur_y: by,
                strength: bst,
                ..
            },
        ) => {
            let (colors, alphas, ratios) = interpolate_gradient_stops(ac, aa, ar, bc, ba, br, t);
            Some(FilterSpec::GradientBevel {
                distance: lerp(*ad, *bd, t),
                angle: lerp(*aang, *bang, t),
                colors,
                alphas,
                ratios,
                blur_x: lerp(*ax, *bx, t).clamp(0.0, 255.0),
                blur_y: lerp(*ay, *by, t).clamp(0.0, 255.0),
                strength: lerp(*ast, *bst, t).clamp(0.0, 255.0),
                quality: *quality,
                placement: placement.clone(),
                knockout: *knockout,
            })
        }
        _ => None,
    }
}

/// Interpolates two filter stacks at `t` (`0.0..=1.0`).
///
/// When one endpoint has filters that are missing on the other endpoint, synthesizes a matching
/// zero-strength / zero-blur dummy filter at the opposite endpoint (Flash 8–CS6 tween behavior)
/// so filters smoothly animate in and out across tweens.
pub fn interpolate_filter_list(a: &[FilterSpec], b: &[FilterSpec], t: f64) -> Vec<FilterSpec> {
    if a.is_empty() && b.is_empty() {
        return Vec::new();
    }
    if a.len() == b.len()
        && let Some(interpolated) = a
            .iter()
            .zip(b)
            .map(|(fa, fb)| interpolate_filter(fa, fb, t))
            .collect::<Option<Vec<_>>>()
    {
        return interpolated;
    }
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    let mut b_cursor = 0usize;
    for fa in a {
        if let Some(rel_idx) = b[b_cursor..]
            .iter()
            .position(|fb| std::mem::discriminant(fa) == std::mem::discriminant(fb))
        {
            let match_idx = b_cursor + rel_idx;
            for fb in &b[b_cursor..match_idx] {
                let dummy = zero_dummy_filter(fb);
                if let Some(interp) = interpolate_filter(&dummy, fb, t) {
                    out.push(interp);
                }
            }
            if let Some(interp) = interpolate_filter(fa, &b[match_idx], t) {
                out.push(interp);
            }
            b_cursor = match_idx + 1;
        } else {
            let dummy = zero_dummy_filter(fa);
            if let Some(interp) = interpolate_filter(fa, &dummy, t) {
                out.push(interp);
            }
        }
    }
    for fb in &b[b_cursor..] {
        let dummy = zero_dummy_filter(fb);
        if let Some(interp) = interpolate_filter(&dummy, fb, t) {
            out.push(interp);
        }
    }
    out.truncate(crate::MAX_OBJECT_FILTERS);
    out
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
                    lerp(ra.corner_radii[i], rb.corner_radii[i], t).clamp(0.0, 1e9)
                }),
                corners_locked: ra.corners_locked,
            };
            (!r.is_default()).then_some(PrimitiveOptions::Rectangle(r))
        }
        (Some(PrimitiveOptions::Oval(oa)), Some(PrimitiveOptions::Oval(ob))) => {
            let o = OvalOptions {
                start_angle: lerp(oa.start_angle, ob.start_angle, t).clamp(0.0, 360.0),
                end_angle: lerp(oa.end_angle, ob.end_angle, t).clamp(0.0, 360.0),
                inner_radius: lerp(oa.inner_radius, ob.inner_radius, t).clamp(0.0, 99.0),
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
        alpha: lerp(x.alpha, y.alpha, t).clamp(0.0, 1.0),
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
    if !a.filters.is_empty() || !b.filters.is_empty() {
        e.filters = interpolate_filter_list(&a.filters, &b.filters, t);
    }
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
            *width = lerp(*aw, *bw, t).max(1e-6);
            *height = lerp(*ah, *bh, t).max(1e-6);
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
            *width = lerp(*aw, *bw, t).max(1e-6);
            *height = lerp(*ah, *bh, t).max(1e-6);
            *fill = color_lerp(*af, *bf, t);
            *stroke = color_lerp(*as_, *bs, t);
            *stroke_width = lerp(*az, *bz, t).max(0.);
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
            *font_size = lerp(*a, *b, t).max(1.0);
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

/// Sample a layer using the legacy/default 24 fps expression clock.
///
/// Project-aware rendering should call [`sample_layer_at_fps`] so expression `time` and `fps`
/// variables match the document frame rate.
pub fn sample_layer(layer: &Layer, frame: u32) -> Vec<Element> {
    sample_layer_at_fps(layer, frame, 24.0)
}

fn apply_layer_and_keyframe_filters(
    mut elements: Vec<Element>,
    key_filters: &[FilterSpec],
    layer_filters: &[FilterSpec],
) -> Vec<Element> {
    if key_filters.is_empty() && layer_filters.is_empty() {
        return elements;
    }
    for el in &mut elements {
        for f in key_filters.iter().chain(layer_filters.iter()) {
            if f.is_enabled() && el.filters.len() < crate::MAX_OBJECT_FILTERS {
                el.filters.push(f.clone());
            }
        }
    }
    elements
}

/// Sample a layer with an explicit document frame rate for property-expression evaluation.
pub fn sample_layer_at_fps(layer: &Layer, frame: u32, fps: f64) -> Vec<Element> {
    let Some(key) = layer.key_at(frame) else {
        return vec![];
    };
    if key.keyed_properties.is_some()
        || (key.tween.is_some() && key.tween_kind == TweenKind::Motion)
    {
        let elements = sample_motion(layer, frame);
        let start_key = motion_span(layer, frame).map(|(s, _, _)| s).unwrap_or(key);
        let key_filters = sample_motion_key_filters(layer, frame, start_key);
        let evaluated =
            apply_keyframe_rigs_and_appearances(elements, start_key, None, 0.0, frame, fps);
        return apply_layer_and_keyframe_filters(
            evaluated,
            &key_filters,
            &layer.properties.filters,
        );
    }
    let Some(ease) = key.tween else {
        let evaluated =
            apply_keyframe_rigs_and_appearances(key.elements.clone(), key, None, 0.0, frame, fps);
        return apply_layer_and_keyframe_filters(
            evaluated,
            &key.filters,
            &layer.properties.filters,
        );
    };
    let Some(next) = next_key(layer, key.frame) else {
        let evaluated =
            apply_keyframe_rigs_and_appearances(key.elements.clone(), key, None, 0.0, frame, fps);
        return apply_layer_and_keyframe_filters(
            evaluated,
            &key.filters,
            &layer.properties.filters,
        );
    };
    let t = easing(
        (frame - key.frame) as f64 / (next.frame - key.frame) as f64,
        ease,
    );
    let key_filters = if !key.filters.is_empty() || !next.filters.is_empty() {
        interpolate_filter_list(&key.filters, &next.filters, t)
    } else {
        Vec::new()
    };
    if key.tween_kind == TweenKind::Shape {
        let elements = crate::shape_tween::shape_tween_elements_with_hints(
            &key.elements,
            &next.elements,
            &key.shape_hints,
            t,
        );
        let evaluated =
            apply_keyframe_rigs_and_appearances(elements, key, Some(next), t, frame, fps);
        return apply_layer_and_keyframe_filters(
            evaluated,
            &key_filters,
            &layer.properties.filters,
        );
    }
    let elements: Vec<Element> = key
        .elements
        .iter()
        .map(|e| {
            next.elements
                .iter()
                .find(|n| n.id == e.id)
                .map_or_else(|| e.clone(), |n| interpolate_classic(e, n, t, key))
        })
        .collect();
    let evaluated = apply_keyframe_rigs_and_appearances(elements, key, Some(next), t, frame, fps);
    apply_layer_and_keyframe_filters(evaluated, &key_filters, &layer.properties.filters)
}

fn interpolate_asset_warp_rig(a: &AssetWarpRig, b: Option<&AssetWarpRig>, t: f64) -> AssetWarpRig {
    let Some(b) = b else {
        return a.clone();
    };
    let pins = a
        .pins
        .iter()
        .enumerate()
        .map(|(idx, pa)| {
            let pb = b
                .pins
                .iter()
                .find(|p| p.id == pa.id)
                .or_else(|| b.pins.get(idx));
            if let Some(pb) = pb {
                WarpPin {
                    id: pa.id.clone(),
                    kind: pa.kind,
                    rest_pos: pa.rest_pos,
                    pos: [lerp(pa.pos[0], pb.pos[0], t), lerp(pa.pos[1], pb.pos[1], t)],
                    rotation: lerp(pa.rotation, pb.rotation, t),
                    stiffness: lerp(pa.stiffness, pb.stiffness, t),
                }
            } else {
                pa.clone()
            }
        })
        .collect();
    AssetWarpRig {
        element_id: a.element_id.clone(),
        pins,
        envelop_subdivisions: a.envelop_subdivisions,
    }
}

fn apply_keyframe_rigs_and_appearances(
    mut elements: Vec<Element>,
    key: &Keyframe,
    next: Option<&Keyframe>,
    t: f64,
    frame: u32,
    fps: f64,
) -> Vec<Element> {
    fn reserve_synthetic_id(occupied: &mut HashSet<Id>, preferred: Id) -> Id {
        if occupied.insert(preferred.clone()) {
            return preferred;
        }
        for suffix in 2_u32.. {
            let candidate = format!("{preferred}-{suffix}");
            if occupied.insert(candidate.clone()) {
                return candidate;
            }
        }
        unreachable!("the bounded sampled layer always leaves a free synthetic ID")
    }

    // 0. Apply deterministic property expressions (`property_expressions`, T-121).
    if !key.property_expressions.is_empty() {
        for (idx, el) in elements.iter_mut().enumerate() {
            crate::motion_tools::apply_keyframe_property_expressions(
                &mut el.transform,
                &key.property_expressions,
                frame,
                fps,
                idx as u32,
            );
        }
    }

    // 1. Solve IK armatures (interpolating target_pos when tweened).
    for armature in &key.ik_armatures {
        let mut arm = armature.clone();
        if let Some(next_key) = next
            && let Some(next_arm) = next_key.ik_armatures.iter().find(|a| a.id == armature.id)
        {
            arm.target_pos = [
                lerp(armature.target_pos[0], next_arm.target_pos[0], t),
                lerp(armature.target_pos[1], next_arm.target_pos[1], t),
            ];
        }
        crate::geometry::solve_ik_armature(&mut elements, &arm);
    }

    // 2. Apply element parenting hierarchy (`element_parents`) so children inherit parent transforms.
    if !key.element_parents.is_empty() {
        let base_snapshot: Vec<Element> = elements.to_vec();
        let parent_of = |cid: &str| -> Option<&str> {
            key.element_parents
                .iter()
                .find(|p| p.child_id == cid)
                .map(|p| p.parent_id.as_str())
        };
        for el in elements.iter_mut() {
            if parent_of(&el.id).is_none() {
                continue;
            }
            let mut m = transform_matrix(el.transform);
            let mut cursor = parent_of(&el.id);
            let mut depth = 0usize;
            while let Some(pid) = cursor {
                if depth >= 64 {
                    break;
                }
                let Some(parent) = base_snapshot.iter().find(|e| e.id == pid) else {
                    break;
                };
                m = affine_multiply(transform_matrix(parent.transform), m);
                cursor = parent_of(pid);
                depth += 1;
            }
            if let Some(world_t) = Transform::from_matrix(m) {
                el.transform.x = world_t.x;
                el.transform.y = world_t.y;
                el.transform.scale_x = world_t.scale_x;
                el.transform.scale_y = world_t.scale_y;
                el.transform.rotation = world_t.rotation;
                el.transform.skew = world_t.skew;
            }
        }
    }

    // 3. Apply Asset Warp rigs (interpolating pin positions and rotations when tweened).
    if !key.asset_warps.is_empty() {
        for el in elements.iter_mut() {
            if let Some(rig_a) = key.asset_warp(&el.id) {
                let rig_b = next.and_then(|nk| nk.asset_warp(&el.id));
                let rig = interpolate_asset_warp_rig(rig_a, rig_b, t);
                *el = crate::geometry::apply_asset_warp_to_element(el, &rig);
            }
        }
    }

    // 3b. Apply non-destructive vector effects (`vector_effects`, T-120).
    let mut working_elements: Vec<Element> = if key.vector_effects.is_empty() {
        elements
    } else {
        let snapshot = elements.clone();
        let mut occupied_ids: HashSet<Id> =
            elements.iter().map(|element| element.id.clone()).collect();
        let mut vec_out = Vec::with_capacity(elements.len());
        for el in elements.iter() {
            if let Some(veff) = key.vector_effects(&el.id) {
                let expanded = crate::vector_effects::expand_vector_effects(el, veff);
                vec_out.push(expanded.clone());
                if let Some(blend_spec) = &veff.vector_blend
                    && let Some(target_el) = snapshot
                        .iter()
                        .find(|cand| cand.id == blend_spec.target_element_id)
                {
                    for mut step in crate::vector_effects::generate_vector_blend_steps(
                        &expanded, target_el, blend_spec,
                    ) {
                        step.id = reserve_synthetic_id(&mut occupied_ids, step.id);
                        vec_out.push(step);
                    }
                }
            } else {
                vec_out.push(el.clone());
            }
        }
        vec_out
    };

    // 4. Expand non-destructive appearance stacks and vector modifiers (`appearances`).
    if key.appearances.is_empty() {
        return working_elements;
    }
    // Reserve every input ID before expanding any appearance. Generated glyph/line/paint IDs
    // therefore cannot steal the identity of an authored element or an earlier blend step that
    // happens to appear later in painter order.
    let mut occupied_ids: HashSet<Id> = working_elements
        .iter()
        .map(|element| element.id.clone())
        .collect();
    let mut out = Vec::with_capacity(working_elements.len());
    for el in working_elements.drain(..) {
        if let Some(app) = key.appearance(&el.id) {
            let mut kept_input_id = false;
            for mut evaluated in crate::geometry::evaluate_element_appearance(&el, app) {
                if evaluated.id == el.id && !kept_input_id {
                    kept_input_id = true;
                } else {
                    evaluated.id = reserve_synthetic_id(&mut occupied_ids, evaluated.id);
                }
                out.push(evaluated);
            }
        } else {
            out.push(el);
        }
    }
    out
}

/// Sample the active 2D `CameraPose` of a timeline at `frame`, interpolating across tweened keyframes (T-113).
pub fn sample_timeline_camera(timeline: &Timeline, frame: u32) -> Option<CameraPose> {
    for layer in &timeline.layers {
        let Some(key) = layer.key_at(frame) else {
            continue;
        };
        let Some(cam_a) = key.camera else {
            continue;
        };
        if let Some(ease) = key.tween
            && let Some(next) = next_key(layer, key.frame)
            && let Some(cam_b) = next.camera
        {
            let t = easing(
                (frame - key.frame) as f64 / (next.frame - key.frame) as f64,
                ease,
            );
            return Some(CameraPose {
                x: lerp(cam_a.x, cam_b.x, t),
                y: lerp(cam_a.y, cam_b.y, t),
                zoom: lerp(cam_a.zoom, cam_b.zoom, t).clamp(0.01, 100.0),
                rotation: lerp(cam_a.rotation, cam_b.rotation, t),
                focal_length: lerp(cam_a.focal_length, cam_b.focal_length, t).max(10.0),
                color_transform: crate::ColorTransform {
                    multipliers: std::array::from_fn(|i| {
                        lerp(
                            cam_a.color_transform.multipliers[i],
                            cam_b.color_transform.multipliers[i],
                            t,
                        )
                    }),
                    offsets: std::array::from_fn(|i| {
                        lerp(
                            cam_a.color_transform.offsets[i],
                            cam_b.color_transform.offsets[i],
                            t,
                        )
                    }),
                },
            });
        }
        return Some(cam_a);
    }
    None
}

/// Compute the 2D camera view + depth-parallax matrix `[a, b, c, d, tx, ty]` for a layer at `z_depth` (T-113).
pub fn camera_layer_matrix(camera: &CameraPose, z_depth: f64, stage_center: [f64; 2]) -> [f64; 6] {
    let focal = camera.focal_length.max(10.0);
    let parallax = (focal / (focal + z_depth)).clamp(0.05, 20.0);
    let eff_zoom = lerp(1.0, camera.zoom.clamp(0.01, 100.0), parallax);
    let eff_rot = (-camera.rotation * parallax).to_radians();
    let (s, c) = eff_rot.sin_cos();
    let pan_x = -camera.x * parallax;
    let pan_y = -camera.y * parallax;
    // Transform around stage_center: T(center) * R * S * T(-center + pan)
    let a = c * eff_zoom;
    let b = s * eff_zoom;
    let c_mat = -s * eff_zoom;
    let d = c * eff_zoom;
    let dx = pan_x - stage_center[0];
    let dy = pan_y - stage_center[1];
    let tx = stage_center[0] + a * dx + c_mat * dy;
    let ty = stage_center[1] + b * dx + d * dy;
    [a, b, c_mat, d, tx, ty]
}

fn interpolate_classic(a: &Element, b: &Element, t: f64, key: &Keyframe) -> Element {
    let mut e = interpolate(a, b, t);
    if !key.tween_scale {
        e.transform.scale_x = a.transform.scale_x;
        e.transform.scale_y = a.transform.scale_y;
        match (&a.kind, &mut e.kind) {
            (
                ElementKind::Video {
                    width: aw,
                    height: ah,
                },
                ElementKind::Video { width, height },
            )
            | (
                ElementKind::Rectangle {
                    width: aw,
                    height: ah,
                    ..
                },
                ElementKind::Rectangle { width, height, .. },
            )
            | (
                ElementKind::Ellipse {
                    width: aw,
                    height: ah,
                    ..
                },
                ElementKind::Ellipse { width, height, .. },
            ) => {
                *width = *aw;
                *height = *ah;
            }
            _ => {}
        }
    }
    let turns = (key.classic_rotate_turns.min(Keyframe::MAX_ROTATE_TURNS) as f64) * 360.0;
    e.transform.rotation = match key.classic_rotate_dir {
        ClassicRotateDirection::Auto => {
            if turns == 0.0 {
                lerp(a.transform.rotation, b.transform.rotation, t)
            } else {
                let delta = b.transform.rotation - a.transform.rotation;
                let sign = if delta < 0.0 { -1.0 } else { 1.0 };
                a.transform.rotation + (delta + sign * turns) * t
            }
        }
        ClassicRotateDirection::None => a.transform.rotation,
        ClassicRotateDirection::Cw => {
            let cw_delta = (b.transform.rotation - a.transform.rotation).rem_euclid(360.0) + turns;
            a.transform.rotation + cw_delta * t
        }
        ClassicRotateDirection::Ccw => {
            let ccw_delta = (a.transform.rotation - b.transform.rotation).rem_euclid(360.0) + turns;
            a.transform.rotation - ccw_delta * t
        }
    };
    e
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
        TweenProperty::Alpha => transform.alpha = value(property).clamp(0.0, 1.0),
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

/// Effective property ease for one object in a Motion Tween. Format-10 object
/// overrides take precedence; absent overrides retain the legacy shared ease.
pub fn effective_property_ease_for(
    start: &Keyframe,
    element_id: &str,
    property: TweenProperty,
) -> Easing {
    start.motion_property_ease_for(element_id, property)
}

fn sample_motion_key_filters(layer: &Layer, frame: u32, fallback: &Keyframe) -> Vec<FilterSpec> {
    let Some((start, properties, end)) = motion_span(layer, frame) else {
        return fallback.filters.clone();
    };
    let mut filter_keys: Vec<(u32, &[FilterSpec])> = vec![(start.frame, &start.filters)];
    for prop_key in &properties {
        if filter_keys
            .last()
            .is_none_or(|(_, last_f)| *last_f != prop_key.filters.as_slice())
        {
            filter_keys.push((prop_key.frame, &prop_key.filters));
        }
    }
    if filter_keys.len() <= 1 {
        return start.filters.clone();
    }
    let last = properties.last().map_or(start.frame, |k| k.frame);
    let span_end = end.unwrap_or(last).max(last).max(start.frame + 1);
    let length = (span_end - start.frame) as f64;
    let progress = ((frame - start.frame) as f64 / length).min(1.);
    let ease = start.tween.unwrap_or(Easing::Linear);
    let eff_time = start.frame as f64 + easing(progress, ease) * length;
    let mut prev = filter_keys[0];
    let mut next_fk = None;
    for &(kf, kf_filters) in &filter_keys[1..] {
        if (kf as f64) <= eff_time {
            prev = (kf, kf_filters);
        } else {
            next_fk = Some((kf, kf_filters));
            break;
        }
    }
    match next_fk {
        Some((nf, nf_filters)) => {
            let seg_t =
                ((eff_time - prev.0 as f64) / ((nf - prev.0).max(1) as f64)).clamp(0.0, 1.0);
            interpolate_filter_list(prev.1, nf_filters, seg_t)
        }
        None => prev.1.to_vec(),
    }
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
    start
        .elements
        .iter()
        .map(|element| {
            let mut out = element.clone();
            out.transform = motion_pose(start, &properties, element, progress, length);
            let mut filter_keys: Vec<(u32, &[FilterSpec])> = vec![(start.frame, &element.filters)];
            for prop_key in &properties {
                if let Some(ke) = prop_key.elements.iter().find(|e| e.id == element.id)
                    && filter_keys
                        .last()
                        .is_none_or(|(_, last_f)| *last_f != ke.filters.as_slice())
                {
                    filter_keys.push((prop_key.frame, &ke.filters));
                }
            }
            if filter_keys.len() > 1 {
                let ease = start.tween.unwrap_or(Easing::Linear);
                let eff_time = start.frame as f64 + easing(progress, ease) * length;
                let mut prev = filter_keys[0];
                let mut next_fk = None;
                for &(kf, kf_filters) in &filter_keys[1..] {
                    if (kf as f64) <= eff_time {
                        prev = (kf, kf_filters);
                    } else {
                        next_fk = Some((kf, kf_filters));
                        break;
                    }
                }
                out.filters = match next_fk {
                    Some((nf, nf_filters)) => {
                        let seg_t = ((eff_time - prev.0 as f64) / ((nf - prev.0).max(1) as f64))
                            .clamp(0.0, 1.0);
                        interpolate_filter_list(prev.1, nf_filters, seg_t)
                    }
                    None => prev.1.to_vec(),
                };
            }
            if start.motion_orient_to_path_for(&element.id) {
                let x_ease = effective_property_ease_for(start, &element.id, TweenProperty::X);
                let y_ease = effective_property_ease_for(start, &element.id, TweenProperty::Y);
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
    let use_roving = matches!(property, TweenProperty::X | TweenProperty::Y)
        && properties
            .iter()
            .any(|key| key.motion_key_roving_for(&element.id));
    let roving_map = if use_roving {
        Some(roving_frame_times(start, properties, element))
    } else {
        None
    };
    let effective_time = |frame: u32| -> f64 {
        roving_map
            .as_ref()
            .and_then(|m| m.iter().find(|(f, _)| *f == frame).map(|(_, t)| *t))
            .unwrap_or(frame as f64)
    };
    let mut previous = (start.frame as f64, element.transform);
    let mut next = None;
    for key in properties {
        if !key
            .motion_key_properties_for(&element.id)
            .contains(&property)
        {
            continue;
        }
        let Some(keyed) = key.elements.iter().find(|e| e.id == element.id) else {
            continue;
        };
        let kf = effective_time(key.frame);
        if kf <= time {
            previous = (kf, keyed.transform);
        } else {
            next = Some((kf, keyed.transform));
            break;
        }
    }
    let (to, t) = match next {
        Some((frame, to)) => (
            to,
            ((time - previous.0) / (frame - previous.0).max(1e-9)).clamp(0., 1.),
        ),
        None => (previous.1, 0.),
    };
    set_property(transform, property, &previous.1, &to, t);
}

fn roving_frame_times(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
) -> Vec<(u32, f64)> {
    let mut raw: Vec<(u32, [f64; 2], bool)> = vec![(
        start.frame,
        [element.transform.x, element.transform.y],
        false,
    )];
    let mut cur_x = element.transform.x;
    let mut cur_y = element.transform.y;
    for key in properties {
        let kp = key.motion_key_properties_for(&element.id);
        if !(kp.contains(&TweenProperty::X) || kp.contains(&TweenProperty::Y)) {
            continue;
        }
        if let Some(el) = key.elements.iter().find(|e| e.id == element.id) {
            if kp.contains(&TweenProperty::X) {
                cur_x = el.transform.x;
            }
            if kp.contains(&TweenProperty::Y) {
                cur_y = el.transform.y;
            }
            raw.push((
                key.frame,
                [cur_x, cur_y],
                key.motion_key_roving_for(&element.id),
            ));
        }
    }
    let n = raw.len();
    if n < 3 {
        return raw.into_iter().map(|(f, _, _)| (f, f as f64)).collect();
    }
    if let Some(last) = raw.last_mut() {
        last.2 = false;
    }
    let mut cum = Vec::with_capacity(n);
    cum.push(0.0);
    for pair in raw.windows(2) {
        let d = (pair[1].1[0] - pair[0].1[0]).hypot(pair[1].1[1] - pair[0].1[1]);
        cum.push(cum.last().copied().unwrap_or(0.0) + d);
    }
    let mut out: Vec<(u32, f64)> = raw.iter().map(|&(f, _, _)| (f, f as f64)).collect();
    let mut i = 0;
    while i < n {
        if raw[i].2 {
            let left = i.saturating_sub(1);
            let mut right = i + 1;
            while right + 1 < n && raw[right].2 {
                right += 1;
            }
            let span_len = cum[right] - cum[left];
            let t0 = raw[left].0 as f64;
            let t1 = raw[right].0 as f64;
            for k in i..right {
                if span_len > 1e-9 {
                    let frac = (cum[k] - cum[left]) / span_len;
                    out[k].1 = lerp(t0, t1, frac);
                }
            }
            i = right;
        } else {
            i += 1;
        }
    }
    out
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
        let ease = effective_property_ease_for(start, &element.id, property);
        let time = start.frame as f64 + easing(progress, ease) * length;
        interpolate_motion_property(&mut transform, start, properties, element, property, time);
    }
    apply_motion_curve(
        &mut transform,
        start,
        properties,
        element,
        start.frame as f64
            + easing(
                progress,
                effective_property_ease_for(start, &element.id, TweenProperty::X),
            ) * length,
        start.frame as f64
            + easing(
                progress,
                effective_property_ease_for(start, &element.id, TweenProperty::Y),
            ) * length,
    );
    transform
}

/// Pose of one motion-tweened element at an explicit fractional frame `time`.
fn motion_pose_at_time(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    time: f64,
) -> Transform {
    let mut transform = motion_pose_at_time_linear(start, properties, element, time);
    apply_motion_curve(&mut transform, start, properties, element, time, time);
    transform
}

fn motion_pose_at_time_linear(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    time: f64,
) -> Transform {
    let mut transform = element.transform;
    for property in TweenProperty::ALL {
        interpolate_motion_property(&mut transform, start, properties, element, property, time);
    }
    transform
}

/// Position-track waypoints without a spatial curve or temporal easing. These are the
/// authoring anchors used to construct a motion path; non-position ◆ keys add no anchors.
pub fn motion_path_waypoints(layer: &Layer, frame: u32, element_id: &str) -> Vec<(u32, [f64; 2])> {
    let Some((start, properties, _)) = motion_span(layer, frame) else {
        return vec![];
    };
    let Some(element) = start
        .elements
        .iter()
        .find(|element| element.id == element_id)
    else {
        return vec![];
    };
    position_waypoints(start, &properties, element)
}

fn position_waypoints(
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
) -> Vec<(u32, [f64; 2])> {
    let mut points = vec![(start.frame, [element.transform.x, element.transform.y])];
    for key in properties {
        let keys = key.motion_key_properties_for(&element.id);
        if keys.contains(&TweenProperty::X) || keys.contains(&TweenProperty::Y) {
            let pose = motion_pose_at_time_linear(start, properties, element, key.frame as f64);
            points.push((key.frame, [pose.x, pose.y]));
        }
    }
    points
}

/// Flatten one open spatial motion path, with cumulative arc lengths shared by editor and
/// runtime. Invalid or excessively complex paths return `None`, never a truncated curve.
pub fn motion_path_polyline(commands: &[PathCommand]) -> Option<GuidePath> {
    let contour = crate::geometry::flatten_path(
        commands,
        crate::FlattenOptions {
            tolerance: 0.05,
            ..Default::default()
        },
    )
    .ok()?
    .into_iter()
    .next()?;
    if contour.closed || contour.points.len() < 2 {
        return None;
    }
    Some(GuidePath::from_points(contour.points))
}

/// One line or cubic segment as an arc-length sampler, useful for motion-handle hit tests.
pub fn motion_path_segment(start: [f64; 2], command: PathCommand) -> Option<GuidePath> {
    motion_path_polyline(&[
        PathCommand::MoveTo {
            x: start[0],
            y: start[1],
        },
        command,
    ])
}

fn apply_motion_curve(
    transform: &mut Transform,
    start: &Keyframe,
    properties: &[&Keyframe],
    element: &Element,
    x_time: f64,
    y_time: f64,
) {
    let (commands, reference) = match start.motion_path_for(&element.id) {
        MotionPathSource::None => return,
        MotionPathSource::Legacy(commands) => {
            let Some(reference) = start.elements.first() else {
                return;
            };
            (commands, reference)
        }
        MotionPathSource::Object(commands) => (commands, element),
    };
    let points = position_waypoints(start, properties, reference);
    if points.len() < 2 {
        return;
    }
    let Some(path) = motion_path_polyline(commands) else {
        return;
    };
    // Initially one segment ends at each position key. If authoring inserts or removes a
    // key later, project it onto the retained curve so the editable geometry is not lost.
    let mut lengths = vec![0.];
    if commands.len() == points.len() {
        let mut previous = match commands[0] {
            PathCommand::MoveTo { x, y } => [x, y],
            _ => return,
        };
        for command in &commands[1..] {
            let Some(segment) = motion_path_segment(previous, *command) else {
                return;
            };
            lengths.push(lengths.last().unwrap() + segment.length());
            previous = segment.point_at(segment.length());
        }
    } else {
        lengths.extend(
            points[1..points.len() - 1]
                .iter()
                .map(|(_, p)| path.nearest(*p)),
        );
        lengths.push(path.length());
    }
    let point_at_time = |time: f64| {
        let i = points
            .partition_point(|(frame, _)| (*frame as f64) <= time)
            .saturating_sub(1);
        if i + 1 >= points.len() {
            return path.point_at(path.length());
        }
        let fraction =
            ((time - points[i].0 as f64) / (points[i + 1].0 - points[i].0) as f64).clamp(0., 1.);
        path.point_at(lerp(lengths[i], lengths[i + 1], fraction))
    };
    let base_x = motion_pose_at_time_linear(start, properties, reference, x_time).x;
    let base_y = motion_pose_at_time_linear(start, properties, reference, y_time).y;
    transform.x += point_at_time(x_time)[0] - base_x;
    transform.y += point_at_time(y_time)[1] - base_y;
}

/// Sample a layer within its timeline: classic tweens (and motion tweens with
/// `snap_to_guide`) on a layer guided by a motion guide move their objects'
/// registration points along the guide's first stroke.
pub fn sample_layer_in(timeline: &Timeline, layer: &Layer, frame: u32) -> Vec<Element> {
    sample_layer_in_at_fps(timeline, layer, frame, 24.0)
}

/// Sample a layer within its timeline with an explicit document frame rate for expressions.
pub fn sample_layer_in_at_fps(
    timeline: &Timeline,
    layer: &Layer,
    frame: u32,
    fps: f64,
) -> Vec<Element> {
    let mut elements = sample_layer_at_fps(layer, frame, fps);
    let Some(guide) = layer
        .properties
        .motion_guide
        .as_ref()
        .and_then(|id| timeline.layers.iter().find(|l| &l.id == id))
    else {
        return elements;
    };
    if let Some((start, properties, end)) = motion_span(layer, frame)
        && let Some(path) = guide_polyline(guide, start.frame)
    {
        let last = properties.last().map_or(start.frame, |k| k.frame);
        let span_end = end.unwrap_or(last).max(last).max(start.frame + 1);
        let length = (span_end - start.frame) as f64;
        let progress = ((frame - start.frame) as f64 / length).min(1.);
        for element in &mut elements {
            let Some(a) = start.elements.iter().find(|e| e.id == element.id) else {
                continue;
            };
            if !start.motion_snap_to_guide_for(&element.id) {
                continue;
            }
            let x_ease = effective_property_ease_for(start, &element.id, TweenProperty::X);
            let time = start.frame as f64 + easing(progress, x_ease) * length;
            let s0 = path.nearest([a.transform.x, a.transform.y]);
            let mut waypoints = vec![(start.frame as f64, s0)];
            for key in &properties {
                let keyed = key.motion_key_properties_for(&element.id);
                if keyed.contains(&TweenProperty::X) || keyed.contains(&TweenProperty::Y) {
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
            if start.motion_orient_to_path_for(&element.id) {
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
        let s0 = if key.snap_to_guide {
            0.
        } else {
            path.nearest([a.transform.x, a.transform.y])
        };
        let s1 = if key.snap_to_guide {
            path.length()
        } else {
            path.nearest([b.transform.x, b.transform.y])
        };
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
    fn from_points(points: Vec<[f64; 2]>) -> Self {
        let mut lengths = vec![0.];
        for pair in points.windows(2) {
            let distance = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            lengths.push(lengths.last().unwrap() + distance);
        }
        Self { points, lengths }
    }

    pub fn points(&self) -> &[[f64; 2]] {
        &self.points
    }
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
pub fn placement_start(layer: &Layer, frame: u32, id: &str) -> u32 {
    let mut keys: Vec<_> = layer
        .keyframes
        .iter()
        .filter(|k| k.frame <= frame)
        .collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.frame));
    let target = keys.iter().find_map(|key| {
        let element = key.elements.iter().find(|element| element.id == id)?;
        let ElementKind::Instance { symbol_id, .. } = &element.kind else {
            return None;
        };
        let ordinal = key.elements[..key
            .elements
            .iter()
            .position(|candidate| candidate.id == id)?]
            .iter()
            .filter(|candidate| {
                matches!(&candidate.kind, ElementKind::Instance { symbol_id: candidate_symbol, .. } if candidate_symbol == symbol_id)
            })
            .count();
        Some((symbol_id.as_str(), ordinal))
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
        let Some((symbol, ordinal)) = target else {
            break;
        };
        let same_symbol = |element: &&Element| matches!(&element.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == symbol);
        let motion_match = motion_span(layer, key.frame).and_then(|(span_start, _, _)| {
            if span_start.motion_tracks.is_empty() {
                return span_start
                    .sync_symbols
                    .then(|| key.elements.iter().find(same_symbol))
                    .flatten();
            }
            let matched = key.elements.iter().filter(same_symbol).nth(ordinal)?;
            let track_element = span_start
                .elements
                .iter()
                .filter(same_symbol)
                .nth(ordinal)
                .or_else(|| {
                    span_start
                        .elements
                        .iter()
                        .find(|element| element.id == matched.id)
                })?;
            span_start
                .motion_sync_symbols_for(&track_element.id)
                .then_some(matched)
        });
        let classic_match =
            (key.tween.is_some() && key.tween_kind == TweenKind::Classic && key.sync_symbols)
                .then(|| key.elements.iter().find(same_symbol))
                .flatten();
        if seen_current && let Some(matched) = motion_match.or(classic_match) {
            current_id = &matched.id;
            start = key.frame;
            continue;
        }
        break;
    }
    start
}

pub(crate) fn instance_frame_from_age(
    playback: InstancePlayback,
    age: u32,
    frame_count: u32,
) -> u32 {
    let count = frame_count.max(1);
    match playback {
        InstancePlayback::Independent | InstancePlayback::Loop => age % count,
        InstancePlayback::LoopFrom(start) => age.saturating_add(start) % count,
        InstancePlayback::PlayOnce => age.min(count - 1),
        InstancePlayback::PlayOnceFrom(start) => age.saturating_add(start).min(count - 1),
        InstancePlayback::SingleFrame(frame) => frame.min(count - 1),
    }
}

/// Evaluated pose of one exact symbol placement in its owner's timeline.
///
/// Authoring features that need the placement's coordinate space must use this
/// result instead of the serialized `Element::transform`: tweening, motion
/// guides, element parenting and the timeline camera can all change the matrix
/// displayed at a particular frame.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedInstancePose {
    pub symbol_id: Id,
    pub frame: u32,
    pub matrix: [f64; 6],
    /// Nine-slice scaling deforms the child non-linearly at this placement, so
    /// one affine matrix cannot describe an in-context child editing space.
    pub nonlinear_scale9: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstancePoseError {
    OwnerSymbolMissing,
    FrameOutOfRange,
    LayerMissing,
    PlacementMissing,
    PlacementIsNotInstance,
    ChildSymbolMissing,
    PlacementNotEvaluated,
    AmbiguousPlacement,
    NonFiniteMatrix,
    SingularMatrix,
}

impl std::fmt::Display for InstancePoseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OwnerSymbolMissing => "the owner symbol does not exist",
            Self::FrameOutOfRange => "the saved parent frame no longer exists",
            Self::LayerMissing => "the saved parent layer no longer exists",
            Self::PlacementMissing => "the saved symbol placement no longer exists",
            Self::PlacementIsNotInstance => "the saved placement is no longer a symbol instance",
            Self::ChildSymbolMissing => "the placed child symbol no longer exists",
            Self::PlacementNotEvaluated => {
                "the saved symbol placement is not active in the parent timeline"
            }
            Self::AmbiguousPlacement => {
                "the placement identity is ambiguous in the parent timeline"
            }
            Self::NonFiniteMatrix => "the evaluated symbol placement matrix is not finite",
            Self::SingularMatrix => "the evaluated symbol placement matrix is not invertible",
        })
    }
}

impl std::error::Error for InstancePoseError {}

/// Resolve one placement by its exact layer and element identity at an exact
/// parent frame. The returned matrix is in the owner symbol's timeline space.
pub fn evaluate_instance_pose(
    project: &Project,
    owner_symbol_id: &str,
    frame: u32,
    layer_id: &str,
    element_id: &str,
) -> Result<EvaluatedInstancePose, InstancePoseError> {
    let owner = project
        .symbol(owner_symbol_id)
        .ok_or(InstancePoseError::OwnerSymbolMissing)?;
    if frame >= owner.timeline.frame_count {
        return Err(InstancePoseError::FrameOutOfRange);
    }
    let layer = owner
        .timeline
        .layers
        .iter()
        .find(|layer| layer.id == layer_id)
        .ok_or(InstancePoseError::LayerMissing)?;
    let sampled = sample_layer_in_at_fps(&owner.timeline, layer, frame, f64::from(project.fps))
        .into_iter()
        .find(|element| element.id == element_id)
        .ok_or(InstancePoseError::PlacementMissing)?;
    let ElementKind::Instance { symbol_id, .. } = &sampled.kind else {
        return Err(InstancePoseError::PlacementIsNotInstance);
    };
    let child = project
        .symbol(symbol_id)
        .ok_or(InstancePoseError::ChildSymbolMissing)?;
    let nonlinear_scale9 = sampled
        .scale9_grid()
        .or_else(|| child.scale9_grid())
        .is_some()
        && ((sampled.transform.scale_x.abs() - 1.0).abs() > 1e-6
            || (sampled.transform.scale_y.abs() - 1.0).abs() > 1e-6);

    let mut matches = snapshot_mode(project, owner_symbol_id, frame, &|_| None, &|_| None, true)
        .nodes
        .into_iter()
        .filter(|node| {
            node.path.len() == 1
                && node.path[0] == element_id
                && node.symbol_id.as_deref() == Some(symbol_id.as_str())
        });
    let node = matches
        .next()
        .ok_or(InstancePoseError::PlacementNotEvaluated)?;
    if matches.next().is_some() {
        return Err(InstancePoseError::AmbiguousPlacement);
    }
    if node.world.iter().any(|value| !value.is_finite()) {
        return Err(InstancePoseError::NonFiniteMatrix);
    }
    let determinant = node.world[0] * node.world[3] - node.world[1] * node.world[2];
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return Err(InstancePoseError::SingularMatrix);
    }
    Ok(EvaluatedInstancePose {
        symbol_id: symbol_id.clone(),
        frame: node.frame,
        matrix: node.world,
        nonlinear_scale9,
    })
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
        blend_mode: BlendMode::Normal,
        element_blend_mode: BlendMode::Normal,
        cache_as_bitmap: false,
        filters: vec![],
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
            scale9: None,
        },
        &[],
        0,
    );
    result
}

#[derive(Clone, Copy, Debug)]
struct Scale9Mapping {
    grid: [f64; 4],
    bounds: [f64; 4],
    sx: f64,
    sy: f64,
}

impl Scale9Mapping {
    fn map_axis(v: f64, b0: f64, bw: f64, g0: f64, gw: f64, s: f64) -> f64 {
        if bw <= 1e-6 || gw <= 1e-6 {
            return v * s;
        }
        let left_margin = (g0 - b0).max(0.0);
        let right_margin = ((b0 + bw) - (g0 + gw)).max(0.0);
        let total_scaled = bw * s;
        let new_gw = (total_scaled - left_margin - right_margin).max(0.0);
        let new_b0 = b0 * s;
        if v <= g0 {
            new_b0 + (v - b0)
        } else if v >= g0 + gw {
            new_b0 + left_margin + new_gw + (v - (g0 + gw))
        } else {
            new_b0 + left_margin + (v - g0) * (new_gw / gw)
        }
    }

    fn map_point(&self, x: f64, y: f64) -> [f64; 2] {
        [
            Self::map_axis(
                x,
                self.bounds[0],
                self.bounds[2],
                self.grid[0],
                self.grid[2],
                self.sx,
            ),
            Self::map_axis(
                y,
                self.bounds[1],
                self.bounds[3],
                self.grid[1],
                self.grid[3],
                self.sy,
            ),
        ]
    }

    fn apply_to_element(&self, e: &mut Element) {
        let tx = e.transform.x;
        let ty = e.transform.y;
        let Some((commands, fill_rule)) = e.to_path_commands() else {
            let [nx, ny] = self.map_point(tx, ty);
            e.transform.x = nx;
            e.transform.y = ny;
            return;
        };
        let mapped: Vec<PathCommand> = commands
            .into_iter()
            .map(|cmd| match cmd {
                PathCommand::MoveTo { x, y } => {
                    let [nx, ny] = self.map_point(tx + x, ty + y);
                    PathCommand::MoveTo { x: nx, y: ny }
                }
                PathCommand::LineTo { x, y } => {
                    let [nx, ny] = self.map_point(tx + x, ty + y);
                    PathCommand::LineTo { x: nx, y: ny }
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let [ncx, ncy] = self.map_point(tx + cx, ty + cy);
                    let [nx, ny] = self.map_point(tx + x, ty + y);
                    PathCommand::QuadraticTo {
                        cx: ncx,
                        cy: ncy,
                        x: nx,
                        y: ny,
                    }
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let [ncx1, ncy1] = self.map_point(tx + cx1, ty + cy1);
                    let [ncx2, ncy2] = self.map_point(tx + cx2, ty + cy2);
                    let [nx, ny] = self.map_point(tx + x, ty + y);
                    PathCommand::CubicTo {
                        cx1: ncx1,
                        cy1: ncy1,
                        cx2: ncx2,
                        cy2: ncy2,
                        x: nx,
                        y: ny,
                    }
                }
                PathCommand::Close => PathCommand::Close,
            })
            .collect();
        e.transform.x = 0.0;
        e.transform.y = 0.0;
        e.transform.primitive = None;
        match &e.kind {
            ElementKind::Rectangle {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::Ellipse {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                e.kind = ElementKind::Path {
                    commands: mapped,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    stroke_width: *stroke_width,
                };
            }
            ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                stroke_width,
                ..
            } => {
                e.kind = ElementKind::PaintedPath {
                    commands: mapped,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: fill_gradient.clone(),
                    stroke_gradient: stroke_gradient.clone(),
                    stroke_width: *stroke_width,
                };
            }
            _ => {}
        }
    }
}

fn symbol_natural_bounds(symbol: &Symbol, frame: u32, fallback: [f64; 4], fps: f64) -> [f64; 4] {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for layer in &symbol.timeline.layers {
        for e in sample_layer_in_at_fps(&symbol.timeline, layer, frame, fps) {
            let tx = e.transform.x;
            let ty = e.transform.y;
            match &e.kind {
                ElementKind::Rectangle { width, height, .. }
                | ElementKind::Ellipse { width, height, .. }
                | ElementKind::Video { width, height } => {
                    min_x = min_x.min(tx);
                    min_y = min_y.min(ty);
                    max_x = max_x.max(tx + *width);
                    max_y = max_y.max(ty + *height);
                }
                ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                    if let Ok(Some(b)) = crate::geometry::path_bounds(commands) {
                        min_x = min_x.min(tx + b.x_min);
                        min_y = min_y.min(ty + b.y_min);
                        max_x = max_x.max(tx + b.x_max);
                        max_y = max_y.max(ty + b.y_max);
                    }
                }
                _ => {}
            }
        }
    }
    if min_x.is_finite() && max_x > min_x && max_y > min_y {
        [min_x, min_y, max_x - min_x, max_y - min_y]
    } else {
        fallback
    }
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
    scale9: Option<Scale9Mapping>,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        symbol: &Symbol,
        frame: u32,
        parent: &[Id],
        inherited: InheritedDisplay,
        overrides: &[InstancePropertyOverride],
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
                f64::from(self.project.fps),
            ));
        self.result
            .blend_groups
            .extend(crate::authored_layers::layer_blend_plans(
                &symbol.timeline,
                frame,
                parent,
                self.include_guides,
                f64::from(self.project.fps),
            ));
        // Layer zero is the backmost layer; list order is painter order everywhere.
        let active_camera = sample_timeline_camera(&symbol.timeline, frame);
        let stage_center = [
            self.project.width as f64 * 0.5,
            self.project.height as f64 * 0.5,
        ];
        for layer in &symbol.timeline.layers {
            if !layer_effectively_visible(&symbol.timeline, layer)
                || layer.properties.role == LayerRole::Folder
                || (!self.include_guides && layer.properties.role == LayerRole::Guide)
            {
                continue;
            }
            let layer_blend = layer_effective_blend_mode(&symbol.timeline, layer);
            let synced_layer = layer_effectively_synced(&symbol.timeline, layer);
            let active_key = layer.key_at(frame);
            let active_key_overrides = active_key
                .map(|k| k.instance_overrides.as_slice())
                .unwrap_or(&[]);
            let active_controllers = active_key
                .map(|k| k.controller_elements.as_slice())
                .unwrap_or(&[]);
            let (layer_world_base, layer_camera_ct) = if let Some(cam) = &active_camera
                && !layer.properties.attach_to_camera
            {
                let cam_m = camera_layer_matrix(cam, layer.properties.z_depth, stage_center);
                (
                    affine_multiply(inherited.matrix, cam_m),
                    cam.color_transform.then(inherited.color_transform),
                )
            } else {
                (inherited.matrix, inherited.color_transform)
            };
            for mut e in
                sample_layer_in_at_fps(&symbol.timeline, layer, frame, f64::from(self.project.fps))
            {
                if !self.include_guides && active_controllers.iter().any(|cid| cid == &e.id) {
                    continue;
                }
                if self.result.nodes.len() >= MAX_NODES {
                    return;
                }
                let mut override_frame: Option<u32> = None;
                for ov in overrides {
                    if (!e.name.is_empty() && ov.target_name == e.name) || ov.target_name == e.id {
                        if let Some(vis) = ov.visible {
                            e.visible = vis;
                        }
                        if let Some(f) = ov.frame {
                            override_frame = Some(f);
                        }
                        if let Some(txt) = &ov.text {
                            match &mut e.kind {
                                ElementKind::Text { text, .. }
                                | ElementKind::TextBox { text, .. } => {
                                    *text = txt.clone();
                                }
                                _ => {}
                            }
                        }
                        if let Some(col) = ov.color {
                            match &mut e.kind {
                                ElementKind::Text { color, .. } => *color = col,
                                ElementKind::TextBox { format, .. } => {
                                    format.color = ((col[0] as u32) << 16)
                                        | ((col[1] as u32) << 8)
                                        | (col[2] as u32);
                                }
                                ElementKind::Rectangle { fill, .. }
                                | ElementKind::Ellipse { fill, .. }
                                | ElementKind::Path { fill, .. }
                                | ElementKind::PaintedPath { fill, .. } => *fill = col,
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(s9) = &inherited.scale9 {
                    s9.apply_to_element(&mut e);
                }
                let mut path = parent.to_vec();
                path.push(e.id.clone());
                let patch = (self.patch)(&path);
                let visible = patch.as_ref().and_then(|p| p.visible).unwrap_or(e.visible);
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
                let world = affine_multiply(layer_world_base, transform_matrix(e.transform));
                let opacity = inherited.alpha * e.transform.alpha.clamp(0., 1.);
                let color_transform = e
                    .transform
                    .color_transform
                    .with_alpha(e.transform.alpha)
                    .then(layer_camera_ct);
                let effective_visible = inherited.visible && visible;
                let blend_mode = if e.blend_mode.is_normal() {
                    layer_blend
                } else {
                    e.blend_mode
                };
                let element_blend_mode = e.blend_mode;
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
                    blend_mode,
                    element_blend_mode,
                    cache_as_bitmap: e.cache_as_bitmap,
                    filters: e.filters.clone(),
                    kind: None,
                };
                if let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &e.kind
                {
                    if !element_blend_mode.is_normal() {
                        self.result
                            .blend_groups
                            .push(crate::authored_layers::instance_blend_plan(
                                &path,
                                element_blend_mode,
                            ));
                    }
                    let Some(child) = self.project.symbol(symbol_id) else {
                        continue;
                    };
                    let count = child.timeline.frame_count.max(1);
                    let age = if synced_layer {
                        frame
                    } else {
                        frame.saturating_sub(placement_start(layer, frame, &e.id))
                    };
                    let child_frame = if let Some(ov_f) = override_frame {
                        ov_f.min(count - 1)
                    } else if child.kind == SymbolKind::Button {
                        button_frame.min(2).min(count - 1)
                    } else if synced_layer {
                        instance_frame_from_age(*playback, age, count)
                    } else {
                        match playback {
                            InstancePlayback::Independent => {
                                (self.clock)(&path).unwrap_or(age % count) % count
                            }
                            _ => instance_frame_from_age(*playback, age, count),
                        }
                    };
                    node.symbol_id = Some(child.id.clone());
                    node.frame = child_frame;
                    node.independent = child.kind != SymbolKind::Button
                        && !synced_layer
                        && matches!(playback, InstancePlayback::Independent);
                    self.result.nodes.push(node);
                    let child_overrides: Vec<InstancePropertyOverride> = active_key_overrides
                        .iter()
                        .filter(|ov| {
                            ov.instance_id == e.id
                                || (!e.name.is_empty() && ov.instance_id == e.name)
                        })
                        .cloned()
                        .collect();
                    let active_s9 = e.scale9_grid().or_else(|| child.scale9_grid());
                    let (child_world, child_s9) = if let Some(grid) = active_s9
                        && ((e.transform.scale_x.abs() - 1.0).abs() > 1e-6
                            || (e.transform.scale_y.abs() - 1.0).abs() > 1e-6)
                    {
                        let mut unscaled = e.transform;
                        unscaled.scale_x = e.transform.scale_x.signum();
                        unscaled.scale_y = e.transform.scale_y.signum();
                        let bounds = symbol_natural_bounds(
                            child,
                            child_frame,
                            grid,
                            f64::from(self.project.fps),
                        );
                        (
                            affine_multiply(layer_world_base, transform_matrix(unscaled)),
                            Some(Scale9Mapping {
                                grid,
                                bounds,
                                sx: e.transform.scale_x.abs(),
                                sy: e.transform.scale_y.abs(),
                            }),
                        )
                    } else {
                        (world, inherited.scale9)
                    };
                    // Invisible clips still exist and advance their timelines.
                    self.walk(
                        child,
                        child_frame,
                        &path,
                        InheritedDisplay {
                            matrix: child_world,
                            alpha: opacity,
                            visible: effective_visible,
                            color_transform,
                            scale9: child_s9,
                        },
                        &child_overrides,
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
                            path_fingerprint: None,
                            kind,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod filter_evaluation_tests {
    use super::*;

    #[test]
    fn gradient_glow_and_bevel_interpolate_and_single_side_filters_tween_smoothly() {
        let gg_start = FilterSpec::GradientGlow {
            distance: 2.0,
            angle: 30.0,
            colors: vec![0xff0000, 0x00ff00],
            alphas: vec![0.2, 0.8],
            ratios: vec![0, 200],
            blur_x: 4.0,
            blur_y: 6.0,
            strength: 1.0,
            quality: 2,
            placement: "inner".into(),
            knockout: false,
        };
        let gg_end = FilterSpec::GradientGlow {
            distance: 10.0,
            angle: 90.0,
            colors: vec![0x0000ff, 0xffffff],
            alphas: vec![0.6, 1.0],
            ratios: vec![20, 255],
            blur_x: 12.0,
            blur_y: 14.0,
            strength: 3.0,
            quality: 2,
            placement: "inner".into(),
            knockout: false,
        };
        let mid = interpolate_filter_list(&[gg_start], std::slice::from_ref(&gg_end), 0.5);
        assert_eq!(mid.len(), 1);
        match &mid[0] {
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                ..
            } => {
                assert!((*distance - 6.0).abs() < 1e-6);
                assert!((*angle - 60.0).abs() < 1e-6);
                assert!((*blur_x - 8.0).abs() < 1e-6);
                assert!((*blur_y - 10.0).abs() < 1e-6);
                assert!((*strength - 2.0).abs() < 1e-6);
                assert_eq!(colors, &vec![0x800080, 0x80ff80]);
                assert!((alphas[0] - 0.4).abs() < 1e-6);
                assert!((alphas[1] - 0.9).abs() < 1e-6);
                assert_eq!(ratios, &vec![10, 228]);
            }
            other => panic!("expected GradientGlow, got {other:?}"),
        }

        // Single-side filter on start or end keyframe synthesizes a zero-strength/zero-blur dummy filter.
        let fade_in = interpolate_filter_list(&[], &[gg_end], 0.5);
        assert_eq!(fade_in.len(), 1);
        if let FilterSpec::GradientGlow {
            distance,
            blur_x,
            strength,
            alphas,
            ..
        } = &fade_in[0]
        {
            assert!((*distance - 5.0).abs() < 1e-6);
            assert!((*blur_x - 6.0).abs() < 1e-6);
            assert!((*strength - 1.5).abs() < 1e-6);
            assert!((alphas[0] - 0.3).abs() < 1e-6);
            assert!((alphas[1] - 0.5).abs() < 1e-6);
        } else {
            panic!("expected GradientGlow fade-in");
        }
    }

    #[test]
    fn keyframe_and_layer_filters_combine_and_interpolate_on_sampled_layer() {
        let mut project = Project::blank();
        let layer = &mut project.symbols[0].timeline.layers[0];
        layer.properties.filters = vec![FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 4.0,
            quality: 1,
        }];
        let base_el = Element {
            id: "box-1".into(),
            name: "box".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 30.0,
                fill: [200, 100, 50, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        layer.keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![base_el.clone()],
                tween: Some(Easing::Linear),
                filters: vec![FilterSpec::Glow {
                    color: 0xff0000,
                    alpha: 1.0,
                    blur_x: 10.0,
                    blur_y: 10.0,
                    strength: 2.0,
                    quality: 1,
                    inner: false,
                    knockout: false,
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 4,
                elements: vec![base_el],
                filters: vec![],
                ..Keyframe::default()
            },
        ];
        project.symbols[0].timeline.frame_count = 5;
        project.upgrade_format_for_features();
        assert_eq!(project.format_version, 11);
        project.validate().unwrap();

        let sampled = sample_layer(&project.symbols[0].timeline.layers[0], 2);
        assert_eq!(sampled.len(), 1);
        assert_eq!(sampled[0].filters.len(), 2);
        // Keyframe filter faded to 50% at frame 2 + layer Blur(4, 4).
        assert_eq!(
            sampled[0].filters[0],
            FilterSpec::Glow {
                color: 0xff0000,
                alpha: 0.5,
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.0,
                quality: 1,
                inner: false,
                knockout: false,
            }
        );
        assert_eq!(
            sampled[0].filters[1],
            FilterSpec::Blur {
                blur_x: 4.0,
                blur_y: 4.0,
                quality: 1,
            }
        );
    }
}
