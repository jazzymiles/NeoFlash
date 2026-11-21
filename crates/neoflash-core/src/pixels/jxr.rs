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

// ============================================================================
// ITU-T Rec. T.832 / jxrlib Constant Tables
// ============================================================================

const CONTEXTX: usize = 8;
const CTDC: usize = 5;
const NUMVLCTABLES: usize = 21; // CONTEXTX * 2 + CTDC
const AVG_NDIFF: i32 = 3;
const MAXTOTAL: u32 = 32767;
const SHIFTZERO: i32 = 1;
const QPFRACBITS: i32 = 2;
const MODELWEIGHT: i32 = 70;
const ORIENT_WEIGHT: i32 = 4;
const MAX_CHANNELS: usize = 4;

const BLK_OFFSET: [usize; 16] = [
    0, 64, 16, 80, 128, 192, 144, 208, 32, 96, 48, 112, 160, 224, 176, 240,
];
const BLK_OFFSET_UV: [usize; 4] = [0, 32, 16, 48];
const BLK_OFFSET_UV_422: [usize; 8] = [0, 64, 16, 80, 32, 96, 48, 112];

const DCT_INDEX: [[usize; 16]; 3] = [
    [0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15],
    [0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15],
    [
        0, 128, 64, 208, 32, 240, 48, 224, 16, 192, 80, 144, 112, 176, 96, 160,
    ],
];

const IDX_CC: [[usize; 16]; 16] = [
    [
        0x00, 0x01, 0x05, 0x04, 0x40, 0x41, 0x45, 0x44, 0x80, 0x81, 0x85, 0x84, 0xc0, 0xc1, 0xc5,
        0xc4,
    ],
    [
        0x02, 0x03, 0x07, 0x06, 0x42, 0x43, 0x47, 0x46, 0x82, 0x83, 0x87, 0x86, 0xc2, 0xc3, 0xc7,
        0xc6,
    ],
    [
        0x0a, 0x0b, 0x0f, 0x0e, 0x4a, 0x4b, 0x4f, 0x4e, 0x8a, 0x8b, 0x8f, 0x8e, 0xca, 0xcb, 0xcf,
        0xce,
    ],
    [
        0x08, 0x09, 0x0d, 0x0c, 0x48, 0x49, 0x4d, 0x4c, 0x88, 0x89, 0x8d, 0x8c, 0xc8, 0xc9, 0xcd,
        0xcc,
    ],
    [
        0x10, 0x11, 0x15, 0x14, 0x50, 0x51, 0x55, 0x54, 0x90, 0x91, 0x95, 0x94, 0xd0, 0xd1, 0xd5,
        0xd4,
    ],
    [
        0x12, 0x13, 0x17, 0x16, 0x52, 0x53, 0x57, 0x56, 0x92, 0x93, 0x97, 0x96, 0xd2, 0xd3, 0xd7,
        0xd6,
    ],
    [
        0x1a, 0x1b, 0x1f, 0x1e, 0x5a, 0x5b, 0x5f, 0x5e, 0x9a, 0x9b, 0x9f, 0x9e, 0xda, 0xdb, 0xdf,
        0xde,
    ],
    [
        0x18, 0x19, 0x1d, 0x1c, 0x58, 0x59, 0x5d, 0x5c, 0x98, 0x99, 0x9d, 0x9c, 0xd8, 0xd9, 0xdd,
        0xdc,
    ],
    [
        0x20, 0x21, 0x25, 0x24, 0x60, 0x61, 0x65, 0x64, 0xa0, 0xa1, 0xa5, 0xa4, 0xe0, 0xe1, 0xe5,
        0xe4,
    ],
    [
        0x22, 0x23, 0x27, 0x26, 0x62, 0x63, 0x67, 0x66, 0xa2, 0xa3, 0xa7, 0xa6, 0xe2, 0xe3, 0xe7,
        0xe6,
    ],
    [
        0x2a, 0x2b, 0x2f, 0x2e, 0x6a, 0x6b, 0x6f, 0x6e, 0xaa, 0xab, 0xaf, 0xae, 0xea, 0xeb, 0xef,
        0xee,
    ],
    [
        0x28, 0x29, 0x2d, 0x2c, 0x68, 0x69, 0x6d, 0x6c, 0xa8, 0xa9, 0xad, 0xac, 0xe8, 0xe9, 0xed,
        0xec,
    ],
    [
        0x30, 0x31, 0x35, 0x34, 0x70, 0x71, 0x75, 0x74, 0xb0, 0xb1, 0xb5, 0xb4, 0xf0, 0xf1, 0xf5,
        0xf4,
    ],
    [
        0x32, 0x33, 0x37, 0x36, 0x72, 0x73, 0x77, 0x76, 0xb2, 0xb3, 0xb7, 0xb6, 0xf2, 0xf3, 0xf7,
        0xf6,
    ],
    [
        0x3a, 0x3b, 0x3f, 0x3e, 0x7a, 0x7b, 0x7f, 0x7e, 0xba, 0xbb, 0xbf, 0xbe, 0xfa, 0xfb, 0xff,
        0xfe,
    ],
    [
        0x38, 0x39, 0x3d, 0x3c, 0x78, 0x79, 0x7d, 0x7c, 0xb8, 0xb9, 0xbd, 0xbc, 0xf8, 0xf9, 0xfd,
        0xfc,
    ],
];

const IDX_CC_420: [[usize; 8]; 8] = [
    [0x00, 0x01, 0x05, 0x04, 0x20, 0x21, 0x25, 0x24],
    [0x02, 0x03, 0x07, 0x06, 0x22, 0x23, 0x27, 0x26],
    [0x0a, 0x0b, 0x0f, 0x0e, 0x2a, 0x2b, 0x2f, 0x2e],
    [0x08, 0x09, 0x0d, 0x0c, 0x28, 0x29, 0x2d, 0x2c],
    [0x10, 0x11, 0x15, 0x14, 0x30, 0x31, 0x35, 0x34],
    [0x12, 0x13, 0x17, 0x16, 0x32, 0x33, 0x37, 0x36],
    [0x1a, 0x1b, 0x1f, 0x1e, 0x3a, 0x3b, 0x3f, 0x3e],
    [0x18, 0x19, 0x1d, 0x1c, 0x38, 0x39, 0x3d, 0x3c],
];

const ZIGZAG_INV_4X4_LOWPASS: [usize; 16] = [0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15];
const ZIGZAG_INV_4X4_H: [usize; 16] = [0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15];
const ZIGZAG_INV_4X4_V: [usize; 16] = [0, 4, 8, 5, 1, 12, 9, 6, 2, 13, 3, 15, 7, 10, 14, 11];

const SIGNIFICANT_RUN_BIN: [i32; 15] = [-1, -1, -1, -1, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0];
const SIGNIFICANT_RUN_FIXED_LENGTH: [u8; 15] = [0, 0, 1, 1, 3, 0, 0, 1, 1, 2, 0, 0, 0, 0, 1];

const G_RES_LUT: [i32; 65] = [
    131, 127, 123, 119, 115, 111, 107, 103, 99, 95, 91, 87, 83, 79, 75, 71, 67, 63, 59, 55, 51, 47,
    43, 39, 35, 31, 27, 23, 19, 15, 11, 7, 0, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57,
    61, 65, 69, 73, 77, 81, 85, 89, 93, 97, 101, 105, 109, 113, 117, 121, 125, 129,
];

const QP_RECIP_TABLE: [(u32, i32); 32] = [
    (0x0000_0000, 0),
    (0x0000_0000, 0),
    (0x0000_0000, 1),
    (0xaaaa_aaab, 1),
    (0x0000_0000, 2),
    (0xcccc_cccd, 2),
    (0xaaaa_aaab, 2),
    (0x9249_2493, 2),
    (0x0000_0000, 3),
    (0xe38e_38e4, 3),
    (0xcccc_cccd, 3),
    (0xba2e_8ba3, 3),
    (0xaaaa_aaab, 3),
    (0x9d89_d89e, 3),
    (0x9249_2493, 3),
    (0x8888_8889, 3),
    (0x0000_0000, 4),
    (0xf0f0_f0f1, 4),
    (0xe38e_38e4, 4),
    (0xd794_35e6, 4),
    (0xcccc_cccd, 4),
    (0xc30c_30c4, 4),
    (0xba2e_8ba3, 4),
    (0xb216_42c9, 4),
    (0xaaaa_aaab, 4),
    (0xa3d7_0a3e, 4),
    (0x9d89_d89e, 4),
    (0x97b4_25ee, 4),
    (0x9249_2493, 4),
    (0x8d3d_cb09, 4),
    (0x8888_8889, 4),
    (0x8421_0843, 4),
];

// SSIM-optimized 8-bit QP tables from JxrEncApp.c: [Y_DC/LP, U_DC/LP, V_DC/LP, Y_HP, U_HP, V_HP]
const DPK_QPS_420: [[u8; 6]; 11] = [
    [67, 77, 80, 75, 82, 86],
    [58, 67, 71, 63, 74, 78],
    [50, 60, 64, 54, 66, 69],
    [46, 55, 59, 49, 60, 63],
    [41, 48, 53, 43, 52, 56],
    [35, 43, 48, 36, 44, 49],
    [29, 37, 41, 30, 38, 41],
    [22, 29, 33, 22, 29, 33],
    [15, 20, 26, 14, 20, 25],
    [9, 14, 18, 8, 14, 17],
    [4, 6, 7, 3, 5, 5],
];

const DPK_QPS_8: [[u8; 6]; 12] = [
    [67, 93, 98, 71, 98, 104],
    [59, 83, 88, 61, 89, 95],
    [50, 76, 81, 53, 85, 90],
    [46, 71, 77, 47, 79, 85],
    [41, 67, 71, 42, 75, 78],
    [34, 59, 65, 35, 66, 72],
    [30, 54, 60, 29, 60, 66],
    [24, 48, 53, 22, 53, 58],
    [18, 39, 45, 17, 43, 48],
    [13, 34, 38, 11, 35, 38],
    [8, 20, 24, 7, 22, 25],
    [2, 5, 6, 2, 5, 6],
];

// Adaptive Huffman decode lookup tables (HUFFMAN_DECODE_ROOT_BITS = 5, LOG = 3)
const G4_HUFF_LOOKUP: [i16; 40] = [
    19, 19, 19, 19, 27, 27, 27, 27, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
];

const G5_HUFF_LOOKUP: [[i16; 42]; 2] = [
    [
        28, 28, 36, 36, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        11, 11, 11, 11, 19, 19, 19, 19, 27, 27, 27, 27, 35, 35, 35, 35, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G6_HUFF_LOOKUP: [[i16; 44]; 4] = [
    [
        13, 29, 44, 44, 19, 19, 19, 19, 34, 34, 34, 34, 34, 34, 34, 34, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        12, 12, 28, 28, 43, 43, 43, 43, 2, 2, 2, 2, 2, 2, 2, 2, 18, 18, 18, 18, 18, 18, 18, 18, 34,
        34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        4, 4, 12, 12, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18, 26, 26, 26, 26, 26, 26, 26,
        26, 34, 34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        5, 13, 36, 36, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18, 25, 25, 25, 25, 25, 25, 25,
        25, 25, 25, 25, 25, 25, 25, 25, 25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G7_HUFF_LOOKUP: [[i16; 46]; 2] = [
    [
        45, 53, 36, 36, 27, 27, 27, 27, 2, 2, 2, 2, 2, 2, 2, 2, 10, 10, 10, 10, 10, 10, 10, 10, 18,
        18, 18, 18, 18, 18, 18, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 37, 28, 28, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G8_HUFF_LOOKUP: [[i16; 48]; 2] = [
    [
        53, 21, 28, 28, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59, 2, 2, 2, 2, 2, 2, 2, 2, 34,
        34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        52, 52, 20, 20, 3, 3, 3, 3, 11, 11, 11, 11, 27, 27, 27, 27, 35, 35, 35, 35, 43, 43, 43, 43,
        58, 58, 58, 58, 58, 58, 58, 58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G9_HUFF_LOOKUP: [[i16; 50]; 2] = [
    [
        13, 29, 37, 61, 20, 20, 68, 68, 3, 3, 3, 3, 51, 51, 51, 51, 41, 41, 41, 41, 41, 41, 41, 41,
        41, 41, 41, 41, 41, 41, 41, 41, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 53, 28, 28, 11, 11, 11, 11, 19, 19, 19, 19, 43, 43, 43, 43, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, -32734, 4, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G12_HUFF_LOOKUP: [[i16; 56]; 5] = [
    [
        -32736, 5, 76, 76, 37, 53, 69, 85, 43, 43, 43, 43, 91, 91, 91, 91, 57, 57, 57, 57, 57, 57,
        57, 57, 57, 57, 57, 57, 57, 57, 57, 57, -32734, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 85, 13, 53, 4, 4, 36, 36, 43, 43, 43, 43, 67, 67, 67, 67, 75, 75, 75, 75, 91, 91,
        91, 91, 58, 58, 58, 58, 58, 58, 58, 58, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 37, 92, 92, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59, 67, 67, 67, 67, 75, 75,
        75, 75, 2, 2, 2, 2, 2, 2, 2, 2, -32734, -32732, 2, 3, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 29, 37, 69, 3, 3, 3, 3, 43, 43, 43, 43, 59, 59, 59, 59, 75, 75, 75, 75, 91, 91, 91,
        91, 10, 10, 10, 10, 10, 10, 10, 10, -32734, 10, 2, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 93, 28, 28, 60, 60, 76, 76, 3, 3, 3, 3, 43, 43, 43, 43, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9, 9, 9, 9, 9, -32734, -32732, -32730, 2, 4, 8, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0,
    ],
];

// Adaptive Huffman encode code tables & discriminant delta tables
const G4_CODE_TABLE: [i32; 9] = [4, 1, 1, 1, 2, 0, 3, 1, 3];

const G5_CODE_TABLE: [i32; 22] = [
    5, 1, 1, 1, 2, 1, 3, 0, 4, 1, 4, // table 0
    5, 1, 1, 0, 3, 1, 3, 2, 3, 3, 3, // table 1
];
const G5_DELTA_TABLE: [i32; 5] = [0, -1, 0, 1, 1];

const G6_CODE_TABLE: [i32; 52] = [
    6, 1, 1, 0, 5, 1, 3, 1, 5, 1, 2, 1, 4, // table 0
    6, 1, 2, 0, 4, 2, 2, 1, 4, 3, 2, 1, 3, // table 1
    6, 0, 4, 1, 4, 1, 2, 2, 2, 3, 2, 1, 3, // table 2
    6, 0, 5, 1, 5, 1, 2, 1, 1, 1, 4, 1, 3, // table 3
];
const G6_DELTA_TABLE: [i32; 18] = [
    -1, 1, 1, 1, 0, 1, // 0
    -2, 0, 0, 2, 0, 0, // 1
    -1, -1, 0, 1, -2, 0, // 2
];

const G7_CODE_TABLE: [i32; 30] = [
    7, 1, 2, 2, 2, 3, 2, 1, 3, 1, 4, 0, 5, 1, 5, // table 0
    7, 1, 1, 1, 2, 1, 3, 1, 4, 1, 5, 0, 6, 1, 6, // table 1
];
const G7_DELTA_TABLE: [i32; 7] = [1, 0, -1, -1, -1, -1, -1];

const G8_CODE_TABLE: [i32; 34] = [
    8, 2, 2, 1, 3, 1, 5, 1, 4, 3, 2, 2, 3, 0, 5, 3, 3, // table 0
    8, 1, 3, 2, 3, 1, 4, 3, 3, 4, 3, 5, 3, 0, 4, 3, 2, // table 1
];

const G9_CODE_TABLE: [i32; 38] = [
    9, 2, 3, 0, 5, 2, 4, 1, 5, 2, 5, 1, 1, 3, 3, 3, 5, 3, 4, // table 0
    9, 1, 1, 1, 3, 2, 3, 1, 4, 1, 6, 3, 3, 1, 5, 0, 7, 1, 7, // table 1
];
const G9_DELTA_TABLE: [i32; 9] = [2, 2, 1, 1, -1, -2, -2, -2, -3];

const G12_CODE_TABLE: [i32; 125] = [
    12, 1, 5, 1, 6, 0, 7, 1, 7, 4, 5, 2, 3, 5, 5, 1, 1, 6, 5, 1, 4, 7, 5, 3, 3, // table 0
    12, 2, 4, 2, 5, 0, 6, 1, 6, 3, 4, 2, 3, 3, 5, 3, 2, 3, 3, 4, 3, 1, 5, 5, 3, // table 1
    12, 3, 2, 1, 3, 0, 7, 1, 7, 1, 5, 2, 3, 2, 7, 3, 3, 4, 3, 5, 3, 3, 7, 1, 4, // table 2
    12, 1, 3, 3, 2, 0, 7, 1, 5, 2, 5, 2, 3, 1, 7, 3, 3, 3, 5, 4, 3, 1, 6, 5, 3, // table 3
    12, 2, 3, 1, 1, 1, 7, 1, 4, 2, 7, 3, 3, 0, 8, 2, 4, 3, 7, 3, 4, 1, 8, 1, 5, // table 4
];
const G12_DELTA_TABLE: [i32; 48] = [
    1, 1, 1, 1, 1, 0, 0, -1, 2, 1, 0, 0, // 0
    2, 2, -1, -1, -1, 0, -2, -1, 0, 0, -2, -1, // 1
    -1, 1, 0, 2, 0, 0, 0, 0, -2, 0, 1, 1, // 2
    0, 1, 0, 1, -2, 0, -1, -1, -2, -1, -2, -2, // 3
];

const A_ALPHABET: [usize; NUMVLCTABLES] = [
    5, 4, 8, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7,
];

// ============================================================================
// MSB-First Unstuffed BitWriter & BitReader (ITU-T Rec. T.832)
// ============================================================================

struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    bits: u8,
}

impl BitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn put_bits(&mut self, value: u32, count: u8) {
        debug_assert!(count <= 32);
        if count == 0 {
            return;
        }
        let mask = if count == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << count) - 1
        };
        self.acc = (self.acc << count) | (u64::from(value) & mask);
        self.bits += count;
        while self.bits >= 8 {
            self.bits -= 8;
            self.bytes.push(((self.acc >> self.bits) & 0xFF) as u8);
        }
    }

    #[inline]
    fn put_bit(&mut self, bit: bool) {
        self.put_bits(u32::from(bit), 1);
    }

    fn fill_to_byte(&mut self) {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.put_bits(0, pad);
        }
    }

    fn write_bytes_aligned(&mut self, slice: &[u8]) {
        self.fill_to_byte();
        self.bytes.extend_from_slice(slice);
    }

    fn finish(mut self) -> Vec<u8> {
        self.fill_to_byte();
        self.bytes
    }
}

#[derive(Clone)]
struct BitReader<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    acc: u64,
    bits: u8,
    eof_pad_bytes: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            acc: 0,
            bits: 0,
            eof_pad_bytes: 0,
        }
    }

    #[inline]
    fn ensure_bits(&mut self, count: u8) -> Result<(), BitmapError> {
        while self.bits < count {
            if self.byte_pos < self.bytes.len() {
                self.acc = (self.acc << 8) | u64::from(self.bytes[self.byte_pos]);
                self.byte_pos += 1;
                self.bits += 8;
            } else if self.eof_pad_bytes < 8 {
                // Allow bounded zero-fill lookahead at packet tail, matching jxrlib's
                // zero-padded 2*PACKETLENGTH circular BitIOInfo lookahead window.
                self.acc <<= 8;
                self.eof_pad_bytes += 1;
                self.bits += 8;
            } else {
                return Err(error("unexpected EOF in JPEG-XR bitstream"));
            }
        }
        Ok(())
    }

    #[inline]
    fn peek_bits(&mut self, count: u8) -> Result<u32, BitmapError> {
        debug_assert!(count <= 32);
        if count == 0 {
            return Ok(0);
        }
        self.ensure_bits(count)?;
        let shift = self.bits - count;
        let mask = if count == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << count) - 1
        };
        Ok(((self.acc >> shift) & mask) as u32)
    }

    #[inline]
    fn flush_bits(&mut self, count: u8) -> Result<(), BitmapError> {
        debug_assert!(count <= 32);
        if count == 0 {
            return Ok(());
        }
        self.ensure_bits(count)?;
        self.bits -= count;
        if self.bits == 0 {
            self.acc = 0;
        } else {
            self.acc &= (1u64 << self.bits) - 1;
        }
        if self.eof_pad_bytes > 0 {
            let unconsumed_pad_bits = self.eof_pad_bytes * 8;
            if usize::from(self.bits) + 7 < unconsumed_pad_bits {
                return Err(error("unexpected EOF consuming JPEG-XR packet bits"));
            }
        }
        Ok(())
    }

    #[inline]
    fn get_bits(&mut self, count: u8) -> Result<u32, BitmapError> {
        let val = self.peek_bits(count)?;
        self.flush_bits(count)?;
        Ok(val)
    }

    #[inline]
    fn get_bool(&mut self) -> Result<bool, BitmapError> {
        Ok(self.get_bits(1)? != 0)
    }

    #[inline]
    fn get_sign_mask(&mut self) -> Result<i32, BitmapError> {
        Ok(if self.get_bool()? { -1 } else { 0 })
    }

    /// Reads `c_bits` plus a sign bit when the magnitude is nonzero (`_getBit16s`).
    #[inline]
    fn get_bits_signed(&mut self, c_bits: u8) -> Result<i32, BitmapError> {
        let peek = self.peek_bits(c_bits + 1)? as i32;
        let sign = peek & 1;
        let val = ((peek >> 1) ^ (-sign)) + sign;
        self.flush_bits(c_bits + u8::from(val != 0))?;
        Ok(val)
    }

    fn flush_to_byte(&mut self) -> Result<(), BitmapError> {
        let rem = self.bits & 7;
        if rem > 0 {
            self.flush_bits(rem)?;
        }
        Ok(())
    }

    fn byte_position(&self) -> usize {
        let total_loaded = self.byte_pos + self.eof_pad_bytes;
        total_loaded - usize::from(self.bits / 8)
    }

    /// Decodes a symbol from a `HUFFMAN_DECODE_ROOT_BITS = 5` lookup table (`getHuff`).
    #[inline]
    fn get_huff(&mut self, dec_table: &[i16]) -> Result<usize, BitmapError> {
        let root = self.peek_bits(5)? as usize;
        let entry = i32::from(dec_table[root]);
        self.flush_bits(if entry < 0 { 5 } else { (entry & 7) as u8 })?;
        let mut sym = entry >> 3;
        if sym < 0 {
            sym = entry;
            loop {
                let bit = self.get_bits(1)? as i32;
                let idx = (sym + 32768 + bit) as usize;
                let next = dec_table
                    .get(idx)
                    .copied()
                    .ok_or_else(|| error("corrupt JPEG-XR Huffman prefix code"))?;
                sym = i32::from(next);
                if sym >= 0 {
                    break;
                }
            }
        }
        Ok(sym as usize)
    }

    /// Reads a T.832 variable-length word with optional `0xFD..=0xFF` escape (`GetVLWordEsc`).
    fn get_vlw_esc(&mut self) -> Result<(usize, u8), BitmapError> {
        self.flush_to_byte()?;
        let first = self.get_bits(8)? as u8;
        if first >= 0xFD {
            Ok((0, first))
        } else if first < 0xFB {
            let lo = self.get_bits(8)? as usize;
            Ok(((usize::from(first) << 8) | lo, 0))
        } else if first == 0xFB {
            let hi = self.get_bits(16)? as usize;
            let lo = self.get_bits(16)? as usize;
            Ok(((hi << 16) | lo, 0))
        } else {
            // 0xFC: 8-byte unsigned integer
            let w0 = u64::from(self.get_bits(16)?);
            let w1 = u64::from(self.get_bits(16)?);
            let w2 = u64::from(self.get_bits(16)?);
            let w3 = u64::from(self.get_bits(16)?);
            let full = (w0 << 48) | (w1 << 32) | (w2 << 16) | w3;
            let sz =
                usize::try_from(full).map_err(|_| error("JPEG-XR 64-bit VLW_ESC size overflow"))?;
            Ok((sz, 0))
        }
    }
}

