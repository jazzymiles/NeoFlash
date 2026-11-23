use super::bitstream::*;
use super::tables::*;
use super::*;

// ============================================================================
// Frequency-Domain Prediction, Quantization & CBP Modeling
// ============================================================================

pub(super) fn get_ac_pred_mode(mb: &MbInfo, cf: ColorFormat) -> i32 {
    let c0 = &mb.block_dc[0];
    let mut str_h = c0[1].abs() + c0[2].abs() + c0[3].abs();
    let mut str_v = c0[4].abs() + c0[8].abs() + c0[12].abs();

    if cf != ColorFormat::YOnly {
        let cu = &mb.block_dc[1];
        let cv = &mb.block_dc[2];
        str_h += cu[1].abs() + cv[1].abs();
        match cf {
            ColorFormat::Yuv420 => {
                str_v += cu[2].abs() + cv[2].abs();
            }
            ColorFormat::Yuv422 => {
                str_v += cu[2].abs() + cv[2].abs() + cu[6].abs() + cv[6].abs();
                str_h += cu[5].abs() + cv[5].abs();
            }
            ColorFormat::Yuv444 => {
                str_v += cu[4].abs() + cv[4].abs();
            }
            ColorFormat::YOnly => {}
        }
    }

    if str_h * ORIENT_WEIGHT < str_v {
        1
    } else if str_v * ORIENT_WEIGHT < str_h {
        0
    } else {
        2
    }
}

pub(super) fn get_dcac_pred_mode(
    cf: ColorFormat,
    ctx_left: bool,
    ctx_top: bool,
    q_index_lp: u8,
    mb_x: usize,
    pred_cur: &[Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
) -> (i32, i32) {
    let dc_mode = if ctx_left && ctx_top {
        3
    } else if ctx_left {
        1
    } else if ctx_top {
        0
    } else {
        let i_l = pred_cur[0][mb_x - 1].dc;
        let i_t = pred_prev[0][mb_x].dc;
        let i_tl = pred_prev[0][mb_x - 1].dc;
        let (str_h, str_v) = if cf == ColorFormat::YOnly {
            ((i_tl - i_l).abs(), (i_tl - i_t).abs())
        } else {
            let scale = match cf {
                ColorFormat::Yuv420 => 8,
                ColorFormat::Yuv422 => 4,
                ColorFormat::Yuv444 | ColorFormat::YOnly => 2,
            };
            let lu = pred_cur[1][mb_x - 1].dc;
            let tu = pred_prev[1][mb_x].dc;
            let tlu = pred_prev[1][mb_x - 1].dc;
            let lv = pred_cur[2][mb_x - 1].dc;
            let tv = pred_prev[2][mb_x].dc;
            let tlv = pred_prev[2][mb_x - 1].dc;
            (
                (i_tl - i_l).abs() * scale + (tlu - lu).abs() + (tlv - lv).abs(),
                (i_tl - i_t).abs() * scale + (tlu - tu).abs() + (tlv - tv).abs(),
            )
        };
        if str_h * ORIENT_WEIGHT < str_v {
            1
        } else if str_v * ORIENT_WEIGHT < str_h {
            0
        } else {
            2
        }
    };

    let mut ad_mode = 2;
    if dc_mode == 1 && q_index_lp == pred_prev[0][mb_x].qp_index {
        ad_mode = 1;
    }
    if dc_mode == 0 && q_index_lp == pred_cur[0][mb_x - 1].qp_index {
        ad_mode = 0;
    }
    (dc_mode, ad_mode << 2)
}

pub(super) fn update_pred_info(
    cf: ColorFormat,
    channels: usize,
    mb: &MbInfo,
    mb_x: usize,
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
) {
    let full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };
    for i in 0..full_ch {
        let p = &mb.block_dc[i];
        let info = &mut pred_cur[i][mb_x];
        info.dc = p[0];
        info.qp_index = mb.q_index_lp;
        info.ad = [p[1], p[2], p[3], p[4], p[8], p[12]];
    }
    if cf == ColorFormat::Yuv420 {
        for i in 1..3 {
            let p = &mb.block_dc[i];
            let info = &mut pred_cur[i][mb_x];
            info.dc = p[0];
            info.qp_index = mb.q_index_lp;
            info.ad[0] = p[1];
            info.ad[1] = p[2];
        }
    } else if cf == ColorFormat::Yuv422 {
        for i in 1..3 {
            let p = &mb.block_dc[i];
            let info = &mut pred_cur[i][mb_x];
            info.dc = p[0];
            info.qp_index = mb.q_index_lp;
            info.ad[0] = p[1];
            info.ad[1] = p[2];
            info.ad[2] = p[5];
            info.ad[3] = p[6];
            info.ad[4] = p[4];
        }
    }
}

