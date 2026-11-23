use super::tables::*;
use super::*;

// ============================================================================
// Reversible YCoCg Color Conversion & Chroma Downsampling / Interpolation
// ============================================================================

#[inline]
pub(super) fn df_odd(d0: i32, d1: i32, d2: i32, d3: i32, d4: i32) -> i32 {
    (((d1 + d2 + d3) << 2) + (d2 << 1) + d0 + d4 + 8) >> 4
}

pub(super) fn downsample_uv_row(
    cf: ColorFormat,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    res_u: &mut [i32],
    res_v: &mut [i32],
    a0: &mut [Vec<i32>; MAX_CHANNELS],
    a1: &mut [Vec<i32>; MAX_CHANNELS],
) {
    let full_w = cmb_width * 16;
    for ch in 1..=2usize {
        let res: &mut [i32] = if ch == 1 { res_u } else { res_v };
        // Horizontal 5-tap downsampling: 444 -> 422
        let c_shift = usize::from(cf == ColorFormat::Yuv422);
        for row in 0..16usize {
            let mut d0 = res[IDX_CC[row][2]];
            let mut d4 = d0;
            let mut d1 = res[IDX_CC[row][1]];
            let mut d3 = d1;
            let mut d2 = res[IDX_CC[row][0]];

            let mut col = 0usize;
            while col + 2 < full_w {
                let dst_idx = ((col >> 4) << (8 - c_shift)) + IDX_CC[row][(col & 15) >> c_shift];
                let filt = df_odd(d0, d1, d2, d3, d4);
                if cf == ColorFormat::Yuv422 {
                    a1[ch][dst_idx] = filt;
                } else {
                    res[dst_idx] = filt;
                }
                d0 = d2;
                d1 = d3;
                d2 = d4;
                d3 = res[(((col + 3) >> 4) << 8) + IDX_CC[row][(col + 3) & 0xF]];
                d4 = res[(((col + 4) >> 4) << 8) + IDX_CC[row][(col + 4) & 0xF]];
                col += 2;
            }
            d4 = d2;
            let dst_idx = ((col >> 4) << (8 - c_shift)) + IDX_CC[row][(col & 15) >> c_shift];
            let filt = df_odd(d0, d1, d2, d3, d4);
            if cf == ColorFormat::Yuv422 {
                a1[ch][dst_idx] = filt;
            } else {
                res[dst_idx] = filt;
            }
        }

        // Vertical 5-tap downsampling: 422 -> 420
        if cf == ColorFormat::Yuv420 {
            let delay_base = cmb_width * 256;
            let row_stride = cmb_width * 8;
            for col in 0..row_stride {
                let mb_off = (col >> 3) << 8;
                let px_off = (col & 7) << 1;

                let (mut d0, mut d1, mut d2, mut d3, mut d4);
                if c_row == 0 {
                    d0 = res[mb_off + IDX_CC[2][px_off]];
                    d4 = d0;
                    d1 = res[mb_off + IDX_CC[1][px_off]];
                    d3 = d1;
                    d2 = res[mb_off + IDX_CC[0][px_off]];
                } else {
                    d0 = res[delay_base + col];
                    d1 = res[delay_base + row_stride + col];
                    d2 = res[delay_base + 2 * row_stride + col];
                    d3 = res[delay_base + 3 * row_stride + col];
                    d4 = res[mb_off + IDX_CC[0][px_off]];
                    a0[ch][((col >> 3) << 6) + IDX_CC_420[7][col & 7]] = df_odd(d0, d1, d2, d3, d4);

                    d0 = res[delay_base + 2 * row_stride + col];
                    d1 = res[delay_base + 3 * row_stride + col];
                    d2 = res[mb_off + IDX_CC[0][px_off]];
                    d3 = res[mb_off + IDX_CC[1][px_off]];
                    d4 = res[mb_off + IDX_CC[2][px_off]];
                }

                for r2 in (0..12usize).step_by(2) {
                    a1[ch][((col >> 3) << 6) + IDX_CC_420[r2 >> 1][col & 7]] =
                        df_odd(d0, d1, d2, d3, d4);
                    d0 = d2;
                    d1 = d3;
                    d2 = d4;
                    d3 = res[mb_off + IDX_CC[r2 + 3][px_off]];
                    d4 = res[mb_off + IDX_CC[r2 + 4][px_off]];
                }

                a1[ch][((col >> 3) << 6) + IDX_CC_420[6][col & 7]] = df_odd(d0, d1, d2, d3, d4);
                d0 = d2;
                d1 = d3;
                d2 = d4;
                d3 = res[mb_off + IDX_CC[15][px_off]];

                if c_row + 1 == cmb_height {
                    d4 = d2;
                    a1[ch][((col >> 3) << 6) + IDX_CC_420[7][col & 7]] = df_odd(d0, d1, d2, d3, d4);
                } else {
                    for r in 0..4usize {
                        res[delay_base + r * row_stride + col] =
                            res[mb_off + IDX_CC[r + 12][px_off]];
                    }
                }
            }
        }
    }
}