// ============================================================================
// Adaptive Huffman, Adaptive Model, CBP Model, Coding Context & Quantizer
// ============================================================================

#[derive(Clone)]
struct AdaptiveHuffman {
    n_symbols: usize,
    table: &'static [i32],
    delta: Option<&'static [i32]>,
    delta1: Option<&'static [i32]>,
    dec_table: &'static [i16],
    table_index: i32,
    initialized: bool,
    discriminant: i32,
    discriminant1: i32,
    upper_bound: i32,
    lower_bound: i32,
}

impl AdaptiveHuffman {
    fn new(n_symbols: usize) -> Self {
        let mut ah = Self {
            n_symbols,
            table: &G4_CODE_TABLE,
            delta: None,
            delta1: None,
            dec_table: &G4_HUFF_LOOKUP,
            table_index: 0,
            initialized: false,
            discriminant: 0,
            discriminant1: 0,
            upper_bound: 0,
            lower_bound: 0,
        };
        ah.adapt_discriminant();
        ah
    }

    fn adapt_discriminant(&mut self) {
        const THRESHOLD: i32 = 8;
        const MEMORY: i32 = 8;
        const MAX_TABLES: [i32; 13] = [0, 0, 0, 0, 1, 2, 4, 2, 2, 2, 0, 0, 5];
        const SECOND_DISC: [i32; 13] = [0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1];

        let sym = self.n_symbols;
        if !self.initialized {
            self.initialized = true;
            self.discriminant = 0;
            self.discriminant1 = 0;
            self.table_index = SECOND_DISC[sym];
        }

        let d_l = self.discriminant;
        let d_h = if SECOND_DISC[sym] != 0 {
            self.discriminant1
        } else {
            self.discriminant
        };

        let mut changed = false;
        if d_l < self.lower_bound {
            self.table_index -= 1;
            changed = true;
        } else if d_h > self.upper_bound {
            self.table_index += 1;
            changed = true;
        }
        if changed {
            self.discriminant = 0;
            self.discriminant1 = 0;
        }
        self.discriminant = self
            .discriminant
            .clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);
        self.discriminant1 = self
            .discriminant1
            .clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);

        let t = self.table_index.clamp(0, MAX_TABLES[sym] - 1) as usize;
        self.table_index = t as i32;
        self.lower_bound = if t == 0 { i32::MIN } else { -THRESHOLD };
        self.upper_bound = if self.table_index == MAX_TABLES[sym] - 1 {
            1 << 30
        } else {
            THRESHOLD
        };

        let stride = sym * 2 + 1;
        self.delta = None;
        self.delta1 = None;
        match sym {
            4 => {
                self.table = &G4_CODE_TABLE;
                self.dec_table = &G4_HUFF_LOOKUP;
            }
            5 => {
                self.table = &G5_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G5_DELTA_TABLE);
                self.dec_table = &G5_HUFF_LOOKUP[t];
            }
            6 => {
                self.table = &G6_CODE_TABLE[stride * t..stride * (t + 1)];
                let d1_idx = t - usize::from(t + 1 == MAX_TABLES[sym] as usize);
                let d0_idx = (t + usize::from(t == 0)) - 1;
                self.delta1 = Some(&G6_DELTA_TABLE[sym * d1_idx..sym * (d1_idx + 1)]);
                self.delta = Some(&G6_DELTA_TABLE[sym * d0_idx..sym * (d0_idx + 1)]);
                self.dec_table = &G6_HUFF_LOOKUP[t];
            }
            7 => {
                self.table = &G7_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G7_DELTA_TABLE);
                self.dec_table = &G7_HUFF_LOOKUP[t];
            }
            8 => {
                // Per jxrlib adapthuff.c line 494: symbol-8 table stays fixed at table 0
                self.table = &G8_CODE_TABLE[..stride];
                self.dec_table = &G8_HUFF_LOOKUP[0];
            }
            9 => {
                self.table = &G9_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G9_DELTA_TABLE);
                self.dec_table = &G9_HUFF_LOOKUP[t];
            }
            12 => {
                self.table = &G12_CODE_TABLE[stride * t..stride * (t + 1)];
                let d1_idx = t - usize::from(t + 1 == MAX_TABLES[sym] as usize);
                let d0_idx = (t + usize::from(t == 0)) - 1;
                self.delta1 = Some(&G12_DELTA_TABLE[sym * d1_idx..sym * (d1_idx + 1)]);
                self.delta = Some(&G12_DELTA_TABLE[sym * d0_idx..sym * (d0_idx + 1)]);
                self.dec_table = &G12_HUFF_LOOKUP[t];
            }
            _ => unreachable!("unsupported T.832 Huffman alphabet size"),
        }
    }

    #[inline]
    fn put_symbol(&self, bw: &mut BitWriter, idx: usize) {
        let code = self.table[idx * 2 + 1] as u32;
        let len = self.table[idx * 2 + 2] as u8;
        bw.put_bits(code, len);
    }

    #[inline]
    fn put_symbol_with_sign(&self, bw: &mut BitWriter, idx: usize, is_neg: bool) {
        let code = (self.table[idx * 2 + 1] as u32) * 2 + u32::from(is_neg);
        let len = (self.table[idx * 2 + 2] as u8) + 1;
        bw.put_bits(code, len);
    }

    #[inline]
    fn add_delta(&mut self, idx: usize) {
        if let Some(d) = self.delta {
            self.discriminant += d[idx];
        }
    }

    #[inline]
    fn add_both_deltas(&mut self, idx: usize) {
        if let Some(d) = self.delta {
            self.discriminant += d[idx];
        }
        if let Some(d1) = self.delta1 {
            self.discriminant1 += d1[idx];
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelBand {
    Dc = 0,
    Lp = 1,
    Ac = 2,
}

#[derive(Clone, Copy)]
struct AdaptiveModel {
    flc_state: [i32; 2],
    flc_bits: [i32; 2],
    band: ModelBand,
}

impl AdaptiveModel {
    fn new(band: ModelBand) -> Self {
        let init_bits = match band {
            ModelBand::Dc => 8,
            ModelBand::Lp => 4,
            ModelBand::Ac => 0,
        };
        Self {
            flc_state: [0, 0],
            flc_bits: [init_bits, init_bits],
            band,
        }
    }

    fn update_mb(&mut self, cf: ColorFormat, channels: usize, mut laplacian_mean: [i32; 2]) {
        const WEIGHT0: [i32; 3] = [240, 12, 1];
        const WEIGHT1: [[i32; 4]; 3] = [[0, 240, 120, 80], [0, 12, 6, 4], [0, 16, 8, 5]];
        const WEIGHT2: [i32; 6] = [120, 37, 2, 120, 18, 1];

        let b_idx = self.band as usize;
        laplacian_mean[0] *= WEIGHT0[b_idx];
        match cf {
            ColorFormat::Yuv420 => {
                laplacian_mean[1] *= WEIGHT2[b_idx];
            }
            ColorFormat::Yuv422 => {
                laplacian_mean[1] *= WEIGHT2[3 + b_idx];
            }
            ColorFormat::Yuv444 | ColorFormat::YOnly => {
                laplacian_mean[1] *= WEIGHT1[b_idx][(channels - 1).min(3)];
                if self.band == ModelBand::Ac {
                    laplacian_mean[1] >>= 4;
                }
            }
        }

        for j in 0..2 {
            let lm = laplacian_mean[j];
            let mut ms = self.flc_state[j];
            let mut delta = (lm - MODELWEIGHT) >> 2;
            if delta <= -8 {
                delta = (delta + 4).max(-16);
                ms += delta;
                if ms < -8 {
                    if self.flc_bits[j] == 0 {
                        ms = -8;
                    } else {
                        ms = 0;
                        self.flc_bits[j] -= 1;
                    }
                }
            } else if delta >= 8 {
                delta = (delta - 4).min(15);
                ms += delta;
                if ms > 8 {
                    if self.flc_bits[j] >= 15 {
                        self.flc_bits[j] = 15;
                        ms = 8;
                    } else {
                        ms = 0;
                        self.flc_bits[j] += 1;
                    }
                }
            }
            self.flc_state[j] = ms;
            if cf == ColorFormat::YOnly {
                break;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CbpModel {
    count0: [i32; 2],
    count1: [i32; 2],
    state: [i32; 2],
}

impl CbpModel {
    fn new() -> Self {
        Self {
            count0: [-4, -4],
            count1: [4, 4],
            state: [0, 0],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct AdaptiveScan {
    total: u32,
    scan: usize,
}

#[derive(Clone)]
struct CodingContext {
    adapt_huff_cbp_cy: AdaptiveHuffman,
    adapt_huff_cbp_cy1: AdaptiveHuffman,
    ah_expt: [AdaptiveHuffman; NUMVLCTABLES],
    scan_lowpass: [AdaptiveScan; 16],
    scan_horiz: [AdaptiveScan; 16],
    scan_vert: [AdaptiveScan; 16],
    model_ac: AdaptiveModel,
    model_lp: AdaptiveModel,
    model_dc: AdaptiveModel,
    cbp_count_zero: i32,
    cbp_count_max: i32,
    cbp_model: CbpModel,
    trim_flex_bits: i32,
}

impl CodingContext {
    fn new(cf: ColorFormat, trim_flex_bits: u8) -> Self {
        let cbp_size = if cf == ColorFormat::YOnly { 5 } else { 9 };
        let mut ctx = Self {
            adapt_huff_cbp_cy: AdaptiveHuffman::new(cbp_size),
            adapt_huff_cbp_cy1: AdaptiveHuffman::new(5),
            ah_expt: std::array::from_fn(|k| AdaptiveHuffman::new(A_ALPHABET[k])),
            scan_lowpass: [AdaptiveScan::default(); 16],
            scan_horiz: [AdaptiveScan::default(); 16],
            scan_vert: [AdaptiveScan::default(); 16],
            model_ac: AdaptiveModel::new(ModelBand::Ac),
            model_lp: AdaptiveModel::new(ModelBand::Lp),
            model_dc: AdaptiveModel::new(ModelBand::Dc),
            cbp_count_zero: 1,
            cbp_count_max: 1,
            cbp_model: CbpModel::new(),
            trim_flex_bits: i32::from(trim_flex_bits.min(15)),
        };
        ctx.reset();
        ctx
    }

    fn reset(&mut self) {
        self.adapt_huff_cbp_cy.initialized = false;
        self.adapt_huff_cbp_cy1.initialized = false;
        for ah in &mut self.ah_expt {
            ah.initialized = false;
        }
        self.adapt_lowpass();
        self.adapt_highpass();
        for i in 0..16 {
            self.scan_lowpass[i] = AdaptiveScan {
                total: 0,
                scan: ZIGZAG_INV_4X4_LOWPASS[i],
            };
            self.scan_horiz[i] = AdaptiveScan {
                total: 0,
                scan: DCT_INDEX[0][ZIGZAG_INV_4X4_H[i]],
            };
            self.scan_vert[i] = AdaptiveScan {
                total: 0,
                scan: DCT_INDEX[0][ZIGZAG_INV_4X4_V[i]],
            };
        }
        self.model_ac = AdaptiveModel::new(ModelBand::Ac);
        self.model_lp = AdaptiveModel::new(ModelBand::Lp);
        self.model_dc = AdaptiveModel::new(ModelBand::Dc);
        self.cbp_count_zero = 1;
        self.cbp_count_max = 1;
        self.cbp_model = CbpModel::new();
    }

    fn adapt_lowpass(&mut self) {
        for kk in 0..(CONTEXTX + CTDC) {
            self.ah_expt[kk].adapt_discriminant();
        }
    }

    fn adapt_highpass(&mut self) {
        self.adapt_huff_cbp_cy.adapt_discriminant();
        self.adapt_huff_cbp_cy1.adapt_discriminant();
        for kk in 0..CONTEXTX {
            self.ah_expt[kk + CONTEXTX + CTDC].adapt_discriminant();
        }
    }

    fn reset_lowpass_scan_totals(&mut self) {
        let mut weight = 32u32;
        self.scan_lowpass[0].total = MAXTOTAL;
        for k in 1..16 {
            self.scan_lowpass[k].total = weight;
            weight -= 2;
        }
    }

    fn reset_highpass_scan_totals(&mut self) {
        let mut weight = 32u32;
        self.scan_horiz[0].total = MAXTOTAL;
        self.scan_vert[0].total = MAXTOTAL;
        for k in 1..16 {
            self.scan_horiz[k].total = weight;
            self.scan_vert[k].total = weight;
            weight -= 2;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Quantizer {
    qp: i32,
    offset: i32,
    man: u32,
    exp: i32,
}

impl Quantizer {
    fn from_index(index: u8, shift: i32, scaled_arith: bool, is_dc: bool) -> Self {
        let mut q = Self {
            qp: 1,
            offset: 0,
            man: 0,
            exp: 0,
        };
        if index == 0 {
            // Lossless QP
            q.qp = 1;
        } else if !scaled_arith {
            let ci_shift = -QPFRACBITS; // -2
            let (man, exp) = if index < 32 {
                (usize::from(index + 3) >> 2, ci_shift + 2)
            } else if index < 48 {
                (
                    (16 + usize::from(index & 0xF) + 1) >> 1,
                    i32::from(index >> 4) - 1 + 1 + ci_shift,
                )
            } else {
                (
                    16 + usize::from(index & 0xF),
                    i32::from(index >> 4) - 1 + ci_shift,
                )
            };
            q.qp = (man as i32) << exp;
            q.man = QP_RECIP_TABLE[man].0;
            q.exp = QP_RECIP_TABLE[man].1 + exp;
            q.offset = (q.qp * 3 + 1) >> 3;
        } else {
            let (man, exp) = if index < 16 {
                (usize::from(index), shift)
            } else {
                (
                    16 + usize::from(index & 0xF),
                    i32::from(index >> 4) - 1 + shift,
                )
            };
            q.qp = (man as i32) << exp;
            q.man = QP_RECIP_TABLE[man].0;
            q.exp = QP_RECIP_TABLE[man].1 + exp;
            q.offset = (q.qp * 3 + 1) >> 3;
        }
        if is_dc {
            q.offset = q.qp >> 1;
        }
        q
    }

    #[inline]
    fn quantize(self, v: i32) -> i32 {
        if self.qp <= 1 && self.man == 0 && self.exp == 0 {
            return v;
        }
        let m = v >> 31;
        let abs_plus_off = ((v ^ m) - m + self.offset) as u32;
        let q = if self.man == 0 {
            (abs_plus_off >> (self.exp as u32)) as i32
        } else {
            let hi = ((u64::from(abs_plus_off) * u64::from(self.man)) >> 32) as u32;
            (hi >> (self.exp as u32)) as i32
        };
        (q ^ m) - m
    }
}

fn format_quantizer_row(
    indices: &[u8; MAX_CHANNELS],
    ch_mode: u8,
    channels: usize,
    shifted_uv: bool,
    scaled_arith: bool,
    is_dc: bool,
) -> [Quantizer; MAX_CHANNELS] {
    let mut out = [Quantizer::default(); MAX_CHANNELS];
    for ch in 0..channels {
        let idx = if ch == 0 || ch_mode == 0 {
            indices[0]
        } else if ch_mode == 1 {
            indices[1]
        } else {
            indices[ch]
        };
        let shift = if ch > 0 && shifted_uv {
            SHIFTZERO - 1
        } else {
            SHIFTZERO
        };
        out[ch] = Quantizer::from_index(idx, shift, scaled_arith, is_dc);
    }
    out
}

/// Maps `JPEGXREncoderOptions.quantization` (`0..=100`, where `0` is lossless) onto T.832 `QP_INDEX`
/// values `[Y_DC/LP, U_DC/LP, V_DC/LP, Y_HP, U_HP, V_HP]` using `jxrlib`'s SSIM tables.
fn map_flash_quantization_to_qps(quantization: u8, cf: ColorFormat) -> [u8; 6] {
    if quantization == 0 {
        return [0; 6];
    }
    let mut iq = (100.0_f32 - f32::from(quantization.min(100))) / 100.0_f32;
    let subsampled = matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422);
    if iq > 0.8 && !subsampled {
        iq = 0.8 + (iq - 0.8) * 1.5;
    }
    let scaled = 10.0 * iq;
    let max_row = if subsampled { 9usize } else { 10usize };
    let qi = (scaled.floor() as usize).min(max_row);
    let qf = (scaled - (qi as f32)).clamp(0.0, 1.0);

    let mut out = [0u8; 6];
    for c in 0..6 {
        let (lo, hi) = if subsampled {
            (
                f32::from(DPK_QPS_420[qi][c]),
                f32::from(DPK_QPS_420[qi + 1][c]),
            )
        } else {
            (f32::from(DPK_QPS_8[qi][c]), f32::from(DPK_QPS_8[qi + 1][c]))
        };
        let interp = (0.5 + lo * (1.0 - qf) + hi * qf) as u8;
        out[c] = interp.max(2);
    }
    out
}

#[derive(Clone, Copy, Default)]
struct PredInfo {
    qp_index: u8,
    cbp: i32,
    dc: i32,
    ad: [i32; 6],
}

#[derive(Clone, Default)]
struct MbInfo {
    block_dc: [[i32; 16]; MAX_CHANNELS],
    orientation: i32,
    cbp: [i32; MAX_CHANNELS],
    diff_cbp: [i32; MAX_CHANNELS],
    q_index_lp: u8,
    q_index_hp: u8,
}

#[derive(Clone)]
struct TileQuantizers {
    dc: [Quantizer; MAX_CHANNELS],
    lp: Vec<[Quantizer; MAX_CHANNELS]>,
    hp: Vec<[Quantizer; MAX_CHANNELS]>,
    num_qp_lp: u8,
    num_qp_hp: u8,
    bits_lp: u8,
    bits_hp: u8,
}

impl Default for TileQuantizers {
    fn default() -> Self {
        Self {
            dc: [Quantizer::default(); MAX_CHANNELS],
            lp: vec![[Quantizer::default(); MAX_CHANNELS]],
            hp: vec![[Quantizer::default(); MAX_CHANNELS]],
            num_qp_lp: 1,
            num_qp_hp: 1,
            bits_lp: 0,
            bits_hp: 0,
        }
    }
}

fn dquant_bits(num_qp: u8) -> u8 {
    if num_qp < 2 {
        0
    } else if num_qp < 4 {
        1
    } else if num_qp < 6 {
        2
    } else if num_qp < 10 {
        3
    } else {
        4
    }
}

// ============================================================================
// Reversible Photo Core Transform (PCT) & Overlap Filtering Operators
// ============================================================================

#[inline]
fn idx_at(base: isize, offset: isize) -> usize {
    let pos = base + offset;
    debug_assert!(pos >= 0);
    pos as usize
}

#[inline]
fn rotate1(a: &mut i32, b: &mut i32) {
    *b -= (*a + 1) >> 1;
    *a += (*b + 1) >> 1;
}

#[inline]
fn rotate2(a: &mut i32, b: &mut i32) {
    *b -= (*a * 3 + 4) >> 3;
    *a += (*b * 3 + 4) >> 3;
}

#[inline]
fn irotate1(a: &mut i32, b: &mut i32) {
    *a -= (*b + 1) >> 1;
    *b += (*a + 1) >> 1;
}

#[inline]
fn irotate2(a: &mut i32, b: &mut i32) {
    *a -= (*b * 3 + 4) >> 3;
    *b += (*a * 3 + 4) >> 3;
}

#[inline]
fn dct_2x2_dn_vals(mut a: i32, mut b: i32, c_in: i32, mut d: i32) -> (i32, i32, i32, i32) {
    a += d;
    b -= c_in;
    let t = (a - b) >> 1;
    let c = t - d;
    d = t - c_in;
    a -= d;
    b += c;
    (a, b, c, d)
}

#[inline]
fn dct_2x2_dn(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn dct_2x2_dn_enc(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0] >> 1, buf[i1] >> 1, buf[i2] >> 1, buf[i3] >> 1);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn dct_2x2_dn_dec(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a * 2;
    buf[i1] = b * 2;
    buf[i2] = c * 2;
    buf[i3] = d * 2;
}

#[inline]
fn dct_2x2_up(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, c_in, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    a += d;
    b -= c_in;
    let t = (a - b + 1) >> 1;
    let c = t - d;
    d = t - c_in;
    a -= d;
    b += c;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn fwd_odd_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let mut a = buf[i0];
    let mut b = -buf[i1];
    let mut c = -buf[i2];
    let mut d = buf[i3];
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a += (b * 3 + 4) >> 3;
    b -= (a * 3 + 3) >> 2;
    a += (b * 3 + 3) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn fwd_odd_odd_pre_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a += (b * 3 + 4) >> 3;
    b -= (a * 3 + 2) >> 2;
    a += (b * 3 + 6) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    (a, b, c, d)
}

#[inline]
fn fwd_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    b -= c;
    a += d;
    c += (b + 1) >> 1;
    d = ((a + 1) >> 1) - d;
    rotate2(&mut a, &mut b);
    rotate2(&mut c, &mut d);
    d += b >> 1;
    c -= (a + 1) >> 1;
    b -= d;
    a += c;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

fn str_dct_4x4_stage1(buf: &mut [i32], base: isize) {
    dct_2x2_dn(buf, base, 0, 4, 8, 12);
    dct_2x2_dn(buf, base, 1, 5, 9, 13);
    dct_2x2_dn(buf, base, 2, 6, 10, 14);
    dct_2x2_dn(buf, base, 3, 7, 11, 15);
    dct_2x2_up(buf, base, 0, 1, 2, 3);
    fwd_odd_odd(buf, base, 15, 14, 13, 12);
    fwd_odd(buf, base, 5, 4, 7, 6);
    fwd_odd(buf, base, 10, 8, 11, 9);
}

fn str_dct_4x4_second_stage(buf: &mut [i32], base: isize) {
    dct_2x2_dn(buf, base, 0, 192, 48, 240);
    dct_2x2_dn(buf, base, 64, 128, 112, 176);
    dct_2x2_dn(buf, base, 16, 208, 32, 224);
    dct_2x2_dn(buf, base, 80, 144, 96, 160);
    dct_2x2_up(buf, base, 0, 64, 16, 80);
    fwd_odd_odd(buf, base, 160, 224, 176, 240);
    fwd_odd(buf, base, 128, 192, 144, 208);
    fwd_odd(buf, base, 32, 48, 96, 112);
}

fn str_normalize_enc(buf: &mut [i32], base: isize, is_chroma: bool) {
    if is_chroma {
        for off in (0..256isize).step_by(16) {
            buf[idx_at(base, off)] >>= 1;
        }
    }
}

#[inline]
fn str_hst_enc_vals(mut a: i32, mut b: i32, pc: i32, pd: i32) -> (i32, i32, i32, i32) {
    let mut d = pc;
    let mut c = pd;
    a += c;
    b -= d;
    c = ((a - b) >> 1) - c;
    d += b >> 1;
    b += c;
    a -= (d * 3 + 4) >> 3;
    (a, b, c, d)
}

#[inline]
fn str_hst_enc1_vals(mut a: i32, mut d: i32) -> (i32, i32) {
    d -= a >> 7;
    d += a >> 10;
    d -= (a * 3) >> 4;
    a -= (d * 3) >> 3;
    d = (a >> 1) - d;
    a -= d;
    (a, d)
}

#[inline]
fn str_hst_enc1_edge(a: &mut i32, pd: &mut i32) {
    let mut av = *a;
    let mut d = -(*pd);
    av -= d;
    d += av >> 1;
    av -= (d * 3 + 4) >> 3;
    d -= av >> 7;
    d += av >> 10;
    d -= (av * 3) >> 4;
    av -= (d * 3) >> 3;
    d = (av >> 1) - d;
    av -= d;
    *a = av;
    *pd = d;
}

#[inline]
fn str_pre4_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    rotate1(&mut c, &mut d);
    str_hst_enc1_edge(&mut a, &mut d);
    str_hst_enc1_edge(&mut b, &mut c);
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
fn str_pre4(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_pre4_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn str_pre4_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_pre4_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

#[inline]
fn str_pre2_vals(mut a: i32, mut b: i32) -> (i32, i32) {
    b -= (a + 2) >> 2;
    a -= (b + 1) >> 1;
    a -= b >> 5;
    a -= b >> 9;
    a -= b >> 13;
    b -= (a + 2) >> 2;
    (a, b)
}

#[inline]
fn str_pre2(buf: &mut [i32], base: isize, o0: isize, o1: isize) {
    let (i0, i1) = (idx_at(base, o0), idx_at(base, o1));
    let (a, b) = str_pre2_vals(buf[i0], buf[i1]);
    buf[i0] = a;
    buf[i1] = b;
}

#[inline]
fn str_pre2_split(a0: &mut [i32], b0: isize, o0: isize, a1: &mut [i32], b1: isize, o1: isize) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b1, o1));
    let (a, b) = str_pre2_vals(a0[i0], a1[i1]);
    a0[i0] = a;
    a1[i1] = b;
}

#[inline]
fn str_pre2x2_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    b -= (a + 2) >> 2;
    a -= (b + 1) >> 1;
    a -= b >> 5;
    a -= b >> 9;
    a -= b >> 13;
    b -= (a + 2) >> 2;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
fn str_pre2x2(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_pre2x2_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn str_pre2x2_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_pre2x2_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

fn str_pre_4x4_core(q0: &mut [i32; 4], q1: &mut [i32; 4], q2: &mut [i32; 4], q3: &mut [i32; 4]) {
    for k in 0..4 {
        let (a, b, c, d) = str_hst_enc_vals(q0[k], q2[k], q1[k], q3[k]);
        let (a, d) = str_hst_enc1_vals(a, d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
    let (mut c12, mut c13) = (q1[2], q1[3]);
    rotate1(&mut c12, &mut c13);
    q1[2] = c12;
    q1[3] = c13;

    let (mut c10, mut c11) = (q1[0], q1[1]);
    rotate1(&mut c10, &mut c11);
    q1[0] = c10;
    q1[1] = c11;

    let (mut c21, mut c23) = (q2[1], q2[3]);
    rotate1(&mut c21, &mut c23);
    q2[1] = c21;
    q2[3] = c23;

    let (mut c20, mut c22) = (q2[0], q2[2]);
    rotate1(&mut c20, &mut c22);
    q2[0] = c20;
    q2[2] = c22;

    let (d0, d1, d2, d3) = fwd_odd_odd_pre_vals(q3[0], q3[1], q3[2], q3[3]);
    *q3 = [d0, d1, d2, d3];

    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q2[k], q1[k], q3[k]);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
}

fn str_pre_4x4_stage1(buf: &mut [i32], base: isize, offset: isize) {
    let p0 = idx_at(base, 12);
    let p1 = idx_at(base, 20);
    let p2 = idx_at(base, 72 - offset);
    let p3 = idx_at(base, 80 - offset);
    let mut q0 = [buf[p0], buf[p0 + 1], buf[p0 + 2], buf[p0 + 3]];
    let mut q1 = [buf[p1], buf[p1 + 1], buf[p1 + 2], buf[p1 + 3]];
    let mut q2 = [buf[p2], buf[p2 + 1], buf[p2 + 2], buf[p2 + 3]];
    let mut q3 = [buf[p3], buf[p3 + 1], buf[p3 + 2], buf[p3 + 3]];
    str_pre_4x4_core(&mut q0, &mut q1, &mut q2, &mut q3);
    buf[p0..p0 + 4].copy_from_slice(&q0);
    buf[p1..p1 + 4].copy_from_slice(&q1);
    buf[p2..p2 + 4].copy_from_slice(&q2);
    buf[p3..p3 + 4].copy_from_slice(&q3);
}

fn str_pre_4x4_stage1_split(a0: &mut [i32], b0: isize, a1: &mut [i32], b1: isize, offset: isize) {
    let p0 = idx_at(b0, 12);
    let p2 = idx_at(b0, 72 - offset);
    let p1 = idx_at(b1, 4);
    let p3 = idx_at(b1, 64 - offset);
    let mut q0 = [a0[p0], a0[p0 + 1], a0[p0 + 2], a0[p0 + 3]];
    let mut q1 = [a1[p1], a1[p1 + 1], a1[p1 + 2], a1[p1 + 3]];
    let mut q2 = [a0[p2], a0[p2 + 1], a0[p2 + 2], a0[p2 + 3]];
    let mut q3 = [a1[p3], a1[p3 + 1], a1[p3 + 2], a1[p3 + 3]];
    str_pre_4x4_core(&mut q0, &mut q1, &mut q2, &mut q3);
    a0[p0..p0 + 4].copy_from_slice(&q0);
    a0[p2..p2 + 4].copy_from_slice(&q2);
    a1[p1..p1 + 4].copy_from_slice(&q1);
    a1[p3..p3 + 4].copy_from_slice(&q3);
}

fn str_pre_4x4_stage2_split(a0: &mut [i32], p0: isize, a1: &mut [i32], p1: isize) {
    let o0 = [-96isize, -32, -80, -16];
    let o2 = [96isize, 32, 112, 48];
    let o1 = [-112isize, -48, -128, -64];
    let o3 = [80isize, 16, 64, 0];

    let mut q0 = [0i32; 4];
    let mut q1 = [0i32; 4];
    let mut q2 = [0i32; 4];
    let mut q3 = [0i32; 4];
    for k in 0..4 {
        let (a, b, c, d) = str_hst_enc_vals(
            a0[idx_at(p0, o0[k])],
            a0[idx_at(p0, o2[k])],
            a1[idx_at(p1, o1[k])],
            a1[idx_at(p1, o3[k])],
        );
        let (a, d) = str_hst_enc1_vals(a, d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    // ROTATE1(p1[-48], p1[-112]), ROTATE1(p1[-64], p1[-128]), ROTATE1(p0[112], p0[96]), ROTATE1(p0[48], p0[32])
    let (mut r0, mut r1) = (q1[1], q1[0]);
    rotate1(&mut r0, &mut r1);
    q1[1] = r0;
    q1[0] = r1;

    let (mut r2, mut r3) = (q1[3], q1[2]);
    rotate1(&mut r2, &mut r3);
    q1[3] = r2;
    q1[2] = r3;

    let (mut r4, mut r5) = (q2[2], q2[0]);
    rotate1(&mut r4, &mut r5);
    q2[2] = r4;
    q2[0] = r5;

    let (mut r6, mut r7) = (q2[3], q2[1]);
    rotate1(&mut r6, &mut r7);
    q2[3] = r6;
    q2[1] = r7;

    // fwdOddOddPre(p1 + 0, p1 + 64, p1 + 16, p1 + 80) -> indices [3, 2, 1, 0] of q3
    let (d0, d1, d2, d3) = fwd_odd_odd_pre_vals(q3[3], q3[2], q3[1], q3[0]);
    q3[3] = d0;
    q3[2] = d1;
    q3[1] = d2;
    q3[0] = d3;

    // strDCT2x2dn(p0[o0], p1[o1], p0[o2], p1[o3])
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q1[k], q2[k], q3[k]);
        a0[idx_at(p0, o0[k])] = a;
        a1[idx_at(p1, o1[k])] = b;
        a0[idx_at(p0, o2[k])] = c;
        a1[idx_at(p1, o3[k])] = d;
    }
}

/// Forward 2-stage PCT + overlap filter (`transformMacroblock` in `strFwdTransform.c`) for single-tile soft boundaries.
#[allow(clippy::too_many_arguments)]
fn transform_macroblock_enc(
    cf: ColorFormat,
    channels: usize,
    overlap: Overlap,
    scaled_arith: bool,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    c_column: usize,
    a0: &mut [Vec<i32>; MAX_CHANNELS],
    a1: &mut [Vec<i32>; MAX_CHANNELS],
    pred_before: &mut [[i32; 2]; 2],
    pred_after: &mut [[i32; 2]; 2],
) {
    let left = c_column == 0;
    let right = c_column == cmb_width;
    let top = c_row == 0;
    let bottom = c_row == cmb_height;
    let left_or_right = left || right;
    let top_or_bottom = top || bottom;
    let top_or_left = left || top;
    let left_adj = c_column == 1;
    let right_adj = c_column + 1 == cmb_width;

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    for i in 0..full_planes {
        let p0 = (c_column * 256) as isize;
        let p1 = (c_column * 256) as isize;
        let buf0 = &mut a0[i];
        let buf1 = &mut a1[i];

        if overlap != Overlap::None {
            if top && left {
                str_pre4(buf1, p1, 0, 1, 2, 3);
            }
            if top && right {
                str_pre4(buf1, p1, -59, -60, -57, -58);
            }
            if bottom && left {
                str_pre4(buf0, p0, 58, 59, 56, 57);
            }
            if bottom && right {
                str_pre4(buf0, p0, -1, -2, -3, -4);
            }
            if !right && !bottom {
                let j_start = if left { 0isize } else { -64isize };
                if top {
                    let mut j = j_start;
                    while j < 192 {
                        str_pre4(buf1, p1 + j, 5, 4, 64, 65);
                        str_pre4(buf1, p1 + j, 7, 6, 66, 67);
                        j += 64;
                    }
                } else {
                    let mut j = j_start;
                    while j < 192 {
                        str_pre_4x4_stage1_split(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                        j += 64;
                    }
                }

                if left {
                    if !top {
                        str_pre4_split(buf0, p0, 58, 56, buf1, p1, 0, 2);
                        str_pre4_split(buf0, p0, 59, 57, buf1, p1, 1, 3);
                    }
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre4(buf1, p1 + j, 74, 72, 80, 82);
                        str_pre4(buf1, p1 + j, 75, 73, 81, 83);
                        j += 16;
                    }
                } else {
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre_4x4_stage1(buf1, p1 + j, 0);
                        j += 16;
                    }
                }

                for off in [0isize, 16, 32, 64, 80, 96, 128, 144, 160] {
                    str_pre_4x4_stage1(buf1, p1 + off, 0);
                }
            }

            if bottom {
                let mut j = if left { 48isize } else { -16isize };
                let j_end = if right { -16isize } else { 240isize };
                while j < j_end {
                    str_pre4(buf0, p0 + j, 15, 14, 74, 75);
                    str_pre4(buf0, p0 + j, 13, 12, 72, 73);
                    j += 64;
                }
            }

            if right && !bottom {
                if !top {
                    str_pre4_split(buf0, p0, -1, -3, buf1, p1, -59, -57);
                    str_pre4_split(buf0, p0, -2, -4, buf1, p1, -60, -58);
                }
                let mut j = -64isize;
                while j < -16 {
                    str_pre4(buf1, p1 + j, 15, 13, 21, 23);
                    str_pre4(buf1, p1 + j, 14, 12, 20, 22);
                    j += 16;
                }
            }
        }

        if !top {
            let mut j = if left { 48isize } else { -16isize };
            let j_end = if right { 48isize } else { 240isize };
            while j < j_end {
                str_dct_4x4_stage1(buf0, p0 + j);
                j += 64;
            }
        }

        if !bottom {
            let mut j = if left { 0isize } else { -64isize };
            let j_end = if right { 0isize } else { 192isize };
            while j < j_end {
                str_dct_4x4_stage1(buf1, p1 + j);
                str_dct_4x4_stage1(buf1, p1 + j + 16);
                str_dct_4x4_stage1(buf1, p1 + j + 32);
                j += 64;
            }
        }

        if overlap == Overlap::Two {
            if top && left {
                str_pre4(buf1, p1, 0, 64, 16, 80);
            }
            if top && right {
                str_pre4(buf1, p1, -128, -64, -112, -48);
            }
            if bottom && left {
                str_pre4(buf0, p0, 32, 96, 48, 112);
            }
            if bottom && right {
                str_pre4(buf0, p0, -96, -32, -80, -16);
            }
            if left_or_right && !top_or_bottom {
                if left {
                    str_pre4_split(buf0, p0, 32, 48, buf1, p1, 0, 16);
                    str_pre4_split(buf0, p0, 96, 112, buf1, p1, 64, 80);
                }
                if right {
                    str_pre4_split(buf0, p0 - 128, 32, 48, buf1, p1 - 128, 0, 16);
                    str_pre4_split(buf0, p0 - 128, 96, 112, buf1, p1 - 128, 64, 80);
                }
            }
            if !left_or_right {
                if top_or_bottom {
                    if top {
                        str_pre4(buf1, p1, -128, -64, 0, 64);
                        str_pre4(buf1, p1, -112, -48, 16, 80);
                    }
                    if bottom {
                        str_pre4(buf0, p0 + 32, -128, -64, 0, 64);
                        str_pre4(buf0, p0 + 32, -112, -48, 16, 80);
                    }
                } else {
                    str_pre_4x4_stage2_split(buf0, p0, buf1, p1);
                }
            }
        }

        if !top_or_left {
            if scaled_arith {
                str_normalize_enc(buf0, p0 - 256, i != 0);
            }
            str_dct_4x4_second_stage(buf0, p0 - 256);
        }
    }

    if cf == ColorFormat::Yuv420 {
        for i in 0..2 {
            let p0 = (c_column * 64) as isize;
            let p1 = (c_column * 64) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if overlap != Overlap::None {
                if top && left {
                    str_pre4(buf1, p1, 0, 1, 2, 3);
                }
                if top && right {
                    str_pre4(buf1, p1, -27, -28, -25, -26);
                }
                if bottom && left {
                    str_pre4(buf0, p0, 26, 27, 24, 25);
                }
                if bottom && right {
                    str_pre4(buf0, p0, -1, -2, -3, -4);
                }
                if !right && !bottom {
                    let j_start = if left { 0isize } else { -32isize };
                    if top {
                        let mut j = j_start;
                        while j < 32 {
                            str_pre4(buf1, p1 + j, 5, 4, 32, 33);
                            str_pre4(buf1, p1 + j, 7, 6, 34, 35);
                            j += 32;
                        }
                    } else {
                        let mut j = j_start;
                        while j < 32 {
                            str_pre_4x4_stage1_split(buf0, p0 + 16 + j, buf1, p1 + j, 32);
                            j += 32;
                        }
                    }

                    if left {
                        if !top {
                            str_pre4_split(buf0, p0, 26, 24, buf1, p1, 0, 2);
                            str_pre4_split(buf0, p0, 27, 25, buf1, p1, 1, 3);
                        }
                        str_pre4(buf1, p1, 10, 8, 16, 18);
                        str_pre4(buf1, p1, 11, 9, 17, 19);
                    } else {
                        str_pre_4x4_stage1(buf1, p1 - 32, 32);
                    }
                    str_pre_4x4_stage1(buf1, p1, 32);
                }

                if bottom {
                    let mut j = if left { 16isize } else { -16isize };
                    let j_end = if right { -16isize } else { 32isize };
                    while j < j_end {
                        str_pre4(buf0, p0 + j, 15, 14, 42, 43);
                        str_pre4(buf0, p0 + j, 13, 12, 40, 41);
                        j += 32;
                    }
                }

                if right && !bottom {
                    if !top {
                        str_pre4_split(buf0, p0, -1, -3, buf1, p1, -27, -25);
                        str_pre4_split(buf0, p0, -2, -4, buf1, p1, -28, -26);
                    }
                    str_pre4(buf1, p1, -17, -19, -11, -9);
                    str_pre4(buf1, p1, -18, -20, -12, -10);
                }
            }

            if !top {
                let mut j = if left { 16isize } else { -16isize };
                let j_end = if right { 16isize } else { 48isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf0, p0 + j);
                    j += 32;
                }
            }

            if !bottom {
                let mut j = if left { 0isize } else { -32isize };
                let j_end = if right { 0isize } else { 32isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf1, p1 + j);
                    j += 32;
                }
            }

            if overlap == Overlap::Two {
                if left_adj && top {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] -= v;
                }
                if right_adj && top {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -32)] -= pred_before[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] -= v;
                }
                if right_adj && bottom {
                    pred_before[i][1] = buf0[idx_at(p0, 16)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if left_or_right && !top_or_bottom {
                    if left {
                        str_pre2_split(buf0, p0, 16, buf1, p1, 0);
                    }
                    if right {
                        str_pre2_split(buf0, p0, -16, buf1, p1, -32);
                    }
                }
                if !left_or_right {
                    if top_or_bottom {
                        if top {
                            str_pre2(buf1, p1, -32, 0);
                        }
                        if bottom {
                            str_pre2(buf0, p0, -16, 16);
                        }
                    } else {
                        str_pre2x2_split(buf0, p0, -16, 16, buf1, p1, -32, 0);
                    }
                }

                if left_adj && top {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] += v;
                }
                if right_adj && top {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -32)] += pred_after[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] += v;
                }
                if right_adj && bottom {
                    pred_after[i][1] = buf0[idx_at(p0, 16)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top_or_left {
                if !scaled_arith {
                    dct_2x2_dn(buf0, p0, -64, -32, -48, -16);
                } else {
                    dct_2x2_dn_enc(buf0, p0, -64, -32, -48, -16);
                }
            }
        }
    }

    if cf == ColorFormat::Yuv422 {
        for i in 0..2 {
            let p0 = (c_column * 128) as isize;
            let p1 = (c_column * 128) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if overlap != Overlap::None {
                if top && left {
                    str_pre4(buf1, p1, 0, 1, 2, 3);
                }
                if top && right {
                    str_pre4(buf1, p1, -59, -60, -57, -58);
                }
                if bottom && left {
                    str_pre4(buf0, p0, 58, 59, 56, 57);
                }
                if bottom && right {
                    str_pre4(buf0, p0, -1, -2, -3, -4);
                }
                if !right && !bottom {
                    let j_start = if left { 0isize } else { -64isize };
                    if top {
                        let mut j = j_start;
                        while j < 64 {
                            str_pre4(buf1, p1 + j, 5, 4, 64, 65);
                            str_pre4(buf1, p1 + j, 7, 6, 66, 67);
                            j += 64;
                        }
                    } else {
                        let mut j = j_start;
                        while j < 64 {
                            str_pre_4x4_stage1_split(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                            j += 64;
                        }
                    }

                    if left {
                        if !top {
                            str_pre4_split(buf0, p0, 58, 56, buf1, p1, 0, 2);
                            str_pre4_split(buf0, p0, 59, 57, buf1, p1, 1, 3);
                        }
                        let mut j = 0isize;
                        while j < 48 {
                            str_pre4(buf1, p1 + j, 10, 8, 16, 18);
                            str_pre4(buf1, p1 + j, 11, 9, 17, 19);
                            j += 16;
                        }
                    } else {
                        let mut j = -64isize;
                        while j < -16 {
                            str_pre_4x4_stage1(buf1, p1 + j, 0);
                            j += 16;
                        }
                    }
                    str_pre_4x4_stage1(buf1, p1, 0);
                    str_pre_4x4_stage1(buf1, p1 + 16, 0);
                    str_pre_4x4_stage1(buf1, p1 + 32, 0);
                }

                if bottom {
                    let mut j = if left { 48isize } else { -16isize };
                    let j_end = if right { -16isize } else { 112isize };
                    while j < j_end {
                        str_pre4(buf0, p0 + j, 15, 14, 74, 75);
                        str_pre4(buf0, p0 + j, 13, 12, 72, 73);
                        j += 64;
                    }
                }

                if right && !bottom {
                    if !top {
                        str_pre4_split(buf0, p0, -1, -3, buf1, p1, -59, -57);
                        str_pre4_split(buf0, p0, -2, -4, buf1, p1, -60, -58);
                    }
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre4(buf1, p1 + j, 15, 13, 21, 23);
                        str_pre4(buf1, p1 + j, 14, 12, 20, 22);
                        j += 16;
                    }
                }
            }

            if !top {
                let mut j = if left { 48isize } else { -16isize };
                let j_end = if right { 48isize } else { 112isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf0, p0 + j);
                    j += 64;
                }
            }

            if !bottom {
                let mut j = if left { 0isize } else { -64isize };
                let j_end = if right { 0isize } else { 64isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf1, p1 + j);
                    str_dct_4x4_stage1(buf1, p1 + j + 16);
                    str_dct_4x4_stage1(buf1, p1 + j + 32);
                    j += 64;
                }
            }

            if overlap == Overlap::Two {
                if left_adj && top {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] -= v;
                }
                if right_adj && top {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -64)] -= pred_before[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] -= v;
                }
                if right_adj && bottom {
                    pred_before[i][1] = buf0[idx_at(p0, 48)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if !bottom {
                    if left_or_right {
                        if !top {
                            if left {
                                str_pre2_split(buf0, p0, 48, buf1, p1, 0);
                            }
                            if right {
                                str_pre2_split(buf0, p0, -16, buf1, p1, -64);
                            }
                        }
                        if left {
                            str_pre2(buf1, p1, 16, 32);
                        }
                        if right {
                            str_pre2(buf1, p1, -48, -32);
                        }
                    }
                    if !left_or_right {
                        if top {
                            str_pre2(buf1, p1, -64, 0);
                        } else {
                            str_pre2x2_split(buf0, p0, -16, 48, buf1, p1, -64, 0);
                        }
                        str_pre2x2(buf1, p1, -48, 16, -32, 32);
                    }
                }
                if bottom && !left_or_right {
                    str_pre2(buf0, p0, -16, 48);
                }

                if left_adj && top {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] += v;
                }
                if right_adj && top {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -64)] += pred_after[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] += v;
                }
                if right_adj && bottom {
                    pred_after[i][1] = buf0[idx_at(p0, 48)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top_or_left {
                if !scaled_arith {
                    dct_2x2_dn(buf0, p0, -128, -64, -112, -48);
                    dct_2x2_dn(buf0, p0, -96, -32, -80, -16);
                } else {
                    dct_2x2_dn_enc(buf0, p0, -128, -64, -112, -48);
                    dct_2x2_dn_enc(buf0, p0, -96, -32, -80, -16);
                }
                let i96 = idx_at(p0, -96);
                let i128 = idx_at(p0, -128);
                buf0[i96] -= buf0[i128];
                buf0[i128] += (buf0[i96] + 1) >> 1;
            }
        }
    }
}

// Inverse PCT & Post-Filtering Operators (invTransformMacroblock_alteredOperators_hard)

#[inline]
fn inv_odd_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a -= (b * 3 + 3) >> 3;
    b += (a * 3 + 3) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    buf[i0] = a;
    buf[i1] = -b;
    buf[i2] = -c;
    buf[i3] = d;
}

#[inline]
fn inv_odd_odd_post_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a -= (b * 3 + 6) >> 3;
    b += (a * 3 + 2) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    (a, b, c, d)
}

#[inline]
fn inv_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    b += d;
    a -= c;
    d -= b >> 1;
    c += (a + 1) >> 1;
    irotate2(&mut a, &mut b);
    irotate2(&mut c, &mut d);
    c -= (b + 1) >> 1;
    d = ((a + 1) >> 1) - d;
    b += c;
    a -= d;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

fn str_idct_4x4_stage1(buf: &mut [i32], base: isize) {
    dct_2x2_up(buf, base, 0, 1, 2, 3);
    inv_odd(buf, base, 5, 4, 7, 6);
    inv_odd(buf, base, 10, 8, 11, 9);
    inv_odd_odd(buf, base, 15, 14, 13, 12);
    dct_2x2_dn(buf, base, 0, 4, 8, 12);
    dct_2x2_dn(buf, base, 1, 5, 9, 13);
    dct_2x2_dn(buf, base, 2, 6, 10, 14);
    dct_2x2_dn(buf, base, 3, 7, 11, 15);
}

fn str_idct_4x4_stage2(buf: &mut [i32], base: isize) {
    inv_odd(buf, base, 32, 48, 96, 112);
    inv_odd(buf, base, 128, 192, 144, 208);
    inv_odd_odd(buf, base, 160, 224, 176, 240);
    dct_2x2_up(buf, base, 0, 64, 16, 80);
    dct_2x2_dn(buf, base, 0, 192, 48, 240);
    dct_2x2_dn(buf, base, 64, 128, 112, 176);
    dct_2x2_dn(buf, base, 16, 208, 32, 224);
    dct_2x2_dn(buf, base, 80, 144, 96, 160);
}

fn str_normalize_dec(buf: &mut [i32], base: isize, is_chroma: bool) {
    if is_chroma {
        for off in (0..256isize).step_by(16) {
            let idx = idx_at(base, off);
            buf[idx] *= 2;
        }
    }
}

#[inline]
fn str_hst_dec1_alt_vals(mut a: i32, mut d: i32) -> (i32, i32) {
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    (a, d)
}

#[inline]
fn str_hst_dec1_edge(pa: &mut i32, pd: &mut i32) {
    let mut a = *pa;
    let mut d = *pd;
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    a += (d * 3 + 4) >> 3;
    d -= a >> 1;
    a += d;
    *pa = a;
    *pd = -d;
}

#[inline]
fn str_hst_dec_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    b -= c;
    a += (d * 3 + 4) >> 3;
    d -= b >> 1;
    c = ((a - b) >> 1) - c;
    (a - c, b + d, d, c)
}

#[inline]
fn str_post4_alt_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    str_hst_dec1_edge(&mut a, &mut d);
    str_hst_dec1_edge(&mut b, &mut c);
    irotate1(&mut c, &mut d);
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
fn str_post4_alt(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_post4_alt_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn str_post4_alt_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_post4_alt_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

#[inline]
fn str_post2_alt_vals(mut a: i32, mut b: i32) -> (i32, i32) {
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    (a, b)
}

#[inline]
fn str_post2_alt(buf: &mut [i32], base: isize, o0: isize, o1: isize) {
    let (i0, i1) = (idx_at(base, o0), idx_at(base, o1));
    let (a, b) = str_post2_alt_vals(buf[i0], buf[i1]);
    buf[i0] = a;
    buf[i1] = b;
}

#[inline]
fn str_post2_alt_split(a0: &mut [i32], b0: isize, o0: isize, a1: &mut [i32], b1: isize, o1: isize) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b1, o1));
    let (a, b) = str_post2_alt_vals(a0[i0], a1[i1]);
    a0[i0] = a;
    a1[i1] = b;
}