pub(super) fn quantize_macroblock(
    cf: ColorFormat,
    channels: usize,
    subband: Subband,
    tile: &TileQuantizers,
    mb: &mut MbInfo,
    planes: &mut [&mut [i32]; MAX_CHANNELS],
) {
    for ch in 0..channels {
        let is_uv = ch > 0 && cf != ColorFormat::YOnly;
        let num_blk = if is_uv { cf.chroma_blocks() } else { 16 };
        let offsets: &[usize] = match num_blk {
            4 => &BLK_OFFSET_UV,
            8 => &BLK_OFFSET_UV_422,
            _ => &BLK_OFFSET,
        };
        let qp_dc = tile.dc[ch];
        let qp_lp = tile.lp[usize::from(mb.q_index_lp)][ch];
        let qp_hp = tile.hp[usize::from(mb.q_index_hp)][ch];
        let plane = &mut *planes[ch];

        for (j, &blk_off) in offsets.iter().enumerate() {
            if j == 0 {
                plane[blk_off] = qp_dc.quantize(plane[blk_off]);
            } else if subband != Subband::DcOnly {
                plane[blk_off] = qp_lp.quantize(plane[blk_off]);
            }
            if subband != Subband::DcOnly && subband != Subband::NoHighpass {
                for i in 1..16 {
                    plane[blk_off + i] = qp_hp.quantize(plane[blk_off + i]);
                }
            }
        }
    }

    for ch in 0..channels {
        let plane = &*planes[ch];
        mb.block_dc[ch] = [0; 16];
        if ch > 0 && cf == ColorFormat::Yuv422 {
            for i in 0..8 {
                mb.block_dc[ch][i] = plane[BLK_OFFSET_UV_422[i]];
            }
        } else if ch > 0 && cf == ColorFormat::Yuv420 {
            for i in 0..4 {
                mb.block_dc[ch][i] = plane[BLK_OFFSET_UV[i]];
            }
        } else {
            for i in 0..16 {
                mb.block_dc[ch][i] = plane[DCT_INDEX[2][i]];
            }
        }
    }
}

