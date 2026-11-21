//! Compile the editor's evaluated placement clocks to native SVG keyframes.
//! The browser owns interpolation, repeat/stop timing and visibility. The
//! compiler checks fitted easing curves against every authored frame before
//! dropping samples, including compound motion in nested symbols.
use super::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];

#[derive(Clone, Copy)]
struct Clock {
    frames: u32,
    fps: u32,
    repeats: Option<u32>,
}

impl Clock {
    fn timing(self) -> String {
        let seconds = f64::from(self.frames) / f64::from(self.fps);
        let repeats = self
            .repeats
            .map_or_else(|| "indefinite".into(), |n| n.to_string());
        format!("dur=\"{seconds:.8}s\" repeatCount=\"{repeats}\" fill=\"freeze\"")
    }
    fn offset(self, frame: u32) -> String {
        (f64::from(frame) / f64::from(self.frames)).to_string()
    }
}

/// Scalar easing represented by the two Y coordinates of a cubic Bézier.
/// X controls 1/3 and 2/3 give an exactly linear clock.
type Spline = [f64; 2];
const LINEAR: Spline = [1. / 3., 2. / 3.];

fn fitted_span(rows: &[Vec<f64>], frames: &[u32], tolerance: f64) -> Option<Spline> {
    let first = &rows[0];
    let n = rows.len() - 1;
    if n <= 1 {
        return Some(LINEAR);
    }
    let time =
        |index: usize| f64::from(frames[index] - frames[0]) / f64::from(frames[n] - frames[0]);
    let last = &rows[n];
    let axis = (0..first.len()).max_by(|&i, &j| {
        (last[i] - first[i])
            .abs()
            .total_cmp(&(last[j] - first[j]).abs())
    })?;
    let delta = last[axis] - first[axis];
    let mut curve = LINEAR;
    if delta.abs() > tolerance {
        let observed = |index: usize| (rows[index][axis] - first[axis]) / delta;
        if n == 2 {
            let t = time(1);
            let bend = (observed(1) - t) / (t * (1. - t));
            curve = [(1. + bend) / 3., (2. + bend) / 3.];
        } else {
            let constraints = |index: usize| {
                let t = time(index);
                (
                    3. * (1. - t).powi(2) * t,
                    3. * (1. - t) * t * t,
                    observed(index) - t * t * t,
                )
            };
            let (a, b, c) = constraints(n / 3);
            let (d, e, f) = constraints(n - n / 3);
            let det = a * e - b * d;
            curve = [(c * e - b * f) / det, (a * f - c * d) / det];
        }
        // SMIL requires both spline control points inside the unit square.
        curve = curve.map(|y| y.clamp(0., 1.));
    }
    for (index, row) in rows.iter().enumerate().take(n).skip(1) {
        let t = time(index);
        let u = 3. * (1. - t).powi(2) * t * curve[0] + 3. * (1. - t) * t * t * curve[1] + t.powi(3);
        if row
            .iter()
            .enumerate()
            .any(|(i, &v)| (v - (first[i] + (last[i] - first[i]) * u)).abs() > tolerance)
        {
            return None;
        }
    }
    Some(curve)
}

fn fit(
    rows: &[Vec<f64>],
    frames: &[u32],
    lo: usize,
    hi: usize,
    tolerance: f64,
    keys: &mut Vec<(usize, Spline)>,
) {
    if let Some(spline) = fitted_span(&rows[lo..=hi], &frames[lo..=hi], tolerance) {
        keys.push((hi, spline));
    } else {
        let mid = lo + (hi - lo) / 2;
        fit(rows, frames, lo, mid, tolerance, keys);
        fit(rows, frames, mid, hi, tolerance, keys);
    }
}

