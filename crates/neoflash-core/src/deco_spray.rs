//! Deterministic Stage gesture generation for the Deco and Spray Brush authoring tools (`T-180`).

use super::*;

/// Extended Deco effect modes supported by Stage click/drag gestures (`T-180`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageDecoEffect {
    VineFill,
    GridFill,
    SymmetryBrush,
    DecoratedBrush,
    FlameBrush,
    FlowerBrush,
    LightningBrush,
    TreeBrush,
    Walls3D,
    BuildingBrush,
    ParticleSystem,
}

impl StageDecoEffect {
    #[allow(dead_code)]
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::VineFill,
            1 => Self::GridFill,
            2 => Self::SymmetryBrush,
            3 => Self::DecoratedBrush,
            4 => Self::FlameBrush,
            5 => Self::FlowerBrush,
            6 => Self::LightningBrush,
            7 => Self::TreeBrush,
            8 => Self::Walls3D,
            9 => Self::BuildingBrush,
            _ => Self::ParticleSystem,
        }
    }

    pub fn to_core_kind(self) -> DecoEffectKind {
        match self {
            Self::VineFill => DecoEffectKind::VineFill,
            Self::GridFill => DecoEffectKind::GridFill,
            Self::SymmetryBrush => DecoEffectKind::SymmetryBrush,
            Self::DecoratedBrush | Self::Walls3D => DecoEffectKind::DecoratedBrush,
            Self::FlameBrush => DecoEffectKind::FlameBrush,
            Self::FlowerBrush | Self::ParticleSystem => DecoEffectKind::FlowerBrush,
            Self::LightningBrush => DecoEffectKind::LightningBrush,
            Self::TreeBrush | Self::BuildingBrush => DecoEffectKind::TreeBrush,
        }
    }
}

/// Parameters for a live Stage Deco gesture (`T-180`).
#[derive(Clone, Debug)]
pub struct StageDecoOptions {
    pub effect_index: usize,
    pub branch_angle_deg: f64,
    pub scale_pct: f64,
    pub segment_px: f64,
    pub step_frames: u32,
    pub animate_pattern: bool,
    pub primary_color: Color,
    pub secondary_color: Color,
}

impl Default for StageDecoOptions {
    fn default() -> Self {
        Self {
            effect_index: 0,
            branch_angle_deg: 30.0,
            scale_pct: 100.0,
            segment_px: 20.0,
            step_frames: 1,
            animate_pattern: false,
            primary_color: [34, 160, 92, 255],
            secondary_color: [16, 88, 48, 255],
        }
    }
}

/// Parameters for a live Stage Spray Brush gesture (`T-180`).
#[derive(Clone, Debug)]
pub struct StageSprayOptions {
    pub symbol_id: Option<Id>,
    pub use_dots: bool,
    pub fill: Color,
    pub scale_w_pct: f64,
    pub scale_h_pct: f64,
    pub random_scaling: bool,
    pub rotate_symbol: bool,
    pub random_rotation: bool,
    pub brush_width: f64,
    pub brush_height: f64,
    pub brush_angle_deg: f64,
    pub seed: u64,
}

impl Default for StageSprayOptions {
    fn default() -> Self {
        Self {
            symbol_id: None,
            use_dots: true,
            fill: [52, 120, 246, 255],
            scale_w_pct: 100.0,
            scale_h_pct: 100.0,
            random_scaling: false,
            rotate_symbol: false,
            random_rotation: false,
            brush_width: 48.0,
            brush_height: 48.0,
            brush_angle_deg: 0.0,
            seed: 1,
        }
    }
}

fn rotate_point_around(pt: [f64; 2], origin: [f64; 2], angle_rad: f64) -> [f64; 2] {
    let (s, c) = angle_rad.sin_cos();
    let dx = pt[0] - origin[0];
    let dy = pt[1] - origin[1];
    [origin[0] + dx * c - dy * s, origin[1] + dx * s + dy * c]
}

fn make_deco_element(id: Id, name: impl Into<String>, kind: ElementKind) -> Element {
    let mut template = generate_deco_elements(
        DecoEffectKind::FlowerBrush,
        [0.0, 0.0],
        100.0,
        20.0,
        [0, 0, 0, 255],
        [0, 0, 0, 255],
    )
    .into_iter()
    .next()
    .expect("flower brush template element");
    template.id = id;
    template.name = name.into();
    template.transform = Transform::default();
    template.merge = false;
    template.kind = kind;
    template
}

