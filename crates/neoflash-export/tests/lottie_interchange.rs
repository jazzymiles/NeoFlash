//! Comprehensive Lottie JSON (`.json`) and dotLottie (`.lottie` ZIP) interchange,
//! security/preview hardening, image transcoding, atomic cancellation, structured
//! diagnostics, and independent Bodymovin 5.x spec-conformance test suite (`T-308`).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ExtendedColorType, ImageEncoder};
use neoflash_core::{
    AppearancePaintEntry, AuthoredBitmapFill, BlendMode, CameraPose, ClassicRotateDirection,
    ColorTransform, Easing, Element, ElementAppearance, ElementKind, ElementParentLink, FillRule,
    FilterSpec, FrameScript, FrameSound, GradientKind, GradientPaint, GradientSpread, GradientStop,
    InstancePlayback, InstancePropertyOverride, Keyframe, Layer, LayerProperties, LayerRole,
    MaskMode, ObjectEvent, ObjectScript, OvalOptions, PathCommand, PatternBrushKind,
    PerspectiveGridSpec, PrimitiveOptions, Project, RectangleOptions, ResourceUnit, Script,
    ScriptLanguage, SourceUnit, StrokeProfilePreset, StrokeWidthProfile, Symbol, SymbolKind,
    TextFormat, Timeline, Transform, TweenKind, TweenProperty, flatten_scenes_with_ranges,
    sample_layer_in, transform_matrix,
};
use neoflash_export::{
    LOTTIE_VERSION, LottieFormat, LottieImageFormat, LottieImageMode, LottieMovieClipMode,
    LottieNoteKind, LottieOptions, LottieTargetPlayer, export_dotlottie, export_dotlottie_bytes,
    export_dotlottie_bytes_with_cancel, export_lottie, export_lottie_json_with_cancel,
    export_project_lottie, export_project_lottie_with_cancel, validate_dotlottie_bytes,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new(label: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        loop {
            let dir = std::env::temp_dir().join(format!(
                "neoflash-t308-{label}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("failed to create temp test dir {}: {e}", dir.display()),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn find_headless_chrome() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn run_chrome_dump_dom(html_path: &Path) -> Option<String> {
    let chrome = find_headless_chrome()?;
    let url = url::Url::from_file_path(html_path).ok()?;
    let profile_dir = TempTestDir::new("chrome-profile");
    let out = Command::new(chrome)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--allow-file-access-from-files")
        .arg(format!("--user-data-dir={}", profile_dir.path().display()))
        .arg("--virtual-time-budget=2500")
        .arg("--dump-dom")
        .arg(url.as_str())
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "headless Chrome failed for {}: status={}, stderr={}",
            html_path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn make_rgba_png(width: u32, height: u32, rgba_fn: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&rgba_fn(x, y));
        }
    }
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn make_rgb_bmp(width: u32, height: u32, rgb_fn: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&rgb_fn(x, y));
        }
    }
    let mut buf = Vec::new();
    image::codecs::bmp::BmpEncoder::new(&mut buf)
        .encode(&pixels, width, height, ExtendedColorType::Rgb8)
        .unwrap();
    buf
}

// ============================================================================
// Independent Bodymovin v5.x + dotLottie Spec-Conformance Evaluator
// ============================================================================

#[derive(Clone, Debug)]
struct IndependentLottieDoc {
    root: Value,
    assets_by_id: BTreeMap<String, Value>,
}

impl IndependentLottieDoc {
    fn from_value(root: Value) -> Self {
        assert_eq!(root["v"].as_str(), Some(LOTTIE_VERSION));
        let mut assets_by_id = BTreeMap::new();
        if let Some(assets) = root["assets"].as_array() {
            for asset in assets {
                let id = asset["id"].as_str().expect("asset must have string id");
                assert!(
                    assets_by_id.insert(id.to_owned(), asset.clone()).is_none(),
                    "duplicate asset id '{id}'"
                );
            }
        }
        Self { root, assets_by_id }
    }

    fn from_dotlottie_bytes(bytes: &[u8]) -> Self {
        let root = validate_dotlottie_bytes(bytes).expect("valid dotLottie archive");
        Self::from_value(root)
    }

    /// Evaluate the world 2D affine matrix `[a, b, c, d, tx, ty]` of a named layer
    /// inside `layers` at `frame`, resolving `parent` chains and `ao: 1` auto-orient.
    fn eval_layer_world_matrix(&self, layers: &[Value], ind: i64, frame: f64) -> [f64; 6] {
        let layer = layers
            .iter()
            .find(|l| l["ind"].as_i64() == Some(ind))
            .unwrap_or_else(|| panic!("missing layer ind={ind}"));
        let local = eval_ks_matrix(&layer["ks"], layer["ao"].as_i64() == Some(1), frame);
        if let Some(parent_ind) = layer["parent"].as_i64() {
            let parent_mat = self.eval_layer_world_matrix(layers, parent_ind, frame);
            mul_affine(parent_mat, local)
        } else {
            local
        }
    }

    /// Evaluate the internal precomp frame displayed by a `ty: 0` precomp layer at `parent_frame`.
    fn eval_precomp_child_frame(&self, layer: &Value, parent_frame: f64, fps: f64) -> f64 {
        assert_eq!(layer["ty"].as_i64(), Some(0));
        if layer.get("tm").is_some() && !layer["tm"].is_null() {
            let seconds = eval_scalar_prop(&layer["tm"], parent_frame);
            seconds * fps
        } else {
            let st = layer["st"].as_f64().unwrap_or(0.0);
            let sr = layer["sr"].as_f64().unwrap_or(1.0);
            (parent_frame - st) * sr
        }
    }
}

fn mul_affine(p: [f64; 6], c: [f64; 6]) -> [f64; 6] {
    [
        p[0] * c[0] + p[2] * c[1],
        p[1] * c[0] + p[3] * c[1],
        p[0] * c[2] + p[2] * c[3],
        p[1] * c[2] + p[3] * c[3],
        p[0] * c[4] + p[2] * c[5] + p[4],
        p[1] * c[4] + p[3] * c[5] + p[5],
    ]
}

fn transform_point(m: [f64; 6], pt: [f64; 2]) -> [f64; 2] {
    [
        m[0] * pt[0] + m[2] * pt[1] + m[4],
        m[1] * pt[0] + m[3] * pt[1] + m[5],
    ]
}

fn solve_cubic_bezier_y(x1: f64, y1: f64, x2: f64, y2: f64, u: f64) -> f64 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let bx = |s: f64| -> f64 {
        let inv = 1.0 - s;
        3.0 * inv * inv * s * x1 + 3.0 * inv * s * s * x2 + s * s * s
    };
    let dbx = |s: f64| -> f64 {
        let inv = 1.0 - s;
        3.0 * inv * inv * x1 + 6.0 * inv * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2)
    };
    let mut s = u;
    for _ in 0..12 {
        let err = bx(s) - u;
        if err.abs() < 1e-9 {
            break;
        }
        let d = dbx(s);
        if d.abs() < 1e-9 {
            break;
        }
        s = (s - err / d).clamp(0.0, 1.0);
    }
    let inv = 1.0 - s;
    3.0 * inv * inv * s * y1 + 3.0 * inv * s * s * y2 + s * s * s
}

fn handle_component(v: &Value, dim: usize) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        Some(n)
    } else if let Some(arr) = v.as_array() {
        arr.get(dim).or_else(|| arr.first()).and_then(Value::as_f64)
    } else {
        None
    }
}

fn eval_ease_factor(kf: &Value, dim: usize, raw_u: f64) -> f64 {
    if kf["h"].as_i64() == Some(1) {
        return 0.0;
    }
    let ox = handle_component(&kf["o"]["x"], dim).unwrap_or(0.0);
    let oy = handle_component(&kf["o"]["y"], dim).unwrap_or(0.0);
    let ix = handle_component(&kf["i"]["x"], dim).unwrap_or(1.0);
    let iy = handle_component(&kf["i"]["y"], dim).unwrap_or(1.0);
    solve_cubic_bezier_y(ox.clamp(0.0, 1.0), oy, ix.clamp(0.0, 1.0), iy, raw_u)
}

fn as_vec_f64(v: &Value) -> Vec<f64> {
    if let Some(n) = v.as_f64() {
        vec![n]
    } else if let Some(arr) = v.as_array() {
        arr.iter().filter_map(Value::as_f64).collect()
    } else {
        vec![]
    }
}

fn eval_vec_prop(prop: &Value, frame: f64) -> Vec<f64> {
    if prop["a"].as_i64() != Some(1) {
        return as_vec_f64(&prop["k"]);
    }
    let kfs = prop["k"].as_array().expect("animated k must be array");
    assert!(!kfs.is_empty());
    let first_t = kfs[0]["t"].as_f64().unwrap_or(0.0);
    if frame <= first_t || kfs.len() == 1 {
        return as_vec_f64(&kfs[0]["s"]);
    }
    let last_idx = kfs.len() - 1;
    let last_t = kfs[last_idx]["t"].as_f64().unwrap_or(0.0);
    if frame >= last_t {
        let s = as_vec_f64(&kfs[last_idx]["s"]);
        if !s.is_empty() {
            return s;
        }
        return as_vec_f64(&kfs[last_idx - 1]["e"]);
    }
    for pair in kfs.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let ta = a["t"].as_f64().unwrap();
        let tb = b["t"].as_f64().unwrap();
        if frame < tb {
            let sa = as_vec_f64(&a["s"]);
            let mut sb = as_vec_f64(&b["s"]);
            if sb.is_empty() {
                sb = as_vec_f64(&a["e"]);
            }
            if a["h"].as_i64() == Some(1) || (tb - ta).abs() <= 1e-9 {
                return sa;
            }
            let raw_u = ((frame - ta) / (tb - ta)).clamp(0.0, 1.0);
            // Spatial cubic bezier tangents (`to` / `ti`) on 2D/3D position keyframes
            if let (Some(to_arr), Some(ti_arr)) = (a["to"].as_array(), a["ti"].as_array())
                && sa.len() >= 2
                && sb.len() >= 2
                && (to_arr
                    .iter()
                    .any(|x| x.as_f64().unwrap_or(0.0).abs() > 1e-9)
                    || ti_arr
                        .iter()
                        .any(|x| x.as_f64().unwrap_or(0.0).abs() > 1e-9))
            {
                let u = eval_ease_factor(a, 0, raw_u).clamp(0.0, 1.0);
                let inv = 1.0 - u;
                let mut out = Vec::with_capacity(sa.len());
                for dim in 0..sa.len() {
                    let p0 = sa[dim];
                    let p3 = sb[dim];
                    let c1 = p0 + to_arr.get(dim).and_then(Value::as_f64).unwrap_or(0.0);
                    let c2 = p3 + ti_arr.get(dim).and_then(Value::as_f64).unwrap_or(0.0);
                    out.push(
                        inv * inv * inv * p0
                            + 3.0 * inv * inv * u * c1
                            + 3.0 * inv * u * u * c2
                            + u * u * u * p3,
                    );
                }
                return out;
            }
            return sa
                .iter()
                .enumerate()
                .map(|(dim, &va)| {
                    let vb = sb.get(dim).copied().unwrap_or(va);
                    let u = eval_ease_factor(a, dim, raw_u);
                    va + (vb - va) * u
                })
                .collect();
        }
    }
    as_vec_f64(&kfs[last_idx]["s"])
}

