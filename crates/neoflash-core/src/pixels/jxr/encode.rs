use super::bitstream::*;
use super::color_transform::*;
use super::entropy_highpass::*;
use super::entropy_lowpass::*;
use super::pct_forward::*;
use super::prediction::*;
use super::tables::*;
use super::*;

// ============================================================================
// Codestream Header Writing & Spatial Single-Tile Encoder Pipeline
// ============================================================================

pub(super) struct PlaneEncState {
    pub(super) cf: ColorFormat,
    pub(super) channels: usize,
    pub(super) scaled_arith: bool,
    pub(super) overlap: Overlap,
    pub(super) tile: TileQuantizers,
    pub(super) ctx: CodingContext,
    pub(super) mb: MbInfo,
    pub(super) a0: [Vec<i32>; MAX_CHANNELS],
    pub(super) a1: [Vec<i32>; MAX_CHANNELS],
    pub(super) res_u: Vec<i32>,
    pub(super) res_v: Vec<i32>,
    pub(super) pred_cur: [Vec<PredInfo>; MAX_CHANNELS],
    pub(super) pred_prev: [Vec<PredInfo>; MAX_CHANNELS],
    pub(super) pred_before: [[i32; 2]; 2],
    pub(super) pred_after: [[i32; 2]; 2],
}

impl PlaneEncState {
    pub(super) fn new(
        cf: ColorFormat,
        cmb_width: usize,
        quantization: u8,
        trim_flex_bits: u8,
        overlap: Overlap,
        force_scaled_arith: bool,
    ) -> (
        Self,
        [u8; MAX_CHANNELS],
        [u8; MAX_CHANNELS],
        [u8; MAX_CHANNELS],
    ) {
        let channels = cf.num_channels();
        let uv_res_change = matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422);
        let qps = map_flash_quantization_to_qps(quantization, cf);
        let scaled_arith = force_scaled_arith || quantization > 0 || uv_res_change;

        let (dc_idx, lp_idx, hp_idx) = if cf == ColorFormat::YOnly {
            ([qps[0], 0, 0, 0], [qps[0], 0, 0, 0], [qps[3], 0, 0, 0])
        } else {
            (
                [qps[0], qps[1], qps[2], 0],
                [qps[0], qps[1], qps[2], 0],
                [qps[3], qps[4], qps[5], 0],
            )
        };
        let ch_mode = if channels > 1 { 2 } else { 0 };
        let tile = TileQuantizers {
            dc: format_quantizer_row(&dc_idx, ch_mode, channels, true, scaled_arith, true),
            lp: vec![format_quantizer_row(
                &lp_idx,
                ch_mode,
                channels,
                true,
                scaled_arith,
                false,
            )],
            hp: vec![format_quantizer_row(
                &hp_idx,
                ch_mode,
                channels,
                false,
                scaled_arith,
                false,
            )],
            num_qp_lp: 1,
            num_qp_hp: 1,
            bits_lp: 0,
            bits_hp: 0,
        };

        let ch_stride = |ch: usize| -> usize {
            if ch == 0 {
                256
            } else {
                cf.chroma_blocks() * 16
            }
        };
        let a0 = std::array::from_fn(|ch| {
            if ch < channels {
                vec![0i32; cmb_width * ch_stride(ch)]
            } else {
                Vec::new()
            }
        });
        let a1 = std::array::from_fn(|ch| {
            if ch < channels {
                vec![0i32; cmb_width * ch_stride(ch)]
            } else {
                Vec::new()
            }
        });
        let res_len = if uv_res_change {
            (256 + if cf == ColorFormat::Yuv420 { 32 } else { 0 }) * cmb_width + 256
        } else {
            0
        };
        let pred_cur = std::array::from_fn(|ch| {
            if ch < channels {
                vec![PredInfo::default(); cmb_width]
            } else {
                Vec::new()
            }
        });
        let pred_prev = std::array::from_fn(|ch| {
            if ch < channels {
                vec![PredInfo::default(); cmb_width]
            } else {
                Vec::new()
            }
        });

