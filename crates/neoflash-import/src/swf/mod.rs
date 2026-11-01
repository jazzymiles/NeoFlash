//! Bounded SWF (`FWS` uncompressed, `CWS` zlib, `ZWS` LZMA SWF 13+) container
//! decompression, structural tag inspection, and editable NeoFlash `Project` recovery.
//!
//! Never executes AVM1/AVM2 bytecode during import; bytecode is statically decoded
//! into editable ActionScript 1/2/3 source.

mod avm1;
mod avm2;
mod bits;
mod lzma;
mod media;
mod shape;
mod text;
mod timeline;

use crate::{ImportError, ImportReport};
use neoflash_core::Project;
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
    pub raw_bytes: Vec<u8>,
    pub body: Option<Vec<u8>>,
    pub zws_error: Option<String>,
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
            let body = raw[8..].to_vec();
            Ok(SwfContainer {
                compression: SwfCompression::Uncompressed,
                swf_version,
                declared_file_length: declared,
                compressed_bytes: None,
                raw_bytes: raw,
                body: Some(body),
                zws_error: None,
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
            let compressed_len = compressed.len();
            Ok(SwfContainer {
                compression: SwfCompression::Zlib,
                swf_version,
                declared_file_length: declared,
                compressed_bytes: Some(compressed_len),
                raw_bytes: raw,
                body: Some(body),
                zws_error: None,
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
            let props: [u8; 5] = raw[12..17].try_into().unwrap();
            let expected_body = (declared as usize) - 8;
            match lzma::decompress_zws(props, &raw[17..], expected_body, MAX_SWF_BYTES - 8) {
                Ok(body) => Ok(SwfContainer {
                    compression: SwfCompression::Lzma,
                    swf_version,
                    declared_file_length: declared,
                    compressed_bytes: Some(compressed_len),
                    raw_bytes: raw,
                    body: Some(body),
                    zws_error: None,
                }),
                Err(err) => Ok(SwfContainer {
                    compression: SwfCompression::Lzma,
                    swf_version,
                    declared_file_length: declared,
                    compressed_bytes: Some(compressed_len),
                    raw_bytes: raw,
                    body: None,
                    zws_error: Some(err),
                }),
            }
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

pub(crate) fn convert(
    container: &SwfContainer,
    location: &str,
    report: &mut ImportReport,
) -> Option<Project> {
    let Some(body) = container.body.as_deref() else {
        let msg = container
            .zws_error
            .as_deref()
            .unwrap_or("ZWS LZMA stream could not be decompressed");
        report.error(
            "swf_lzma_corrupt",
            location,
            format!("Invalid or corrupt ZWS LZMA payload: {msg}."),
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
        return None;
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
        return None;
    };

    let [xmin, xmax, ymin, ymax] = bounds;
    let mut invalid_rect = false;
    if xmax < xmin || ymax < ymin {
        invalid_rect = true;
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
        return None;
    }

    let frame_rate_raw = u16::from_le_bytes(body[rect_bytes..rect_bytes + 2].try_into().unwrap());
    let frame_rate = f64::from(frame_rate_raw) / 256.0;
    let frame_count = u16::from_le_bytes(body[rect_bytes + 2..rect_bytes + 4].try_into().unwrap());

    let mut state = TagWalkState::default();
    let end_tag_reached = walk_tags(&body[rect_bytes + 4..], 0, location, &mut state, report);

    let inspection = SwfInspection {
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
    };
    report.swf = Some(inspection.clone());

    if invalid_rect || state.budget_exceeded {
        return None;
    }

    timeline::build_project_from_swf(
        container,
        &inspection,
        body,
        rect_bytes + 4,
        location,
        report,
    )
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
            report.warning(
                "swf_truncated_tag_stream",
                format!("{location}#tag@{tag_offset}"),
                "Truncated 2-byte SWF tag header; earlier complete records were retained.",
            );
            return false;
        }
        let raw_hdr = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        pos += 2;
        let tag_code = raw_hdr >> 6;
        let short_len = usize::from(raw_hdr & 0x3f);
        let tag_len = if short_len == 0x3f {
            if pos + 4 > bytes.len() {
                report.warning(
                    "swf_truncated_tag_stream",
                    format!("{location}#tag@{tag_offset}"),
                    format!(
                        "Truncated 4-byte extended length on SWF tag {tag_code}; earlier complete records were retained."
                    ),
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
            report.warning(
                "swf_truncated_tag_stream",
                format!("{location}#tag@{tag_offset}"),
                format!("Overflowing length on SWF tag {tag_code}; earlier complete records were retained."),
            );
            return false;
        };
        if end > bytes.len() {
            report.warning(
                "swf_truncated_tag_stream",
                format!("{location}#tag@{tag_offset}"),
                format!(
                    "SWF tag {tag_code} declares {tag_len} bytes, exceeding remaining {} stream bytes; earlier complete records were retained.",
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
            2 | 22 | 32 | 46 | 83 | 84 => state.tags.shapes += 1,
            4 | 26 | 70 => state.tags.placements += 1,
            6 | 20 | 21 | 35 | 36 | 90 => state.tags.bitmaps += 1,
            7 | 34 => state.tags.buttons += 1,
            9 => {
                if depth == 0 && state.background_color.is_none() && payload.len() >= 3 {
                    state.background_color = Some(format!(
                        "#{:02x}{:02x}{:02x}",
                        payload[0], payload[1], payload[2]
                    ));
                }
            }
            10 | 48 | 75 | 91 => state.tags.fonts += 1,
            11 | 33 | 37 => state.tags.texts += 1,
            12 | 59 => state.tags.do_action += 1,
            14 | 18 | 45 => state.tags.sounds += 1,
            39 => {
                state.tags.define_sprite += 1;
                if depth > 0 {
                    report.warning(
                        "swf_nested_sprite",
                        format!("{location}#tag@{tag_offset}"),
                        "Nested DefineSprite inside DefineSprite is not walked recursively.",
                    );
                } else if payload.len() < 4 {
                    report.warning(
                        "swf_truncated_tag_stream",
                        format!("{location}#sprite@{tag_offset}"),
                        "DefineSprite payload is shorter than its 4-byte SpriteID/FrameCount header.",
                    );
                } else {
                    let sprite_id = u16::from_le_bytes([payload[0], payload[1]]);
                    let sprite_loc = format!("{location}#sprite:{sprite_id}");
                    let _ = walk_tags(&payload[4..], depth + 1, &sprite_loc, state, report);
                }
            }
            69 => {
                state.tags.file_attributes += 1;
                if let Some(&flags) = payload.first()
                    && (flags & 0x08) != 0
                {
                    state.as3_flag = true;
                }
            }
            72 | 82 => state.tags.do_abc += 1,
            _ => {}
        }
    }

    report.warning(
        "swf_truncated_tag_stream",
        format!("{location}#tags"),
        "SWF tag stream ended without a terminating End (0) tag; complete records were retained.",
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
