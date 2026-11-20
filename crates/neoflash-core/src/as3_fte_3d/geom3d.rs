use super::*;

// ============================================================================
// 4. 3D DisplayObject & Transform Algebra + Top-Level Fte3dRuntime
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display3dPose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub scale_z: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
}

impl Default for Display3dPose {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
        }
    }
}

pub fn compose_display_matrix3d(pose: &Display3dPose) -> [f64; 16] {
    let rx = pose.rotation_x.to_radians();
    let ry = pose.rotation_y.to_radians();
    let rz = pose.rotation_z.to_radians();
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    let b00 = cz * cy;
    let b01 = sz * cy;
    let b02 = -sy;
    let b10 = cz * sy * sx - sz * cx;
    let b11 = sz * sy * sx + cz * cx;
    let b12 = cy * sx;
    let b20 = cz * sy * cx + sz * sx;
    let b21 = sz * sy * cx - cz * sx;
    let b22 = cy * cx;
    [
        b00 * pose.scale_x,
        b01 * pose.scale_x,
        b02 * pose.scale_x,
        0.0,
        b10 * pose.scale_y,
        b11 * pose.scale_y,
        b12 * pose.scale_y,
        0.0,
        b20 * pose.scale_z,
        b21 * pose.scale_z,
        b22 * pose.scale_z,
        0.0,
        pose.x,
        pose.y,
        pose.z,
        1.0,
    ]
}

pub fn decompose_display_matrix3d(raw: &[f64; 16]) -> Display3dPose {
    let m = crate::as3_geom::Matrix3D {
        raw_data: raw.map(|v| v as f32),
    };
    let mut gv = crate::as3_geom::GeometryValue::Matrix(m);
    if let Ok(val) = gv.call("decompose", &serde_json::json!(["eulerAngles"]))
        && let Some(arr) = val.get("data").and_then(|d| d.as_array())
        && arr.len() == 3
    {
        let p = arr[0].as_array();
        let r = arr[1].as_array();
        let s = arr[2].as_array();
        if let (Some(p), Some(r), Some(s)) = (p, r, s) {
            let g = |a: &[serde_json::Value], i: usize, def: f64| {
                a.get(i).and_then(|v| v.as_f64()).unwrap_or(def)
            };
            return Display3dPose {
                x: g(p, 0, raw[12]),
                y: g(p, 1, raw[13]),
                z: g(p, 2, raw[14]),
                rotation_x: g(r, 0, 0.0).to_degrees(),
                rotation_y: g(r, 1, 0.0).to_degrees(),
                rotation_z: g(r, 2, 0.0).to_degrees(),
                scale_x: g(s, 0, 1.0),
                scale_y: g(s, 1, 1.0),
                scale_z: g(s, 2, 1.0),
            };
        }
    }
    Display3dPose {
        x: raw[12],
        y: raw[13],
        z: raw[14],
        ..Display3dPose::default()
    }
}

pub fn multiply_matrix3d_chain(chain_root_to_leaf: &[[f64; 16]]) -> [f64; 16] {
    let mut acc = crate::as3_geom::Matrix3D::IDENTITY;
    for raw in chain_root_to_leaf {
        let next = crate::as3_geom::Matrix3D {
            raw_data: raw.map(|v| v as f32),
        };
        acc = acc.multiply(&next);
    }
    acc.raw_data.map(|v| v as f64)
}

pub fn relative_matrix_3d(
    target_chain_root_to_leaf: &[[f64; 16]],
    relative_to_chain_root_to_leaf: &[[f64; 16]],
) -> [f64; 16] {
    let target_world = crate::as3_geom::Matrix3D {
        raw_data: multiply_matrix3d_chain(target_chain_root_to_leaf).map(|v| v as f32),
    };
    let rel_world = crate::as3_geom::Matrix3D {
        raw_data: multiply_matrix3d_chain(relative_to_chain_root_to_leaf).map(|v| v as f32),
    };
    let rel_inv = rel_world
        .invert()
        .unwrap_or(crate::as3_geom::Matrix3D::IDENTITY);
    rel_inv.multiply(&target_world).raw_data.map(|v| v as f64)
}

pub fn local3d_to_global(
    chain_root_to_leaf: &[[f64; 16]],
    point3d: [f64; 3],
    focal_length: f64,
    projection_center: [f64; 2],
) -> [f64; 2] {
    let m = multiply_matrix3d_chain(chain_root_to_leaf);
    let [lx, ly, lz] = point3d;
    let wx = m[0] * lx + m[4] * ly + m[8] * lz + m[12];
    let wy = m[1] * lx + m[5] * ly + m[9] * lz + m[13];
    let wz = m[2] * lx + m[6] * ly + m[10] * lz + m[14];
    if focal_length > 1e-6 {
        let denom = focal_length + wz;
        let scale = if denom.abs() > 1e-6 {
            focal_length / denom
        } else {
            1.0
        };
        [
            projection_center[0] + (wx - projection_center[0]) * scale,
            projection_center[1] + (wy - projection_center[1]) * scale,
        ]
    } else {
        [wx, wy]
    }
}

pub fn global_to_local3d(
    chain_root_to_leaf: &[[f64; 16]],
    point2d: [f64; 2],
    focal_length: f64,
    projection_center: [f64; 2],
) -> [f64; 3] {
    let m = multiply_matrix3d_chain(chain_root_to_leaf);
    let [sx, sy] = point2d;
    let [cx, cy] = projection_center;
    if focal_length > 1e-6 {
        // Intersect perspective ray through (sx, sy) with local z = 0 plane:
        // wx - cx = u * (f + wz), wy - cy = v * (f + wz), where u = (sx - cx)/f, v = (sy - cy)/f.
        let u = (sx - cx) / focal_length;
        let v = (sy - cy) / focal_length;
        let a11 = m[0] - u * m[2];
        let a12 = m[4] - u * m[6];
        let b1 = cx + u * (focal_length + m[14]) - m[12];
        let a21 = m[1] - v * m[2];
        let a22 = m[5] - v * m[6];
        let b2 = cy + v * (focal_length + m[14]) - m[13];
        let det = a11 * a22 - a12 * a21;
        if det.abs() > 1e-9 {
            let lx = (b1 * a22 - a12 * b2) / det;
            let ly = (a11 * b2 - b1 * a21) / det;
            return [lx, ly, 0.0];
        }
    }
    let world = crate::as3_geom::Matrix3D {
        raw_data: m.map(|v| v as f32),
    };
    if let Some(inv) = world.invert() {
        let r = inv.raw_data.map(|v| v as f64);
        [
            r[0] * sx + r[4] * sy + r[12],
            r[1] * sx + r[5] * sy + r[13],
            r[2] * sx + r[6] * sy + r[14],
        ]
    } else {
        [sx - m[12], sy - m[13], 0.0]
    }
}