fn eval_scalar_prop(prop: &Value, frame: f64) -> f64 {
    eval_vec_prop(prop, frame).first().copied().unwrap_or(0.0)
}

fn eval_position_and_auto_orient(p_prop: &Value, auto_orient: bool, frame: f64) -> ([f64; 2], f64) {
    let pos = if p_prop["s"].as_bool() == Some(true) {
        [
            eval_scalar_prop(&p_prop["x"], frame),
            eval_scalar_prop(&p_prop["y"], frame),
        ]
    } else {
        let v = eval_vec_prop(p_prop, frame);
        [
            v.first().copied().unwrap_or(0.0),
            v.get(1).copied().unwrap_or(0.0),
        ]
    };
    if !auto_orient {
        return (pos, 0.0);
    }
    let eps = 1e-2;
    let sample_p = |t: f64| -> [f64; 2] {
        if p_prop["s"].as_bool() == Some(true) {
            [
                eval_scalar_prop(&p_prop["x"], t),
                eval_scalar_prop(&p_prop["y"], t),
            ]
        } else {
            let v = eval_vec_prop(p_prop, t);
            [
                v.first().copied().unwrap_or(0.0),
                v.get(1).copied().unwrap_or(0.0),
            ]
        }
    };
    let p_fwd = sample_p(frame + eps);
    let (dx, dy) = if (p_fwd[0] - pos[0]).hypot(p_fwd[1] - pos[1]) > 1e-9 {
        (p_fwd[0] - pos[0], p_fwd[1] - pos[1])
    } else {
        let p_bwd = sample_p((frame - eps).max(0.0));
        (pos[0] - p_bwd[0], pos[1] - p_bwd[1])
    };
    let orient_deg = if dx.hypot(dy) > 1e-9 {
        dy.atan2(dx).to_degrees()
    } else {
        0.0
    };
    (pos, orient_deg)
}

/// Evaluate Bodymovin `ks` 2D affine matrix:
/// `M = T(p) * R(r + auto_orient) * Skew(sk, sa) * S(s / 100) * T(-a)`
/// where Lottie's `sk` skews the Y axis by `-tan(sk)` in rotated frame (with `sa = 0`).
fn eval_ks_matrix(ks: &Value, auto_orient: bool, frame: f64) -> [f64; 6] {
    let a = if ks.get("a").is_some() && !ks["a"].is_null() {
        let v = eval_vec_prop(&ks["a"], frame);
        [
            v.first().copied().unwrap_or(0.0),
            v.get(1).copied().unwrap_or(0.0),
        ]
    } else {
        [0.0, 0.0]
    };
    let (p, ao_deg) = if ks.get("p").is_some() && !ks["p"].is_null() {
        eval_position_and_auto_orient(&ks["p"], auto_orient, frame)
    } else {
        ([0.0, 0.0], 0.0)
    };
    let s = if ks.get("s").is_some() && !ks["s"].is_null() {
        let v = eval_vec_prop(&ks["s"], frame);
        [
            v.first().copied().unwrap_or(100.0) / 100.0,
            v.get(1).copied().unwrap_or(100.0) / 100.0,
        ]
    } else {
        [1.0, 1.0]
    };
    let r_deg = if ks.get("r").is_some() && !ks["r"].is_null() {
        eval_scalar_prop(&ks["r"], frame)
    } else {
        0.0
    } + ao_deg;
    let sk_deg = if ks.get("sk").is_some() && !ks["sk"].is_null() {
        eval_scalar_prop(&ks["sk"], frame)
    } else {
        0.0
    };
    let sa_deg = if ks.get("sa").is_some() && !ks["sa"].is_null() {
        eval_scalar_prop(&ks["sa"], frame)
    } else {
        0.0
    };

    let t_p = [1.0, 0.0, 0.0, 1.0, p[0], p[1]];
    let rad = r_deg.to_radians();
    let (sr, cr) = rad.sin_cos();
    let m_r = [cr, sr, -sr, cr, 0.0, 0.0];

    let m_skew = if sk_deg.abs() > 1e-9 {
        let sa_rad = sa_deg.to_radians();
        let (ssa, csa) = sa_rad.sin_cos();
        let rot_neg_sa = [csa, -ssa, ssa, csa, 0.0, 0.0];
        let rot_pos_sa = [csa, ssa, -ssa, csa, 0.0, 0.0];
        let shear = [1.0, 0.0, -sk_deg.to_radians().tan(), 1.0, 0.0, 0.0];
        mul_affine(rot_neg_sa, mul_affine(shear, rot_pos_sa))
    } else {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    };
    let m_s = [s[0], 0.0, 0.0, s[1], 0.0, 0.0];
    let t_neg_a = [1.0, 0.0, 0.0, 1.0, -a[0], -a[1]];

    mul_affine(
        t_p,
        mul_affine(m_r, mul_affine(m_skew, mul_affine(m_s, t_neg_a))),
    )
}

#[derive(Clone, Debug)]
struct LottieBezierShape {
    closed: bool,
    v: Vec<[f64; 2]>,
    i: Vec<[f64; 2]>,
    o: Vec<[f64; 2]>,
}

fn parse_bezier_dict(val: &Value) -> LottieBezierShape {
    let closed = val["c"].as_bool().unwrap_or(false);
    let parse_pts = |arr: &Value| -> Vec<[f64; 2]> {
        arr.as_array()
            .expect("bezier point array")
            .iter()
            .map(|pt| [pt[0].as_f64().unwrap(), pt[1].as_f64().unwrap()])
            .collect()
    };
    let v = parse_pts(&val["v"]);
    let i = parse_pts(&val["i"]);
    let o = parse_pts(&val["o"]);
    assert_eq!(v.len(), i.len());
    assert_eq!(v.len(), o.len());
    LottieBezierShape { closed, v, i, o }
}

fn eval_shape_ks(ks: &Value, frame: f64) -> LottieBezierShape {
    if ks["a"].as_i64() != Some(1) {
        return parse_bezier_dict(&ks["k"]);
    }
    let kfs = ks["k"].as_array().expect("animated shape kfs");
    let first_t = kfs[0]["t"].as_f64().unwrap();
    if frame <= first_t || kfs.len() == 1 {
        return parse_bezier_dict(&kfs[0]["s"][0]);
    }
    let last_idx = kfs.len() - 1;
    let last_t = kfs[last_idx]["t"].as_f64().unwrap();
    if frame >= last_t {
        if let Some(s0) = kfs[last_idx]["s"].as_array().and_then(|a| a.first()) {
            return parse_bezier_dict(s0);
        }
        return parse_bezier_dict(&kfs[last_idx - 1]["e"][0]);
    }
    for pair in kfs.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let ta = a["t"].as_f64().unwrap();
        let tb = b["t"].as_f64().unwrap();
        if frame < tb {
            let sa = parse_bezier_dict(&a["s"][0]);
            if a["h"].as_i64() == Some(1) || (tb - ta).abs() <= 1e-9 {
                return sa;
            }
            let sb = if let Some(s0) = b["s"].as_array().and_then(|arr| arr.first()) {
                parse_bezier_dict(s0)
            } else {
                parse_bezier_dict(&a["e"][0])
            };
            assert_eq!(
                sa.v.len(),
                sb.v.len(),
                "animated shape keyframes must have matching vertex counts"
            );
            let raw_u = ((frame - ta) / (tb - ta)).clamp(0.0, 1.0);
            let u = eval_ease_factor(a, 0, raw_u);
            let lerp2 = |p0: [f64; 2], p1: [f64; 2]| -> [f64; 2] {
                [p0[0] + (p1[0] - p0[0]) * u, p0[1] + (p1[1] - p0[1]) * u]
            };
            return LottieBezierShape {
                closed: sa.closed,
                v: sa
                    .v
                    .iter()
                    .zip(&sb.v)
                    .map(|(&p0, &p1)| lerp2(p0, p1))
                    .collect(),
                i: sa
                    .i
                    .iter()
                    .zip(&sb.i)
                    .map(|(&p0, &p1)| lerp2(p0, p1))
                    .collect(),
                o: sa
                    .o
                    .iter()
                    .zip(&sb.o)
                    .map(|(&p0, &p1)| lerp2(p0, p1))
                    .collect(),
            };
        }
    }
    parse_bezier_dict(&kfs[last_idx]["s"][0])
}

// ============================================================================
// 1. Defect 1: Hostile `</script>`, `\u2028`, `\u2029` Neutralization + Chrome Smoke
// ============================================================================