pub(super) fn pred_macroblock_enc(
    cf: ColorFormat,
    channels: usize,
    ctx_left: bool,
    ctx_top: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    planes: &mut [&mut [i32]; MAX_CHANNELS],
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
) {
    let full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };
    let (dc_mode, ad_mode) = get_dcac_pred_mode(
        cf,
        ctx_left,
        ctx_top,
        mb.q_index_lp,
        mb_x,
        pred_cur,
        pred_prev,
    );
    let ac_mode = get_ac_pred_mode(mb, cf);
    mb.orientation = 2 - ac_mode;

    update_pred_info(cf, channels, mb, mb_x, pred_cur);

    for i in 0..full_ch {
        let org = &mut mb.block_dc[i];
        match dc_mode {
            1 => org[0] -= pred_prev[i][mb_x].dc,
            0 => org[0] -= pred_cur[i][mb_x - 1].dc,
            2 => org[0] -= (pred_cur[i][mb_x - 1].dc + pred_prev[i][mb_x].dc) >> 1,
            _ => {}
        }
        if ad_mode == 4 {
            let r = &pred_prev[i][mb_x].ad;
            org[4] -= r[3];
            org[8] -= r[4];
            org[12] -= r[5];
        } else if ad_mode == 0 {
            let r = &pred_cur[i][mb_x - 1].ad;
            org[1] -= r[0];
            org[2] -= r[1];
            org[3] -= r[2];
        }

        let plane = &mut *planes[i];
        if ac_mode == 1 {
            for k in (0..=192usize).step_by(64) {
                for j in [48usize, 32, 16] {
                    plane[k + j + 10] -= plane[k + j + 10 - 16];
                    plane[k + j + 2] -= plane[k + j + 2 - 16];
                    plane[k + j + 9] -= plane[k + j + 9 - 16];
                }
            }
        } else if ac_mode == 0 {
            for k in (0..64usize).step_by(16) {
                for j in [192usize, 128, 64] {
                    plane[k + j + 5] -= plane[k + j + 5 - 64];
                    plane[k + j + 1] -= plane[k + j + 1 - 64];
                    plane[k + j + 6] -= plane[k + j + 6 - 64];
                }
            }
        }
    }

    if cf == ColorFormat::Yuv420 {
        for i in 1..3 {
            let org = &mut mb.block_dc[i];
            match dc_mode {
                1 => org[0] -= pred_prev[i][mb_x].dc,
                0 => org[0] -= pred_cur[i][mb_x - 1].dc,
                2 => org[0] -= (pred_cur[i][mb_x - 1].dc + pred_prev[i][mb_x].dc + 1) >> 1,
                _ => {}
            }
            if ad_mode == 4 {
                org[2] -= pred_prev[i][mb_x].ad[1];
            } else if ad_mode == 0 {
                org[1] -= pred_cur[i][mb_x - 1].ad[0];
            }

            let plane = &mut *planes[i];
            if ac_mode == 1 {
                for j in [16usize, 48] {
                    plane[j + 10] -= plane[j + 10 - 16];
                    plane[j + 2] -= plane[j + 2 - 16];
                    plane[j + 9] -= plane[j + 9 - 16];
                }
            } else if ac_mode == 0 {
                for j in [32usize, 48] {
                    plane[j + 5] -= plane[j + 5 - 32];
                    plane[j + 1] -= plane[j + 1 - 32];
                    plane[j + 6] -= plane[j + 6 - 32];
                }
            }
        }
    } else if cf == ColorFormat::Yuv422 {
        for i in 1..3 {
            let org = &mut mb.block_dc[i];
            match dc_mode {
                1 => org[0] -= pred_prev[i][mb_x].dc,
                0 => org[0] -= pred_cur[i][mb_x - 1].dc,
                2 => org[0] -= (pred_cur[i][mb_x - 1].dc + pred_prev[i][mb_x].dc + 1) >> 1,
                _ => {}
            }
            if ad_mode == 4 {
                org[4] -= pred_prev[i][mb_x].ad[4];
                org[6] -= org[2];
                org[2] -= pred_prev[i][mb_x].ad[3];
            } else if ad_mode == 0 {
                org[4] -= pred_cur[i][mb_x - 1].ad[4];
                org[1] -= pred_cur[i][mb_x - 1].ad[0];
                org[5] -= pred_cur[i][mb_x - 1].ad[2];
            } else if dc_mode == 1 {
                org[6] -= org[2];
            }

            let plane = &mut *planes[i];
            if ac_mode == 1 {
                for j in [48usize, 32, 16] {
                    for k in [0usize, 64] {
                        plane[j + k + 10] -= plane[j + k + 10 - 16];
                        plane[j + k + 2] -= plane[j + k + 2 - 16];
                        plane[j + k + 9] -= plane[j + k + 9 - 16];
                    }
                }
            } else if ac_mode == 0 {
                for j in (64..=112usize).step_by(16) {
                    plane[j + 5] -= plane[j + 5 - 64];
                    plane[j + 1] -= plane[j + 1 - 64];
                    plane[j + 6] -= plane[j + 6 - 64];
                }
            }
        }
    }
}

