//! Bounded AS3 geometry values owned and calculated in Rust.
//! Adapted from Ruffle 07d3e430631333f80f754df01a968bdcd80d98c5.
//! Matrix algorithms copied from render/src/matrix3d.rs; AVM2 wrappers removed.
//! Vector3D algorithms translated from core/src/avm2/globals/flash/geom/Vector3D.as.
//! Copyright (c) 2018-2022 Ruffle LLC and Ruffle contributors; MIT selected.
//! Rotation and bulk-projection algorithms additionally Copyright (c) 2013-2021 Joshua Granick and
//! other OpenFL contributors (MIT), through Ruffle's documented adaptation.
//! See docs/AS3-REUSE-PROVENANCE.md for pinned sources, modifications and notices.
use serde_json::{Value, json};

/// The transformation matrix for 3D used by Flash display objects.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Matrix3D {
    /// 4x4 matrix elements.
    pub raw_data: [f32; 16],
}

impl Matrix3D {
    pub const IDENTITY: Self = Self {
        raw_data: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn scale(x: f32, y: f32, z: f32) -> Self {
        Self {
            raw_data: [
                x, 0.0, 0.0, 0.0, //
                0.0, y, 0.0, 0.0, //
                0.0, 0.0, z, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn translate(x: f32, y: f32, z: f32) -> Self {
        Self {
            raw_data: [
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                x, y, z, 1.0,
            ],
        }
    }

    pub fn transpose_in_place(&mut self) {
        self.raw_data.swap(1, 4);
        self.raw_data.swap(2, 8);
        self.raw_data.swap(3, 12);
        self.raw_data.swap(6, 9);
        self.raw_data.swap(7, 13);
        self.raw_data.swap(11, 14);
    }

    pub fn determinant(&self) -> f32 {
        let m = &self.raw_data;

        /// The determinant of a 3x3 matrix, stored in column-major order.
        fn determinant_3x3(m: [f32; 9]) -> f32 {
            m[0] * (m[4] * m[8] - m[7] * m[5]) - m[3] * (m[1] * m[8] - m[7] * m[2])
                + m[6] * (m[1] * m[5] - m[4] * m[2])
        }

        // When projection is 0, Flash ignores translation, suggesting there is
        // a fast path here.
        if m[3] == 0.0 && m[7] == 0.0 && m[11] == 0.0 {
            // This is equivalent to doing a Laplace expansion over the last
            // column, but considering only the last row is non-zero, we can
            // skip other rows.
            //
            // Usually we'd calculate the determinant of the inner 3x3 matrix
            // and multiply by the last element, but Flash appears to factor the
            // last element into the formula (observable when it's +-Infinity).
            determinant_3x3([
                m[0],
                m[1],
                m[2] * m[15],
                m[4],
                m[5],
                m[6] * m[15],
                m[8],
                m[9],
                m[10] * m[15],
            ])
        } else {
            // Based on the condition above, it looks like we should do the
            // Laplace expansion over the last column, but that's not what
            // happens. This is observable when using +-Infinity in a dense
            // matrix; infinities are preserved only in the first row, which
            // means that no two infinities are subtracted in the expression,
            // suggesting we should do the Laplace expansion over that row.
            let det0 = determinant_3x3([m[5], m[6], m[7], m[9], m[10], m[11], m[13], m[14], m[15]]);
            let det4 = determinant_3x3([m[1], m[2], m[3], m[9], m[10], m[11], m[13], m[14], m[15]]);
            let det8 = determinant_3x3([m[1], m[2], m[3], m[5], m[6], m[7], m[13], m[14], m[15]]);
            let det12 = determinant_3x3([m[1], m[2], m[3], m[5], m[6], m[7], m[9], m[10], m[11]]);
            m[0] * det0 - m[4] * det4 + m[8] * det8 - m[12] * det12
        }
    }

    pub fn invert(&self) -> Option<Self> {
        let d = self.determinant();
        let invertible = d.abs() > 0.00000000001;

        if !invertible {
            return None;
        }

        let d = 1.0 / d;

        let m = self.raw_data;
        let m11 = m[0];
        let m21 = m[4];
        let m31 = m[8];
        let m41 = m[12];
        let m12 = m[1];
        let m22 = m[5];
        let m32 = m[9];
        let m42 = m[13];
        let m13 = m[2];
        let m23 = m[6];
        let m33 = m[10];
        let m43 = m[14];
        let m14 = m[3];
        let m24 = m[7];
        let m34 = m[11];
        let m44 = m[15];

        #[rustfmt::skip]
        let raw_data = [
             d * (m22 * (m33 * m44 - m43 * m34) - m32 * (m23 * m44 - m43 * m24) + m42 * (m23 * m34 - m33 * m24)),
            -d * (m12 * (m33 * m44 - m43 * m34) - m32 * (m13 * m44 - m43 * m14) + m42 * (m13 * m34 - m33 * m14)),
             d * (m12 * (m23 * m44 - m43 * m24) - m22 * (m13 * m44 - m43 * m14) + m42 * (m13 * m24 - m23 * m14)),
            -d * (m12 * (m23 * m34 - m33 * m24) - m22 * (m13 * m34 - m33 * m14) + m32 * (m13 * m24 - m23 * m14)),
            -d * (m21 * (m33 * m44 - m43 * m34) - m31 * (m23 * m44 - m43 * m24) + m41 * (m23 * m34 - m33 * m24)),
             d * (m11 * (m33 * m44 - m43 * m34) - m31 * (m13 * m44 - m43 * m14) + m41 * (m13 * m34 - m33 * m14)),
            -d * (m11 * (m23 * m44 - m43 * m24) - m21 * (m13 * m44 - m43 * m14) + m41 * (m13 * m24 - m23 * m14)),
             d * (m11 * (m23 * m34 - m33 * m24) - m21 * (m13 * m34 - m33 * m14) + m31 * (m13 * m24 - m23 * m14)),
             d * (m21 * (m32 * m44 - m42 * m34) - m31 * (m22 * m44 - m42 * m24) + m41 * (m22 * m34 - m32 * m24)),
            -d * (m11 * (m32 * m44 - m42 * m34) - m31 * (m12 * m44 - m42 * m14) + m41 * (m12 * m34 - m32 * m14)),
             d * (m11 * (m22 * m44 - m42 * m24) - m21 * (m12 * m44 - m42 * m14) + m41 * (m12 * m24 - m22 * m14)),
            -d * (m11 * (m22 * m34 - m32 * m24) - m21 * (m12 * m34 - m32 * m14) + m31 * (m12 * m24 - m22 * m14)),
            -d * (m21 * (m32 * m43 - m42 * m33) - m31 * (m22 * m43 - m42 * m23) + m41 * (m22 * m33 - m32 * m23)),
             d * (m11 * (m32 * m43 - m42 * m33) - m31 * (m12 * m43 - m42 * m13) + m41 * (m12 * m33 - m32 * m13)),
            -d * (m11 * (m22 * m43 - m42 * m23) - m21 * (m12 * m43 - m42 * m13) + m41 * (m12 * m23 - m22 * m13)),
             d * (m11 * (m22 * m33 - m32 * m23) - m21 * (m12 * m33 - m32 * m13) + m31 * (m12 * m23 - m22 * m13)),
        ];

        Some(Self { raw_data })
    }

    pub fn multiply(&self, rhs: &Self) -> Self {
        let lhs = &self.raw_data;
        let rhs = &rhs.raw_data;
        let mut result = [0.0; 16];

        for column in 0..4 {
            for row in 0..4 {
                result[column * 4 + row] = lhs[row] * rhs[column * 4]
                    + lhs[4 + row] * rhs[column * 4 + 1]
                    + lhs[8 + row] * rhs[column * 4 + 2]
                    + lhs[12 + row] * rhs[column * 4 + 3];
            }
        }

        Self { raw_data: result }
    }
}

#[derive(Clone, Debug)]
pub enum GeometryValue {
    Vector([f64; 4]),
    Matrix(Matrix3D),
}
fn numbers(value: &Value) -> Result<Vec<f64>, String> {
    let values = value
        .as_array()
        .ok_or("Geometry argument must be an array")?;
    if values.len() > 12288 {
        return Err("Geometry array exceeds 12288 components".into());
    }
    values
        .iter()
        .map(|v| {
            v.as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| "Geometry currently supports finite numeric components only".into())
        })
        .collect()
}
fn number(value: &Value) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "Geometry currently supports finite numeric arguments only".into())
}
fn vector(value: &Value) -> Result<[f64; 4], String> {
    numbers(value)?
        .try_into()
        .map_err(|_| "Vector3D requires four components".into())
}
fn matrix(value: &Value) -> Result<Matrix3D, String> {
    let data: [f64; 16] = numbers(value)?
        .try_into()
        .map_err(|_| "Matrix3D requires sixteen components")?;
    if data.iter().any(|v| !(*v as f32).is_finite()) {
        return Err("Matrix3D components exceed finite f32 range".into());
    }
    Ok(Matrix3D {
        raw_data: data.map(|v| v as f32),
    })
}
fn numeric(value: f64) -> Result<Value, String> {
    if !value.is_finite() {
        return Err("Geometry operation produced a non-finite result; this edge case is outside the current profile".into());
    }
    Ok(json!(value))
}
fn vector_output(data: [f64; 4]) -> Result<Value, String> {
    if data.iter().any(|v| !v.is_finite()) {
        return Err("Geometry operation produced non-finite vector components".into());
    }
    Ok(json!({"type":"Vector3D","data":data}))
}
fn matrix_output(data: Matrix3D) -> Result<Value, String> {
    if data.raw_data.iter().any(|v| !v.is_finite()) {
        return Err("Geometry operation produced non-finite matrix components".into());
    }
    Ok(json!({"type":"Matrix3D","data":data.raw_data}))
}
fn length(v: [f64; 4]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn dot(a: [f64; 4], b: [f64; 4]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl GeometryValue {
    pub fn new(kind: &str, data: &Value) -> Result<Self, String> {
        match kind {
            "Vector3D" => Ok(Self::Vector(vector(data)?)),
            "Matrix3D" if data.is_null() => Ok(Self::Matrix(Matrix3D::IDENTITY)),
            "Matrix3D" => Ok(Self::Matrix(matrix(data)?)),
            _ => Err(format!("Unsupported geometry type '{kind}'")),
        }
    }
    pub fn snapshot(&self) -> Value {
        match self {
            Self::Vector(v) => json!(v),
            Self::Matrix(m) => json!(m.raw_data),
        }
    }
    /// Clone before applying: rejected inputs never partially mutate a value.
    pub fn call(&mut self, operation: &str, args: &Value) -> Result<Value, String> {
        let args = args
            .as_array()
            .ok_or("Geometry call arguments must be an array")?;
        let mut next = self.clone();
        let result = next.apply(operation, args)?;
        match &next {
            Self::Vector(v) if v.iter().any(|v| !v.is_finite()) => {
                return Err("Geometry mutation produced non-finite vector components".into());
            }
            Self::Matrix(m) if m.raw_data.iter().any(|v| !v.is_finite()) => {
                return Err("Geometry mutation produced non-finite matrix components".into());
            }
            _ => {}
        }
        *self = next;
        Ok(result)
    }
    fn apply(&mut self, op: &str, args: &[Value]) -> Result<Value, String> {
        let null = Value::Null;
        let a = |i: usize| args.get(i).unwrap_or(&null);
        match self {
            Self::Vector(v) => {
                let component = |name: &str| match name {
                    "x" => Some(0),
                    "y" => Some(1),
                    "z" => Some(2),
                    "w" => Some(3),
                    _ => None,
                };
                if let Some(i) = component(op) {
                    return numeric(v[i]);
                }
                if let Some(i) = op.strip_prefix("set:").and_then(component) {
                    v[i] = number(a(0))?;
                    return Ok(Value::Null);
                }
                match op {
                    "length" => return numeric(length(*v)),
                    "lengthSquared" => return numeric(dot(*v, *v)),
                    "clone" => return vector_output(*v),
                    "toString" => {
                        return Ok(json!(format!("Vector3D({}, {}, {})", v[0], v[1], v[2])));
                    }
                    "add" | "subtract" | "crossProduct" | "dotProduct" | "distance"
                    | "angleBetween" => {
                        let b = vector(a(0))?;
                        return match op {
                            "add" => vector_output([v[0] + b[0], v[1] + b[1], v[2] + b[2], 0.0]),
                            "subtract" => {
                                vector_output([v[0] - b[0], v[1] - b[1], v[2] - b[2], 0.0])
                            }
                            "crossProduct" => vector_output([
                                v[1] * b[2] - v[2] * b[1],
                                v[2] * b[0] - v[0] * b[2],
                                v[0] * b[1] - v[1] * b[0],
                                1.0,
                            ]),
                            "dotProduct" => numeric(dot(*v, b)),
                            "distance" => {
                                numeric(length([b[0] - v[0], b[1] - v[1], b[2] - v[2], 0.0]))
                            }
                            _ => numeric((dot(*v, b) / (length(*v) * length(b))).acos()),
                        };
                    }
                    "incrementBy" | "decrementBy" | "copyFrom" => {
                        let b = vector(a(0))?;
                        for i in 0..3 {
                            v[i] = match op {
                                "incrementBy" => v[i] + b[i],
                                "decrementBy" => v[i] - b[i],
                                _ => b[i],
                            };
                        }
                    }
                    "equals" => {
                        let b = vector(a(0))?;
                        let four = a(1).as_bool().unwrap_or(false);
                        return Ok(json!((0..if four { 4 } else { 3 }).all(|i| v[i] == b[i])));
                    }
                    "nearEquals" => {
                        let b = vector(a(0))?;
                        let tolerance = number(a(1))?;
                        let four = a(2).as_bool().unwrap_or(false);
                        let xyz = (0..3).all(|i| (v[i] - b[i]).abs() < tolerance);
                        // Preserve Ruffle's documented Flash assignment quirk for allFour.
                        if xyz && four {
                            v[3] = b[3];
                        }
                        return Ok(json!(xyz && (!four || b[3].abs() < tolerance)));
                    }
                    "set:components" => *v = vector(a(0))?,
                    "setTo" => {
                        for (i, item) in v.iter_mut().take(3).enumerate() {
                            *item = number(a(i))?;
                        }
                    }
                    "scaleBy" => {
                        let s = number(a(0))?;
                        for item in v.iter_mut().take(3) {
                            *item *= s;
                        }
                    }
                    "negate" => {
                        for item in v.iter_mut().take(3) {
                            *item = -*item;
                        }
                    }
                    "project" => {
                        if v[3] == 0.0 {
                            return Err("Vector3D.project with w=0 produces non-finite coordinates and is outside the current profile".into());
                        }
                        let w = v[3];
                        for item in v.iter_mut().take(3) {
                            *item /= w;
                        }
                    }
                    "normalize" => {
                        let len = length(*v);
                        if len > 0.0 {
                            for item in v.iter_mut().take(3) {
                                *item /= len;
                            }
                        }
                        return numeric(len);
                    }
                    _ => {
                        return Err(format!(
                            "Vector3D.{op} is unsupported in the current Rust profile"
                        ));
                    }
                }
            }
            Self::Matrix(m) => match op {
                "rawData" => return Ok(json!(m.raw_data)),
                "set:rawData" => *m = matrix(a(0))?,
                "position" => {
                    return vector_output([
                        m.raw_data[12] as f64,
                        m.raw_data[13] as f64,
                        m.raw_data[14] as f64,
                        0.0,
                    ]);
                }
                "set:position" => {
                    let p = vector(a(0))?;
                    for i in 0..3 {
                        m.raw_data[12 + i] = p[i] as f32;
                    }
                }
                "determinant" => return numeric(m.determinant() as f64),
                "clone" => return matrix_output(*m),
                "identity" => *m = Matrix3D::IDENTITY,
                "transpose" => m.transpose_in_place(),
                "invert" => {
                    if let Some(inverse) = m.invert() {
                        *m = inverse;
                        return Ok(json!(true));
                    }
                    return Ok(json!(false));
                }
                "copyFrom" => *m = matrix(a(0))?,
                "append" => *m = matrix(a(0))?.multiply(m),
                "prepend" => *m = m.multiply(&matrix(a(0))?),
                "appendTranslation" => {
                    for i in 0..3 {
                        m.raw_data[12 + i] += number(a(i))? as f32;
                    }
                }
                "prependTranslation" => {
                    *m = m.multiply(&Matrix3D::translate(
                        number(a(0))? as f32,
                        number(a(1))? as f32,
                        number(a(2))? as f32,
                    ))
                }
                "appendScale" | "prependScale" => {
                    let x = number(a(0))? as f32;
                    let y = number(a(1))? as f32;
                    let z = number(a(2))? as f32;
                    if x == 0.0 || y == 0.0 || z == 0.0 {
                        return Err(
                            "Matrix3D scale components must be non-zero (AS3 error 2183)".into(),
                        );
                    }
                    let s = Matrix3D::scale(x, y, z);
                    *m = if op == "appendScale" {
                        s.multiply(m)
                    } else {
                        m.multiply(&s)
                    };
                }
                "appendRotation" | "prependRotation" => {
                    let rotation = rotation(
                        number(a(0))?,
                        vector(a(1))?,
                        if a(2).is_null() {
                            [0.0; 4]
                        } else {
                            vector(a(2))?
                        },
                    );
                    *m = if op == "appendRotation" {
                        rotation.multiply(m)
                    } else {
                        m.multiply(&rotation)
                    };
                }
                "transformVector" | "deltaTransformVector" => {
                    return vector_output(transform(m, vector(a(0))?, op == "transformVector"));
                }
                // Ported from Ruffle Utils3D.as. Scalar projection deliberately
                // uses transformVector's f32 intermediate, then f64 division.
                "projectVector" => {
                    let mut projected = transform(m, vector(a(0))?, true);
                    if projected[3] == 0.0 {
                        return Err("Utils3D.projectVector has zero homogeneous w; non-finite projection is outside the current profile".into());
                    }
                    let w = projected[3];
                    for component in projected.iter_mut().take(3) {
                        *component /= w;
                    }
                    return vector_output(projected);
                }
                // Ruffle's OpenFL-derived bulk projection: retain UV coordinates,
                // replace each third UVT value with inverse homogeneous w.
                "projectVectors" => {
                    let verts = numbers(a(0))?;
                    if verts.len() % 3 != 0 {
                        return Err("Utils3D.projectVectors requires complete xyz triples".into());
                    }
                    let mut projected = numbers(a(1))?;
                    let mut uvts = numbers(a(2))?;
                    uvts.resize(uvts.len().max(verts.len()), 0.0);
                    projected.resize(projected.len().max(verts.len() / 3 * 2), 0.0);
                    let n = m.raw_data.map(|f| f as f64);
                    let mut i = 0;
                    let mut j = 0;
                    while i + 2 < verts.len() {
                        let x = verts[i];
                        let y = verts[i + 1];
                        let z = verts[i + 2];
                        let x1 = x * n[0] + y * n[4] + z * n[8] + n[12];
                        let y1 = x * n[1] + y * n[5] + z * n[9] + n[13];
                        let w1 = x * n[3] + y * n[7] + z * n[11] + n[15];
                        if w1 == 0.0 {
                            return Err("Utils3D.projectVectors has zero homogeneous w; non-finite projection is outside the current profile".into());
                        }
                        projected[j] = x1 / w1;
                        projected[j + 1] = y1 / w1;
                        uvts[i + 2] = 1.0 / w1;
                        i += 3;
                        j += 2;
                    }
                    if projected.iter().chain(uvts.iter()).any(|v| !v.is_finite()) {
                        return Err("Utils3D.projectVectors produced non-finite output".into());
                    }
                    return Ok(json!({"projected":projected,"uvts":uvts}));
                }
                "transformVectors" => {
                    let input = numbers(a(0))?;
                    let mut output = numbers(a(1))?;
                    let count = input.len() / 3 * 3;
                    output.resize(output.len().max(count), 0.0);
                    let mr = m.raw_data.map(|f| f as f64);
                    for i in (0..count).step_by(3) {
                        let x = input[i];
                        let y = input[i + 1];
                        let z = input[i + 2];
                        for row in 0..3 {
                            output[i + row] =
                                mr[row] * x + mr[4 + row] * y + mr[8 + row] * z + mr[12 + row];
                        }
                    }
                    if output.iter().any(|v| !v.is_finite()) {
                        return Err("Matrix3D.transformVectors produced non-finite output".into());
                    }
                    return Ok(json!(output));
                }
                "copyColumnFrom" | "copyRowFrom" | "copyColumnTo" | "copyRowTo" => {
                    let index = number(a(0))?;
                    if index.fract() != 0.0 || !(0.0..=3.0).contains(&index) {
                        return Err("Matrix3D row/column index must be 0 through 3".into());
                    }
                    let index = index as usize;
                    let column = op.contains("Column");
                    if op.ends_with("From") {
                        let p = vector(a(1))?;
                        for (i, item) in p.iter().enumerate() {
                            m.raw_data[if column { index * 4 + i } else { i * 4 + index }] =
                                *item as f32;
                        }
                    } else {
                        return vector_output(std::array::from_fn(|i| {
                            m.raw_data[if column { index * 4 + i } else { i * 4 + index }] as f64
                        }));
                    }
                }
                "copyRawDataFrom" | "copyRawDataTo" => {
                    let mut values = numbers(a(0))?;
                    let index = number(a(1))?;
                    if index.fract() != 0.0 || !(0.0..=12272.0).contains(&index) {
                        return Err("Matrix3D raw-data index must be an integer in 0..12272".into());
                    }
                    let index = index as usize;
                    let transpose = a(2).as_bool().unwrap_or(false);
                    if op == "copyRawDataFrom" {
                        if values.len() < index + 16 {
                            return Err(
                                "Matrix3D.copyRawDataFrom requires sixteen available values".into(),
                            );
                        }
                        *m = matrix(&json!(&values[index..index + 16]))?;
                        if transpose {
                            m.transpose_in_place();
                        }
                    } else {
                        values.resize(values.len().max(index + 16), 0.0);
                        let mut copied = *m;
                        if transpose {
                            copied.transpose_in_place();
                        }
                        for i in 0..16 {
                            values[index + i] = copied.raw_data[i] as f64;
                        }
                        return Ok(json!(values));
                    }
                }
                _ => {
                    return Err(format!(
                        "Matrix3D.{op} is unsupported in the current Rust profile; decomposition, recomposition, interpolation and pointAt remain pending"
                    ));
                }
            },
        }
        Ok(Value::Null)
    }
}

// Adapted from Ruffle's AVM2 transform_vector/delta_transform_vector: f32 math.
fn transform(m: &Matrix3D, v: [f64; 4], translated: bool) -> [f64; 4] {
    let [x, y, z, _] = v.map(|f| f as f32);
    let r = m.raw_data;
    std::array::from_fn(|i| {
        (r[i] * x + r[4 + i] * y + r[8 + i] * z + if translated { r[12 + i] } else { 0.0 }) as f64
    })
}

// Adapted verbatim in algebra/order from Ruffle append_rotation (OpenFL-derived).
fn rotation(degrees: f64, axis: [f64; 4], pivot: [f64; 4]) -> Matrix3D {
    let [tx, ty, tz, _] = pivot;
    let radian = degrees * std::f64::consts::PI / 180.0;
    let cos = radian.cos();
    let sin = radian.sin();
    let [mut x, mut y, mut z, _] = axis;
    let mut x2 = x * x;
    let mut y2 = y * y;
    let mut z2 = z * z;
    let ls = x2 + y2 + z2;
    if ls != 0.0 {
        let l = ls.sqrt();
        x /= l;
        y /= l;
        z /= l;
        x2 /= ls;
        y2 /= ls;
        z2 /= ls;
    }
    let ccos = 1.0 - cos;
    Matrix3D {
        raw_data: [
            x2 + (y2 + z2) * cos,
            x * y * ccos + z * sin,
            x * z * ccos - y * sin,
            0.0,
            x * y * ccos - z * sin,
            y2 + (x2 + z2) * cos,
            y * z * ccos + x * sin,
            0.0,
            x * z * ccos + y * sin,
            y * z * ccos - x * sin,
            z2 + (x2 + y2) * cos,
            0.0,
            (tx * (y2 + z2) - x * (ty * y + tz * z)) * ccos + (ty * z - tz * y) * sin,
            (ty * (x2 + z2) - y * (tx * x + tz * z)) * ccos + (tz * x - tx * z) * sin,
            (tz * (x2 + y2) - z * (tx * x + ty * y)) * ccos + (tx * y - ty * x) * sin,
            1.0,
        ]
        .map(|f| f as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapted_geometry_family() {
        let mut v = GeometryValue::new("Vector3D", &json!([3, 4, 0, 7])).unwrap();
        assert_eq!(v.call("normalize", &json!([])).unwrap(), json!(5.0));
        assert_eq!(v.call("w", &json!([])).unwrap(), json!(7.0));
        let cross = v.call("crossProduct", &json!([[0, 0, 1, 0]])).unwrap();
        assert_eq!(cross["data"][3], json!(1.0));
        let mut m = GeometryValue::new("Matrix3D", &Value::Null).unwrap();
        m.call("appendScale", &json!([2, 3, 4])).unwrap();
        m.call("appendTranslation", &json!([10, 20, 30])).unwrap();
        let transformed = m.call("transformVector", &json!([[1, 2, 3, 99]])).unwrap();
        assert_eq!(transformed["data"], json!([12.0, 26.0, 42.0, 1.0]));
        assert_eq!(m.call("determinant", &json!([])).unwrap(), json!(24.0));
        assert_eq!(m.call("invert", &json!([])).unwrap(), json!(true));
        let original = m
            .call("transformVector", &json!([transformed["data"]]))
            .unwrap();
        for (actual, expected) in original["data"]
            .as_array()
            .unwrap()
            .iter()
            .zip([1.0, 2.0, 3.0, 1.0])
        {
            assert!((actual.as_f64().unwrap() - expected).abs() < 0.00001);
        }
        let before = m.snapshot();
        assert!(m.call("appendScale", &json!([0, 1, 1])).is_err());
        assert_eq!(m.snapshot(), before);
        m.call("identity", &json!([])).unwrap();
        m.call("appendRotation", &json!([90, [0, 0, 1, 0], null]))
            .unwrap();
        let rotated = m.call("transformVector", &json!([[1, 0, 0, 0]])).unwrap();
        assert!((rotated["data"][1].as_f64().unwrap() - 1.0).abs() < 0.00001);
        m.call("identity", &json!([])).unwrap();
        let mut raw = m.snapshot().as_array().unwrap().clone();
        raw[11] = json!(0.25);
        m.call("set:rawData", &json!([raw])).unwrap();
        let projected = m.call("projectVector", &json!([[8, 4, 4, 0]])).unwrap();
        assert_eq!(projected["data"], json!([4.0, 2.0, 2.0, 2.0]));
        let projected = m
            .call(
                "projectVectors",
                &json!([[8, 4, 4, 6, 3, 8], [], [0.1, 0.2, 0, 0.3, 0.4, 0]]),
            )
            .unwrap();
        assert_eq!(projected["projected"], json!([4.0, 2.0, 2.0, 1.0]));
        assert_eq!(projected["uvts"][0], json!(0.1));
        assert_eq!(projected["uvts"][2], json!(0.5));
        assert!((projected["uvts"][5].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-10);
        assert!(
            m.call("decompose", &json!([]))
                .unwrap_err()
                .contains("unsupported")
        );
    }
}