pub(super) fn interpolate_uv_row(
    cf: ColorFormat,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize, // note: c_row in outputMBRow is 1..=cmb_height
    a0: &[Vec<i32>; MAX_CHANNELS],
    a1: &[Vec<i32>; MAX_CHANNELS],
    res_u: &mut [i32],
    res_v: &mut [i32],
) {
    let c_width = cmb_width * 16;
    let src_u = &a0[1];
    let src_v = &a0[2];

    if cf == ColorFormat::Yuv422 {
        for row in 0..16usize {
            let mut idx_d = 0usize;
            for col in (0..c_width).step_by(2) {
                let idx_s = ((col >> 4) << 7) + IDX_CC[row][(col >> 1) & 7];
                idx_d = ((col >> 4) << 8) + IDX_CC[row][col & 15];
                res_u[idx_d] = src_u[idx_s];
                res_v[idx_d] = src_v[idx_s];
                if col > 0 {
                    let il = col - 2;
                    let ic = col - 1;
                    let idx_l = ((il >> 4) << 8) + IDX_CC[row][il & 15];
                    let idx_c = ((ic >> 4) << 8) + IDX_CC[row][ic & 15];
                    res_u[idx_c] = (res_u[idx_l] + res_u[idx_d] + 1) >> 1;
                    res_v[idx_c] = (res_v[idx_l] + res_v[idx_d] + 1) >> 1;
                }
            }
            let last_idx = (((c_width - 1) >> 4) << 8) + IDX_CC[row][(c_width - 1) & 15];
            res_u[last_idx] = res_u[idx_d];
            res_v[last_idx] = res_v[idx_d];
        }
    } else if cf == ColorFormat::Yuv420 {
        for col in (0..c_width).step_by(2) {
            let c_mb = (col >> 4) << 8;
            let c_pix = col & 15;
            let mut idx_d = 0usize;
            for row in (0..16usize).step_by(2) {
                let idx_s = ((col >> 4) << 6) + IDX_CC_420[row >> 1][(col >> 1) & 7];
                idx_d = c_mb + IDX_CC[row][c_pix];
                res_u[idx_d] = src_u[idx_s];
                res_v[idx_d] = src_v[idx_s];
                if row > 0 {
                    let idx_t = c_mb + IDX_CC[row - 2][c_pix];
                    let idx_c = c_mb + IDX_CC[row - 1][c_pix];
                    res_u[idx_c] = (res_u[idx_t] + res_u[idx_d] + 1) >> 1;
                    res_v[idx_c] = (res_v[idx_t] + res_v[idx_d] + 1) >> 1;
                }
            }
            let idx_last = c_mb + IDX_CC[15][c_pix];
            if c_row == cmb_height {
                res_u[idx_last] = res_u[idx_d];
                res_v[idx_last] = res_v[idx_d];
            } else {
                let idx_b = ((col >> 4) << 6) + IDX_CC_420[0][(col >> 1) & 7];
                res_u[idx_last] = (a1[1][idx_b] + res_u[idx_d] + 1) >> 1;
                res_v[idx_last] = (a1[2][idx_b] + res_v[idx_d] + 1) >> 1;
            }
        }

        for row in 0..16usize {
            let mut idx_s = 0usize;
            let mut col = 1usize;
            while col < c_width - 2 {
                let idx_l = (((col - 1) >> 4) << 8) + IDX_CC[row][(col - 1) & 15];
                let idx_d = ((col >> 4) << 8) + IDX_CC[row][col & 15];
                idx_s = (((col + 1) >> 4) << 8) + IDX_CC[row][(col + 1) & 15];
                res_u[idx_d] = (res_u[idx_s] + res_u[idx_l] + 1) >> 1;
                res_v[idx_d] = (res_v[idx_s] + res_v[idx_l] + 1) >> 1;
                col += 2;
            }
            let idx_last = (((c_width - 1) >> 4) << 8) + IDX_CC[row][(c_width - 1) & 15];
            res_u[idx_last] = res_u[idx_s];
            res_v[idx_last] = res_v[idx_s];
        }
    }
}
