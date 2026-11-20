//! Advanced motion tools (`T-121`):
//! - Deterministic property expressions (`PropertyExpression`, `evaluate_property_expression`)
//! - Live Motion Sketch capture & Ramer-Douglas-Peucker / Laplacian smoothing (`MotionSketchSample`,
//!   `capture_motion_sketch_to_keyframes`, `smooth_motion_keyframes`)
//! - Keyframe Wiggler (`WigglerNoiseKind`, `WigglerDimension`, `WigglerConfig`, `apply_wiggler_to_keyframes`)
//! - Preview Work Area (`PreviewWorkArea`), rich Timeline Markers (`TimelineMarker`), and Stage A/B
//!   comparison snapshots (`StageSnapshotAB`).

use crate::{
    Easing, Element, Id, Keyframe, Layer, PathCommand, Timeline, Transform, TweenProperty,
    sample_layer_at_fps,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// 1. Deterministic Property Expressions
// ============================================================================

/// Side-effect-free procedural expression attached to a [`TweenProperty`] on a [`Keyframe`] (`T-121`, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyExpression {
    pub property: TweenProperty,
    pub expression: String,
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default = "default_amp")]
    pub amplitude: f64,
    #[serde(default = "default_freq")]
    pub frequency: f64,
    #[serde(default = "default_decay")]
    pub decay: f64,
    #[serde(default)]
    pub phase: f64,
    #[serde(default)]
    pub seed: u32,
}

const fn bool_true() -> bool {
    true
}
const fn default_amp() -> f64 {
    10.0
}
const fn default_freq() -> f64 {
    2.0
}
const fn default_decay() -> f64 {
    1.5
}

impl Default for PropertyExpression {
    fn default() -> Self {
        Self {
            property: TweenProperty::X,
            expression: "value".into(),
            enabled: true,
            amplitude: 10.0,
            frequency: 2.0,
            decay: 1.5,
            phase: 0.0,
            seed: 1,
        }
    }
}

fn hash_noise_u32(seed: u32, step: i64) -> f64 {
    let mut x = seed
        .wrapping_add((step as u32).wrapping_mul(0x9E37_79B9))
        .wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    ((x as f64) / (u32::MAX as f64)) * 2.0 - 1.0
}

/// Deterministic 1D smooth value noise in `-1.0..=1.0`.
pub fn deterministic_smooth_noise(seed: u32, t: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    let i0 = t.floor() as i64;
    let frac = t - (i0 as f64);
    let smooth = frac * frac * (3.0 - 2.0 * frac);
    let v0 = hash_noise_u32(seed, i0);
    let v1 = hash_noise_u32(seed, i0.wrapping_add(1));
    v0 + (v1 - v0) * smooth
}

const MAX_PROPERTY_EXPRESSION_BYTES: usize = 4_096;
const MAX_PROPERTY_EXPRESSION_TOKENS: usize = 512;
const MAX_PROPERTY_EXPRESSION_DEPTH: usize = 64;

#[derive(Clone, Debug)]
enum ExprToken {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    Comma,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExprParseError(String);

impl std::fmt::Display for ExprParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn expression_error(message: impl Into<String>) -> ExprParseError {
    ExprParseError(message.into())
}

fn push_expression_token(
    tokens: &mut Vec<ExprToken>,
    token: ExprToken,
) -> Result<(), ExprParseError> {
    if tokens.len() >= MAX_PROPERTY_EXPRESSION_TOKENS {
        return Err(expression_error(format!(
            "expression exceeds {MAX_PROPERTY_EXPRESSION_TOKENS} tokens"
        )));
    }
    tokens.push(token);
    Ok(())
}

fn tokenize_expression(src: &str) -> Result<Vec<ExprToken>, ExprParseError> {
    if src.len() > MAX_PROPERTY_EXPRESSION_BYTES {
        return Err(expression_error(format!(
            "expression exceeds {MAX_PROPERTY_EXPRESSION_BYTES} UTF-8 bytes"
        )));
    }

    let mut tokens = Vec::new();
    let mut chars = src.char_indices().peekable();
    while let Some((byte, ch)) = chars.next() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        match ch {
            '+' => push_expression_token(&mut tokens, ExprToken::Plus)?,
            '-' => push_expression_token(&mut tokens, ExprToken::Minus)?,
            '*' => push_expression_token(&mut tokens, ExprToken::Star)?,
            '/' => push_expression_token(&mut tokens, ExprToken::Slash)?,
            '%' => push_expression_token(&mut tokens, ExprToken::Percent)?,
            '(' => push_expression_token(&mut tokens, ExprToken::LParen)?,
            ')' => push_expression_token(&mut tokens, ExprToken::RParen)?,
            ',' => push_expression_token(&mut tokens, ExprToken::Comma)?,
            c if c.is_ascii_digit() || c == '.' => {
                let mut end = byte + c.len_utf8();
                let mut digit_count = usize::from(c.is_ascii_digit());
                let mut saw_dot = c == '.';
                while let Some(&(next_byte, next)) = chars.peek() {
                    if next.is_ascii_digit() {
                        chars.next();
                        digit_count += 1;
                        end = next_byte + next.len_utf8();
                    } else if next == '.' {
                        if saw_dot {
                            return Err(expression_error(format!(
                                "invalid number at byte {next_byte}"
                            )));
                        }
                        chars.next();
                        saw_dot = true;
                        end = next_byte + next.len_utf8();
                    } else {
                        break;
                    }
                }
                if digit_count == 0 {
                    return Err(expression_error(format!("invalid number at byte {byte}")));
                }
                let number = src[byte..end]
                    .parse::<f64>()
                    .map_err(|_| expression_error(format!("invalid number at byte {byte}")))?;
                if !number.is_finite() {
                    return Err(expression_error(format!("invalid number at byte {byte}")));
                }
                push_expression_token(&mut tokens, ExprToken::Num(number))?;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut end = byte + c.len_utf8();
                while let Some(&(next_byte, next)) = chars.peek() {
                    if next.is_ascii_alphanumeric() || next == '_' {
                        chars.next();
                        end = next_byte + next.len_utf8();
                    } else {
                        break;
                    }
                }
                push_expression_token(&mut tokens, ExprToken::Ident(src[byte..end].to_owned()))?;
            }
            _ => {
                return Err(expression_error(format!(
                    "unexpected character at byte {byte}"
                )));
            }
        }
    }
    Ok(tokens)
}

struct ExprEvalContext<'a> {
    expr: &'a PropertyExpression,
    base_value: f64,
    time_sec: f64,
    frame: u32,
    fps: f64,
    index: u32,
    mentions_value: bool,
}

struct ExprParser<'tokens, 'context, 'expression> {
    tokens: &'tokens [ExprToken],
    pos: usize,
    depth: usize,
    ctx: Option<&'context ExprEvalContext<'expression>>,
}

impl<'tokens, 'context, 'expression> ExprParser<'tokens, 'context, 'expression> {
    fn peek(&self) -> Option<&'tokens ExprToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&'tokens ExprToken> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn parse_complete(&mut self) -> Result<f64, ExprParseError> {
        let value = self.parse_expr()?;
        if self.pos != self.tokens.len() {
            return Err(expression_error(format!(
                "unexpected trailing token at token {}",
                self.pos + 1
            )));
        }
        Ok(value)
    }

