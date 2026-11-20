use super::stage3d::{SamplerState, TextureRecord};
use super::*;

// ============================================================================
// 2. Stage3D Engine (Context3D, AGAL v1/v2 VM & Software Rasterizer)
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub(super) struct AgalInst {
    pub(super) opcode: u8,
    pub(super) dst_reg: u16,
    pub(super) dst_mask: u8,
    pub(super) dst_type: u8,
    pub(super) src1: u64,
    pub(super) src2: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) enum CompiledShader {
    #[default]
    Fallback,
    Agal(Vec<AgalInst>),
}

pub(super) fn parse_agal(bytes: &[u8], expected_type: u8) -> Result<CompiledShader, Fte3dError> {
    if bytes.is_empty() {
        return Err(Fte3dError::Argument(
            "Program3D.upload requires non-empty vertex and fragment ByteArrays.".into(),
        ));
    }
    if bytes.len() >= 7 && bytes[0] == 0xa0 && bytes[5] == 0xa1 {
        let ver = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        if ver == 0 || ver > 4 {
            return Err(Fte3dError::Argument(format!(
                "Unsupported AGAL version {ver}."
            )));
        }
        let prog_type = bytes[6];
        if prog_type != expected_type {
            return Err(Fte3dError::Argument(format!(
                "AGAL program type mismatch: expected {expected_type}, got {prog_type}."
            )));
        }
        let body = &bytes[7..];
        if !body.len().is_multiple_of(24) {
            return Err(Fte3dError::Argument(
                "AGAL instruction stream must be a multiple of 24 bytes.".into(),
            ));
        }
        let mut insts = Vec::with_capacity(body.len() / 24);
        for chunk in body.chunks_exact(24) {
            let opcode = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as u8;
            let dst = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let src1 = u64::from_le_bytes([
                chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14],
                chunk[15],
            ]);
            let src2 = u64::from_le_bytes([
                chunk[16], chunk[17], chunk[18], chunk[19], chunk[20], chunk[21], chunk[22],
                chunk[23],
            ]);
            insts.push(AgalInst {
                opcode,
                dst_reg: (dst & 0xffff) as u16,
                dst_mask: ((dst >> 16) & 0x0f) as u8,
                dst_type: ((dst >> 24) & 0x0f) as u8,
                src1,
                src2,
            });
        }
        Ok(CompiledShader::Agal(insts))
    } else {
        Ok(CompiledShader::Fallback)
    }
}

pub fn validate_stage3d_enum<'a>(kind: &str, value: &'a str) -> Result<&'a str, Fte3dError> {
    let ok = match kind {
        "profile" => matches!(
            value,
            "baseline"
                | "baselineConstrained"
                | "baselineExtended"
                | "standard"
                | "standardConstrained"
                | "standardExtended"
                | "enhanced"
        ),
        "renderMode" => matches!(value, "auto" | "software"),
        "bufferUsage" => matches!(value, "staticDraw" | "dynamicDraw"),
        "programType" => matches!(value, "vertex" | "fragment"),
        "vertexFormat" => matches!(value, "float1" | "float2" | "float3" | "float4" | "bytes4"),
        "textureFormat" => matches!(
            value,
            "bgra"
                | "bgraPacked4444"
                | "bgrPacked565"
                | "compressed"
                | "compressedAlpha"
                | "rgbaHalfFloat"
        ),
        "blendFactor" => matches!(
            value,
            "one"
                | "zero"
                | "sourceAlpha"
                | "sourceColor"
                | "destinationAlpha"
                | "destinationColor"
                | "oneMinusSourceAlpha"
                | "oneMinusSourceColor"
                | "oneMinusDestinationAlpha"
                | "oneMinusDestinationColor"
        ),
        "compareMode" => matches!(
            value,
            "always"
                | "never"
                | "less"
                | "lessEqual"
                | "equal"
                | "greaterEqual"
                | "greater"
                | "notEqual"
        ),
        "triangleFace" => matches!(value, "none" | "back" | "front" | "frontAndBack"),
        "stencilAction" => matches!(
            value,
            "keep"
                | "zero"
                | "incrementSaturate"
                | "decrementSaturate"
                | "invert"
                | "incrementWrap"
                | "decrementWrap"
                | "set"
        ),
        "wrapMode" => matches!(
            value,
            "clamp" | "repeat" | "clamp_u_repeat_v" | "repeat_u_clamp_v"
        ),
        "textureFilter" => matches!(
            value,
            "nearest"
                | "linear"
                | "anisotropic2x"
                | "anisotropic4x"
                | "anisotropic8x"
                | "anisotropic16x"
        ),
        "mipFilter" => matches!(value, "mipnone" | "mipnearest" | "miplinear"),
        "fillMode" => matches!(value, "solid" | "wireframe"),
        _ => false,
    };
    if ok {
        Ok(value)
    } else {
        Err(Fte3dError::Argument(format!(
            "Invalid Stage3D {kind}: '{value}'."
        )))
    }
}

