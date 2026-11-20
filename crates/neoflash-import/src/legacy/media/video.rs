//! Sequential decoder and FLV container muxer for binary FLA `CMediaVideoStream` payloads.
//! Wire-layout facts follow JPEXS FLA Convert `MediaPayloadRecordDecoder.video`;
//! this implementation is independent Rust with strict bounds checking and zero byte scanning.

use super::{BinaryMediaError, error, is_mpeg_frame_header};

const MAX_VIDEO_STREAM_BYTES: usize = 64 * 1024 * 1024;
const MAX_VIDEO_PACKETS: usize = 200_000;
const MAX_VIDEO_STRING_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum BinaryVideoPayload {
    /// ActionScript-controlled `Video` library item (`videoType == 0` / empty tracks) with no media bytes.
    EmptyViewport,
    /// Valid standalone video container (`video/x-flv`, `video/mp4`, `video/webm`, `video/ogg`, `video/avi`).
    Container {
        media_type: &'static str,
        extension: &'static str,
        bytes: Vec<u8>,
    },
    /// Proprietary embedded codec without a standalone container/transcoder (e.g. raw VP6 alpha).
    UnsupportedCodec(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedBinaryVideo {
    pub revision: u8,
    pub video_type: u32,
    pub width: f64,
    pub height: f64,
    pub fps: f64,
    pub duration_seconds: f64,
    pub import_audio: bool,
    pub sync_video: bool,
    pub payload: BinaryVideoPayload,
}

#[derive(Clone, Copy, Debug)]
struct VideoPacketIndex {
    offset: usize,
    len: usize,
    frame_count: u32,
    flags: i32,
}

struct ParsedTrack<'a> {
    kind: u32,
    indices: Vec<VideoPacketIndex>,
    data: &'a [u8],
}

struct VideoReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> VideoReader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], BinaryMediaError> {
        let end = self
            .at
            .checked_add(size)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| {
                error(
                    "fla_video_truncated",
                    self.at,
                    "Video media field exceeds remaining stream bytes.",
                )
            })?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, BinaryMediaError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, BinaryMediaError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, BinaryMediaError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, BinaryMediaError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, BinaryMediaError> {
        let pos = self.at;
        let value = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if !value.is_finite() {
            return Err(error(
                "fla_video_nonfinite",
                pos,
                "Nonfinite floating-point value in video stream header.",
            ));
        }
        Ok(value)
    }

    fn cstring_len(&mut self) -> Result<Option<u32>, BinaryMediaError> {
        let first = self.u8()?;
        if first < 0xff {
            return Ok(Some(u32::from(first)));
        }
        match self.u16()? {
            0xfffe => Ok(None),
            0xffff => Ok(Some(self.u32()?)),
            other => Ok(Some(u32::from(other))),
        }
    }

    fn skip_cstring(&mut self) -> Result<(), BinaryMediaError> {
        let pos = self.at;
        let first = self.cstring_len()?;
        let (utf16, units) = match first {
            Some(n) => (false, n),
            None => {
                let Some(n) = self.cstring_len()? else {
                    return Err(error(
                        "fla_video_string",
                        pos,
                        "Repeated Unicode CString marker in video stream.",
                    ));
                };
                (true, n)
            }
        };
        let count = if units == u32::MAX { 0 } else { units as usize };
        let byte_len = count
            .checked_mul(if utf16 { 2 } else { 1 })
            .ok_or_else(|| error("fla_video_string", pos, "Video CString length overflow."))?;
        if byte_len > MAX_VIDEO_STRING_BYTES {
            return Err(error(
                "fla_video_string",
                pos,
                "Video CString exceeds 1 MiB.",
            ));
        }
        self.take(byte_len)?;
        Ok(())
    }

    fn track(&mut self) -> Result<ParsedTrack<'a>, BinaryMediaError> {
        let kind = self.u32()?;
        let mut indices = Vec::new();
        loop {
            let marker = self.u8()?;
            if marker == 0 {
                break;
            }
            if indices.len() >= MAX_VIDEO_PACKETS {
                return Err(error(
                    "fla_video_limit",
                    self.at,
                    "Video packet index count exceeds 200000.",
                ));
            }
            let pos = self.at;
            let offset_raw = self.i32()?;
            let count_raw = self.i32()?;
            let frame_raw = self.i32()?;
            let flags = self.i32()?;
            if offset_raw < 0 || count_raw < 0 {
                return Err(error(
                    "fla_video_index",
                    pos,
                    "Negative packet offset or byte length in video track.",
                ));
            }
            indices.push(VideoPacketIndex {
                offset: offset_raw as usize,
                len: count_raw as usize,
                frame_count: frame_raw.max(1) as u32,
                flags,
            });
        }
        let data_pos = self.at;
        let data_len = self.u32()? as usize;
        let data = self.take(data_len)?;
        for pkt in &indices {
            if pkt
                .offset
                .checked_add(pkt.len)
                .is_none_or(|end| end > data.len())
            {
                return Err(error(
                    "fla_video_index",
                    data_pos,
                    "Video packet slice exceeds track buffer.",
                ));
            }
        }
        Ok(ParsedTrack {
            kind,
            indices,
            data,
        })
    }
}