#[inline]
fn str_post2x2_alt_vals(mut a: i32, mut b: i32, mut c: i32, mut d: i32) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
fn str_post2x2_alt(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_post2x2_alt_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
fn str_post2x2_alt_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_post2x2_alt_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

fn str_post_4x4_core_alt(
    q0: &mut [i32; 4],
    q1: &mut [i32; 4],
    q2: &mut [i32; 4],
    q3: &mut [i32; 4],
) {
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q2[k], q1[k], q3[k]);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    let (d0, d1, d2, d3) = inv_odd_odd_post_vals(q3[0], q3[1], q3[2], q3[3]);
    *q3 = [d0, d1, d2, d3];

    let (mut c12, mut c13) = (q1[2], q1[3]);
    irotate1(&mut c12, &mut c13);
    q1[2] = c12;
    q1[3] = c13;

    let (mut c10, mut c11) = (q1[0], q1[1]);
    irotate1(&mut c10, &mut c11);
    q1[0] = c10;
    q1[1] = c11;

    let (mut c21, mut c23) = (q2[1], q2[3]);
    irotate1(&mut c21, &mut c23);
    q2[1] = c21;
    q2[3] = c23;

    let (mut c20, mut c22) = (q2[0], q2[2]);
    irotate1(&mut c20, &mut c22);
    q2[0] = c20;
    q2[2] = c22;

    for k in 0..4 {
        let (a, d) = str_hst_dec1_alt_vals(q0[k], q3[k]);
        let (a, b, c, d) = str_hst_dec_vals(a, q2[k], q1[k], d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
}

fn str_post_4x4_stage1_alt(buf: &mut [i32], base: isize, offset: isize) {
    let p0 = idx_at(base, 12);
    let p1 = idx_at(base, 20);
    let p2 = idx_at(base, 72 - offset);
    let p3 = idx_at(base, 80 - offset);
    let mut q0 = [buf[p0], buf[p0 + 1], buf[p0 + 2], buf[p0 + 3]];
    let mut q1 = [buf[p1], buf[p1 + 1], buf[p1 + 2], buf[p1 + 3]];
    let mut q2 = [buf[p2], buf[p2 + 1], buf[p2 + 2], buf[p2 + 3]];
    let mut q3 = [buf[p3], buf[p3 + 1], buf[p3 + 2], buf[p3 + 3]];
    str_post_4x4_core_alt(&mut q0, &mut q1, &mut q2, &mut q3);
    buf[p0..p0 + 4].copy_from_slice(&q0);
    buf[p1..p1 + 4].copy_from_slice(&q1);
    buf[p2..p2 + 4].copy_from_slice(&q2);
    buf[p3..p3 + 4].copy_from_slice(&q3);
}

fn str_post_4x4_stage1_split_alt(
    a0: &mut [i32],
    b0: isize,
    a1: &mut [i32],
    b1: isize,
    offset: isize,
) {
    let p0 = idx_at(b0, 12);
    let p2 = idx_at(b0, 72 - offset);
    let p1 = idx_at(b1, 4);
    let p3 = idx_at(b1, 64 - offset);
    let mut q0 = [a0[p0], a0[p0 + 1], a0[p0 + 2], a0[p0 + 3]];
    let mut q1 = [a1[p1], a1[p1 + 1], a1[p1 + 2], a1[p1 + 3]];
    let mut q2 = [a0[p2], a0[p2 + 1], a0[p2 + 2], a0[p2 + 3]];
    let mut q3 = [a1[p3], a1[p3 + 1], a1[p3 + 2], a1[p3 + 3]];
    str_post_4x4_core_alt(&mut q0, &mut q1, &mut q2, &mut q3);
    a0[p0..p0 + 4].copy_from_slice(&q0);
    a0[p2..p2 + 4].copy_from_slice(&q2);
    a1[p1..p1 + 4].copy_from_slice(&q1);
    a1[p3..p3 + 4].copy_from_slice(&q3);
}

fn str_post_4x4_stage2_split_alt(a0: &mut [i32], p0: isize, a1: &mut [i32], p1: isize) {
    let o0 = [-96isize, -32, -80, -16];
    let o2 = [96isize, 32, 112, 48];
    let o1 = [-112isize, -48, -128, -64];
    let o3 = [80isize, 16, 64, 0];

    let mut q0 = [0i32; 4];
    let mut q1 = [0i32; 4];
    let mut q2 = [0i32; 4];
    let mut q3 = [0i32; 4];
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(
            a0[idx_at(p0, o0[k])],
            a0[idx_at(p0, o2[k])],
            a1[idx_at(p1, o1[k])],
            a1[idx_at(p1, o3[k])],
        );
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    let (d0, d1, d2, d3) = inv_odd_odd_post_vals(q3[3], q3[2], q3[1], q3[0]);
    q3[3] = d0;
    q3[2] = d1;
    q3[1] = d2;
    q3[0] = d3;

    let (mut r0, mut r1) = (q2[3], q2[1]);
    irotate1(&mut r0, &mut r1);
    q2[3] = r0;
    q2[1] = r1;

    let (mut r2, mut r3) = (q2[2], q2[0]);
    irotate1(&mut r2, &mut r3);
    q2[2] = r2;
    q2[0] = r3;

    let (mut r4, mut r5) = (q1[3], q1[2]);
    irotate1(&mut r4, &mut r5);
    q1[3] = r4;
    q1[2] = r5;

    let (mut r6, mut r7) = (q1[1], q1[0]);
    irotate1(&mut r6, &mut r7);
    q1[1] = r6;
    q1[0] = r7;

    for k in 0..4 {
        let (a, d) = str_hst_dec1_alt_vals(q0[k], q3[k]);
        let (a, b, c, d) = str_hst_dec_vals(a, q1[k], q2[k], d);
        a0[idx_at(p0, o0[k])] = a;
        a1[idx_at(p1, o1[k])] = b;
        a0[idx_at(p0, o2[k])] = c;
        a1[idx_at(p1, o3[k])] = d;
    }
}

/// Inverse 2-stage PCT + overlap filter (`invTransformMacroblock_alteredOperators_hard` in `strInvTransform.c`).
#[allow(clippy::too_many_arguments)]
fn inv_transform_macroblock_dec(
    cf: ColorFormat,
    channels: usize,
    overlap: Overlap,
    scaled_arith: bool,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    c_column: usize,
    b_vert_tb: bool,
    b_hori_tb: bool,
    b_one_left_vtb: bool,
    b_one_right_vtb: bool,
    a0: &mut [Vec<i32>; MAX_CHANNELS],
    a1: &mut [Vec<i32>; MAX_CHANNELS],
    pred_before: &mut [[i32; 2]; 2],
    pred_after: &mut [[i32; 2]; 2],
) {
    let left = c_column == 0;
    let right = c_column == cmb_width;
    let top = c_row == 0;
    let bottom = c_row == cmb_height;
    let top_or_bottom = top || bottom;
    let left_or_right = left || right;
    let bottom_or_right = bottom || right;
    let left_adj = c_column == 1;
    let right_adj = c_column + 1 == cmb_width;

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    for i in 0..full_planes {
        let p0 = (c_column * 256) as isize;
        let p1 = (c_column * 256) as isize;
        let buf0 = &mut a0[i];
        let buf1 = &mut a1[i];

        if !bottom_or_right {
            str_idct_4x4_stage2(buf1, p1);
            if scaled_arith {
                str_normalize_dec(buf1, p1, i != 0);
            }
        }

        if overlap == Overlap::Two {
            if (top || b_hori_tb) && (left || b_vert_tb) {
                str_post4_alt(buf1, p1, 0, 64, 16, 80);
            }
            if (top || b_hori_tb) && (right || b_vert_tb) {
                str_post4_alt(buf1, p1, -128, -64, -112, -48);
            }
            if (bottom || b_hori_tb) && (left || b_vert_tb) {
                str_post4_alt(buf0, p0, 32, 96, 48, 112);
            }
            if (bottom || b_hori_tb) && (right || b_vert_tb) {
                str_post4_alt(buf0, p0, -96, -32, -80, -16);
            }
            if (left_or_right || b_vert_tb) && !top_or_bottom && !b_hori_tb {
                if left || b_vert_tb {
                    str_post4_alt_split(buf0, p0, 32, 48, buf1, p1, 0, 16);
                    str_post4_alt_split(buf0, p0, 96, 112, buf1, p1, 64, 80);
                }
                if right || b_vert_tb {
                    str_post4_alt_split(buf0, p0 - 128, 32, 48, buf1, p1 - 128, 0, 16);
                    str_post4_alt_split(buf0, p0 - 128, 96, 112, buf1, p1 - 128, 64, 80);
                }
            }
            if !left_or_right {
                if (top_or_bottom || b_hori_tb) && !b_vert_tb {
                    if top || b_hori_tb {
                        str_post4_alt(buf1, p1, -128, -64, 0, 64);
                        str_post4_alt(buf1, p1, -112, -48, 16, 80);
                    }
                    if bottom || b_hori_tb {
                        str_post4_alt(buf0, p0 + 32, -128, -64, 0, 64);
                        str_post4_alt(buf0, p0 + 32, -112, -48, 16, 80);
                    }
                }
                if !top_or_bottom && !b_hori_tb && !b_vert_tb {
                    str_post_4x4_stage2_split_alt(buf0, p0, buf1, p1);
                }
            }
        }

        if !top {
            let mut j = if left { 32isize } else { -96isize };
            let j_end = if right { 32isize } else { 160isize };
            while j < j_end {
                str_idct_4x4_stage1(buf0, p0 + j);
                str_idct_4x4_stage1(buf0, p0 + j + 16);
                j += 64;
            }
        }

        if !bottom {
            let mut j = if left { 0isize } else { -128isize };
            let j_end = if right { 0isize } else { 128isize };
            while j < j_end {
                str_idct_4x4_stage1(buf1, p1 + j);
                str_idct_4x4_stage1(buf1, p1 + j + 16);
                j += 64;
            }
        }

        if overlap != Overlap::None {
            if left_or_right || b_vert_tb {
                if (top || b_hori_tb) && (left || b_vert_tb) {
                    str_post4_alt(buf1, p1, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -59, -60, -57, -58);
                }
                if (bottom || b_hori_tb) && (left || b_vert_tb) {
                    str_post4_alt(buf0, p0, 58, 59, 56, 57);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if left || b_vert_tb {
                    let j = 10isize;
                    if !top {
                        let base = p0 + 16 + j;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                        str_post4_alt(buf0, base, 16, 14, 22, 24);
                        str_post4_alt(buf0, base, 17, 15, 23, 25);
                    }
                    if !bottom {
                        let base = p1 + j;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                    }
                    if !top_or_bottom && !b_hori_tb {
                        str_post4_alt_split(buf0, p0, 48 + j, 48 + j - 2, buf1, p1, j - 10, j - 8);
                        str_post4_alt_split(
                            buf0,
                            p0,
                            48 + j + 1,
                            48 + j - 1,
                            buf1,
                            p1,
                            j - 9,
                            j - 7,
                        );
                    }
                }
                if right || b_vert_tb {
                    let j = -64isize + 14;
                    if !top {
                        let base = p0 + 16 + j;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                        str_post4_alt(buf0, base, 16, 14, 22, 24);
                        str_post4_alt(buf0, base, 17, 15, 23, 25);
                    }
                    if !bottom {
                        let base = p1 + j;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                    }
                    if !top_or_bottom && !b_hori_tb {
                        str_post4_alt_split(buf0, p0, 48 + j, 48 + j - 2, buf1, p1, j - 10, j - 8);
                        str_post4_alt_split(
                            buf0,
                            p0,
                            48 + j + 1,
                            48 + j - 1,
                            buf1,
                            p1,
                            j - 9,
                            j - 7,
                        );
                    }
                }
            }

            if top || b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post4_alt(buf1, p1 + j, 5, 4, 64, 65);
                        str_post4_alt(buf1, p1 + j, 7, 6, 66, 67);
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                    }
                    j += 64;
                }
            }

            if bottom || b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post_4x4_stage1_alt(buf0, p0 + 16 + j, 0);
                        str_post_4x4_stage1_alt(buf0, p0 + 32 + j, 0);
                        str_post4_alt(buf0, p0 + 48 + j, 15, 14, 74, 75);
                        str_post4_alt(buf0, p0 + 48 + j, 13, 12, 72, 73);
                    }
                    j += 64;
                }
            }

            if !top && !bottom && !b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post_4x4_stage1_alt(buf0, p0 + 16 + j, 0);
                        str_post_4x4_stage1_alt(buf0, p0 + 32 + j, 0);
                        str_post_4x4_stage1_split_alt(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                    }
                    j += 64;
                }
            }
        }
    }

    if cf == ColorFormat::Yuv420 {
        for i in 0..2 {
            let p0 = (c_column * 64) as isize;
            let p1 = (c_column * 64) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if !bottom_or_right {
                if !scaled_arith {
                    dct_2x2_dn(buf1, p1, 0, 32, 16, 48);
                } else {
                    dct_2x2_dn_dec(buf1, p1, 0, 32, 16, 48);
                }
            }

            if overlap == Overlap::Two {
                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -32)] -= pred_before[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_before[i][1] = buf0[idx_at(p0, 16)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if (left_or_right || b_vert_tb) && !top_or_bottom && !b_hori_tb {
                    if left || b_vert_tb {
                        str_post2_alt_split(buf0, p0, 16, buf1, p1, 0);
                    }
                    if right || b_vert_tb {
                        str_post2_alt_split(buf0, p0, -16, buf1, p1, -32);
                    }
                }
                if !left_or_right {
                    if (top_or_bottom || b_hori_tb) && !b_vert_tb {
                        if top || b_hori_tb {
                            str_post2_alt(buf1, p1, -32, 0);
                        }
                        if bottom || b_hori_tb {
                            str_post2_alt(buf0, p0, -16, 16);
                        }
                    } else if !top_or_bottom && !b_hori_tb && !b_vert_tb {
                        str_post2x2_alt_split(buf0, p0, -16, 16, buf1, p1, -32, 0);
                    }
                }

                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] += v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -32)] += pred_after[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] += v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_after[i][1] = buf0[idx_at(p0, 16)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top {
                let mut j = if left {
                    48isize
                } else if left_adj || b_one_right_vtb {
                    -48isize
                } else {
                    -16isize
                };
                let j_end = if right || b_vert_tb { 16isize } else { 48isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf0, p0 + j);
                    j += 32;
                }
            }

            if !bottom {
                let mut j = if left {
                    32isize
                } else if left_adj || b_one_right_vtb {
                    -64isize
                } else {
                    -32isize
                };
                let j_end = if right || b_vert_tb { 0isize } else { 32isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf1, p1 + j);
                    j += 32;
                }
            }

            if overlap != Overlap::None {
                if (top || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf1, p1 - 64, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -27, -28, -25, -26);
                }
                if (bottom || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf0, p0 - 64, 26, 27, 24, 25);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if !left && !top {
                    if left_adj || b_one_right_vtb {
                        if !bottom && !b_hori_tb {
                            str_post4_alt_split(buf0, p0 - 64, 26, 24, buf1, p1 - 64, 0, 2);
                            str_post4_alt_split(buf0, p0 - 64, 27, 25, buf1, p1 - 64, 1, 3);
                        }
                        str_post4_alt(buf0, p0 - 64, 10, 8, 16, 18);
                        str_post4_alt(buf0, p0 - 64, 11, 9, 17, 19);
                    }
                    if bottom || b_hori_tb {
                        str_post4_alt(buf0, p0 - 48, 15, 14, 42, 43);
                        str_post4_alt(buf0, p0 - 48, 13, 12, 40, 41);
                        if !right && !b_vert_tb {
                            str_post4_alt(buf0, p0 - 16, 15, 14, 42, 43);
                            str_post4_alt(buf0, p0 - 16, 13, 12, 40, 41);
                        }
                    } else {
                        str_post_4x4_stage1_split_alt(buf0, p0 - 48, buf1, p1 - 64, 32);
                        if !right && !b_vert_tb {
                            str_post_4x4_stage1_split_alt(buf0, p0 - 16, buf1, p1 - 32, 32);
                        }
                    }

                    if right || b_vert_tb {
                        if !bottom && !b_hori_tb {
                            str_post4_alt_split(buf0, p0, -2, -4, buf1, p1, -28, -26);
                            str_post4_alt_split(buf0, p0, -1, -3, buf1, p1, -27, -25);
                        }
                        str_post4_alt(buf0, p0, -18, -20, -12, -10);
                        str_post4_alt(buf0, p0, -17, -19, -11, -9);
                    } else {
                        str_post_4x4_stage1_alt(buf0, p0 - 32, 32);
                    }
                    str_post_4x4_stage1_alt(buf0, p0 - 64, 32);
                }

                if top || b_hori_tb {
                    if !left {
                        let base = p1 - 64 + 4;
                        str_post4_alt(buf1, base, 1, 0, 28, 29);
                        str_post4_alt(buf1, base, 3, 2, 30, 31);
                    }
                    if !left && !right && !b_vert_tb {
                        let base = p1 - 32 + 4;
                        str_post4_alt(buf1, base, 1, 0, 28, 29);
                        str_post4_alt(buf1, base, 3, 2, 30, 31);
                    }
                }
            }
        }
    }

    if cf == ColorFormat::Yuv422 {
        for i in 0..2 {
            let p0 = (c_column * 128) as isize;
            let p1 = (c_column * 128) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if !bottom_or_right {
                let i0 = idx_at(p1, 0);
                let i32 = idx_at(p1, 32);
                buf1[i0] -= (buf1[i32] + 1) >> 1;
                buf1[i32] += buf1[i0];

                if !scaled_arith {
                    dct_2x2_dn(buf1, p1, 0, 64, 16, 80);
                    dct_2x2_dn(buf1, p1, 32, 96, 48, 112);
                } else {
                    dct_2x2_dn_dec(buf1, p1, 0, 64, 16, 80);
                    dct_2x2_dn_dec(buf1, p1, 32, 96, 48, 112);
                }
            }

            if overlap == Overlap::Two {
                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -64)] -= pred_before[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_before[i][1] = buf0[idx_at(p0, 48)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if !bottom {
                    if left_or_right || b_vert_tb {
                        if !top && !b_hori_tb {
                            if left || b_vert_tb {
                                str_post2_alt_split(buf0, p0, 48, buf1, p1, 0);
                            }
                            if right || b_vert_tb {
                                str_post2_alt_split(buf0, p0, -16, buf1, p1, -64);
                            }
                        }
                        if left || b_vert_tb {
                            str_post2_alt(buf1, p1, 16, 32);
                        }
                        if right || b_vert_tb {
                            str_post2_alt(buf1, p1, -48, -32);
                        }
                    }
                    if !left_or_right && !b_vert_tb {
                        if top || b_hori_tb {
                            str_post2_alt(buf1, p1, -64, 0);
                        } else {
                            str_post2x2_alt_split(buf0, p0, -16, 48, buf1, p1, -64, 0);
                        }
                        str_post2x2_alt(buf1, p1, -48, 16, -32, 32);
                    }
                }
                if (bottom || b_hori_tb) && !left_or_right && !b_vert_tb {
                    str_post2_alt(buf0, p0, -16, 48);
                }

                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] += v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -64)] += pred_after[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] += v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_after[i][1] = buf0[idx_at(p0, 48)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top {
                let mut j = if left {
                    112isize
                } else if left_adj || b_one_right_vtb {
                    -80isize
                } else {
                    -16isize
                };
                let j_end = if right || b_vert_tb {
                    48isize
                } else {
                    112isize
                };
                while j < j_end {
                    str_idct_4x4_stage1(buf0, p0 + j);
                    j += 64;
                }
            }

            if !bottom {
                let mut j = if left {
                    64isize
                } else if left_adj || b_one_right_vtb {
                    -128isize
                } else {
                    -64isize
                };
                let j_end = if right || b_vert_tb { 0isize } else { 64isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf1, p1 + j);
                    str_idct_4x4_stage1(buf1, p1 + j + 16);
                    str_idct_4x4_stage1(buf1, p1 + j + 32);
                    j += 64;
                }
            }

            if overlap != Overlap::None {
                if (top || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf1, p1 - 128, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -59, -60, -57, -58);
                }
                if (bottom || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf0, p0 - 128, 58, 59, 56, 57);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if !top {
                    if left_adj || b_one_right_vtb {
                        let base = p0 + 42 - 128;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                    }
                    if right || b_vert_tb {
                        let base = p0 - 18;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_alt(buf0, p0 + j + 32, 0);
                        j += 64;
                    }
                }

                if !bottom {
                    if left_adj || b_one_right_vtb {
                        let base = p1 + 10 - 128;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                        str_post4_alt(buf1, base + 16, 0, -2, 6, 8);
                        str_post4_alt(buf1, base + 16, 1, -1, 7, 9);
                    }
                    if right || b_vert_tb {
                        let base = p1 - 50;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                        str_post4_alt(buf1, base + 16, 0, -2, 6, 8);
                        str_post4_alt(buf1, base + 16, 1, -1, 7, 9);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                        str_post_4x4_stage1_alt(buf1, p1 + j + 16, 0);
                        j += 64;
                    }
                }

                if top_or_bottom || b_hori_tb {
                    if top || b_hori_tb {
                        let mut j = if left { 0isize } else { -128isize };
                        let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                        while j < j_end {
                            let base = p1 + 5 + j;
                            str_post4_alt(buf1, base, 0, -1, 59, 60);
                            str_post4_alt(buf1, base, 2, 1, 61, 62);
                            j += 64;
                        }
                    }
                    if bottom || b_hori_tb {
                        let mut j = if left { 0isize } else { -128isize };
                        let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                        while j < j_end {
                            let base = p0 + 61 + j;
                            str_post4_alt(buf0, base, 0, -1, 59, 60);
                            str_post4_alt(buf0, base, 2, 1, 61, 62);
                            j += 64;
                        }
                    }
                } else {
                    if left_adj || b_one_right_vtb {
                        let j = -128isize;
                        str_post4_alt_split(buf0, p0 + j, 58, 56, buf1, p1 + j, 0, 2);
                        str_post4_alt_split(buf0, p0 + j, 59, 57, buf1, p1 + j, 1, 3);
                    }
                    if right || b_vert_tb {
                        let j = -60isize;
                        str_post4_alt_split(buf0, p0 + j, 58, 56, buf1, p1 + j, 0, 2);
                        str_post4_alt_split(buf0, p0 + j, 59, 57, buf1, p1 + j, 1, 3);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_split_alt(buf0, p0 + j + 48, buf1, p1 + j, 0);
                        j += 64;
                    }
                }
            }
        }
    }
}

