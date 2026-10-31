//! Motion editor painters: the frame scale above the property lanes, one `Curve · <property>` lane per
//! animated property with its keyframe diamonds, and the lane playhead (Panel / Motion editor, §6).
use crate::theme::palette;
use crate::tokens::{TextStyle, space};
use eframe::egui::{self, Painter, Pos2, Rect, Shape, Stroke};

/// Keyframe on a motion lane: `frame` (0-based inside the span) and normalised `value` (0 = bottom, 1 = top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneKey {
    pub frame: f32,
    pub value: f32,
}

/// Frame numbers (`NF2/Mono Small`, `text/tertiary`) every `step` frames across `rect`.
pub fn paint_frame_scale(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    frames: u32,
    step: u32,
) {
    let p = palette(ctx);
    let frames = frames.max(1);
    let mut f = 1;
    while f <= frames {
        let x = rect.left() + (f - 1) as f32 / frames as f32 * rect.width();
        let g = ctx.fonts_mut(|fonts| {
            fonts.layout_job(egui::text::LayoutJob::single_section(
                f.to_string(),
                crate::typography::format(TextStyle::MonoSmall, p.text_tertiary),
            ))
        });
        painter.galley(
            egui::pos2(x + space::S1, rect.center().y - g.size().y / 2.0),
            g,
            p.text_tertiary,
        );
        f += step.max(1);
    }
}

/// `Curve · <property>` lane: bottom separator, smooth curve through `keys` and ◆ keyframes.
/// `selected` = the edited property (accent curve on `bg/selected`).
pub fn paint_curve_lane(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    frames: u32,
    keys: &[LaneKey],
    selected: bool,
) {
    let p = palette(ctx);
    if selected {
        painter.rect_filled(rect, 0.0, p.bg_selected);
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, p.border_subtle),
    );
    let frames = frames.max(2) as f32;
    let inner = rect.shrink2(egui::vec2(space::S3, space::S3));
    let pos = |k: LaneKey| {
        egui::pos2(
            inner.left() + k.frame / (frames - 1.0) * inner.width(),
            inner.bottom() - k.value.clamp(0.0, 1.0) * inner.height(),
        )
    };
    let color = if selected {
        p.accent_default
    } else {
        p.text_tertiary
    };
    // Flat-tangent cubic segments between keys (Flash motion editor default "smooth" points).
    let mut points: Vec<Pos2> = Vec::new();
    for pair in keys.windows(2) {
        let (a, b) = (pos(pair[0]), pos(pair[1]));
        for i in 0..=16 {
            let t = i as f32 / 16.0;
            let s = t * t * (3.0 - 2.0 * t);
            points.push(egui::pos2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * s));
        }
    }
    if points.len() > 1 {
        painter.add(Shape::line(
            points,
            Stroke::new(if selected { 2.0_f32 } else { 1.5 }, color),
        ));
    }
    for k in keys {
        paint_key_diamond(painter, pos(*k), color);
    }
}

/// ◆ keyframe marker (10 × 10) used on motion lanes and in the keyframe navigator.
pub fn paint_key_diamond(painter: &Painter, center: Pos2, color: egui::Color32) {
    let r = 5.0;
    painter.add(Shape::convex_polygon(
        vec![
            center + egui::vec2(0.0, -r),
            center + egui::vec2(r, 0.0),
            center + egui::vec2(0.0, r),
            center + egui::vec2(-r, 0.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Playhead across the lanes (`timeline/playhead`) with the frame number above.
pub fn paint_lane_playhead(
    painter: &Painter,
    ctx: &egui::Context,
    x: f32,
    scale: Rect,
    bottom: f32,
    frame: u32,
) {
    let p = palette(ctx);
    painter.vline(
        x,
        scale.top()..=bottom,
        Stroke::new(1.0_f32, p.timeline_playhead),
    );
    let g = ctx.fonts_mut(|fonts| {
        fonts.layout_job(egui::text::LayoutJob::single_section(
            frame.to_string(),
            crate::typography::format(TextStyle::MonoSmall, p.timeline_playhead),
        ))
    });
    painter.galley(
        egui::pos2(x + 2.0, scale.top() + 2.0),
        g,
        p.timeline_playhead,
    );
}