        (
            Self {
                cf,
                channels,
                scaled_arith,
                overlap,
                tile,
                ctx: CodingContext::new(cf, trim_flex_bits),
                mb: MbInfo::default(),
                a0,
                a1,
                res_u: vec![0i32; res_len],
                res_v: vec![0i32; res_len],
                pred_cur,
                pred_prev,
                pred_before: [[0; 2]; 2],
                pred_after: [[0; 2]; 2],
            },
            dc_idx,
            lp_idx,
            hp_idx,
        )
    }

    pub(super) fn advance_one_mb_row(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.pred_cur[ch], &mut self.pred_prev[ch]);
        }
    }

    pub(super) fn swap_mr_buffers(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.a0[ch], &mut self.a1[ch]);
        }
    }

    pub(super) fn transform_and_encode_at(
        &mut self,
        cmb_width: usize,
        cmb_height: usize,
        c_row: usize,
        c_column: usize,
        bw: &mut BitWriter,
    ) {
        transform_macroblock_enc(
            self.cf,
            self.channels,
            self.overlap,
            self.scaled_arith,
            cmb_width,
            cmb_height,
            c_row,
            c_column,
            &mut self.a0,
            &mut self.a1,
            &mut self.pred_before,
            &mut self.pred_after,
        );

        if c_column > 0 && c_row > 0 {
            let mb_x = c_column - 1;
            let ctx_left = mb_x == 0;
            let ctx_top = c_row - 1 == 0;
            let reset_rgi = (mb_x & 0xF) == 0;
            let reset_ctx = reset_rgi || (mb_x + 1 == cmb_width);

            self.mb.q_index_lp = 0;
            self.mb.q_index_hp = 0;

            let mut dummy1 = [];
            let mut dummy2 = [];
            let mut dummy3 = [];
            let [s0, s1, s2, s3] = &mut self.a0;
            let y_off = mb_x * 256;
            let uv_len = self.cf.chroma_blocks() * 16;
            let uv_off = mb_x * uv_len;
            let p0_slice: &mut [i32] = &mut s0[y_off..y_off + 256];
            let p1_slice: &mut [i32] = if self.channels > 1 {
                &mut s1[uv_off..uv_off + uv_len]
            } else {
                &mut dummy1
            };
            let p2_slice: &mut [i32] = if self.channels > 2 {
                &mut s2[uv_off..uv_off + uv_len]
            } else {
                &mut dummy2
            };
            let p3_slice: &mut [i32] = if self.channels > 3 {
                &mut s3[uv_off..uv_off + uv_len]
            } else {
                &mut dummy3
            };
            let mut planes: [&mut [i32]; MAX_CHANNELS] = [p0_slice, p1_slice, p2_slice, p3_slice];

            quantize_macroblock(
                self.cf,
                self.channels,
                Subband::All,
                &self.tile,
                &mut self.mb,
                &mut planes,
            );
            pred_macroblock_enc(
                self.cf,
                self.channels,
                ctx_left,
                ctx_top,
                mb_x,
                &mut self.mb,
                &mut planes,
                &mut self.pred_cur,
                &self.pred_prev,
            );
            encode_macroblock_dc(
                self.cf,
                self.channels,
                Subband::All,
                reset_ctx,
                &self.mb,
                &mut self.ctx,
                bw,
            );
            encode_macroblock_lowpass(
                self.cf,
                self.channels,
                reset_rgi,
                reset_ctx,
                &self.mb,
                &mut self.ctx,
                bw,
            );
            encode_macroblock_highpass(
                self.cf,
                self.channels,
                Subband::All,
                ctx_left,
                ctx_top,
                reset_rgi,
                reset_ctx,
                mb_x,
                &mut self.mb,
                &planes,
                &mut self.pred_cur,
                &self.pred_prev,
                &mut self.ctx,
                bw,
            );
        }
    }
}

pub(super) fn write_quantizer_header(
    bw: &mut BitWriter,
    indices: &[u8; MAX_CHANNELS],
    channels: usize,
) {
    if channels > 1 {
        bw.put_bits(2, 2); // INDEPENDENT channel mode
        for &qp in &indices[..channels] {
            bw.put_bits(u32::from(qp), 8);
        }
    } else {
        bw.put_bits(u32::from(indices[0]), 8);
    }
}

