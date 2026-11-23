//! Pure-Rust ITU-T Rec. T.832 / ISO/IEC 29199-2 (JPEG-XR) baseline encoder and decoder.
//!
//! Adapted to safe Rust from Microsoft's BSD-2-Clause `jxrlib` reference implementation
//! (`https://github.com/4creators/jxrlib`, pinned commit `1ef17dd8b7b6e958241914c8911d942a43837233`).
//!
//! Copyright (c) Microsoft Corp.
//! All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions are met:
//!
//! * Redistributions of source code must retain the above copyright notice,
//!   this list of conditions and the following disclaimer.
//! * Redistributions in binary form must reproduce the above copyright notice,
//!   this list of conditions and the following disclaimer in the documentation
//!   and/or other materials provided with the distribution.
//!
//! THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
//! AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//! IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
//! ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
//! LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
//! CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
//! SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
//! INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
//! CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
//! ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
//! POSSIBILITY OF SUCH DAMAGE.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use super::{BitmapError, MAX_BITMAP_DIMENSION, error};
use crate::resource_image::DecodedImage;

/// `GUID_PKPixelFormat24bppBGR` (`{6fddc324-4e03-4bfe-b185-3d77768dc90c}`) in mixed-endian LE layout.
pub const GUID_PIXEL_FORMAT_24BPP_BGR: [u8; 16] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9, 0x0C,
];

/// `GUID_PKPixelFormat32bppBGRA` (`{6fddc324-4e03-4bfe-b185-3d77768dc90f}`) in mixed-endian LE layout.
pub const GUID_PIXEL_FORMAT_32BPP_BGRA: [u8; 16] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9, 0x0F,
];

const GUID_PIXEL_FORMAT_32BPP_RGB: [u8; 16] = [
    0x95, 0x6B, 0x8C, 0xD9, 0xFE, 0x3E, 0xD6, 0x47, 0xBB, 0x25, 0xEB, 0x17, 0x48, 0xAB, 0x0C, 0xF1,
];

const GUID_PIXEL_FORMAT_32BPP_RGBA: [u8; 16] = [
    0x2D, 0xAD, 0xC7, 0xF5, 0x8D, 0x6A, 0xDD, 0x43, 0xA7, 0xA8, 0xA2, 0x99, 0x35, 0x26, 0x1A, 0xE9,
];

const GUID_PREFIX_WIC: [u8; 15] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
];

pub const JXR_MAGIC: [u8; 4] = [0x49, 0x49, 0xBC, 0x01];
pub const WMPHOTO_SIGNATURE: [u8; 8] = *b"WMPHOTO\0";

// ITU-T Rec. T.832 Annex A IFD Tag IDs
pub const TAG_PIXEL_FORMAT: u16 = 0xBC01;
pub const TAG_TRANSFORMATION: u16 = 0xBC02;
pub const TAG_IMAGE_WIDTH: u16 = 0xBC80;
pub const TAG_IMAGE_HEIGHT: u16 = 0xBC81;
pub const TAG_WIDTH_RESOLUTION: u16 = 0xBC82;
pub const TAG_HEIGHT_RESOLUTION: u16 = 0xBC83;
pub const TAG_IMAGE_OFFSET: u16 = 0xBCC0;
pub const TAG_IMAGE_BYTE_COUNT: u16 = 0xBCC1;
pub const TAG_ALPHA_OFFSET: u16 = 0xBCC2;
pub const TAG_ALPHA_BYTE_COUNT: u16 = 0xBCC3;

// Legacy alias tags accepted on decode for defensive compatibility
const TAG_IMAGE_OFFSET_ALT: u16 = 0xBCE0;
const TAG_IMAGE_BYTE_COUNT_ALT: u16 = 0xBCE1;
const TAG_ALPHA_OFFSET_ALT: u16 = 0xBCE2;
const TAG_ALPHA_BYTE_COUNT_ALT: u16 = 0xBCE3;