fn animation(
    rows: &[Vec<f64>],
    frames: &[u32],
    clock: Clock,
    target: &str,
    tolerance: f64,
    format: impl Fn(&[f64]) -> String,
) -> Option<String> {
    if rows.len() <= 1 || rows.iter().all(|row| row == &rows[0]) {
        return None;
    }
    let mut keys = vec![(0, LINEAR)];
    // Split off stationary sections so a long hold never introduces many keys at its boundary.
    let mut lo = 0;
    let mut i = 0;
    while i + 1 < rows.len() {
        let mut j = i;
        while j + 1 < rows.len() && rows[j] == rows[j + 1] {
            j += 1;
        }
        if j > i {
            if i > lo {
                fit(rows, frames, lo, i, tolerance, &mut keys);
            }
            keys.push((j, LINEAR));
            lo = j;
        }
        i = j + 1;
    }
    if lo + 1 < rows.len() {
        fit(rows, frames, lo, rows.len() - 1, tolerance, &mut keys);
    }

    let mut values = Vec::new();
    let mut times = Vec::new();
    let mut splines = Vec::new();
    let mut add = |frame: u32, row: usize, spline: Spline| {
        if !values.is_empty() {
            splines.push(format!(
                ".333333 {} .666667 {}",
                fmt_num(spline[0], 4),
                fmt_num(spline[1], 4)
            ));
        }
        values.push(format(&rows[row]));
        times.push(clock.offset(frame));
    };
    if frames[0] > 0 {
        add(0, 0, LINEAR);
    }
    for (idx, spline) in &keys {
        add(frames[*idx], *idx, *spline);
    }
    add(clock.frames, rows.len() - 1, LINEAR);
    Some(format!(
        "<{target} {} calcMode=\"spline\" keyTimes=\"{}\" keySplines=\"{}\" values=\"{}\"/>",
        clock.timing(),
        times.join(";"),
        splines.join(";"),
        values.join(";")
    ))
}

/// SVG expresses affine transforms as translation, rotation, signed scale and X shear.
/// Use scale before shear so a rank-one transform still decomposes exactly.
fn decompose([a, b, c, d, x, y]: [f64; 6]) -> [f64; 6] {
    let sx = a.hypot(b);
    if sx == 0. {
        return [x, y, (-c).atan2(d).to_degrees(), 0., c.hypot(d), 0.];
    }
    [
        x,
        y,
        b.atan2(a).to_degrees(),
        sx,
        (a * d - b * c) / sx,
        ((a * c + b * d) / (sx * sx)).atan().to_degrees(),
    ]
}

fn path_parts(kind: &ElementKind) -> Option<(String, Vec<f64>)> {
    let (ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. }) = kind
    else {
        return None;
    };
    let mut pattern = String::with_capacity(commands.len());
    let mut points = Vec::new();
    for cmd in commands {
        let (letter, coordinates): (char, &[f64]) = match *cmd {
            PathCommand::MoveTo { x, y } => ('M', &[x, y]),
            PathCommand::LineTo { x, y } => ('L', &[x, y]),
            PathCommand::QuadraticTo { cx, cy, x, y } => ('Q', &[cx, cy, x, y]),
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => ('C', &[cx1, cy1, cx2, cy2, x, y]),
            PathCommand::Close => ('Z', &[]),
        };
        pattern.push(letter);
        points.extend_from_slice(coordinates);
    }
    (!points.is_empty()).then_some((pattern, points))
}

fn path_value(pattern: &str, values: &[f64], precision: usize) -> String {
    let mut out = String::new();
    let mut coordinates = values.iter();
    for letter in pattern.chars() {
        out.push(letter);
        let count = match letter {
            'M' | 'L' => 2,
            'Q' => 4,
            'C' => 6,
            _ => 0,
        };
        for (i, value) in coordinates.by_ref().take(count).enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&fmt_num(*value, precision));
        }
    }
    out
}

struct Run {
    frames: Vec<u32>,
    artwork: AuthoredDraw,
    first_kind: Option<ElementKind>,
    poses: Vec<[f64; 7]>,
    path_pattern: String,
    paths: Vec<Vec<f64>>,
}