fn push_amf0_key(out: &mut Vec<u8>, key: &str) {
    out.extend_from_slice(&(key.len() as u16).to_be_bytes());
    out.extend_from_slice(key.as_bytes());
}

fn push_amf0_number_prop(out: &mut Vec<u8>, key: &str, value: f64) {
    push_amf0_key(out, key);
    out.push(0x00);
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_flv_tag(
    out: &mut Vec<u8>,
    tag_type: u8,
    timestamp_ms: u32,
    prefix: &[u8],
    payload: &[u8],
) -> Result<(), BinaryMediaError> {
    let data_size = prefix.len().saturating_add(payload.len());
    if data_size > 0x00ff_ffff
        || out.len().saturating_add(11 + data_size + 4) > neoflash_core::MAX_RESOURCE_BYTES
    {
        return Err(error(
            "fla_video_limit",
            0,
            "Muxed FLV video exceeds the 16 MiB single-resource limit.",
        ));
    }
    out.push(tag_type);
    out.push(((data_size >> 16) & 0xff) as u8);
    out.push(((data_size >> 8) & 0xff) as u8);
    out.push((data_size & 0xff) as u8);
    out.push(((timestamp_ms >> 16) & 0xff) as u8);
    out.push(((timestamp_ms >> 8) & 0xff) as u8);
    out.push((timestamp_ms & 0xff) as u8);
    out.push(((timestamp_ms >> 24) & 0xff) as u8);
    out.extend_from_slice(&[0, 0, 0]); // StreamID = 0
    out.extend_from_slice(prefix);
    out.extend_from_slice(payload);
    let prev_size = (11 + data_size) as u32;
    out.extend_from_slice(&prev_size.to_be_bytes());
    Ok(())
}

fn build_on_metadata_body(
    duration_secs: f64,
    width: f64,
    height: f64,
    fps: f64,
    codec_id: u8,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(128);
    // AMF0 string "onMetaData"
    body.push(0x02);
    push_amf0_key(&mut body, "onMetaData");
    // AMF0 ECMA array (5 properties)
    body.push(0x08);
    body.extend_from_slice(&5u32.to_be_bytes());
    push_amf0_number_prop(&mut body, "duration", duration_secs.max(0.0));
    push_amf0_number_prop(&mut body, "width", width);
    push_amf0_number_prop(&mut body, "height", height);
    push_amf0_number_prop(&mut body, "framerate", fps);
    push_amf0_number_prop(&mut body, "videocodecid", f64::from(codec_id));
    // Object end marker 00 00 09
    body.extend_from_slice(&[0x00, 0x00, 0x09]);
    body
}

fn audio_header_byte(
    first_pkt: &[u8],
    sample_rate: f64,
    bits_per_sample: i32,
    channels: i32,
) -> u8 {
    let is_mp3 = first_pkt.len() >= 4
        && is_mpeg_frame_header([first_pkt[0], first_pkt[1], first_pkt[2], first_pkt[3]]);
    let sound_format = if is_mp3 { 2u8 } else { 3u8 }; // 2 = MP3, 3 = Linear PCM LE
    let sound_rate = if sample_rate >= 33_000.0 {
        3u8
    } else if sample_rate >= 16_000.0 {
        2u8
    } else if sample_rate >= 8_000.0 {
        1u8
    } else {
        0u8
    };
    let sound_size = u8::from(bits_per_sample >= 16 || is_mp3);
    let sound_type = u8::from(channels >= 2);
    (sound_format << 4) | (sound_rate << 2) | (sound_size << 1) | sound_type
}

#[allow(clippy::too_many_arguments)]
fn mux_flv_container(
    video_type: u32,
    width: f64,
    height: f64,
    fps: f64,
    duration_seconds: f64,
    audio_sample_rate: f64,
    audio_bits_per_sample: i32,
    audio_channels: i32,
    video_track: &ParsedTrack<'_>,
    audio_track: Option<&ParsedTrack<'_>>,
) -> Result<Vec<u8>, BinaryMediaError> {
    let codec_id = video_type as u8;
    let safe_fps = if fps > 0.0 { fps } else { 24.0 };
    let has_audio = audio_track.is_some_and(|t| !t.indices.is_empty() && !t.data.is_empty());

    let mut flv = Vec::with_capacity(
        video_track
            .data
            .len()
            .saturating_add(audio_track.map_or(0, |a| a.data.len()))
            .saturating_add(256 + video_track.indices.len() * 16)
            .min(neoflash_core::MAX_RESOURCE_BYTES),
    );
    // 9-byte FLV v1 header + 4-byte PreviousTagSize0
    flv.extend_from_slice(b"FLV");
    flv.push(0x01);
    flv.push(0x01 | if has_audio { 0x04 } else { 0x00 });
    flv.extend_from_slice(&9u32.to_be_bytes());
    flv.extend_from_slice(&0u32.to_be_bytes());

    let computed_duration = if duration_seconds > 0.0 {
        duration_seconds
    } else {
        let total_frames: u64 = video_track
            .indices
            .iter()
            .map(|p| u64::from(p.frame_count))
            .sum();
        total_frames as f64 / safe_fps
    };
    let meta_body = build_on_metadata_body(computed_duration, width, height, safe_fps, codec_id);
    push_flv_tag(&mut flv, 18, 0, &[], &meta_body)?;

    // Build chronological packet schedule `(timestamp_ms, is_audio, index)`.
    let mut events =
        Vec::with_capacity(video_track.indices.len() + audio_track.map_or(0, |a| a.indices.len()));
    let mut v_frames = 0u64;
    for (idx, pkt) in video_track.indices.iter().enumerate() {
        let ts_ms = ((v_frames as f64 * 1000.0) / safe_fps)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        events.push((ts_ms, false, idx));
        v_frames = v_frames.saturating_add(u64::from(pkt.frame_count));
    }
    let mut audio_hdr = 0u8;
    if let Some(a_track) = audio_track.filter(|t| has_audio && !t.indices.is_empty()) {
        let first = &a_track.indices[0];
        audio_hdr = audio_header_byte(
            &a_track.data[first.offset..first.offset + first.len],
            audio_sample_rate,
            audio_bits_per_sample,
            audio_channels,
        );
        let mut a_frames = 0u64;
        for (idx, pkt) in a_track.indices.iter().enumerate() {
            let ts_ms = ((a_frames as f64 * 1000.0) / safe_fps)
                .round()
                .clamp(0.0, f64::from(u32::MAX)) as u32;
            events.push((ts_ms, true, idx));
            a_frames = a_frames.saturating_add(u64::from(pkt.frame_count));
        }
    }
    events.sort_by_key(|&(ts, is_audio, idx)| (ts, !is_audio, idx));

    for (ts_ms, is_audio, idx) in events {
        if is_audio {
            if let Some(a_track) = audio_track {
                let pkt = &a_track.indices[idx];
                let slice = &a_track.data[pkt.offset..pkt.offset + pkt.len];
                if !slice.is_empty() {
                    push_flv_tag(&mut flv, 8, ts_ms, &[audio_hdr], slice)?;
                }
            }
        } else {
            let pkt = &video_track.indices[idx];
            let slice = &video_track.data[pkt.offset..pkt.offset + pkt.len];
            let is_keyframe = idx == 0 || (pkt.flags & 1) != 0;
            let frame_type = if is_keyframe { 1u8 } else { 2u8 };
            let tag_hdr = (frame_type << 4) | (codec_id & 0x0f);
            match codec_id {
                // VP6FLVVideoPacket prepends 1 byte horizontal/vertical crop adjustment (`0x00`).
                4 => push_flv_tag(&mut flv, 9, ts_ms, &[tag_hdr, 0x00], slice)?,
                // AVCVideoPacket prepends AVCPacketType (`1` = NALU, `0` = seq header) + 24-bit CTO.
                7 => {
                    let avc_type = u8::from(!(idx == 0 && slice.starts_with(&[0x01])));
                    push_flv_tag(&mut flv, 9, ts_ms, &[tag_hdr, avc_type, 0, 0, 0], slice)?;
                }
                _ => push_flv_tag(&mut flv, 9, ts_ms, &[tag_hdr], slice)?,
            }
        }
    }

    Ok(flv)
}

fn sniff_bundled_video_container(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        ("video/mp4", "mp4")
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        ("video/webm", "webm")
    } else if bytes.starts_with(b"OggS") {
        ("video/ogg", "ogv")
    } else if bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && (&bytes[8..12] == b"AVI " || &bytes[8..12] == b"AVIX")
    {
        ("video/avi", "avi")
    } else if bytes.starts_with(b"FLV\x01") {
        ("video/x-flv", "flv")
    } else {
        ("video/mp4", "mp4")
    }
}