#[test]
fn defect_1_html_preview_neutralizes_script_injection_and_unicode_separators_in_chrome() {
    let dir = TempTestDir::new("defect1-xss");
    let hostile = "</script><script>window.__xss=1;window.__t051_marker=1;</script>\u{2028}\u{2029}<img src=x onerror=\"window.__xss=1\">";

    let mut project = Project::blank();
    project.name = format!("Title {hostile}");
    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.name = format!("Scene {hostile}");
        root.timeline.frame_count = 6;
        root.timeline.layers[0].name = format!("Layer {hostile}");
        root.timeline.layers[0].keyframes[0].label = format!("Marker {hostile}");
        root.timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "txt-static".into(),
                name: format!("Static {hostile}"),
                transform: Transform {
                    x: 120.0,
                    y: 90.0,
                    ..Transform::default()
                },
                kind: ElementKind::Text {
                    text: format!("Payload {hostile}"),
                    font_family: "Arial".into(),
                    font_size: 20.0,
                    color: [255, 255, 255, 255],
                },
                ..Element::default()
            },
            Element {
                id: "txt-box".into(),
                name: format!("Dynamic {hostile}"),
                transform: Transform {
                    x: 120.0,
                    y: 160.0,
                    ..Transform::default()
                },
                kind: ElementKind::TextBox {
                    text: format!("Box {hostile}"),
                    width: 240.0,
                    height: 60.0,
                    format: TextFormat::default(),
                    word_wrap: true,
                    multiline: true,
                },
                ..Element::default()
            },
        ];
    }

    for (fmt, out_name) in [
        (LottieFormat::Json, "xss_loose.json"),
        (LottieFormat::DotLottie, "xss_pkg.lottie"),
    ] {
        let out_path = dir.path().join(out_name);
        let report = export_project_lottie(
            &project,
            &out_path,
            &LottieOptions {
                format: fmt,
                html_preview: true,
                pretty: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();

        let preview_path = report
            .html_preview_path
            .as_ref()
            .expect("html_preview_path must be set");
        let html = fs::read_to_string(preview_path).unwrap();

        // Raw HTML must never contain unescaped script-closing payload or raw U+2028/U+2029.
        assert!(
            !html.contains("</script><script>"),
            "preview HTML leaked unescaped </script> breakout!"
        );
        assert!(
            !html.contains("<img src=x"),
            "preview HTML leaked unescaped HTML tag!"
        );
        assert!(
            !html.contains('\u{2028}') && !html.contains('\u{2029}'),
            "preview HTML leaked raw Unicode line/paragraph separator!"
        );
        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(html.contains("\\u2028"));
        assert!(html.contains("\\u2029"));

        // Verify in real headless Chrome that the inline script parses cleanly, executes zero
        // injected payload, and preserves the exact round-tripped strings in `animationData`.
        if let Some(dom) = run_chrome_dump_dom(preview_path) {
            assert!(
                dom.contains("data-lottie-ready=\"true\""),
                "Chrome preview did not reach ready state (script syntax error?):\n{dom}"
            );
            assert!(
                dom.contains("data-lottie-xss=\"false\""),
                "Hostile script executed in Chrome preview! DOM:\n{dom}"
            );
            assert!(dom.contains("data-lottie-dangling-refs=\"0\""));
        }
    }
}

// ============================================================================
// 2. Defect 2: Every Referenced Image Asset Present Across Loose JSON,
//    Preview HTML, dotLottie ZIP & Manifest + Real Chrome Smoke
// ============================================================================

#[test]
fn defect_2_every_referenced_image_asset_is_present_across_json_dotlottie_zip_and_preview_in_chrome()
 {
    let dir = TempTestDir::new("defect2-assets");
    let png_red = make_rgba_png(8, 6, |_, _| [240, 40, 40, 255]);
    let png_green = make_rgba_png(6, 8, |_, _| [40, 220, 80, 255]);
    let png_blue = make_rgba_png(10, 10, |x, y| {
        [40, 100, 245, if (x + y) % 2 == 0 { 255 } else { 180 }]
    });

    let mut project = Project::blank();
    project.format_version = 9;
    // Include two resources whose sanitized slugs collide ("hero/a" -> "hero_a" vs "hero_a")
    // plus one nested precomp bitmap reused across both root and nested precomp!
    project.resources.push(
        ResourceUnit::from_bytes("hero/a", "bitmaps/hero_slash.png", "image/png", &png_red)
            .unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes("hero_a", "bitmaps/hero_under.png", "image/png", &png_green)
            .unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes("badge", "bitmaps/badge.png", "image/png", &png_blue).unwrap(),
    );

    let nested_sym = Symbol {
        id: "sym-badge".into(),
        name: "NestedBadge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "badge-layer".into(),
                name: "BadgeLayer".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "nested-bmp".into(),
                        name: "NestedBmp".into(),
                        transform: Transform {
                            x: 20.0,
                            y: 20.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Bitmap {
                            resource_id: "badge".into(),
                            width: 10,
                            height: 10,
                            smoothing: true,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };
    project.symbols.push(nested_sym);

    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 6;
        root.timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "bmp-1".into(),
                name: "HeroSlash".into(),
                transform: Transform {
                    x: 40.0,
                    y: 40.0,
                    ..Transform::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "hero/a".into(),
                    width: 8,
                    height: 6,
                    smoothing: true,
                },
                ..Element::default()
            },
            Element {
                id: "bmp-2".into(),
                name: "HeroUnder".into(),
                transform: Transform {
                    x: 90.0,
                    y: 40.0,
                    ..Transform::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "hero_a".into(),
                    width: 6,
                    height: 8,
                    smoothing: true,
                },
                ..Element::default()
            },
            Element {
                id: "inst-badge".into(),
                name: "BadgeInst".into(),
                transform: Transform {
                    x: 150.0,
                    y: 60.0,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: "sym-badge".into(),
                    playback: InstancePlayback::Loop,
                },
                ..Element::default()
            },
        ];
    }

    // Also verify invalid/missing bitmap resource projects fail validation atomically without writing files.
    let mut bad_proj = project.clone();
    bad_proj.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "bmp-missing".into(),
            name: "MissingBmp".into(),
            transform: Transform::default(),
            kind: ElementKind::Bitmap {
                resource_id: "does-not-exist".into(),
                width: 16,
                height: 16,
                smoothing: false,
            },
            ..Element::default()
        });
    let bad_out = dir.path().join("should_not_exist.lottie");
    assert!(export_project_lottie(&bad_proj, &bad_out, &LottieOptions::default()).is_err());
    assert!(!bad_out.exists());

    for (case_name, format, image_mode) in [
        ("json_embed", LottieFormat::Json, LottieImageMode::Embed),
        ("json_folder", LottieFormat::Json, LottieImageMode::Folder),
        ("dot_embed", LottieFormat::DotLottie, LottieImageMode::Embed),
        (
            "dot_folder",
            LottieFormat::DotLottie,
            LottieImageMode::Folder,
        ),
    ] {
        let sub = dir.path().join(case_name);
        fs::create_dir_all(&sub).unwrap();
        let ext = match format {
            LottieFormat::Json => "json",
            LottieFormat::DotLottie => "lottie",
        };
        let out_path = sub.join(format!("export.{ext}"));
        let report = export_project_lottie(
            &project,
            &out_path,
            &LottieOptions {
                format,
                image_mode,
                html_preview: true,
                pretty: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();

        let anim_json: Value = if format == LottieFormat::DotLottie {
            let zip_bytes = fs::read(&out_path).unwrap();
            let parsed = validate_dotlottie_bytes(&zip_bytes).unwrap();
            // Also verify manifest.json and ZIP image entries directly.
            let mut za = zip::ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
            let mut manifest_str = String::new();
            za.by_name("manifest.json")
                .unwrap()
                .read_to_string(&mut manifest_str)
                .unwrap();
            let manifest: Value = serde_json::from_str(&manifest_str).unwrap();
            assert_eq!(manifest["version"], "1.0");
            assert_eq!(manifest["generator"], "NeoFlash");
            assert_eq!(manifest["animations"][0]["id"], "animation");

            // All 3 valid bitmaps are packaged inside `images/*` in the `.lottie` archive.
            let image_assets: Vec<&Value> = parsed["assets"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a.get("p").is_some())
                .collect();
            assert_eq!(
                image_assets.len(),
                3,
                "3 valid bitmaps expected in {case_name}"
            );
            let mut seen_filenames = BTreeSet::new();
            for ia in image_assets {
                assert_eq!(ia["e"].as_i64(), Some(0));
                assert_eq!(ia["u"].as_str(), Some("images/"));
                let p = ia["p"].as_str().unwrap();
                assert!(
                    seen_filenames.insert(p.to_owned()),
                    "collision on image filename {p}"
                );
                let zip_rel = format!("images/{p}");
                assert!(
                    za.by_name(&zip_rel).is_ok(),
                    "dotLottie ZIP missing '{zip_rel}' in {case_name}"
                );
                // Because `html_preview: true`, sibling `images/<p>` must ALSO exist on disk so
                // `<stem>.preview.html` loads every image without 404s!
                assert!(
                    sub.join(&zip_rel).is_file(),
                    "sibling preview image '{}' missing on disk in {case_name}",
                    sub.join(&zip_rel).display()
                );
            }
            parsed
        } else {
            let raw = fs::read_to_string(&out_path).unwrap();
            let parsed: Value = serde_json::from_str(&raw).unwrap();
            let image_assets: Vec<&Value> = parsed["assets"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a.get("p").is_some())
                .collect();
            assert_eq!(image_assets.len(), 3);
            for ia in image_assets {
                if image_mode == LottieImageMode::Embed {
                    assert_eq!(ia["e"].as_i64(), Some(1));
                    assert!(
                        ia["p"]
                            .as_str()
                            .unwrap()
                            .starts_with("data:image/png;base64,")
                    );
                } else {
                    assert_eq!(ia["e"].as_i64(), Some(0));
                    let rel = format!("{}{}", ia["u"].as_str().unwrap(), ia["p"].as_str().unwrap());
                    assert!(
                        sub.join(&rel).is_file(),
                        "loose folder image '{}' missing on disk",
                        sub.join(&rel).display()
                    );
                }
            }
            parsed
        };

        // Verify zero dangling `refId`s across root and precomps.
        let doc = IndependentLottieDoc::from_value(anim_json);
        for layer in doc.root["layers"].as_array().unwrap() {
            if let Some(r) = layer["refId"].as_str() {
                assert!(doc.assets_by_id.contains_key(r), "dangling root refId {r}");
            }
        }
        for asset in doc.assets_by_id.values() {
            if let Some(comp_layers) = asset["layers"].as_array() {
                for layer in comp_layers {
                    if let Some(r) = layer["refId"].as_str() {
                        assert!(
                            doc.assets_by_id.contains_key(r),
                            "dangling precomp refId {r}"
                        );
                    }
                }
            }
        }

        // Verify in real headless Chrome that ALL 3 images decode & load cleanly in preview HTML!
        let preview_path = report.html_preview_path.as_ref().unwrap();
        if let Some(dom) = run_chrome_dump_dom(preview_path) {
            assert!(
                dom.contains("data-lottie-ready=\"true\""),
                "Chrome preview not ready in {case_name}:\n{dom}"
            );
            assert!(
                dom.contains("data-lottie-dangling-refs=\"0\""),
                "Dangling refs in Chrome preview ({case_name}):\n{dom}"
            );
            assert!(
                dom.contains("data-lottie-images-expected=\"3\""),
                "Expected 3 image assets in Chrome preview ({case_name}):\n{dom}"
            );
            assert!(
                dom.contains("data-lottie-images-loaded=\"3\"")
                    && dom.contains("data-lottie-images-failed=\"0\"")
                    && dom.contains("data-lottie-images-verified=\"true\""),
                "Image asset failed to load in Chrome preview ({case_name}):\n{dom}"
            );
        }
    }
}

// ============================================================================
// 3. Defect 3: Real PNG / JPEG / WebP Encoding — MIME, Extension, Magic Bytes
//    & Decoded Pixel Verification
// ============================================================================

#[test]
fn defect_3_png_jpeg_webp_transcoding_matches_mime_extension_magic_bytes_and_decoded_pixels() {
    let dir = TempTestDir::new("defect3-transcode");
    let stage_bg = [40u8, 80, 160, 255];

    // 1. Source BMP (`BM` magic — unsupported raw in Lottie, must transcode even on `Original`)
    let bmp_bytes = make_rgb_bmp(8, 8, |x, y| {
        if x < 4 && y < 4 {
            [220, 30, 50]
        } else {
            [30, 190, 90]
        }
    });
    assert_eq!(&bmp_bytes[0..2], b"BM");

    // 2. Source PNG with 50% translucent alpha (`128`) to prove JPEG composites over `stage_bg`
    //    and PNG/WebP preserve exact RGBA alpha!
    let translucent_px = [240u8, 200, 40, 128];
    let png_alpha_bytes = make_rgba_png(8, 8, |_, _| translucent_px);
    assert_eq!(&png_alpha_bytes[0..8], b"\x89PNG\r\n\x1a\n");

    let mut project = Project::blank();
    project.format_version = 9;
    project.background = stage_bg;
    project.resources.push(
        ResourceUnit::from_bytes("res-bmp", "img/quad.bmp", "image/bmp", &bmp_bytes).unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes(
            "res-alpha",
            "img/translucent.png",
            "image/png",
            &png_alpha_bytes,
        )
        .unwrap(),
    );
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: "e-bmp".into(),
            name: "BmpQuad".into(),
            transform: Transform {
                x: 20.0,
                y: 20.0,
                ..Transform::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "res-bmp".into(),
                width: 8,
                height: 8,
                smoothing: false,
            },
            ..Element::default()
        },
        Element {
            id: "e-alpha".into(),
            name: "AlphaPng".into(),
            transform: Transform {
                x: 60.0,
                y: 20.0,
                ..Transform::default()
            },
            kind: ElementKind::Bitmap {
                resource_id: "res-alpha".into(),
                width: 8,
                height: 8,
                smoothing: false,
            },
            ..Element::default()
        },
    ];

    let assert_encoded_image =
        |raw_bytes: &[u8], img_fmt: LottieImageFormat, is_alpha_source: bool| {
            match img_fmt {
                LottieImageFormat::Original | LottieImageFormat::Png => {
                    assert!(
                        raw_bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                        "expected PNG magic bytes for {img_fmt:?}"
                    );
                    let decoded =
                        image::load_from_memory_with_format(raw_bytes, image::ImageFormat::Png)
                            .expect("must decode strictly as PNG")
                            .to_rgba8();
                    assert_eq!(decoded.dimensions(), (8, 8));
                    if is_alpha_source {
                        assert_eq!(decoded.get_pixel(2, 2).0, translucent_px);
                    } else {
                        assert_eq!(decoded.get_pixel(1, 1).0, [220, 30, 50, 255]);
                        assert_eq!(decoded.get_pixel(6, 6).0, [30, 190, 90, 255]);
                    }
                }
                LottieImageFormat::Jpeg => {
                    assert!(
                        raw_bytes.starts_with(b"\xFF\xD8\xFF"),
                        "expected JPEG FF D8 FF magic bytes (not renamed PNG!)"
                    );
                    let decoded =
                        image::load_from_memory_with_format(raw_bytes, image::ImageFormat::Jpeg)
                            .expect("must decode strictly as JPEG")
                            .to_rgba8();
                    assert_eq!(decoded.dimensions(), (8, 8));
                    if is_alpha_source {
                        // Expected composite of [240, 200, 40, 128] over [40, 80, 160]
                        let expected_r = ((240u16 * 128 + 40u16 * 127 + 127) / 255) as i16;
                        let expected_g = ((200u16 * 128 + 80u16 * 127 + 127) / 255) as i16;
                        let expected_b = ((40u16 * 128 + 160u16 * 127 + 127) / 255) as i16;
                        let px = decoded.get_pixel(4, 4).0;
                        assert!((px[0] as i16 - expected_r).abs() <= 6, "jpeg R: {px:?}");
                        assert!((px[1] as i16 - expected_g).abs() <= 6, "jpeg G: {px:?}");
                        assert!((px[2] as i16 - expected_b).abs() <= 6, "jpeg B: {px:?}");
                        assert_eq!(px[3], 255);
                    }
                }
                LottieImageFormat::Webp => {
                    assert!(
                        raw_bytes.len() >= 12
                            && &raw_bytes[0..4] == b"RIFF"
                            && &raw_bytes[8..12] == b"WEBP",
                        "expected RIFF....WEBP magic bytes (not renamed PNG!)"
                    );
                    let decoded =
                        image::load_from_memory_with_format(raw_bytes, image::ImageFormat::WebP)
                            .expect("must decode strictly as WebP")
                            .to_rgba8();
                    assert_eq!(decoded.dimensions(), (8, 8));
                    if is_alpha_source {
                        assert_eq!(decoded.get_pixel(2, 2).0, translucent_px);
                    } else {
                        assert_eq!(decoded.get_pixel(1, 1).0, [220, 30, 50, 255]);
                        assert_eq!(decoded.get_pixel(6, 6).0, [30, 190, 90, 255]);
                    }
                }
            }
        };

    for (img_fmt, expected_mime, expected_ext) in [
        (
            LottieImageFormat::Original,
            "data:image/png;base64,",
            ".png",
        ),
        (LottieImageFormat::Png, "data:image/png;base64,", ".png"),
        (LottieImageFormat::Jpeg, "data:image/jpeg;base64,", ".jpg"),
        (LottieImageFormat::Webp, "data:image/webp;base64,", ".webp"),
    ] {
        // A. Loose JSON + Embed mode
        let (val_embed, rep_embed) = export_lottie(
            &project,
            &LottieOptions {
                format: LottieFormat::Json,
                image_mode: LottieImageMode::Embed,
                image_format: img_fmt,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        if img_fmt == LottieImageFormat::Original {
            assert!(
                rep_embed
                    .notes
                    .iter()
                    .any(|n| n.code.as_deref() == Some("LOTTIE_BITMAP_TRANSCODED"))
            );
        }
        if img_fmt == LottieImageFormat::Jpeg {
            assert!(
                rep_embed
                    .notes
                    .iter()
                    .any(|n| n.code.as_deref() == Some("LOTTIE_JPEG_ALPHA_COMPOSITED"))
            );
        }
        for asset in val_embed["assets"].as_array().unwrap() {
            let id = asset["id"].as_str().unwrap();
            let uri = asset["p"].as_str().unwrap();
            assert!(
                uri.starts_with(expected_mime),
                "expected MIME prefix '{expected_mime}' for {img_fmt:?}, got '{uri}'"
            );
            let b64 = uri.strip_prefix(expected_mime).unwrap();
            let raw = STANDARD.decode(b64).unwrap();
            assert_encoded_image(&raw, img_fmt, id.contains("alpha"));
        }

        // B. Loose JSON + Folder mode
        let folder_sub = dir.path().join(format!("folder_{img_fmt:?}"));
        fs::create_dir_all(&folder_sub).unwrap();
        let json_out = folder_sub.join("anim.json");
        export_project_lottie(
            &project,
            &json_out,
            &LottieOptions {
                format: LottieFormat::Json,
                image_mode: LottieImageMode::Folder,
                image_format: img_fmt,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let parsed_folder: Value =
            serde_json::from_str(&fs::read_to_string(&json_out).unwrap()).unwrap();
        for asset in parsed_folder["assets"].as_array().unwrap() {
            let id = asset["id"].as_str().unwrap();
            let p = asset["p"].as_str().unwrap();
            assert!(
                p.ends_with(expected_ext),
                "expected extension '{expected_ext}' for {img_fmt:?}, got '{p}'"
            );
            let disk_bytes = fs::read(folder_sub.join("images").join(p)).unwrap();
            assert_encoded_image(&disk_bytes, img_fmt, id.contains("alpha"));
        }

        // C. dotLottie (.lottie) ZIP archive
        let (zip_bytes, _) = export_dotlottie_bytes(
            &project,
            &LottieOptions {
                image_format: img_fmt,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let dot_anim = validate_dotlottie_bytes(&zip_bytes).unwrap();
        let mut za = zip::ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
        for asset in dot_anim["assets"].as_array().unwrap() {
            let id = asset["id"].as_str().unwrap();
            let p = asset["p"].as_str().unwrap();
            assert!(p.ends_with(expected_ext));
            let mut entry_bytes = Vec::new();
            za.by_name(&format!("images/{p}"))
                .unwrap()
                .read_to_end(&mut entry_bytes)
                .unwrap();
            assert_encoded_image(&entry_bytes, img_fmt, id.contains("alpha"));
        }
    }
}

// ============================================================================
// 4. Defect 4: Cancellable & Atomic Estimate + Export & Malformed ZIP Rejection
// ============================================================================

#[test]
fn defect_4_atomic_cancellation_and_dotlottie_archive_hardening() {
    let dir = TempTestDir::new("defect4-atomic");
    let png = make_rgba_png(12, 12, |x, y| [x as u8 * 16, y as u8 * 16, 200, 255]);

    let mut project = Project::blank();
    project.format_version = 9;
    project
        .resources
        .push(ResourceUnit::from_bytes("bmp", "img/pic.png", "image/png", &png).unwrap());
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.frame_count = 24;
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            tween: Some(Easing::EaseInOut),
            elements: vec![Element {
                id: "b1".into(),
                name: "Pic".into(),
                transform: Transform {
                    x: 10.0,
                    y: 20.0,
                    ..Transform::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "bmp".into(),
                    width: 12,
                    height: 12,
                    smoothing: true,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 23,
            elements: vec![Element {
                id: "b1".into(),
                name: "Pic".into(),
                transform: Transform {
                    x: 220.0,
                    y: 140.0,
                    rotation: 90.0,
                    ..Transform::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "bmp".into(),
                    width: 12,
                    height: 12,
                    smoothing: true,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        },
    ];

    // 1. Estimate & in-memory dotLottie cancellation via AtomicBool
    let pre_cancelled = AtomicBool::new(true);
    assert!(
        export_lottie_json_with_cancel(&project, &LottieOptions::default(), &pre_cancelled, |_| {
            true
        })
        .is_err()
    );
    assert!(
        export_dotlottie_bytes_with_cancel(
            &project,
            &LottieOptions::default(),
            &pre_cancelled,
            |_| true
        )
        .is_err()
    );

    // 2. Cancelling at EVERY progress step (0..=4) for both loose Folder JSON + HTML preview
    //    and dotLottie + HTML preview must:
    //    (a) leave ZERO files or directories in a fresh target folder, AND
    //    (b) leave pre-existing target files 100% untouched!
    for (label, fmt, ext) in [
        ("json_folder", LottieFormat::Json, "json"),
        ("dotlottie", LottieFormat::DotLottie, "lottie"),
    ] {
        for cancel_step in 0..=4usize {
            // (a) Fresh directory (including uncreated nested subdir `nested/out.<ext>`)
            let clean_parent = dir.path().join(format!("clean_{label}_{cancel_step}"));
            fs::create_dir_all(&clean_parent).unwrap();
            let nested_target = clean_parent.join("nested").join(format!("out.{ext}"));
            let cancel_flag = AtomicBool::new(false);
            let res = export_project_lottie_with_cancel(
                &project,
                &nested_target,
                &LottieOptions {
                    format: fmt,
                    image_mode: LottieImageMode::Folder,
                    html_preview: true,
                    pretty: true,
                    ..LottieOptions::default()
                },
                &cancel_flag,
                |p| {
                    if p.completed_steps >= cancel_step {
                        cancel_flag.store(true, Ordering::Relaxed);
                    }
                    true
                },
            );
            assert!(
                res.is_err(),
                "expected cancellation error at step {cancel_step} for {label}"
            );
            let leftover: Vec<_> = fs::read_dir(&clean_parent)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                leftover.is_empty(),
                "Cancel at step {cancel_step} ({label}) left partial files/dirs: {leftover:?}"
            );

            // (b) Existing file overwrite protection
            let overwrite_dir = dir.path().join(format!("overwrite_{label}_{cancel_step}"));
            fs::create_dir_all(&overwrite_dir).unwrap();
            let existing_file = overwrite_dir.join(format!("existing.{ext}"));
            let original_bytes = format!("ORIGINAL_SENTINEL_{label}_{cancel_step}").into_bytes();
            fs::write(&existing_file, &original_bytes).unwrap();

            let cancel_overwrite = AtomicBool::new(false);
            let res_ow = export_project_lottie_with_cancel(
                &project,
                &existing_file,
                &LottieOptions {
                    format: fmt,
                    image_mode: LottieImageMode::Folder,
                    html_preview: true,
                    ..LottieOptions::default()
                },
                &cancel_overwrite,
                |p| {
                    if p.completed_steps >= cancel_step {
                        cancel_overwrite.store(true, Ordering::Relaxed);
                    }
                    true
                },
            );
            assert!(res_ow.is_err());
            assert_eq!(
                fs::read(&existing_file).unwrap(),
                original_bytes,
                "Cancel at step {cancel_step} ({label}) corrupted existing file!"
            );
            let ow_entries: Vec<_> = fs::read_dir(&overwrite_dir)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                ow_entries,
                vec![format!("existing.{ext}")],
                "Cancel left stray temp/preview/images entries: {ow_entries:?}"
            );
        }
    }

    // 3. `validate_dotlottie_bytes` rejects path traversal, missing manifest/animation,
    //    dangling refIds, missing image entries, and fake image magic bytes.
    assert!(validate_dotlottie_bytes(b"not a zip").is_err());

    let build_custom_zip = |files: &[(&str, &[u8])]| -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for &(name, data) in files {
            zw.start_file(name, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap().into_inner()
    };

    let valid_manifest =
        br#"{"version":"1.0","generator":"NeoFlash","animations":[{"id":"animation"}]}"#;
    let minimal_anim =
        br#"{"v":"5.7.14","fr":24,"ip":0,"op":10,"w":100,"h":100,"assets":[],"layers":[]}"#;

    // Valid baseline passes
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[
            ("manifest.json", valid_manifest),
            ("animations/animation.json", minimal_anim),
        ]))
        .is_ok()
    );

    // Path traversal & unsafe entry names rejected
    for bad_entry in [
        "../evil.json",
        "animations/../../evil.json",
        "/abs/path.json",
        "images\\backslash.png",
        "C:drive.json",
    ] {
        assert!(
            validate_dotlottie_bytes(&build_custom_zip(&[
                ("manifest.json", valid_manifest),
                ("animations/animation.json", minimal_anim),
                (bad_entry, b"{}"),
            ]))
            .is_err(),
            "failed to reject unsafe ZIP path '{bad_entry}'"
        );
    }

    // Missing manifest or missing animation file rejected
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[(
            "animations/animation.json",
            minimal_anim
        )]))
        .is_err()
    );
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[("manifest.json", valid_manifest)])).is_err()
    );

    // Dangling refId rejected
    let dangling_anim = br#"{"v":"5.7.14","fr":24,"ip":0,"op":10,"w":100,"h":100,"assets":[],"layers":[{"ty":2,"nm":"bad","refId":"img_missing"}]}"#;
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[
            ("manifest.json", valid_manifest),
            ("animations/animation.json", dangling_anim),
        ]))
        .is_err()
    );

    // Referenced image missing from archive OR having fake magic bytes rejected
    let img_anim = br#"{"v":"5.7.14","fr":24,"ip":0,"op":10,"w":100,"h":100,"assets":[{"id":"img_1","w":8,"h":8,"u":"images/","p":"img_1.png","e":0}],"layers":[{"ty":2,"nm":"ok","refId":"img_1"}]}"#;
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[
            ("manifest.json", valid_manifest),
            ("animations/animation.json", img_anim),
        ]))
        .is_err()
    );
    assert!(
        validate_dotlottie_bytes(&build_custom_zip(&[
            ("manifest.json", valid_manifest),
            ("animations/animation.json", img_anim),
            ("images/img_1.png", b"FAKE_NOT_PNG_BYTES"),
        ]))
        .is_err()
    );
}