pub(super) fn compare_pass(mode: &str, src: f32, dst: f32) -> bool {
    match mode {
        "always" => true,
        "never" => false,
        "less" => src < dst,
        "lessEqual" => src <= dst,
        "equal" => (src - dst).abs() <= 1e-6,
        "greaterEqual" => src >= dst,
        "greater" => src > dst,
        "notEqual" => (src - dst).abs() > 1e-6,
        _ => true,
    }
}

pub(super) fn compare_stencil(mode: &str, reference: u8, current: u8, mask: u8) -> bool {
    let r = reference & mask;
    let c = current & mask;
    match mode {
        "always" => true,
        "never" => false,
        "less" => r < c,
        "lessEqual" => r <= c,
        "equal" => r == c,
        "greaterEqual" => r >= c,
        "greater" => r > c,
        "notEqual" => r != c,
        _ => true,
    }
}

pub(super) fn apply_stencil_op(action: &str, current: u8, reference: u8, write_mask: u8) -> u8 {
    let next = match action {
        "keep" => current,
        "zero" => 0,
        "set" => reference,
        "invert" => !current,
        "incrementSaturate" => current.saturating_add(1),
        "decrementSaturate" => current.saturating_sub(1),
        "incrementWrap" => current.wrapping_add(1),
        "decrementWrap" => current.wrapping_sub(1),
        _ => current,
    };
    (current & !write_mask) | (next & write_mask)
}

pub(super) fn blend_factor_vec(factor: &str, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    match factor {
        "one" => [1.0, 1.0, 1.0, 1.0],
        "zero" => [0.0, 0.0, 0.0, 0.0],
        "sourceAlpha" => [src[3]; 4],
        "oneMinusSourceAlpha" => [1.0 - src[3]; 4],
        "destinationAlpha" => [dst[3]; 4],
        "oneMinusDestinationAlpha" => [1.0 - dst[3]; 4],
        "sourceColor" => src,
        "oneMinusSourceColor" => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2], 1.0 - src[3]],
        "destinationColor" => dst,
        "oneMinusDestinationColor" => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2], 1.0 - dst[3]],
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

pub(super) fn read_agal_src(
    token: u64,
    is_vertex: bool,
    attrs: &[[f32; 4]; 8],
    consts: &[[f32; 4]],
    temps: &[[f32; 4]; 32],
    out_reg: &[f32; 4],
    varyings: &[[f32; 4]; 8],
) -> [f32; 4] {
    let base_num = (token & 0xffff) as i32;
    let indirect_offset = ((token >> 16) & 0xff) as i8 as i32;
    let swizzle = ((token >> 24) & 0xff) as u8;
    let reg_type = ((token >> 32) & 0x0f) as u8;
    let is_indirect = ((token >> 63) & 1) == 1;

    let reg_idx = if is_indirect {
        let idx_type = ((token >> 40) & 0x0f) as u8;
        let idx_comp = ((token >> 48) & 0x03) as usize;
        let idx_vec = match idx_type {
            0 if is_vertex => attrs.get(base_num as usize).copied().unwrap_or_default(),
            1 => consts.get(base_num as usize).copied().unwrap_or_default(),
            2 => temps.get(base_num as usize).copied().unwrap_or_default(),
            _ => [0.0; 4],
        };
        (idx_vec[idx_comp].floor() as i32 + indirect_offset).max(0) as usize
    } else {
        base_num.max(0) as usize
    };

    let raw = match reg_type {
        0 if is_vertex => attrs.get(reg_idx).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]),
        1 => consts.get(reg_idx).copied().unwrap_or([0.0; 4]),
        2 => temps.get(reg_idx).copied().unwrap_or([0.0; 4]),
        3 => *out_reg,
        4 => varyings.get(reg_idx).copied().unwrap_or([0.0; 4]),
        _ => [0.0; 4],
    };

    [
        raw[(swizzle & 0x03) as usize],
        raw[((swizzle >> 2) & 0x03) as usize],
        raw[((swizzle >> 4) & 0x03) as usize],
        raw[((swizzle >> 6) & 0x03) as usize],
    ]
}