    fn parse_expr(&mut self) -> Result<f64, ExprParseError> {
        let mut val = self.parse_term()?;
        while let Some(tok) = self.peek() {
            match tok {
                ExprToken::Plus => {
                    self.next();
                    val += self.parse_term()?;
                }
                ExprToken::Minus => {
                    self.next();
                    val -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(val)
    }

    fn parse_term(&mut self) -> Result<f64, ExprParseError> {
        let mut val = self.parse_unary()?;
        while let Some(tok) = self.peek() {
            match tok {
                ExprToken::Star => {
                    self.next();
                    val *= self.parse_unary()?;
                }
                ExprToken::Slash => {
                    self.next();
                    let denom = self.parse_unary()?;
                    if denom.abs() > 1e-12 {
                        val /= denom;
                    }
                }
                ExprToken::Percent => {
                    self.next();
                    let denom = self.parse_unary()?;
                    if denom.abs() > 1e-12 {
                        val %= denom;
                    }
                }
                _ => break,
            }
        }
        Ok(val)
    }

    fn parse_unary(&mut self) -> Result<f64, ExprParseError> {
        let mut negative = false;
        loop {
            match self.peek() {
                Some(ExprToken::Plus) => {
                    self.next();
                }
                Some(ExprToken::Minus) => {
                    self.next();
                    negative = !negative;
                }
                _ => break,
            }
        }
        let value = self.parse_primary()?;
        Ok(if negative { -value } else { value })
    }

    fn enter_nested(&mut self) -> Result<(), ExprParseError> {
        if self.depth >= MAX_PROPERTY_EXPRESSION_DEPTH {
            return Err(expression_error(format!(
                "expression nesting exceeds {MAX_PROPERTY_EXPRESSION_DEPTH}"
            )));
        }
        self.depth += 1;
        Ok(())
    }

    fn parse_group(&mut self) -> Result<f64, ExprParseError> {
        self.enter_nested()?;
        let result = (|| {
            let value = self.parse_expr()?;
            if !matches!(self.peek(), Some(ExprToken::RParen)) {
                return Err(expression_error("expected ')'"));
            }
            self.next();
            Ok(value)
        })();
        self.depth -= 1;
        result
    }

    fn parse_call(&mut self, function: &str) -> Result<f64, ExprParseError> {
        self.enter_nested()?;
        let result = (|| {
            let mut args = Vec::new();
            if matches!(self.peek(), Some(ExprToken::RParen)) {
                self.next();
                return Ok(self.eval_call(function, &args));
            }
            if self.peek().is_none() {
                return Err(expression_error("expected ')'"));
            }

            loop {
                args.push(self.parse_expr()?);
                match self.peek() {
                    Some(ExprToken::Comma) => {
                        self.next();
                    }
                    Some(ExprToken::RParen) => {
                        self.next();
                        break;
                    }
                    None => return Err(expression_error("expected ')'")),
                    Some(_) => return Err(expression_error("expected ',' or ')'")),
                }
            }
            Ok(self.eval_call(function, &args))
        })();
        self.depth -= 1;
        result
    }

    fn parse_primary(&mut self) -> Result<f64, ExprParseError> {
        let Some(tok) = self.next() else {
            return Err(expression_error("expected expression"));
        };
        match tok {
            ExprToken::Num(number) => Ok(*number),
            ExprToken::LParen => self.parse_group(),
            ExprToken::Ident(name) => {
                let lower = name.to_ascii_lowercase();
                if matches!(self.peek(), Some(ExprToken::LParen)) {
                    self.next();
                    self.parse_call(&lower)
                } else {
                    Ok(self.eval_identifier(&lower))
                }
            }
            _ => Err(expression_error("expected expression")),
        }
    }

    fn eval_identifier(&self, identifier: &str) -> f64 {
        let Some(ctx) = self.ctx else {
            return 0.0;
        };
        match identifier {
            "value" | "base" => ctx.base_value,
            "time" | "t" => ctx.time_sec,
            "frame" => ctx.frame as f64,
            "fps" => ctx.fps,
            "index" => ctx.index as f64,
            "amp" | "amplitude" => ctx.expr.amplitude,
            "freq" | "frequency" => ctx.expr.frequency,
            "decay" => ctx.expr.decay,
            "phase" => ctx.expr.phase,
            "pi" => std::f64::consts::PI,
            "tau" => std::f64::consts::TAU,
            _ => 0.0,
        }
    }

    fn eval_call(&self, func: &str, args: &[f64]) -> f64 {
        let Some(ctx) = self.ctx else {
            return 0.0;
        };
        let a0 = args.first().copied().unwrap_or(0.0);
        let a1 = args.get(1).copied().unwrap_or(0.0);
        let a2 = args.get(2).copied().unwrap_or(0.0);
        let seed = ctx.expr.seed.wrapping_add(ctx.index.wrapping_mul(31));
        match func {
            "sin" => a0.sin(),
            "cos" => a0.cos(),
            "tan" => a0.tan(),
            "abs" => a0.abs(),
            "sqrt" => a0.max(0.0).sqrt(),
            "floor" => a0.floor(),
            "ceil" => a0.ceil(),
            "round" => a0.round(),
            "min" => a0.min(a1),
            "max" => a0.max(a1),
            "clamp" => {
                if [a0, a1, a2].iter().all(|value| value.is_finite()) {
                    a0.clamp(a1, a2.max(a1))
                } else {
                    f64::NAN
                }
            }
            "noise" => deterministic_smooth_noise(seed, a0),
            "wiggle" => {
                let freq = if args.is_empty() {
                    ctx.expr.frequency
                } else {
                    a0
                };
                let amp = if args.len() < 2 {
                    ctx.expr.amplitude
                } else {
                    a1
                };
                let offset =
                    deterministic_smooth_noise(seed, ctx.time_sec * freq + ctx.expr.phase) * amp;
                if ctx.mentions_value {
                    offset
                } else {
                    ctx.base_value + offset
                }
            }
            "spring" => {
                let freq = if args.is_empty() {
                    ctx.expr.frequency
                } else {
                    a0
                };
                let decay = if args.len() < 2 { ctx.expr.decay } else { a1 };
                let amp = args.get(2).copied().unwrap_or(ctx.expr.amplitude);
                let offset = amp
                    * (-decay.max(0.0) * ctx.time_sec).exp()
                    * (ctx.time_sec * freq * std::f64::consts::TAU + ctx.expr.phase).sin();
                if ctx.mentions_value {
                    offset
                } else {
                    ctx.base_value + offset
                }
            }
            "bounce" => {
                let amp = if args.is_empty() {
                    ctx.expr.amplitude
                } else {
                    a0
                };
                let decay = if args.len() < 2 { ctx.expr.decay } else { a1 };
                let freq = args.get(2).copied().unwrap_or(ctx.expr.frequency);
                let offset = amp
                    * (-decay.max(0.0) * ctx.time_sec).exp()
                    * (ctx.time_sec * freq * std::f64::consts::PI + ctx.expr.phase)
                        .sin()
                        .abs();
                if ctx.mentions_value {
                    offset
                } else {
                    ctx.base_value + offset
                }
            }
            "saw" => {
                let freq = if args.is_empty() {
                    ctx.expr.frequency
                } else {
                    a0
                };
                let amp = if args.len() < 2 {
                    ctx.expr.amplitude
                } else {
                    a1
                };
                let offset = ((ctx.time_sec * freq + ctx.expr.phase).fract() * 2.0 - 1.0) * amp;
                if ctx.mentions_value {
                    offset
                } else {
                    ctx.base_value + offset
                }
            }
            _ => 0.0,
        }
    }
}

pub(crate) fn validate_property_expression_syntax(source: &str) -> Result<(), String> {
    let tokens = tokenize_expression(source).map_err(|error| error.to_string())?;
    let mut parser = ExprParser {
        tokens: &tokens,
        pos: 0,
        depth: 0,
        ctx: None,
    };
    parser
        .parse_complete()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Evaluate a deterministic [`PropertyExpression`] for `base_value` at `(time_sec, frame, fps, index)` (`T-121`).
pub fn evaluate_property_expression(
    expr: &PropertyExpression,
    base_value: f64,
    time_sec: f64,
    frame: u32,
    fps: f64,
    index: u32,
) -> f64 {
    if !expr.enabled || expr.expression.trim().is_empty() {
        return base_value;
    }
    let Ok(tokens) = tokenize_expression(&expr.expression) else {
        return base_value;
    };
    let mentions_value = tokens.iter().any(|t| {
        matches!(
            t,
            ExprToken::Ident(s) if s.eq_ignore_ascii_case("value") || s.eq_ignore_ascii_case("base")
        )
    });
    let ctx = ExprEvalContext {
        expr,
        base_value,
        time_sec,
        frame,
        fps: fps.max(1.0),
        index,
        mentions_value,
    };
    let mut parser = ExprParser {
        tokens: &tokens,
        pos: 0,
        depth: 0,
        ctx: Some(&ctx),
    };
    match parser.parse_complete() {
        Ok(out) if out.is_finite() => out,
        Ok(_) | Err(_) => base_value,
    }
}

/// Apply all enabled [`PropertyExpression`]s on a keyframe to `transform` at `frame` (`T-121`).
pub fn apply_keyframe_property_expressions(
    transform: &mut Transform,
    expressions: &[PropertyExpression],
    frame: u32,
    fps: f64,
    index: u32,
) {
    if expressions.is_empty() {
        return;
    }
    let eff_fps = fps.max(1.0);
    let time_sec = (frame as f64) / eff_fps;
    for expr in expressions {
        if !expr.enabled {
            continue;
        }
        match expr.property {
            TweenProperty::X => {
                transform.x = evaluate_property_expression(
                    expr,
                    transform.x,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                );
            }
            TweenProperty::Y => {
                transform.y = evaluate_property_expression(
                    expr,
                    transform.y,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                );
            }
            TweenProperty::ScaleX => {
                transform.scale_x = evaluate_property_expression(
                    expr,
                    transform.scale_x,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                )
                .clamp(-1000.0, 1000.0);
            }
            TweenProperty::ScaleY => {
                transform.scale_y = evaluate_property_expression(
                    expr,
                    transform.scale_y,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                )
                .clamp(-1000.0, 1000.0);
            }
            TweenProperty::Rotation => {
                transform.rotation = evaluate_property_expression(
                    expr,
                    transform.rotation,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                );
            }
            TweenProperty::Skew => {
                transform.skew = evaluate_property_expression(
                    expr,
                    transform.skew,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                );
            }
            TweenProperty::Alpha => {
                transform.alpha = evaluate_property_expression(
                    expr,
                    transform.alpha,
                    time_sec,
                    frame,
                    eff_fps,
                    index,
                )
                .clamp(0.0, 1.0);
            }
            TweenProperty::Color => {}
        }
    }
}

// ============================================================================
// 2. Motion Sketch Capture & Smoothing
// ============================================================================

/// One pointer sample recorded during live Motion Sketch capture (`T-121`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionSketchSample {
    pub time_sec: f64,
    pub position: [f64; 2],
}

fn perpendicular_distance_frame_pos(
    pt: (u32, [f64; 2]),
    start: (u32, [f64; 2]),
    end: (u32, [f64; 2]),
) -> f64 {
    let dt = (end.0 as f64) - (start.0 as f64);
    if dt.abs() <= 1e-9 {
        return (pt.1[0] - start.1[0]).hypot(pt.1[1] - start.1[1]);
    }
    let u = (((pt.0 as f64) - (start.0 as f64)) / dt).clamp(0.0, 1.0);
    let interp_x = start.1[0] + (end.1[0] - start.1[0]) * u;
    let interp_y = start.1[1] + (end.1[1] - start.1[1]) * u;
    (pt.1[0] - interp_x).hypot(pt.1[1] - interp_y)
}

fn rdp_simplify_keyframes(points: &[(u32, [f64; 2])], tolerance: f64) -> Vec<(u32, [f64; 2])> {
    if points.len() <= 2 || tolerance <= 0.0 {
        return points.to_vec();
    }
    let first = points[0];
    let last = *points.last().unwrap();
    let mut max_dist = 0.0;
    let mut split_idx = 0usize;
    for (i, &pt) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = perpendicular_distance_frame_pos(pt, first, last);
        if d > max_dist {
            max_dist = d;
            split_idx = i;
        }
    }
    if max_dist > tolerance && split_idx > 0 {
        let mut left = rdp_simplify_keyframes(&points[..=split_idx], tolerance);
        let right = rdp_simplify_keyframes(&points[split_idx..], tolerance);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

/// Convert live [`MotionSketchSample`] pointer samples into quantized `(frame, position)` keyframes
/// starting at `start_frame` with optional Ramer-Douglas-Peucker simplification (`T-121`).
pub fn capture_motion_sketch_to_keyframes(
    samples: &[MotionSketchSample],
    fps: f64,
    start_frame: u32,
    smoothing_tolerance: f64,
) -> Vec<(u32, [f64; 2])> {
    if samples.is_empty() {
        return Vec::new();
    }
    let eff_fps = fps.max(1.0);
    let t0 = samples[0].time_sec;
    let mut by_frame: BTreeMap<u32, [f64; 2]> = BTreeMap::new();
    for s in samples {
        let rel_t = (s.time_sec - t0).max(0.0);
        let frame = start_frame.saturating_add((rel_t * eff_fps).round() as u32);
        by_frame.insert(frame, s.position);
    }
    let quantized: Vec<(u32, [f64; 2])> = by_frame.into_iter().collect();
    rdp_simplify_keyframes(&quantized, smoothing_tolerance.max(0.0))
}

/// Smooth interior `(frame, position)` keyframes using Laplacian averaging controlled by
/// `strength` in `0.0..=1.0` while preserving start and end keyframes (`T-121`).
pub fn smooth_motion_keyframes(points: &[(u32, [f64; 2])], strength: f64) -> Vec<(u32, [f64; 2])> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let alpha = strength.clamp(0.0, 1.0);
    let mut out = points.to_vec();
    for i in 1..points.len() - 1 {
        let prev = points[i - 1].1;
        let cur = points[i].1;
        let next = points[i + 1].1;
        let avg_x = (prev[0] + cur[0] * 2.0 + next[0]) * 0.25;
        let avg_y = (prev[1] + cur[1] * 2.0 + next[1]) * 0.25;
        out[i].1 = [
            cur[0] + (avg_x - cur[0]) * alpha,
            cur[1] + (avg_y - cur[1]) * alpha,
        ];
    }
    out
}

// ============================================================================
// 3. Wiggler
// ============================================================================

/// Noise waveform style for the Wiggler tool (`T-121`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WigglerNoiseKind {
    #[default]
    Smooth,
    Jagged,
}

/// Spatial dimension coupling for Wiggler position/scale perturbations (`T-121`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WigglerDimension {
    #[default]
    AllIndependently,
    AllSame,
    HorizontalOnly,
    VerticalOnly,
}

/// Configuration for the keyframe Wiggler (`T-121`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WigglerConfig {
    pub property: TweenProperty,
    #[serde(default)]
    pub noise_kind: WigglerNoiseKind,
    #[serde(default)]
    pub dimension: WigglerDimension,
    pub frequency_hz: f64,
    pub magnitude: f64,
    pub seed: u32,
}