// ============================================================================
// 5. Independent Lottie 5.x + dotLottie Spec Evaluator vs `neoflash_core`
//    Across Vectors, Gradients, Classic/Motion/Shape Tweens, Precomps,
//    Multi-Scene Flattening, Parenting, Camera Parallax & Track Mattes
// ============================================================================

#[test]
fn independent_lottie_spec_evaluator_matches_neoflash_core_across_vectors_tweens_symbols_scenes_and_masks()
 {
    let dir = TempTestDir::new("interchange-parity");

    // ------------------------------------------------------------------------
    // Part A: Vector & Paint Fidelity (compound holes `d: 1|3`, quadratic-to-cubic
    // exact elevation, rounded rects, partial ovals with inner radius, variable-width
    // stroke profile, dashed gradient strokes, and radial focal highlight `h`/`a`)
    // ------------------------------------------------------------------------
    let mut vec_proj = Project::blank();
    vec_proj.format_version = 9;
    let root_id = vec_proj.root.clone();
    {
        let root = vec_proj.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 4;

        // 1. Multi-contour compound shape: CW outer square (0,0)->(100,0)->(100,100)->(0,100)
        //    + CCW inner hole (25,25)->(25,75)->(75,75)->(75,25) + Quadratic curve contour!
        let compound_cmds = vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: 100.0, y: 0.0 },
            PathCommand::LineTo { x: 100.0, y: 100.0 },
            PathCommand::LineTo { x: 0.0, y: 100.0 },
            PathCommand::Close,
            PathCommand::MoveTo { x: 25.0, y: 25.0 },
            PathCommand::LineTo { x: 25.0, y: 75.0 },
            PathCommand::LineTo { x: 75.0, y: 75.0 },
            PathCommand::LineTo { x: 75.0, y: 25.0 },
            PathCommand::Close,
        ];
        let radial_grad = GradientPaint {
            kind: GradientKind::Radial,
            matrix: [0.08, 0.0, 0.0, 0.08, 50.0, 50.0],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 220, 40, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [220, 30, 90, 128],
                },
            ],
            focal_point_ratio: -0.6,
            spread: GradientSpread::Extend,
            linear_rgb: false,
        };
        let linear_stroke_grad = GradientPaint {
            kind: GradientKind::Linear,
            matrix: [0.06, 0.0, 0.0, 0.06, 50.0, 50.0],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [20, 200, 255, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [120, 40, 255, 255],
                },
            ],
            focal_point_ratio: 0.0,
            spread: GradientSpread::Extend,
            linear_rgb: false,
        };

        // 2. Exact Quadratic-to-Cubic curve: P0=(0,0), Q=(60,90), P1=(120,0)
        let quad_cmds = vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::QuadraticTo {
                cx: 60.0,
                cy: 90.0,
                x: 120.0,
                y: 0.0,
            },
        ];

        root.timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "compound".into(),
                name: "DonutWithHole".into(),
                transform: Transform::default(),
                kind: ElementKind::PaintedPath {
                    commands: compound_cmds,
                    fill_rule: FillRule::EvenOdd,
                    fill: [255, 0, 0, 255],
                    stroke: [0, 0, 0, 255],
                    fill_gradient: Some(radial_grad),
                    stroke_gradient: Some(linear_stroke_grad.clone()),
                    stroke_width: 4.0,
                },
                ..Element::default()
            },
            Element {
                id: "quad".into(),
                name: "ExactQuad".into(),
                transform: Transform::default(),
                kind: ElementKind::Path {
                    commands: quad_cmds,
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [20, 40, 60, 255],
                    stroke_width: 3.0,
                },
                ..Element::default()
            },
            Element {
                id: "per-corner-rect".into(),
                name: "AsymRect".into(),
                transform: Transform {
                    x: 200.0,
                    y: 40.0,
                    primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                        corner_radii: [4.0, 12.0, 20.0, 0.0],
                        corners_locked: false,
                    })),
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 50.0,
                    fill: [60, 140, 240, 255],
                    stroke: [10, 20, 30, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            },
            Element {
                id: "partial-oval".into(),
                name: "RingSector".into(),
                transform: Transform {
                    x: 320.0,
                    y: 40.0,
                    primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                        start_angle: 30.0,
                        end_angle: 240.0,
                        inner_radius: 0.45,
                        close_path: true,
                    })),
                    ..Transform::default()
                },
                kind: ElementKind::Ellipse {
                    width: 90.0,
                    height: 70.0,
                    fill: [240, 140, 40, 255],
                    stroke: [40, 20, 10, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            },
            Element {
                id: "tapered-stroke".into(),
                name: "VariableWidthStroke".into(),
                transform: Transform {
                    x: 40.0,
                    y: 180.0,
                    primitive: Some(PrimitiveOptions::StrokeProfile(StrokeWidthProfile {
                        preset: StrokeProfilePreset::TaperBoth,
                        stop_count: 0,
                        stops: Default::default(),
                    })),
                    ..Transform::default()
                },
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 140.0, y: 20.0 },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [250, 90, 120, 255],
                    stroke_width: 10.0,
                },
                ..Element::default()
            },
        ];
        // Attach dashed gradient stroke appearance to `quad`
        root.timeline.layers[0].keyframes[0]
            .appearances
            .push(ElementAppearance {
                element_id: "quad".into(),
                entries: vec![AppearancePaintEntry::Stroke {
                    color: [20, 40, 60, 255],
                    gradient: Some(linear_stroke_grad),
                    width: 3.0,
                    swatch_id: None,
                    offset_distance: 0.0,
                    pattern_brush: Some(PatternBrushKind::DashPattern {
                        dash: 12.0,
                        gap: 6.0,
                    }),
                    opacity: 1.0,
                }],
                ..ElementAppearance::default()
            });
    }

    fn find_layer<'a>(layers: &'a [Value], suffix: &str) -> &'a Value {
        layers
            .iter()
            .find(|l| {
                l["nm"]
                    .as_str()
                    .is_some_and(|s| s == suffix || s.ends_with(&format!(" · {suffix}")))
            })
            .unwrap_or_else(|| {
                let names: Vec<_> = layers.iter().filter_map(|l| l["nm"].as_str()).collect();
                panic!("missing layer ending with '{suffix}'; available={names:?}")
            })
    }

    let (vec_json, _) = export_lottie(&vec_proj, &LottieOptions::default()).unwrap();
    let vec_doc = IndependentLottieDoc::from_value(vec_json);
    let vec_layers = vec_doc.root["layers"].as_array().unwrap();

    // Verify compound donut layer: two `sh` items with opposite winding `d: 1` and `d: 3`,
    // `gf` radial fill (`t: 2`, `r: 2` even-odd, `h: 60.0`, `a: 180.0`, color+opacity stops = 2*4 + 2*2 = 12 floats!)
    let donut_layer = find_layer(vec_layers, "DonutWithHole");
    let donut_items = donut_layer["shapes"][0]["it"].as_array().unwrap();
    let sh_dirs: Vec<i64> = donut_items
        .iter()
        .filter(|it| it["ty"] == "sh")
        .map(|it| it["d"].as_i64().unwrap())
        .collect();
    assert_eq!(
        sh_dirs,
        vec![1, 3],
        "outer contour CW (1), inner hole CCW (3)"
    );
    let gf = donut_items
        .iter()
        .find(|it| it["ty"] == "gf")
        .expect("gf item");
    assert_eq!(gf["t"].as_i64(), Some(2));
    assert_eq!(gf["r"].as_i64(), Some(2));
    assert!((gf["h"]["k"].as_f64().unwrap() - 60.0).abs() < 1e-3);
    assert!((gf["a"]["k"].as_f64().unwrap() - 180.0).abs() < 1e-3);
    assert_eq!(gf["g"]["p"].as_i64(), Some(2));
    assert_eq!(
        gf["g"]["k"]["k"].as_array().unwrap().len(),
        12,
        "2 RGB stops (8 floats) + 2 opacity stops (4 floats) = 12"
    );

    // Verify exact quadratic-to-cubic degree elevation AND dashed gradient stroke `gs` + `d` array!
    let quad_layer = find_layer(vec_layers, "ExactQuad");
    let quad_items = quad_layer["shapes"][0]["it"].as_array().unwrap();
    let quad_sh = eval_shape_ks(&quad_items[0]["ks"], 0.0);
    assert_eq!(quad_sh.v.len(), 2);
    // P0=(0,0), Q=(60,90), P1=(120,0) => C1 = P0 + 2/3*(Q-P0) = (40, 60), C2 = P1 + 2/3*(Q-P1) = (80, 60)
    // Relative tangents: o[0] = (40, 60), i[1] = C2 - P1 = (-40, 60).
    let p0 = quad_sh.v[0];
    let p1 = quad_sh.v[1];
    let c1 = [p0[0] + quad_sh.o[0][0], p0[1] + quad_sh.o[0][1]];
    let c2 = [p1[0] + quad_sh.i[1][0], p1[1] + quad_sh.i[1][1]];
    for u in [0.2f64, 0.5, 0.8] {
        let inv = 1.0 - u;
        let qx = inv * inv * 0.0 + 2.0 * inv * u * 60.0 + u * u * 120.0;
        let qy = inv * inv * 0.0 + 2.0 * inv * u * 90.0 + u * u * 0.0;
        let cx = inv * inv * inv * p0[0]
            + 3.0 * inv * inv * u * c1[0]
            + 3.0 * inv * u * u * c2[0]
            + u * u * u * p1[0];
        let cy = inv * inv * inv * p0[1]
            + 3.0 * inv * inv * u * c1[1]
            + 3.0 * inv * u * u * c2[1]
            + u * u * u * p1[1];
        assert!((qx - cx).abs() < 1e-2 && (qy - cy).abs() < 1e-2);
    }
    let gs = quad_items
        .iter()
        .find(|it| it["ty"] == "gs")
        .expect("dashed gradient stroke gs");
    let dashes = gs["d"].as_array().expect("native Lottie dash array d");
    assert_eq!(dashes[0]["n"], "d");
    assert!((dashes[0]["v"]["k"].as_f64().unwrap() - 12.0).abs() < 1e-3);
    assert_eq!(dashes[1]["n"], "g");
    assert!((dashes[1]["v"]["k"].as_f64().unwrap() - 6.0).abs() < 1e-3);

    // Asymmetric per-corner rectangle, partial ring sector oval, and variable-width tapered stroke
    // all export as exact closed bezier contours (`sh` + `fl`)!
    for nm in ["AsymRect", "RingSector", "VariableWidthStroke"] {
        let l = find_layer(vec_layers, nm);
        let items = l["shapes"][0]["it"].as_array().unwrap();
        assert!(
            items.iter().any(|it| it["ty"] == "sh"),
            "{nm} should emit bezier sh"
        );
        assert!(
            items.iter().any(|it| it["ty"] == "fl"),
            "{nm} should emit fill fl"
        );
    }

    // ------------------------------------------------------------------------
    // Part B: Classic Tween Matrix Parity (CW/CCW multi-turn spins, `None` hold,
    // `tween_scale: false`, simultaneous rotation + skew + scale, overshoot cubic
    // bezier, motion guide + `orient_to_path` -> `ao: 1`) & Shape Tween Morph
    // ------------------------------------------------------------------------
    let mut anim_proj = Project::blank();
    anim_proj.format_version = 9;
    let root_id = anim_proj.root.clone();
    {
        let root = anim_proj.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 21;
        root.timeline.layers.clear();

        // Layer 0: Guide layer for guided classic tween
        root.timeline.layers.push(Layer {
            id: "guide-1".into(),
            name: "ArcGuide".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                role: LayerRole::Guide,
                ..LayerProperties::default()
            },
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "gpath".into(),
                    name: "GPath".into(),
                    transform: Transform::default(),
                    kind: ElementKind::Path {
                        commands: vec![
                            PathCommand::MoveTo { x: 20.0, y: 200.0 },
                            PathCommand::CubicTo {
                                cx1: 120.0,
                                cy1: 40.0,
                                cx2: 280.0,
                                cy2: 40.0,
                                x: 380.0,
                                y: 200.0,
                            },
                        ],
                        fill_rule: FillRule::NonZero,
                        fill: [0; 4],
                        stroke: [100, 100, 100, 255],
                        stroke_width: 1.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
        });

        // Layer 1: Classic tween with simultaneous rotation + CCW 1 extra turn + skew + scale
        //          + overshoot CubicBezier!
        let rect_kind = ElementKind::Rectangle {
            width: 40.0,
            height: 20.0,
            fill: [240, 80, 60, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        };
        root.timeline.layers.push(Layer {
            id: "classic-skew-ccw".into(),
            name: "ClassicSkewCcw".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::CubicBezier {
                        x1: 0.25,
                        y1: 1.35,
                        x2: 0.65,
                        y2: -0.2,
                    }),
                    classic_rotate_dir: ClassicRotateDirection::Ccw,
                    classic_rotate_turns: 1,
                    elements: vec![Element {
                        id: "skew-box".into(),
                        name: "SkewBox".into(),
                        transform: Transform {
                            x: 50.0,
                            y: 60.0,
                            scale_x: 1.2,
                            scale_y: 0.8,
                            rotation: 20.0,
                            skew: 18.0,
                            ..Transform::default()
                        },
                        kind: rect_kind.clone(),
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 20,
                    elements: vec![Element {
                        id: "skew-box".into(),
                        name: "SkewBox".into(),
                        transform: Transform {
                            x: 290.0,
                            y: 150.0,
                            scale_x: 1.8,
                            scale_y: 1.3,
                            rotation: 50.0,
                            skew: -14.0,
                            ..Transform::default()
                        },
                        kind: rect_kind.clone(),
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });

        // Layer 2: Guided classic tween with `orient_to_path: true` and `tween_scale: false`
        root.timeline.layers.push(Layer {
            id: "guided-classic".into(),
            name: "GuidedClassic".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                motion_guide: Some("guide-1".into()),
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::EaseInOut),
                    orient_to_path: true,
                    snap_to_guide: true,
                    tween_scale: false,
                    elements: vec![Element {
                        id: "rocket".into(),
                        name: "Rocket".into(),
                        transform: Transform {
                            x: 0.0,
                            y: 180.0,
                            scale_x: 1.0,
                            scale_y: 1.0,
                            ..Transform::default()
                        },
                        kind: rect_kind.clone(),
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 20,
                    elements: vec![Element {
                        id: "rocket".into(),
                        name: "Rocket".into(),
                        transform: Transform {
                            x: 360.0,
                            y: 180.0,
                            scale_x: 2.5,
                            scale_y: 2.5,
                            ..Transform::default()
                        },
                        kind: rect_kind.clone(),
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });

        // Layer 3: Motion tween with `◆` sparse `keyed_properties` + `motion_path` + `orient_to_path`
        root.timeline.layers.push(Layer {
            id: "motion-span".into(),
            name: "MotionSpan".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Motion,
                    orient_to_path: true,
                    motion_path: Some(vec![
                        PathCommand::MoveTo { x: 40.0, y: 300.0 },
                        PathCommand::CubicTo {
                            cx1: 140.0,
                            cy1: 220.0,
                            cx2: 260.0,
                            cy2: 380.0,
                            x: 360.0,
                            y: 300.0,
                        },
                    ]),
                    elements: vec![Element {
                        id: "m-orb".into(),
                        name: "MotionOrb".into(),
                        transform: Transform {
                            x: 40.0,
                            y: 300.0,
                            alpha: 1.0,
                            ..Transform::default()
                        },
                        kind: rect_kind,
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 10,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Motion,
                    keyed_properties: Some(vec![TweenProperty::Alpha]),
                    elements: vec![Element {
                        id: "m-orb".into(),
                        name: "MotionOrb".into(),
                        transform: Transform {
                            x: 200.0,
                            y: 300.0,
                            alpha: 0.25,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 40.0,
                            height: 20.0,
                            fill: [240, 80, 60, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 20,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Motion,
                    keyed_properties: Some(vec![
                        TweenProperty::X,
                        TweenProperty::Y,
                        TweenProperty::Alpha,
                    ]),
                    elements: vec![Element {
                        id: "m-orb".into(),
                        name: "MotionOrb".into(),
                        transform: Transform {
                            x: 360.0,
                            y: 300.0,
                            alpha: 0.9,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 40.0,
                            height: 20.0,
                            fill: [240, 80, 60, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });

        // Layer 4: Shape tween morphing a triangle into a pentagon!
        root.timeline.layers.push(Layer {
            id: "shape-morph".into(),
            name: "ShapeMorph".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::EaseInOut),
                    tween_kind: TweenKind::Shape,
                    elements: vec![Element {
                        id: "poly".into(),
                        name: "Poly".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 40.0, y: 10.0 },
                                PathCommand::LineTo { x: 80.0, y: 70.0 },
                                PathCommand::LineTo { x: 10.0, y: 70.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [50, 200, 120, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 20,
                    elements: vec![Element {
                        id: "poly".into(),
                        name: "Poly".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 200.0, y: 10.0 },
                                PathCommand::LineTo { x: 240.0, y: 35.0 },
                                PathCommand::LineTo { x: 225.0, y: 80.0 },
                                PathCommand::LineTo { x: 175.0, y: 80.0 },
                                PathCommand::LineTo { x: 160.0, y: 35.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [200, 60, 220, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });
    }

    let dot_path = dir.path().join("tweens.lottie");
    export_dotlottie(&anim_proj, &dot_path, &LottieOptions::default()).unwrap();
    let anim_doc = IndependentLottieDoc::from_dotlottie_bytes(&fs::read(&dot_path).unwrap());
    let anim_layers = anim_doc.root["layers"].as_array().unwrap();
    let core_tl = &anim_proj.symbol(&root_id).unwrap().timeline;

    // Guide layer itself is NEVER emitted as visible artwork.
    assert!(!anim_layers.iter().any(|l| {
        l["nm"]
            .as_str()
            .is_some_and(|s| s.contains("GPath") || s.contains("ArcGuide"))
    }));

    // Verify ClassicSkewCcw 2D affine world-point parity against `neoflash_core::sample_layer_in`
    // across sampled frames `0, 4, 10, 15, 20` (proving exact skew + CCW multi-turn + overshoot!).
    let skew_layer = find_layer(anim_layers, "SkewBox");
    let skew_ind = skew_layer["ind"].as_i64().unwrap();
    // Confirm CCW 1 extra turn unwrapped from +20° down to 20 - 330 - 360 = -670°!
    let r_end = eval_scalar_prop(&skew_layer["ks"]["r"], 20.0);
    assert!(
        (r_end - (-670.0)).abs() < 1e-2,
        "CCW 1-turn from 20deg to 50deg must unwrap to -670deg, got {r_end}"
    );
    for f in [0u32, 4, 10, 15, 20] {
        let core_el = &sample_layer_in(core_tl, &core_tl.layers[1], f)[0];
        let core_mat = transform_matrix(core_el.transform);
        let lottie_mat = anim_doc.eval_layer_world_matrix(anim_layers, skew_ind, f as f64);
        for test_pt in [[0.0, 0.0], [20.0, 10.0], [40.0, 20.0]] {
            let p_core = transform_point(core_mat, test_pt);
            let p_lot = transform_point(lottie_mat, test_pt);
            let dist = (p_core[0] - p_lot[0]).hypot(p_core[1] - p_lot[1]);
            let tol = if f == 0 || f == 20 { 1e-2 } else { 0.8 };
            assert!(
                dist < tol,
                "frame {f} pt {test_pt:?}: core={p_core:?} vs lottie={p_lot:?} (dist={dist}, tol={tol})"
            );
        }
    }

    // Verify GuidedClassic: `ao == 1` or baked guide tangent rotation, AND `tween_scale: false`
    // keeps scale at 100% until frame 20!
    let rocket_layer = find_layer(anim_layers, "Rocket");
    let rocket_ind = rocket_layer["ind"].as_i64().unwrap();
    for f in [0u32, 5, 10, 15] {
        let core_el = &sample_layer_in(core_tl, &core_tl.layers[2], f)[0];
        assert!((core_el.transform.scale_x - 1.0).abs() < 1e-6);
        let lottie_mat = anim_doc.eval_layer_world_matrix(anim_layers, rocket_ind, f as f64);
        let core_mat = transform_matrix(core_el.transform);
        let c_center = transform_point(core_mat, [20.0, 10.0]);
        let l_center = transform_point(lottie_mat, [20.0, 10.0]);
        assert!(
            (c_center[0] - l_center[0]).hypot(c_center[1] - l_center[1]) < 1.5,
            "guided rocket center mismatch at frame {f}: core={c_center:?}, lottie={l_center:?}"
        );
    }

    // Verify MotionSpan: `ao: 1`, `◆` alpha dip to 25% at frame 10 and 90% at frame 20!
    let orb_layer = find_layer(anim_layers, "MotionOrb");
    assert_eq!(orb_layer["ao"].as_i64(), Some(1));
    assert!((eval_scalar_prop(&orb_layer["ks"]["o"], 10.0) - 25.0).abs() < 1.0);
    assert!((eval_scalar_prop(&orb_layer["ks"]["o"], 20.0) - 90.0).abs() < 1.0);

    // Verify ShapeMorph: single animated `ks` bezier track whose centroid moves smoothly from ~43 to ~200!
    let poly_layer = find_layer(anim_layers, "Poly");
    let poly_items = poly_layer["shapes"][0]["it"].as_array().unwrap();
    assert_eq!(poly_items[0]["ks"]["a"].as_i64(), Some(1));
    let sh_f0 = eval_shape_ks(&poly_items[0]["ks"], 0.0);
    let sh_f10 = eval_shape_ks(&poly_items[0]["ks"], 10.0);
    let sh_f20 = eval_shape_ks(&poly_items[0]["ks"], 20.0);
    assert_eq!(sh_f0.v.len(), sh_f20.v.len());
    let avg_x = |s: &LottieBezierShape| s.v.iter().map(|p| p[0]).sum::<f64>() / s.v.len() as f64;
    assert!(
        avg_x(&sh_f0) < 60.0
            && avg_x(&sh_f10) > 90.0
            && avg_x(&sh_f10) < 160.0
            && avg_x(&sh_f20) > 180.0
    );

    // ------------------------------------------------------------------------
    // Part C: Nested Precomps (`LoopFrom`, `PlayOnceFrom`, `SingleFrame`, 1-frame
    // Graphic hold, `Button` Up-state isolation), Multi-Scene Flattening,
    // Intra-Layer Parenting + Controller Filtering, Camera Parallax & Track Mattes
    // ------------------------------------------------------------------------
    let mut comp_proj = Project::blank();
    comp_proj.format_version = 9;
    comp_proj.fps = 24;

    // 6-frame Graphic symbol
    let graphic_sym = Symbol {
        id: "sym-gfx6".into(),
        name: "ClockGraphic".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "gl".into(),
                name: "Hand".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "hand".into(),
                        name: "Hand".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 10.0,
                            height: 30.0,
                            fill: [255, 255, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };
    // 1-frame static Graphic symbol (must NOT vanish after frame 0 on a 10-frame parent!)
    let mut one_frame_layers = graphic_sym.timeline.layers.clone();
    one_frame_layers[0].id = "one-l".into();
    one_frame_layers[0].keyframes[0].elements[0].id = "one-hand".into();
    let one_frame_sym = Symbol {
        id: "sym-one".into(),
        name: "StaticLogo".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            scripts: vec![],
            guides: vec![],
            layers: one_frame_layers,
        },
    };
    // 4-frame Button symbol (Up=0, Over=1, Down=2, Hit=3 -> must export Up state ONLY + diagnostic!)
    let mut button_layers = graphic_sym.timeline.layers.clone();
    button_layers[0].id = "btn-l".into();
    button_layers[0].keyframes[0].elements[0].id = "btn-up".into();
    button_layers[0].keyframes.push(Keyframe {
        frame: 1,
        elements: vec![Element {
            id: "over-only".into(),
            name: "OverState".into(),
            transform: Transform::default(),
            kind: ElementKind::Ellipse {
                width: 40.0,
                height: 40.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }],
        ..Keyframe::default()
    });
    let button_sym = Symbol {
        id: "sym-btn".into(),
        name: "PlayButton".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: vec![],
            guides: vec![],
            layers: button_layers,
        },
    };
    // Scene 2 MovieClip symbol (`4` frames) so `scenes = [root (10 frames), scene2 (4 frames)]` -> 14 total!
    let scene2_sym = Symbol {
        id: "scene-2".into(),
        name: "Scene 2".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "s2-l".into(),
                name: "OutroText".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "txt-outro".into(),
                        name: "Outro".into(),
                        transform: Transform {
                            x: 100.0,
                            y: 120.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Text {
                            text: "FIN".into(),
                            font_family: "Helvetica".into(),
                            font_size: 32.0,
                            color: [255, 240, 200, 255],
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };

    comp_proj
        .symbols
        .extend([graphic_sym, one_frame_sym, button_sym, scene2_sym]);
    let root_id = comp_proj.root.clone();
    comp_proj.scenes = vec![root_id.clone(), "scene-2".into()];

    {
        let root = comp_proj.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 10;
        root.timeline.layers = vec![
            // Alpha Inverted Mask layer (`MaskMode::Alpha`, `mask_invert: true`)
            Layer {
                id: "alpha-mask".into(),
                name: "SpotlightMask".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    mask_mode: MaskMode::Alpha,
                    mask_invert: true,
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "mask-circle".into(),
                        name: "MaskCircle".into(),
                        transform: Transform {
                            x: 80.0,
                            y: 80.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Ellipse {
                            width: 120.0,
                            height: 120.0,
                            fill: [255, 255, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Masked content layer with instances + intra-layer parenting + controller null + camera!
            Layer {
                id: "masked-content".into(),
                name: "MaskedStage".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    mask: Some("alpha-mask".into()),
                    blend_mode: BlendMode::Multiply,
                    z_depth: 250.0,
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    camera: Some(CameraPose {
                        x: 40.0,
                        y: -20.0,
                        zoom: 1.25,
                        rotation: 0.0,
                        focal_length: 1000.0,
                        color_transform: ColorTransform::default(),
                    }),
                    controller_elements: vec!["rig-null".into()],
                    element_parents: vec![ElementParentLink {
                        child_id: "inst-loop-from".into(),
                        parent_id: "rig-null".into(),
                    }],
                    elements: vec![
                        Element {
                            id: "rig-null".into(),
                            name: "RigController".into(),
                            transform: Transform {
                                x: 100.0,
                                y: 50.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 10.0,
                                height: 10.0,
                                fill: [255, 0, 255, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "inst-loop-from".into(),
                            name: "LoopFrom2".into(),
                            transform: Transform {
                                x: 30.0,
                                y: 10.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-gfx6".into(),
                                playback: InstancePlayback::LoopFrom(2),
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "inst-play-once".into(),
                            name: "PlayOnce1".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: "sym-gfx6".into(),
                                playback: InstancePlayback::PlayOnceFrom(1),
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "inst-single".into(),
                            name: "SingleFrame4".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: "sym-gfx6".into(),
                                playback: InstancePlayback::SingleFrame(4),
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "inst-one".into(),
                            name: "StaticLogoInst".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: "sym-one".into(),
                                playback: InstancePlayback::Loop,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "inst-btn".into(),
                            name: "PlayBtnInst".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: "sym-btn".into(),
                                playback: InstancePlayback::Independent,
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
            },
        ];
    }

    let (comp_json, comp_report) = export_lottie(&comp_proj, &LottieOptions::default()).unwrap();
    let comp_doc = IndependentLottieDoc::from_value(comp_json);
    let (flattened, scene_ranges) = flatten_scenes_with_ranges(&comp_proj);
    let flattened_fc = flattened
        .symbol(&flattened.root)
        .unwrap()
        .timeline
        .frame_count;
    assert_eq!(comp_report.frame_count, flattened_fc);
    assert_eq!(comp_doc.root["op"].as_u64(), Some(u64::from(flattened_fc)));
    assert_eq!(scene_ranges.len(), 2);
    assert!(
        comp_doc.root["markers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["cm"] == "Scene 2" && m["tm"].as_f64() == Some(10.0)),
        "expected Scene 2 marker at frame 10"
    );

    let comp_layers = comp_doc.root["layers"].as_array().unwrap();
    // Controller element `RigController` must NOT be emitted as visible geometry, OR if emitted as
    // a null (`ty: 3`) parent carrier, it has zero visible shapes while parenting `LoopFrom2`!
    assert!(
        !comp_layers.iter().any(|l| {
            l["nm"]
                .as_str()
                .is_some_and(|s| s.contains("RigController"))
                && l["ty"].as_i64() != Some(3)
        }),
        "controller element must never render as visible shape!"
    );

    // Check `InstancePlayback` via `tm` time remapping:
    let fps = 24.0;
    let l_loop = find_layer(comp_layers, "LoopFrom2");
    // LoopFrom(2) on 6-frame symbol: frame 0 -> child 2, frame 3 -> child 5, frame 4 -> wraps to child 0!
    assert!((comp_doc.eval_precomp_child_frame(l_loop, 0.0, fps) - 2.0).abs() < 1e-2);
    assert!((comp_doc.eval_precomp_child_frame(l_loop, 3.0, fps) - 5.0).abs() < 1e-2);
    assert!((comp_doc.eval_precomp_child_frame(l_loop, 4.0, fps) - 0.0).abs() < 1e-2);

    let l_once = find_layer(comp_layers, "PlayOnce1");
    // PlayOnceFrom(1) on 6-frame symbol: frame 0 -> 1, frame 4 -> 5, frame 8 -> clamps at 5!
    assert!((comp_doc.eval_precomp_child_frame(l_once, 0.0, fps) - 1.0).abs() < 1e-2);
    assert!((comp_doc.eval_precomp_child_frame(l_once, 4.0, fps) - 5.0).abs() < 1e-2);
    assert!((comp_doc.eval_precomp_child_frame(l_once, 8.0, fps) - 5.0).abs() < 1e-2);

    let l_single = find_layer(comp_layers, "SingleFrame4");
    assert!((comp_doc.eval_precomp_child_frame(l_single, 0.0, fps) - 4.0).abs() < 1e-2);
    assert!((comp_doc.eval_precomp_child_frame(l_single, 9.0, fps) - 4.0).abs() < 1e-2);

    // 1-frame Graphic & Button both emit `tm = 0` so they stay visible across all 10 frames!
    let l_one = find_layer(comp_layers, "StaticLogoInst");
    assert_eq!(l_one["tm"]["k"].as_f64(), Some(0.0));
    let l_btn = find_layer(comp_layers, "PlayBtnInst");
    assert_eq!(l_btn["tm"]["k"].as_f64(), Some(0.0));
    let btn_precomp = &comp_doc.assets_by_id[l_btn["refId"].as_str().unwrap()];
    let btn_comp_layers = btn_precomp["layers"].as_array().unwrap();
    assert_eq!(
        btn_comp_layers.len(),
        1,
        "Button precomp exports Up frame 0 only"
    );
    assert_eq!(btn_comp_layers[0]["op"].as_i64(), Some(1));
    assert!(
        !btn_comp_layers
            .iter()
            .any(|l| l["nm"].as_str().is_some_and(|s| s.contains("OverState")))
    );
    assert!(
        comp_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_BUTTON_INTERACTIVE"))
    );

    // Verify Alpha Inverted Track Matte (`tt: 2` paired with immediately preceding `td: 1` matte carrier)
    // AND Multiply blend mode (`bm: 1`)!
    assert_eq!(l_loop["bm"].as_i64(), Some(1));
    let loop_idx = comp_layers
        .iter()
        .position(|l| {
            l["nm"]
                .as_str()
                .is_some_and(|s| s.ends_with(" · LoopFrom2") || s == "LoopFrom2")
        })
        .unwrap();
    assert_eq!(comp_layers[loop_idx]["tt"].as_i64(), Some(2));
    assert_eq!(comp_layers[loop_idx - 1]["td"].as_i64(), Some(1));

    // Verify Scene 2 static `Text` layer (`ty: 5`, `ip == 10`, `op == 14`, `fonts.list` populated,
    // and `LOTTIE_TEXT_FONT_DEPENDENCY` diagnostic emitted)!
    let text_layer = find_layer(comp_layers, "Outro");
    assert_eq!(text_layer["ty"].as_i64(), Some(5));
    assert_eq!(text_layer["ip"].as_i64(), Some(10));
    assert_eq!(text_layer["op"].as_i64(), Some(14));
    assert_eq!(text_layer["t"]["d"]["k"][0]["s"]["t"], "FIN");
    assert!(
        comp_doc.root["fonts"]["list"]
            .as_array()
            .is_some_and(|list| !list.is_empty()),
        "root fonts.list must declare fonts used by ty:5 text layers"
    );
    assert!(
        comp_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_TEXT_FONT_DEPENDENCY"))
    );
}

// ============================================================================
// 6. Actionable Structured Diagnostics (Zero Silent Drops)
// ============================================================================

#[test]
fn structured_diagnostics_cover_every_unsupported_or_approximated_construct_without_silent_drops() {
    let png = make_rgba_png(4, 4, |_, _| [255, 128, 0, 255]);
    let mut project = Project::blank();
    project.format_version = 11;
    project.document_class = Some("MainApp".into());
    project.sources.push(SourceUnit {
        id: "src-1".into(),
        path: "MainApp.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package { public class MainApp {} }".into(),
    });
    project.resources.push(
        ResourceUnit::from_bytes("sfx", "audio/hit.wav", "audio/wav", &[1, 2, 3, 4]).unwrap(),
    );
    project
        .resources
        .push(ResourceUnit::from_bytes("tex", "img/tex.png", "image/png", &png).unwrap());

    let child_sym = Symbol {
        id: "sym-card".into(),
        name: "Card".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "card-l".into(),
                name: "CardBg".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    scale9_grid: Some([4.0, 4.0, 32.0, 32.0]),
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "c-rect".into(),
                        name: "CRect".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 40.0,
                            height: 40.0,
                            fill: [100, 120, 140, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };
    project.symbols.push(child_sym);

    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 8;
        root.timeline.scripts.push(FrameScript {
            layer: None,
            frame: 2,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "gotoAndPlay(1);".into(),
            },
        });

        let luma_mask_layer = Layer {
            id: "luma-mask".into(),
            name: "LumaMask".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                role: LayerRole::Mask,
                mask_mode: MaskMode::Luminance,
                ..LayerProperties::default()
            },
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "lm-rect".into(),
                    name: "LumaRect".into(),
                    transform: Transform::default(),
                    kind: ElementKind::Rectangle {
                        width: 200.0,
                        height: 200.0,
                        fill: [255, 255, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
        };

        root.timeline.layers[0].name = "DiagLayer".into();
        root.timeline.layers[0].properties.mask = Some("luma-mask".into());
        root.timeline.layers.insert(0, luma_mask_layer);
        root.timeline.layers[1].keyframes[0] = Keyframe {
            frame: 0,
            sound: Some(FrameSound::new("sfx")),
            perspective_grid: Some(PerspectiveGridSpec {
                enabled: true,
                ..PerspectiveGridSpec::default()
            }),
            bitmap_fills: vec![AuthoredBitmapFill {
                element_id: "picture".into(),
                resource_id: "tex".into(),
                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                repeat: true,
                smoothing: true,
            }],
            instance_overrides: vec![InstancePropertyOverride {
                instance_id: "card-inst".into(),
                target_name: "CRect".into(),
                visible: Some(false),
                ..InstancePropertyOverride::default()
            }],
            elements: vec![
                Element {
                    id: "picture".into(),
                    name: "Tiled picture".into(),
                    kind: ElementKind::Rectangle {
                        width: 20.0,
                        height: 20.0,
                        fill: [255; 4],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
                Element {
                    id: "vid-1".into(),
                    name: "IntroVideo".into(),
                    transform: Transform::default(),
                    kind: ElementKind::Video {
                        width: 320.0,
                        height: 180.0,
                    },
                    ..Element::default()
                },
                Element {
                    id: "dyn-txt".into(),
                    name: "ScoreField".into(),
                    transform: Transform::default(),
                    kind: ElementKind::TextBox {
                        text: "0000".into(),
                        width: 120.0,
                        height: 30.0,
                        format: TextFormat::default(),
                        word_wrap: false,
                        multiline: false,
                    },
                    ..Element::default()
                },
                Element {
                    id: "card-inst".into(),
                    name: "CardInstance".into(),
                    transform: Transform {
                        color_transform: ColorTransform {
                            multipliers: [1.0, 0.8, 0.8, 1.0],
                            offsets: [60.0, 0.0, 0.0, 0.0],
                        },
                        ..Transform::default()
                    },
                    blend_mode: BlendMode::Erase,
                    filters: vec![FilterSpec::Blur {
                        blur_x: 8.0,
                        blur_y: 8.0,
                        quality: 2,
                    }],
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::Click,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "play();".into(),
                        },
                    }],
                    kind: ElementKind::Instance {
                        symbol_id: "sym-card".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                },
                Element {
                    id: "grad-spread-shape".into(),
                    name: "SpreadGrad".into(),
                    transform: Transform::default(),
                    kind: ElementKind::PaintedPath {
                        commands: vec![
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::LineTo { x: 50.0, y: 0.0 },
                            PathCommand::LineTo { x: 50.0, y: 50.0 },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::NonZero,
                        fill: [255, 255, 255, 255],
                        stroke: [0; 4],
                        fill_gradient: Some(GradientPaint {
                            kind: GradientKind::Linear,
                            matrix: [0.05, 0.0, 0.0, 0.05, 25.0, 25.0],
                            stops: vec![
                                GradientStop {
                                    ratio: 0,
                                    color: [255, 0, 0, 255],
                                },
                                GradientStop {
                                    ratio: 255,
                                    color: [0, 0, 255, 255],
                                },
                            ],
                            focal_point_ratio: 0.0,
                            spread: GradientSpread::Reflect,
                            linear_rgb: true,
                        }),
                        stroke_gradient: None,
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
            ],
            ..Keyframe::default()
        };
    }

    let (_, report) = export_lottie(
        &project,
        &LottieOptions {
            movie_clip_mode: LottieMovieClipMode::FirstFrame,
            player: LottieTargetPlayer::Skottie,
            ..LottieOptions::default()
        },
    )
    .unwrap();

    let codes: BTreeSet<&str> = report
        .notes
        .iter()
        .filter_map(|n| n.code.as_deref())
        .collect();

    for required_code in [
        "LOTTIE_PROJECT_SCRIPTS",
        "LOTTIE_FRAME_SCRIPT",
        "LOTTIE_OBJECT_SCRIPT",
        "LOTTIE_TIMELINE_SOUND",
        "LOTTIE_VIDEO_ELEMENT",
        "LOTTIE_DYNAMIC_INPUT_TEXT",
        "LOTTIE_DISPLAY_FILTER",
        "LOTTIE_SCALE9_GRID",
        "LOTTIE_BLEND_MODE_UNSUPPORTED",
        "LOTTIE_PERSPECTIVE_3D",
        "LOTTIE_BITMAP_FILL",
        "LOTTIE_INSTANCE_OVERRIDE",
        "LOTTIE_COLOR_TRANSFORM_APPROX",
        "LOTTIE_MOVIECLIP_FIRST_FRAME",
        "LOTTIE_GRADIENT_SPREAD",
        "LOTTIE_GRADIENT_LINEAR_RGB",
        "LOTTIE_LUMA_MATTE_PLAYER",
    ] {
        assert!(
            codes.contains(required_code),
            "missing structured diagnostic code '{required_code}'; got codes: {codes:?}"
        );
    }

    // Every note must have non-empty human-readable location & message, and warnings mirror notes.
    for note in &report.notes {
        assert!(!note.location.trim().is_empty());
        assert!(!note.message.trim().is_empty());
        assert!(
            note.code
                .as_deref()
                .is_some_and(|c| c.starts_with("LOTTIE_"))
        );
    }
    assert!(report.not_exported_count > 0);
    assert!(report.approximated_count > 0);
    assert_eq!(
        report.approximated_count,
        report
            .notes
            .iter()
            .filter(|n| n.kind == LottieNoteKind::Approximated)
            .count()
    );
}