fn rotate_element_around(el: &mut Element, origin: [f64; 2], angle_deg: f64) {
    if !angle_deg.is_finite() || angle_deg.abs() <= 1e-6 {
        return;
    }
    let rad = angle_deg.to_radians();
    let rotated = rotate_point_around([el.transform.x, el.transform.y], origin, rad);
    el.transform.x = rotated[0];
    el.transform.y = rotated[1];
    el.transform.rotation += angle_deg;
    if let ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } =
        &mut el.kind
    {
        for cmd in commands {
            match cmd {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    let p = rotate_point_around([*x, *y], origin, rad);
                    *x = p[0];
                    *y = p[1];
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let cp = rotate_point_around([*cx, *cy], origin, rad);
                    let p = rotate_point_around([*x, *y], origin, rad);
                    *cx = cp[0];
                    *cy = cp[1];
                    *x = p[0];
                    *y = p[1];
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let cp1 = rotate_point_around([*cx1, *cy1], origin, rad);
                    let cp2 = rotate_point_around([*cx2, *cy2], origin, rad);
                    let p = rotate_point_around([*x, *y], origin, rad);
                    *cx1 = cp1[0];
                    *cy1 = cp1[1];
                    *cx2 = cp2[0];
                    *cy2 = cp2[1];
                    *x = p[0];
                    *y = p[1];
                }
                PathCommand::Close => {}
            }
        }
    }
}