/// Sequentially decode a binary FLA `CMediaVideoStream` `Media *` stream.
pub fn decode_binary_video(bytes: &[u8]) -> Result<DecodedBinaryVideo, BinaryMediaError> {
    if bytes.is_empty() {
        return Err(error("fla_video_empty", 0, "Video media stream is empty."));
    }
    if bytes.len() > MAX_VIDEO_STREAM_BYTES {
        return Err(error(
            "fla_video_limit",
            0,
            "Video media stream exceeds 64 MiB.",
        ));
    }
    let mut r = VideoReader { bytes, at: 0 };
    let revision = r.u8()?;
    if revision > 3 {
        return Err(error(
            "fla_video_revision",
            0,
            "Video stream revision exceeds supported revision 3.",
        ));
    }
    let video_type = r.u32()?;
    let frame_rate = r.f64()?;
    let audio_sample_rate = r.f64()?;
    let audio_bits_per_sample = r.i32()?;
    let audio_channels = r.i32()?;
    let video_width = r.i32()?;
    let video_height = r.i32()?;
    let duration_seconds = r.f64()?;
    let _is_special = r.u32()?;
    let import_audio = r.u8()? != 0;
    let sync_video = r.u8()? != 0;
    let _drop_ratio_index = r.u32()?;
    let _quality = r.i32()?;
    let _scale_ratio = r.f64()?;
    let _key_frame_interval = r.u32()?;

    let track_a = r.track()?;
    let track_b = r.track()?;

    let mut bundled_bytes: &[u8] = &[];
    let mut has_bundled = false;
    if revision >= 3 {
        has_bundled = r.u32()? != 0;
        r.skip_cstring()?;
        let bundled_len = r.u32()? as usize;
        bundled_bytes = r.take(bundled_len)?;
    }

    if r.at != bytes.len() {
        return Err(error(
            "fla_video_trailing",
            r.at,
            "Trailing bytes remain after video stream payload.",
        ));
    }

    let width = if video_width > 0 {
        f64::from(video_width).clamp(1.0, 8192.0)
    } else {
        160.0
    };
    let height = if video_height > 0 {
        f64::from(video_height).clamp(1.0, 8192.0)
    } else {
        120.0
    };
    let fps = if frame_rate > 0.0 {
        frame_rate.clamp(0.1, 1000.0)
    } else {
        24.0
    };

    let (video_track, audio_track) = if track_a.kind == 1 && track_b.kind == 0 {
        (&track_b, &track_a)
    } else {
        (&track_a, &track_b)
    };

    let payload = if has_bundled && !bundled_bytes.is_empty() {
        if bundled_bytes.len() > neoflash_core::MAX_RESOURCE_BYTES {
            return Err(error(
                "fla_video_limit",
                0,
                "Bundled video exceeds the 16 MiB single-resource limit.",
            ));
        }
        let (media_type, extension) = sniff_bundled_video_container(bundled_bytes);
        BinaryVideoPayload::Container {
            media_type,
            extension,
            bytes: bundled_bytes.to_vec(),
        }
    } else if video_type == 0 || (video_track.indices.is_empty() && audio_track.indices.is_empty())
    {
        BinaryVideoPayload::EmptyViewport
    } else if matches!(video_type, 2 | 3 | 4 | 6 | 7) {
        let flv = mux_flv_container(
            video_type,
            width,
            height,
            fps,
            duration_seconds,
            audio_sample_rate,
            audio_bits_per_sample,
            audio_channels,
            video_track,
            import_audio.then_some(audio_track),
        )?;
        BinaryVideoPayload::Container {
            media_type: "video/x-flv",
            extension: "flv",
            bytes: flv,
        }
    } else if video_type == 5 {
        BinaryVideoPayload::UnsupportedCodec(
            "On2 VP6 alpha video streams cannot be transcoded in pure Rust",
        )
    } else if video_type == 1 {
        BinaryVideoPayload::UnsupportedCodec(
            "Legacy Flash JPEG packet video streams are not supported as standalone containers",
        )
    } else {
        BinaryVideoPayload::UnsupportedCodec("Unrecognized embedded Flash video codec")
    };

    Ok(DecodedBinaryVideo {
        revision,
        video_type,
        width,
        height,
        fps,
        duration_seconds: duration_seconds.max(0.0),
        import_audio,
        sync_video,
        payload,
    })
}
