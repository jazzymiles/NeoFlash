use super::bitstream::*;
use super::tables::*;
use super::*;

// ============================================================================
// ITU-T Rec. T.832 Macroblock Entropy Encoding & Decoding (segenc.c / segdec.c)
// ============================================================================

pub(super) fn encode_significant_abs_level(
    abs_level_in: u32,
    ah: &mut AdaptiveHuffman,
    bw: &mut BitWriter,
) {
    const A_INDEX: [usize; 16] = [0, 1, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5];
    const A_FIXED: [u8; 6] = [0, 0, 1, 2, 2, 2];

    debug_assert!(abs_level_in > 0);
    let abs_level = abs_level_in - 1;
    if abs_level >= 16 {
        let mut i = abs_level >> 5;
        let mut fixed = 4u8;
        while i != 0 {
            fixed += 1;
            i >>= 1;
        }
        ah.add_delta(6);
        ah.put_symbol(bw, 6);
        if fixed > 18 {
            bw.put_bits(15, 4);
            if fixed > 21 {
                bw.put_bits(3, 2);
                bw.put_bits(u32::from(fixed - 22), 3);
            } else {
                bw.put_bits(u32::from(fixed - 19), 2);
            }
        } else {
            bw.put_bits(u32::from(fixed - 4), 4);
        }
        bw.put_bits(abs_level, fixed);
    } else {
        let idx = A_INDEX[abs_level as usize];
        let fixed = A_FIXED[idx];
        ah.add_delta(idx);
        ah.put_symbol(bw, idx);
        bw.put_bits(abs_level, fixed);
    }
}

pub(super) fn decode_significant_abs_level(
    ah: &mut AdaptiveHuffman,
    br: &mut BitReader<'_>,
) -> Result<i32, BitmapError> {
    const A_REMAP: [i32; 6] = [2, 3, 4, 6, 10, 14];
    const A_FIXED: [u8; 6] = [0, 0, 1, 2, 2, 2];

    let idx = br.get_huff(ah.dec_table)?;
    if idx > 6 {
        return Err(error("invalid JPEG-XR significant abs level symbol"));
    }
    ah.add_delta(idx);
    if idx < 2 {
        Ok((idx as i32) + 2)
    } else if idx < 6 {
        let fixed = A_FIXED[idx];
        Ok(A_REMAP[idx] + (br.get_bits(fixed)? as i32))
    } else {
        let mut fixed = (br.get_bits(4)? as u8) + 4;
        if fixed == 19 {
            fixed += br.get_bits(2)? as u8;
            if fixed == 22 {
                fixed += br.get_bits(3)? as u8;
            }
        }
        if fixed >= 31 {
            return Err(error("JPEG-XR abs level escape bit width overflow"));
        }
        let rem = br.get_bits(fixed)? as i64;
        let level = 2i64 + (1i64 << fixed) + rem;
        i32::try_from(level).map_err(|_| error("JPEG-XR abs level overflow"))
    }
}

pub(super) fn encode_significant_run(
    run: i32,
    max_run: i32,
    ah0: &AdaptiveHuffman,
    bw: &mut BitWriter,
) {
    const A_INDEX: [usize; 34] = [
        0, 1, 2, 2, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, // bin 0
        0, 1, 2, 2, 3, 3, 4, 4, 4, 4, 0, 0, 0, 0, // bin 1
        0, 1, 2, 3, 4, 4, // bin 2
    ];
    const G_LEN: [u8; 4] = [3, 3, 2, 1];

    if max_run < 5 {
        if max_run > 1 {
            let len = G_LEN[(max_run - run) as usize] - ((4 - max_run) as u8);
            bw.put_bits(u32::from(max_run != run), len);
        }
        return;
    }

    let bin = SIGNIFICANT_RUN_BIN[max_run as usize] as usize;
    let idx = A_INDEX[(run as usize) + bin * 14 - 1];
    let flc = SIGNIFICANT_RUN_FIXED_LENGTH[idx + bin * 5];
    ah0.put_symbol(bw, idx);
    bw.put_bits((run + 1) as u32, flc);
}

pub(super) fn decode_significant_run(
    max_run: i32,
    ah0: &AdaptiveHuffman,
    br: &mut BitReader<'_>,
) -> Result<i32, BitmapError> {
    const A_REMAP: [i32; 15] = [1, 2, 3, 5, 7, 1, 2, 3, 5, 7, 1, 2, 3, 4, 5];

    if max_run <= 0 {
        return Err(error("invalid JPEG-XR run bound"));
    }
    if max_run < 5 {
        if max_run == 1 || br.get_bool()? {
            return Ok(1);
        } else if max_run == 2 || br.get_bool()? {
            return Ok(2);
        } else if max_run == 3 || br.get_bool()? {
            return Ok(3);
        }
        return Ok(4);
    }

    let bin = SIGNIFICANT_RUN_BIN[max_run.min(14) as usize] as usize;
    let sym = br.get_huff(ah0.dec_table)?;
    let idx = sym + bin * 5;
    let mut run = A_REMAP[idx];
    let flc = SIGNIFICANT_RUN_FIXED_LENGTH[idx];
    if flc > 0 {
        run += br.get_bits(flc)? as i32;
    }
    Ok(run)
}

