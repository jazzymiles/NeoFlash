//! T-101 / R-056: every icon grid has its own optical master.
//!
//! 12 and 16 px: 1 pt strokes whose straight horizontal / vertical segments sit on half pixels,
//! fill edges on whole pixels. 20 px: 1.5 pt strokes on .25 / .75, fill edges on half points.
//! A fill edge may sit on a stroke axis of the same icon (the stroke covers it). Icons asked for at
//! another grid size come from that grid's master instead of being scaled.
use neoflash_ui::icons::{i12, i16, i20, Icon};

/// (stroked, stroke width, axis-aligned straight segments as (axis, coordinate)).
type PathFacts = (bool, Option<f32>, Vec<(usize, f32)>);

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn numbers(d: &str) -> Vec<(char, Vec<f32>)> {
    let mut out: Vec<(char, Vec<f32>)> = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, out: &mut Vec<(char, Vec<f32>)>| {
        if !num.is_empty() {
            out.last_mut()
                .expect("command before number")
                .1
                .push(num.parse().unwrap());
            num.clear();
        }
    };
    for ch in d.chars() {
        if "MLHVCZ".contains(ch) {
            flush(&mut num, &mut out);
            out.push((ch, Vec::new()));
        } else if ch == ' ' || ch == ',' || (ch == '-' && !num.is_empty()) {
            flush(&mut num, &mut out);
            if ch == '-' {
                num.push(ch);
            }
        } else {
            num.push(ch);
        }
    }
    flush(&mut num, &mut out);
    out
}

fn facts(svg: &str) -> Vec<PathFacts> {
    svg.split("<path")
        .skip(1)
        .map(|rest| {
            let tag = &rest[..rest.find("/>").unwrap()];
            let stroked = attr(tag, "stroke").is_some_and(|s| s != "none");
            // SVG's specified default stroke width is 1 user unit. Figma omits
            // the attribute when a 12/16 px optical master uses that default.
            let width = attr(tag, "stroke-width")
                .map(|w| w.parse().unwrap())
                .or_else(|| stroked.then_some(1.0));
            let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
            let mut segs = Vec::new();
            for (cmd, a) in numbers(&attr(tag, "d").unwrap()) {
                // Commands may repeat their arguments (implicit L after M is not used by the export).
                let step = match cmd {
                    'M' | 'L' => 2,
                    'H' | 'V' => 1,
                    'C' => 6,
                    _ => 0,
                };
                let chunks: Vec<&[f32]> = if step == 0 {
                    vec![&[]]
                } else {
                    a.chunks(step).collect()
                };
                for c in chunks {
                    let next = match cmd {
                        'M' => {
                            start = (c[0], c[1]);
                            cur = start;
                            continue;
                        }
                        'L' => (c[0], c[1]),
                        'H' => (c[0], cur.1),
                        'V' => (cur.0, c[0]),
                        'C' => (c[4], c[5]),
                        _ => start,
                    };
                    if cmd != 'C' {
                        if (next.0 - cur.0).abs() < 1e-3 && (next.1 - cur.1).abs() > 1e-3 {
                            segs.push((0, cur.0));
                        }
                        if (next.1 - cur.1).abs() < 1e-3 && (next.0 - cur.0).abs() > 1e-3 {
                            segs.push((1, cur.1));
                        }
                    }
                    cur = next;
                }
            }
            (stroked, width, segs)
        })
        .collect()
}

fn on_grid(v: f32, step: f32, phase: f32) -> bool {
    let r = (v - phase) / step;
    (r - r.round()).abs() < 1e-3
}

#[test]
fn every_grid_has_its_own_optical_master() {
    assert_eq!(
        i12::ALL.len(),
        i16::ALL.len(),
        "a 12 px master for every 16 px icon"
    );
    for icon in i16::ALL {
        let small = Icon::by_path(&format!("12/{}", icon.name)).expect("12 px master");
        assert_eq!(small.size, 12);
        assert!(
            small.svg.contains("viewBox=\"0 0 12 12\""),
            "{}",
            small.path()
        );
    }
    let mut checked = 0;
    for icon in i12::ALL.iter().chain(&i16::ALL).chain(&i20::ALL) {
        let fine = icon.size == 20;
        let (width, stroke_phase, fill_step) = if fine {
            (1.5, 0.25, 0.5)
        } else {
            (1.0, 0.5, 1.0)
        };
        let paths = facts(icon.svg);
        let axes: Vec<(usize, f32)> = paths
            .iter()
            .filter(|p| p.0)
            .flat_map(|p| p.2.iter().copied())
            .collect();
        for (stroked, w, segs) in &paths {
            if *stroked {
                assert_eq!(*w, Some(width), "{} stroke width", icon.path());
            }
            for &(axis, v) in segs {
                let ok = if *stroked {
                    on_grid(v, if fine { 0.5 } else { 1.0 }, stroke_phase)
                } else {
                    on_grid(v, fill_step, 0.0)
                        || axes.iter().any(|&(a, k)| a == axis && (k - v).abs() < 1e-3)
                };
                assert!(
                    ok,
                    "{}: {} edge at {v} (axis {axis}) is off the grid",
                    icon.path(),
                    if *stroked { "stroke" } else { "fill" }
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 2000,
        "the check saw the straight segments ({checked})"
    );
}

#[test]
fn icons_asked_for_at_another_grid_use_that_master() {
    assert_eq!(i16::EYE.at(12.0), i12::EYE);
    assert_eq!(i12::EYE.at(16.0), i16::EYE);
    assert_eq!(i20::HAND.at(16.0), i16::HAND);
    assert_eq!(i16::HAND.at(20.0), i20::HAND);
    // No master of that size: the icon itself is scaled.
    assert_eq!(i16::EYE.at(14.0), i16::EYE);
    assert_eq!(i20::TOOL_PEN.at(16.0), i20::TOOL_PEN);
    assert_eq!(i16::EYE.at(16.0), i16::EYE);
}
