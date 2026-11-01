//! Bounded AS3 geometry values owned and calculated in Rust.
//! Adapted from Ruffle 07d3e430631333f80f754df01a968bdcd80d98c5.
//! Matrix algorithms copied from render/src/matrix3d.rs; AVM2 wrappers removed.
//! Vector3D algorithms translated from core/src/avm2/globals/flash/geom/Vector3D.as.
//! Copyright (c) 2018-2022 Ruffle LLC and Ruffle contributors; MIT selected.
//! Rotation and bulk-projection algorithms additionally Copyright (c) 2013-2021 Joshua Granick and
//! other OpenFL contributors (MIT), through Ruffle's documented adaptation.
//! See docs/AS3-REUSE-PROVENANCE.md for pinned sources, modifications and notices.
//! Standalone PerspectiveProjection additionally adapts render/src/perspective_projection.rs;
//! see docs/RUNTIME-GEOMETRY-PROVENANCE.md for its bounded profile.
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
    /// Standalone projection: field of view followed by center x/y. Width is 500.
    Projection([f64; 3]),
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
            "PerspectiveProjection" if data.is_null() => Ok(Self::Projection([55., 250., 250.])),
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
            Self::Projection(p) => json!(p),
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
            Self::Projection(p) if p.iter().any(|v| !v.is_finite()) => {
                return Err("PerspectiveProjection mutation produced non-finite values".into());
            }
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
            Self::Projection(p) => {
                // Adapted from Ruffle render/src/perspective_projection.rs and AVM2
                // accessors. Unbound objects use width=500; no display association.
                match op {
                    "fieldOfView" => return numeric(p[0]),
                    "set:fieldOfView" => {
                        let fov = number(a(0))?;
                        if fov <= 0. || fov >= 180. {
                            return Err("PerspectiveProjection.fieldOfView requires 0 < value < 180 degrees".into());
                        }
                        p[0] = fov;
                    }
                    "focalLength" => return numeric(projection_focal_length(p[0]) as f64),
                    "set:focalLength" => {
                        let focal = number(a(0))?;
                        if focal <= 0. {
                            return Err("PerspectiveProjection.focalLength must be positive".into());
                        }
                        let fov = (250. / focal).atan().to_degrees() * 2.;
                        if fov <= 0. || fov >= 180. {
                            return Err("PerspectiveProjection focal length exceeds the finite projection profile".into());
                        }
                        p[0] = fov;
                    }
                    "projectionCenter" => return Ok(json!({"type":"Point","data":[p[1],p[2]]})),
                    "set:projectionCenter" => {
                        let center = numbers(a(0))?;
                        if center.len() != 2 {
                            return Err(
                                "PerspectiveProjection.projectionCenter requires a Point".into()
                            );
                        }
                        p[1] = center[0];
                        p[2] = center[1];
                    }
                    "toMatrix3D" => {
                        let f = projection_focal_length(p[0]);
                        return matrix_output(Matrix3D {
                            raw_data: [
                                f, 0., 0., 0., 0., f, 0., 0., 0., 0., 1., 1., 0., 0., 0., 0.,
                            ],
                        });
                    }
                    _ => {
                        return Err(format!(
                            "PerspectiveProjection.{op} is outside the standalone projection profile"
                        ));
                    }
                }
            }
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
                    for (i, value) in p.iter().take(3).enumerate() {
                        m.raw_data[12 + i] = *value as f32;
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
                "decompose" => {
                    let style = if a(0).is_null() {
                        "eulerAngles"
                    } else {
                        a(0).as_str()
                            .ok_or("Matrix3D.decompose orientationStyle must be a String")?
                    };
                    let [pos, rot, scale] = decompose_matrix(m, style)?;
                    return Ok(json!({
                        "type": "Vector3DList",
                        "data": [pos, rot, scale]
                    }));
                }
                "recompose" => {
                    let pos = vector(a(0))?;
                    let rot = vector(a(1))?;
                    let scale = vector(a(2))?;
                    let style = if a(3).is_null() {
                        "eulerAngles"
                    } else {
                        a(3).as_str()
                            .ok_or("Matrix3D.recompose orientationStyle must be a String")?
                    };
                    if let Some(recomposed) = recompose_matrix(pos, rot, scale, style)? {
                        *m = recomposed;
                        return Ok(json!(true));
                    }
                    return Ok(json!(false));
                }
                "interpolate" => {
                    let target = matrix(a(0))?;
                    let percent = number(a(1))?;
                    return matrix_output(interpolate_matrices(m, &target, percent));
                }
                "interpolateTo" => {
                    let target = matrix(a(0))?;
                    let percent = number(a(1))?;
                    *m = interpolate_matrices(m, &target, percent);
                }
                "pointAt" => {
                    let pos = vector(a(0))?;
                    let at = if a(1).is_null() {
                        [0.0, 0.0, -1.0, 0.0]
                    } else {
                        vector(a(1))?
                    };
                    let up = if a(2).is_null() {
                        [0.0, -1.0, 0.0, 0.0]
                    } else {
                        vector(a(2))?
                    };
                    *m = point_at_matrix(pos, at, up);
                }
                "pointTowards" => {
                    let percent = number(a(0))?;
                    let pos = vector(a(1))?;
                    let at = if a(2).is_null() {
                        [0.0, 0.0, -1.0, 0.0]
                    } else {
                        vector(a(2))?
                    };
                    let up = if a(3).is_null() {
                        [0.0, -1.0, 0.0, 0.0]
                    } else {
                        vector(a(3))?
                    };
                    let target = point_at_matrix(pos, at, up);
                    return matrix_output(interpolate_matrices(m, &target, percent));
                }
                _ => {
                    return Err(format!(
                        "Matrix3D.{op} is unsupported in the current Rust profile"
                    ));
                }
            },
        }
        Ok(Value::Null)
    }
}

