//! Rust-owned finite 2D ActionScript geometry values.
//! Individual algorithms adapted from Ruffle 07d3e430631333f80f754df01a968bdcd80d98c5
//! core/src/avm2/globals/flash/geom/{Point,Rectangle,Matrix,ColorTransform}.as.
//! Copyright (c) 2018-2022 Ruffle LLC <ruffle@ruffle.rs> and Ruffle contributors.
//! MIT selected; see docs/RUNTIME-GEOMETRY-PROVENANCE.md and THIRD_PARTY_NOTICES.txt.
//! Packed color interpolation additionally adapts HaxeFlixel FlxColor.interpolate
//! c2030e3c9b6463577319f9697f7b72c1bd546566 (MIT): copyright 2009 Adam Saltsman,
//! 2012 Matt Tuttle, 2013 HaxeFlixel Team.
use serde_json::{Value, json};

const POINT: &[&str] = &["x", "y"];
const RECTANGLE: &[&str] = &["x", "y", "width", "height"];
const MATRIX: &[&str] = &["a", "b", "c", "d", "tx", "ty"];
const COLOR: &[&str] = &[
    "redMultiplier",
    "greenMultiplier",
    "blueMultiplier",
    "alphaMultiplier",
    "redOffset",
    "greenOffset",
    "blueOffset",
    "alphaOffset",
];
const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];

#[derive(Debug, Clone, PartialEq)]
pub enum Geometry2dValue {
    Point([f64; 2]),
    Rectangle([f64; 4]),
    Matrix([f64; 6]),
    ColorTransform([f64; 8]),
}
fn finite(value: f64) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Geometry currently requires finite numeric values and results".into())
    }
}
fn number(value: &Value) -> Result<f64, String> {
    finite(
        value
            .as_f64()
            .ok_or("Geometry requires finite numeric arguments")?,
    )
}
fn array<const N: usize>(value: &Value) -> Result<[f64; N], String> {
    let values = value
        .as_array()
        .ok_or("Geometry argument requires numeric coordinates")?;
    if values.len() != N {
        return Err(format!("Geometry argument requires {N} coordinates"));
    }
    let mut result = [0.; N];
    for (i, item) in values.iter().enumerate() {
        result[i] = number(item)?;
    }
    Ok(result)
}
fn numeric(value: f64) -> Result<Value, String> {
    Ok(json!(finite(value)?))
}
fn output(kind: &str, values: &[f64]) -> Result<Value, String> {
    for value in values {
        finite(*value)?;
    }
    Ok(json!({"type":kind,"data":values}))
}
fn empty(rect: &[f64; 4]) -> bool {
    rect[2] <= 0. || rect[3] <= 0.
}
fn edges(rect: &[f64; 4]) -> Result<[f64; 4], String> {
    Ok([
        rect[0],
        rect[1],
        finite(rect[0] + rect[2])?,
        finite(rect[1] + rect[3])?,
    ])
}
fn intersection(first: &[f64; 4], second: &[f64; 4]) -> Result<[f64; 4], String> {
    if empty(first) || empty(second) {
        return Ok([0.; 4]);
    }
    let a = edges(first)?;
    let b = edges(second)?;
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    let w = a[2].min(b[2]) - x;
    let h = a[3].min(b[3]) - y;
    Ok(if w > 0. && h > 0. {
        [x, y, w, h]
    } else {
        [0.; 4]
    })
}
fn compose(before: [f64; 6], after: [f64; 6]) -> [f64; 6] {
    let [a, b, c, d, tx, ty] = before;
    let [u, v, w, z, x, y] = after;
    [
        u * a + w * b,
        v * a + z * b,
        u * c + w * d,
        v * c + z * d,
        u * tx + w * ty + x,
        v * tx + z * ty + y,
    ]
}
fn to_u32(value: f64) -> u32 {
    if !value.is_finite() || value == 0. {
        0
    } else {
        value.trunc().rem_euclid(4294967296.) as u32
    }
}
fn motion_ratio(value: &Value) -> Result<f64, String> {
    let value = number(value)?;
    if !(0. ..=1.).contains(&value) {
        return Err("Motion Color ratios require values from 0 through 1".into());
    }
    Ok(value)
}
fn motion_rgb(value: &Value) -> Result<u32, String> {
    let value = number(value)?;
    if value.fract() != 0. || !(0. ..=4294967295.).contains(&value) {
        return Err("Motion Color requires an unsigned 32-bit color value".into());
    }
    Ok(value as u32)
}
fn tint_multiplier(v: &[f64; 8]) -> Result<f64, String> {
    if v[0] != v[1] || v[0] != v[2] || !(0. ..=1.).contains(&v[0]) {
        return Err(
            "Motion Color tint access requires equal RGB multipliers from 0 through 1".into(),
        );
    }
    Ok(1. - v[0])
}
fn tint_color(v: &[f64; 8]) -> Result<u32, String> {
    let amount = tint_multiplier(v)?;
    if amount == 0. {
        return Ok(0);
    }
    let mut color = 0;
    for (i, offset) in v[4..7].iter().enumerate() {
        let channel = finite(offset / amount)?.round();
        if !(0. ..=255.).contains(&channel) {
            return Err("Motion Color tint offsets exceed the RGB range".into());
        }
        color |= (channel as u32) << (16 - i * 8);
    }
    Ok(color)
}
fn set_tint(v: &mut [f64; 8], color: u32, amount: f64) {
    v[..3].fill(1. - amount);
    for i in 0..3 {
        v[i + 4] = ((color >> (16 - i * 8)) & 255) as f64 * amount;
    }
}
fn row_index(value: &Value) -> Result<u32, String> {
    let value = number(value)?;
    if value.fract() != 0. || !(0. ..=4294967295.).contains(&value) {
        return Err("Matrix row/column index requires an unsigned integer".into());
    }
    Ok(value as u32)
}