pub(super) fn pred_dcac_dec(
    cf: ColorFormat,
    channels: usize,
    ctx_left: bool,
    ctx_top: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    pred_cur: &[Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
) {
    let full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };
    let (dc_mode, ad_mode) = get_dcac_pred_mode(
        cf,
        ctx_left,
        ctx_top,
        mb.q_index_lp,
        mb_x,
        pred_cur,
        pred_prev,
    );

    for ii in 0..full_ch {
        let org = &mut mb.block_dc[ii];
        match dc_mode {
            1 => org[0] += pred_prev[ii][mb_x].dc,
            0 => org[0] += pred_cur[ii][mb_x - 1].dc,
            2 => org[0] += (pred_cur[ii][mb_x - 1].dc + pred_prev[ii][mb_x].dc) >> 1,
            _ => {}
        }
        if ad_mode == 4 {
            let r = &pred_prev[ii][mb_x].ad;
            org[4] += r[3];
            org[8] += r[4];
            org[12] += r[5];
        } else if ad_mode == 0 {
            let r = &pred_cur[ii][mb_x - 1].ad;
            org[1] += r[0];
            org[2] += r[1];
            org[3] += r[2];
        }
    }

    if cf == ColorFormat::Yuv420 {
        for ii in 1..3 {
            let org = &mut mb.block_dc[ii];
            match dc_mode {
                1 => org[0] += pred_prev[ii][mb_x].dc,
                0 => org[0] += pred_cur[ii][mb_x - 1].dc,
                2 => org[0] += (pred_cur[ii][mb_x - 1].dc + pred_prev[ii][mb_x].dc + 1) >> 1,
                _ => {}
            }
            if ad_mode == 4 {
                org[2] += pred_prev[ii][mb_x].ad[1];
            } else if ad_mode == 0 {
                org[1] += pred_cur[ii][mb_x - 1].ad[0];
            }
        }
    } else if cf == ColorFormat::Yuv422 {
        for ii in 1..3 {
            let org = &mut mb.block_dc[ii];
            match dc_mode {
                1 => org[0] += pred_prev[ii][mb_x].dc,
                0 => org[0] += pred_cur[ii][mb_x - 1].dc,
                2 => org[0] += (pred_cur[ii][mb_x - 1].dc + pred_prev[ii][mb_x].dc + 1) >> 1,
                _ => {}
            }
            if ad_mode == 4 {
                org[4] += pred_prev[ii][mb_x].ad[4];
                org[2] += pred_prev[ii][mb_x].ad[3];
                org[6] += org[2];
            } else if ad_mode == 0 {
                org[4] += pred_cur[ii][mb_x - 1].ad[4];
                org[1] += pred_cur[ii][mb_x - 1].ad[0];
                org[5] += pred_cur[ii][mb_x - 1].ad[2];
            } else if dc_mode == 1 {
                org[6] += org[2];
            }
        }
    }

    mb.orientation = 2 - get_ac_pred_mode(mb, cf);
}

pub(super) fn dequantize_macroblock(
    cf: ColorFormat,
    channels: usize,
    subband: Subband,
    tile: &TileQuantizers,
    mb: &MbInfo,
    planes: &mut [&mut [i32]; MAX_CHANNELS],
) {
    for i in 0..channels {
        let qp_dc = tile.dc[i].qp;
        let qp_lp = tile.lp[usize::from(mb.q_index_lp)][i].qp;
        let rec = &mut *planes[i];
        let org = &mb.block_dc[i];
        rec[0] = org[0] * qp_dc;

        if subband != Subband::DcOnly {
            if i == 0 || !matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
                for k in 1..16 {
                    rec[DCT_INDEX[2][k]] = org[k] * qp_lp;
                }
            } else if cf == ColorFormat::Yuv422 {
                rec[64] = org[1] * qp_lp;
                rec[16] = org[2] * qp_lp;
                rec[80] = org[3] * qp_lp;
                rec[32] = org[4] * qp_lp;
                rec[96] = org[5] * qp_lp;
                rec[48] = org[6] * qp_lp;
                rec[112] = org[7] * qp_lp;
            } else {
                rec[32] = org[1] * qp_lp;
                rec[16] = org[2] * qp_lp;
                rec[48] = org[3] * qp_lp;
            }
        }
    }
}

pub(super) fn pred_ac_dec(
    cf: ColorFormat,
    channels: usize,
    mb: &MbInfo,
    planes: &mut [&mut [i32]; MAX_CHANNELS],
) {
    const BLK_IDX_TOP: [usize; 12] = [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15];
    let full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };
    let ac_mode = 2 - mb.orientation;

    for i in 0..full_ch {
        let src = &mut *planes[i];
        if ac_mode == 1 {
            for &blk in &BLK_IDX_TOP {
                let off = 16 * blk;
                src[off + 2] += src[off - 16 + 2];
                src[off + 10] += src[off - 16 + 10];
                src[off + 9] += src[off - 16 + 9];
            }
        } else if ac_mode == 0 {
            for off in (64..256usize).step_by(16) {
                src[off + 1] += src[off - 64 + 1];
                src[off + 5] += src[off - 64 + 5];
                src[off + 6] += src[off - 64 + 6];
            }
        }
    }

    if cf == ColorFormat::Yuv420 {
        for ch in 1..3 {
            let src = &mut *planes[ch];
            if ac_mode == 1 {
                for j in [1usize, 3] {
                    let off = 16 * j;
                    src[off + 2] += src[off - 16 + 2];
                    src[off + 10] += src[off - 16 + 10];
                    src[off + 9] += src[off - 16 + 9];
                }
            } else if ac_mode == 0 {
                for j in 2..=3usize {
                    let off = 16 * j;
                    src[off + 1] += src[off - 32 + 1];
                    src[off + 5] += src[off - 32 + 5];
                    src[off + 6] += src[off - 32 + 6];
                }
            }
        }
    } else if cf == ColorFormat::Yuv422 {
        for ch in 1..3 {
            let src = &mut *planes[ch];
            if ac_mode == 1 {
                for j in 2..8usize {
                    let off = BLK_OFFSET_UV_422[j];
                    src[off + 10] += src[off - 16 + 10];
                    src[off + 2] += src[off - 16 + 2];
                    src[off + 9] += src[off - 16 + 9];
                }
            } else if ac_mode == 0 {
                for j in (1..8usize).step_by(2) {
                    let off = BLK_OFFSET_UV_422[j];
                    src[off + 1] += src[off - 64 + 1];
                    src[off + 5] += src[off - 64 + 5];
                    src[off + 6] += src[off - 64 + 6];
                }
            }
        }
    }
}