// ============================================================================
// Frequency-Domain Prediction, Quantization & CBP Modeling
// ============================================================================

fn get_ac_pred_mode(mb: &MbInfo, cf: ColorFormat) -> i32 {
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

fn get_dcac_pred_mode(
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

fn update_pred_info(
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

fn quantize_macroblock(
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

fn pred_macroblock_enc(
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

fn pred_dcac_dec(
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

fn dequantize_macroblock(
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

fn pred_ac_dec(
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
fn num_ones_u16(v: i32) -> i32 {
    ((v as u32) & 0xFFFF).count_ones() as i32
}

#[inline]
fn saturate32(v: i32) -> i32 {
    v.clamp(-16, 15)
}

fn update_cbp_model_state(model: &mut CbpModel, c1: usize, n_orig: i32) {
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
fn pred_cbp_enc(
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
fn pred_cbp_dec(
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

// ============================================================================
// ITU-T Rec. T.832 Macroblock Entropy Encoding & Decoding (segenc.c / segdec.c)
// ============================================================================

fn encode_significant_abs_level(abs_level_in: u32, ah: &mut AdaptiveHuffman, bw: &mut BitWriter) {
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

fn decode_significant_abs_level(
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

fn encode_significant_run(run: i32, max_run: i32, ah0: &AdaptiveHuffman, bw: &mut BitWriter) {
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

fn decode_significant_run(
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

fn encode_block(
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

fn decode_index_at_loc(
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

fn decode_block_lp(
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

fn decode_block_highpass(
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

fn adaptive_scan(
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

fn encode_macroblock_dc(
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

fn decode_qp_index(br: &mut BitReader<'_>, bits: u8) -> Result<u8, BitmapError> {
    if !br.get_bool()? {
        Ok(0)
    } else {
        Ok((br.get_bits(bits)? as u8) + 1)
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_macroblock_dc(
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

fn encode_macroblock_lowpass(
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
fn decode_macroblock_lowpass(
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

fn code_cbp_enc(cf: ColorFormat, mb: &MbInfo, ctx: &mut CodingContext, bw: &mut BitWriter) {
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

fn decode_cbp(
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
fn encode_macroblock_highpass(
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
fn decode_block_adaptive<'a>(
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
fn decode_macroblock_highpass<'a>(
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

// ============================================================================
// Reversible YCoCg Color Conversion & Chroma Downsampling / Interpolation
// ============================================================================

#[inline]
fn df_odd(d0: i32, d1: i32, d2: i32, d3: i32, d4: i32) -> i32 {
    (((d1 + d2 + d3) << 2) + (d2 << 1) + d0 + d4 + 8) >> 4
}

fn downsample_uv_row(
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

fn interpolate_uv_row(
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

// ============================================================================
// Codestream Header Writing & Spatial Single-Tile Encoder Pipeline
// ============================================================================

struct PlaneEncState {
    cf: ColorFormat,
    channels: usize,
    scaled_arith: bool,
    overlap: Overlap,
    tile: TileQuantizers,
    ctx: CodingContext,
    mb: MbInfo,
    a0: [Vec<i32>; MAX_CHANNELS],
    a1: [Vec<i32>; MAX_CHANNELS],
    res_u: Vec<i32>,
    res_v: Vec<i32>,
    pred_cur: [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: [Vec<PredInfo>; MAX_CHANNELS],
    pred_before: [[i32; 2]; 2],
    pred_after: [[i32; 2]; 2],
}

impl PlaneEncState {
    fn new(
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

    fn advance_one_mb_row(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.pred_cur[ch], &mut self.pred_prev[ch]);
        }
    }

    fn swap_mr_buffers(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.a0[ch], &mut self.a1[ch]);
        }
    }

    fn transform_and_encode_at(
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

fn write_quantizer_header(bw: &mut BitWriter, indices: &[u8; MAX_CHANNELS], channels: usize) {
    if channels > 1 {
        bw.put_bits(2, 2); // INDEPENDENT channel mode
        for &qp in &indices[..channels] {
            bw.put_bits(u32::from(qp), 8);
        }
    } else {
        bw.put_bits(u32::from(indices[0]), 8);
    }
}

fn write_image_plane_header(
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
fn write_wmi_image_header(
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

fn write_null_index_table_and_spatial_packet_header(bw: &mut BitWriter, trim_flex_bits: u8) {
    bw.fill_to_byte();
    // PROFILE_LEVEL_INFO (VLW_ESC 4 bytes: profile_idc = 111, level_idc = 255, LAST_FLAG = 1)
    bw.write_bytes_aligned(&[0x00, 0x04, 111, 255, 0x00, 0x01]);
    // TILE_SPATIAL packet startcode: 0x00000100 (tile 0, packet type 0 = spatial)
    bw.write_bytes_aligned(&[0x00, 0x00, 0x01, 0x00]);
    if trim_flex_bits > 0 {
        bw.put_bits(u32::from(trim_flex_bits), 4);
    }
}

fn input_mb_row_rgb_and_alpha(
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

fn encode_primary_codestream(
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

fn encode_planar_alpha_codestream(
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

// ============================================================================
// ITU-T Rec. T.832 Codestream Header Parsing & Spatial+Frequency Decoder
// ============================================================================

#[derive(Clone, Debug)]
struct ParsedPlaneHeader {
    cf: ColorFormat,
    scaled_arith: bool,
    subband: Subband,
    qp_mode: u32,
    qp_dc: [u8; MAX_CHANNELS],
    qp_lp: [u8; MAX_CHANNELS],
    qp_hp: [u8; MAX_CHANNELS],
}

#[derive(Clone, Debug)]
struct ParsedImageHeader {
    width: u32,
    height: u32,
    extra_top: usize,
    extra_left: usize,
    extra_bottom: usize,
    extra_right: usize,
    hard_tiles: bool,
    bitstream_fmt: BitstreamFormat,
    orientation: u8,
    index_table_present: bool,
    overlap: Overlap,
    trim_flexbits_flag: bool,
    alpha_channel: bool,
    output_clr_fmt: u8,
    tile_x: Vec<usize>,
    tile_y: Vec<usize>,
}

fn read_quantizer_header(
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

fn read_image_plane_header(br: &mut BitReader<'_>) -> Result<ParsedPlaneHeader, BitmapError> {
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

fn read_wmi_image_header(br: &mut BitReader<'_>) -> Result<ParsedImageHeader, BitmapError> {
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

enum TileBandReaders<'a> {
    Spatial(BitReader<'a>),
    Frequency {
        dc: BitReader<'a>,
        lp: Option<BitReader<'a>>,
        ac: Option<BitReader<'a>>,
        fl: Option<BitReader<'a>>,
    },
}

struct PlaneDecState {
    hdr: ParsedPlaneHeader,
    channels: usize,
    tiles: Vec<TileQuantizers>,
    contexts: Vec<CodingContext>,
    mb: MbInfo,
    a0: [Vec<i32>; MAX_CHANNELS],
    a1: [Vec<i32>; MAX_CHANNELS],
    res_u: Vec<i32>,
    res_v: Vec<i32>,
    pred_cur: [Vec<PredInfo>; MAX_CHANNELS],
    pred_prev: [Vec<PredInfo>; MAX_CHANNELS],
    pred_before: [[i32; 2]; 2],
    pred_after: [[i32; 2]; 2],
    tile_col: usize,
    tile_row: usize,
    trfm_mb_y: usize,
    trfm_tile_x: usize,
    trfm_tile_y: usize,
    b_vert_tb: bool,
    b_hori_tb: bool,
    b_one_left_vtb: bool,
    b_one_right_vtb: bool,
}

impl PlaneDecState {
    fn new(hdr: ParsedPlaneHeader, cmb_width: usize, num_tiles_v: usize) -> Self {
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

    fn zero_a1(&mut self) {
        for ch in 0..self.channels {
            self.a1[ch].fill(0);
        }
    }

    fn advance_one_mb_row(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.pred_cur[ch], &mut self.pred_prev[ch]);
        }
    }

    fn swap_mr_buffers(&mut self) {
        for ch in 0..self.channels {
            std::mem::swap(&mut self.a0[ch], &mut self.a1[ch]);
        }
    }

    fn read_tile_header_dc(
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

    fn read_tile_header_lp(
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

    fn read_tile_header_hp(
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

    fn update_hard_tile_flags(&mut self, img: &ParsedImageHeader, c_row: usize, c_col: usize) {
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

    fn decode_mb_entropy(
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

fn read_packet_header(br: &mut BitReader<'_>) -> Result<(), BitmapError> {
    if br.get_bits(8)? != 0 || br.get_bits(8)? != 0 || br.get_bits(8)? != 1 {
        return Err(error(
            "invalid JPEG-XR packet startcode (expected 0x000001)",
        ));
    }
    let _ = br.get_bits(8)?;
    Ok(())
}

struct DecodedCodestream {
    width: u32,
    height: u32,
    has_interleaved_alpha: bool,
    rgba: Vec<u8>,
    y_only_samples: Option<Vec<u8>>,
}

fn decode_codestream(stream: &[u8]) -> Result<DecodedCodestream, BitmapError> {
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
fn apply_orientation(x: usize, y: usize, w: usize, h: usize, orientation: u8) -> (usize, usize) {
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

// ============================================================================
// Public ITU-T Rec. T.832 `.jxr` Container Encoder & Decoder
// ============================================================================

/// Encodes big-endian straight `ARGB8` pixels (`[A, R, G, B]` per pixel) into a spec-conformant
/// ITU-T Rec. T.832 / ISO/IEC 29199-2 `.jxr` container.
///
/// Uses `GUID_PKPixelFormat24bppBGR` when `transparent == false` and `GUID_PKPixelFormat32bppBGRA`
/// with a planar `ALPHA_OFFSET` (`0xBCC2`) / `ALPHA_BYTE_COUNT` (`0xBCC3`) codestream when
/// `transparent == true` (matching `jxrlib` / Flash `BitmapData.encode` baseline conventions).
pub fn encode_jxr_argb(
    width: u32,
    height: u32,
    transparent: bool,
    argb_be: &[u8],
    quantization: u8,
    color_space: &str,
    trim_flex_bits: u8,
) -> Result<Vec<u8>, BitmapError> {
    encode_jxr_with_alpha_mode(
        width,
        height,
        if transparent {
            JxrAlphaMode::Planar
        } else {
            JxrAlphaMode::None
        },
        argb_be,
        quantization,
        color_space,
        trim_flex_bits,
    )
}

/// Encodes big-endian straight `ARGB8` pixels into a `.jxr` container using the requested alpha layout
/// (`None` -> `24bppBGR`, `Planar` -> `32bppBGRA` with `ALPHA_OFFSET` / `ALPHA_BYTE_COUNT`,
/// `Interleaved` -> `32bppBGRA` with `ALPHA_IMAGE_PLANE_FLAG = 1`).
pub fn encode_jxr_with_alpha_mode(
    width: u32,
    height: u32,
    alpha_mode: JxrAlphaMode,
    argb_be: &[u8],
    quantization: u8,
    color_space: &str,
    trim_flex_bits: u8,
) -> Result<Vec<u8>, BitmapError> {
    if width == 0 || height == 0 {
        return Err(error(
            "JPEG-XR encoding requires a nonempty clipped rectangle",
        ));
    }
    if width > MAX_BITMAP_DIMENSION || height > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR dimensions must be in 1..=4096"));
    }
    if trim_flex_bits > 15 {
        return Err(error("JPEGXREncoderOptions.trimFlexBits requires 0..=15"));
    }
    let subsampling = ChromaSubsampling::from_color_space(color_space)?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| error("JPEG-XR pixel buffer size overflow"))?;
    if argb_be.len() != expected {
        return Err(error("JPEG-XR ARGB input buffer length mismatch"));
    }

    let has_alpha = alpha_mode != JxrAlphaMode::None;
    let interleaved = alpha_mode == JxrAlphaMode::Interleaved;
    let primary_stream = encode_primary_codestream(
        width,
        height,
        argb_be,
        subsampling,
        quantization,
        trim_flex_bits,
        interleaved,
    );
    let planar_alpha_stream = if alpha_mode == JxrAlphaMode::Planar {
        Some(encode_planar_alpha_codestream(
            width,
            height,
            argb_be,
            quantization,
            trim_flex_bits,
        ))
    } else {
        None
    };

    let has_planar_tags = planar_alpha_stream.is_some();
    let num_entries: u16 = if has_planar_tags { 10 } else { 8 };
    let ifd_offset: u32 = 8;
    let ifd_bytes = 2 + usize::from(num_entries) * 12 + 4;
    let guid_offset = (ifd_offset as usize + ifd_bytes) as u32;
    let image_offset = guid_offset + 16;
    let image_byte_count = primary_stream.len() as u32;
    let (alpha_offset, alpha_byte_count) = match &planar_alpha_stream {
        Some(alpha_bytes) => (image_offset + image_byte_count, alpha_bytes.len() as u32),
        None => (0u32, 0u32),
    };

    let total_len = image_offset as usize
        + primary_stream.len()
        + planar_alpha_stream.as_ref().map_or(0, Vec::len);
    let mut out = Vec::with_capacity(total_len);

    // 8-byte little-endian ITU-T T.832 Annex A container header
    out.extend_from_slice(&JXR_MAGIC);
    out.extend_from_slice(&ifd_offset.to_le_bytes());

    // IFD entry count + sorted 12-byte entries
    out.extend_from_slice(&num_entries.to_le_bytes());
    let mut write_ifd_entry = |tag: u16, field_type: u16, count: u32, val_or_off: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&val_or_off.to_le_bytes());
    };

    write_ifd_entry(TAG_PIXEL_FORMAT, TIFF_TYPE_BYTE, 16, guid_offset);
    write_ifd_entry(TAG_TRANSFORMATION, TIFF_TYPE_LONG, 1, 0);
    write_ifd_entry(TAG_IMAGE_WIDTH, TIFF_TYPE_LONG, 1, width);
    write_ifd_entry(TAG_IMAGE_HEIGHT, TIFF_TYPE_LONG, 1, height);
    write_ifd_entry(TAG_WIDTH_RESOLUTION, TIFF_TYPE_FLOAT, 1, 96.0_f32.to_bits());
    write_ifd_entry(
        TAG_HEIGHT_RESOLUTION,
        TIFF_TYPE_FLOAT,
        1,
        96.0_f32.to_bits(),
    );
    write_ifd_entry(TAG_IMAGE_OFFSET, TIFF_TYPE_LONG, 1, image_offset);
    write_ifd_entry(TAG_IMAGE_BYTE_COUNT, TIFF_TYPE_LONG, 1, image_byte_count);
    if has_planar_tags {
        write_ifd_entry(TAG_ALPHA_OFFSET, TIFF_TYPE_LONG, 1, alpha_offset);
        write_ifd_entry(TAG_ALPHA_BYTE_COUNT, TIFF_TYPE_LONG, 1, alpha_byte_count);
    }

    // Next IFD offset = 0
    out.extend_from_slice(&0u32.to_le_bytes());

    // 16-byte WIC PIXEL_FORMAT GUID
    let guid = if has_alpha {
        &GUID_PIXEL_FORMAT_32BPP_BGRA
    } else {
        &GUID_PIXEL_FORMAT_24BPP_BGR
    };
    out.extend_from_slice(guid);

    // Primary + optional planar alpha codestreams
    out.extend_from_slice(&primary_stream);
    if let Some(alpha_bytes) = &planar_alpha_stream {
        out.extend_from_slice(alpha_bytes);
    }

    Ok(out)
}

#[derive(Debug)]
struct ParsedJxrContainer {
    width: u32,
    height: u32,
    has_alpha_guid: bool,
    is_premultiplied: bool,
    image_offset: usize,
    image_end: usize,
    alpha_offset: usize,
    alpha_end: usize,
}

fn parse_jxr_container(bytes: &[u8]) -> Result<ParsedJxrContainer, BitmapError> {
    if bytes.len() < 16 || bytes[..4] != JXR_MAGIC {
        return Err(error(
            "not a valid little-endian ITU-T T.832 .jxr container",
        ));
    }
    let ifd_offset = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if ifd_offset < 8 || ifd_offset + 2 > bytes.len() {
        return Err(error("JPEG-XR IFD offset out of bounds"));
    }
    let num_entries = u16::from_le_bytes([bytes[ifd_offset], bytes[ifd_offset + 1]]) as usize;
    let entries_end = ifd_offset
        .checked_add(2)
        .and_then(|v| v.checked_add(num_entries.checked_mul(12)?))
        .and_then(|v| v.checked_add(4))
        .ok_or_else(|| error("JPEG-XR IFD table overflow"))?;
    if entries_end > bytes.len() {
        return Err(error("truncated JPEG-XR IFD table"));
    }

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut has_alpha_guid = false;
    let mut is_premultiplied = false;
    let mut saw_pixel_format = false;
    let mut image_offset: Option<usize> = None;
    let mut image_byte_count: Option<usize> = None;
    let mut alpha_offset: usize = 0;
    let mut alpha_byte_count: usize = 0;

    let read_u32_tag = |ftype: u16, count: u32, raw_val: [u8; 4]| -> Result<u32, BitmapError> {
        if count != 1 {
            return Err(error("JPEG-XR scalar IFD tag requires count == 1"));
        }
        match ftype {
            TIFF_TYPE_BYTE => Ok(u32::from(raw_val[0])),
            TIFF_TYPE_SHORT => Ok(u32::from(u16::from_le_bytes([raw_val[0], raw_val[1]]))),
            TIFF_TYPE_LONG => Ok(u32::from_le_bytes(raw_val)),
            _ => Err(error("unsupported TIFF field type for JPEG-XR integer tag")),
        }
    };

    for i in 0..num_entries {
        let pos = ifd_offset + 2 + i * 12;
        let tag = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        let ftype = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
        let count = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]);
        let raw_val = [
            bytes[pos + 8],
            bytes[pos + 9],
            bytes[pos + 10],
            bytes[pos + 11],
        ];
        let val_u32 = u32::from_le_bytes(raw_val);

        match tag {
            TAG_PIXEL_FORMAT => {
                if ftype != TIFF_TYPE_BYTE || count != 16 {
                    return Err(error("JPEG-XR PIXEL_FORMAT tag requires 16 BYTEs"));
                }
                let goff = val_u32 as usize;
                if goff + 16 > bytes.len() {
                    return Err(error("JPEG-XR PIXEL_FORMAT GUID offset out of bounds"));
                }
                let guid = &bytes[goff..goff + 16];
                if guid == GUID_PIXEL_FORMAT_32BPP_RGB {
                    has_alpha_guid = false;
                } else if guid == GUID_PIXEL_FORMAT_32BPP_RGBA {
                    has_alpha_guid = true;
                } else if guid[..15] == GUID_PREFIX_WIC {
                    match guid[15] {
                        0x08 | 0x0C | 0x0D | 0x0E => {
                            // 8bppGray, 24bppBGR, 24bppRGB, 32bppBGR
                            has_alpha_guid = false;
                        }
                        0x0F | 0x1E => {
                            // 32bppBGRA, 32bppRGBA (legacy subtype)
                            has_alpha_guid = true;
                        }
                        0x10 | 0x1F => {
                            // 32bppPBGRA, 32bppPRGBA
                            has_alpha_guid = true;
                            is_premultiplied = true;
                        }
                        _ => return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID subtype")),
                    }
                } else {
                    return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID"));
                }
                saw_pixel_format = true;
            }
            TAG_IMAGE_WIDTH => width = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_HEIGHT => height = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_OFFSET | TAG_IMAGE_OFFSET_ALT => {
                image_offset = Some(read_u32_tag(ftype, count, raw_val)? as usize);
            }
            TAG_IMAGE_BYTE_COUNT | TAG_IMAGE_BYTE_COUNT_ALT => {
                image_byte_count = Some(read_u32_tag(ftype, count, raw_val)? as usize);
            }
            TAG_ALPHA_OFFSET | TAG_ALPHA_OFFSET_ALT => {
                alpha_offset = read_u32_tag(ftype, count, raw_val)? as usize;
            }
            TAG_ALPHA_BYTE_COUNT | TAG_ALPHA_BYTE_COUNT_ALT => {
                alpha_byte_count = read_u32_tag(ftype, count, raw_val)? as usize;
            }
            _ => {}
        }
    }

    if !saw_pixel_format {
        return Err(error("missing JPEG-XR PIXEL_FORMAT IFD tag (0xBC01)"));
    }
    let image_offset =
        image_offset.ok_or_else(|| error("missing JPEG-XR IMAGE_OFFSET (0xBCC0)"))?;
    let image_byte_count =
        image_byte_count.ok_or_else(|| error("missing JPEG-XR IMAGE_BYTE_COUNT (0xBCC1)"))?;
    if image_offset >= bytes.len() {
        return Err(error("JPEG-XR IMAGE_OFFSET out of bounds"));
    }
    // Note: jxrlib's ReadContainer permits byte_count == 0 (unbounded to EOF) or clamped to EOF.
    let image_end = if image_byte_count == 0 {
        if alpha_offset > image_offset && alpha_offset <= bytes.len() {
            alpha_offset
        } else {
            bytes.len()
        }
    } else {
        let end = image_offset
            .checked_add(image_byte_count)
            .ok_or_else(|| error("JPEG-XR IMAGE_OFFSET overflow"))?;
        if end > bytes.len() {
            return Err(error("JPEG-XR primary codestream range out of bounds"));
        }
        end
    };
    if image_end.saturating_sub(image_offset) < 16 {
        return Err(error("JPEG-XR primary codestream too short"));
    }

    let alpha_end = if alpha_offset > 0 {
        if alpha_offset >= bytes.len() || alpha_byte_count < 16 {
            return Err(error("JPEG-XR planar alpha codestream range out of bounds"));
        }
        // Note: jxrlib's JXRGlueJxr.c WriteContainerPost writes deAlphaByteCount = nCbAlpha + nOffAlpha;
        // clamp to bytes.len() when alpha_offset + alpha_byte_count overshoots EOF.
        let end = bytes
            .len()
            .min(alpha_offset.saturating_add(alpha_byte_count));
        if end.saturating_sub(alpha_offset) < 16 {
            return Err(error("JPEG-XR planar alpha codestream too short"));
        }
        end
    } else {
        0
    };

    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            let mut br = BitReader::new(&bytes[image_offset..image_end]);
            let hdr = read_wmi_image_header(&mut br)?;
            if hdr.orientation >= 4 {
                (hdr.height, hdr.width)
            } else {
                (hdr.width, hdr.height)
            }
        }
    };
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR image dimensions must be in 1..=4096"));
    }

    Ok(ParsedJxrContainer {
        width: w,
        height: h,
        has_alpha_guid,
        is_premultiplied,
        image_offset,
        image_end,
        alpha_offset,
        alpha_end,
    })
}

/// Reads `(width, height)` from an ITU-T T.832 `.jxr` container without decoding macroblocks.
pub fn jxr_dimensions(bytes: &[u8]) -> Result<(u32, u32), BitmapError> {
    let parsed = parse_jxr_container(bytes)?;
    Ok((parsed.width, parsed.height))
}

/// Decodes an ITU-T T.832 `.jxr` container into straight `RGBA8` (`DecodedImage`).
pub fn decode_jxr_to_rgba(bytes: &[u8]) -> Result<DecodedImage, BitmapError> {
    let container = parse_jxr_container(bytes)?;
    let primary_slice = &bytes[container.image_offset..container.image_end];
    let mut decoded = decode_codestream(primary_slice)?;
    if let Some(y_samples) = decoded.y_only_samples.take() {
        decoded.rgba = Vec::with_capacity(y_samples.len() * 4);
        for y in y_samples {
            decoded.rgba.extend_from_slice(&[y, y, y, 255]);
        }
    }
    if decoded.width != container.width || decoded.height != container.height {
        return Err(error(
            "JPEG-XR IFD dimensions do not match IMAGE_HEADER dimensions",
        ));
    }

    if !container.has_alpha_guid {
        for px in decoded.rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    } else if !decoded.has_interleaved_alpha && container.alpha_offset > 0 {
        let alpha_slice = &bytes[container.alpha_offset..container.alpha_end];
        let alpha_dec = decode_codestream(alpha_slice)?;
        let Some(alpha_samples) = alpha_dec.y_only_samples else {
            return Err(error(
                "JPEG-XR planar alpha stream did not decode as Y_ONLY",
            ));
        };
        if alpha_dec.width != decoded.width
            || alpha_dec.height != decoded.height
            || alpha_samples.len() * 4 != decoded.rgba.len()
        {
            return Err(error("JPEG-XR planar alpha dimensions mismatch"));
        }
        for (px, &a) in decoded
            .rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&alpha_samples)
        {
            px[3] = a;
        }
    }

    if container.is_premultiplied {
        for px in decoded.rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            if a == 0 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            } else if a < 255 {
                px[0] = ((u32::from(px[0]) * 255 + a / 2) / a).min(255) as u8;
                px[1] = ((u32::from(px[1]) * 255 + a / 2) / a).min(255) as u8;
                px[2] = ((u32::from(px[2]) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }

    Ok(DecodedImage {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argb_to_rgba(argb: &[u8], transparent: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(argb.len());
        for px in argb.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[1], px[2], px[3], if transparent { px[0] } else { 255 }]);
        }
        out
    }

    fn psnr_rgb(expected_rgba: &[u8], actual_rgba: &[u8]) -> f64 {
        assert_eq!(expected_rgba.len(), actual_rgba.len());
        let mut mse_sum = 0.0_f64;
        let mut count = 0usize;
        for (e, a) in expected_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(actual_rgba.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                let d = f64::from(e[c]) - f64::from(a[c]);
                mse_sum += d * d;
                count += 1;
            }
        }
        let mse = mse_sum / (count as f64);
        if mse == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0 * 255.0 / mse).log10()
        }
    }

    #[test]
    fn lossless_444_roundtrips_odd_dimension_rgb_and_rgba_bit_exact() {
        let (w, h) = (19u32, 13u32);
        let mut argb = Vec::with_capacity((w * h * 4) as usize);
        let mut state = 0x9E37_79B9_u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let a = ((state >> 24) as u8).wrapping_add((x * 13 + y * 7) as u8);
                let r = ((state >> 16) as u8).wrapping_add((x * 29) as u8);
                let g = ((state >> 8) as u8).wrapping_add((y * 37) as u8);
                let b = (state as u8).wrapping_add(((x + y) * 19) as u8);
                argb.extend_from_slice(&[a, r, g, b]);
            }
        }

        // 1) Opaque 24bppBGR lossless 19x13
        let jxr_rgb = encode_jxr_argb(w, h, false, &argb, 0, "4:4:4", 0).unwrap();
        assert_eq!(&jxr_rgb[..4], &[0x49, 0x49, 0xBC, 0x01]);
        assert!(
            jxr_rgb.windows(8).any(|win| win == b"WMPHOTO\0"),
            "must contain WMPHOTO\\0 IMAGE_HEADER"
        );
        assert!(
            jxr_rgb
                .windows(16)
                .any(|win| win == GUID_PIXEL_FORMAT_24BPP_BGR),
            "opaque .jxr must emit 24bppBGR GUID"
        );
        assert_eq!(jxr_dimensions(&jxr_rgb).unwrap(), (w, h));
        let dec_rgb = decode_jxr_to_rgba(&jxr_rgb).unwrap();
        assert_eq!((dec_rgb.width, dec_rgb.height), (w, h));
        assert_eq!(dec_rgb.rgba.as_ref(), argb_to_rgba(&argb, false).as_slice());

        // 2) Transparent 32bppBGRA planar alpha (ALPHA_OFFSET 0xBCC2 / ALPHA_BYTE_COUNT 0xBCC3) lossless 19x13
        let jxr_rgba = encode_jxr_argb(w, h, true, &argb, 0, "auto", 0).unwrap();
        assert!(
            jxr_rgba
                .windows(16)
                .any(|win| win == GUID_PIXEL_FORMAT_32BPP_BGRA),
            "transparent .jxr must emit 32bppBGRA GUID"
        );
        let parsed = parse_jxr_container(&jxr_rgba).unwrap();
        assert!(parsed.alpha_offset > parsed.image_offset);
        assert!(parsed.alpha_end > parsed.alpha_offset + 16);
        let dec_rgba = decode_jxr_to_rgba(&jxr_rgba).unwrap();
        assert_eq!(dec_rgba.rgba.as_ref(), argb_to_rgba(&argb, true).as_slice());

        // 3) Transparent 32bppBGRA interleaved alpha (ALPHA_IMAGE_PLANE_FLAG = 1) lossless 19x13
        let jxr_interleaved =
            encode_jxr_with_alpha_mode(w, h, JxrAlphaMode::Interleaved, &argb, 0, "4:4:4", 0)
                .unwrap();
        let dec_interleaved = decode_jxr_to_rgba(&jxr_interleaved).unwrap();
        assert_eq!(
            dec_interleaved.rgba.as_ref(),
            argb_to_rgba(&argb, true).as_slice()
        );
    }

    #[test]
    fn subsampling_quantization_and_trim_flexbits_produce_high_psnr_and_monotonic_sizes() {
        let (w, h) = (48u32, 32u32);
        let mut grad_argb = Vec::with_capacity((w * h * 4) as usize);
        let mut rich_argb = Vec::with_capacity((w * h * 4) as usize);
        let mut rng = 0x1234_5678_u32;
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 255) / (w - 1)) as u8;
                let g = ((y * 255) / (h - 1)) as u8;
                let b = (((x + y) * 255) / (w + h - 2)) as u8;
                let a = (128 + ((x * 127) / (w - 1))) as u8;
                grad_argb.extend_from_slice(&[a, r, g, b]);

                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let jitter_r = ((rng >> 24) & 0x1F) as u8;
                let jitter_g = ((rng >> 16) & 0x1F) as u8;
                let jitter_b = ((rng >> 8) & 0x1F) as u8;
                rich_argb.extend_from_slice(&[
                    255,
                    r.saturating_add(jitter_r),
                    g.saturating_add(jitter_g),
                    b.saturating_add(jitter_b),
                ]);
            }
        }
        let expected_grad_rgba = argb_to_rgba(&grad_argb, true);

        // Smooth gradient PSNR across color spaces & quantizations
        for cs in ["4:4:4", "4:2:2", "4:2:0"] {
            let s20 = encode_jxr_argb(w, h, true, &grad_argb, 20, cs, 0).unwrap();
            let d20 = decode_jxr_to_rgba(&s20).unwrap();
            let psnr20 = psnr_rgb(&expected_grad_rgba, &d20.rgba);
            assert!(
                psnr20 > 36.0,
                "expected high PSNR (>36 dB) at q=20 for {cs}, got {psnr20:.2} dB"
            );
        }

        // Monotonic byte counts across quantization: q=0 > q=20 > q=60 > q=90
        let b_q0 = encode_jxr_argb(w, h, false, &rich_argb, 0, "4:4:4", 0).unwrap();
        let b_q20 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 0).unwrap();
        let b_q60 = encode_jxr_argb(w, h, false, &rich_argb, 60, "4:4:4", 0).unwrap();
        let b_q90 = encode_jxr_argb(w, h, false, &rich_argb, 90, "4:4:4", 0).unwrap();
        assert!(
            b_q0.len() > b_q20.len() && b_q20.len() > b_q60.len() && b_q60.len() > b_q90.len(),
            "expected monotonic quantization sizes: q0={} > q20={} > q60={} > q90={}",
            b_q0.len(),
            b_q20.len(),
            b_q60.len(),
            b_q90.len()
        );

        // Monotonic byte counts across chroma subsampling: 4:4:4 > 4:2:2 > 4:2:0
        let b_444 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:4:4", 0).unwrap();
        let b_422 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:2:2", 0).unwrap();
        let b_420 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:2:0", 0).unwrap();
        assert!(
            b_444.len() > b_422.len() && b_422.len() > b_420.len(),
            "expected monotonic subsampling sizes: 444={} > 422={} > 420={}",
            b_444.len(),
            b_422.len(),
            b_420.len()
        );

        // trimFlexBits = 4 produces strictly smaller stream than trimFlexBits = 0
        let b_trim0 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 0).unwrap();
        let b_trim4 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 4).unwrap();
        assert!(
            b_trim0.len() > b_trim4.len(),
            "expected trimFlexBits=4 ({}) < trimFlexBits=0 ({})",
            b_trim4.len(),
            b_trim0.len()
        );
        let d_trim4 = decode_jxr_to_rgba(&b_trim4).unwrap();
        assert_eq!((d_trim4.width, d_trim4.height), (w, h));
    }

    /// Reference 11x7 `32bppBGRA` lossless planar-alpha frequency-ordered `.jxr` bitstream
    /// generated by Microsoft `jxrlib` `JxrEncApp -c 22 -a 2 -q 1`.
    const JXRLIB_FREQ_11X7_PLANAR_JXR: &[u8] = &[
        0x49, 0x49, 0xBC, 0x01, 0x20, 0x00, 0x00, 0x00, 0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE,
        0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x0A, 0x00, 0x01, 0xBC, 0x01, 0x00, 0x10, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00,
        0x00, 0x02, 0xBC, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0xBC,
        0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0B, 0x00, 0x00, 0x00, 0x81, 0xBC, 0x04, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x82, 0xBC, 0x0B, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0xC0, 0x42, 0x83, 0xBC, 0x0B, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0,
        0x42, 0xC0, 0xBC, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x9E, 0x00, 0x00, 0x00, 0xC1, 0xBC,
        0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x4E, 0x04, 0x00, 0x00, 0xC2, 0xBC, 0x04, 0x00, 0x01,
        0x00, 0x00, 0x00, 0xEC, 0x04, 0x00, 0x00, 0xC3, 0xBC, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x6C, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x57, 0x4D, 0x50, 0x48, 0x4F, 0x54, 0x4F,
        0x00, 0x11, 0x45, 0xC0, 0x71, 0x00, 0x0A, 0x00, 0x06, 0x60, 0x00, 0xC0, 0x00, 0x00, 0x0C,
        0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x49,
        0xFF, 0xFF, 0x00, 0x00, 0x01, 0x01, 0x9F, 0x0D, 0x4A, 0x90, 0x00, 0x00, 0x01, 0x02, 0xFC,
        0x83, 0x10, 0x8C, 0xCA, 0x28, 0x66, 0xB4, 0xAF, 0x33, 0x02, 0x0F, 0x5B, 0xAC, 0x85, 0xB6,
        0xC1, 0xA3, 0x83, 0x18, 0x40, 0x84, 0x10, 0x63, 0x12, 0x20, 0x63, 0x20, 0xF5, 0x1C, 0x23,
        0x1B, 0xEA, 0xE6, 0xAF, 0xA2, 0x39, 0x73, 0x1C, 0x7D, 0x0F, 0xF1, 0x45, 0x85, 0x0E, 0x10,
        0x13, 0x51, 0x16, 0x0F, 0x0B, 0xCC, 0x7F, 0x3C, 0x14, 0x61, 0x0A, 0xA2, 0x07, 0x64, 0x00,
        0x00, 0x00, 0x01, 0x03, 0x7E, 0xF0, 0x89, 0xC6, 0x11, 0xB0, 0xC2, 0x21, 0x18, 0x48, 0x11,
        0x84, 0x74, 0x2E, 0x38, 0x61, 0x1B, 0x0C, 0x22, 0xA1, 0x14, 0x61, 0x0F, 0x18, 0x45, 0x22,
        0x08, 0xBB, 0xAA, 0xD8, 0x41, 0x21, 0x86, 0x11, 0x04, 0x18, 0x45, 0x22, 0x09, 0x2C, 0x20,
        0x8D, 0x06, 0x61, 0x84, 0x00, 0x61, 0x09, 0x10, 0x49, 0x51, 0x84, 0x46, 0x20, 0x88, 0xF8,
        0x24, 0x16, 0x13, 0x4D, 0xC0, 0x08, 0x22, 0x11, 0x84, 0x50, 0x30, 0x88, 0x46, 0x10, 0xB1,
        0x8C, 0x30, 0x81, 0x8C, 0x22, 0x11, 0x04, 0x30, 0x40, 0x04, 0x8C, 0x24, 0xF0, 0x82, 0x09,
        0xFE, 0x08, 0x7F, 0x02, 0x30, 0x80, 0x0C, 0x23, 0x11, 0x22, 0x08, 0x70, 0x91, 0x84, 0x52,
        0x23, 0x08, 0x20, 0xC2, 0x08, 0x20, 0x82, 0x04, 0x30, 0x88, 0x84, 0x11, 0x27, 0x84, 0x99,
        0xC2, 0x4B, 0xF0, 0x8E, 0xC4, 0x11, 0x18, 0xC2, 0x49, 0x88, 0x23, 0xB1, 0x04, 0x10, 0x41,
        0x17, 0x8C, 0x20, 0x22, 0x09, 0x38, 0x23, 0x88, 0x21, 0xC2, 0x08, 0x58, 0x81, 0x0C, 0x20,
        0x9C, 0x3E, 0x11, 0x36, 0x08, 0x30, 0xCC, 0x31, 0x44, 0x11, 0x10, 0x80, 0x8C, 0x10, 0x80,
        0x88, 0x24, 0x29, 0x80, 0x8C, 0x08, 0xC5, 0x1E, 0xE2, 0x43, 0x70, 0x92, 0xC3, 0x09, 0x20,
        0x23, 0x08, 0x51, 0x04, 0x5A, 0x20, 0x63, 0x08, 0x68, 0x82, 0x32, 0x10, 0x49, 0xA1, 0x90,
        0x43, 0x0C, 0x22, 0x21, 0x7D, 0x1B, 0x84, 0xB0, 0xA2, 0x47, 0xA1, 0xB2, 0x08, 0xA7, 0x07,
        0x1B, 0x87, 0x44, 0x12, 0x1B, 0xCF, 0x7C, 0x82, 0x61, 0x10, 0xCA, 0x41, 0x0E, 0x44, 0x12,
        0x47, 0x24, 0x10, 0x47, 0xDC, 0x10, 0x61, 0x04, 0x9E, 0x19, 0x46, 0x11, 0x50, 0x8E, 0x30,
        0x8A, 0x86, 0x00, 0x41, 0x13, 0x88, 0x26, 0xB2, 0x08, 0x22, 0xC1, 0x84, 0x54, 0x2F, 0xC0,
        0xF7, 0x8E, 0x20, 0x8A, 0x4C, 0xF2, 0x09, 0x35, 0x19, 0x85, 0xC8, 0x22, 0x98, 0xA1, 0x84,
        0x04, 0xB4, 0x54, 0x30, 0x92, 0x72, 0x09, 0x14, 0x50, 0x82, 0x5D, 0xE9, 0xA7, 0x90, 0x6E,
        0x77, 0x46, 0x85, 0xD0, 0xC2, 0x4C, 0x0B, 0x08, 0x24, 0x86, 0x8C, 0x83, 0x44, 0x13, 0x06,
        0x3C, 0x62, 0x49, 0xAE, 0x8D, 0xD1, 0xA2, 0xA1, 0x31, 0xDF, 0x69, 0x82, 0x50, 0x08, 0x41,
        0x84, 0xB4, 0x18, 0x44, 0x23, 0x08, 0xD8, 0x41, 0x1B, 0x08, 0x23, 0x71, 0x04, 0xC9, 0x08,
        0x22, 0xD1, 0x04, 0xED, 0x88, 0x26, 0xEC, 0x41, 0x22, 0x06, 0x10, 0xFE, 0x09, 0x80, 0xC2,
        0x7D, 0xF8, 0x47, 0x03, 0x09, 0x3E, 0x30, 0x91, 0xC3, 0x08, 0x40, 0xC2, 0x41, 0x08, 0x20,
        0x62, 0x09, 0x89, 0x1E, 0x18, 0x48, 0x51, 0x04, 0xBC, 0x18, 0xC3, 0x08, 0x88, 0x61, 0x30,
        0xBC, 0x12, 0xD2, 0x08, 0x0F, 0x08, 0xA0, 0x41, 0x29, 0x46, 0x12, 0x8C, 0x41, 0x31, 0x82,
        0x09, 0x4C, 0x20, 0x96, 0x63, 0x09, 0xDD, 0x10, 0x4C, 0x40, 0xC2, 0x25, 0x10, 0x4D, 0x10,
        0x82, 0x44, 0x8C, 0x27, 0x1C, 0x61, 0x2C, 0xBC, 0x24, 0x3E, 0x12, 0xD7, 0x04, 0x8B, 0x10,
        0x48, 0xE1, 0x04, 0x44, 0x20, 0x98, 0x51, 0x81, 0x10, 0x48, 0x91, 0x04, 0x08, 0x61, 0x18,
        0x08, 0x26, 0x20, 0x41, 0x1F, 0x88, 0x25, 0xD0, 0x82, 0x2E, 0x10, 0x44, 0xBE, 0x12, 0x37,
        0x09, 0x8C, 0xE1, 0x1E, 0x08, 0x26, 0x00, 0x61, 0x38, 0x63, 0x10, 0x41, 0x1A, 0x0B, 0x8C,
        0x24, 0x38, 0x81, 0x8C, 0x24, 0x20, 0x82, 0x6E, 0x44, 0x0C, 0x41, 0x0A, 0x10, 0x49, 0x0E,
        0x08, 0x44, 0x4C, 0x13, 0x16, 0x20, 0x87, 0x0C, 0x18, 0xF8, 0x82, 0x46, 0x0C, 0x20, 0x03,
        0xE3, 0x09, 0xC0, 0x19, 0x84, 0x11, 0x19, 0x0F, 0x15, 0xC5, 0xC2, 0x03, 0xC2, 0x60, 0x06,
        0x13, 0x24, 0x30, 0x03, 0x08, 0x00, 0xC2, 0x48, 0x88, 0x22, 0xA1, 0x84, 0x56, 0x30, 0x89,
        0x84, 0x11, 0x48, 0x82, 0x2F, 0x18, 0x44, 0xC3, 0x08, 0xA4, 0x61, 0x09, 0xF0, 0x8E, 0xB0,
        0x8C, 0xB8, 0x4A, 0xE1, 0x84, 0x34, 0xC2, 0x07, 0x18, 0x4D, 0x30, 0x82, 0x0D, 0x06, 0x13,
        0x24, 0x21, 0xC4, 0x10, 0xF2, 0x3E, 0xF9, 0xF0, 0x3E, 0x10, 0x4F, 0xBD, 0x20, 0xA0, 0xFB,
        0xE6, 0x13, 0x2B, 0x0A, 0x30, 0x48, 0x2D, 0x20, 0x98, 0x26, 0xC1, 0x84, 0x46, 0x6A, 0xF0,
        0x46, 0x42, 0x0A, 0x0F, 0x0C, 0x82, 0x0A, 0x30, 0x0C, 0x24, 0x98, 0xC2, 0x49, 0x0C, 0x24,
        0x00, 0x82, 0x44, 0x8C, 0x25, 0x98, 0xC2, 0x27, 0x20, 0x92, 0xD1, 0x04, 0x23, 0x82, 0x1D,
        0x84, 0x07, 0x8E, 0x20, 0x93, 0x63, 0x08, 0x30, 0x82, 0x5E, 0xE8, 0xB8, 0x88, 0x30, 0x90,
        0x22, 0x08, 0x40, 0x84, 0x37, 0x31, 0x94, 0x61, 0x2D, 0x69, 0x09, 0xE8, 0x82, 0x3D, 0x7C,
        0xC3, 0x09, 0x5E, 0x32, 0x15, 0x22, 0x45, 0x84, 0x11, 0x98, 0x8C, 0x24, 0xD7, 0xB8, 0xA3,
        0x09, 0x4A, 0x30, 0x92, 0xE3, 0x09, 0x10, 0x20, 0x88, 0x06, 0x11, 0xD8, 0xC2, 0x53, 0x88,
        0x26, 0x54, 0x41, 0x24, 0x44, 0x00, 0x41, 0x18, 0x8C, 0x26, 0xB8, 0x41, 0x35, 0xBE, 0x0F,
        0x09, 0x15, 0x82, 0x58, 0x8C, 0x26, 0xD8, 0x41, 0x09, 0x10, 0x4F, 0xE8, 0xC2, 0x41, 0x08,
        0x24, 0xF8, 0x82, 0x77, 0x06, 0x10, 0xB1, 0x04, 0x93, 0x18, 0x46, 0x42, 0x09, 0x6A, 0x20,
        0x92, 0x82, 0x08, 0xEB, 0xC2, 0x54, 0xC1, 0x0F, 0xC1, 0x16, 0x8C, 0x22, 0x61, 0x84, 0x64,
        0x3E, 0x20, 0x96, 0x22, 0x08, 0xF0, 0x41, 0x2B, 0xC6, 0x11, 0xF0, 0x82, 0x31, 0x10, 0x4B,
        0x31, 0x84, 0x4A, 0x30, 0x93, 0xC3, 0x09, 0x0D, 0xC1, 0x21, 0x20, 0x84, 0xF0, 0x8E, 0xC5,
        0x0C, 0x21, 0x22, 0x08, 0x94, 0x61, 0x12, 0x8C, 0x24, 0xD0, 0xC2, 0x04, 0x30, 0x43, 0x09,
        0x56, 0x30, 0x8C, 0x07, 0x08, 0x24, 0x38, 0x82, 0x25, 0xE0, 0x94, 0x70, 0x4C, 0x16, 0x14,
        0x61, 0x0B, 0x10, 0x49, 0xA1, 0x84, 0x76, 0x20, 0x98, 0x51, 0x84, 0x2C, 0x61, 0x28, 0x86,
        0x0C, 0x61, 0x1D, 0x0C, 0x24, 0x30, 0x82, 0x32, 0x18, 0x49, 0xC1, 0x84, 0xCF, 0xF8, 0x4C,
        0xCE, 0x12, 0xEF, 0x84, 0xB9, 0x12, 0x20, 0x87, 0x0C, 0x20, 0xE2, 0x09, 0x2C, 0x30, 0x83,
        0x8C, 0x27, 0xD8, 0x47, 0x10, 0x42, 0x84, 0x38, 0x82, 0x20, 0x23, 0xFD, 0xC2, 0x0D, 0xC2,
        0x35, 0x1A, 0x20, 0x92, 0xE2, 0x08, 0x70, 0xC8, 0x33, 0x08, 0x20, 0x02, 0x08, 0x48, 0x82,
        0x24, 0x18, 0x45, 0x63, 0x10, 0x41, 0x02, 0x10, 0x40, 0x38, 0xD0, 0x42, 0xF8, 0x4D, 0x18,
        0xB9, 0x00, 0x98, 0x45, 0xB1, 0x03, 0x10, 0x40, 0x86, 0x8C, 0x84, 0x01, 0xF3, 0x64, 0x53,
        0x3C, 0xC2, 0x4E, 0x72, 0x30, 0x8B, 0x89, 0x3A, 0xF8, 0x41, 0x0B, 0xA4, 0x13, 0x1F, 0xD3,
        0x2B, 0xB4, 0xE1, 0x05, 0x18, 0x41, 0x07, 0x86, 0x13, 0x0A, 0x20, 0x8A, 0xC4, 0x11, 0x40,
        0xC2, 0x3C, 0x18, 0x45, 0x02, 0x08, 0xA8, 0x41, 0x03, 0x18, 0x45, 0x43, 0x04, 0x23, 0xE8,
        0x2C, 0x88, 0x20, 0xC2, 0xE4, 0x13, 0x3B, 0x18, 0xE4, 0x4C, 0x18, 0xE3, 0x84, 0x41, 0x6A,
        0x06, 0x12, 0x47, 0x10, 0x45, 0x9E, 0x20, 0x92, 0xFB, 0xEF, 0xE3, 0x41, 0x16, 0x86, 0x12,
        0x58, 0x44, 0x10, 0x45, 0x25, 0x44, 0x12, 0x5C, 0xDD, 0x1D, 0x5A, 0xDC, 0x56, 0x4A, 0x14,
        0x57, 0x4D, 0x50, 0x48, 0x4F, 0x54, 0x4F, 0x00, 0x11, 0x45, 0xC0, 0x01, 0x00, 0x0A, 0x00,
        0x06, 0x00, 0x80, 0x20, 0x08, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x1D, 0xFF,
        0xFF, 0x00, 0x00, 0x01, 0x01, 0x2E, 0x80, 0x00, 0x00, 0x01, 0x02, 0xF1, 0x44, 0x2C, 0x1E,
        0x10, 0x82, 0x4C, 0x91, 0x10, 0x76, 0xDA, 0xA6, 0xD4, 0xFB, 0xF4, 0x0D, 0xF1, 0xD2, 0x5E,
        0x00, 0x00, 0x01, 0x03, 0xF0, 0x83, 0x0C, 0x24, 0x68, 0xC8, 0x30, 0x9D, 0x21, 0x84, 0x90,
        0x18, 0x48, 0x61, 0x10, 0x61, 0x30, 0x23, 0x09, 0xB7, 0x18, 0xC3, 0x09, 0xAF, 0x10, 0x47,
        0x22, 0x08, 0x67, 0x8D, 0x84, 0x07, 0x84, 0x66, 0x30, 0x93, 0xA3, 0x09, 0x1A, 0x20, 0x99,
        0x21, 0x04, 0x4C, 0x20, 0x91, 0x03, 0x84, 0x12, 0xF0, 0x67, 0x10, 0x48, 0xB1, 0x84, 0xAA,
        0x18, 0x44, 0x42, 0x08, 0x0F, 0x02, 0x9E, 0x11, 0x60, 0x82, 0x48, 0x08, 0x24, 0x68, 0x87,
        0x10, 0x4C, 0x58, 0x86, 0x10, 0x48, 0x01, 0x04, 0x44, 0x24, 0x41, 0x0B, 0x18, 0x42, 0x06,
        0x12, 0x54, 0x5B, 0xC2, 0x15, 0x17, 0x84, 0x8F, 0x10, 0x47, 0xA2, 0x08, 0x78, 0xC2, 0x23,
        0x10, 0x43, 0x06, 0x11, 0xB0, 0xC2, 0x45, 0x8C, 0xE3, 0x09, 0x0C, 0x30, 0x8F, 0x06, 0x12,
        0x24, 0x62, 0x0C, 0x21, 0xFE, 0x11, 0x0E, 0x11, 0x16, 0x08, 0xC0, 0x41, 0x22, 0x04, 0x51,
        0x84, 0x9D, 0x19, 0x84, 0x12, 0x04, 0x64, 0x18, 0x47, 0x23, 0x08, 0xA0, 0x41, 0x24, 0x86,
        0x12, 0xF4, 0x60, 0x86, 0x12, 0x47, 0x82, 0x39, 0xC2, 0x25, 0xC1, 0x13, 0x88, 0x61, 0x23,
        0x09, 0x95, 0x18, 0x01, 0x04, 0x14, 0x60, 0x04, 0x11, 0x08, 0x85, 0x82, 0x38, 0x87, 0x17,
        0xF3, 0xE2, 0xF0, 0x63, 0x44, 0x12, 0x88, 0x78, 0x47, 0x10, 0x42, 0x44, 0x13, 0x24, 0x24,
        0x42, 0x0C, 0x22, 0x91, 0x8E, 0x20, 0x95, 0x43, 0x09, 0x41, 0xE1, 0x19, 0xE1, 0x29, 0x78,
        0x47, 0x83, 0x28, 0xCC, 0x61, 0x13, 0xCC, 0xB1, 0x84, 0x10, 0xA8, 0xF1, 0x5E, 0x85, 0xA7,
        0x48, 0x24, 0xA6, 0x42, 0x09, 0x23, 0xCC, 0x7A, 0x28, 0xD9, 0x04, 0x91, 0x9B, 0x30, 0x91,
        0xE2, 0x71, 0xE0, 0x80, 0x8C, 0x23, 0x31, 0x04, 0xB6, 0x18, 0x46, 0x83, 0x08, 0x78, 0x82,
        0x38, 0x20, 0x97, 0x11, 0x04, 0x5A, 0x30, 0x89, 0xC4, 0x3F, 0x04, 0x0B, 0xF8, 0xE3, 0x08,
        0x09, 0x04, 0x06, 0x41, 0x2A, 0xE3, 0x08, 0x48, 0xE1, 0x84, 0xA4, 0x33, 0x44, 0x10, 0x02,
        0x17, 0x2A, 0x98, 0x4A, 0x14, 0x61, 0x33, 0x32, 0x41, 0x84, 0xC4, 0x49, 0x8F, 0xC6, 0x96,
        0xA5, 0x18, 0x4A, 0xD1, 0x0A, 0x30, 0x9B, 0x3D, 0x20, 0x84, 0x46, 0x13, 0x42, 0x3F, 0x04,
        0x10, 0x41, 0x89, 0xE7, 0x16, 0xED, 0xDD, 0x36, 0xC0,
    ];

    #[test]
    fn decodes_reference_jxrlib_frequency_bitstream_bit_exact() {
        let (w, h) = (11u32, 7u32);
        let mut expected_rgba = Vec::with_capacity((w * h * 4) as usize);
        let mut state = 0xCAFE_BABE_u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let r = ((state >> 24) as u8).wrapping_add((x * 23) as u8);
                let g = ((state >> 16) as u8).wrapping_add((y * 41) as u8);
                let b = ((state >> 8) as u8).wrapping_add(((x + y) * 17) as u8);
                let a = (state as u8) ^ ((x * 31 + y * 13) as u8);
                expected_rgba.extend_from_slice(&[r, g, b, a]);
            }
        }

        let decoded = decode_jxr_to_rgba(JXRLIB_FREQ_11X7_PLANAR_JXR).unwrap();
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.rgba.as_ref(), expected_rgba.as_slice());
    }

    #[test]
    fn bidirectional_interoperability_with_reference_jxrlib_tools() {
        use std::path::Path;
        use std::process::Command;

        let dec_bin =
            Path::new("/Users/miles/Projects/NeoFlash/target/jxr-research/jxrlib/build/JxrDecApp");
        let enc_bin =
            Path::new("/Users/miles/Projects/NeoFlash/target/jxr-research/jxrlib/build/JxrEncApp");
        if !dec_bin.is_file() || !enc_bin.is_file() {
            return;
        }

        let dir = std::env::temp_dir().join(format!("neoflash_jxr_interop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        fn read_tif_pixels(data: &[u8], expected_len: usize) -> Vec<u8> {
            let ifd_off = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
            let ntags = u16::from_le_bytes(data[ifd_off..ifd_off + 2].try_into().unwrap()) as usize;
            let mut strip_off = None;
            for i in 0..ntags {
                let off = ifd_off + 2 + i * 12;
                let tid = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
                let ttype = u16::from_le_bytes(data[off + 2..off + 4].try_into().unwrap());
                let cnt = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
                let val = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
                if tid == 273 {
                    strip_off = Some(if ttype == 4 && cnt == 1 {
                        val as usize
                    } else {
                        (val & 0xFFFF) as usize
                    });
                }
            }
            let s = strip_off.expect("TIFF StripOffsets tag 273");
            data[s..s + expected_len].to_vec()
        }

        fn write_rgba_tif(path: &Path, width: u32, height: u32, rgba: &[u8]) {
            let num_tags: u16 = 10;
            let ifd_off: u32 = 8;
            let bps_off: u32 = ifd_off + 2 + u32::from(num_tags) * 12 + 4;
            let strip_off: u32 = bps_off + 8;
            let mut out = Vec::with_capacity(strip_off as usize + rgba.len());
            out.extend_from_slice(b"II");
            out.extend_from_slice(&42u16.to_le_bytes());
            out.extend_from_slice(&ifd_off.to_le_bytes());
            out.extend_from_slice(&num_tags.to_le_bytes());
            let mut tag = |tid: u16, ttype: u16, count: u32, val: u32| {
                out.extend_from_slice(&tid.to_le_bytes());
                out.extend_from_slice(&ttype.to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(&val.to_le_bytes());
            };
            tag(256, 4, 1, width);
            tag(257, 4, 1, height);
            tag(258, 3, 4, bps_off);
            tag(259, 3, 1, 1);
            tag(262, 3, 1, 2);
            tag(273, 4, 1, strip_off);
            tag(277, 3, 1, 4);
            tag(278, 4, 1, height);
            tag(279, 4, 1, rgba.len() as u32);
            tag(338, 3, 1, 2);
            out.extend_from_slice(&0u32.to_le_bytes());
            for _ in 0..4 {
                out.extend_from_slice(&8u16.to_le_bytes());
            }
            assert_eq!(out.len(), strip_off as usize);
            out.extend_from_slice(rgba);
            std::fs::write(path, out).unwrap();
        }

        // A) NeoFlash encode_jxr_argb -> reference JxrDecApp
        let (w, h) = (19u32, 13u32);
        let mut argb = Vec::with_capacity((w * h * 4) as usize);
        let mut state = 0x9E37_79B9_u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let a = ((state >> 24) as u8).wrapping_add((x * 13 + y * 7) as u8);
                let r = ((state >> 16) as u8).wrapping_add((x * 29) as u8);
                let g = ((state >> 8) as u8).wrapping_add((y * 37) as u8);
                let b = (state as u8).wrapping_add(((x + y) * 19) as u8);
                argb.extend_from_slice(&[a, r, g, b]);
            }
        }

        // 1) Lossless 19x13 opaque 24bppBGR -> JxrDecApp -c 0 -> 24bppRGB TIFF
        let jxr_rgb = encode_jxr_argb(w, h, false, &argb, 0, "4:4:4", 0).unwrap();
        let jxr_rgb_path = dir.join("nf_19x13_rgb.jxr");
        let tif_rgb_path = dir.join("nf_19x13_rgb.tif");
        std::fs::write(&jxr_rgb_path, &jxr_rgb).unwrap();
        let st = Command::new(dec_bin)
            .args([
                "-i",
                jxr_rgb_path.to_str().unwrap(),
                "-o",
                tif_rgb_path.to_str().unwrap(),
                "-c",
                "0",
            ])
            .status()
            .unwrap();
        assert!(st.success(), "JxrDecApp must accept NeoFlash 24bppBGR .jxr");
        let tif_rgb_bytes = std::fs::read(&tif_rgb_path).unwrap();
        let dec_rgb_pixels = read_tif_pixels(&tif_rgb_bytes, (w * h * 3) as usize);
        let expected_rgb: Vec<u8> = argb
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[1], px[2], px[3]])
            .collect();
        assert_eq!(
            dec_rgb_pixels, expected_rgb,
            "JxrDecApp 24bppBGR lossless bit-exact match"
        );

        // 2) Lossless 19x13 planar-alpha 32bppBGRA & interleaved-alpha 32bppBGRA -> JxrDecApp -c 22
        let expected_rgba = argb_to_rgba(&argb, true);
        for (label, bytes) in [
            (
                "planar",
                encode_jxr_argb(w, h, true, &argb, 0, "4:4:4", 0).unwrap(),
            ),
            (
                "interleaved",
                encode_jxr_with_alpha_mode(w, h, JxrAlphaMode::Interleaved, &argb, 0, "4:4:4", 0)
                    .unwrap(),
            ),
        ] {
            let jxr_path = dir.join(format!("nf_19x13_{label}.jxr"));
            let tif_path = dir.join(format!("nf_19x13_{label}.tif"));
            std::fs::write(&jxr_path, &bytes).unwrap();
            let st = Command::new(dec_bin)
                .args([
                    "-i",
                    jxr_path.to_str().unwrap(),
                    "-o",
                    tif_path.to_str().unwrap(),
                    "-c",
                    "22",
                ])
                .status()
                .unwrap();
            assert!(
                st.success(),
                "JxrDecApp must accept NeoFlash {label} 32bppBGRA .jxr"
            );
            let tif_bytes = std::fs::read(&tif_path).unwrap();
            let dec_rgba_pixels = read_tif_pixels(&tif_bytes, (w * h * 4) as usize);
            assert_eq!(
                dec_rgba_pixels, expected_rgba,
                "JxrDecApp {label} 32bppBGRA lossless bit-exact match"
            );
        }

        // 3) Lossy / subsampled / flexbit-trimmed NeoFlash streams -> JxrDecApp agrees with decode_jxr_to_rgba
        let (gw, gh) = (48u32, 32u32);
        let mut grad_argb = Vec::with_capacity((gw * gh * 4) as usize);
        for y in 0..gh {
            for x in 0..gw {
                let r = ((x * 255) / (gw - 1)) as u8;
                let g = ((y * 255) / (gh - 1)) as u8;
                let b = (((x + y) * 255) / (gw + gh - 2)) as u8;
                let a = (128 + ((x * 127) / (gw - 1))) as u8;
                grad_argb.extend_from_slice(&[a, r, g, b]);
            }
        }
        for (idx, (cs, q, trim)) in [
            ("4:4:4", 20u8, 0u8),
            ("4:2:2", 20, 0),
            ("4:2:0", 20, 0),
            ("4:4:4", 60, 0),
            ("4:4:4", 90, 0),
            ("4:4:4", 20, 4),
        ]
        .into_iter()
        .enumerate()
        {
            let stream = encode_jxr_argb(gw, gh, true, &grad_argb, q, cs, trim).unwrap();
            let nf_dec = decode_jxr_to_rgba(&stream).unwrap();
            let jxr_path = dir.join(format!("nf_lossy_{idx}.jxr"));
            let tif_path = dir.join(format!("nf_lossy_{idx}.tif"));
            std::fs::write(&jxr_path, &stream).unwrap();
            let st = Command::new(dec_bin)
                .args([
                    "-i",
                    jxr_path.to_str().unwrap(),
                    "-o",
                    tif_path.to_str().unwrap(),
                    "-c",
                    "22",
                ])
                .status()
                .unwrap();
            assert!(
                st.success(),
                "JxrDecApp must accept lossy stream cs={cs} q={q} trim={trim}"
            );
            let tif_bytes = std::fs::read(&tif_path).unwrap();
            let ref_pixels = read_tif_pixels(&tif_bytes, (gw * gh * 4) as usize);
            assert_eq!(
                nf_dec.rgba.as_ref(),
                ref_pixels.as_slice(),
                "decode_jxr_to_rgba must match JxrDecApp bit-for-bit on cs={cs} q={q} trim={trim}"
            );
        }

        // B) Reference JxrEncApp (frequency + spatial + interleaved + 422 + 420 + overlap2 + trim) -> decode_jxr_to_rgba
        let src_tif = dir.join("src_19x13.tif");
        write_rgba_tif(&src_tif, w, h, &expected_rgba);
        for (idx, extra_args) in [
            vec!["-c", "22", "-a", "2", "-q", "1"],
            vec!["-c", "22", "-a", "3", "-q", "1"],
            vec!["-c", "22", "-a", "2", "-q", "1", "-f"],
            vec!["-c", "22", "-a", "2", "-q", "20", "-d", "2", "-l", "1"],
            vec![
                "-c", "22", "-a", "2", "-q", "35", "-d", "1", "-l", "2", "-F", "4",
            ],
        ]
        .into_iter()
        .enumerate()
        {
            let jxr_path = dir.join(format!("ref_enc_{idx}.jxr"));
            let tif_path = dir.join(format!("ref_dec_{idx}.tif"));
            let st_enc = Command::new(enc_bin)
                .arg("-i")
                .arg(&src_tif)
                .arg("-o")
                .arg(&jxr_path)
                .args(&extra_args)
                .status()
                .unwrap();
            assert!(st_enc.success(), "JxrEncApp case {idx}");
            let st_dec = Command::new(dec_bin)
                .args([
                    "-i",
                    jxr_path.to_str().unwrap(),
                    "-o",
                    tif_path.to_str().unwrap(),
                    "-c",
                    "22",
                ])
                .status()
                .unwrap();
            assert!(st_dec.success(), "JxrDecApp case {idx}");
            let jxr_bytes = std::fs::read(&jxr_path).unwrap();
            let ref_rgba =
                read_tif_pixels(&std::fs::read(&tif_path).unwrap(), (w * h * 4) as usize);
            let nf_dec = decode_jxr_to_rgba(&jxr_bytes).unwrap();
            assert_eq!(
                nf_dec.rgba.as_ref(),
                ref_rgba.as_slice(),
                "decode_jxr_to_rgba must match JxrDecApp bit-for-bit on JxrEncApp case {idx} ({extra_args:?})"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