pub(super) fn encode_block(
    is_chroma: bool,
    local_coef: &[i32; 32],
    num_nonzero: usize,
    ah_expt: &mut [AdaptiveHuffman; NUMVLCTABLES],
    ctx_offset: usize,
    bw: &mut BitWriter,
    mut location: i32,
) {
    const G_CODE_15: [u32; 4] = [0, 6, 2, 7];
    const G_LEN_15: [u8; 4] = [1, 3, 2, 3];

    let lev0 = local_coef[1];
    let sr = i32::from(local_coef[0] == 0);
    let sl = i32::from((lev0 + 1) as u32 > 2);
    let srn = if num_nonzero == 1 {
        0
    } else if local_coef[2] > 0 {
        2
    } else {
        1
    };
    let idx0 = (srn * 4 + sl * 2 + sr) as usize;
    {
        let ah = &mut ah_expt[ctx_offset + usize::from(is_chroma) * 3];
        ah.add_both_deltas(idx0);
        ah.put_symbol_with_sign(bw, idx0, lev0 < 0);
    }

    let mut cont = (sr & srn) as usize;
    if sl != 0 {
        encode_significant_abs_level(
            (lev0.unsigned_abs()) - 1,
            &mut ah_expt[6 + ctx_offset + cont],
            bw,
        );
    }
    if sr == 0 {
        encode_significant_run(local_coef[0], 15 - location, &ah_expt[0], bw);
    }
    location += local_coef[0] + 1;

    let mut prev_srn = srn;
    for k in 1..num_nonzero {
        if prev_srn == 2 {
            encode_significant_run(local_coef[k * 2], 15 - location, &ah_expt[0], bw);
        }
        location += local_coef[k * 2] + 1;
        let cur_srn = if k + 1 == num_nonzero {
            0
        } else if local_coef[k * 2 + 2] > 0 {
            2
        } else {
            1
        };
        let lev = local_coef[k * 2 + 1];
        let cur_sl = i32::from((lev + 1) as u32 > 2);
        let idx = (cur_srn * 2 + cur_sl) as usize;
        if location < 15 {
            let ah = &mut ah_expt[ctx_offset + cont + 1 + usize::from(is_chroma) * 3];
            ah.add_both_deltas(idx);
            ah.put_symbol_with_sign(bw, idx, lev < 0);
        } else if location == 15 {
            bw.put_bits(G_CODE_15[idx] * 2 + u32::from(lev < 0), G_LEN_15[idx] + 1);
        } else {
            bw.put_bits((idx as u32) * 2 + u32::from(lev < 0), 2);
        }

        cont &= cur_srn as usize;
        if cur_sl != 0 {
            encode_significant_abs_level(
                (lev.unsigned_abs()) - 1,
                &mut ah_expt[6 + ctx_offset + cont],
                bw,
            );
        }
        prev_srn = cur_srn;
    }
}

pub(super) fn decode_index_at_loc(
    loc: i32,
    ah: &mut AdaptiveHuffman,
    br: &mut BitReader<'_>,
) -> Result<usize, BitmapError> {
    if loc < 15 {
        let idx = br.get_huff(ah.dec_table)?;
        ah.add_both_deltas(idx);
        Ok(idx)
    } else if loc == 15 {
        if !br.get_bool()? {
            Ok(0)
        } else if !br.get_bool()? {
            Ok(2)
        } else if br.get_bool()? {
            Ok(3)
        } else {
            Ok(1)
        }
    } else {
        Ok(br.get_bits(1)? as usize)
    }
}