#[inline]
pub(super) fn num_ones_u16(v: i32) -> i32 {
    ((v as u32) & 0xFFFF).count_ones() as i32
}

#[inline]
pub(super) fn saturate32(v: i32) -> i32 {
    v.clamp(-16, 15)
}

pub(super) fn update_cbp_model_state(model: &mut CbpModel, c1: usize, n_orig: i32) {
    model.count0[c1] = saturate32(model.count0[c1] + n_orig - AVG_NDIFF);
    model.count1[c1] = saturate32(model.count1[c1] + 16 - n_orig - AVG_NDIFF);
    if model.count0[c1] < 0 {
        if model.count0[c1] < model.count1[c1] {
            model.state[c1] = 1;
        } else {
            model.state[c1] = 2;
        }
    } else if model.count1[c1] < 0 {
        model.state[c1] = 2;
    } else {
        model.state[c1] = 0;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pred_cbp_enc(
    cf: ColorFormat,
    channels: usize,
    ctx_left: bool,
    ctx_top: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    planes: &[&mut [i32]; MAX_CHANNELS],
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
    ctx: &mut CodingContext,
) {
    for ch in 0..channels {
        let is_uv = ch > 0;
        let num_blk = if is_uv { cf.chroma_blocks() } else { 16 };
        let offsets: &[usize] = match num_blk {
            4 => &BLK_OFFSET_UV,
            8 => &BLK_OFFSET_UV_422,
            _ => &BLK_OFFSET,
        };
        let threshold = (1u32 << (ctx.model_ac.flc_bits[usize::from(is_uv)] as u32)) - 1;
        let threshold2 = threshold * 2 + 1;
        let mut cbp = 0i32;
        let plane = &*planes[ch];
        for (j, &blk_off) in offsets.iter().enumerate() {
            for i in 1..16 {
                let v = (plane[blk_off + i] as u32).wrapping_add(threshold);
                if v >= threshold2 {
                    cbp |= 1 << j;
                    break;
                }
            }
        }
        mb.cbp[ch] = cbp;
        pred_cur[ch][mb_x].cbp = cbp;

        let c1 = usize::from(ch > 0);
        if num_blk == 16 {
            let mut pred = if ctx_left {
                if ctx_top {
                    1
                } else {
                    (pred_prev[ch][mb_x].cbp >> 10) & 1
                }
            } else {
                (pred_cur[ch][mb_x - 1].cbp >> 5) & 1
            };
            pred |= (cbp & 0x3300) << 2;
            pred |= (cbp & 0x00CC) << 6;
            pred |= (cbp & 0x0033) << 2;
            pred |= (cbp & 0x0011) << 1;
            pred |= (cbp & 0x0002) << 3;

            mb.diff_cbp[ch] = match ctx.cbp_model.state[c1] {
                0 => pred ^ cbp,
                1 => cbp,
                _ => cbp ^ 0xFFFF,
            };
            update_cbp_model_state(&mut ctx.cbp_model, c1, num_ones_u16(cbp));
        } else if num_blk == 8 {
            let mut pred = if ctx_left {
                if ctx_top {
                    1
                } else {
                    (pred_prev[ch][mb_x].cbp >> 6) & 1
                }
            } else {
                (pred_cur[ch][mb_x - 1].cbp >> 1) & 1
            };
            pred |= (cbp & 0x01) << 1;
            pred |= (cbp & 0x03) << 2;
            pred |= (cbp & 0x0C) << 2;
            pred |= (cbp & 0x30) << 2;

            mb.diff_cbp[ch] = match ctx.cbp_model.state[1] {
                0 => pred ^ cbp,
                1 => cbp,
                _ => cbp ^ 0xFF,
            };
            update_cbp_model_state(&mut ctx.cbp_model, 1, num_ones_u16(cbp) * 2);
        } else {
            let mut pred = if ctx_left {
                if ctx_top {
                    1
                } else {
                    (pred_prev[ch][mb_x].cbp >> 2) & 1
                }
            } else {
                (pred_cur[ch][mb_x - 1].cbp >> 1) & 1
            };
            pred |= (cbp & 0x01) << 1;
            pred |= (cbp & 0x03) << 2;

            mb.diff_cbp[ch] = match ctx.cbp_model.state[1] {
                0 => pred ^ cbp,
                1 => cbp,
                _ => cbp ^ 0x0F,
            };
            update_cbp_model_state(&mut ctx.cbp_model, 1, num_ones_u16(cbp) * 4);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pred_cbp_dec(
    cf: ColorFormat,
    channels: usize,
    ctx_left: bool,
    ctx_top: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
    ctx: &mut CodingContext,
) {
    let full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };
    for ch in 0..full_ch {
        let c1 = usize::from(ch > 0);
        let mut cbp = mb.diff_cbp[ch];
        if ctx.cbp_model.state[c1] == 0 {
            if ctx_left {
                if ctx_top {
                    cbp ^= 1;
                } else {
                    cbp ^= (pred_prev[ch][mb_x].cbp >> 10) & 1;
                }
            } else {
                cbp ^= (pred_cur[ch][mb_x - 1].cbp >> 5) & 1;
            }
            cbp ^= 0x02 & (cbp << 1);
            cbp ^= 0x10 & (cbp << 3);
            cbp ^= 0x20 & (cbp << 1);
            cbp ^= (cbp & 0x0033) << 2;
            cbp ^= (cbp & 0x00CC) << 6;
            cbp ^= (cbp & 0x3300) << 2;
        } else if ctx.cbp_model.state[c1] == 2 {
            cbp ^= 0xFFFF;
        }
        mb.cbp[ch] = cbp;
        pred_cur[ch][mb_x].cbp = cbp;
        update_cbp_model_state(&mut ctx.cbp_model, c1, num_ones_u16(cbp));
    }

    if cf == ColorFormat::Yuv422 {
        for ch in 1..3 {
            let mut cbp = mb.diff_cbp[ch];
            if ctx.cbp_model.state[1] == 0 {
                if ctx_left {
                    if ctx_top {
                        cbp ^= 1;
                    } else {
                        cbp ^= (pred_prev[ch][mb_x].cbp >> 6) & 1;
                    }
                } else {
                    cbp ^= (pred_cur[ch][mb_x - 1].cbp >> 1) & 1;
                }
                cbp ^= (cbp & 0x01) << 1;
                cbp ^= (cbp & 0x03) << 2;
                cbp ^= (cbp & 0x0C) << 2;
                cbp ^= (cbp & 0x30) << 2;
            } else if ctx.cbp_model.state[1] == 2 {
                cbp ^= 0xFF;
            }
            mb.cbp[ch] = cbp;
            pred_cur[ch][mb_x].cbp = cbp;
            update_cbp_model_state(&mut ctx.cbp_model, 1, num_ones_u16(cbp) * 2);
        }
    } else if cf == ColorFormat::Yuv420 {
        for ch in 1..3 {
            let mut cbp = mb.diff_cbp[ch];
            if ctx.cbp_model.state[1] == 0 {
                if ctx_left {
                    if ctx_top {
                        cbp ^= 1;
                    } else {
                        cbp ^= (pred_prev[ch][mb_x].cbp >> 2) & 1;
                    }
                } else {
                    cbp ^= (pred_cur[ch][mb_x - 1].cbp >> 1) & 1;
                }
                cbp ^= 0x02 & (cbp << 1);
                cbp ^= (cbp & 0x03) << 2;
            } else if ctx.cbp_model.state[1] == 2 {
                cbp ^= 0x0F;
            }
            mb.cbp[ch] = cbp;
            pred_cur[ch][mb_x].cbp = cbp;
            update_cbp_model_state(&mut ctx.cbp_model, 1, num_ones_u16(cbp) * 4);
        }
    }
}