fn matrix_point(v: &[f64; 6], p: [f64; 2], translated: bool) -> [f64; 2] {
    [
        v[0] * p[0] + v[2] * p[1] + if translated { v[4] } else { 0. },
        v[1] * p[0] + v[3] * p[1] + if translated { v[5] } else { 0. },
    ]
}
fn set_matrix_axis(v: &mut [f64; 6], horizontal: bool, scale: f64, angle: f64) {
    if horizontal {
        v[0] = angle.cos() * scale;
        v[1] = angle.sin() * scale;
    } else {
        v[2] = -angle.sin() * scale;
        v[3] = angle.cos() * scale;
    }
}
// Axis decomposition/reconstruction adapted from Ruffle display_object.rs;
// the existing affine composition and mapping kernels implement pivot changes.
fn matrix_transformer(v: &mut [f64; 6], name: &str, args: &[Value]) -> Result<Value, String> {
    let arg = |i: usize| number(args.get(i).unwrap_or(&Value::Null));
    let sx = v[0].hypot(v[1]);
    let sy = v[2].hypot(v[3]);
    let skew_x = (-v[2]).atan2(v[3]);
    let skew_y = v[1].atan2(v[0]);
    match name {
        "getScaleX" => return numeric(sx),
        "getScaleY" => return numeric(sy),
        "getSkewX" => return numeric(skew_x.to_degrees()),
        "getSkewXRadians" => return numeric(skew_x),
        "getSkewY" | "getRotation" => return numeric(skew_y.to_degrees()),
        "getSkewYRadians" | "getRotationRadians" => return numeric(skew_y),
        "setScaleX" => set_matrix_axis(v, true, arg(0)?, skew_y),
        "setScaleY" => set_matrix_axis(v, false, arg(0)?, skew_x),
        "setSkewX" | "setSkewXRadians" => {
            let angle = arg(0)?;
            set_matrix_axis(
                v,
                false,
                sy,
                if name.ends_with("Radians") {
                    angle
                } else {
                    angle.to_radians()
                },
            );
        }
        "setSkewY" | "setSkewYRadians" => {
            let angle = arg(0)?;
            set_matrix_axis(
                v,
                true,
                sx,
                if name.ends_with("Radians") {
                    angle
                } else {
                    angle.to_radians()
                },
            );
        }
        "setRotation" | "setRotationRadians" => {
            let angle = arg(0)?;
            let angle = if name.ends_with("Radians") {
                angle
            } else {
                angle.to_radians()
            };
            set_matrix_axis(v, true, sx, angle);
            set_matrix_axis(v, false, sy, skew_x + angle - skew_y);
        }
        "rotateAroundInternalPoint" | "rotateAroundExternalPoint" => {
            let point = [arg(0)?, arg(1)?];
            let point = if name == "rotateAroundInternalPoint" {
                matrix_point(v, point, true)
            } else {
                point
            };
            let angle = arg(2)?.to_radians();
            let (s, c) = angle.sin_cos();
            v[4] -= point[0];
            v[5] -= point[1];
            *v = compose(*v, [c, s, -s, c, 0., 0.]);
            v[4] += point[0];
            v[5] += point[1];
        }
        "matchInternalPointWithExternal" => {
            let internal = array::<2>(args.first().unwrap_or(&Value::Null))?;
            let external = array::<2>(args.get(1).unwrap_or(&Value::Null))?;
            let mapped = matrix_point(v, internal, false);
            v[4] = external[0] - mapped[0];
            v[5] = external[1] - mapped[1];
        }
        _ => {
            return Err(format!(
                "MatrixTransformer.{name} is outside the finite profile"
            ));
        }
    }
    Ok(Value::Null)
}