impl Default for WigglerConfig {
    fn default() -> Self {
        Self {
            property: TweenProperty::X,
            noise_kind: WigglerNoiseKind::Smooth,
            dimension: WigglerDimension::AllIndependently,
            frequency_hz: 3.0,
            magnitude: 12.0,
            seed: 42,
        }
    }
}

fn sample_wiggler_noise(kind: WigglerNoiseKind, seed: u32, phase: f64) -> f64 {
    match kind {
        WigglerNoiseKind::Smooth => deterministic_smooth_noise(seed, phase),
        WigglerNoiseKind::Jagged => hash_noise_u32(seed, (phase * 4.0).round() as i64),
    }
}

/// Apply the Wiggler across `start_frame..=end_frame` on `layer` for `target_id`, baking
/// perturbed keyframes and returning the number of keyframes updated/created (`T-121`).
pub fn apply_wiggler_to_keyframes(
    layer: &mut Layer,
    target_id: &str,
    start_frame: u32,
    end_frame: u32,
    fps: f64,
    config: &WigglerConfig,
) -> usize {
    if end_frame < start_frame {
        return 0;
    }
    let eff_fps = fps.max(1.0);
    let mut sampled_frames = Vec::new();
    for f in start_frame..=end_frame {
        let mut elements = sample_layer_at_fps(layer, f, eff_fps);
        let Some(el) = elements.iter_mut().find(|e| e.id == target_id) else {
            continue;
        };
        let phase = ((f - start_frame) as f64 / eff_fps) * config.frequency_hz.max(0.1);
        let n1 = sample_wiggler_noise(config.noise_kind, config.seed, phase) * config.magnitude;
        let n2 = match config.dimension {
            WigglerDimension::AllSame => n1,
            _ => {
                sample_wiggler_noise(config.noise_kind, config.seed.wrapping_add(97), phase)
                    * config.magnitude
            }
        };
        match config.property {
            TweenProperty::X | TweenProperty::Y => match config.dimension {
                WigglerDimension::AllIndependently | WigglerDimension::AllSame => {
                    el.transform.x += n1;
                    el.transform.y += n2;
                }
                WigglerDimension::HorizontalOnly => {
                    el.transform.x += n1;
                }
                WigglerDimension::VerticalOnly => {
                    el.transform.y += n2;
                }
            },
            TweenProperty::ScaleX | TweenProperty::ScaleY => match config.dimension {
                WigglerDimension::AllIndependently | WigglerDimension::AllSame => {
                    el.transform.scale_x = (el.transform.scale_x + n1 * 0.01).max(0.01);
                    el.transform.scale_y = (el.transform.scale_y + n2 * 0.01).max(0.01);
                }
                WigglerDimension::HorizontalOnly => {
                    el.transform.scale_x = (el.transform.scale_x + n1 * 0.01).max(0.01);
                }
                WigglerDimension::VerticalOnly => {
                    el.transform.scale_y = (el.transform.scale_y + n2 * 0.01).max(0.01);
                }
            },
            TweenProperty::Rotation => {
                el.transform.rotation += n1;
            }
            TweenProperty::Skew => {
                el.transform.skew += n1;
            }
            TweenProperty::Alpha => {
                el.transform.alpha = (el.transform.alpha + n1 * 0.01).clamp(0.0, 1.0);
            }
            TweenProperty::Color => {}
        }
        sampled_frames.push((f, elements));
    }

    let mut count = 0usize;
    for (f, elements) in sampled_frames {
        if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == f) {
            existing.elements = elements;
        } else {
            layer.keyframes.push(Keyframe {
                frame: f,
                elements,
                ..Default::default()
            });
        }
        count += 1;
    }
    layer.normalize_motion_keys();
    count
}