pub(super) fn decode_block_lp(
    is_chroma: bool,
    local_coef: &mut [i32; 32],
    ah_expt: &mut [AdaptiveHuffman; NUMVLCTABLES],
    ctx_offset: usize,
    br: &mut BitReader<'_>,
    mut location: i32,
) -> Result<usize, BitmapError> {
    let base_ah = ctx_offset + usize::from(is_chroma) * 3;
    let first_idx = {
        let ah = &mut ah_expt[base_ah];
        let idx = br.get_huff(ah.dec_table)?;
        ah.add_both_deltas(idx);
        idx
    };
    let mut sr = (first_idx & 1) as i32;
    let mut srn = (first_idx >> 2) as i32;
    let mut cont = (sr & srn) as usize;
    let sign0 = br.get_sign_mask()?;

    if (first_idx & 2) != 0 {
        let abs_lvl = decode_significant_abs_level(&mut ah_expt[6 + ctx_offset + cont], br)?;
        local_coef[1] = (abs_lvl ^ sign0) - sign0;
    } else {
        local_coef[1] = 1 | sign0;
    }
    local_coef[0] = if sr == 0 {
        decode_significant_run(15 - location, &ah_expt[0], br)?
    } else {
        0
    };
    location += local_coef[0] + 1;

    let mut num_nonzero = 1usize;
    while srn != 0 {
        if num_nonzero >= 16 {
            return Err(error("JPEG-XR lowpass run-level count overflow"));
        }
        sr = srn & 1;
        local_coef[num_nonzero * 2] = if sr == 0 {
            decode_significant_run(15 - location, &ah_expt[0], br)?
        } else {
            0
        };
        location += local_coef[num_nonzero * 2] + 1;
        let idx = decode_index_at_loc(location, &mut ah_expt[base_ah + cont + 1], br)?;
        srn = (idx >> 1) as i32;
        if srn >= 3 {
            return Err(error("invalid JPEG-XR SRn symbol"));
        }
        cont &= srn as usize;
        let sign = br.get_sign_mask()?;
        if (idx & 1) != 0 {
            let abs_lvl = decode_significant_abs_level(&mut ah_expt[6 + ctx_offset + cont], br)?;
            local_coef[num_nonzero * 2 + 1] = (abs_lvl ^ sign) - sign;
        } else {
            local_coef[num_nonzero * 2 + 1] = 1 | sign;
        }
        num_nonzero += 1;
    }
    Ok(num_nonzero)
}

pub(super) fn decode_block_highpass(
    is_chroma: bool,
    ah_expt: &mut [AdaptiveHuffman; NUMVLCTABLES],
    br: &mut BitReader<'_>,
    qp_shifted: i32,
    coeffs: &mut [i32],
    scan: &mut [AdaptiveScan; 16],
) -> Result<usize, BitmapError> {
    let ctx_offset = CTDC + CONTEXTX;
    let base_ah = ctx_offset + usize::from(is_chroma) * 3;
    let mut loc = 1i32;

    let first_idx = {
        let ah = &mut ah_expt[base_ah];
        let idx = br.get_huff(ah.dec_table)?;
        ah.add_both_deltas(idx);
        idx
    };
    let mut sr = (first_idx & 1) as i32;
    let mut srn = (first_idx >> 2) as i32;
    let mut cont = (sr & srn) as usize;
    let sign0 = br.get_sign_mask()?;

    let mut level = (qp_shifted ^ sign0) - sign0;
    if (first_idx & 2) != 0 {
        level *= decode_significant_abs_level(&mut ah_expt[6 + ctx_offset + cont], br)?;
    }
    if sr == 0 {
        loc += decode_significant_run(15 - loc, &ah_expt[0], br)?;
    }
    let uloc = (loc as usize) & 0xF;
    coeffs[scan[uloc].scan] = level;
    scan[uloc].total += 1;
    if uloc > 0 && scan[uloc].total > scan[uloc - 1].total {
        scan.swap(uloc, uloc - 1);
    }
    loc = ((uloc + 1) & 0xF) as i32;

    let mut num_nonzero = 1usize;
    while srn != 0 {
        sr = srn & 1;
        if sr == 0 {
            loc += decode_significant_run(15 - loc, &ah_expt[0], br)?;
            if loc >= 16 {
                return Err(error("JPEG-XR highpass run exceeded 4x4 block"));
            }
        }
        let idx = decode_index_at_loc(loc + 1, &mut ah_expt[base_ah + cont + 1], br)?;
        srn = (idx >> 1) as i32;
        if srn >= 3 {
            return Err(error("invalid JPEG-XR highpass SRn symbol"));
        }
        cont &= srn as usize;
        let sign = br.get_sign_mask()?;
        level = (qp_shifted ^ sign) - sign;
        if (idx & 1) != 0 {
            level *= decode_significant_abs_level(&mut ah_expt[6 + ctx_offset + cont], br)?;
        }
        let uloc = (loc as usize) & 0xF;
        coeffs[scan[uloc].scan] = level;
        scan[uloc].total += 1;
        if uloc > 0 && scan[uloc].total > scan[uloc - 1].total {
            scan.swap(uloc, uloc - 1);
        }
        loc = ((uloc + 1) & 0xF) as i32;
        num_nonzero += 1;
    }
    Ok(num_nonzero)
}

