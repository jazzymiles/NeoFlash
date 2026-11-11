#![allow(clippy::too_many_arguments)]

pub mod as2_suite;
pub mod as3_suite;
pub mod express5;

use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use neoflash_core::{
    BitmapCompression, BitmapExportSettings, Color, Element, ElementKind, FillRule, GradientKind,
    GradientPaint, GradientSpread, GradientStop, InstancePlayback, Keyframe, Layer,
    LayerProperties, PathCommand, Project, ResourceUnit, Symbol, SymbolKind, Timeline, Transform,
    build_minimal_mjpeg_avi,
};
use std::io::Cursor;

pub fn tf(x: f64, y: f64) -> Transform {
    Transform {
        x,
        y,
        ..Transform::default()
    }
}

pub fn rect_el(
    id: &str,
    name: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Rectangle {
            width: w,
            height: h,
            fill,
            stroke,
            stroke_width,
        },
        ..Element::default()
    }
}

pub fn ellipse_el(
    id: &str,
    name: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Ellipse {
            width: w,
            height: h,
            fill,
            stroke,
            stroke_width,
        },
        ..Element::default()
    }
}

pub fn text_el(
    id: &str,
    name: &str,
    x: f64,
    y: f64,
    text: &str,
    font_size: f64,
    color: Color,
) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Text {
            text: text.into(),
            font_family: "Arial".into(),
            font_size,
            color,
        },
        ..Element::default()
    }
}

pub fn instance_el(
    id: &str,
    name: &str,
    symbol_id: &str,
    x: f64,
    y: f64,
    playback: InstancePlayback,
) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback,
        },
        ..Element::default()
    }
}

pub fn path_el(
    id: &str,
    name: &str,
    x: f64,
    y: f64,
    commands: Vec<PathCommand>,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill,
            stroke,
            stroke_width,
        },
        ..Element::default()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn gradient_rect_el(
    id: &str,
    name: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    stops: Vec<(u8, Color)>,
    vertical: bool,
) -> Element {
    let commands = vec![
        PathCommand::MoveTo { x: 0.0, y: 0.0 },
        PathCommand::LineTo { x: w, y: 0.0 },
        PathCommand::LineTo { x: w, y: h },
        PathCommand::LineTo { x: 0.0, y: h },
        PathCommand::Close,
    ];
    let matrix = if vertical {
        [0.0, h / 1638.4, -w / 1638.4, 0.0, w * 0.5, h * 0.5]
    } else {
        [w / 1638.4, 0.0, 0.0, h / 1638.4, w * 0.5, h * 0.5]
    };
    let fallback = stops.first().map(|(_, c)| *c).unwrap_or([16, 24, 40, 255]);
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::PaintedPath {
            commands,
            fill_rule: FillRule::NonZero,
            fill: fallback,
            stroke: [0, 0, 0, 0],
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                matrix,
                stops: stops
                    .into_iter()
                    .map(|(ratio, color)| GradientStop { ratio, color })
                    .collect(),
                focal_point_ratio: 0.0,
                spread: GradientSpread::Extend,
                linear_rgb: false,
            }),
            stroke_gradient: None,
            stroke_width: 0.0,
        },
        ..Element::default()
    }
}