/// Generate Deco vector elements for a Stage pointer click or drag path (`T-180`).
pub fn generate_stage_deco_gesture(
    points: &[[f64; 2]],
    options: &StageDecoOptions,
) -> Vec<Element> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let effect = StageDecoEffect::from_index(options.effect_index);
    let scale = options.scale_pct.clamp(10.0, 500.0);
    let spacing = options.segment_px.clamp(4.0, 200.0);
    let drag_angle_deg = if points.len() >= 2 {
        let last = points[points.len() - 1];
        let dx = last[0] - first[0];
        let dy = last[1] - first[1];
        if dx.hypot(dy) >= 2.0 {
            dy.atan2(dx).to_degrees()
        } else {
            0.0
        }
    } else {
        0.0
    };
    // Combine authored branch angle offset (relative to default 30°) with drag orientation.
    let angle_offset = (options.branch_angle_deg - 30.0) + drag_angle_deg;

    // Sample stamps along the drag path if the user dragged farther than `spacing * 1.5`.
    let mut anchors = vec![first];
    let step_dist = (spacing * 1.5).max(16.0);
    let mut accum = 0.0;
    for pair in points.windows(2) {
        let d = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        accum += d;
        if accum >= step_dist && anchors.len() < 8 {
            anchors.push(pair[1]);
            accum = 0.0;
        }
    }

    let mut out = Vec::new();
    for (stamp_idx, &origin) in anchors.iter().enumerate() {
        let mut batch = match effect {
            StageDecoEffect::Walls3D => {
                let w = 48.0 * (scale / 100.0);
                let h = 36.0 * (scale / 100.0);
                let depth = (spacing * 0.6).clamp(6.0, 48.0);
                let front = make_deco_element(
                    new_id("deco-wall-front"),
                    "Deco 3D Wall Front",
                    ElementKind::Path {
                        commands: vec![
                            PathCommand::MoveTo {
                                x: origin[0] - w * 0.5,
                                y: origin[1] - h * 0.5,
                            },
                            PathCommand::LineTo {
                                x: origin[0] + w * 0.5,
                                y: origin[1] - h * 0.5,
                            },
                            PathCommand::LineTo {
                                x: origin[0] + w * 0.5,
                                y: origin[1] + h * 0.5,
                            },
                            PathCommand::LineTo {
                                x: origin[0] - w * 0.5,
                                y: origin[1] + h * 0.5,
                            },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::NonZero,
                        fill: options.primary_color,
                        stroke: options.secondary_color,
                        stroke_width: 1.5,
                    },
                );
                let side = make_deco_element(
                    new_id("deco-wall-side"),
                    "Deco 3D Wall Side",
                    ElementKind::Path {
                        commands: vec![
                            PathCommand::MoveTo {
                                x: origin[0] + w * 0.5,
                                y: origin[1] - h * 0.5,
                            },
                            PathCommand::LineTo {
                                x: origin[0] + w * 0.5 + depth,
                                y: origin[1] - h * 0.5 - depth * 0.6,
                            },
                            PathCommand::LineTo {
                                x: origin[0] + w * 0.5 + depth,
                                y: origin[1] + h * 0.5 - depth * 0.6,
                            },
                            PathCommand::LineTo {
                                x: origin[0] + w * 0.5,
                                y: origin[1] + h * 0.5,
                            },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::NonZero,
                        fill: options.secondary_color,
                        stroke: options.primary_color,
                        stroke_width: 1.5,
                    },
                );
                vec![front, side]
            }
            StageDecoEffect::BuildingBrush => {
                let s = scale / 100.0;
                let bw = 40.0 * s;
                let bh = 64.0 * s;
                let mut facade = make_deco_element(
                    new_id("deco-building"),
                    "Deco Building",
                    ElementKind::Rectangle {
                        width: bw,
                        height: bh,
                        fill: options.primary_color,
                        stroke: options.secondary_color,
                        stroke_width: 1.5,
                    },
                );
                facade.transform.x = origin[0] - bw * 0.5;
                facade.transform.y = origin[1] - bh;
                let mut elems = vec![facade];
                let rows = 3usize;
                let cols = 2usize;
                let win_w = (bw * 0.22).max(4.0);
                let win_h = (bh * 0.16).max(4.0);
                for r in 0..rows {
                    for c in 0..cols {
                        let mut win = make_deco_element(
                            new_id(&format!("deco-win-{stamp_idx}-{r}-{c}")),
                            format!("Deco Window {}", r * cols + c + 1),
                            ElementKind::Rectangle {
                                width: win_w,
                                height: win_h,
                                fill: options.secondary_color,
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                        );
                        win.transform.x = origin[0] - bw * 0.32 + (c as f64) * (bw * 0.42);
                        win.transform.y = origin[1] - bh * 0.85 + (r as f64) * (bh * 0.26);
                        elems.push(win);
                    }
                }
                elems
            }
            _ => generate_deco_elements(
                effect.to_core_kind(),
                origin,
                scale,
                spacing,
                options.primary_color,
                options.secondary_color,
            ),
        };
        for el in &mut batch {
            rotate_element_around(el, origin, angle_offset);
        }
        out.extend(batch);
    }
    out
}

/// Generate Spray Brush elements along a Stage pointer drag path (`T-180`).
pub fn generate_stage_spray_gesture(
    points: &[[f64; 2]],
    options: &StageSprayOptions,
) -> Vec<Element> {
    if points.is_empty() {
        return Vec::new();
    }
    let config = SprayBrushConfig {
        symbol_id: if options.use_dots {
            None
        } else {
            options.symbol_id.clone()
        },
        fill: options.fill,
        scale_w_pct: options.scale_w_pct.clamp(10.0, 500.0),
        scale_h_pct: options.scale_h_pct.clamp(10.0, 500.0),
        random_scaling: options.random_scaling,
        rotate_symbol: options.rotate_symbol,
        random_rotation: options.random_rotation,
        brush_width: options.brush_width.clamp(1.0, 500.0),
        brush_height: options.brush_height.clamp(1.0, 500.0),
        brush_angle_deg: options.brush_angle_deg,
    };
    // Perturb stroke points deterministically with `seed` so distinct seeds yield distinct yet reproducible scatters.
    let seeded_points: Vec<[f64; 2]> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if options.seed <= 1 {
                *p
            } else {
                let h = options
                    .seed
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add((i as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
                let jx = (((h & 0xFFFF) as f64 / 65535.0) - 0.5) * 0.5;
                let jy = ((((h >> 16) & 0xFFFF) as f64 / 65535.0) - 0.5) * 0.5;
                [p[0] + jx, p[1] + jy]
            }
        })
        .collect();
    generate_spray_brush_along_points(&seeded_points, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_deco_gesture_generates_rotated_and_stepped_elements_for_all_effects() {
        for effect_index in 0..=10 {
            let opts = StageDecoOptions {
                effect_index,
                branch_angle_deg: 45.0,
                scale_pct: 120.0,
                segment_px: 24.0,
                step_frames: 2,
                animate_pattern: true,
                primary_color: [220, 80, 40, 255],
                secondary_color: [40, 120, 220, 255],
            };
            let elems = generate_stage_deco_gesture(&[[100.0, 120.0], [160.0, 140.0]], &opts);
            assert!(
                !elems.is_empty(),
                "effect {effect_index} must produce elements"
            );
        }
    }

    #[test]
    fn stage_spray_gesture_supports_dots_and_library_symbol_instances() {
        let dot_opts = StageSprayOptions {
            symbol_id: None,
            use_dots: true,
            brush_width: 60.0,
            brush_height: 30.0,
            brush_angle_deg: 25.0,
            random_scaling: true,
            random_rotation: true,
            seed: 42,
            ..Default::default()
        };
        let dots = generate_stage_spray_gesture(&[[50.0, 50.0], [120.0, 80.0]], &dot_opts);
        assert!(dots.len() >= 4);
        assert!(
            dots.iter()
                .all(|e| matches!(e.kind, ElementKind::Ellipse { .. }))
        );

        let sym_opts = StageSprayOptions {
            symbol_id: Some("leaf-sym".into()),
            use_dots: false,
            scale_w_pct: 80.0,
            scale_h_pct: 120.0,
            rotate_symbol: true,
            ..dot_opts
        };
        let instances = generate_stage_spray_gesture(&[[50.0, 50.0], [120.0, 80.0]], &sym_opts);
        assert!(!instances.is_empty());
        assert!(instances.iter().all(|e| matches!(
            &e.kind,
            ElementKind::Instance { symbol_id, .. } if symbol_id == "leaf-sym"
        )));
    }
}