// ============================================================================
// 4. Preview Work Area, Timeline Markers & Stage Snapshot A/B
// ============================================================================

/// Preview Work Area bounds (`in_frame..=out_frame`) and frame-skip cadence (`T-121`, format 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewWorkArea {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    pub in_frame: u32,
    pub out_frame: u32,
    #[serde(default)]
    pub skip_nth_frame: u32,
}

impl Default for PreviewWorkArea {
    fn default() -> Self {
        Self {
            enabled: true,
            in_frame: 0,
            out_frame: 24,
            skip_nth_frame: 0,
        }
    }
}

impl PreviewWorkArea {
    /// Step from `current_frame` to the next preview frame inside the work area.
    pub fn next_frame(&self, current_frame: u32, loop_playback: bool) -> u32 {
        let step = self.skip_nth_frame.saturating_add(1);
        let lo = self.in_frame;
        let hi = self.out_frame.max(lo);
        if current_frame < lo {
            return lo;
        }
        let candidate = current_frame.saturating_add(step);
        if candidate > hi {
            if loop_playback { lo } else { hi }
        } else {
            candidate
        }
    }
}

/// Rich timeline span marker with comment, colour tag, and cue-point parameters (`T-121`, format 9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineMarker {
    pub id: Id,
    pub frame: u32,
    #[serde(default)]
    pub duration_frames: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_tag: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cue_point_parameters: BTreeMap<String, String>,
}

/// Authoring Stage A/B comparison snapshot (`T-121`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageSnapshotAB {
    pub label: String,
    pub frame: u32,
    pub elements: Vec<Element>,
}

impl Timeline {
    /// Active [`PreviewWorkArea`] stored on this timeline (`T-121`, format 9).
    pub fn work_area(&self) -> Option<PreviewWorkArea> {
        self.layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find_map(|k| k.work_area)
    }

    /// Set or clear the [`PreviewWorkArea`] on this timeline (`T-121`, format 9).
    pub fn set_work_area(&mut self, work_area: Option<PreviewWorkArea>) {
        for layer in &mut self.layers {
            for key in &mut layer.keyframes {
                key.work_area = None;
            }
        }
        if let Some(first_layer) = self.layers.first_mut()
            && let Some(first_key) = first_layer.keyframes.first_mut()
        {
            first_key.work_area = work_area;
        }
    }

    /// Ordered [`TimelineMarker`]s stored on this timeline (`T-121`, format 9).
    pub fn markers(&self) -> Vec<TimelineMarker> {
        let mut all: Vec<TimelineMarker> = self
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| k.markers.iter().cloned())
            .collect();
        all.sort_by_key(|m| m.frame);
        all
    }

    /// Replace all [`TimelineMarker`]s on this timeline (`T-121`, format 9).
    pub fn set_markers(&mut self, mut markers: Vec<TimelineMarker>) {
        markers.sort_by_key(|m| m.frame);
        for layer in &mut self.layers {
            for key in &mut layer.keyframes {
                key.markers.clear();
            }
        }
        if let Some(first_layer) = self.layers.first_mut()
            && let Some(first_key) = first_layer.keyframes.first_mut()
        {
            first_key.markers = markers;
        }
    }
}

// ============================================================================
// 5. Exact Easing Time-Reversal & De Casteljau Motion-Path Geometry (`T-399`)
// ============================================================================

/// Invert an [`Easing`] curve in time so that `easing(u, reverse_easing(e)) == 1.0 - easing(1.0 - u, e)`
/// for every invertible variant (`Linear`, `EaseIn`, `EaseOut`, `EaseInOut`, `Flash`, `CubicBezier`).
pub fn reverse_easing(ease: Easing) -> Easing {
    match ease {
        Easing::Linear => Easing::Linear,
        Easing::EaseIn => Easing::EaseOut,
        Easing::EaseOut => Easing::EaseIn,
        Easing::EaseInOut => Easing::EaseInOut,
        Easing::Flash { acceleration } => Easing::Flash {
            acceleration: -acceleration,
        },
        Easing::CubicBezier { x1, y1, x2, y2 } => Easing::CubicBezier {
            x1: (1.0 - x2).clamp(0.0, 1.0),
            y1: 1.0 - y2,
            x2: (1.0 - x1).clamp(0.0, 1.0),
            y2: 1.0 - y1,
        },
        Easing::Bounce | Easing::Elastic => ease,
    }
}

#[inline]
fn lerp_pt(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn command_endpoint(command: PathCommand) -> Option<[f64; 2]> {
    let pt = match command {
        PathCommand::MoveTo { x, y }
        | PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => [x, y],
        PathCommand::Close => return None,
    };
    (pt[0].is_finite() && pt[1].is_finite()).then_some(pt)
}

fn command_controls_finite(command: PathCommand) -> bool {
    match command {
        PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
            x.is_finite() && y.is_finite()
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            cx.is_finite() && cy.is_finite() && x.is_finite() && y.is_finite()
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => [cx1, cy1, cx2, cy2, x, y].iter().all(|v| v.is_finite()),
        PathCommand::Close => false,
    }
}

fn eval_bezier_segment(p0: [f64; 2], command: PathCommand, u: f64) -> [f64; 2] {
    let u = u.clamp(0.0, 1.0);
    let inv = 1.0 - u;
    match command {
        PathCommand::LineTo { x, y } => lerp_pt(p0, [x, y], u),
        PathCommand::QuadraticTo { cx, cy, x, y } => [
            inv * inv * p0[0] + 2.0 * inv * u * cx + u * u * x,
            inv * inv * p0[1] + 2.0 * inv * u * cy + u * u * y,
        ],
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let i2 = inv * inv;
            let u2 = u * u;
            [
                i2 * inv * p0[0] + 3.0 * i2 * u * cx1 + 3.0 * inv * u2 * cx2 + u2 * u * x,
                i2 * inv * p0[1] + 3.0 * i2 * u * cy1 + 3.0 * inv * u2 * cy2 + u2 * u * y,
            ]
        }
        PathCommand::MoveTo { x, y } => [x, y],
        PathCommand::Close => p0,
    }
}

