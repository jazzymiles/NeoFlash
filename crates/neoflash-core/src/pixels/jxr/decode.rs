use super::bitstream::*;
use super::color_transform::*;
use super::entropy_highpass::*;
use super::entropy_lowpass::*;
use super::pct_inverse::*;
use super::prediction::*;
use super::tables::*;
use super::*;

// ============================================================================
// ITU-T Rec. T.832 Codestream Header Parsing & Spatial+Frequency Decoder
// ============================================================================

#[derive(Clone, Debug)]
pub(super) struct ParsedPlaneHeader {
    pub(super) cf: ColorFormat,
    pub(super) scaled_arith: bool,
    pub(super) subband: Subband,
    pub(super) qp_mode: u32,
    pub(super) qp_dc: [u8; MAX_CHANNELS],
    pub(super) qp_lp: [u8; MAX_CHANNELS],
    pub(super) qp_hp: [u8; MAX_CHANNELS],
}

#[derive(Clone, Debug)]
pub(super) struct ParsedImageHeader {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) extra_top: usize,
    pub(super) extra_left: usize,
    pub(super) extra_bottom: usize,
    pub(super) extra_right: usize,
    pub(super) hard_tiles: bool,
    pub(super) bitstream_fmt: BitstreamFormat,
    pub(super) orientation: u8,
    pub(super) index_table_present: bool,
    pub(super) overlap: Overlap,
    pub(super) trim_flexbits_flag: bool,
    pub(super) alpha_channel: bool,
    pub(super) output_clr_fmt: u8,
    pub(super) tile_x: Vec<usize>,
    pub(super) tile_y: Vec<usize>,
}

pub(super) fn read_quantizer_header(
    br: &mut BitReader<'_>,
    channels: usize,
) -> Result<(u8, [u8; MAX_CHANNELS]), BitmapError> {
    let mut qps = [0u8; MAX_CHANNELS];
    let ch_mode = if channels > 1 {
        br.get_bits(2)? as u8
    } else {
        0
    };
    qps[0] = br.get_bits(8)? as u8;
    if ch_mode == 1 {
        qps[1] = br.get_bits(8)? as u8;
    } else if ch_mode > 0 {
        for qp in &mut qps[1..channels] {
            *qp = br.get_bits(8)? as u8;
        }
    }
    Ok((ch_mode, qps))
}

pub(super) fn read_image_plane_header(
    br: &mut BitReader<'_>,
) -> Result<ParsedPlaneHeader, BitmapError> {
    let cf = ColorFormat::from_internal_code(br.get_bits(3)? as u8)?;
    let scaled_arith = br.get_bool()?;
    let subband = Subband::from_code(br.get_bits(4)? as u8)?;
    let channels = cf.num_channels();

    if cf != ColorFormat::YOnly {
        let _ = br.get_bits(8)?; // chroma centering X/Y
    }

    let mut qp_mode = 0u32;
    let mut qp_dc = [0u8; MAX_CHANNELS];
    let mut qp_lp = [0u8; MAX_CHANNELS];
    let mut qp_hp = [0u8; MAX_CHANNELS];

    if br.get_bool()? {
        let (mode, q) = read_quantizer_header(br, channels)?;
        qp_dc = q;
        qp_mode += u32::from(mode) << 3;
    } else {
        qp_mode += 1;
    }

    if subband != Subband::DcOnly {
        if !br.get_bool()? {
            qp_mode += 0x200;
            if br.get_bool()? {
                let (mode, q) = read_quantizer_header(br, channels)?;
                qp_lp = q;
                qp_mode += u32::from(mode) << 5;
            } else {
                qp_mode += 2;
            }
        } else {
            qp_lp = qp_dc;
            qp_mode += ((qp_mode & 1) << 1) + ((qp_mode & 0x18) << 2);
        }

        if subband != Subband::NoHighpass {
            if !br.get_bool()? {
                qp_mode += 0x400;
                if br.get_bool()? {
                    let (mode, q) = read_quantizer_header(br, channels)?;
                    qp_hp = q;
                    qp_mode += u32::from(mode) << 7;
                } else {
                    qp_mode += 4;
                }
            } else {
                qp_hp = qp_lp;
                qp_mode += ((qp_mode & 2) << 1) + ((qp_mode & 0x60) << 2);
            }
        }
    }

    br.flush_to_byte()?;
    Ok(ParsedPlaneHeader {
        cf,
        scaled_arith,
        subband,
        qp_mode,
        qp_dc,
        qp_lp,
        qp_hp,
    })
}