fn append(
    slot: &mut Vec<Run>,
    mut draw: AuthoredDraw,
    frame: u32,
    morph: bool,
    cut: bool,
) -> usize {
    let [x, y, mut rotation, sx, sy, shear] = decompose(draw.item.transform);
    // The evaluator already composed ancestor/placement alpha into the colour transform.
    let ct = &mut draw.item.color_transform;
    let mut alpha = if ct.offsets[3] == 0. && (0.0..=1.0).contains(&ct.multipliers[3]) {
        std::mem::replace(&mut ct.multipliers[3], 1.)
    } else {
        1.
    };
    // SVG text carries a solid RGB fill; retain the authored text's alpha
    // in the placement opacity track (including any additive colour offset).
    if let ElementKind::Text { color, .. } = &mut draw.item.kind {
        alpha *= f64::from(apply_ct(*color, *ct)[3]) / 255.;
        color[3] = 255;
        ct.multipliers[3] = 1.;
        ct.offsets[3] = 0.;
    }
    draw.item.alpha = 1.;
    draw.item.transform = IDENTITY;
    let shape = morph.then(|| path_parts(&draw.item.kind)).flatten();
    let (pattern, coordinates) = shape.unwrap_or_default();
    let first_kind = if pattern.is_empty() {
        None
    } else {
        let kind = draw.item.kind.clone();
        match &mut draw.item.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                commands.clear()
            }
            _ => {}
        }
        Some(kind)
    };
    if let Some(prev) = slot.last().and_then(|run| run.poses.last()) {
        rotation = prev[2] + ((rotation - prev[2] + 180.).rem_euclid(360.) - 180.);
    }
    let pose = [x, y, rotation, sx, sy, shear, alpha];
    let continues = slot.last().is_some_and(|r| {
        r.frames.last().is_some_and(|last| last + 1 == frame)
            && r.artwork == draw
            && r.path_pattern == pattern
            && (!cut
                || (r.poses.last() == Some(&pose)
                    && (coordinates.is_empty() || r.paths.last() == Some(&coordinates))))
    });
    if !continues {
        slot.push(Run {
            frames: Vec::new(),
            artwork: draw,
            first_kind,
            poses: Vec::new(),
            path_pattern: pattern,
            paths: Vec::new(),
        });
    }
    let run = slot.last_mut().expect("a visible draw always has a run");
    // Keep the endpoints of a hold, regardless of its duration. A later fit
    // uses the original frame numbers, including gaps between retained samples.
    let n = run.poses.len();
    if n >= 2
        && run.poses[n - 2..] == [pose, pose]
        && (coordinates.is_empty()
            || (run.paths[n - 1] == coordinates && run.paths[n - 2] == coordinates))
    {
        run.frames[n - 1] = frame;
        return 0;
    }
    run.poses.push(pose);
    run.frames.push(frame);
    let bytes = size_of::<[f64; 7]>() + size_of::<u32>() + coordinates.len() * size_of::<f64>();
    if !coordinates.is_empty() {
        run.paths.push(coordinates);
    }
    bytes
}

fn visibility(start: u32, end: u32, clock: Clock) -> String {
    if start == 0 && end == clock.frames {
        return String::new();
    }
    let mut times = vec!["0".to_owned()];
    let mut values = vec![if start == 0 { "visible" } else { "hidden" }];
    if start > 0 {
        times.push(clock.offset(start));
        values.push("visible");
    }
    if end < clock.frames {
        times.push(clock.offset(end));
        values.push("hidden");
    }
    format!(
        "<animate attributeName=\"visibility\" {} calcMode=\"discrete\" values=\"{}\" keyTimes=\"{}\"/>",
        clock.timing(),
        values.join(";"),
        times.join(";")
    )
}