fn deriv_bezier_segment(p0: [f64; 2], command: PathCommand, u: f64) -> [f64; 2] {
    let u = u.clamp(0.0, 1.0);
    let inv = 1.0 - u;
    match command {
        PathCommand::LineTo { x, y } => [x - p0[0], y - p0[1]],
        PathCommand::QuadraticTo { cx, cy, x, y } => [
            2.0 * inv * (cx - p0[0]) + 2.0 * u * (x - cx),
            2.0 * inv * (cy - p0[1]) + 2.0 * u * (y - cy),
        ],
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let i2 = inv * inv;
            let u2 = u * u;
            [
                3.0 * i2 * (cx1 - p0[0]) + 6.0 * inv * u * (cx2 - cx1) + 3.0 * u2 * (x - cx2),
                3.0 * i2 * (cy1 - p0[1]) + 6.0 * inv * u * (cy2 - cy1) + 3.0 * u2 * (y - cy2),
            ]
        }
        PathCommand::MoveTo { .. } | PathCommand::Close => [0.0, 0.0],
    }
}

/// Reverse an open `MoveTo` + (`LineTo` / `QuadraticTo` / `CubicTo`) path exactly without baking
/// or flattening: reverses anchor order, swaps `CubicTo` `(cx1, cy1) <-> (cx2, cy2)`, and keeps
/// `QuadraticTo` `(cx, cy)`.
pub fn reverse_path_commands(commands: &[PathCommand]) -> Option<Vec<PathCommand>> {
    if commands.len() < 2 {
        return None;
    }
    let PathCommand::MoveTo { x: x0, y: y0 } = commands[0] else {
        return None;
    };
    if !x0.is_finite() || !y0.is_finite() {
        return None;
    }
    let mut anchors = Vec::with_capacity(commands.len());
    anchors.push([x0, y0]);
    for &cmd in &commands[1..] {
        if matches!(cmd, PathCommand::MoveTo { .. } | PathCommand::Close)
            || !command_controls_finite(cmd)
        {
            return None;
        }
        anchors.push(command_endpoint(cmd)?);
    }
    let last = *anchors.last()?;
    let mut out = Vec::with_capacity(commands.len());
    out.push(PathCommand::MoveTo {
        x: last[0],
        y: last[1],
    });
    for (idx, &cmd) in commands[1..].iter().enumerate().rev() {
        let prev = anchors[idx];
        let rev_cmd = match cmd {
            PathCommand::LineTo { .. } => PathCommand::LineTo {
                x: prev[0],
                y: prev[1],
            },
            PathCommand::QuadraticTo { cx, cy, .. } => PathCommand::QuadraticTo {
                cx,
                cy,
                x: prev[0],
                y: prev[1],
            },
            PathCommand::CubicTo {
                cx1, cy1, cx2, cy2, ..
            } => PathCommand::CubicTo {
                cx1: cx2,
                cy1: cy2,
                cx2: cx1,
                cy2: cy1,
                x: prev[0],
                y: prev[1],
            },
            PathCommand::MoveTo { .. } | PathCommand::Close => return None,
        };
        out.push(rev_cmd);
    }
    Some(out)
}