pub fn radial_glow_el(
    id: &str,
    name: &str,
    cx: f64,
    cy: f64,
    radius: f64,
    inner: Color,
    mid: Color,
    outer: Color,
) -> Element {
    let k = 0.552_284_75 * radius;
    let commands = vec![
        PathCommand::MoveTo { x: 0.0, y: -radius },
        PathCommand::CubicTo {
            cx1: k,
            cy1: -radius,
            cx2: radius,
            cy2: -k,
            x: radius,
            y: 0.0,
        },
        PathCommand::CubicTo {
            cx1: radius,
            cy1: k,
            cx2: k,
            cy2: radius,
            x: 0.0,
            y: radius,
        },
        PathCommand::CubicTo {
            cx1: -k,
            cy1: radius,
            cx2: -radius,
            cy2: k,
            x: -radius,
            y: 0.0,
        },
        PathCommand::CubicTo {
            cx1: -radius,
            cy1: -k,
            cx2: -k,
            cy2: -radius,
            x: 0.0,
            y: -radius,
        },
        PathCommand::Close,
    ];
    let scale = radius / 819.2;
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(cx, cy),
        kind: ElementKind::PaintedPath {
            commands,
            fill_rule: FillRule::NonZero,
            fill: inner,
            stroke: [0, 0, 0, 0],
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Radial,
                matrix: [scale, 0.0, 0.0, scale, 0.0, 0.0],
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: inner,
                    },
                    GradientStop {
                        ratio: 140,
                        color: mid,
                    },
                    GradientStop {
                        ratio: 255,
                        color: outer,
                    },
                ],
                focal_point_ratio: 0.0,
                spread: GradientSpread::Extend,
                linear_rgb: false,
            }),
            stroke_gradient: None,
            stroke_width: 0.0,
        },
        ..Element::default()
    }
}

pub fn single_key_layer(id: &str, name: &str, elements: Vec<Element>) -> Layer {
    Layer {
        id: id.into(),
        name: name.into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements,
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    }
}

pub fn empty_holder_symbol(id: &str, name: &str) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(&format!("{id}_layer"), "Holder", vec![])],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

pub fn dashboard_chrome_elements(
    prefix: &str,
    eyebrow: &str,
    title: &str,
    subtitle: &str,
    accent: Color,
) -> Vec<Element> {
    vec![
        gradient_rect_el(
            &format!("{prefix}_bg"),
            "",
            0.0,
            0.0,
            960.0,
            540.0,
            vec![(0, [11, 17, 32, 255]), (255, [20, 30, 52, 255])],
            true,
        ),
        rect_el(
            &format!("{prefix}_header_card"),
            "",
            36.0,
            24.0,
            888.0,
            108.0,
            [22, 33, 56, 235],
            accent,
            1.5,
        ),
        rect_el(
            &format!("{prefix}_stage_card"),
            "",
            36.0,
            150.0,
            888.0,
            362.0,
            [15, 23, 42, 240],
            [51, 65, 85, 255],
            1.5,
        ),
        text_el(
            &format!("{prefix}_eyebrow"),
            "eyebrow",
            56.0,
            38.0,
            eyebrow,
            12.0,
            accent,
        ),
        text_el(
            &format!("{prefix}_title"),
            "title",
            56.0,
            60.0,
            title,
            26.0,
            [241, 245, 249, 255],
        ),
        text_el(
            &format!("{prefix}_subtitle"),
            "subtitle",
            56.0,
            96.0,
            subtitle,
            14.0,
            [148, 163, 184, 255],
        ),
        text_el(
            &format!("{prefix}_status"),
            "statusText",
            56.0,
            170.0,
            "INITIALIZING...",
            18.0,
            [125, 211, 252, 255],
        ),
        text_el(
            &format!("{prefix}_detail"),
            "detailText",
            56.0,
            204.0,
            "",
            14.0,
            [203, 213, 225, 255],
        ),
    ]
}

pub fn base_dashboard_project(
    name: &str,
    root_symbol: Symbol,
    extra_symbols: Vec<Symbol>,
) -> Project {
    let mut symbols = extra_symbols;
    let root_id = root_symbol.id.clone();
    symbols.push(root_symbol);
    let mut project = Project {
        format_version: 5,
        name: name.into(),
        width: 960,
        height: 540,
        fps: 24,
        background: [11, 17, 32, 255],
        root: root_id,
        symbols,
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
    };
    project.upgrade_format_for_features();
    project
}

