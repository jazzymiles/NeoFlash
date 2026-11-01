//! Bounded, read-only SWF container decompression and tag header inspection.
//!
//! Never executes AVM1/AVM2 bytecode or claims editable source recovery.

use crate::{ImportError, ImportReport};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

pub(crate) const MAX_SWF_BYTES: usize = 64 * 1024 * 1024;
const MAX_SWF_TAGS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwfCompression {
    Uncompressed,
    Zlib,
    Lzma,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwfTagCounts {
    pub total: usize,
    pub show_frame: usize,
    pub define_sprite: usize,
    pub shapes: usize,
    pub bitmaps: usize,
    pub sounds: usize,
    pub buttons: usize,
    pub texts: usize,
    pub fonts: usize,
    pub placements: usize,
    pub do_action: usize,
    pub do_abc: usize,
    pub file_attributes: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwfInspection {
    pub compression: SwfCompression,
    pub swf_version: u8,
    pub declared_file_length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decompressed_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_bounds_twips: Option<[i32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_px: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_px: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    pub as3_flag: bool,
    pub has_do_action: bool,
    pub has_do_abc: bool,
    pub end_tag_reached: bool,
    pub tags: SwfTagCounts,
}

pub(crate) struct SwfContainer {
    pub compression: SwfCompression,
    pub swf_version: u8,
    pub declared_file_length: u32,
    pub compressed_bytes: Option<usize>,
    pub body: Option<Vec<u8>>,
}

pub(crate) fn read_container(path: &Path, header: &[u8]) -> Result<SwfContainer, ImportError> {
    if header.len() < 8 {
        return Err(ImportError::Container("truncated SWF header".into()));
    }
    let sig = &header[..3];
    let swf_version = header[3];
    let declared = u32::from_le_bytes(header[4..8].try_into().unwrap());
    if declared < 8 {
        return Err(ImportError::Container("invalid SWF declared length".into()));
    }
    if (declared as usize) > MAX_SWF_BYTES {
        return Err(ImportError::Container(
            "SWF declared length exceeds 64 MiB limit".into(),
        ));
    }
    let file_len = fs::metadata(path)?.len();
    if file_len > MAX_SWF_BYTES as u64 {
        return Err(ImportError::Container(
            "SWF container exceeds 64 MiB limit".into(),
        ));
    }
    let mut raw = Vec::new();
    File::open(path)?
        .take(MAX_SWF_BYTES as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > MAX_SWF_BYTES {
        return Err(ImportError::Container(
            "SWF container exceeds 64 MiB limit".into(),
        ));
    }
    if raw.len() < 8 {
        return Err(ImportError::Container("truncated SWF header".into()));
    }

    match sig {
        b"FWS" => {
            if raw.len() != declared as usize {
                return Err(ImportError::Container(
                    "uncompressed SWF length does not match its header".into(),
                ));
            }
            Ok(SwfContainer {
                compression: SwfCompression::Uncompressed,
                swf_version,
                declared_file_length: declared,
                compressed_bytes: None,
                body: Some(raw[8..].to_vec()),
            })
        }
        b"CWS" => {
            let compressed = &raw[8..];
            if compressed.is_empty() {
                return Err(ImportError::Container(
                    "truncated zlib-compressed SWF stream".into(),
                ));
            }
            let expected_body = (declared as usize) - 8;
            let body = decompress_cws_bounded(compressed, expected_body)?;
            Ok(SwfContainer {
                compression: SwfCompression::Zlib,
                swf_version,
                declared_file_length: declared,
                compressed_bytes: Some(compressed.len()),
                body: Some(body),
            })
        }
        b"ZWS" => {
            if raw.len() < 17 {
                return Err(ImportError::Container("truncated ZWS SWF header".into()));
            }
            let compressed_len = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
            if 17usize.checked_add(compressed_len) != Some(raw.len()) {
                return Err(ImportError::Container(
                    "ZWS compressed payload length does not match container size".into(),
                ));
            }
            Ok(SwfContainer {
                compression: SwfCompression::Lzma,
                swf_version,
                declared_file_length: declared,
                compressed_bytes: Some(compressed_len),
                body: None,
            })
        }
        _ => Err(ImportError::Container("unrecognized SWF signature".into())),
    }
}

fn decompress_cws_bounded(compressed: &[u8], expected_body: usize) -> Result<Vec<u8>, ImportError> {
    if expected_body > MAX_SWF_BYTES - 8 {
        return Err(ImportError::Container(
            "SWF decompressed payload exceeds 64 MiB limit".into(),
        ));
    }
    let mut decoder = flate2::Decompress::new(true);
    let mut body = Vec::with_capacity(expected_body.min(64 * 1024));
    let mut chunk = [0u8; 32 * 1024];
    let mut in_offset = 0usize;
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let flush = if in_offset == compressed.len() {
            flate2::FlushDecompress::Finish
        } else {
            flate2::FlushDecompress::None
        };
        let status = decoder
            .decompress(&compressed[in_offset..], &mut chunk, flush)
            .map_err(|e| ImportError::Container(format!("invalid CWS zlib stream: {e}")))?;
        let consumed = (decoder.total_in() - before_in) as usize;
        let produced = (decoder.total_out() - before_out) as usize;
        in_offset += consumed;
        if body
            .len()
            .checked_add(produced)
            .is_none_or(|next| next > expected_body || next > MAX_SWF_BYTES - 8)
        {
            return Err(ImportError::Container(
                "decompressed CWS payload exceeds declared SWF length or 64 MiB limit".into(),
            ));
        }
        body.extend_from_slice(&chunk[..produced]);
        match status {
            flate2::Status::StreamEnd => break,
            flate2::Status::Ok | flate2::Status::BufError => {
                if consumed == 0 && produced == 0 {
                    return Err(ImportError::Container(
                        "truncated or incomplete CWS zlib stream".into(),
                    ));
                }
            }
        }
    }
    if in_offset != compressed.len() {
        return Err(ImportError::Container(
            "CWS container has trailing bytes after zlib stream".into(),
        ));
    }
    if body.len() != expected_body {
        return Err(ImportError::Container(
            "decompressed CWS length does not match declared SWF header".into(),
        ));
    }
    Ok(body)
}

pub(crate) fn inspect(container: &SwfContainer, location: &str, report: &mut ImportReport) {
    report.error(
        "swf_not_implemented",
        location,
        "SWF container inspected read-only. Playback, bytecode execution/decompilation and editable source recovery are not implemented.",
    );

    let Some(body) = container.body.as_deref() else {
        report.warning(
            "swf_lzma_unsupported",
            location,
            "ZWS (LZMA-compressed SWF 13+) header validated; tag payload is not decompressed because no offline LZMA decoder is bundled.",
        );
        report.swf = Some(SwfInspection {
            compression: container.compression,
            swf_version: container.swf_version,
            declared_file_length: container.declared_file_length,
            compressed_bytes: container.compressed_bytes,
            decompressed_bytes: None,
            stage_bounds_twips: None,
            width_px: None,
            height_px: None,
            frame_rate: None,
            frame_count: None,
            background_color: None,
            as3_flag: false,
            has_do_action: false,
            has_do_abc: false,
            end_tag_reached: false,
            tags: SwfTagCounts::default(),
        });
        return;
    };

    let decompressed_bytes = Some(body.len() + 8);
    let Some((bounds, rect_bytes)) = parse_rect(body) else {
        report.error(
            "swf_truncated",
            format!("{location}#header"),
            "SWF movie header ends before stage RECT is complete.",
        );
        report.swf = Some(SwfInspection {
            compression: container.compression,
            swf_version: container.swf_version,
            declared_file_length: container.declared_file_length,
            compressed_bytes: container.compressed_bytes,
            decompressed_bytes,
            stage_bounds_twips: None,
            width_px: None,
            height_px: None,
            frame_rate: None,
            frame_count: None,
            background_color: None,
            as3_flag: false,
            has_do_action: false,
            has_do_abc: false,
            end_tag_reached: false,
            tags: SwfTagCounts::default(),
        });
        return;
    };

    let [xmin, xmax, ymin, ymax] = bounds;
    if xmax < xmin || ymax < ymin {
        report.error(
            "swf_invalid_rect",
            format!("{location}#header"),
            format!("SWF stage RECT has inverted bounds: [{xmin}, {xmax}, {ymin}, {ymax}]."),
        );
    }
    let width_px = ((i64::from(xmax) - i64::from(xmin)).max(0) as f64) / 20.0;
    let height_px = ((i64::from(ymax) - i64::from(ymin)).max(0) as f64) / 20.0;

    if body.len() < rect_bytes + 4 {
        report.error(
            "swf_truncated",
            format!("{location}#header"),
            "SWF movie header ends before frame_rate and frame_count.",
        );
        report.swf = Some(SwfInspection {
            compression: container.compression,
            swf_version: container.swf_version,
            declared_file_length: container.declared_file_length,
            compressed_bytes: container.compressed_bytes,
            decompressed_bytes,
            stage_bounds_twips: Some(bounds),
            width_px: Some(width_px),
            height_px: Some(height_px),
            frame_rate: None,
            frame_count: None,
            background_color: None,
            as3_flag: false,
            has_do_action: false,
            has_do_abc: false,
            end_tag_reached: false,
            tags: SwfTagCounts::default(),
        });
        return;
    }

    let frame_rate_raw = u16::from_le_bytes(body[rect_bytes..rect_bytes + 2].try_into().unwrap());
    let frame_rate = f64::from(frame_rate_raw) / 256.0;
    let frame_count = u16::from_le_bytes(body[rect_bytes + 2..rect_bytes + 4].try_into().unwrap());

    let mut state = TagWalkState::default();
    let end_tag_reached = walk_tags(&body[rect_bytes + 4..], 0, location, &mut state, report);

    report.statistics.symbols = state.tags.define_sprite + state.tags.buttons;
    report.statistics.keyframes = state.tags.show_frame;
    report.statistics.elements = state.tags.shapes + state.tags.bitmaps + state.tags.texts;
    report.statistics.scripts = state.tags.do_action + state.tags.do_abc;

    report.swf = Some(SwfInspection {
        compression: container.compression,
        swf_version: container.swf_version,
        declared_file_length: container.declared_file_length,
        compressed_bytes: container.compressed_bytes,
        decompressed_bytes,
        stage_bounds_twips: Some(bounds),
        width_px: Some(width_px),
        height_px: Some(height_px),
        frame_rate: Some(frame_rate),
        frame_count: Some(frame_count),
        background_color: state.background_color,
        as3_flag: state.as3_flag,
        has_do_action: state.tags.do_action > 0,
        has_do_abc: state.tags.do_abc > 0,
        end_tag_reached,
        tags: state.tags,
    });
}

#[derive(Default)]
struct TagWalkState {
    tags: SwfTagCounts,
    background_color: Option<String>,
    as3_flag: bool,
    budget_exceeded: bool,
}

fn walk_tags(
    bytes: &[u8],
    depth: usize,
    location: &str,
    state: &mut TagWalkState,
    report: &mut ImportReport,
) -> bool {
    let mut pos = 0usize;
    while pos < bytes.len() {
        if state.tags.total >= MAX_SWF_TAGS {
            if !state.budget_exceeded {
                state.budget_exceeded = true;
                report.error(
                    "swf_tag_budget_exceeded",
                    format!("{location}#tags"),
                    "SWF exceeds 1,000,000 tag inspection budget.",
                );
            }
            return false;
        }
        let tag_offset = pos;
        if pos + 2 > bytes.len() {
            report.error(
                "swf_truncated",
                format!("{location}#tag@{tag_offset}"),
                "Truncated 2-byte SWF tag header.",
            );
            return false;
        }
        let raw_hdr = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        pos += 2;
        let tag_code = raw_hdr >> 6;
        let short_len = usize::from(raw_hdr & 0x3f);
        let tag_len = if short_len == 0x3f {
            if pos + 4 > bytes.len() {
                report.error(
                    "swf_truncated",
                    format!("{location}#tag@{tag_offset}"),
                    format!("Truncated 4-byte extended length on SWF tag {tag_code}."),
                );
                return false;
            }
            let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            len
        } else {
            short_len
        };
        let Some(end) = pos.checked_add(tag_len) else {
            report.error(
                "swf_truncated",
                format!("{location}#tag@{tag_offset}"),
                format!("Overflowing length on SWF tag {tag_code}."),
            );
            return false;
        };
        if end > bytes.len() {
            report.error(
                "swf_truncated",
                format!("{location}#tag@{tag_offset}"),
                format!(
                    "SWF tag {tag_code} declares {tag_len} bytes, exceeding remaining {} stream bytes.",
                    bytes.len() - pos
                ),
            );
            return false;
        }
        let payload = &bytes[pos..end];
        pos = end;
        state.tags.total += 1;

        match tag_code {
            0 => {
                if pos < bytes.len() {
                    report.warning(
                        "swf_trailing_bytes",
                        format!("{location}#tag@{tag_offset}"),
                        format!("{} trailing byte(s) follow SWF End tag.", bytes.len() - pos),
                    );
                }
                return true;
            }
            1 => state.tags.show_frame += 1,
            // DefineShape, DefineShape2, DefineShape3, DefineMorphShape, DefineShape4, DefineMorphShape2
            2 | 22 | 32 | 46 | 83 | 84 => state.tags.shapes += 1,
            // PlaceObject, PlaceObject2, PlaceObject3
            4 | 26 | 70 => state.tags.placements += 1,
            // DefineBits, DefineBitsLossless, DefineBitsJPEG2, DefineBitsJPEG3, DefineBitsLossless2, DefineBitsJPEG4
            6 | 20 | 21 | 35 | 36 | 90 => state.tags.bitmaps += 1,
            // DefineButton, DefineButton2
            7 | 34 => state.tags.buttons += 1,
            // SetBackgroundColor
            9 => {
                if depth == 0 && state.background_color.is_none() && payload.len() >= 3 {
                    state.background_color = Some(format!(
                        "#{:02x}{:02x}{:02x}",
                        payload[0], payload[1], payload[2]
                    ));
                }
            }
            // DefineFont, DefineFont2, DefineFont3, DefineFont4
            10 | 48 | 75 | 91 => state.tags.fonts += 1,
            // DefineText, DefineText2, DefineEditText
            11 | 33 | 37 => state.tags.texts += 1,
            // DoAction, DoInitAction
            12 | 59 => state.tags.do_action += 1,
            // DefineSound, SoundStreamHead, SoundStreamHead2
            14 | 18 | 45 => state.tags.sounds += 1,
            // DefineSprite (only top-level sprites may define nested control tags)
            39 => {
                state.tags.define_sprite += 1;
                if depth > 0 {
                    report.warning(
                        "swf_nested_sprite",
                        format!("{location}#tag@{tag_offset}"),
                        "Nested DefineSprite inside DefineSprite is not walked recursively.",
                    );
                } else if payload.len() < 4 {
                    report.error(
                        "swf_truncated",
                        format!("{location}#sprite@{tag_offset}"),
                        "DefineSprite payload is shorter than its 4-byte SpriteID/FrameCount header.",
                    );
                } else {
                    let sprite_id = u16::from_le_bytes([payload[0], payload[1]]);
                    let sprite_loc = format!("{location}#sprite:{sprite_id}");
                    let _ = walk_tags(&payload[4..], depth + 1, &sprite_loc, state, report);
                }
            }
            // FileAttributes
            69 => {
                state.tags.file_attributes += 1;
                if let Some(&flags) = payload.first()
                    && (flags & 0x08) != 0
                {
                    state.as3_flag = true;
                }
            }
            // DoABC, DoABC2
            72 | 82 => state.tags.do_abc += 1,
            _ => {}
        }
    }

    report.error(
        "swf_truncated",
        format!("{location}#tags"),
        "SWF tag stream ended without a terminating End (0) tag.",
    );
    false
}

fn parse_rect(bytes: &[u8]) -> Option<([i32; 4], usize)> {
    let first = *bytes.first()?;
    let nbits = usize::from(first >> 3);
    let total_bits = 5 + 4 * nbits;
    let rect_bytes = total_bits.div_ceil(8);
    if bytes.len() < rect_bytes {
        return None;
    }
    let xmin = read_signed_bits(bytes, 5, nbits);
    let xmax = read_signed_bits(bytes, 5 + nbits, nbits);
    let ymin = read_signed_bits(bytes, 5 + 2 * nbits, nbits);
    let ymax = read_signed_bits(bytes, 5 + 3 * nbits, nbits);
    Some(([xmin, xmax, ymin, ymax], rect_bytes))
}

fn read_signed_bits(bytes: &[u8], bit_offset: usize, nbits: usize) -> i32 {
    if nbits == 0 {
        return 0;
    }
    let mut value = 0u32;
    for i in 0..nbits {
        let bit_index = bit_offset + i;
        let byte = bytes[bit_index / 8];
        let bit = (byte >> (7 - (bit_index % 8))) & 1;
        value = (value << 1) | u32::from(bit);
    }
    let shift = 32 - nbits;
    ((value << shift) as i32) >> shift
}