pub(super) fn read_wmi_image_header(
    br: &mut BitReader<'_>,
) -> Result<ParsedImageHeader, BitmapError> {
    for &expected in &WMPHOTO_SIGNATURE {
        if br.get_bits(8)? as u8 != expected {
            return Err(error(
                "missing WMPHOTO\\0 signature in JPEG-XR IMAGE_HEADER",
            ));
        }
    }
    let version = br.get_bits(4)? as u8;
    if version != CODEC_VERSION {
        return Err(error("unsupported JPEG-XR codec version"));
    }
    let subversion = br.get_bits(4)? as u8;
    if subversion != CODEC_SUBVERSION_OLD
        && subversion != CODEC_SUBVERSION_NEWSCALING_SOFT_TILES
        && subversion != CODEC_SUBVERSION_NEWSCALING_HARD_TILES
    {
        return Err(error("unsupported JPEG-XR codec subversion"));
    }
    let hard_tiles = subversion == CODEC_SUBVERSION_NEWSCALING_HARD_TILES;

    let tiling_present = br.get_bool()?;
    let bitstream_fmt = if br.get_bool()? {
        BitstreamFormat::Frequency
    } else {
        BitstreamFormat::Spatial
    };
    let orientation = br.get_bits(3)? as u8;
    let index_table_present = br.get_bool()?;
    let overlap = Overlap::from_code(br.get_bits(2)? as u8)?;

    let short_header = br.get_bool()?;
    let _long_word = br.get_bool()?;
    let windowing = br.get_bool()?;
    let trim_flexbits_flag = br.get_bool()?;
    let tile_stretch = br.get_bool()?;
    let _rb_swapped = br.get_bool()?;
    let _reserved = br.get_bool()?;
    let alpha_channel = br.get_bool()?;

    let output_clr_fmt = br.get_bits(4)? as u8;
    let src_bd = br.get_bits(4)? as u8;
    if src_bd != BD_8 {
        return Err(error(
            "only 8-bit-per-channel JPEG-XR bitstreams are supported",
        ));
    }
    if output_clr_fmt != OUT_CLR_RGB && output_clr_fmt != OUT_CLR_YONLY {
        return Err(error("unsupported JPEG-XR output color format"));
    }

    let size_bits = if short_header { 16 } else { 32 };
    let mut width = br.get_bits(size_bits)?.saturating_add(1);
    let mut height = br.get_bits(size_bits)?.saturating_add(1);

    let (num_slices_v, num_slices_h) = if tiling_present {
        (
            (br.get_bits(12)? as usize) + 1,
            (br.get_bits(12)? as usize) + 1,
        )
    } else {
        (1usize, 1usize)
    };
    if !index_table_present
        && (bitstream_fmt == BitstreamFormat::Frequency || num_slices_v > 1 || num_slices_h > 1)
    {
        return Err(error(
            "JPEG-XR tiled or frequency-mode stream requires an index table",
        ));
    }
    if num_slices_v > 256 || num_slices_h > 256 {
        return Err(error("JPEG-XR tile count exceeds baseline limit"));
    }

    let tile_bits = if short_header { 8 } else { 16 };
    let mut tile_x = Vec::with_capacity(num_slices_v);
    tile_x.push(0usize);
    for i in 0..(num_slices_v - 1) {
        let step = br.get_bits(tile_bits)? as usize;
        tile_x.push(tile_x[i] + step);
    }
    let mut tile_y = Vec::with_capacity(num_slices_h);
    tile_y.push(0usize);
    for i in 0..(num_slices_h - 1) {
        let step = br.get_bits(tile_bits)? as usize;
        tile_y.push(tile_y[i] + step);
    }
    if tile_stretch {
        for _ in 0..(num_slices_v * num_slices_h) {
            let _ = br.get_bits(8)?;
        }
    }

    let (mut extra_top, mut extra_left, mut extra_bottom, mut extra_right) = (0usize, 0, 0, 0);
    if windowing {
        extra_top = br.get_bits(6)? as usize;
        extra_left = br.get_bits(6)? as usize;
        extra_bottom = br.get_bits(6)? as usize;
        extra_right = br.get_bits(6)? as usize;
    } else {
        if (width & 0xF) != 0 {
            extra_right = 16 - ((width as usize) & 0xF);
        }
        if (height & 0xF) != 0 {
            extra_bottom = 16 - ((height as usize) & 0xF);
        }
    }

    if (((width as usize + extra_left + extra_right) & 0xF)
        + ((height as usize + extra_top + extra_bottom) & 0xF))
        != 0
    {
        if ((width as usize & 0xF) + (height as usize & 0xF) + extra_left + extra_top) != 0
            || (width as usize) <= extra_right
            || (height as usize) <= extra_bottom
        {
            return Err(error("invalid JPEG-XR windowing margins"));
        }
        width -= extra_right as u32;
        height -= extra_bottom as u32;
    }

    if width == 0 || height == 0 || width > MAX_BITMAP_DIMENSION || height > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR image dimensions must be in 1..=4096"));
    }

    br.flush_to_byte()?;
    Ok(ParsedImageHeader {
        width,
        height,
        extra_top,
        extra_left,
        extra_bottom,
        extra_right,
        hard_tiles,
        bitstream_fmt,
        orientation,
        index_table_present,
        overlap,
        trim_flexbits_flag,
        alpha_channel,
        output_clr_fmt,
        tile_x,
        tile_y,
    })
}

pub(super) enum TileBandReaders<'a> {
    Spatial(BitReader<'a>),
    Frequency {
        dc: BitReader<'a>,
        lp: Option<BitReader<'a>>,
        ac: Option<BitReader<'a>>,
        fl: Option<BitReader<'a>>,
    },
}

pub(super) struct PlaneDecState {
    pub(super) hdr: ParsedPlaneHeader,
    pub(super) channels: usize,
    pub(super) tiles: Vec<TileQuantizers>,
    pub(super) contexts: Vec<CodingContext>,
    pub(super) mb: MbInfo,
    pub(super) a0: [Vec<i32>; MAX_CHANNELS],
    pub(super) a1: [Vec<i32>; MAX_CHANNELS],
    pub(super) res_u: Vec<i32>,
    pub(super) res_v: Vec<i32>,
    pub(super) pred_cur: [Vec<PredInfo>; MAX_CHANNELS],
    pub(super) pred_prev: [Vec<PredInfo>; MAX_CHANNELS],
    pub(super) pred_before: [[i32; 2]; 2],
    pub(super) pred_after: [[i32; 2]; 2],
    pub(super) tile_col: usize,
    pub(super) tile_row: usize,
    pub(super) trfm_mb_y: usize,
    pub(super) trfm_tile_x: usize,
    pub(super) trfm_tile_y: usize,
    pub(super) b_vert_tb: bool,
    pub(super) b_hori_tb: bool,
    pub(super) b_one_left_vtb: bool,
    pub(super) b_one_right_vtb: bool,
}

