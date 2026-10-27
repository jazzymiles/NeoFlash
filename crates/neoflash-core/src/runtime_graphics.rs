//! Bounded runtime drawing state. Paint batches are owned by one display object.
use crate::{Color, ElementKind, FillRule, PathCommand};
use serde::{Deserialize, Serialize};

pub const MAX_GRAPHICS_COMMANDS: usize = 100_000;
pub const MAX_GRAPHICS_OPS: usize = 4096;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphicsOp {
    Clear,
    BeginFill {
        color: u32,
        alpha: f64,
    },
    EndFill,
    LineStyle {
        width: Option<f64>,
        color: u32,
        alpha: f64,
    },
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    CurveTo {
        cx: f64,
        cy: f64,
        x: f64,
        y: f64,
    },
    DrawRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    DrawEllipse {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stroke {
    width: f64,
    color: Color,
}
#[derive(Clone, Debug)]
struct StrokeRun {
    style: Stroke,
    commands: Vec<PathCommand>,
    open: bool,
}
#[derive(Clone, Debug, Default)]
struct Group {
    fill: Option<Color>,
    commands: Vec<PathCommand>,
    open: bool,
    strokes: Vec<StrokeRun>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct GraphicsState {
    groups: Vec<Group>,
    stroke: Option<Stroke>,
    pen: [f64; 2],
    /// Counts stored commands and paint runs, including invisible pending paths.
    cost: usize,
}

fn color(value: u32, alpha: f64) -> Color {
    [
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
        (alpha * 255.).round() as u8,
    ]
}
fn valid_number(value: f64) -> bool {
    value.is_finite() && value.abs() <= 1e9
}

impl GraphicsOp {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        use GraphicsOp::*;
        let coordinates: Vec<f64> = match self {
            Clear | EndFill => vec![],
            BeginFill { color, alpha } | LineStyle { color, alpha, .. } => {
                if *color > 0xff_ffff || !alpha.is_finite() || !(0.0..=1.0).contains(alpha) {
                    return Err("Graphics color must be uint24 and alpha in 0..=1");
                }
                if let LineStyle {
                    width: Some(width), ..
                } = self
                    && (!width.is_finite() || *width <= 0. || *width > 255.)
                {
                    return Err(
                        "Graphics supports positive round strokes up to 255; hairlines are unsupported",
                    );
                }
                vec![]
            }
            MoveTo { x, y } | LineTo { x, y } => vec![*x, *y],
            CurveTo { cx, cy, x, y } => vec![*cx, *cy, *x, *y],
            DrawRect {
                x,
                y,
                width,
                height,
            }
            | DrawEllipse {
                x,
                y,
                width,
                height,
            } => {
                if *width < 0. || *height < 0. {
                    return Err("Graphics dimensions must be nonnegative");
                }
                vec![*x, *y, *width, *height, *x + *width, *y + *height]
            }
        };
        if coordinates.into_iter().all(valid_number) {
            Ok(())
        } else {
            Err("Graphics coordinates must be finite and within ±1e9")
        }
    }
    /// A conservative constant-time reservation prevents quadratic path validation.
    pub(crate) fn reservation(&self) -> usize {
        match self {
            Self::DrawEllipse { .. } => 18,
            Self::DrawRect { .. } => 16,
            _ => 6,
        }
    }
}

impl GraphicsState {
    pub(crate) fn cost(&self) -> usize {
        self.cost
    }
    fn group(&mut self) -> &mut Group {
        if self.groups.is_empty() {
            self.groups.push(Group::default());
            self.cost += 1;
        }
        self.groups.last_mut().expect("group created")
    }
    fn new_group(&mut self, fill: Option<Color>) {
        self.groups.push(Group {
            fill,
            ..Group::default()
        });
        self.cost += 1;
    }
    fn append(&mut self, commands: &[PathCommand], separate: bool) {
        let pen = self.pen;
        let stroke = self.stroke;
        let mut added = 0;
        let group = self.group();
        if group.fill.is_some() {
            if !separate && !group.open {
                group.commands.push(PathCommand::MoveTo {
                    x: pen[0],
                    y: pen[1],
                });
                added += 1;
            }
            group.commands.extend_from_slice(commands);
            added += commands.len();
            group.open = !separate;
        }
        if let Some(style) = stroke {
            if group.strokes.last().is_none_or(|run| run.style != style) {
                group.strokes.push(StrokeRun {
                    style,
                    commands: vec![],
                    open: false,
                });
                added += 1;
            }
            let run = group.strokes.last_mut().expect("stroke created");
            if !separate && !run.open {
                run.commands.push(PathCommand::MoveTo {
                    x: pen[0],
                    y: pen[1],
                });
                added += 1;
            }
            run.commands.extend_from_slice(commands);
            added += commands.len();
            run.open = !separate;
        }
        self.cost += added;
    }
    /// Caller validates/reserves the complete operation batch before invoking this.
    pub(crate) fn apply(&mut self, operation: &GraphicsOp) {
        use GraphicsOp::*;
        match *operation {
            Clear => *self = Self::default(),
            BeginFill {
                color: value,
                alpha,
            } => self.new_group(Some(color(value, alpha))),
            EndFill => self.new_group(None),
            LineStyle {
                width,
                color: value,
                alpha,
            } => {
                self.stroke = width.map(|width| Stroke {
                    width,
                    color: color(value, alpha),
                });
                // Even an identical style begins a new stroke run at the current pen.
                if let Some(group) = self.groups.last_mut()
                    && let Some(run) = group.strokes.last_mut()
                {
                    run.open = false;
                }
            }
            MoveTo { x, y } => {
                self.pen = [x, y];
                if let Some(group) = self.groups.last_mut() {
                    group.open = false;
                    if let Some(run) = group.strokes.last_mut() {
                        run.open = false;
                    }
                }
            }
            LineTo { x, y } => {
                self.append(&[PathCommand::LineTo { x, y }], false);
                self.pen = [x, y];
            }
            CurveTo { cx, cy, x, y } => {
                self.append(&[PathCommand::QuadraticTo { cx, cy, x, y }], false);
                self.pen = [x, y];
            }
            DrawRect {
                x,
                y,
                width,
                height,
            } => {
                self.append(
                    &[
                        PathCommand::MoveTo { x, y },
                        PathCommand::LineTo { x: x + width, y },
                        PathCommand::LineTo {
                            x: x + width,
                            y: y + height,
                        },
                        PathCommand::LineTo { x, y: y + height },
                        PathCommand::Close,
                    ],
                    true,
                );
            }
            DrawEllipse {
                x,
                y,
                width,
                height,
            } => {
                // Shared cubic approximation: the same path is rendered and picked
                // natively and in Canvas (no separate browser-only ellipse geometry).
                let k = 0.552_284_749_830_793_6;
                let rx = width / 2.;
                let ry = height / 2.;
                let cx = x + rx;
                let cy = y + ry;
                self.append(
                    &[
                        PathCommand::MoveTo {
                            x: x + width,
                            y: cy,
                        },
                        PathCommand::CubicTo {
                            cx1: x + width,
                            cy1: cy + k * ry,
                            cx2: cx + k * rx,
                            cy2: y + height,
                            x: cx,
                            y: y + height,
                        },
                        PathCommand::CubicTo {
                            cx1: cx - k * rx,
                            cy1: y + height,
                            cx2: x,
                            cy2: cy + k * ry,
                            x,
                            y: cy,
                        },
                        PathCommand::CubicTo {
                            cx1: x,
                            cy1: cy - k * ry,
                            cx2: cx - k * rx,
                            cy2: y,
                            x: cx,
                            y,
                        },
                        PathCommand::CubicTo {
                            cx1: cx + k * rx,
                            cy1: y,
                            cx2: x + width,
                            cy2: cy - k * ry,
                            x: x + width,
                            y: cy,
                        },
                        PathCommand::Close,
                    ],
                    true,
                );
            }
        }
    }
    pub(crate) fn paints(&self) -> Vec<ElementKind> {
        self.paints_with_color(crate::ColorTransform::default())
    }
    pub(crate) fn paints_with_color(&self, color: crate::ColorTransform) -> Vec<ElementKind> {
        let mut paints = vec![];
        for group in &self.groups {
            if let Some(fill) = group.fill
                && group.commands.len() > 1
            {
                paints.push(ElementKind::Path {
                    commands: group.commands.clone(),
                    fill_rule: FillRule::EvenOdd,
                    fill: color.apply(fill),
                    stroke: [0; 4],
                    stroke_width: 0.,
                });
            }
            for run in &group.strokes {
                if run.commands.len() > 1 {
                    paints.push(ElementKind::Path {
                        commands: run.commands.clone(),
                        fill_rule: FillRule::EvenOdd,
                        fill: [0; 4],
                        stroke: color.apply(run.style.color),
                        stroke_width: run.style.width,
                    });
                }
            }
        }
        paints
    }
}