// Ruffle preserves an f64 angle but returns f32 focal length/matrix components.
fn projection_focal_length(field_of_view: f64) -> f32 {
    250. * ((std::f64::consts::PI - field_of_view.to_radians()) / 2.).tan() as f32
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

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn normalize3(v: [f64; 3]) -> Option<[f64; 3]> {
    let len = norm3(v);
    (len > 1e-12).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

fn matrix_to_quaternion(r: [[f64; 3]; 3]) -> [f64; 4] {
    // r[col][row] holds orthonormal basis columns X, Y, Z.
    let (r00, r10, r20) = (r[0][0], r[0][1], r[0][2]);
    let (r01, r11, r21) = (r[1][0], r[1][1], r[1][2]);
    let (r02, r12, r22) = (r[2][0], r[2][1], r[2][2]);
    let trace = r00 + r11 + r22;
    let mut q = if trace > 0.0 {
        let s = (trace + 1.0).max(0.0).sqrt() * 2.0;
        [(r21 - r12) / s, (r02 - r20) / s, (r10 - r01) / s, 0.25 * s]
    } else if r00 > r11 && r00 > r22 {
        let s = (1.0 + r00 - r11 - r22).max(0.0).sqrt() * 2.0;
        [0.25 * s, (r01 + r10) / s, (r02 + r20) / s, (r21 - r12) / s]
    } else if r11 > r22 {
        let s = (1.0 + r11 - r00 - r22).max(0.0).sqrt() * 2.0;
        [(r01 + r10) / s, 0.25 * s, (r12 + r21) / s, (r02 - r20) / s]
    } else {
        let s = (1.0 + r22 - r00 - r11).max(0.0).sqrt() * 2.0;
        [(r02 + r20) / s, (r12 + r21) / s, 0.25 * s, (r10 - r01) / s]
    };
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 1e-12 {
        for item in &mut q {
            *item /= n;
        }
    } else {
        q = [0.0, 0.0, 0.0, 1.0];
    }
    q
}

fn quaternion_to_basis(q: [f64; 4]) -> [[f64; 3]; 3] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let [x, y, z, w] = if n > 1e-12 {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn decompose_matrix(m: &Matrix3D, style: &str) -> Result<[[f64; 4]; 3], String> {
    if !matches!(style, "eulerAngles" | "axisAngle" | "quaternion") {
        return Err(format!(
            "Invalid Orientation3D style '{style}'; expected eulerAngles, axisAngle or quaternion"
        ));
    }
    let d = m.raw_data.map(|v| v as f64);
    let pos = [d[12], d[13], d[14], 0.0];
    let mut c0 = [d[0], d[1], d[2]];
    let mut c1 = [d[4], d[5], d[6]];
    let mut c2 = [d[8], d[9], d[10]];
    let sx = norm3(c0);
    let sy = norm3(c1);
    let mut sz = norm3(c2);
    let det3 = c0[0] * (c1[1] * c2[2] - c2[1] * c1[2]) - c1[0] * (c0[1] * c2[2] - c2[1] * c0[2])
        + c2[0] * (c0[1] * c1[2] - c1[1] * c0[2]);
    if det3 < 0.0 {
        sz = -sz;
    }
    if sx != 0.0 {
        c0 = [c0[0] / sx, c0[1] / sx, c0[2] / sx];
    }
    if sy != 0.0 {
        c1 = [c1[0] / sy, c1[1] / sy, c1[2] / sy];
    }
    if sz != 0.0 {
        c2 = [c2[0] / sz, c2[1] / sz, c2[2] / sz];
    }
    let basis = [c0, c1, c2];
    let rot = match style {
        "eulerAngles" => {
            let ry = (-c0[2]).clamp(-1.0, 1.0).asin();
            let cy = ry.cos();
            let (rx, rz) = if cy.abs() > 1e-6 {
                (c1[2].atan2(c2[2]), c0[1].atan2(c0[0]))
            } else {
                ((-c2[1]).atan2(c1[1]), 0.0)
            };
            [rx, ry, rz, 0.0]
        }
        "quaternion" => matrix_to_quaternion(basis),
        "axisAngle" => {
            let mut q = matrix_to_quaternion(basis);
            if q[3] < 0.0 {
                q = [-q[0], -q[1], -q[2], -q[3]];
            }
            let angle = 2.0 * q[3].clamp(-1.0, 1.0).acos();
            let sin_half = (1.0 - q[3] * q[3]).max(0.0).sqrt();
            if sin_half > 1e-6 {
                [q[0] / sin_half, q[1] / sin_half, q[2] / sin_half, angle]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        }
        _ => unreachable!(),
    };
    let scale = [sx, sy, sz, 0.0];
    if [pos, rot, scale].iter().flatten().any(|v| !v.is_finite()) {
        return Err("Matrix3D.decompose produced non-finite components".into());
    }
    Ok([pos, rot, scale])
}

fn recompose_matrix(
    pos: [f64; 4],
    rot: [f64; 4],
    scale: [f64; 4],
    style: &str,
) -> Result<Option<Matrix3D>, String> {
    if !matches!(style, "eulerAngles" | "axisAngle" | "quaternion") {
        return Err(format!(
            "Invalid Orientation3D style '{style}'; expected eulerAngles, axisAngle or quaternion"
        ));
    }
    if scale[0] == 0.0 || scale[1] == 0.0 || scale[2] == 0.0 {
        return Ok(None);
    }
    let basis = match style {
        "eulerAngles" => {
            let (sx, cx) = rot[0].sin_cos();
            let (sy, cy) = rot[1].sin_cos();
            let (sz, cz) = rot[2].sin_cos();
            [
                [cz * cy, sz * cy, -sy],
                [cz * sy * sx - sz * cx, sz * sy * sx + cz * cx, cy * sx],
                [cz * sy * cx + sz * sx, sz * sy * cx - cz * sx, cy * cx],
            ]
        }
        "quaternion" => quaternion_to_basis(rot),
        "axisAngle" => {
            let half = rot[3] * 0.5;
            let (sh, ch) = half.sin_cos();
            let q = if let Some([ax, ay, az]) = normalize3([rot[0], rot[1], rot[2]]) {
                [ax * sh, ay * sh, az * sh, ch]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            quaternion_to_basis(q)
        }
        _ => unreachable!(),
    };
    let raw = [
        basis[0][0] * scale[0],
        basis[0][1] * scale[0],
        basis[0][2] * scale[0],
        0.0,
        basis[1][0] * scale[1],
        basis[1][1] * scale[1],
        basis[1][2] * scale[1],
        0.0,
        basis[2][0] * scale[2],
        basis[2][1] * scale[2],
        basis[2][2] * scale[2],
        0.0,
        pos[0],
        pos[1],
        pos[2],
        1.0,
    ]
    .map(|v| v as f32);
    if raw.iter().any(|v| !v.is_finite()) {
        return Err("Matrix3D.recompose produced non-finite matrix components".into());
    }
    Ok(Some(Matrix3D { raw_data: raw }))
}

fn slerp_quat(a: [f64; 4], mut b: [f64; 4], t: f64) -> [f64; 4] {
    let mut cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if cos < 0.0 {
        cos = -cos;
        b = [-b[0], -b[1], -b[2], -b[3]];
    }
    let (w0, w1) = if cos > 0.9995 {
        (1.0 - t, t)
    } else {
        let omega = cos.clamp(-1.0, 1.0).acos();
        let sin = omega.sin();
        (((1.0 - t) * omega).sin() / sin, (t * omega).sin() / sin)
    };
    let mut out = [
        a[0] * w0 + b[0] * w1,
        a[1] * w0 + b[1] * w1,
        a[2] * w0 + b[2] * w1,
        a[3] * w0 + b[3] * w1,
    ];
    let n = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2] + out[3] * out[3]).sqrt();
    if n > 1e-12 {
        for item in &mut out {
            *item /= n;
        }
    }
    out
}

fn interpolate_matrices(from: &Matrix3D, to: &Matrix3D, percent: f64) -> Matrix3D {
    if let (Ok([p0, q0, s0]), Ok([p1, q1, s1])) = (
        decompose_matrix(from, "quaternion"),
        decompose_matrix(to, "quaternion"),
    ) {
        let pos = [
            p0[0] + (p1[0] - p0[0]) * percent,
            p0[1] + (p1[1] - p0[1]) * percent,
            p0[2] + (p1[2] - p0[2]) * percent,
            0.0,
        ];
        let scale = [
            s0[0] + (s1[0] - s0[0]) * percent,
            s0[1] + (s1[1] - s0[1]) * percent,
            s0[2] + (s1[2] - s0[2]) * percent,
            0.0,
        ];
        let quat = slerp_quat(q0, q1, percent);
        if let Ok(Some(recomposed)) = recompose_matrix(pos, quat, scale, "quaternion") {
            return recomposed;
        }
    }
    Matrix3D {
        raw_data: std::array::from_fn(|i| {
            let a = from.raw_data[i] as f64;
            let b = to.raw_data[i] as f64;
            (a + (b - a) * percent) as f32
        }),
    }
}

fn point_at_matrix(pos: [f64; 4], at: [f64; 4], up: [f64; 4]) -> Matrix3D {
    let z_axis =
        normalize3([pos[0] - at[0], pos[1] - at[1], pos[2] - at[2]]).unwrap_or([0.0, 0.0, 1.0]);
    let up_neg = normalize3([-up[0], -up[1], -up[2]]).unwrap_or([0.0, 1.0, 0.0]);
    let x_axis = normalize3(cross3(up_neg, z_axis)).unwrap_or_else(|| {
        let fallback = if z_axis[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        normalize3(cross3(fallback, z_axis)).unwrap_or([1.0, 0.0, 0.0])
    });
    let y_axis = normalize3(cross3(z_axis, x_axis)).unwrap_or([0.0, 1.0, 0.0]);
    Matrix3D {
        raw_data: [
            x_axis[0], x_axis[1], x_axis[2], 0.0, //
            y_axis[0], y_axis[1], y_axis[2], 0.0, //
            z_axis[0], z_axis[1], z_axis[2], 0.0, //
            pos[0], pos[1], pos[2], 1.0,
        ]
        .map(|v| v as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapted_geometry_family() {
        let mut p = GeometryValue::new("PerspectiveProjection", &Value::Null).unwrap();
        p.call("set:fieldOfView", &json!([90])).unwrap();
        assert_eq!(p.call("focalLength", &json!([])).unwrap(), json!(250.));
        p.call("set:projectionCenter", &json!([[25, 75]])).unwrap();
        assert_eq!(
            p.call("projectionCenter", &json!([])).unwrap()["data"],
            json!([25., 75.])
        );
        let projection = p.call("toMatrix3D", &json!([])).unwrap();
        assert_eq!(projection["data"][11], json!(1.));
        assert_eq!(projection["data"][15], json!(0.));
        p.call("set:focalLength", &json!([250])).unwrap();
        assert!(
            (p.call("fieldOfView", &json!([])).unwrap().as_f64().unwrap() - 90.).abs() < 0.0001
        );
        let before = p.snapshot();
        assert!(p.call("set:fieldOfView", &json!([180])).is_err());
        assert_eq!(p.snapshot(), before);
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
        m.call("identity", &json!([])).unwrap();
        m.call("appendScale", &json!([2, 3, 4])).unwrap();
        m.call("appendRotation", &json!([30, [1, 0, 0, 0], null]))
            .unwrap();
        m.call("appendRotation", &json!([20, [0, 1, 0, 0], null]))
            .unwrap();
        m.call("appendRotation", &json!([45, [0, 0, 1, 0], null]))
            .unwrap();
        m.call("appendTranslation", &json!([10, 20, 30])).unwrap();
        for style in ["eulerAngles", "axisAngle", "quaternion"] {
            let parts = m.call("decompose", &json!([style])).unwrap();
            let mut copy = GeometryValue::new("Matrix3D", &Value::Null).unwrap();
            assert_eq!(
                copy.call(
                    "recompose",
                    &json!([parts["data"][0], parts["data"][1], parts["data"][2], style])
                )
                .unwrap(),
                json!(true)
            );
            for (a, b) in m
                .snapshot()
                .as_array()
                .unwrap()
                .iter()
                .zip(copy.snapshot().as_array().unwrap())
            {
                assert!(
                    (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-4,
                    "style {style}"
                );
            }
        }
        let mut ident = GeometryValue::new("Matrix3D", &Value::Null).unwrap();
        ident
            .call("pointAt", &json!([[0, 0, 0, 0], null, null]))
            .unwrap();
        assert_eq!(ident.snapshot(), json!(Matrix3D::IDENTITY.raw_data));
        let mid = ident
            .call("interpolate", &json!([m.snapshot(), 0.5]))
            .unwrap();
        assert_eq!(mid["data"][12], json!(5.0));
        assert_eq!(mid["data"][13], json!(10.0));
        assert_eq!(mid["data"][14], json!(15.0));
    }
}