pub(super) fn adaptive_scan(
    coeffs: &[i32],
    residual: &mut [i32; 16],
    scan: &mut [AdaptiveScan; 16],
    model_bits: i32,
    trim_bits: i32,
    rl_coeffs: &mut [i32; 32],
) -> usize {
    if model_bits == 0 {
        let mut run = 1i32;
        let mut num_nonzero = 0usize;
        let lvl1 = coeffs[scan[1].scan];
        if lvl1 != 0 {
            scan[1].total += 1;
            rl_coeffs[0] = 0;
            rl_coeffs[1] = lvl1;
            num_nonzero = 1;
            run = 0;
        }
        for k in 2..16 {
            let lvl = coeffs[scan[k].scan];
            run += 1;
            if lvl != 0 {
                scan[k].total += 1;
                if scan[k].total > scan[k - 1].total {
                    scan.swap(k, k - 1);
                }
                rl_coeffs[num_nonzero * 2] = run - 1;
                rl_coeffs[num_nonzero * 2 + 1] = lvl;
                num_nonzero += 1;
                run = 0;
            }
        }
        num_nonzero
    } else if model_bits <= trim_bits {
        let mut run = 1i32;
        let mut num_nonzero = 0usize;
        let th_off = (1u32 << (model_bits as u32)) - 1;
        let th = th_off * 2 + 1;
        let lvl1 = coeffs[scan[1].scan];
        if (lvl1 as u32).wrapping_add(th_off) >= th {
            let tmp = (lvl1.abs()) >> model_bits;
            scan[1].total += 1;
            rl_coeffs[0] = 0;
            rl_coeffs[1] = if lvl1 < 0 { -tmp } else { tmp };
            num_nonzero = 1;
            run = 0;
        }
        for k in 2..16 {
            run += 1;
            let lvl = coeffs[scan[k].scan];
            if (lvl as u32).wrapping_add(th_off) >= th {
                let tmp = (lvl.abs()) >> model_bits;
                scan[k].total += 1;
                if scan[k].total > scan[k - 1].total {
                    scan.swap(k, k - 1);
                }
                rl_coeffs[num_nonzero * 2] = run - 1;
                rl_coeffs[num_nonzero * 2 + 1] = if lvl < 0 { -tmp } else { tmp };
                num_nonzero += 1;
                run = 0;
            }
        }
        num_nonzero
    } else if trim_bits == 0 && model_bits < 6 {
        let mut run = 0i32;
        let mut num_nonzero = 0usize;
        let th_off = (1u32 << (model_bits as u32)) - 1;
        let th = th_off * 2 + 1;
        let sk1 = scan[1].scan;
        let lvl1 = coeffs[sk1];
        if (lvl1 as u32).wrapping_add(th_off) >= th {
            let abs_v = lvl1.abs();
            let tmp = abs_v >> model_bits;
            residual[sk1] = (abs_v & (th_off as i32)) * 2;
            scan[1].total += 1;
            rl_coeffs[0] = 0;
            rl_coeffs[1] = if lvl1 < 0 { -tmp } else { tmp };
            num_nonzero = 1;
        } else {
            run = 1;
            residual[sk1] = G_RES_LUT[(lvl1 + 32) as usize];
        }
        for k in 2..16 {
            let sk = scan[k].scan;
            let lvl = coeffs[sk];
            if (lvl as u32).wrapping_add(th_off) >= th {
                let abs_v = lvl.abs();
                let tmp = abs_v >> model_bits;
                residual[sk] = (abs_v & (th_off as i32)) * 2;
                scan[k].total += 1;
                if scan[k].total > scan[k - 1].total {
                    scan.swap(k, k - 1);
                }
                rl_coeffs[num_nonzero * 2] = run;
                rl_coeffs[num_nonzero * 2 + 1] = if lvl < 0 { -tmp } else { tmp };
                num_nonzero += 1;
                run = 0;
            } else {
                run += 1;
                residual[sk] = G_RES_LUT[(lvl + 32) as usize];
            }
        }
        num_nonzero
    } else {
        let mut run = 0i32;
        let mut num_nonzero = 0usize;
        let th_off = (1u32 << (model_bits as u32)) - 1;
        let th = th_off * 2 + 1;
        let sk1 = scan[1].scan;
        let lvl1 = coeffs[sk1];
        if (lvl1 as u32).wrapping_add(th_off) >= th {
            let abs_v = lvl1.abs();
            let tmp = abs_v >> model_bits;
            residual[sk1] = ((abs_v & (th_off as i32)) >> trim_bits) * 2;
            scan[1].total += 1;
            rl_coeffs[0] = 0;
            rl_coeffs[1] = if lvl1 < 0 { -tmp } else { tmp };
            num_nonzero = 1;
        } else {
            run = 1;
            let sign = -i32::from(lvl1 < 0);
            let rounded = ((lvl1 + sign) >> trim_bits) - sign;
            let rsign = -i32::from(rounded < 0);
            residual[sk1] = (rounded ^ rsign) * 4 + (6 & rsign) + i32::from(rounded != 0);
        }
        for k in 2..16 {
            let sk = scan[k].scan;
            let lvl = coeffs[sk];
            if (lvl as u32).wrapping_add(th_off) >= th {
                let abs_v = lvl.abs();
                let tmp = abs_v >> model_bits;
                residual[sk] = ((abs_v & (th_off as i32)) >> trim_bits) * 2;
                scan[k].total += 1;
                if scan[k].total > scan[k - 1].total {
                    scan.swap(k, k - 1);
                }
                rl_coeffs[num_nonzero * 2] = run;
                rl_coeffs[num_nonzero * 2 + 1] = if lvl < 0 { -tmp } else { tmp };
                num_nonzero += 1;
                run = 0;
            } else {
                run += 1;
                let sign = -i32::from(lvl < 0);
                let rounded = ((lvl + sign) >> trim_bits) - sign;
                let rsign = -i32::from(rounded < 0);
                residual[sk] = (rounded ^ rsign) * 4 + (6 & rsign) + i32::from(rounded != 0);
            }
        }
        num_nonzero
    }
}