// TIFF field types
const TIFF_TYPE_BYTE: u16 = 1;
const TIFF_TYPE_SHORT: u16 = 3;
const TIFF_TYPE_LONG: u16 = 4;
const TIFF_TYPE_FLOAT: u16 = 11;

// T.832 codec version & subversion constants
const CODEC_VERSION: u8 = 1;
const CODEC_SUBVERSION_OLD: u8 = 0;
const CODEC_SUBVERSION_NEWSCALING_SOFT_TILES: u8 = 1;
const CODEC_SUBVERSION_NEWSCALING_HARD_TILES: u8 = 9;

// T.832 internal & external color formats
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorFormat {
    YOnly = 0,
    Yuv420 = 1,
    Yuv422 = 2,
    Yuv444 = 3,
}

impl ColorFormat {
    fn from_internal_code(code: u8) -> Result<Self, BitmapError> {
        match code {
            0 => Ok(Self::YOnly),
            1 => Ok(Self::Yuv420),
            2 => Ok(Self::Yuv422),
            3 => Ok(Self::Yuv444),
            _ => Err(error("unsupported JPEG-XR internal color format")),
        }
    }

    fn num_channels(self) -> usize {
        match self {
            Self::YOnly => 1,
            Self::Yuv420 | Self::Yuv422 | Self::Yuv444 => 3,
        }
    }

    fn chroma_blocks(self) -> usize {
        match self {
            Self::YOnly => 0,
            Self::Yuv420 => 4,
            Self::Yuv422 => 8,
            Self::Yuv444 => 16,
        }
    }
}

const OUT_CLR_YONLY: u8 = 0;
const OUT_CLR_RGB: u8 = 7;
const BD_8: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Overlap {
    None = 0,
    One = 1,
    Two = 2,
}

impl Overlap {
    fn from_code(code: u8) -> Result<Self, BitmapError> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            _ => Err(error("invalid JPEG-XR overlap mode (3 is reserved)")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitstreamFormat {
    Spatial = 0,
    Frequency = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subband {
    All = 0,
    NoFlexbits = 1,
    NoHighpass = 2,
    DcOnly = 3,
}

impl Subband {
    fn from_code(code: u8) -> Result<Self, BitmapError> {
        match code {
            0 => Ok(Self::All),
            1 => Ok(Self::NoFlexbits),
            2 => Ok(Self::NoHighpass),
            3 => Ok(Self::DcOnly),
            _ => Err(error("unsupported JPEG-XR subband configuration")),
        }
    }

    fn count(self) -> usize {
        match self {
            Self::DcOnly => 1,
            Self::NoHighpass => 2,
            Self::NoFlexbits => 3,
            Self::All => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JxrAlphaMode {
    None,
    Interleaved,
    Planar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromaSubsampling {
    Yuv444,
    Yuv422,
    Yuv420,
}

impl ChromaSubsampling {
    fn from_color_space(color_space: &str) -> Result<Self, BitmapError> {
        match color_space {
            "auto" | "4:4:4" => Ok(Self::Yuv444),
            "4:2:2" => Ok(Self::Yuv422),
            "4:2:0" => Ok(Self::Yuv420),
            _ => Err(error(
                "JPEGXREncoderOptions.colorSpace requires 'auto', '4:4:4', '4:2:2' or '4:2:0'",
            )),
        }
    }

    fn color_format(self) -> ColorFormat {
        match self {
            Self::Yuv420 => ColorFormat::Yuv420,
            Self::Yuv422 => ColorFormat::Yuv422,
            Self::Yuv444 => ColorFormat::Yuv444,
        }
    }
}

/// Checks whether `bytes` begins with the ITU-T T.832 `.jxr` file header (`0x49, 0x49, 0xBC, 0x01`).
pub fn is_jxr_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == JXR_MAGIC
}

mod bitstream;
mod color_transform;
mod container;
mod decode;
mod encode;
mod entropy_highpass;
mod entropy_lowpass;
mod pct_forward;
mod pct_inverse;
mod prediction;
mod tables;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use self::container::{
    decode_jxr_to_rgba, encode_jxr_argb, encode_jxr_with_alpha_mode, jxr_dimensions,
};