/// Synthesizes a clean stereo 16-bit PCM WAV with harmonic chords and optional rhythmic pulse.
pub fn make_harmonic_stereo_wav(duration_ms: u32, freqs_hz: &[f32], chug_pulse: bool) -> Vec<u8> {
    let sample_rate = 22_050u32;
    let frames = ((u64::from(sample_rate) * u64::from(duration_ms)) / 1000) as usize;
    let channels = 2u16;
    let bytes_per_frame = usize::from(channels) * 2;
    let data_bytes = (frames * bytes_per_frame) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    let tau = std::f32::consts::TAU;
    let inv_sr = 1.0 / sample_rate as f32;
    let total_s = (frames as f32 * inv_sr).max(0.01);
    for i in 0..frames {
        let t = i as f32 * inv_sr;
        let attack = (t / 0.08).clamp(0.0, 1.0);
        let release = ((total_s - t) / 0.18).clamp(0.0, 1.0);
        let env = attack * release;

        let mut chord_l = 0.0f32;
        let mut chord_r = 0.0f32;
        for (idx, &f) in freqs_hz.iter().enumerate() {
            let detune = 1.0 + (idx as f32 - 1.0) * 0.0018;
            let s_l = (tau * f * t).sin();
            let s_r = (tau * f * detune * t + 0.35 * idx as f32).sin();
            let weight = 1.0 / (freqs_hz.len() as f32).max(1.0);
            chord_l += s_l * weight;
            chord_r += s_r * weight;
        }

        let pulse = if chug_pulse {
            let beat = (t * 6.0).fract();
            let puff = (-beat * 9.0).exp()
                * ((tau * 92.0 * t).sin() * 0.5 + (tau * 184.0 * t).sin() * 0.3);
            puff * 0.32
        } else {
            0.0
        };

        let l = ((chord_l * 0.55 + pulse) * env * 12_000.0).clamp(-30_000.0, 30_000.0) as i16;
        let r = ((chord_r * 0.55 + pulse * 0.9) * env * 12_000.0).clamp(-30_000.0, 30_000.0) as i16;
        wav.extend_from_slice(&l.to_le_bytes());
        wav.extend_from_slice(&r.to_le_bytes());
    }
    wav
}

pub fn make_crest_png(width: u32, height: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(width, height);
    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    let max_r = cx.min(cy) - 2.0;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let px = if r > max_r {
                [0, 0, 0, 0]
            } else if r > max_r - 3.0 {
                [250, 204, 21, 255]
            } else if (r - max_r * 0.55).abs() < 2.2 {
                [56, 189, 248, 255]
            } else {
                let glow = (1.0 - r / max_r).clamp(0.0, 1.0);
                [
                    (18.0 + glow * 52.0) as u8,
                    (28.0 + glow * 88.0) as u8,
                    (64.0 + glow * 140.0) as u8,
                    245,
                ]
            };
            img.put_pixel(x, y, Rgba(px));
        }
    }
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)
        .expect("encode crest PNG");
    buf.into_inner()
}

pub fn make_crest_resource(id: &str, path: &str) -> ResourceUnit {
    let bytes = make_crest_png(48, 48);
    let mut unit = ResourceUnit::from_bytes(id, path, "image/png", &bytes).expect("crest resource");
    unit.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::LosslessPng,
        use_imported_jpeg_data: true,
        jpeg_quality: 90,
        allow_smoothing: true,
    });
    unit
}

pub fn make_sample_mjpeg_avi() -> Vec<u8> {
    let mut frames = Vec::new();
    for (r, g, b) in [
        (38u8, 132u8, 245u8),
        (245u8, 158u8, 38u8),
        (52u8, 211u8, 153u8),
    ] {
        let mut img = RgbImage::new(64, 36);
        for y in 0..36 {
            for x in 0..64 {
                let stripe = ((x + y) % 16 < 8) as u8 * 24;
                img.put_pixel(
                    x,
                    y,
                    Rgb([r.saturating_add(stripe), g.saturating_add(stripe / 2), b]),
                );
            }
        }
        let mut jpg = Cursor::new(Vec::new());
        img.write_to(&mut jpg, ImageFormat::Jpeg)
            .expect("encode JPEG frame");
        frames.push(jpg.into_inner());
    }
    build_minimal_mjpeg_avi(64, 36, 12, &frames, None)
}