pub(super) fn write_agal_dst(
    dst_type: u8,
    dst_reg: u16,
    dst_mask: u8,
    val: [f32; 4],
    temps: &mut [[f32; 4]; 32],
    out_reg: &mut [f32; 4],
    varyings: &mut [[f32; 4]; 8],
) {
    let target = match dst_type {
        2 => temps.get_mut(dst_reg as usize),
        3 => Some(out_reg),
        4 => varyings.get_mut(dst_reg as usize),
        _ => None,
    };
    if let Some(reg) = target {
        let mask = if dst_mask == 0 { 0x0f } else { dst_mask };
        for lane in 0..4 {
            if (mask & (1 << lane)) != 0 {
                reg[lane] = val[lane];
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_agal(
    insts: &[AgalInst],
    is_vertex: bool,
    attrs: &[[f32; 4]; 8],
    consts: &[[f32; 4]],
    varyings: &mut [[f32; 4]; 8],
    tex_slots: &[Option<u32>; 8],
    samplers: &[SamplerState; 8],
    tex_arena: &BTreeMap<u32, TextureRecord>,
) -> Option<[f32; 4]> {
    let mut temps = [[0.0f32; 4]; 32];
    let mut out = [0.0f32, 0.0, 0.0, 1.0];

    for inst in insts {
        if inst.opcode == 0x27 {
            // kil
            let s1 = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
            if s1.iter().any(|&c| c < 0.0) {
                return None;
            }
            continue;
        }
        if inst.opcode == 0x28 {
            // tex dst, coord, fsN
            let coord = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
            let sampler_idx = (inst.src2 & 0xffff) as usize;
            let sampled = if let Some(Some(tid)) = tex_slots.get(sampler_idx)
                && let Some(tex) = tex_arena.get(tid)
            {
                let st = &samplers[sampler_idx];
                tex.sample_coord(coord, &st.wrap, &st.filter, &st.mipfilter)
            } else {
                [1.0, 1.0, 1.0, 1.0]
            };
            write_agal_dst(
                inst.dst_type,
                inst.dst_reg,
                inst.dst_mask,
                sampled,
                &mut temps,
                &mut out,
                varyings,
            );
            continue;
        }

        let a = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
        let b = read_agal_src(inst.src2, is_vertex, attrs, consts, &temps, &out, varyings);

        let mat_row = |offset: usize| -> [f32; 4] {
            let base = (inst.src2 & 0xffff) as usize + offset;
            let swz = ((inst.src2 >> 24) & 0xff) as u8;
            let raw = consts.get(base).copied().unwrap_or([0.0; 4]);
            if swz == 0 || swz == 0xe4 {
                raw
            } else {
                [
                    raw[(swz & 3) as usize],
                    raw[((swz >> 2) & 3) as usize],
                    raw[((swz >> 4) & 3) as usize],
                    raw[((swz >> 6) & 3) as usize],
                ]
            }
        };

        let dp3 = |u: [f32; 4], v: [f32; 4]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let dp4 = |u: [f32; 4], v: [f32; 4]| dp3(u, v) + u[3] * v[3];

        let res = match inst.opcode {
            0x00 => a, // mov
            0x01 => [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]],
            0x02 => [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]],
            0x03 => [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]],
            0x04 => [
                if b[0] != 0.0 { a[0] / b[0] } else { 0.0 },
                if b[1] != 0.0 { a[1] / b[1] } else { 0.0 },
                if b[2] != 0.0 { a[2] / b[2] } else { 0.0 },
                if b[3] != 0.0 { a[3] / b[3] } else { 0.0 },
            ],
            0x05 => [
                if a[0] != 0.0 { 1.0 / a[0] } else { 0.0 },
                if a[1] != 0.0 { 1.0 / a[1] } else { 0.0 },
                if a[2] != 0.0 { 1.0 / a[2] } else { 0.0 },
                if a[3] != 0.0 { 1.0 / a[3] } else { 0.0 },
            ],
            0x06 => [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].min(b[2]),
                a[3].min(b[3]),
            ],
            0x07 => [
                a[0].max(b[0]),
                a[1].max(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ],
            0x08 => [
                a[0] - a[0].floor(),
                a[1] - a[1].floor(),
                a[2] - a[2].floor(),
                a[3] - a[3].floor(),
            ],
            0x09 => [
                a[0].max(0.0).sqrt(),
                a[1].max(0.0).sqrt(),
                a[2].max(0.0).sqrt(),
                a[3].max(0.0).sqrt(),
            ],
            0x0a => [
                1.0 / a[0].max(1e-12).sqrt(),
                1.0 / a[1].max(1e-12).sqrt(),
                1.0 / a[2].max(1e-12).sqrt(),
                1.0 / a[3].max(1e-12).sqrt(),
            ],
            0x0b => [
                a[0].powf(b[0]),
                a[1].powf(b[1]),
                a[2].powf(b[2]),
                a[3].powf(b[3]),
            ],
            0x0c => [
                a[0].max(1e-12).log2(),
                a[1].max(1e-12).log2(),
                a[2].max(1e-12).log2(),
                a[3].max(1e-12).log2(),
            ],
            0x0d => [a[0].exp2(), a[1].exp2(), a[2].exp2(), a[3].exp2()],
            0x0e => {
                let len = dp3(a, a).sqrt().max(1e-12);
                [a[0] / len, a[1] / len, a[2] / len, a[3]]
            }
            0x0f => [a[0].sin(), a[1].sin(), a[2].sin(), a[3].sin()],
            0x10 => [a[0].cos(), a[1].cos(), a[2].cos(), a[3].cos()],
            0x11 => [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
                0.0,
            ],
            0x12 => [dp3(a, b); 4],
            0x13 => [dp4(a, b); 4],
            0x14 => [a[0].abs(), a[1].abs(), a[2].abs(), a[3].abs()],
            0x15 => [-a[0], -a[1], -a[2], -a[3]],
            0x16 => [
                a[0].clamp(0.0, 1.0),
                a[1].clamp(0.0, 1.0),
                a[2].clamp(0.0, 1.0),
                a[3].clamp(0.0, 1.0),
            ],
            0x17 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                [dp3(a, r0), dp3(a, r1), dp3(a, r2), a[3]]
            }
            0x18 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                let r3 = mat_row(3);
                [dp4(a, r0), dp4(a, r1), dp4(a, r2), dp4(a, r3)]
            }
            0x19 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                [dp4(a, r0), dp4(a, r1), dp4(a, r2), a[3]]
            }
            0x29 => [
                if a[0] >= b[0] { 1.0 } else { 0.0 },
                if a[1] >= b[1] { 1.0 } else { 0.0 },
                if a[2] >= b[2] { 1.0 } else { 0.0 },
                if a[3] >= b[3] { 1.0 } else { 0.0 },
            ],
            0x2a => [
                if a[0] < b[0] { 1.0 } else { 0.0 },
                if a[1] < b[1] { 1.0 } else { 0.0 },
                if a[2] < b[2] { 1.0 } else { 0.0 },
                if a[3] < b[3] { 1.0 } else { 0.0 },
            ],
            0x2c => [
                if (a[0] - b[0]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[1] - b[1]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[2] - b[2]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[3] - b[3]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
            ],
            0x2d => [
                if (a[0] - b[0]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[1] - b[1]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[2] - b[2]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[3] - b[3]).abs() > 1e-6 { 1.0 } else { 0.0 },
            ],
            _ => a,
        };

        write_agal_dst(
            inst.dst_type,
            inst.dst_reg,
            inst.dst_mask,
            res,
            &mut temps,
            &mut out,
            varyings,
        );
    }
    Some(out)
}