impl PlaneDecState {
    pub(super) fn new(hdr: ParsedPlaneHeader, cmb_width: usize, num_tiles_v: usize) -> Self {
        let channels = hdr.cf.num_channels();
        let mut tiles = vec![TileQuantizers::default(); num_tiles_v];

        if (hdr.qp_mode & 1) == 0 {
            let dc_row = format_quantizer_row(
                &hdr.qp_dc,
                ((hdr.qp_mode >> 3) & 3) as u8,
                channels,
                true,
                hdr.scaled_arith,
                false,
            );
            for t in &mut tiles {
                t.dc = dc_row;
            }
        }
        if hdr.subband != Subband::DcOnly && (hdr.qp_mode & 2) == 0 {
            let lp_row = if (hdr.qp_mode & 0x200) == 0 {
                tiles[0].dc
            } else {
                format_quantizer_row(
                    &hdr.qp_lp,
                    ((hdr.qp_mode >> 5) & 3) as u8,
                    channels,
                    true,
                    hdr.scaled_arith,
                    false,
                )
            };
            for t in &mut tiles {
                t.lp = vec![lp_row];
                t.num_qp_lp = 1;
                t.bits_lp = 0;
            }
        }
        if hdr.subband != Subband::DcOnly
            && hdr.subband != Subband::NoHighpass
            && (hdr.qp_mode & 4) == 0
        {
            let hp_row = if (hdr.qp_mode & 0x400) == 0 {
                tiles[0].lp[0]
            } else {
                format_quantizer_row(
                    &hdr.qp_hp,
                    ((hdr.qp_mode >> 7) & 3) as u8,
                    channels,
                    false,
                    hdr.scaled_arith,
                    false,
                )
            };
            for t in &mut tiles {
                t.hp = vec![hp_row];
                t.num_qp_hp = 1;
                t.bits_hp = 0;
            }
        }

        let ch_stride = |ch: usize| -> usize {
            if ch == 0 {
                256
            } else {
                hdr.cf.chroma_blocks() * 16
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
        let uv_res_change = matches!(hdr.cf, ColorFormat::Yuv420 | ColorFormat::Yuv422);
        let res_len = if uv_res_change { 256 * cmb_width } else { 0 };
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

        Self {
            contexts: vec![CodingContext::new(hdr.cf, 0); num_tiles_v],
            hdr,
            channels,
            tiles,
            mb: MbInfo::default(),
            a0,
            a1,
            res_u: vec![0i32; res_len],
            res_v: vec![0i32; res_len],
            pred_cur,
            pred_prev,
            pred_before: [[0; 2]; 2],
            pred_after: [[0; 2]; 2],
            tile_col: 0,
            tile_row: 0,
            trfm_mb_y: 0,
            trfm_tile_x: 0,
            trfm_tile_y: 0,
            b_vert_tb: false,
            b_hori_tb: false,
            b_one_left_vtb: false,
            b_one_right_vtb: false,
        }
    }

    pub(super) fn zero_a1(&mut self) {
        for ch in 0..self.channels {
            self.a1[ch].fill(0);
        }
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

    pub(super) fn read_tile_header_dc(
        &mut self,
        tile_col: usize,
        br: &mut BitReader<'_>,
    ) -> Result<(), BitmapError> {
        if (self.hdr.qp_mode & 1) != 0 {
            let (mode, q) = read_quantizer_header(br, self.channels)?;
            self.tiles[tile_col].dc =
                format_quantizer_row(&q, mode, self.channels, true, self.hdr.scaled_arith, false);
        }
        Ok(())
    }

    pub(super) fn read_tile_header_lp(
        &mut self,
        tile_col: usize,
        br: &mut BitReader<'_>,
    ) -> Result<(), BitmapError> {
        if self.hdr.subband != Subband::DcOnly && (self.hdr.qp_mode & 2) != 0 {
            let use_dc = br.get_bool()?;
            if use_dc {
                let dc = self.tiles[tile_col].dc;
                self.tiles[tile_col].lp = vec![dc];
                self.tiles[tile_col].num_qp_lp = 1;
                self.tiles[tile_col].bits_lp = 0;
            } else {
                let num_qp = (br.get_bits(4)? as u8) + 1;
                let bits = dquant_bits(num_qp);
                let mut rows = Vec::with_capacity(usize::from(num_qp));
                for _ in 0..num_qp {
                    let (mode, q) = read_quantizer_header(br, self.channels)?;
                    rows.push(format_quantizer_row(
                        &q,
                        mode,
                        self.channels,
                        true,
                        self.hdr.scaled_arith,
                        false,
                    ));
                }
                self.tiles[tile_col].lp = rows;
                self.tiles[tile_col].num_qp_lp = num_qp;
                self.tiles[tile_col].bits_lp = bits;
            }
        }
        Ok(())
    }

    pub(super) fn read_tile_header_hp(
        &mut self,
        tile_col: usize,
        br: &mut BitReader<'_>,
    ) -> Result<(), BitmapError> {
        if self.hdr.subband != Subband::DcOnly
            && self.hdr.subband != Subband::NoHighpass
            && (self.hdr.qp_mode & 4) != 0
        {
            let use_lp = br.get_bool()?;
            if use_lp {
                let lp = self.tiles[tile_col].lp.clone();
                let num_qp = self.tiles[tile_col].num_qp_lp;
                self.tiles[tile_col].hp = lp;
                self.tiles[tile_col].num_qp_hp = num_qp;
                self.tiles[tile_col].bits_hp = 0;
            } else {
                let num_qp = (br.get_bits(4)? as u8) + 1;
                let bits = dquant_bits(num_qp);
                let mut rows = Vec::with_capacity(usize::from(num_qp));
                for _ in 0..num_qp {
                    let (mode, q) = read_quantizer_header(br, self.channels)?;
                    rows.push(format_quantizer_row(
                        &q,
                        mode,
                        self.channels,
                        false,
                        self.hdr.scaled_arith,
                        false,
                    ));
                }
                self.tiles[tile_col].hp = rows;
                self.tiles[tile_col].num_qp_hp = num_qp;
                self.tiles[tile_col].bits_hp = bits;
            }
        }
        Ok(())
    }

    pub(super) fn update_hard_tile_flags(
        &mut self,
        img: &ParsedImageHeader,
        c_row: usize,
        c_col: usize,
    ) {
        if img.hard_tiles {
            if c_col == 0 {
                self.b_vert_tb = false;
                self.trfm_tile_y = 0;
            }
            self.b_one_left_vtb = false;
            self.b_one_right_vtb = false;
            // Note: jxrlib strInvTransform.c lines 1208-1228 check uiTileY on cColumn and uiTileX on cRow
            let num_slice_minus1_h = img.tile_y.len() - 1;
            let num_slice_minus1_v = img.tile_x.len() - 1;
            if self.trfm_tile_y > 0
                && self.trfm_tile_y <= num_slice_minus1_h
                && c_col.wrapping_sub(1) == img.tile_y[self.trfm_tile_y]
            {
                self.b_one_right_vtb = true;
            }
            if self.trfm_tile_y < num_slice_minus1_h && c_col == img.tile_y[self.trfm_tile_y + 1] {
                self.b_vert_tb = true;
                self.trfm_tile_y += 1;
            } else {
                self.b_vert_tb = false;
            }
            if self.trfm_tile_y < num_slice_minus1_h
                && c_col + 1 == img.tile_y[self.trfm_tile_y + 1]
            {
                self.b_one_left_vtb = true;
            }

            if c_row == 0 {
                self.b_hori_tb = false;
                self.trfm_tile_x = 0;
            } else if self.trfm_mb_y != c_row
                && self.trfm_tile_x < num_slice_minus1_v
                && c_row == img.tile_x[self.trfm_tile_x + 1]
            {
                self.b_hori_tb = true;
                self.trfm_tile_x += 1;
            } else if self.trfm_mb_y != c_row {
                self.b_hori_tb = false;
            }
        } else {
            self.b_vert_tb = false;
            self.b_hori_tb = false;
            self.b_one_left_vtb = false;
            self.b_one_right_vtb = false;
        }
        self.trfm_mb_y = c_row;
    }

    pub(super) fn decode_mb_entropy(
        &mut self,
        img: &ParsedImageHeader,
        cmb_width: usize,
        c_col: usize,
        ctx_left: bool,
        ctx_top: bool,
        reset_rgi: bool,
        reset_ctx: bool,
        readers: &mut TileBandReaders<'_>,
    ) -> Result<(), BitmapError> {
        let tile = &self.tiles[self.tile_col];
        let ctx = &mut self.contexts[self.tile_col];

        let mut dummy1 = [];
        let mut dummy2 = [];
        let mut dummy3 = [];
        let [s0, s1, s2, s3] = &mut self.a1;
        let y_off = c_col * 256;
        let uv_len = self.hdr.cf.chroma_blocks() * 16;
        let uv_off = c_col * uv_len;
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

        match readers {
            TileBandReaders::Spatial(br) => {
                decode_macroblock_dc(
                    self.hdr.cf,
                    self.channels,
                    img.bitstream_fmt,
                    self.hdr.subband,
                    tile,
                    reset_ctx,
                    &mut self.mb,
                    ctx,
                    br,
                )?;
                if self.hdr.subband != Subband::DcOnly {
                    decode_macroblock_lowpass(
                        self.hdr.cf,
                        self.channels,
                        img.bitstream_fmt,
                        tile,
                        reset_rgi,
                        reset_ctx,
                        &mut self.mb,
                        ctx,
                        br,
                    )?;
                }
                pred_dcac_dec(
                    self.hdr.cf,
                    self.channels,
                    ctx_left,
                    ctx_top,
                    c_col,
                    &mut self.mb,
                    &self.pred_cur,
                    &self.pred_prev,
                );
                dequantize_macroblock(
                    self.hdr.cf,
                    self.channels,
                    self.hdr.subband,
                    tile,
                    &self.mb,
                    &mut planes,
                );
                if self.hdr.subband == Subband::All || self.hdr.subband == Subband::NoFlexbits {
                    decode_macroblock_highpass(
                        self.hdr.cf,
                        self.channels,
                        img.bitstream_fmt,
                        self.hdr.subband,
                        false,
                        tile,
                        ctx_left,
                        ctx_top,
                        reset_rgi,
                        reset_ctx,
                        c_col,
                        &mut self.mb,
                        &mut planes,
                        &mut self.pred_cur,
                        &self.pred_prev,
                        ctx,
                        br,
                        None,
                    )?;
                    pred_ac_dec(self.hdr.cf, self.channels, &self.mb, &mut planes);
                }
            }
            TileBandReaders::Frequency { dc, lp, ac, fl } => {
                decode_macroblock_dc(
                    self.hdr.cf,
                    self.channels,
                    img.bitstream_fmt,
                    self.hdr.subband,
                    tile,
                    reset_ctx,
                    &mut self.mb,
                    ctx,
                    dc,
                )?;
                if self.hdr.subband != Subband::DcOnly {
                    let lp_br = lp
                        .as_mut()
                        .ok_or_else(|| error("missing JPEG-XR LP subband packet"))?;
                    decode_macroblock_lowpass(
                        self.hdr.cf,
                        self.channels,
                        img.bitstream_fmt,
                        tile,
                        reset_rgi,
                        reset_ctx,
                        &mut self.mb,
                        ctx,
                        lp_br,
                    )?;
                }
                pred_dcac_dec(
                    self.hdr.cf,
                    self.channels,
                    ctx_left,
                    ctx_top,
                    c_col,
                    &mut self.mb,
                    &self.pred_cur,
                    &self.pred_prev,
                );
                dequantize_macroblock(
                    self.hdr.cf,
                    self.channels,
                    self.hdr.subband,
                    tile,
                    &self.mb,
                    &mut planes,
                );
                if self.hdr.subband == Subband::All || self.hdr.subband == Subband::NoFlexbits {
                    let ac_br = ac
                        .as_mut()
                        .ok_or_else(|| error("missing JPEG-XR HP subband packet"))?;
                    let skip_fl = fl.is_none();
                    decode_macroblock_highpass(
                        self.hdr.cf,
                        self.channels,
                        img.bitstream_fmt,
                        self.hdr.subband,
                        skip_fl,
                        tile,
                        ctx_left,
                        ctx_top,
                        reset_rgi,
                        reset_ctx,
                        c_col,
                        &mut self.mb,
                        &mut planes,
                        &mut self.pred_cur,
                        &self.pred_prev,
                        ctx,
                        ac_br,
                        fl.as_mut(),
                    )?;
                    pred_ac_dec(self.hdr.cf, self.channels, &self.mb, &mut planes);
                }
            }
        }

        let _ = cmb_width;
        update_pred_info(
            self.hdr.cf,
            self.channels,
            &self.mb,
            c_col,
            &mut self.pred_cur,
        );
        Ok(())
    }
}

pub(super) fn read_packet_header(br: &mut BitReader<'_>) -> Result<(), BitmapError> {
    if br.get_bits(8)? != 0 || br.get_bits(8)? != 0 || br.get_bits(8)? != 1 {
        return Err(error(
            "invalid JPEG-XR packet startcode (expected 0x000001)",
        ));
    }
    let _ = br.get_bits(8)?;
    Ok(())
}

pub(super) struct DecodedCodestream {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) has_interleaved_alpha: bool,
    pub(super) rgba: Vec<u8>,
    pub(super) y_only_samples: Option<Vec<u8>>,
}

pub(super) fn decode_codestream(stream: &[u8]) -> Result<DecodedCodestream, BitmapError> {
    let mut hdr_br = BitReader::new(stream);
    let img = read_wmi_image_header(&mut hdr_br)?;
    let primary_hdr = read_image_plane_header(&mut hdr_br)?;
    let alpha_hdr = if img.alpha_channel {
        let ah = read_image_plane_header(&mut hdr_br)?;
        if ah.cf != ColorFormat::YOnly {
            return Err(error(
                "JPEG-XR interleaved alpha plane requires Y_ONLY format",
            ));
        }
        Some(ah)
    } else {
        None
    };

    let num_tiles_v = img.tile_x.len();
    let num_tiles_h = img.tile_y.len();
    let c_sb = primary_hdr.subband.count();
    let c_num_bit_io = if !img.index_table_present {
        0usize
    } else if img.bitstream_fmt == BitstreamFormat::Spatial {
        num_tiles_v
    } else {
        num_tiles_v * c_sb
    };

    let mut index_entries: Vec<(usize, u8)> = Vec::new();
    if c_num_bit_io > 0 {
        if hdr_br.get_bits(16)? != 1 {
            return Err(error(
                "invalid JPEG-XR index table startcode (expected 0x0001)",
            ));
        }
        let total_entries = c_num_bit_io * num_tiles_h;
        index_entries.reserve(total_entries);
        for _ in 0..total_entries {
            index_entries.push(hdr_br.get_vlw_esc()?);
        }
    }
    let (profile_bytes, _) = hdr_br.get_vlw_esc()?;
    hdr_br.flush_to_byte()?;
    let header_size = hdr_br
        .byte_position()
        .checked_add(profile_bytes)
        .ok_or_else(|| error("JPEG-XR header size overflow"))?;
    if header_size > stream.len() {
        return Err(error("JPEG-XR codestream header exceeds stream length"));
    }

    let padded_w = (img.width as usize) + img.extra_left + img.extra_right;
    let padded_h = (img.height as usize) + img.extra_top + img.extra_bottom;
    let cmb_width = padded_w.div_ceil(16);
    let cmb_height = padded_h.div_ceil(16);

    let mut primary = PlaneDecState::new(primary_hdr, cmb_width, num_tiles_v);
    let mut alpha = alpha_hdr.map(|ah| PlaneDecState::new(ah, cmb_width, num_tiles_v));
    let mut tile_readers: Vec<TileBandReaders<'_>> = Vec::with_capacity(num_tiles_v);

    let out_w = img.width as usize;
    let out_h = img.height as usize;
    let is_y_only = primary.hdr.cf == ColorFormat::YOnly && img.output_clr_fmt == OUT_CLR_YONLY;
    let mut rgba = if is_y_only {
        Vec::new()
    } else {
        vec![0u8; out_w * out_h * 4]
    };
    let mut y_only_out = if is_y_only {
        vec![0u8; out_w * out_h]
    } else {
        Vec::new()
    };

    for c_row in 0..=cmb_height {
        primary.zero_a1();
        if let Some(ref mut a_state) = alpha {
            a_state.zero_a1();
        }

        for c_col in 0..=cmb_width {
            let bottom_or_right = c_row == cmb_height || c_col == cmb_width;
            if !bottom_or_right {
                if c_col == 0 {
                    primary.tile_col = 0;
                } else if primary.tile_col + 1 < num_tiles_v
                    && c_col == img.tile_x[primary.tile_col + 1]
                {
                    primary.tile_col += 1;
                }
                if c_row == 0 {
                    primary.tile_row = 0;
                } else if c_col == 0
                    && primary.tile_row + 1 < num_tiles_h
                    && c_row == img.tile_y[primary.tile_row + 1]
                {
                    primary.tile_row += 1;
                }
                let t_col = primary.tile_col;
                let t_row = primary.tile_row;
                let ctx_left = c_col == img.tile_x[t_col];
                let ctx_top = c_row == img.tile_y[t_row];
                let reset_rgi = ((c_col - img.tile_x[t_col]) & 0xF) == 0;
                let next_tile_x = if t_col + 1 < num_tiles_v {
                    img.tile_x[t_col + 1]
                } else {
                    cmb_width
                };
                let reset_ctx = reset_rgi || (c_col + 1 == next_tile_x);

                // Start of horizontal tile slice: attach packet readers & reset coding contexts
                if c_col == 0 && c_row == img.tile_y[t_row] {
                    tile_readers.clear();
                    for k in 0..num_tiles_v {
                        if img.bitstream_fmt == BitstreamFormat::Spatial {
                            let off = if c_num_bit_io == 0 {
                                header_size
                            } else {
                                header_size
                                    .checked_add(index_entries[c_num_bit_io * t_row + k].0)
                                    .ok_or_else(|| error("JPEG-XR packet offset overflow"))?
                            };
                            if off >= stream.len() {
                                return Err(error("JPEG-XR spatial packet offset out of bounds"));
                            }
                            let mut br = BitReader::new(&stream[off..]);
                            read_packet_header(&mut br)?;
                            let trim = if img.trim_flexbits_flag {
                                br.get_bits(4)? as i32
                            } else {
                                0
                            };
                            primary.contexts[k].trim_flex_bits = trim;
                            primary.contexts[k].reset();
                            if let Some(ref mut a_state) = alpha {
                                a_state.contexts[k].trim_flex_bits = trim;
                                a_state.contexts[k].reset();
                            }
                            tile_readers.push(TileBandReaders::Spatial(br));
                        } else {
                            let base_idx = c_num_bit_io * t_row + k * c_sb;
                            let dc_off = header_size
                                .checked_add(index_entries[base_idx].0)
                                .ok_or_else(|| error("JPEG-XR DC packet offset overflow"))?;
                            if dc_off >= stream.len() {
                                return Err(error("JPEG-XR DC packet offset out of bounds"));
                            }
                            let mut dc_br = BitReader::new(&stream[dc_off..]);
                            read_packet_header(&mut dc_br)?;

                            let lp_br = if c_sb > 1 {
                                let (rel, esc) = index_entries[base_idx + 1];
                                if esc != 0 {
                                    None
                                } else {
                                    let off = header_size
                                        .checked_add(rel)
                                        .ok_or_else(|| error("JPEG-XR LP offset overflow"))?;
                                    if off >= stream.len() {
                                        return Err(error(
                                            "JPEG-XR LP packet offset out of bounds",
                                        ));
                                    }
                                    let mut r = BitReader::new(&stream[off..]);
                                    read_packet_header(&mut r)?;
                                    Some(r)
                                }
                            } else {
                                None
                            };

                            let ac_br = if c_sb > 2 {
                                let (rel, esc) = index_entries[base_idx + 2];
                                if esc != 0 {
                                    None
                                } else {
                                    let off = header_size
                                        .checked_add(rel)
                                        .ok_or_else(|| error("JPEG-XR HP offset overflow"))?;
                                    if off >= stream.len() {
                                        return Err(error(
                                            "JPEG-XR HP packet offset out of bounds",
                                        ));
                                    }
                                    let mut r = BitReader::new(&stream[off..]);
                                    read_packet_header(&mut r)?;
                                    Some(r)
                                }
                            } else {
                                None
                            };

                            let mut trim = 0i32;
                            let fl_br = if c_sb > 3 {
                                let (rel, esc) = index_entries[base_idx + 3];
                                if esc != 0 {
                                    None
                                } else {
                                    let off = header_size
                                        .checked_add(rel)
                                        .ok_or_else(|| error("JPEG-XR FL offset overflow"))?;
                                    if off < stream.len() {
                                        let mut r = BitReader::new(&stream[off..]);
                                        if read_packet_header(&mut r).is_ok() {
                                            if img.trim_flexbits_flag {
                                                trim = r.get_bits(4)? as i32;
                                            }
                                            Some(r)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }
                            } else {
                                None
                            };

                            primary.contexts[k].trim_flex_bits = trim;
                            primary.contexts[k].reset();
                            if let Some(ref mut a_state) = alpha {
                                a_state.contexts[k].trim_flex_bits = trim;
                                a_state.contexts[k].reset();
                            }
                            tile_readers.push(TileBandReaders::Frequency {
                                dc: dc_br,
                                lp: lp_br,
                                ac: ac_br,
                                fl: fl_br,
                            });
                        }
                    }
                }

                // Read per-tile DQUANT headers at top-left macroblock of each tile
                if ctx_left && ctx_top {
                    match &mut tile_readers[t_col] {
                        TileBandReaders::Spatial(br) => {
                            primary.read_tile_header_dc(t_col, br)?;
                            if let Some(ref mut a_state) = alpha {
                                a_state.read_tile_header_dc(t_col, br)?;
                            }
                            if c_sb > 1 {
                                primary.read_tile_header_lp(t_col, br)?;
                                if let Some(ref mut a_state) = alpha {
                                    a_state.read_tile_header_lp(t_col, br)?;
                                }
                            }
                            if c_sb > 2 {
                                primary.read_tile_header_hp(t_col, br)?;
                                if let Some(ref mut a_state) = alpha {
                                    a_state.read_tile_header_hp(t_col, br)?;
                                }
                            }
                        }
                        TileBandReaders::Frequency { dc, lp, ac, .. } => {
                            primary.read_tile_header_dc(t_col, dc)?;
                            if let Some(ref mut a_state) = alpha {
                                a_state.read_tile_header_dc(t_col, dc)?;
                            }
                            if c_sb > 1
                                && let Some(lp_br) = lp.as_mut()
                            {
                                primary.read_tile_header_lp(t_col, lp_br)?;
                                if let Some(ref mut a_state) = alpha {
                                    a_state.read_tile_header_lp(t_col, lp_br)?;
                                }
                            }
                            if c_sb > 2
                                && let Some(ac_br) = ac.as_mut()
                            {
                                primary.read_tile_header_hp(t_col, ac_br)?;
                                if let Some(ref mut a_state) = alpha {
                                    a_state.read_tile_header_hp(t_col, ac_br)?;
                                }
                            }
                        }
                    }
                }

                primary.decode_mb_entropy(
                    &img,
                    cmb_width,
                    c_col,
                    ctx_left,
                    ctx_top,
                    reset_rgi,
                    reset_ctx,
                    &mut tile_readers[t_col],
                )?;
                primary.update_hard_tile_flags(&img, c_row, c_col);
                inv_transform_macroblock_dec(
                    primary.hdr.cf,
                    primary.channels,
                    img.overlap,
                    primary.hdr.scaled_arith,
                    cmb_width,
                    cmb_height,
                    c_row,
                    c_col,
                    primary.b_vert_tb,
                    primary.b_hori_tb,
                    primary.b_one_left_vtb,
                    primary.b_one_right_vtb,
                    &mut primary.a0,
                    &mut primary.a1,
                    &mut primary.pred_before,
                    &mut primary.pred_after,
                );

                if let Some(ref mut a_state) = alpha {
                    a_state.tile_col = t_col;
                    a_state.tile_row = t_row;
                    a_state.decode_mb_entropy(
                        &img,
                        cmb_width,
                        c_col,
                        ctx_left,
                        ctx_top,
                        reset_rgi,
                        reset_ctx,
                        &mut tile_readers[t_col],
                    )?;
                    a_state.update_hard_tile_flags(&img, c_row, c_col);
                    inv_transform_macroblock_dec(
                        a_state.hdr.cf,
                        a_state.channels,
                        img.overlap,
                        a_state.hdr.scaled_arith,
                        cmb_width,
                        cmb_height,
                        c_row,
                        c_col,
                        a_state.b_vert_tb,
                        a_state.b_hori_tb,
                        a_state.b_one_left_vtb,
                        a_state.b_one_right_vtb,
                        &mut a_state.a0,
                        &mut a_state.a1,
                        &mut a_state.pred_before,
                        &mut a_state.pred_after,
                    );
                }
            } else {
                primary.update_hard_tile_flags(&img, c_row, c_col);
                inv_transform_macroblock_dec(
                    primary.hdr.cf,
                    primary.channels,
                    img.overlap,
                    primary.hdr.scaled_arith,
                    cmb_width,
                    cmb_height,
                    c_row,
                    c_col,
                    primary.b_vert_tb,
                    primary.b_hori_tb,
                    primary.b_one_left_vtb,
                    primary.b_one_right_vtb,
                    &mut primary.a0,
                    &mut primary.a1,
                    &mut primary.pred_before,
                    &mut primary.pred_after,
                );
                if let Some(ref mut a_state) = alpha {
                    a_state.update_hard_tile_flags(&img, c_row, c_col);
                    inv_transform_macroblock_dec(
                        a_state.hdr.cf,
                        a_state.channels,
                        img.overlap,
                        a_state.hdr.scaled_arith,
                        cmb_width,
                        cmb_height,
                        c_row,
                        c_col,
                        a_state.b_vert_tb,
                        a_state.b_hori_tb,
                        a_state.b_one_left_vtb,
                        a_state.b_one_right_vtb,
                        &mut a_state.a0,
                        &mut a_state.a1,
                        &mut a_state.pred_before,
                        &mut a_state.pred_after,
                    );
                }
            }
        }

        // Output decoded MB row (c_row - 1) from a0
        if c_row > 0 {
            let mb_row_idx = c_row - 1;
            if matches!(primary.hdr.cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
                interpolate_uv_row(
                    primary.hdr.cf,
                    cmb_width,
                    cmb_height,
                    c_row,
                    &primary.a0,
                    &primary.a1,
                    &mut primary.res_u,
                    &mut primary.res_v,
                );
            }

            let shift = if primary.hdr.scaled_arith {
                SHIFTZERO + QPFRACBITS
            } else {
                0
            };
            let bias = (128i32 << shift)
                + if primary.hdr.scaled_arith {
                    (1i32 << (SHIFTZERO + QPFRACBITS - 1)) - 1
                } else {
                    0
                };

            let a_shift = alpha.as_ref().map_or(0, |a| {
                if a.hdr.scaled_arith {
                    SHIFTZERO + QPFRACBITS
                } else {
                    0
                }
            });
            let a_bias = (128i32 << a_shift)
                + if a_shift > 0 {
                    (1i32 << (a_shift - 1)) - 1
                } else {
                    0
                };

            for i_row in 0..16usize {
                let padded_y = mb_row_idx * 16 + i_row;
                if padded_y < img.extra_top || padded_y >= img.extra_top + out_h {
                    continue;
                }
                let dst_y = padded_y - img.extra_top;

                if is_y_only {
                    let row_out = &mut y_only_out[dst_y * out_w..(dst_y + 1) * out_w];
                    for x in 0..out_w {
                        let px_x = x + img.extra_left;
                        let idx = ((px_x >> 4) << 8) + IDX_CC[i_row][px_x & 0xF];
                        let v = (primary.a0[0][idx] + bias) >> shift;
                        row_out[x] = v.clamp(0, 255) as u8;
                    }
                } else if primary.hdr.cf == ColorFormat::YOnly {
                    // Grayscale expanded to RGB
                    for x in 0..out_w {
                        let px_x = x + img.extra_left;
                        let idx = ((px_x >> 4) << 8) + IDX_CC[i_row][px_x & 0xF];
                        let v = ((primary.a0[0][idx] + bias) >> shift).clamp(0, 255) as u8;
                        let a = if let Some(ref a_state) = alpha {
                            ((a_state.a0[0][idx] + a_bias) >> a_shift).clamp(0, 255) as u8
                        } else {
                            255
                        };
                        let dst_off = (dst_y * out_w + x) * 4;
                        rgba[dst_off..dst_off + 4].copy_from_slice(&[v, v, v, a]);
                    }
                } else {
                    let u_plane =
                        if matches!(primary.hdr.cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
                            &primary.res_u
                        } else {
                            &primary.a0[1]
                        };
                    let v_plane =
                        if matches!(primary.hdr.cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
                            &primary.res_v
                        } else {
                            &primary.a0[2]
                        };

                    for x in 0..out_w {
                        let px_x = x + img.extra_left;
                        let idx = ((px_x >> 4) << 8) + IDX_CC[i_row][px_x & 0xF];
                        let mut g = primary.a0[0][idx] + bias;
                        let mut r = -u_plane[idx];
                        let mut b = v_plane[idx];

                        // Inverse reversible YCoCg (_ICC in strdec.c)
                        g -= r >> 1;
                        r -= ((b + 1) >> 1) - g;
                        b += r;

                        if shift > 0 {
                            r >>= shift;
                            g >>= shift;
                            b >>= shift;
                        }
                        let a = if let Some(ref a_state) = alpha {
                            ((a_state.a0[0][idx] + a_bias) >> a_shift).clamp(0, 255) as u8
                        } else {
                            255
                        };

                        let (out_x, out_y) =
                            apply_orientation(x, dst_y, out_w, out_h, img.orientation);
                        let dst_off =
                            (out_y * if img.orientation >= 4 { out_h } else { out_w } + out_x) * 4;
                        rgba[dst_off] = r.clamp(0, 255) as u8;
                        rgba[dst_off + 1] = g.clamp(0, 255) as u8;
                        rgba[dst_off + 2] = b.clamp(0, 255) as u8;
                        rgba[dst_off + 3] = a;
                    }
                }
            }
        }

        primary.advance_one_mb_row();
        primary.swap_mr_buffers();
        if let Some(ref mut a_state) = alpha {
            a_state.advance_one_mb_row();
            a_state.swap_mr_buffers();
        }
    }

    let (final_w, final_h) = if !is_y_only && img.orientation >= 4 {
        (img.height, img.width)
    } else {
        (img.width, img.height)
    };

    Ok(DecodedCodestream {
        width: final_w,
        height: final_h,
        has_interleaved_alpha: img.alpha_channel,
        rgba,
        y_only_samples: if is_y_only { Some(y_only_out) } else { None },
    })
}

#[inline]
pub(super) fn apply_orientation(
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    orientation: u8,
) -> (usize, usize) {
    match orientation {
        1 => (x, h - 1 - y),         // O_FLIPV
        2 => (w - 1 - x, y),         // O_FLIPH
        3 => (w - 1 - x, h - 1 - y), // O_FLIPVH
        4 => (y, w - 1 - x),         // O_RCW
        5 => (y, x),                 // O_RCW_FLIPV
        6 => (h - 1 - y, w - 1 - x), // O_RCW_FLIPH
        7 => (h - 1 - y, x),         // O_RCW_FLIPVH
        _ => (x, y),                 // O_NONE
    }
}