impl Run {
    fn render(
        mut self,
        project: &Project,
        options: &BannerOptions,
        defs: &mut SharedBannerDefs,
        clock: Clock,
        out: &mut String,
    ) -> Result<(), ExportError> {
        let start = self.frames[0];
        let end = self.frames.last().expect("nonempty run") + 1;
        write!(
            out,
            "<g visibility=\"{}\">{}",
            if start == 0 { "visible" } else { "hidden" },
            visibility(start, end, clock)
        )
        .unwrap();
        let mut groups = 1;
        for (kind, range, identity, tolerance) in [
            ("translate", 0..2, &[0., 0.][..], 0.01),
            ("rotate", 2..3, &[0.][..], 0.0005),
            ("scale", 3..5, &[1., 1.][..], 0.00001),
            ("skewX", 5..6, &[0.][..], 0.0005),
            ("opacity", 6..7, &[1.][..], 0.0001),
        ] {
            if self.poses.iter().all(|p| p[range.clone()] == *identity) {
                continue;
            }
            let rows = self
                .poses
                .iter()
                .map(|pose| pose[range.clone()].to_vec())
                .collect::<Vec<_>>();
            let encode = |vals: &[f64]| {
                vals.iter()
                    .map(|v| fmt_num(*v, 4))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let initial = encode(&rows[0]);
            let (target, attr) = if kind == "opacity" {
                (
                    "animate attributeName=\"opacity\"".to_string(),
                    format!("opacity=\"{initial}\""),
                )
            } else {
                (
                    format!("animateTransform attributeName=\"transform\" type=\"{kind}\""),
                    format!("transform=\"{kind}({initial})\""),
                )
            };
            write!(out, "<g {attr}>").unwrap();
            groups += 1;
            if let Some(a) = animation(&rows, &self.frames, clock, &target, tolerance, encode) {
                out.push_str(&a);
            }
        }
        let mut animated_path = None;
        if let Some(original) = self.first_kind.take() {
            self.artwork.item.kind = original;
            animated_path = animation(
                &self.paths,
                &self.frames,
                clock,
                "animate attributeName=\"d\"",
                0.01,
                |row| path_value(&self.path_pattern, row, options.precision),
            );
        }
        render_banner_draw(
            project,
            &self.artwork,
            options,
            &BTreeMap::new(),
            &BTreeMap::new(),
            animated_path.as_deref(),
            defs,
            out,
        )?;
        for _ in 0..groups {
            out.push_str("</g>");
        }
        Ok(())
    }
}

pub(crate) fn compile_criteo_banner_html(
    project: &Project,
    options: &BannerOptions,
    cancel: &AtomicBool,
) -> Result<String, ExportError> {
    let check_cancel = || {
        if cancel.load(Ordering::Relaxed) {
            Err(ExportError::Options("Criteo estimate cancelled".into()))
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let mut diagnostics = preflight_native_banner(project);
    diagnostics.retain(|d| d.code != "banner-unsupported-shape-tween");
    let mut morph_targets = BTreeSet::new();
    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                let location = format!(
                    "Symbol '{}', layer '{}', frame {}",
                    sym.name,
                    layer.name,
                    key.frame + 1
                );
                let mut unsupported = |feature: &str| {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "criteo-unsupported-appearance".into(),
                        location: location.clone(),
                        message: format!("{feature} requires the standard HTML5 profile. Simplify or bake this layer for the Criteo template."),
                    });
                };
                if layer.properties.role == neoflash_core::LayerRole::Mask {
                    unsupported("Mask layer");
                }
                if !layer.properties.filters.is_empty()
                    || !key.filters.is_empty()
                    || key.elements.iter().any(|el| !el.filters.is_empty())
                {
                    unsupported("Display filters");
                }
                if key.elements.iter().any(|el| {
                    matches!(
                        el.blend_mode,
                        BlendMode::Alpha
                            | BlendMode::Erase
                            | BlendMode::Invert
                            | BlendMode::Subtract
                    )
                }) || matches!(
                    layer.properties.blend_mode,
                    BlendMode::Alpha | BlendMode::Erase | BlendMode::Invert | BlendMode::Subtract
                ) {
                    unsupported("Destination blend mode");
                }
                for element in &key.elements {
                    if let ElementKind::TextBox {
                        text,
                        format,
                        word_wrap,
                        multiline,
                        ..
                    } = &element.kind
                    {
                        let plain = neoflash_core::TextFormat {
                            font: format.font.clone(),
                            size: format.size,
                            color: format.color,
                            ..Default::default()
                        };
                        if *word_wrap
                            || *multiline
                            || text.contains(['\n', '\r'])
                            || format != &plain
                        {
                            unsupported(&format!(
                                "Formatted or multiline text field '{}'",
                                element.id
                            ));
                        }
                    }
                }
                if key.tween_kind == TweenKind::Shape && key.tween.is_some() {
                    morph_targets.extend(key.elements.iter().map(|el| el.id.clone()));
                }
            }
        }
    }
    let issues = diagnostics
        .into_iter()
        .filter(|d| d.blocking)
        .map(|d| format!("{} [{}]: {}", d.location, d.code, d.message))
        .collect::<Vec<_>>();
    if !issues.is_empty() {
        return Err(ExportError::UnsupportedScript(issues.join("\n")));
    }
    let click = options.click_tag_url.as_deref().unwrap_or("").trim();
    if !click.is_empty()
        && (!url::Url::parse(click).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
            || click.chars().any(char::is_control))
    {
        return Err(ExportError::Options(
            "Use an HTTP or HTTPS URL for the Criteo click destination.".into(),
        ));
    }
    let (mut eval, _) = flatten_scenes_with_ranges(project);
    let _ = crate::apply_bitmap_export_settings(&mut eval, options.jpeg_quality);
    let fonts = crate::apply_font_policy(&mut eval, crate::FontExportPolicy::Subset).css_rules;
    let frames = eval
        .symbol(&eval.root)
        .expect("valid project root")
        .timeline
        .frame_count;
    let clock = Clock {
        frames,
        fps: eval.fps,
        repeats: if !options.loop_timeline {
            Some(1)
        } else if options.max_loops == 0 {
            None
        } else {
            Some(options.max_loops)
        },
    };
    // An ordinary key following an untweened hold changes the pose instantly.
    // Look up the owning placement's clock (Graphic and MovieClip can both
    // differ from the root), including a child timeline looping to frame 0.
    let mut jumps: BTreeMap<&str, BTreeSet<u32>> = BTreeMap::new();
    for symbol in &eval.symbols {
        for layer in &symbol.timeline.layers {
            for (i, key) in layer.keyframes.iter().enumerate() {
                let cut = i == 0
                    || (key.keyed_properties.is_none()
                        && (layer.keyframes[i - 1].tween.is_none()
                            || layer.keyframes[i - 1].tween_kind == TweenKind::Motion));
                if cut {
                    for element in &key.elements {
                        jumps.entry(&element.id).or_default().insert(key.frame);
                    }
                }
            }
        }
    }
    let mut previous_clocks = std::collections::HashMap::new();
    let mut sample_bytes = 0;
    let mut playback = AuthoredPlayback::new(&eval, &eval.root, 0);
    let mut slots: Vec<Vec<Run>> = Vec::new();
    for frame in 0..frames {
        check_cancel()?;
        if frame > 0 {
            playback.tick(&eval);
        }
        let clocks = playback.clip_frames();
        let scene = playback.scene(&eval, false, true);
        for (index, draw) in scene.draws.into_iter().enumerate() {
            let morph = morph_targets.contains(&draw.item.element_id);
            if slots.len() <= index {
                slots.push(Vec::new());
            }
            let cut = draw.item.instance_path.iter().enumerate().any(|(i, id)| {
                let parent = &draw.item.instance_path[..i];
                let local = if i == 0 {
                    Some(frame)
                } else {
                    clocks.get(parent).copied()
                };
                local.is_some_and(|f| {
                    (i == 0 || previous_clocks.get(parent) != Some(&f))
                        && jumps
                            .get(id.as_str())
                            .is_some_and(|times| times.contains(&f))
                })
            });
            let layer = draw.item.layer_id.clone();
            sample_bytes += append(&mut slots[index], draw, frame, morph, cut);
            if sample_bytes > 128 * 1024 * 1024 {
                return Err(ExportError::Options(format!(
                    "Criteo animation data exceeds the compiler's 128 MiB sample budget at layer '{layer}', frame {}. Shorten the creative or simplify its geometry.",
                    frame + 1
                )));
            }
        }
        previous_clocks = clocks;
    }
    let mut defs = SharedBannerDefs::default();
    let mut artwork = String::new();
    for run in slots.into_iter().flatten() {
        check_cancel()?;
        run.render(&eval, options, &mut defs, clock, &mut artwork)?;
    }
    let notices = crate::criteo::authored_license_notices(project)?;
    let title = html_escape(&eval.name);
    let [r, g, b, a] = eval.background;
    let bg = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
    let (w, h) = (eval.width, eval.height);
    let click = serde_json::to_string(click)
        .expect("serialize string")
        .replace('<', "\\u003c");
    let pause = if options.autoplay {
        ""
    } else {
        "const svg=document.querySelector('svg');svg.pauseAnimations();svg.setCurrentTime(0);"
    };
    Ok(format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; object-src 'none'">
<link rel="icon" href="data:,"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="ad.size" content="width={w},height={h}"><title>{title}</title>
<style>html,body{{margin:0;background:{bg};overflow:hidden}}svg,a{{display:block}}{fonts}</style></head>
<body><a id="nf-ad" href="#" aria-label="{title}">
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<defs>{}</defs>{artwork}</svg></a>{notices}
<script>
{pause}window.clickTag=window.clickTag||new URLSearchParams(location.search).get('clickTag')||{click};
document.getElementById('nf-ad').addEventListener('click',event=>{{
  event.preventDefault();
  const url=String(window.clickTag||'').trim();
  if(/^https?:\/\//i.test(url)) window.open(url,'_blank','noopener,noreferrer');
}});
</script></body></html>"##,
        defs.defs_markup
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Easing, Element, Keyframe};

    fn creative(fps: u32, tween: Option<Easing>, kind: TweenKind) -> Project {
        let mut p = Project::blank();
        p.width = 300;
        p.height = 250;
        p.fps = fps;
        let path = |x| ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x, y: 0. },
                PathCommand::LineTo { x: 10., y: 10. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [12, 123, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        };
        let from = Element {
            id: "card".into(),
            kind: path(10.),
            ..Default::default()
        };
        let mut to = from.clone();
        if kind == TweenKind::Shape {
            to.kind = path(28.);
        } else {
            to.transform.x = 75.;
            to.transform.alpha = 0.25;
        }
        let tl = &mut p.symbols[0].timeline;
        tl.frame_count = fps * 2 + 1;
        tl.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![from],
                tween,
                tween_kind: kind,
                ..Default::default()
            },
            Keyframe {
                frame: fps,
                elements: vec![to],
                keyed_properties: (kind == TweenKind::Motion).then(|| {
                    vec![
                        neoflash_core::TweenProperty::X,
                        neoflash_core::TweenProperty::Alpha,
                    ]
                }),
                ..Default::default()
            },
        ];
        p.upgrade_format_for_features();
        p
    }

    #[test]
    fn criteo_eased_key_data_is_independent_of_fps() {
        for kind in [TweenKind::Classic, TweenKind::Motion] {
            for rate in [24, 60] {
                let project = creative(rate, Some(Easing::EaseOut), kind);
                let html = compile_criteo_banner_html(
                    &project,
                    &BannerOptions::default(),
                    &AtomicBool::new(false),
                )
                .unwrap();
                assert_eq!(html.matches("<animateTransform ").count(), 1);
                assert_eq!(html.matches("<path id=").count(), 1);
                assert!(
                    html.contains("values=\"0 0;75 0;75 0;75 0\""),
                    "{kind:?}/{rate}"
                );
                assert!(html.contains("values=\"1;0.25;0.25;0.25\""));
                assert!(html.contains(".333333 0.6667 .666667 1"));
            }
        }
    }

    #[test]
    fn criteo_ordinary_key_cuts_without_a_frame_of_gliding() {
        let html = compile_criteo_banner_html(
            &creative(24, None, TweenKind::Classic),
            &BannerOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!html.contains("<animateTransform "));
        assert!(html.contains("transform=\"translate(75 0)\""));
        assert_eq!(html.matches("calcMode=\"discrete\"").count(), 2);
    }

    #[test]
    fn criteo_morph_uses_native_path_animation() {
        let html = compile_criteo_banner_html(
            &creative(24, Some(Easing::Linear), TweenKind::Shape),
            &BannerOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(html.matches("<animate attributeName=\"d\"").count(), 1);
    }
}