impl Geometry2dValue {
    pub fn new(kind: &str, data: &Value) -> Result<Self, String> {
        match kind {
            "Point" => Ok(Self::Point(array(data)?)),
            "Rectangle" => Ok(Self::Rectangle(array(data)?)),
            "Matrix" => Ok(Self::Matrix(array(data)?)),
            "ColorTransform" => Ok(Self::ColorTransform(array(data)?)),
            _ => Err(format!("Unsupported 2D geometry kind '{kind}'")),
        }
    }
    fn values(&self) -> &[f64] {
        match self {
            Self::Point(v) => v,
            Self::Rectangle(v) => v,
            Self::Matrix(v) => v,
            Self::ColorTransform(v) => v,
        }
    }
    fn values_mut(&mut self) -> &mut [f64] {
        match self {
            Self::Point(v) => v,
            Self::Rectangle(v) => v,
            Self::Matrix(v) => v,
            Self::ColorTransform(v) => v,
        }
    }
    fn fields(&self) -> &'static [&'static str] {
        match self {
            Self::Point(_) => POINT,
            Self::Rectangle(_) => RECTANGLE,
            Self::Matrix(_) => MATRIX,
            Self::ColorTransform(_) => COLOR,
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::Rectangle(_) => "Rectangle",
            Self::Matrix(_) => "Matrix",
            Self::ColorTransform(_) => "ColorTransform",
        }
    }
    pub fn snapshot(&self) -> Value {
        json!(self.values())
    }
    /// A rejected result never partially mutates the stored value.
    pub fn call(&mut self, operation: &str, arguments: &Value) -> Result<Value, String> {
        let args = arguments
            .as_array()
            .ok_or("Geometry call arguments require an array")?;
        if args.len() > 8 {
            return Err("Geometry call exceeds eight arguments".into());
        }
        let mut next = self.clone();
        let result = next.apply(operation, args)?;
        for value in next.values() {
            finite(*value)?;
        }
        *self = next;
        Ok(result)
    }
    fn apply(&mut self, op: &str, args: &[Value]) -> Result<Value, String> {
        let null = Value::Null;
        let arg = |i: usize| args.get(i).unwrap_or(&null);
        if let Some(i) = self.fields().iter().position(|field| *field == op) {
            return numeric(self.values()[i]);
        }
        if let Some(field) = op.strip_prefix("set:")
            && let Some(i) = self
                .fields()
                .iter()
                .position(|candidate| *candidate == field)
        {
            self.values_mut()[i] = number(arg(0))?;
            return Ok(Value::Null);
        }
        if op == "clone" {
            return output(self.kind(), self.values());
        }
        match self {
            Self::Point(v) => match op {
                "length" => return numeric(v[0].hypot(v[1])),
                "copyFrom" => *v = array(arg(0))?,
                "setTo" => *v = [number(arg(0))?, number(arg(1))?],
                "add" | "subtract" => {
                    let p = array::<2>(arg(0))?;
                    let sign = if op == "add" { 1. } else { -1. };
                    return output("Point", &[v[0] + sign * p[0], v[1] + sign * p[1]]);
                }
                "equals" => return Ok(json!(*v == array::<2>(arg(0))?)),
                "offset" => {
                    v[0] += number(arg(0))?;
                    v[1] += number(arg(1))?;
                }
                "normalize" => {
                    let thickness = number(arg(0))?;
                    let length = finite(v[0].hypot(v[1]))?;
                    if length != 0. {
                        v[0] = v[0] / length * thickness;
                        v[1] = v[1] / length * thickness;
                    }
                }
                "distance" => {
                    let p = array::<2>(arg(0))?;
                    return numeric((p[0] - v[0]).hypot(p[1] - v[1]));
                }
                "interpolate" => {
                    let p = array::<2>(arg(0))?;
                    let f = number(arg(1))?;
                    return output(
                        "Point",
                        &[p[0] - (p[0] - v[0]) * f, p[1] - (p[1] - v[1]) * f],
                    );
                }
                "polar" => {
                    let length = number(arg(0))?;
                    let angle = number(arg(1))?;
                    return output("Point", &[length * angle.cos(), length * angle.sin()]);
                }
                "toString" => return Ok(json!(format!("(x={}, y={})", v[0], v[1]))),
                _ => return Err(format!("Point.{op} is outside the supported profile")),
            },
            Self::Rectangle(v) => match op {
                "left" | "xMin" => return numeric(v[0]),
                "top" | "yMin" => return numeric(v[1]),
                "right" | "xMax" => return numeric(v[0] + v[2]),
                "bottom" | "yMax" => return numeric(v[1] + v[3]),
                "topLeft" => return output("Point", &v[..2]),
                "bottomRight" => return output("Point", &[v[0] + v[2], v[1] + v[3]]),
                "size" => return output("Point", &v[2..]),
                "set:left" => {
                    let x = number(arg(0))?;
                    v[2] = finite(v[0] + v[2])? - x;
                    v[0] = x;
                }
                "set:top" => {
                    let y = number(arg(0))?;
                    v[3] = finite(v[1] + v[3])? - y;
                    v[1] = y;
                }
                "set:right" => v[2] = number(arg(0))? - v[0],
                "set:bottom" => v[3] = number(arg(0))? - v[1],
                "set:topLeft" => {
                    let p = array::<2>(arg(0))?;
                    *v = [
                        p[0],
                        p[1],
                        finite(v[0] + v[2])? - p[0],
                        finite(v[1] + v[3])? - p[1],
                    ];
                }
                "set:bottomRight" => {
                    let p = array::<2>(arg(0))?;
                    v[2] = p[0] - v[0];
                    v[3] = p[1] - v[1];
                }
                "set:size" => {
                    let p = array::<2>(arg(0))?;
                    v[2] = p[0];
                    v[3] = p[1];
                }
                "copyFrom" => *v = array(arg(0))?,
                "setTo" => {
                    *v = [
                        number(arg(0))?,
                        number(arg(1))?,
                        number(arg(2))?,
                        number(arg(3))?,
                    ]
                }
                "setEmpty" => *v = [0.; 4],
                "isEmpty" => return Ok(json!(empty(v))),
                "equals" => return Ok(json!(*v == array::<4>(arg(0))?)),
                "contains" | "containsPoint" => {
                    let p = if op == "contains" {
                        [number(arg(0))?, number(arg(1))?]
                    } else {
                        array(arg(0))?
                    };
                    let e = edges(v)?;
                    return Ok(json!(
                        p[0] >= e[0] && p[1] >= e[1] && p[0] < e[2] && p[1] < e[3]
                    ));
                }
                "containsRect" => {
                    let a = edges(v)?;
                    let b = edges(&array(arg(0))?)?;
                    return Ok(json!(
                        b[0] >= a[0]
                            && b[1] >= a[1]
                            && b[0] < a[2]
                            && b[1] < a[3]
                            && b[2] > a[0]
                            && b[3] > a[1]
                            && b[2] <= a[2]
                            && b[3] <= a[3]
                    ));
                }
                "intersection" | "intersects" => {
                    let r = intersection(v, &array(arg(0))?)?;
                    return if op == "intersects" {
                        Ok(json!(!empty(&r)))
                    } else {
                        output("Rectangle", &r)
                    };
                }
                "union" => {
                    let r = array::<4>(arg(0))?;
                    let result = if empty(v) {
                        r
                    } else if empty(&r) {
                        *v
                    } else {
                        let a = edges(v)?;
                        let b = edges(&r)?;
                        let x = a[0].min(b[0]);
                        let y = a[1].min(b[1]);
                        [x, y, a[2].max(b[2]) - x, a[3].max(b[3]) - y]
                    };
                    return output("Rectangle", &result);
                }
                "offset" | "offsetPoint" | "inflate" | "inflatePoint" => {
                    let [x, y] = if op.ends_with("Point") {
                        array(arg(0))?
                    } else {
                        [number(arg(0))?, number(arg(1))?]
                    };
                    if op.starts_with("inflate") {
                        v[0] -= x;
                        v[1] -= y;
                        v[2] += 2. * x;
                        v[3] += 2. * y;
                    } else {
                        v[0] += x;
                        v[1] += y;
                    }
                }
                "toString" => {
                    return Ok(json!(format!(
                        "(x={}, y={}, w={}, h={})",
                        v[0], v[1], v[2], v[3]
                    )));
                }
                _ => return Err(format!("Rectangle.{op} is outside the supported profile")),
            },
            Self::Matrix(v) => match op {
                op if op.starts_with("transformer:") => {
                    return matrix_transformer(v, &op[12..], args);
                }
                "copyFrom" => *v = array(arg(0))?,
                "setTo" => {
                    *v = [
                        number(arg(0))?,
                        number(arg(1))?,
                        number(arg(2))?,
                        number(arg(3))?,
                        number(arg(4))?,
                        number(arg(5))?,
                    ]
                }
                "identity" => *v = IDENTITY,
                "concat" => *v = compose(*v, array(arg(0))?),
                "transformPoint" | "deltaTransformPoint" => {
                    let p = array::<2>(arg(0))?;
                    return output("Point", &matrix_point(v, p, op == "transformPoint"));
                }
                "invert" => {
                    let [a, b, c, d, tx, ty] = *v;
                    let determinant = finite(a * d - b * c)?;
                    if determinant == 0. {
                        return Err("Singular Matrix inversion is not supported by the finite geometry profile".into());
                    }
                    *v = [
                        d / determinant,
                        -b / determinant,
                        -c / determinant,
                        a / determinant,
                        (c * ty - d * tx) / determinant,
                        (b * tx - a * ty) / determinant,
                    ];
                }
                "rotate" => {
                    let angle = number(arg(0))?;
                    let c = angle.cos();
                    let s = angle.sin();
                    *v = compose(*v, [c, s, -s, c, 0., 0.]);
                }
                "scale" => *v = compose(*v, [number(arg(0))?, 0., 0., number(arg(1))?, 0., 0.]),
                "translate" => {
                    v[4] += number(arg(0))?;
                    v[5] += number(arg(1))?;
                }
                "createBox" | "createGradientBox" => {
                    let mut sx = number(arg(0))?;
                    let mut sy = number(arg(1))?;
                    let angle = number(arg(2))?;
                    let mut tx = number(arg(3))?;
                    let mut ty = number(arg(4))?;
                    if op == "createGradientBox" {
                        tx = finite(tx + sx / 2.)?;
                        ty = finite(ty + sy / 2.)?;
                        sx /= 1638.4;
                        sy /= 1638.4;
                    }
                    let c = angle.cos();
                    let s = angle.sin();
                    *v = [c * sx, s * sy, -s * sx, c * sy, tx, ty];
                }
                "copyRowFrom" | "copyColumnFrom" => {
                    let row = row_index(arg(0))?;
                    let p = array::<4>(arg(1))?;
                    if row < 2 {
                        let i = row as usize;
                        v[i] = p[0];
                        v[i + 2] = p[1];
                        v[i + 4] = p[2];
                    }
                }
                "copyRowTo" | "copyColumnTo" => {
                    let index = row_index(arg(0))?;
                    let mut p = array::<4>(arg(1))?;
                    if index <= 2 {
                        let xyz = if op == "copyRowTo" {
                            match index {
                                0 => [v[0], v[2], v[4]],
                                1 => [v[1], v[3], v[5]],
                                _ => [0., 0., 1.],
                            }
                        } else {
                            let i = index as usize * 2;
                            [v[i], v[i + 1], if index == 2 { 1. } else { 0. }]
                        };
                        p[..3].copy_from_slice(&xyz);
                    }
                    return output("Vector3D", &p);
                }
                "toString" => {
                    return Ok(json!(format!(
                        "(a={}, b={}, c={}, d={}, tx={}, ty={})",
                        v[0], v[1], v[2], v[3], v[4], v[5]
                    )));
                }
                _ => return Err(format!("Matrix.{op} is outside the supported profile")),
            },
            Self::ColorTransform(v) => match op {
                "motion:interpolateColor" => {
                    let from = motion_rgb(arg(0))?;
                    let to = motion_rgb(arg(1))?;
                    let progress = motion_ratio(arg(2))?;
                    // Adapted from HaxeFlixel FlxColor.interpolate (MIT). All four
                    // channels interpolate independently and truncate toward zero.
                    let mut result = 0u32;
                    for shift in [0, 8, 16, 24] {
                        let first = ((from >> shift) & 255) as f64;
                        let second = ((to >> shift) & 255) as f64;
                        result |= (((second - first) * progress + first) as u32) << shift;
                    }
                    return Ok(json!(result));
                }
                "motion:interpolateTransform" => {
                    let to = array::<8>(arg(0))?;
                    let progress = motion_ratio(arg(1))?;
                    let result: [f64; 8] =
                        std::array::from_fn(|i| (to[i] - v[i]) * progress + v[i]);
                    return output("ColorTransform", &result);
                }
                "motion:brightness" => {
                    if v[0] != v[1] || v[0] != v[2] || v[4] != v[5] || v[4] != v[6] {
                        return Err("Motion Color brightness access requires uniform RGB multipliers and offsets".into());
                    }
                    return numeric(if v[4] > 0. { v[4] / 255. } else { v[0] - 1. });
                }
                "motion:setBrightness" => {
                    let amount = number(arg(0))?;
                    if !(-1. ..=1.).contains(&amount) {
                        return Err(
                            "Motion Color brightness requires values from -1 through 1".into()
                        );
                    }
                    v[..3].fill(1. - amount.abs());
                    v[4..7].fill(if amount > 0. { 255. * amount } else { 0. });
                }
                "motion:tintColor" => return Ok(json!(tint_color(v)?)),
                "motion:tintMultiplier" => return numeric(tint_multiplier(v)?),
                "motion:setTint" => {
                    let color = motion_rgb(arg(0))?;
                    let amount = motion_ratio(arg(1))?;
                    set_tint(v, color, amount);
                }
                "motion:setTintColor" => {
                    let color = motion_rgb(arg(0))?;
                    let amount = tint_multiplier(v)?;
                    set_tint(v, color, amount);
                }
                "motion:setTintMultiplier" => {
                    let amount = motion_ratio(arg(0))?;
                    let color = tint_color(v)?;
                    set_tint(v, color, amount);
                }
                "color" => {
                    return Ok(json!(
                        (to_u32(v[4]) << 16) | (to_u32(v[5]) << 8) | to_u32(v[6])
                    ));
                }
                "legacy:getRGB" => {
                    return Ok(json!(
                        ((to_u32(v[4]) << 16) | (to_u32(v[5]) << 8) | to_u32(v[6])) as i32
                    ));
                }
                "legacy:getTransform" => {
                    let mut result = serde_json::Map::new();
                    for (i, name) in ["ra", "ga", "ba", "aa", "rb", "gb", "bb", "ab"]
                        .iter()
                        .enumerate()
                    {
                        result.insert(
                            (*name).into(),
                            numeric(if i < 4 { v[i] * 100. } else { v[i] })?,
                        );
                    }
                    return Ok(Value::Object(result));
                }
                "legacy:setTransform" => {
                    let values = arg(0)
                        .as_object()
                        .ok_or("Color.setTransform requires an object")?;
                    for (i, name) in ["ra", "ga", "ba", "aa", "rb", "gb", "bb", "ab"]
                        .iter()
                        .enumerate()
                    {
                        if let Some(value) = values.get(*name) {
                            let n = number(value)?;
                            // Ruffle AVM1 Color uses signed 8.8 multipliers and i16 offsets.
                            let n = if i < 4 { finite(n * 256. / 100.)? } else { n };
                            let wrapped = n.trunc().rem_euclid(65536.) as u16 as i16;
                            v[i] = if i < 4 {
                                wrapped as f64 / 256.
                            } else {
                                wrapped as f64
                            };
                        }
                    }
                }
                "set:color" | "legacy:setRGB" => {
                    let color = number(arg(0))?;
                    if op == "set:color"
                        && (color.fract() != 0. || !(0. ..=4294967295.).contains(&color))
                    {
                        return Err("ColorTransform.color requires an unsigned RGB integer".into());
                    }
                    let color = to_u32(color);
                    v[..3].fill(0.);
                    v[4] = ((color >> 16) & 255) as f64;
                    v[5] = ((color >> 8) & 255) as f64;
                    v[6] = (color & 255) as f64;
                }
                "concat" => {
                    let second = array::<8>(arg(0))?;
                    for i in 0..4 {
                        v[i + 4] = v[i + 4] * second[i] + second[i + 4];
                        v[i] *= second[i];
                    }
                }
                "toString" => {
                    return Ok(json!(format!(
                        "({})",
                        COLOR
                            .iter()
                            .zip(v.iter())
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                _ => {
                    return Err(format!(
                        "ColorTransform.{op} is outside the supported profile"
                    ));
                }
            },
        }
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapted_2d_geometry_family() {
        let mut p = Geometry2dValue::new("Point", &json!([3, 4])).unwrap();
        assert_eq!(p.call("length", &json!([])).unwrap(), json!(5.));
        p.call("normalize", &json!([10])).unwrap();
        assert_eq!(p.snapshot(), json!([6., 8.]));
        let mut r = Geometry2dValue::new("Rectangle", &json!([10, 20, 30, 40])).unwrap();
        r.call("set:topLeft", &json!([[5, 7]])).unwrap();
        assert_eq!(r.snapshot(), json!([5., 7., 35., 53.]));
        assert_eq!(
            r.call("containsPoint", &json!([[40, 60]])).unwrap(),
            json!(false)
        );
        assert_eq!(
            r.call("intersection", &json!([[0, 0, 8, 10]])).unwrap()["data"],
            json!([5., 7., 3., 3.])
        );
        let mut m = Geometry2dValue::new("Matrix", &json!(IDENTITY)).unwrap();
        m.call("createBox", &json!([2, 3, 0, 10, 20])).unwrap();
        assert_eq!(
            m.call("transformPoint", &json!([[4, 5]])).unwrap()["data"],
            json!([18., 35.])
        );
        m.call("copyColumnFrom", &json!([0, [7, 8, 9, 10]]))
            .unwrap();
        assert_eq!(m.snapshot(), json!([7., 0., 8., 3., 9., 20.]));
        assert_eq!(
            m.call("copyRowTo", &json!([0, [0, 0, 0, 42]])).unwrap()["data"],
            json!([7., 8., 9., 42.])
        );
        let before = m.clone();
        assert!(m.call("scale", &json!([f64::MAX, f64::MAX])).is_err());
        assert_eq!(m, before);
        m.call("identity", &json!([])).unwrap();
        m.call("transformer:setScaleX", &json!([2])).unwrap();
        m.call("transformer:setScaleY", &json!([3])).unwrap();
        m.call("transformer:setSkewX", &json!([30])).unwrap();
        assert!(
            (m.call("transformer:getSkewX", &json!([]))
                .unwrap()
                .as_f64()
                .unwrap()
                - 30.)
                .abs()
                < 1e-10
        );
        m.call("transformer:setRotation", &json!([60])).unwrap();
        assert!(
            (m.call("transformer:getSkewX", &json!([]))
                .unwrap()
                .as_f64()
                .unwrap()
                - 90.)
                .abs()
                < 1e-10
        );
        m.call("identity", &json!([])).unwrap();
        m.call("translate", &json!([5, 6])).unwrap();
        m.call("transformer:rotateAroundInternalPoint", &json!([1, 2, 90]))
            .unwrap();
        let fixed = m.call("transformPoint", &json!([[1, 2]])).unwrap();
        assert!((fixed["data"][0].as_f64().unwrap() - 6.).abs() < 1e-10);
        assert!((fixed["data"][1].as_f64().unwrap() - 8.).abs() < 1e-10);
        m.call(
            "transformer:matchInternalPointWithExternal",
            &json!([[1, 2], [100, 200]]),
        )
        .unwrap();
        assert_eq!(
            m.call("transformPoint", &json!([[1, 2]])).unwrap()["data"],
            json!([100., 200.])
        );
        let mut c =
            Geometry2dValue::new("ColorTransform", &json!([1, 1, 1, 0.5, 0, 0, 0, 9])).unwrap();
        c.call("set:color", &json!([0x123456])).unwrap();
        assert_eq!(c.call("color", &json!([])).unwrap(), json!(0x123456));
        assert_eq!(&c.values()[..4], &[0., 0., 0., 0.5]);
        c.call("legacy:setTransform", &json!([{"ra":50,"gb":99}]))
            .unwrap();
        let legacy = c.call("legacy:getTransform", &json!([])).unwrap();
        assert_eq!(legacy["ra"], json!(50.));
        assert_eq!(legacy["gb"], json!(99.));
        assert_eq!(legacy["aa"], json!(50.));
        c.call("legacy:setRGB", &json!([-1])).unwrap();
        assert_eq!(
            c.call("legacy:getRGB", &json!([])).unwrap(),
            json!(0xffffff)
        );
        assert_eq!(
            c.call("motion:interpolateColor", &json!([0xff0000, 0x0000ff, 0.5]))
                .unwrap(),
            json!(0x7f007f)
        );
        c.call("motion:setTint", &json!([0x336699, 0.5])).unwrap();
        assert_eq!(
            c.call("motion:tintColor", &json!([])).unwrap(),
            json!(0x336699)
        );
        assert_eq!(
            c.call("motion:tintMultiplier", &json!([])).unwrap(),
            json!(0.5)
        );
        c.call("motion:setBrightness", &json!([-0.5])).unwrap();
        assert_eq!(
            c.call("motion:brightness", &json!([])).unwrap(),
            json!(-0.5)
        );
        assert_eq!(
            c.call(
                "motion:interpolateTransform",
                &json!([[1, 1, 1, 1, 0, 0, 0, 0], 0.5])
            )
            .unwrap()["data"][0],
            json!(0.75)
        );
    }
}