pub(super) fn encode_macroblock_dc(
    cf: ColorFormat,
    channels: usize,
    subband: Subband,
    reset_context: bool,
    mb: &MbInfo,
    ctx: &mut CodingContext,
    bw: &mut BitWriter,
) {
    let mut lap_mean = [0i32, 0i32];
    if cf == ColorFormat::YOnly {
        for j in 0..channels {
            let model_bits = ctx.model_dc.flc_bits[usize::from(j > 0)] as u8;
            let dc = mb.block_dc[j][0];
            let is_neg = dc < 0;
            let abs_dc = dc.unsigned_abs();
            let q_dc = abs_dc >> model_bits;
            if q_dc != 0 {
                bw.put_bit(true);
                encode_significant_abs_level(q_dc, &mut ctx.ah_expt[3], bw);
                lap_mean[usize::from(j > 0)] += 1;
            } else {
                bw.put_bit(false);
            }
            bw.put_bits(abs_dc, model_bits);
            if abs_dc != 0 {
                bw.put_bit(is_neg);
            }
        }
    } else {
        let mb0 = ctx.model_dc.flc_bits[0] as u8;
        let mb1 = ctx.model_dc.flc_bits[1] as u8;
        let dc_y = mb.block_dc[0][0];
        let dc_u = mb.block_dc[1][0];
        let dc_v = mb.block_dc[2][0];
        let q_y = dc_y.unsigned_abs() >> mb0;
        let q_u = dc_u.unsigned_abs() >> mb1;
        let q_v = dc_v.unsigned_abs() >> mb1;

        let idx = usize::from(q_y != 0) * 4 + usize::from(q_u != 0) * 2 + usize::from(q_v != 0);
        ctx.ah_expt[2].put_symbol(bw, idx);

        if q_y != 0 {
            encode_significant_abs_level(q_y, &mut ctx.ah_expt[3], bw);
            lap_mean[0] += 1;
        }
        bw.put_bits(dc_y.unsigned_abs(), mb0);
        if dc_y != 0 {
            bw.put_bit(dc_y < 0);
        }

        if q_u != 0 {
            encode_significant_abs_level(q_u, &mut ctx.ah_expt[4], bw);
            lap_mean[1] += 1;
        }
        bw.put_bits(dc_u.unsigned_abs(), mb1);
        if dc_u != 0 {
            bw.put_bit(dc_u < 0);
        }

        if q_v != 0 {
            encode_significant_abs_level(q_v, &mut ctx.ah_expt[4], bw);
            lap_mean[1] += 1;
        }
        bw.put_bits(dc_v.unsigned_abs(), mb1);
        if dc_v != 0 {
            bw.put_bit(dc_v < 0);
        }
    }

    ctx.model_dc.update_mb(cf, channels, lap_mean);
    if reset_context && subband == Subband::DcOnly {
        ctx.ah_expt[2].adapt_discriminant();
        ctx.ah_expt[3].adapt_discriminant();
        ctx.ah_expt[4].adapt_discriminant();
    }
}