pub(super) fn write_image_plane_header(
    bw: &mut BitWriter,
    cf: ColorFormat,
    scaled_arith: bool,
    dc_idx: &[u8; MAX_CHANNELS],
    lp_idx: &[u8; MAX_CHANNELS],
    hp_idx: &[u8; MAX_CHANNELS],
) {
    let channels = cf.num_channels();
    bw.put_bits(cf as u32, 3); // INTERNAL_CLR_FMT
    bw.put_bit(scaled_arith); // SCALED_FLAG
    bw.put_bits(Subband::All as u32, 4); // BANDS_PRESENT = ALL (0)

    if cf != ColorFormat::YOnly {
        bw.put_bits(0, 4); // CHROMA_CENTERING_X = 0
        bw.put_bits(0, 4); // CHROMA_CENTERING_Y = 0
    }

    // DC frame-uniform QP
    bw.put_bit(true);
    write_quantizer_header(bw, dc_idx, channels);

    // LP frame-uniform QP (independent of DC)
    bw.put_bit(false); // USE_DC_QUANTIZER = 0
    bw.put_bit(true); // LP_FRAME_UNIFORM = 1
    write_quantizer_header(bw, lp_idx, channels);

    // HP frame-uniform QP (independent of LP)
    bw.put_bit(false); // USE_LP_QUANTIZER = 0
    bw.put_bit(true); // HP_FRAME_UNIFORM = 1
    write_quantizer_header(bw, hp_idx, channels);

    bw.fill_to_byte();
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_wmi_image_header(
    bw: &mut BitWriter,
    width: u32,
    height: u32,
    overlap: Overlap,
    trim_flex_bits: u8,
    interleaved_alpha: bool,
    output_clr_fmt: u8,
) {
    let cmb_width = width.div_ceil(16);
    let cmb_height = height.div_ceil(16);
    let short_header = cmb_width <= 255 && cmb_height <= 255;
    let windowing = (width & 0xF) != 0 || (height & 0xF) != 0;

    bw.write_bytes_aligned(&WMPHOTO_SIGNATURE);
    bw.put_bits(u32::from(CODEC_VERSION), 4);
    bw.put_bits(u32::from(CODEC_SUBVERSION_NEWSCALING_SOFT_TILES), 4);

    // Primary flags byte 9
    bw.put_bit(false); // TILING_FLAG = 0
    bw.put_bit(false); // BITSTREAMFORMAT = SPATIAL (0)
    bw.put_bits(0, 3); // ORIENTATION = O_NONE (0)
    bw.put_bit(false); // INDEX_TABLE_PRESENT_FLAG = 0 (single-tile streaming spatial)
    bw.put_bits(overlap as u32, 2); // OVERLAP_MODE

    // Secondary flags byte 10
    bw.put_bit(short_header); // SHORT_HEADER_FLAG
    bw.put_bit(true); // LONG_WORD_FLAG = 1
    bw.put_bit(windowing); // WINDOWING_FLAG
    bw.put_bit(trim_flex_bits > 0); // TRIM_FLEXBITS_FLAG
    bw.put_bit(false); // TILE_STRETCH_FLAG = 0
    bw.put_bits(0, 2); // RESERVED
    bw.put_bit(interleaved_alpha); // ALPHA_IMAGE_PLANE_FLAG

    // Source format byte 11
    bw.put_bits(u32::from(output_clr_fmt), 4);
    bw.put_bits(u32::from(BD_8), 4);

    // WIDTH_MINUS1 / HEIGHT_MINUS1
    let size_bits = if short_header { 16 } else { 32 };
    bw.put_bits(width - 1, size_bits);
    bw.put_bits(height - 1, size_bits);

    if windowing {
        let extra_bottom = (16 - (height & 0xF)) & 0xF;
        let extra_right = (16 - (width & 0xF)) & 0xF;
        bw.put_bits(0, 6); // TOP_MARGIN = 0
        bw.put_bits(0, 6); // LEFT_MARGIN = 0
        bw.put_bits(extra_bottom, 6); // BOTTOM_MARGIN
        bw.put_bits(extra_right, 6); // RIGHT_MARGIN
    }

    bw.fill_to_byte();
}

pub(super) fn write_null_index_table_and_spatial_packet_header(
    bw: &mut BitWriter,
    trim_flex_bits: u8,
) {
    bw.fill_to_byte();
    // PROFILE_LEVEL_INFO (VLW_ESC 4 bytes: profile_idc = 111, level_idc = 255, LAST_FLAG = 1)
    bw.write_bytes_aligned(&[0x00, 0x04, 111, 255, 0x00, 0x01]);
    // TILE_SPATIAL packet startcode: 0x00000100 (tile 0, packet type 0 = spatial)
    bw.write_bytes_aligned(&[0x00, 0x00, 0x01, 0x00]);
    if trim_flex_bits > 0 {
        bw.put_bits(u32::from(trim_flex_bits), 4);
    }
}

pub(super) fn input_mb_row_rgb_and_alpha(
    width: usize,
    height: usize,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    argb_be: &[u8],
    primary: &mut PlaneEncState,
    alpha: Option<&mut PlaneEncState>,
) {
    let shift = if primary.scaled_arith {
        SHIFTZERO + QPFRACBITS
    } else {
        0
    };
    let offset = 128i32 << shift;
    let uv_res_change = matches!(primary.cf, ColorFormat::Yuv420 | ColorFormat::Yuv422);

    for i_row in 0..16usize {
        let src_y = (c_row * 16 + i_row).min(height - 1);
        let row_base = src_y * width * 4;
        for col in 0..width {
            let px = &argb_be[row_base + col * 4..row_base + col * 4 + 4];
            let mut r = i32::from(px[1]) << shift;
            let mut g = i32::from(px[2]) << shift;
            let mut b = i32::from(px[3]) << shift;

            // Forward reversible YCoCg (_CC in strenc.c)
            b -= r;
            r += ((b + 1) >> 1) - g;
            g += r >> 1;

            let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
            primary.a1[0][pos] = g - offset;
            if uv_res_change {
                primary.res_u[pos] = -r;
                primary.res_v[pos] = b;
            } else {
                primary.a1[1][pos] = -r;
                primary.a1[2][pos] = b;
            }
        }

        // Horizontal edge replication to 16-pixel macroblock width
        let last_col = width - 1;
        let last_pos = ((last_col >> 4) << 8) + IDX_CC[i_row][last_col & 0xF];
        for col in width..(cmb_width * 16) {
            let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
            primary.a1[0][pos] = primary.a1[0][last_pos];
            if uv_res_change {
                primary.res_u[pos] = primary.res_u[last_pos];
                primary.res_v[pos] = primary.res_v[last_pos];
            } else {
                primary.a1[1][pos] = primary.a1[1][last_pos];
                primary.a1[2][pos] = primary.a1[2][last_pos];
            }
        }
    }

    if uv_res_change {
        downsample_uv_row(
            primary.cf,
            cmb_width,
            cmb_height,
            c_row,
            &mut primary.res_u,
            &mut primary.res_v,
            &mut primary.a0,
            &mut primary.a1,
        );
    }

    if let Some(a_state) = alpha {
        let a_shift = if a_state.scaled_arith {
            SHIFTZERO + QPFRACBITS
        } else {
            0
        };
        for i_row in 0..16usize {
            let src_y = (c_row * 16 + i_row).min(height - 1);
            let row_base = src_y * width * 4;
            for col in 0..width {
                let a = i32::from(argb_be[row_base + col * 4]);
                let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
                a_state.a1[0][pos] = (a - 128) << a_shift;
            }
            let last_col = width - 1;
            let last_pos = ((last_col >> 4) << 8) + IDX_CC[i_row][last_col & 0xF];
            for col in width..(cmb_width * 16) {
                let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
                a_state.a1[0][pos] = a_state.a1[0][last_pos];
            }
        }
    }
}

pub(super) fn encode_primary_codestream(
    width: u32,
    height: u32,
    argb_be: &[u8],
    subsampling: ChromaSubsampling,
    quantization: u8,
    trim_flex_bits: u8,
    interleaved_alpha: bool,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let cmb_width = w.div_ceil(16);
    let cmb_height = h.div_ceil(16);
    let cf = subsampling.color_format();
    let overlap = if quantization > 50 && cmb_width >= 2 {
        Overlap::Two
    } else {
        Overlap::One
    };

    let (mut primary, dc_idx, lp_idx, hp_idx) =
        PlaneEncState::new(cf, cmb_width, quantization, trim_flex_bits, overlap, false);
    let mut alpha_opt = if interleaved_alpha {
        Some(PlaneEncState::new(
            ColorFormat::YOnly,
            cmb_width,
            quantization,
            trim_flex_bits,
            overlap,
            primary.scaled_arith,
        ))
    } else {
        None
    };

    let mut bw = BitWriter::with_capacity(w * h * 3 + 256);
    write_wmi_image_header(
        &mut bw,
        width,
        height,
        overlap,
        trim_flex_bits,
        interleaved_alpha,
        OUT_CLR_RGB,
    );
    write_image_plane_header(&mut bw, cf, primary.scaled_arith, &dc_idx, &lp_idx, &hp_idx);
    if let Some((ref a_state, ref a_dc, ref a_lp, ref a_hp)) = alpha_opt {
        write_image_plane_header(
            &mut bw,
            ColorFormat::YOnly,
            a_state.scaled_arith,
            a_dc,
            a_lp,
            a_hp,
        );
    }
    write_null_index_table_and_spatial_packet_header(&mut bw, trim_flex_bits);

    for c_row in 0..cmb_height {
        input_mb_row_rgb_and_alpha(
            w,
            h,
            cmb_width,
            cmb_height,
            c_row,
            argb_be,
            &mut primary,
            alpha_opt.as_mut().map(|(s, ..)| s),
        );
        for c_col in 0..=cmb_width {
            primary.transform_and_encode_at(cmb_width, cmb_height, c_row, c_col, &mut bw);
            if let Some((ref mut a_state, ..)) = alpha_opt {
                a_state.transform_and_encode_at(cmb_width, cmb_height, c_row, c_col, &mut bw);
            }
        }
        if c_row > 0 {
            primary.advance_one_mb_row();
            if let Some((ref mut a_state, ..)) = alpha_opt {
                a_state.advance_one_mb_row();
            }
        }
        primary.swap_mr_buffers();
        if let Some((ref mut a_state, ..)) = alpha_opt {
            a_state.swap_mr_buffers();
        }
    }

    // Bottom flush row (c_row == cmb_height)
    for c_col in 0..=cmb_width {
        primary.transform_and_encode_at(cmb_width, cmb_height, cmb_height, c_col, &mut bw);
        if let Some((ref mut a_state, ..)) = alpha_opt {
            a_state.transform_and_encode_at(cmb_width, cmb_height, cmb_height, c_col, &mut bw);
        }
    }

    bw.finish()
}

pub(super) fn encode_planar_alpha_codestream(
    width: u32,
    height: u32,
    argb_be: &[u8],
    quantization: u8,
    trim_flex_bits: u8,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let cmb_width = w.div_ceil(16);
    let cmb_height = h.div_ceil(16);
    let overlap = if quantization > 50 && cmb_width >= 2 {
        Overlap::Two
    } else {
        Overlap::One
    };

    let (mut alpha, dc_idx, lp_idx, hp_idx) = PlaneEncState::new(
        ColorFormat::YOnly,
        cmb_width,
        quantization,
        trim_flex_bits,
        overlap,
        false,
    );

    let mut bw = BitWriter::with_capacity(w * h + 256);
    write_wmi_image_header(
        &mut bw,
        width,
        height,
        overlap,
        trim_flex_bits,
        false,
        OUT_CLR_YONLY,
    );
    write_image_plane_header(
        &mut bw,
        ColorFormat::YOnly,
        alpha.scaled_arith,
        &dc_idx,
        &lp_idx,
        &hp_idx,
    );
    write_null_index_table_and_spatial_packet_header(&mut bw, trim_flex_bits);

    let shift = if alpha.scaled_arith {
        SHIFTZERO + QPFRACBITS
    } else {
        0
    };
    let offset = 128i32 << shift;

    for c_row in 0..cmb_height {
        for i_row in 0..16usize {
            let src_y = (c_row * 16 + i_row).min(h - 1);
            let row_base = src_y * w * 4;
            for col in 0..w {
                let a = i32::from(argb_be[row_base + col * 4]);
                let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
                alpha.a1[0][pos] = (a << shift) - offset;
            }
            let last_col = w - 1;
            let last_pos = ((last_col >> 4) << 8) + IDX_CC[i_row][last_col & 0xF];
            for col in w..(cmb_width * 16) {
                let pos = ((col >> 4) << 8) + IDX_CC[i_row][col & 0xF];
                alpha.a1[0][pos] = alpha.a1[0][last_pos];
            }
        }
        for c_col in 0..=cmb_width {
            alpha.transform_and_encode_at(cmb_width, cmb_height, c_row, c_col, &mut bw);
        }
        if c_row > 0 {
            alpha.advance_one_mb_row();
        }
        alpha.swap_mr_buffers();
    }

    for c_col in 0..=cmb_width {
        alpha.transform_and_encode_at(cmb_width, cmb_height, cmb_height, c_col, &mut bw);
    }

    bw.finish()
}