/// Exact De Casteljau sub-segment extraction on parameter interval `[u0, u1]` (`0 <= u0 <= u1 <= 1`)
/// for a single `LineTo`, `QuadraticTo`, or `CubicTo` segment starting at `p0`.
pub fn subdivide_bezier_segment(
    p0: [f64; 2],
    command: PathCommand,
    u0: f64,
    u1: f64,
) -> Option<([f64; 2], PathCommand)> {
    if !p0[0].is_finite()
        || !p0[1].is_finite()
        || !u0.is_finite()
        || !u1.is_finite()
        || !command_controls_finite(command)
    {
        return None;
    }
    let u0 = u0.clamp(0.0, 1.0);
    let u1 = u1.clamp(0.0, 1.0);
    if u1 + 1e-15 < u0 {
        return None;
    }
    if u0 == 0.0 && u1 == 1.0 {
        return match command {
            PathCommand::LineTo { .. }
            | PathCommand::QuadraticTo { .. }
            | PathCommand::CubicTo { .. } => Some((p0, command)),
            PathCommand::MoveTo { .. } | PathCommand::Close => None,
        };
    }
    if u1 - u0 <= 1e-15 {
        let pt = eval_bezier_segment(p0, command, u0);
        let deg = match command {
            PathCommand::LineTo { .. } => PathCommand::LineTo { x: pt[0], y: pt[1] },
            PathCommand::QuadraticTo { .. } => PathCommand::QuadraticTo {
                cx: pt[0],
                cy: pt[1],
                x: pt[0],
                y: pt[1],
            },
            PathCommand::CubicTo { .. } => PathCommand::CubicTo {
                cx1: pt[0],
                cy1: pt[1],
                cx2: pt[0],
                cy2: pt[1],
                x: pt[0],
                y: pt[1],
            },
            PathCommand::MoveTo { .. } | PathCommand::Close => return None,
        };
        return Some((pt, deg));
    }
    let denom = 1.0 - u0;
    let w = if denom > 1e-15 {
        ((u1 - u0) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    match command {
        PathCommand::LineTo { x, y } => {
            let p1 = [x, y];
            let q0 = lerp_pt(p0, p1, u0);
            let q1 = lerp_pt(p0, p1, u1);
            Some((q0, PathCommand::LineTo { x: q1[0], y: q1[1] }))
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            let c = [cx, cy];
            let p2 = [x, y];
            let a01 = lerp_pt(p0, c, u0);
            let a12 = lerp_pt(c, p2, u0);
            let r0 = lerp_pt(a01, a12, u0);
            let b01 = lerp_pt(r0, a12, w);
            let b12 = lerp_pt(a12, p2, w);
            let q2 = lerp_pt(b01, b12, w);
            Some((
                r0,
                PathCommand::QuadraticTo {
                    cx: b01[0],
                    cy: b01[1],
                    x: q2[0],
                    y: q2[1],
                },
            ))
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let c1 = [cx1, cy1];
            let c2 = [cx2, cy2];
            let p3 = [x, y];
            let a01 = lerp_pt(p0, c1, u0);
            let a12 = lerp_pt(c1, c2, u0);
            let a23 = lerp_pt(c2, p3, u0);
            let a012 = lerp_pt(a01, a12, u0);
            let a123 = lerp_pt(a12, a23, u0);
            let r0 = lerp_pt(a012, a123, u0);
            let b01 = lerp_pt(r0, a123, w);
            let b12 = lerp_pt(a123, a23, w);
            let b23 = lerp_pt(a23, p3, w);
            let b012 = lerp_pt(b01, b12, w);
            let b123 = lerp_pt(b12, b23, w);
            let q3 = lerp_pt(b012, b123, w);
            Some((
                r0,
                PathCommand::CubicTo {
                    cx1: b01[0],
                    cy1: b01[1],
                    cx2: b012[0],
                    cy2: b012[1],
                    x: q3[0],
                    y: q3[1],
                },
            ))
        }
        PathCommand::MoveTo { .. } | PathCommand::Close => None,
    }
}

const GL8_HALF_NODES: [f64; 4] = [
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_3,
];
const GL8_HALF_WEIGHTS: [f64; 4] = [
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];
const ARC_SUB_INTERVALS: usize = 16;

fn bezier_speed(p0: [f64; 2], cmd: PathCommand, u: f64) -> f64 {
    let d = deriv_bezier_segment(p0, cmd, u);
    d[0].hypot(d[1])
}

fn integrate_segment_gl8(p0: [f64; 2], cmd: PathCommand, a: f64, b: f64) -> f64 {
    let span = b - a;
    if span <= 0.0 {
        return 0.0;
    }
    let mid = 0.5 * (a + b);
    let half = 0.5 * span;
    let mut sum = 0.0;
    for (&node, &weight) in GL8_HALF_NODES.iter().zip(&GL8_HALF_WEIGHTS) {
        let dl = bezier_speed(p0, cmd, mid - half * node);
        let dr = bezier_speed(p0, cmd, mid + half * node);
        sum += weight * (dl + dr);
    }
    half * sum
}

#[derive(Clone, Debug)]
struct ExactSegment {
    p0: [f64; 2],
    cmd: PathCommand,
    sub_lengths: [f64; ARC_SUB_INTERVALS + 1],
    length: f64,
}

impl ExactSegment {
    fn new(p0: [f64; 2], cmd: PathCommand) -> Option<Self> {
        if !p0[0].is_finite() || !p0[1].is_finite() || !command_controls_finite(cmd) {
            return None;
        }
        match cmd {
            PathCommand::LineTo { x, y } => {
                let len = (x - p0[0]).hypot(y - p0[1]);
                let mut sub_lengths = [0.0; ARC_SUB_INTERVALS + 1];
                for (i, slot) in sub_lengths.iter_mut().enumerate() {
                    *slot = len * (i as f64 / ARC_SUB_INTERVALS as f64);
                }
                Some(Self {
                    p0,
                    cmd,
                    sub_lengths,
                    length: len,
                })
            }
            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. } => {
                let mut sub_lengths = [0.0; ARC_SUB_INTERVALS + 1];
                let mut acc = 0.0;
                for i in 0..ARC_SUB_INTERVALS {
                    let a = i as f64 / ARC_SUB_INTERVALS as f64;
                    let b = (i + 1) as f64 / ARC_SUB_INTERVALS as f64;
                    acc += integrate_segment_gl8(p0, cmd, a, b);
                    sub_lengths[i + 1] = acc;
                }
                Some(Self {
                    p0,
                    cmd,
                    sub_lengths,
                    length: acc,
                })
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => None,
        }
    }

    fn arc_length_to(&self, u: f64) -> f64 {
        if u <= 0.0 {
            return 0.0;
        }
        if u >= 1.0 {
            return self.length;
        }
        if matches!(self.cmd, PathCommand::LineTo { .. }) {
            return self.length * u;
        }
        let scaled = u * ARC_SUB_INTERVALS as f64;
        let idx = (scaled.floor() as usize).min(ARC_SUB_INTERVALS - 1);
        let base_u = idx as f64 / ARC_SUB_INTERVALS as f64;
        self.sub_lengths[idx] + integrate_segment_gl8(self.p0, self.cmd, base_u, u)
    }

    fn solve_u_for_arc_length(&self, target_s: f64) -> f64 {
        if self.length <= 1e-15 || target_s <= 0.0 {
            return 0.0;
        }
        if target_s >= self.length {
            return 1.0;
        }
        if matches!(self.cmd, PathCommand::LineTo { .. }) {
            return (target_s / self.length).clamp(0.0, 1.0);
        }
        let m = self
            .sub_lengths
            .partition_point(|&s| s < target_s)
            .saturating_sub(1)
            .min(ARC_SUB_INTERVALS - 1);
        let mut low = m as f64 / ARC_SUB_INTERVALS as f64;
        let mut high = (m + 1) as f64 / ARC_SUB_INTERVALS as f64;
        let ds = (self.sub_lengths[m + 1] - self.sub_lengths[m]).max(1e-15);
        let mut u = low + (high - low) * ((target_s - self.sub_lengths[m]) / ds).clamp(0.0, 1.0);
        let tol = 1e-14 * self.length.max(1.0);
        for _ in 0..32 {
            let err = self.arc_length_to(u) - target_s;
            if err.abs() <= tol {
                break;
            }
            if err < 0.0 {
                low = u;
            } else {
                high = u;
            }
            let spd = bezier_speed(self.p0, self.cmd, u);
            let cand = if spd > 1e-12 {
                u - err / spd
            } else {
                0.5 * (low + high)
            };
            u = if cand > low && cand < high {
                cand
            } else {
                0.5 * (low + high)
            };
        }
        u.clamp(0.0, 1.0)
    }

    fn tangent_at_u(&self, u: f64) -> Option<[f64; 2]> {
        let d = deriv_bezier_segment(self.p0, self.cmd, u);
        if d[0].hypot(d[1]) > 1e-9 {
            return Some(d);
        }
        let du = 1e-5;
        let u0 = (u - du).max(0.0);
        let u1 = (u + du).min(1.0);
        if u1 > u0 {
            let a = eval_bezier_segment(self.p0, self.cmd, u0);
            let b = eval_bezier_segment(self.p0, self.cmd, u1);
            let sec = [b[0] - a[0], b[1] - a[1]];
            if sec[0].hypot(sec[1]) > 1e-12 {
                return Some(sec);
            }
        }
        None
    }
}

/// High-precision arc-length sampler and De Casteljau sub-curve extractor for open Bézier motion paths.
#[derive(Clone, Debug)]
pub struct ExactMotionCurve {
    segments: Vec<ExactSegment>,
    cum_lengths: Vec<f64>,
}

impl ExactMotionCurve {
    pub fn from_commands(commands: &[PathCommand]) -> Option<Self> {
        if commands.len() < 2 {
            return None;
        }
        let PathCommand::MoveTo { x: x0, y: y0 } = commands[0] else {
            return None;
        };
        if !x0.is_finite() || !y0.is_finite() {
            return None;
        }
        let mut prev = [x0, y0];
        let mut segments = Vec::with_capacity(commands.len() - 1);
        let mut cum_lengths = Vec::with_capacity(commands.len());
        cum_lengths.push(0.0);
        for &cmd in &commands[1..] {
            if matches!(cmd, PathCommand::MoveTo { .. } | PathCommand::Close) {
                return None;
            }
            let seg = ExactSegment::new(prev, cmd)?;
            let next_len = cum_lengths.last().copied().unwrap_or(0.0) + seg.length;
            cum_lengths.push(next_len);
            prev = command_endpoint(cmd)?;
            segments.push(seg);
        }
        Some(Self {
            segments,
            cum_lengths,
        })
    }

    pub fn from_single_segment(start: [f64; 2], command: PathCommand) -> Option<Self> {
        Self::from_commands(&[
            PathCommand::MoveTo {
                x: start[0],
                y: start[1],
            },
            command,
        ])
    }

    pub fn length(&self) -> f64 {
        self.cum_lengths.last().copied().unwrap_or(0.0)
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    pub fn cumulative_segment_lengths(&self) -> &[f64] {
        &self.cum_lengths
    }

    fn locate(&self, s: f64) -> (usize, f64) {
        let total = self.length();
        if total <= 1e-15 || s <= 0.0 {
            return (0, 0.0);
        }
        if s >= total {
            return (self.segments.len() - 1, 1.0);
        }
        let idx = self
            .cum_lengths
            .partition_point(|&len| len <= s)
            .saturating_sub(1)
            .min(self.segments.len() - 1);
        let local_s = (s - self.cum_lengths[idx]).clamp(0.0, self.segments[idx].length);
        let u = self.segments[idx].solve_u_for_arc_length(local_s);
        (idx, u)
    }

    pub fn point_at(&self, s: f64) -> [f64; 2] {
        let (idx, u) = self.locate(s);
        let seg = &self.segments[idx];
        eval_bezier_segment(seg.p0, seg.cmd, u)
    }

    pub fn tangent_vector_at(&self, s: f64) -> Option<[f64; 2]> {
        let (idx, u) = self.locate(s);
        if let Some(d) = self.segments[idx].tangent_at_u(u) {
            return Some(d);
        }
        for k in (0..idx).rev() {
            if let Some(d) = self.segments[k].tangent_at_u(1.0) {
                return Some(d);
            }
        }
        for k in idx + 1..self.segments.len() {
            if let Some(d) = self.segments[k].tangent_at_u(0.0) {
                return Some(d);
            }
        }
        None
    }

    pub fn parameter_on_single_segment_at(&self, s: f64) -> f64 {
        self.segments
            .first()
            .map_or(0.0, |seg| seg.solve_u_for_arc_length(s))
    }

    pub fn nearest(&self, pt: [f64; 2]) -> f64 {
        const SAMPLES: usize = 64;
        let mut best_dist_sq = f64::INFINITY;
        let mut best_seg = 0usize;
        let mut best_u = 0.0;
        for (idx, seg) in self.segments.iter().enumerate() {
            let steps = if matches!(seg.cmd, PathCommand::LineTo { .. }) {
                1
            } else {
                SAMPLES
            };
            let mut local_best_i = 0usize;
            let mut local_best_d2 = f64::INFINITY;
            for i in 0..=steps {
                let u = i as f64 / steps as f64;
                let p = eval_bezier_segment(seg.p0, seg.cmd, u);
                let dx = p[0] - pt[0];
                let dy = p[1] - pt[1];
                let d2 = dx * dx + dy * dy;
                if d2 < local_best_d2 {
                    local_best_d2 = d2;
                    local_best_i = i;
                }
            }
            let mut lo = (local_best_i.saturating_sub(1)) as f64 / steps as f64;
            let mut hi = ((local_best_i + 1).min(steps)) as f64 / steps as f64;
            for _ in 0..32 {
                let m1 = lo + (hi - lo) / 3.0;
                let m2 = hi - (hi - lo) / 3.0;
                let p1 = eval_bezier_segment(seg.p0, seg.cmd, m1);
                let p2 = eval_bezier_segment(seg.p0, seg.cmd, m2);
                let d1 = (p1[0] - pt[0]).powi(2) + (p1[1] - pt[1]).powi(2);
                let d2 = (p2[0] - pt[0]).powi(2) + (p2[1] - pt[1]).powi(2);
                if d1 <= d2 {
                    hi = m2;
                } else {
                    lo = m1;
                }
            }
            let u = 0.5 * (lo + hi);
            let p = eval_bezier_segment(seg.p0, seg.cmd, u);
            let d2 = (p[0] - pt[0]).powi(2) + (p[1] - pt[1]).powi(2);
            if d2 < best_dist_sq {
                best_dist_sq = d2;
                best_seg = idx;
                best_u = u;
            }
        }
        self.cum_lengths[best_seg] + self.segments[best_seg].arc_length_to(best_u)
    }

    pub fn subdivide_arc_range(&self, s0: f64, s1: f64) -> Option<Vec<PathCommand>> {
        if !s0.is_finite() || !s1.is_finite() {
            return None;
        }
        let total = self.length();
        let s0 = s0.clamp(0.0, total);
        let s1 = s1.clamp(0.0, total);
        if s1 + 1e-12 < s0 {
            return None;
        }
        if total <= 1e-12 || s1 - s0 <= 1e-12 {
            let (idx, u) = self.locate(s0);
            let seg = &self.segments[idx];
            let (start_pt, sub_cmd) = subdivide_bezier_segment(seg.p0, seg.cmd, u, u)?;
            return Some(vec![
                PathCommand::MoveTo {
                    x: start_pt[0],
                    y: start_pt[1],
                },
                sub_cmd,
            ]);
        }
        let mut out = Vec::new();
        let eps = 1e-12 * total.max(1.0);
        for (idx, seg) in self.segments.iter().enumerate() {
            let seg_s0 = self.cum_lengths[idx];
            let seg_s1 = self.cum_lengths[idx + 1];
            if seg_s1 <= s0 + eps || seg_s0 >= s1 - eps {
                continue;
            }
            let local_s0 = (s0 - seg_s0).clamp(0.0, seg.length);
            let local_s1 = (s1 - seg_s0).clamp(0.0, seg.length);
            let u0 = if local_s0 <= eps {
                0.0
            } else {
                seg.solve_u_for_arc_length(local_s0)
            };
            let u1 = if local_s1 + eps >= seg.length {
                1.0
            } else {
                seg.solve_u_for_arc_length(local_s1)
            };
            let (start_pt, sub_cmd) = subdivide_bezier_segment(seg.p0, seg.cmd, u0, u1)?;
            if out.is_empty() {
                out.push(PathCommand::MoveTo {
                    x: start_pt[0],
                    y: start_pt[1],
                });
            }
            out.push(sub_cmd);
        }
        if out.len() < 2 {
            let (idx, u) = self.locate(s0);
            let seg = &self.segments[idx];
            let (start_pt, sub_cmd) = subdivide_bezier_segment(seg.p0, seg.cmd, u, u)?;
            return Some(vec![
                PathCommand::MoveTo {
                    x: start_pt[0],
                    y: start_pt[1],
                },
                sub_cmd,
            ]);
        }
        Some(out)
    }
}

/// Subdivide an open `MoveTo` + (`LineTo` / `QuadraticTo` / `CubicTo`) path onto the normalized
/// arc-length interval `[frac0, frac1]` (`0 <= frac0 <= frac1 <= 1`), preserving exact Bézier
/// curve commands via De Casteljau subdivision.
pub fn subdivide_path_commands(
    commands: &[PathCommand],
    frac0: f64,
    frac1: f64,
) -> Option<Vec<PathCommand>> {
    if !frac0.is_finite() || !frac1.is_finite() {
        return None;
    }
    let frac0 = frac0.clamp(0.0, 1.0);
    let frac1 = frac1.clamp(0.0, 1.0);
    if frac1 + 1e-12 < frac0 {
        return None;
    }
    let curve = ExactMotionCurve::from_commands(commands)?;
    if frac0 == 0.0 && frac1 == 1.0 {
        return Some(commands.to_vec());
    }
    let total = curve.length();
    curve.subdivide_arc_range(frac0 * total, frac1 * total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementKind, LayerProperties, Project, evaluate};

    #[test]
    fn property_expression_time_uses_the_document_frame_rate() {
        let mut project = Project::blank();
        project.format_version = 9;
        project.symbols[0].timeline.layers[0].keyframes[0] = Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "clocked".into(),
                kind: ElementKind::Rectangle {
                    width: 10.0,
                    height: 10.0,
                    fill: [255; 4],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            }],
            property_expressions: vec![PropertyExpression {
                property: TweenProperty::X,
                expression: "time".into(),
                ..PropertyExpression::default()
            }],
            ..Keyframe::default()
        };

        project.fps = 30;
        let at_30 = evaluate(&project, &project.root, 30);
        assert!((at_30[0].transform[4] - 1.0).abs() < 1e-9);

        project.fps = 60;
        let at_60 = evaluate(&project, &project.root, 30);
        assert!((at_60[0].transform[4] - 0.5).abs() < 1e-9);

        // The compatibility wrapper intentionally retains its historical 24 fps clock.
        let legacy = crate::sample_layer(&project.symbols[0].timeline.layers[0], 24);
        assert!((legacy[0].transform.x - 1.0).abs() < 1e-9);
    }

    fn evaluate_source(source: &str, base_value: f64) -> f64 {
        evaluate_property_expression(
            &PropertyExpression {
                expression: source.into(),
                ..PropertyExpression::default()
            },
            base_value,
            0.5,
            12,
            24.0,
            3,
        )
    }

    #[test]
    fn property_expression_parser_preserves_ordinary_compatibility() {
        assert_eq!(evaluate_source(".5 + 1.", 9.0), 1.5);
        assert!((evaluate_source("--((value + sin(time * pi)))", 2.0) - 3.0).abs() < 1e-9);
        assert_eq!(evaluate_source("unknown + mystery(1, 2, 3, 4)", 9.0), 0.0);
        assert_eq!(evaluate_source("min(4, 2, 99)", 9.0), 2.0);
        assert_eq!(evaluate_source("8 / 0 + 6 % 0", 9.0), 14.0);

        let wiggle = PropertyExpression {
            expression: "wiggle()".into(),
            amplitude: 4.0,
            frequency: 3.0,
            seed: 17,
            ..PropertyExpression::default()
        };
        let first = evaluate_property_expression(&wiggle, 20.0, 0.5, 12, 24.0, 3);
        let second = evaluate_property_expression(&wiggle, 20.0, 0.5, 12, 24.0, 3);
        assert_eq!(first, second);
        assert!((first - 20.0).abs() <= 4.0);
    }

    #[test]
    fn property_expression_parser_enforces_exact_budgets_and_full_input() {
        let exactly_max_bytes = format!("1{}", " ".repeat(MAX_PROPERTY_EXPRESSION_BYTES - 1));
        let over_max_bytes = format!("1{}", " ".repeat(MAX_PROPERTY_EXPRESSION_BYTES));
        assert_eq!(exactly_max_bytes.len(), MAX_PROPERTY_EXPRESSION_BYTES);
        assert_eq!(over_max_bytes.len(), MAX_PROPERTY_EXPRESSION_BYTES + 1);
        assert!(validate_property_expression_syntax(&exactly_max_bytes).is_ok());
        assert_eq!(
            validate_property_expression_syntax(&over_max_bytes).unwrap_err(),
            "expression exceeds 4096 UTF-8 bytes"
        );
        assert_eq!(evaluate_source(&exactly_max_bytes, 41.0), 1.0);
        assert_eq!(evaluate_source(&over_max_bytes, 41.0), 41.0);

        let exactly_max_tokens = format!("{}1", "+".repeat(MAX_PROPERTY_EXPRESSION_TOKENS - 1));
        let over_max_tokens = format!("{}1", "+".repeat(MAX_PROPERTY_EXPRESSION_TOKENS));
        assert_eq!(tokenize_expression(&exactly_max_tokens).unwrap().len(), 512);
        assert!(validate_property_expression_syntax(&exactly_max_tokens).is_ok());
        assert_eq!(
            validate_property_expression_syntax(&over_max_tokens).unwrap_err(),
            "expression exceeds 512 tokens"
        );
        assert_eq!(evaluate_source(&exactly_max_tokens, 41.0), 1.0);
        assert_eq!(evaluate_source(&over_max_tokens, 41.0), 41.0);

        let exactly_max_depth = format!(
            "{}1{}",
            "(".repeat(MAX_PROPERTY_EXPRESSION_DEPTH),
            ")".repeat(MAX_PROPERTY_EXPRESSION_DEPTH)
        );
        let over_max_depth = format!(
            "{}1{}",
            "(".repeat(MAX_PROPERTY_EXPRESSION_DEPTH + 1),
            ")".repeat(MAX_PROPERTY_EXPRESSION_DEPTH + 1)
        );
        assert!(validate_property_expression_syntax(&exactly_max_depth).is_ok());
        assert_eq!(
            validate_property_expression_syntax(&over_max_depth).unwrap_err(),
            "expression nesting exceeds 64"
        );
        assert_eq!(evaluate_source(&exactly_max_depth, 41.0), 1.0);
        assert_eq!(evaluate_source(&over_max_depth, 41.0), 41.0);

        let huge_number = "9".repeat(400);
        for malformed in [
            "1 2",
            "1+",
            "()",
            "sin(1",
            "sin(1,)",
            "1..2",
            "value @ 1",
            "1e3",
            &huge_number,
        ] {
            assert_eq!(
                evaluate_source(malformed, 41.0),
                41.0,
                "malformed source must fail closed: {malformed}"
            );
        }
    }

    #[test]
    fn property_expression_non_finite_clamp_fails_closed_without_panicking() {
        let expression = PropertyExpression {
            expression: "clamp(value, amp, 1)".into(),
            amplitude: f64::NAN,
            ..PropertyExpression::default()
        };
        assert_eq!(
            evaluate_property_expression(&expression, 42.0, 0.5, 12, 24.0, 0),
            42.0
        );
    }

    fn project_with_property_expression(expression: PropertyExpression) -> Project {
        let mut project = Project::blank();
        project.format_version = 9;
        project.symbols[0].timeline.layers[0].keyframes[0]
            .property_expressions
            .push(expression);
        project
    }

    #[test]
    fn serialized_projects_validate_all_property_expression_syntax() {
        let hostile_sources = [
            "1+".to_owned(),
            format!("{}1{}", "(".repeat(65), ")".repeat(65)),
            format!("{}1", "+".repeat(512)),
            format!("1{}", " ".repeat(4_096)),
            "9".repeat(400),
        ];
        for source in hostile_sources {
            let project = project_with_property_expression(PropertyExpression {
                expression: source,
                enabled: false,
                ..PropertyExpression::default()
            });
            // Raw serde deliberately bypasses `Project::to_json`, which validates before encoding.
            let raw = serde_json::to_string(&project).unwrap();
            let error = Project::from_json(&raw).unwrap_err().to_string();
            assert!(
                error
                    .contains("symbols[0].timeline.layers[0].keyframes[0].property_expressions[0]"),
                "validation error must retain the exact expression path: {error}"
            );
            assert!(error.contains("invalid property expression"), "{error}");
        }
    }

    #[test]
    fn valid_property_expression_round_trips_and_evaluates_identically() {
        let project = project_with_property_expression(PropertyExpression {
            expression: "--(.5 + value * 2.) + mystery(1, 2, 3, 4)".into(),
            ..PropertyExpression::default()
        });
        let original = &project.symbols[0].timeline.layers[0].keyframes[0].property_expressions[0];
        let original_value = evaluate_property_expression(original, 7.0, 0.25, 6, 24.0, 2);

        let encoded = project.to_json().unwrap();
        let reopened = Project::from_json(&encoded).unwrap();
        let round_tripped =
            &reopened.symbols[0].timeline.layers[0].keyframes[0].property_expressions[0];
        assert_eq!(round_tripped, original);
        assert_eq!(
            evaluate_property_expression(round_tripped, 7.0, 0.25, 6, 24.0, 2),
            original_value
        );
    }

    #[test]
    fn expressions_motion_sketch_wiggler_and_work_area_behave_deterministically() {
        // 1. Property expressions
        let expr_sin = PropertyExpression {
            property: TweenProperty::Y,
            expression: "value + sin(time * pi) * 20".into(),
            enabled: true,
            ..Default::default()
        };
        let v_t0 = evaluate_property_expression(&expr_sin, 100.0, 0.0, 0, 24.0, 0);
        let v_t05 = evaluate_property_expression(&expr_sin, 100.0, 0.5, 12, 24.0, 0);
        assert!((v_t0 - 100.0).abs() < 1e-6);
        assert!((v_t05 - 120.0).abs() < 1e-6);

        let expr_wiggle = PropertyExpression {
            property: TweenProperty::X,
            expression: "wiggle(4, 15)".into(),
            enabled: true,
            seed: 7,
            ..Default::default()
        };
        let w1 = evaluate_property_expression(&expr_wiggle, 50.0, 0.35, 8, 24.0, 1);
        let w2 = evaluate_property_expression(&expr_wiggle, 50.0, 0.35, 8, 24.0, 1);
        assert_eq!(w1, w2, "wiggle expression must be 100% deterministic");
        assert!((w1 - 50.0).abs() <= 15.0);

        // 2. Motion Sketch capture & smoothing
        let samples = vec![
            MotionSketchSample {
                time_sec: 0.0,
                position: [0.0, 0.0],
            },
            MotionSketchSample {
                time_sec: 0.1,
                position: [20.0, 15.0],
            },
            MotionSketchSample {
                time_sec: 0.2,
                position: [40.0, 2.0],
            },
            MotionSketchSample {
                time_sec: 0.3,
                position: [60.0, 18.0],
            },
            MotionSketchSample {
                time_sec: 0.4,
                position: [80.0, 0.0],
            },
        ];
        let captured = capture_motion_sketch_to_keyframes(&samples, 10.0, 0, 1.0);
        assert!(captured.len() >= 2);
        let smoothed = smooth_motion_keyframes(&captured, 0.8);
        assert_eq!(smoothed.first().unwrap().1, [0.0, 0.0]);
        assert_eq!(smoothed.last().unwrap().1, [80.0, 0.0]);

        // 3. Wiggler
        let mut layer = Layer {
            id: "l1".into(),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "hero".into(),
                    name: "Hero".into(),
                    transform: Transform {
                        x: 100.0,
                        y: 100.0,
                        ..Default::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 30.0,
                        height: 30.0,
                        fill: [255, 0, 0, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        };
        let modified = apply_wiggler_to_keyframes(
            &mut layer,
            "hero",
            0,
            4,
            24.0,
            &WigglerConfig {
                property: TweenProperty::X,
                noise_kind: WigglerNoiseKind::Smooth,
                dimension: WigglerDimension::AllIndependently,
                frequency_hz: 4.0,
                magnitude: 20.0,
                seed: 99,
            },
        );
        assert_eq!(modified, 5);
        assert_eq!(layer.keyframes.len(), 5);

        // 4. Preview Work Area
        let wa = PreviewWorkArea {
            enabled: true,
            in_frame: 2,
            out_frame: 6,
            skip_nth_frame: 1,
        };
        assert_eq!(wa.next_frame(2, true), 4);
        assert_eq!(wa.next_frame(6, true), 2);
    }
}