pub(super) fn decode_qp_index(br: &mut BitReader<'_>, bits: u8) -> Result<u8, BitmapError> {
    if !br.get_bool()? {
        Ok(0)
    } else {
        Ok((br.get_bits(bits)? as u8) + 1)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_macroblock_dc(
    cf: ColorFormat,
    channels: usize,
    bitstream_fmt: BitstreamFormat,
    subband: Subband,
    tile: &TileQuantizers,
    reset_context: bool,
    mb: &mut MbInfo,
    ctx: &mut CodingContext,
    br: &mut BitReader<'_>,
) -> Result<(), BitmapError> {
    for ch in 0..channels {
        mb.block_dc[ch] = [0; 16];
    }
    mb.q_index_lp = 0;
    mb.q_index_hp = 0;

    if bitstream_fmt == BitstreamFormat::Spatial && subband != Subband::DcOnly {
        if tile.bits_lp > 0 {
            mb.q_index_lp = decode_qp_index(br, tile.bits_lp)?;
        }
        if subband != Subband::NoHighpass && tile.bits_hp > 0 {
            mb.q_index_hp = decode_qp_index(br, tile.bits_hp)?;
        }
    }
    if tile.bits_hp == 0 && tile.num_qp_hp > 1 {
        mb.q_index_hp = mb.q_index_lp;
    }
    if mb.q_index_lp >= tile.num_qp_lp || mb.q_index_hp >= tile.num_qp_hp {
        return Err(error("JPEG-XR macroblock DQUANT index out of bounds"));
    }

    let mut lap_mean = [0i32, 0i32];
    if cf == ColorFormat::YOnly {
        for ch in 0..channels {
            let mbits = ctx.model_dc.flc_bits[usize::from(ch > 0)] as u8;
            let mut q_dc = if br.get_bool()? {
                lap_mean[usize::from(ch > 0)] += 1;
                decode_significant_abs_level(&mut ctx.ah_expt[3], br)? - 1
            } else {
                0
            };
            if mbits > 0 {
                q_dc = (q_dc << mbits) | (br.get_bits(mbits)? as i32);
            }
            if q_dc != 0 && br.get_bool()? {
                q_dc = -q_dc;
            }
            mb.block_dc[ch][0] = q_dc;
        }
    } else {
        let mb0 = ctx.model_dc.flc_bits[0] as u8;
        let mb1 = ctx.model_dc.flc_bits[1] as u8;
        let idx = br.get_huff(ctx.ah_expt[2].dec_table)?;
        let mut q_y = (idx >> 2) as i32;
        let mut q_u = ((idx >> 1) & 1) as i32;
        let mut q_v = (idx & 1) as i32;

        if q_y != 0 {
            q_y = decode_significant_abs_level(&mut ctx.ah_expt[3], br)? - 1;
            lap_mean[0] += 1;
        }
        if mb0 > 0 {
            q_y = (q_y << mb0) | (br.get_bits(mb0)? as i32);
        }
        if q_y != 0 && br.get_bool()? {
            q_y = -q_y;
        }
        mb.block_dc[0][0] = q_y;

        if q_u != 0 {
            q_u = decode_significant_abs_level(&mut ctx.ah_expt[4], br)? - 1;
            lap_mean[1] += 1;
        }
        if mb1 > 0 {
            q_u = (q_u << mb1) | (br.get_bits(mb1)? as i32);
        }
        if q_u != 0 && br.get_bool()? {
            q_u = -q_u;
        }
        mb.block_dc[1][0] = q_u;

        if q_v != 0 {
            q_v = decode_significant_abs_level(&mut ctx.ah_expt[4], br)? - 1;
            lap_mean[1] += 1;
        }
        if mb1 > 0 {
            q_v = (q_v << mb1) | (br.get_bits(mb1)? as i32);
        }
        if q_v != 0 && br.get_bool()? {
            q_v = -q_v;
        }
        mb.block_dc[2][0] = q_v;
    }

    ctx.model_dc.update_mb(cf, channels, lap_mean);
    if reset_context && (bitstream_fmt == BitstreamFormat::Frequency || subband == Subband::DcOnly)
    {
        ctx.ah_expt[2].adapt_discriminant();
        ctx.ah_expt[3].adapt_discriminant();
        ctx.ah_expt[4].adapt_discriminant();
    }
    Ok(())
}

pub(super) fn encode_macroblock_lowpass(
    cf: ColorFormat,
    channels: usize,
    reset_rgi_totals: bool,
    reset_context: bool,
    mb: &MbInfo,
    ctx: &mut CodingContext,
    bw: &mut BitWriter,
) {
    if reset_rgi_totals {
        ctx.reset_lowpass_scan_totals();
    }

    let scan_full_ch = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    let mut rl_coeffs = [[0i32; 32]; MAX_CHANNELS];
    let mut num_coeffs = [0usize; MAX_CHANNELS];
    let mut residuals = [[0i32; 16]; MAX_CHANNELS];
    let mut uv_buf = [[0i32; 8]; 2];

    for ch in 0..scan_full_ch {
        let mbits = ctx.model_lp.flc_bits[usize::from(ch > 0)];
        num_coeffs[ch] = adaptive_scan(
            &mb.block_dc[ch],
            &mut residuals[ch],
            &mut ctx.scan_lowpass,
            mbits,
            0,
            &mut rl_coeffs[ch],
        );
    }

    if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        const A_REMAP: [usize; 7] = [4, 1, 2, 3, 5, 6, 7];
        let remap_off = usize::from(cf == ColorFormat::Yuv420);
        let count = if cf == ColorFormat::Yuv420 { 6 } else { 14 };
        let mbits = ctx.model_lp.flc_bits[1];
        let mut run = 0i32;
        let mut coef = 0usize;
        for k in 0..count {
            let idx = A_REMAP[remap_off + (k >> 1)];
            let dc = mb.block_dc[(k & 1) + 1][idx];
            let val = dc.abs() >> mbits;
            uv_buf[k & 1][idx] = val;
            if val != 0 {
                rl_coeffs[1][coef * 2] = run;
                rl_coeffs[1][coef * 2 + 1] = if dc < 0 { -val } else { val };
                coef += 1;
                run = 0;
            } else {
                run += 1;
            }
        }
        num_coeffs[1] = coef;
    }

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        2
    } else {
        channels
    };

    if cf != ColorFormat::YOnly {
        let max_cbp = (full_planes as i32) * 4 - 5; // 3 for 2 planes, 7 for 3 planes
        let mut count_m = ctx.cbp_count_max;
        let mut count_z = ctx.cbp_count_zero;
        let mut cbp = i32::from(num_coeffs[0] > 0) + i32::from(num_coeffs[1] > 0) * 2;
        if full_planes == 3 {
            cbp += i32::from(num_coeffs[2] > 0) * 4;
        }

        if count_z <= 0 || count_m < 0 {
            let val = if count_m < count_z {
                max_cbp - cbp
            } else {
                cbp
            };
            if val == 0 {
                bw.put_bits(0, 1);
            } else if val == 1 {
                bw.put_bits(((full_planes as u32) + 1) & 0x6, full_planes as u8);
            } else {
                bw.put_bits((val + max_cbp + 1) as u32, (full_planes + 1) as u8);
            }
        } else {
            bw.put_bits(cbp as u32, full_planes as u8);
        }

        count_m = (count_m + 1 - 4 * i32::from(cbp == max_cbp)).clamp(-8, 7);
        count_z = (count_z + 1 - 4 * i32::from(cbp == 0)).clamp(-8, 7);
        ctx.cbp_count_max = count_m;
        ctx.cbp_count_zero = count_z;
    } else {
        for ch in 0..channels {
            bw.put_bit(num_coeffs[ch] > 0);
        }
    }

    let mut lap_mean = [0i32, 0i32];
    for ch in 0..full_planes {
        let mbits = ctx.model_lp.flc_bits[usize::from(ch > 0)] as u8;
        let coef = num_coeffs[ch];
        if coef > 0 {
            lap_mean[usize::from(ch > 0)] += coef as i32;
            let loc = 1
                + 9 * i32::from(cf == ColorFormat::Yuv420 && ch == 1)
                + i32::from(cf == ColorFormat::Yuv422 && ch == 1);
            encode_block(
                ch > 0,
                &rl_coeffs[ch],
                coef,
                &mut ctx.ah_expt,
                CTDC,
                bw,
                loc,
            );
        }

        if mbits > 0 {
            if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) && ch > 0 {
                let k_end = if cf == ColorFormat::Yuv420 { 4 } else { 8 };
                for k in 1..k_end {
                    let u_dc = mb.block_dc[1][k];
                    bw.put_bits(u_dc.unsigned_abs(), mbits);
                    if uv_buf[0][k] == 0 && u_dc != 0 {
                        bw.put_bit(u_dc < 0);
                    }
                    let v_dc = mb.block_dc[2][k];
                    bw.put_bits(v_dc.unsigned_abs(), mbits);
                    if uv_buf[1][k] == 0 && v_dc != 0 {
                        bw.put_bit(v_dc < 0);
                    }
                }
            } else {
                for k in 1..16 {
                    let res = residuals[ch][k];
                    bw.put_bits((res >> 1) as u32, mbits + ((res & 1) as u8));
                }
            }
        }
    }

    ctx.model_lp.update_mb(cf, channels, lap_mean);
    if reset_context {
        ctx.adapt_lowpass();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_macroblock_lowpass(
    cf: ColorFormat,
    channels: usize,
    bitstream_fmt: BitstreamFormat,
    tile: &TileQuantizers,
    reset_rgi_totals: bool,
    reset_context: bool,
    mb: &mut MbInfo,
    ctx: &mut CodingContext,
    br: &mut BitReader<'_>,
) -> Result<(), BitmapError> {
    if bitstream_fmt != BitstreamFormat::Spatial && tile.bits_lp > 0 {
        mb.q_index_lp = decode_qp_index(br, tile.bits_lp)?;
        if mb.q_index_lp >= tile.num_qp_lp {
            return Err(error("JPEG-XR lowpass DQUANT index out of bounds"));
        }
    }

    if reset_rgi_totals {
        ctx.reset_lowpass_scan_totals();
    }

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        2
    } else {
        channels
    };

    let mut cbp = 0i32;
    if cf != ColorFormat::YOnly {
        let mut count_m = ctx.cbp_count_max;
        let mut count_z = ctx.cbp_count_zero;
        let max_cbp = (full_planes as i32) * 4 - 5;
        if count_z <= 0 || count_m < 0 {
            if br.get_bool()? {
                cbp = 1;
                let k = br.get_bits((full_planes - 1) as u8)? as i32;
                if k != 0 {
                    cbp = k * 2 + (br.get_bits(1)? as i32);
                }
            }
            if count_m < count_z {
                cbp = max_cbp - cbp;
            }
        } else {
            cbp = br.get_bits(full_planes as u8)? as i32;
        }

        count_m = (count_m + 1 - 4 * i32::from(cbp == max_cbp)).clamp(-8, 7);
        count_z = (count_z + 1 - 4 * i32::from(cbp == 0)).clamp(-8, 7);
        ctx.cbp_count_max = count_m;
        ctx.cbp_count_zero = count_z;
    } else {
        for ch in 0..channels {
            cbp |= (br.get_bits(1)? as i32) << ch;
        }
    }

    let mut lap_mean = [0i32, 0i32];
    let mut rl_coeffs = [0i32; 32];
    for ch in 0..full_planes {
        let mbits = ctx.model_lp.flc_bits[usize::from(ch > 0)] as u8;
        if (cbp & 1) != 0 {
            let loc = 1
                + 9 * i32::from(cf == ColorFormat::Yuv420 && ch == 1)
                + i32::from(cf == ColorFormat::Yuv422 && ch == 1);
            let num_nz = decode_block_lp(ch > 0, &mut rl_coeffs, &mut ctx.ah_expt, CTDC, br, loc)?;
            lap_mean[usize::from(ch > 0)] += num_nz as i32;

            if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) && ch > 0 {
                const A_REMAP: [usize; 7] = [4, 1, 2, 3, 5, 6, 7];
                let remap_off = usize::from(cf == ColorFormat::Yuv420);
                let count = if cf == ColorFormat::Yuv420 { 6 } else { 14 };
                let mut temp = [0i32; 16];
                let mut idx = 0usize;
                for k in 0..num_nz {
                    idx = idx.saturating_add(rl_coeffs[k * 2] as usize);
                    temp[idx & 0xF] = rl_coeffs[k * 2 + 1];
                    idx += 1;
                }
                for k in 0..count {
                    mb.block_dc[(k & 1) + 1][A_REMAP[remap_off + (k >> 1)]] = temp[k];
                }
            } else {
                let mut idx = 1usize;
                for k in 0..num_nz {
                    idx = idx.saturating_add(rl_coeffs[k * 2] as usize);
                    if idx >= 16 {
                        return Err(error("JPEG-XR lowpass scan index overflow"));
                    }
                    let sc = ctx.scan_lowpass[idx].scan;
                    mb.block_dc[ch][sc] = rl_coeffs[k * 2 + 1];
                    ctx.scan_lowpass[idx].total += 1;
                    if ctx.scan_lowpass[idx].total > ctx.scan_lowpass[idx - 1].total {
                        ctx.scan_lowpass.swap(idx, idx - 1);
                    }
                    idx += 1;
                }
            }
        }

        if mbits > 0 {
            if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) && ch > 0 {
                let k_end = if cf == ColorFormat::Yuv420 { 4 } else { 8 };
                for k in 1..k_end {
                    for uv_ch in 1..=2 {
                        let cur = mb.block_dc[uv_ch][k];
                        if cur > 0 {
                            mb.block_dc[uv_ch][k] = (cur << mbits) + (br.get_bits(mbits)? as i32);
                        } else if cur < 0 {
                            mb.block_dc[uv_ch][k] = (cur << mbits) - (br.get_bits(mbits)? as i32);
                        } else {
                            let mut fine = br.get_bits(mbits)? as i32;
                            if fine != 0 && br.get_bool()? {
                                fine = -fine;
                            }
                            mb.block_dc[uv_ch][k] = fine;
                        }
                    }
                }
            } else {
                for k in 1..16 {
                    let cur = mb.block_dc[ch][k];
                    if cur > 0 {
                        mb.block_dc[ch][k] = (cur << mbits) + (br.get_bits(mbits)? as i32);
                    } else if cur < 0 {
                        mb.block_dc[ch][k] = (cur << mbits) - (br.get_bits(mbits)? as i32);
                    } else {
                        mb.block_dc[ch][k] = br.get_bits_signed(mbits)?;
                    }
                }
            }
        }

        cbp >>= 1;
    }

    ctx.model_lp.update_mb(cf, channels, lap_mean);
    if reset_context {
        ctx.adapt_lowpass();
    }
    Ok(())
}
