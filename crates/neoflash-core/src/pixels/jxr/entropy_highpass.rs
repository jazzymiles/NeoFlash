use super::bitstream::*;
use super::entropy_lowpass::*;
use super::prediction::{pred_cbp_dec, pred_cbp_enc};
use super::tables::*;
use super::*;

pub(super) fn code_cbp_enc(
    cf: ColorFormat,
    mb: &MbInfo,
    ctx: &mut CodingContext,
    bw: &mut BitWriter,
) {
    const A_NUM_ONES: [usize; 16] = [0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4];
    const A_TAB_LEN: [u8; 16] = [0, 2, 2, 2, 2, 2, 3, 2, 2, 3, 3, 2, 3, 2, 2, 0];
    const A_TAB_CODE: [u32; 16] = [0, 0, 1, 0, 2, 1, 4, 3, 3, 5, 6, 2, 7, 1, 0, 0];
    const G_TAB0: [usize; 16] = [0, 1, 1, 2, 1, 3, 3, 4, 1, 3, 3, 4, 2, 4, 4, 5];
    const G_FL0: [u8; 16] = [0, 2, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2, 1, 2, 2, 0];
    const G_CODE0: [u32; 16] = [0, 0, 1, 0, 2, 0, 1, 0, 3, 2, 3, 1, 1, 2, 3, 0];

    let mut diff_y = mb.diff_cbp[0];
    let mut diff_u = mb.diff_cbp[1];
    let mut diff_v = mb.diff_cbp[2];

    if cf == ColorFormat::Yuv420 {
        diff_y = (diff_y & 0xF)
            + ((diff_u & 1) << 4)
            + ((diff_v & 1) << 5)
            + ((diff_y & 0x00F0) << 2)
            + ((diff_u & 2) << 9)
            + ((diff_v & 2) << 10)
            + ((diff_y & 0x0F00) << 4)
            + ((diff_u & 4) << 14)
            + ((diff_v & 4) << 15)
            + ((diff_y & 0xF000) << 6)
            + ((diff_u & 8) << 19)
            + ((diff_v & 8) << 20);
    } else if cf == ColorFormat::Yuv422 {
        diff_y = (diff_y & 0xF)
            + ((diff_u & 1) << 4)
            + ((diff_u & 4) << 3)
            + ((diff_v & 1) << 6)
            + ((diff_v & 4) << 5)
            + ((diff_y & 0x00F0) << 4)
            + ((diff_u & 2) << 11)
            + ((diff_u & 8) << 10)
            + ((diff_v & 2) << 13)
            + ((diff_v & 8) << 12)
            + ((diff_y & 0x0F00) << 8)
            + ((diff_u & 16) << 16)
            + ((diff_u & 64) << 15)
            + ((diff_v & 16) << 18)
            + ((diff_v & 64) << 17)
            + ((diff_y & 0xF000) << 12)
            + ((diff_u & 32) << 23)
            + ((diff_u & 128) << 22)
            + ((diff_v & 32) << 25)
            + ((diff_v & 128) << 24);
    }

    let mut pattern = 0usize;
    let mut dy = diff_y;
    if cf == ColorFormat::Yuv444 {
        dy |= diff_u | diff_v;
    }
    for _ in 0..4 {
        let non_zero = match cf {
            ColorFormat::Yuv422 => {
                let nz = (dy & 0xFF) != 0;
                dy >>= 8;
                nz
            }
            ColorFormat::Yuv420 => {
                let nz = (dy & 0x3F) != 0;
                dy >>= 6;
                nz
            }
            ColorFormat::Yuv444 | ColorFormat::YOnly => {
                let nz = (dy & 0x0F) != 0;
                dy >>= 4;
                nz
            }
        };
        if non_zero {
            pattern |= 0x10;
        }
        pattern >>= 1;
    }

    let count = A_NUM_ONES[pattern];
    ctx.adapt_huff_cbp_cy1.put_symbol(bw, count);
    ctx.adapt_huff_cbp_cy1.add_delta(count);
    if A_TAB_LEN[pattern] > 0 {
        bw.put_bits(A_TAB_CODE[pattern], A_TAB_LEN[pattern]);
    }

    for _ in 0..4 {
        let mut code_u = 0usize;
        let mut code_v = 0usize;
        let raw_code = match cf {
            ColorFormat::Yuv444 => {
                let cy = (diff_y & 0xF) as usize;
                code_u = (diff_u & 0xF) as usize;
                code_v = (diff_v & 0xF) as usize;
                diff_y >>= 4;
                diff_u >>= 4;
                diff_v >>= 4;
                cy | (usize::from(code_u != 0) << 4) | (usize::from(code_v != 0) << 5)
            }
            ColorFormat::Yuv422 => {
                let c = (diff_y & 0xFF) as usize;
                diff_y >>= 8;
                c
            }
            ColorFormat::Yuv420 => {
                let c = (diff_y & 0x3F) as usize;
                diff_y >>= 6;
                c
            }
            ColorFormat::YOnly => {
                let c = (diff_y & 0x0F) as usize;
                diff_y >>= 4;
                c
            }
        };

        if raw_code != 0 {
            let mut chroma = raw_code >> 4;
            let code = raw_code & 0xF;
            if cf == ColorFormat::Yuv422 {
                code_u = chroma & 3;
                code_v = (chroma >> 2) & 3;
                chroma = usize::from(code_u != 0) + usize::from(code_v != 0) * 2;
            }

            let val = if chroma != 0 {
                if G_TAB0[code] > 2 {
                    8
                } else {
                    G_TAB0[code] + 5
                }
            } else {
                G_TAB0[code] - 1
            };
            ctx.adapt_huff_cbp_cy.put_symbol(bw, val);
            ctx.adapt_huff_cbp_cy.add_delta(val);

            if chroma != 0 {
                if chroma == 1 {
                    bw.put_bits(1, 1);
                } else {
                    bw.put_bits((3 - chroma) as u32, 2);
                }
            }
            if val == 8 {
                if G_TAB0[code] == 3 {
                    bw.put_bits(1, 1);
                } else {
                    bw.put_bits((5 - G_TAB0[code]) as u32, 2);
                }
            }
            if G_FL0[code] > 0 {
                bw.put_bits(G_CODE0[code], G_FL0[code]);
            }

            if cf == ColorFormat::Yuv444 {
                for pat in [code_u, code_v] {
                    if pat != 0 {
                        let cnt = A_NUM_ONES[pat] - 1;
                        ctx.ah_expt[1].put_symbol(bw, cnt);
                        if A_TAB_LEN[pat] > 0 {
                            bw.put_bits(A_TAB_CODE[pat], A_TAB_LEN[pat]);
                        }
                    }
                }
            } else if cf == ColorFormat::Yuv422 {
                for pat in [code_u, code_v] {
                    if pat != 0 {
                        if pat == 1 {
                            bw.put_bits(1, 1);
                        } else {
                            bw.put_bits((3 - pat) as u32, 2);
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn decode_cbp(
    cf: ColorFormat,
    mb: &mut MbInfo,
    ctx: &mut CodingContext,
    br: &mut BitReader<'_>,
) -> Result<(), BitmapError> {
    const A_TAB: [i32; 4] = [6, 9, 10, 12];
    const G_FLC0: [u8; 6] = [0, 2, 1, 2, 2, 0];
    const G_OFF0: [usize; 6] = [0, 4, 2, 8, 12, 1];
    const G_OUT0: [i32; 16] = [0, 15, 3, 12, 1, 2, 4, 8, 5, 6, 9, 10, 7, 11, 13, 14];
    const SHIFT_422: [usize; 4] = [0, 1, 4, 5];

    let mut cbp_y = 0i32;
    let mut cbp_u = 0i32;
    let mut cbp_v = 0i32;

    let sym = br.get_huff(ctx.adapt_huff_cbp_cy1.dec_table)?;
    ctx.adapt_huff_cbp_cy1.add_delta(sym);

    let num_cbp = match sym {
        0 => 0,
        1 => 1i32 << br.get_bits(2)?,
        2 => {
            let v = br.get_bits(2)? as usize;
            if v == 0 {
                3
            } else if v == 1 {
                5
            } else {
                A_TAB[v * 2 + usize::from(br.get_bool()?) - 4]
            }
        }
        3 => 0xF ^ (1i32 << br.get_bits(2)?),
        _ => 0xF,
    };

    for blk in 0..4usize {
        if (num_cbp & (1 << blk)) != 0 {
            let bsym = br.get_huff(ctx.adapt_huff_cbp_cy.dec_table)?;
            ctx.adapt_huff_cbp_cy.add_delta(bsym);
            let mut val = bsym + 1;
            let mut num_blk_cbp = 0i32;

            if val >= 6 {
                if br.get_bool()? {
                    num_blk_cbp = 0x10;
                } else if br.get_bool()? {
                    num_blk_cbp = 0x20;
                } else {
                    num_blk_cbp = 0x30;
                }
                if val == 9 {
                    if br.get_bool()? {
                        // val remains 9
                    } else if br.get_bool()? {
                        val = 10;
                    } else {
                        val = 11;
                    }
                }
                val -= 6;
            }
            if val >= 6 {
                return Err(error("invalid JPEG-XR block CBP code"));
            }
            let mut code1 = G_OFF0[val];
            if G_FLC0[val] > 0 {
                code1 += br.get_bits(G_FLC0[val])? as usize;
            }
            num_blk_cbp += G_OUT0[code1];

            match cf {
                ColorFormat::Yuv444 => {
                    cbp_y |= (num_blk_cbp & 0xF) << (blk * 4);
                    for k in 0..2usize {
                        if ((num_blk_cbp >> (k + 4)) & 1) != 0 {
                            let csym = br.get_huff(ctx.ah_expt[1].dec_table)?;
                            let ccode = match csym {
                                0 => 1i32 << br.get_bits(2)?,
                                1 => {
                                    let v = br.get_bits(2)? as usize;
                                    if v == 0 {
                                        3
                                    } else if v == 1 {
                                        5
                                    } else {
                                        A_TAB[v * 2 + usize::from(br.get_bool()?) - 4]
                                    }
                                }
                                2 => 0xF ^ (1i32 << br.get_bits(2)?),
                                _ => 0xF,
                            };
                            if k == 0 {
                                cbp_u |= ccode << (blk * 4);
                            } else {
                                cbp_v |= ccode << (blk * 4);
                            }
                        }
                    }
                }
                ColorFormat::Yuv420 => {
                    cbp_y |= (num_blk_cbp & 0xF) << (blk * 4);
                    cbp_u |= ((num_blk_cbp >> 4) & 1) << blk;
                    cbp_v |= ((num_blk_cbp >> 5) & 1) << blk;
                }
                ColorFormat::Yuv422 => {
                    cbp_y |= (num_blk_cbp & 0xF) << (blk * 4);
                    for k in 0..2usize {
                        if ((num_blk_cbp >> (k + 4)) & 1) != 0 {
                            let ccode = if br.get_bool()? {
                                1i32
                            } else if br.get_bool()? {
                                4i32
                            } else {
                                5i32
                            };
                            if k == 0 {
                                cbp_u |= ccode << SHIFT_422[blk];
                            } else {
                                cbp_v |= ccode << SHIFT_422[blk];
                            }
                        }
                    }
                }
                ColorFormat::YOnly => {
                    cbp_y |= num_blk_cbp << (blk * 4);
                }
            }
        }
    }

    mb.diff_cbp[0] = cbp_y;
    if cf != ColorFormat::YOnly {
        mb.diff_cbp[1] = cbp_u;
        mb.diff_cbp[2] = cbp_v;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn encode_macroblock_highpass(
    cf: ColorFormat,
    channels: usize,
    subband: Subband,
    ctx_left: bool,
    ctx_top: bool,
    reset_rgi_totals: bool,
    reset_context: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    planes: &[&mut [i32]; MAX_CHANNELS],
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
    ctx: &mut CodingContext,
    bw: &mut BitWriter,
) {
    if reset_rgi_totals {
        ctx.reset_highpass_scan_totals();
    }

    pred_cbp_enc(
        cf, channels, ctx_left, ctx_top, mb_x, mb, planes, pred_cur, pred_prev, ctx,
    );
    code_cbp_enc(cf, mb, ctx, bw);

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    let mut model_bits = ctx.model_ac.flc_bits[0];
    let (mut trim, mut flex, mut mask) =
        if ctx.trim_flex_bits <= model_bits && subband != Subband::NoFlexbits {
            let tr = ctx.trim_flex_bits;
            let fl = model_bits - tr;
            (tr, fl, (1i32 << fl) - 1)
        } else {
            (0, 0, 0)
        };

    let mut lap_mean = [0i32, 0i32];
    let mut is_chroma = false;

    for i in 0..full_planes {
        let mut pattern = mb.cbp[i] as u32;
        let n_blocks = match cf {
            ColorFormat::Yuv420 => {
                pattern |= ((mb.cbp[1] as u32) << 16) | ((mb.cbp[2] as u32) << 20);
                6usize
            }
            ColorFormat::Yuv422 => {
                pattern |= ((mb.cbp[1] as u32) << 16) | ((mb.cbp[2] as u32) << 24);
                8usize
            }
            ColorFormat::Yuv444 | ColorFormat::YOnly => 4usize,
        };

        let mut index = 0usize;
        for blk in 0..n_blocks {
            for sub in 0..4usize {
                let coeffs: &[i32] = if blk < 4 {
                    let off = BLK_OFFSET[index];
                    &planes[i][off..off + 16]
                } else if cf == ColorFormat::Yuv420 {
                    let off = BLK_OFFSET_UV[sub];
                    &planes[blk - 3][off..off + 16]
                } else {
                    let off = BLK_OFFSET_UV_422[(blk & 1) * 4 + sub];
                    &planes[1 + ((blk - 4) >> 1)][off..off + 16]
                };

                if (pattern & 1) == 0 {
                    if flex > 0 {
                        for k in 1..16 {
                            let data = coeffs[DCT_INDEX[0][k]];
                            let atdata = (data.abs()) >> trim;
                            let mut word = (atdata & mask) as u32;
                            let mut len = flex as u8;
                            if atdata != 0 {
                                word = word * 2 + u32::from(data < 0);
                                len += 1;
                            }
                            bw.put_bits(word, len);
                        }
                    }
                } else {
                    let mut local_coef = [0i32; 32];
                    let mut residual = [0i32; 16];
                    let scan = if mb.orientation == 1 {
                        &mut ctx.scan_vert
                    } else {
                        &mut ctx.scan_horiz
                    };
                    let num_nz = adaptive_scan(
                        coeffs,
                        &mut residual,
                        scan,
                        model_bits,
                        trim,
                        &mut local_coef,
                    );
                    lap_mean[usize::from(is_chroma)] += num_nz as i32;
                    encode_block(
                        is_chroma,
                        &local_coef,
                        num_nz,
                        &mut ctx.ah_expt,
                        CTDC + CONTEXTX,
                        bw,
                        1,
                    );
                    if flex > 0 {
                        for k in 1..16 {
                            let r = residual[DCT_INDEX[0][k]];
                            bw.put_bits((r >> 1) as u32, (flex as u8) + ((r & 1) as u8));
                        }
                    }
                }

                pattern >>= 1;
                index += 1;
            }

            if blk == 3 {
                model_bits = ctx.model_ac.flc_bits[1];
                is_chroma = true;
                if ctx.trim_flex_bits <= model_bits && subband != Subband::NoFlexbits {
                    trim = ctx.trim_flex_bits;
                    flex = model_bits - trim;
                    mask = (1i32 << flex) - 1;
                } else {
                    trim = 0;
                    flex = 0;
                    mask = 0;
                }
            }
        }
    }

    ctx.model_ac.update_mb(cf, channels, lap_mean);
    if reset_context {
        ctx.adapt_highpass();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_block_adaptive<'a>(
    no_skip: bool,
    is_chroma: bool,
    ah_expt: &mut [AdaptiveHuffman; NUMVLCTABLES],
    io_ac: &mut BitReader<'a>,
    io_fl: &mut Option<&mut BitReader<'a>>,
    coeffs: &mut [i32],
    scan: &mut [AdaptiveScan; 16],
    model_bits: i32,
    trim: i32,
    qp: i32,
    skip_flexbits: bool,
) -> Result<usize, BitmapError> {
    let flex = if model_bits > trim && !skip_flexbits {
        (model_bits - trim) as u8
    } else {
        0
    };

    let num_nz = if no_skip {
        let qp1 = qp << model_bits;
        decode_block_highpass(is_chroma, ah_expt, io_ac, qp1, coeffs, scan)?
    } else {
        0
    };

    if flex > 0 {
        let fl_reader: &mut BitReader<'a> = match io_fl.as_deref_mut() {
            Some(r) => r,
            None => &mut *io_ac,
        };
        let qp1 = qp << trim;
        for k in 1..16 {
            let pos = DCT_INDEX[0][k];
            let cur = coeffs[pos];
            if cur < 0 {
                let fine = fl_reader.get_bits(flex)? as i32;
                coeffs[pos] -= qp1 * fine;
            } else if cur > 0 {
                let fine = fl_reader.get_bits(flex)? as i32;
                coeffs[pos] += qp1 * fine;
            } else {
                let fine = fl_reader.get_bits_signed(flex)?;
                coeffs[pos] = qp1 * fine;
            }
        }
    }

    Ok(num_nz)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_macroblock_highpass<'a>(
    cf: ColorFormat,
    channels: usize,
    bitstream_fmt: BitstreamFormat,
    subband: Subband,
    skip_flexbits: bool,
    tile: &TileQuantizers,
    ctx_left: bool,
    ctx_top: bool,
    reset_rgi_totals: bool,
    reset_context: bool,
    mb_x: usize,
    mb: &mut MbInfo,
    planes: &mut [&mut [i32]; MAX_CHANNELS],
    pred_cur: &mut [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: &[Vec<PredInfo>; MAX_CHANNELS],
    ctx: &mut CodingContext,
    io_ac: &mut BitReader<'a>,
    mut io_fl: Option<&mut BitReader<'a>>,
) -> Result<(), BitmapError> {
    if reset_rgi_totals {
        ctx.reset_highpass_scan_totals();
    }

    if bitstream_fmt != BitstreamFormat::Spatial && tile.bits_hp > 0 {
        mb.q_index_hp = decode_qp_index(io_ac, tile.bits_hp)?;
        if mb.q_index_hp >= tile.num_qp_hp {
            return Err(error("JPEG-XR highpass DQUANT index out of bounds"));
        }
    } else if tile.bits_hp == 0 && tile.num_qp_hp > 1 {
        mb.q_index_hp = mb.q_index_lp;
    }

    decode_cbp(cf, mb, ctx, io_ac)?;
    pred_cbp_dec(
        cf, channels, ctx_left, ctx_top, mb_x, mb, pred_cur, pred_prev, ctx,
    );

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    let mut model_bits = ctx.model_ac.flc_bits[0];
    let mut lap_mean = [0i32, 0i32];
    let mut is_chroma = false;
    let mut cbp_cy = mb.cbp[0] as u32;
    let n_blocks = match cf {
        ColorFormat::Yuv420 => {
            cbp_cy |= ((mb.cbp[1] as u32) << 16) | ((mb.cbp[2] as u32) << 20);
            6usize
        }
        ColorFormat::Yuv422 => {
            cbp_cy |= ((mb.cbp[1] as u32) << 16) | ((mb.cbp[2] as u32) << 24);
            8usize
        }
        ColorFormat::Yuv444 | ColorFormat::YOnly => 4usize,
    };

    let skip_fl = skip_flexbits || subband == Subband::NoFlexbits;
    for i in 0..full_planes {
        let mut index = 0usize;
        for blk in 0..n_blocks {
            let hp_ch = if full_planes > 1 {
                i
            } else if blk > 3 {
                if cf == ColorFormat::Yuv420 {
                    blk - 3
                } else {
                    (blk / 2) - 1
                }
            } else {
                0
            };
            let qp = tile.hp[usize::from(mb.q_index_hp)][hp_ch].qp;

            for sub in 0..4usize {
                let coeffs: &mut [i32] = if blk < 4 {
                    let off = BLK_OFFSET[index & 0xF];
                    &mut planes[i][off..off + 16]
                } else if cf == ColorFormat::Yuv420 {
                    let off = BLK_OFFSET_UV[sub];
                    &mut planes[blk - 3][off..off + 16]
                } else {
                    let off = (blk & 1) * 32 + BLK_OFFSET_UV_422[sub];
                    &mut planes[1 + ((blk >> 1) & 1)][off..off + 16]
                };

                let scan = if mb.orientation == 1 {
                    &mut ctx.scan_vert
                } else {
                    &mut ctx.scan_horiz
                };
                let num_nz = decode_block_adaptive(
                    (cbp_cy & 1) != 0,
                    is_chroma,
                    &mut ctx.ah_expt,
                    io_ac,
                    &mut io_fl,
                    coeffs,
                    scan,
                    model_bits,
                    ctx.trim_flex_bits,
                    qp,
                    skip_fl,
                )?;
                if num_nz > 16 {
                    return Err(error("JPEG-XR highpass nonzero coefficient overflow"));
                }
                lap_mean[usize::from(is_chroma)] += num_nz as i32;
                index += 1;
                cbp_cy >>= 1;
            }

            if blk == 3 {
                model_bits = ctx.model_ac.flc_bits[1];
                is_chroma = true;
            }
        }
        if i + 1 < full_planes {
            cbp_cy = mb.cbp[i + 1] as u32;
        }
    }

    ctx.model_ac.update_mb(cf, channels, lap_mean);
    if reset_context {
        ctx.adapt_highpass();
    }
    Ok(())
}
