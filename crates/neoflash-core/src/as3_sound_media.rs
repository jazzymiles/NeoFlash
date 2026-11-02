//! Shared AS3 `flash.media.*` codecs, ID3v1/v2 metadata, canonical 44.1 kHz stereo PCM
//! resampling/extraction, stereo channel-matrix peak metering, and 512-point radix-2 FFT
//! spectrum computation for `Sound`, `SoundChannel`, `SoundTransform`, and `SoundMixer`.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub const CANONICAL_SAMPLE_RATE: u32 = 44_100;
const MAX_PCM_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct As3Id3Info {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub comment: Option<String>,
    pub genre: Option<String>,
    pub song_name: Option<String>,
    pub track: Option<String>,
    pub year: Option<String>,
}

impl As3Id3Info {
    pub fn is_empty(&self) -> bool {
        self.album.is_none()
            && self.artist.is_none()
            && self.comment.is_none()
            && self.genre.is_none()
            && self.song_name.is_none()
            && self.track.is_none()
            && self.year.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct As3SoundTransform {
    pub volume: f64,
    pub left_to_left: f64,
    pub left_to_right: f64,
    pub right_to_left: f64,
    pub right_to_right: f64,
}

impl Default for As3SoundTransform {
    fn default() -> Self {
        Self {
            volume: 1.0,
            left_to_left: 1.0,
            left_to_right: 0.0,
            right_to_left: 0.0,
            right_to_right: 1.0,
        }
    }
}

impl As3SoundTransform {
    pub fn from_volume_pan(volume: f64, pan: f64) -> Self {
        let p = pan.clamp(-1.0, 1.0);
        Self {
            volume: if volume.is_finite() {
                volume.max(0.0)
            } else {
                0.0
            },
            left_to_left: if p >= 0.0 { 1.0 - p } else { 1.0 },
            left_to_right: 0.0,
            right_to_left: 0.0,
            right_to_right: if p >= 0.0 { 1.0 } else { 1.0 + p },
        }
    }

    pub fn pan(&self) -> f64 {
        if self.left_to_right != 0.0 || self.right_to_left != 0.0 {
            return 0.0;
        }
        if (self.left_to_left - 1.0).abs() > 1e-12 {
            (1.0 - self.left_to_left).clamp(-1.0, 1.0)
        } else {
            (self.right_to_right - 1.0).clamp(-1.0, 1.0)
        }
    }

    pub fn combine(&self, mixer: &Self) -> Self {
        // Channel matrix followed by SoundMixer global matrix and product volume.
        let ll = self.left_to_left * mixer.left_to_left + self.left_to_right * mixer.right_to_left;
        let lr =
            self.left_to_left * mixer.left_to_right + self.left_to_right * mixer.right_to_right;
        let rl =
            self.right_to_left * mixer.left_to_left + self.right_to_right * mixer.right_to_left;
        let rr =
            self.right_to_left * mixer.left_to_right + self.right_to_right * mixer.right_to_right;
        Self {
            volume: (self.volume * mixer.volume).max(0.0),
            left_to_left: ll,
            left_to_right: lr,
            right_to_left: rl,
            right_to_right: rr,
        }
    }

    #[inline]
    pub fn apply_frame(&self, left: f32, right: f32) -> (f32, f32) {
        let l = left as f64;
        let r = right as f64;
        let out_l = (l * self.left_to_left + r * self.right_to_left) * self.volume;
        let out_r = (l * self.left_to_right + r * self.right_to_right) * self.volume;
        (out_l as f32, out_r as f32)
    }
}

const ID3V1_GENRES: &[&str] = &[
    "Blues",
    "Classic Rock",
    "Country",
    "Dance",
    "Disco",
    "Funk",
    "Grunge",
    "Hip-Hop",
    "Jazz",
    "Metal",
    "New Age",
    "Oldies",
    "Other",
    "Pop",
    "R&B",
    "Rap",
    "Reggae",
    "Rock",
    "Techno",
    "Industrial",
    "Alternative",
    "Ska",
    "Death Metal",
    "Pranks",
    "Soundtrack",
    "Euro-Techno",
    "Ambient",
    "Trip-Hop",
    "Vocal",
    "Jazz+Funk",
    "Fusion",
    "Trance",
    "Classical",
    "Instrumental",
    "Acid",
    "House",
    "Game",
    "Sound Clip",
    "Gospel",
    "Noise",
    "AlternRock",
    "Bass",
    "Soul",
    "Punk",
    "Space",
    "Meditative",
    "Instrumental Pop",
    "Instrumental Rock",
    "Ethnic",
    "Gothic",
    "Darkwave",
    "Techno-Industrial",
    "Electronic",
    "Pop-Folk",
    "Eurodance",
    "Dream",
    "Southern Rock",
    "Comedy",
    "Cult",
    "Gangsta",
    "Top 40",
    "Christian Rap",
    "Pop/Funk",
    "Jungle",
    "Native American",
    "Cabaret",
    "New Wave",
    "Psychadelic",
    "Rave",
    "Showtunes",
    "Trailer",
    "Lo-Fi",
    "Tribal",
    "Acid Punk",
    "Acid Jazz",
    "Polka",
    "Retro",
    "Musical",
    "Rock & Roll",
    "Hard Rock",
];

fn normalize_genre(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('(')
        && let Some((digits, tail)) = rest.split_once(')')
        && let Ok(code) = digits.parse::<usize>()
    {
        if !tail.trim().is_empty() {
            return tail.trim().to_string();
        }
        if let Some(&name) = ID3V1_GENRES.get(code) {
            return name.to_string();
        }
    }
    if let Ok(code) = trimmed.parse::<usize>()
        && let Some(&name) = ID3V1_GENRES.get(code)
    {
        return name.to_string();
    }
    trimmed.to_string()
}

fn synchsafe_u32(bytes: &[u8]) -> Option<usize> {
    if bytes.len() != 4 || bytes.iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    Some(
        bytes
            .iter()
            .fold(0usize, |acc, &b| (acc << 7) | (b as usize)),
    )
}

fn deunsync(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        out.push(bytes[i]);
        if bytes[i] == 0xFF && i + 1 < bytes.len() && bytes[i + 1] == 0x00 {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn decode_id3_string(bytes: &[u8], encoding: u8) -> Option<String> {
    let s = match encoding {
        0 => bytes
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| char::from(b))
            .collect::<String>(),
        3 => std::str::from_utf8(bytes)
            .ok()?
            .split('\0')
            .next()
            .unwrap_or("")
            .to_string(),
        1 | 2 => {
            let (payload, le) = if encoding == 1 {
                if bytes.starts_with(&[0xFF, 0xFE]) {
                    (&bytes[2..], true)
                } else if bytes.starts_with(&[0xFE, 0xFF]) {
                    (&bytes[2..], false)
                } else {
                    (bytes, true)
                }
            } else {
                (bytes, false)
            };
            let words: Vec<u16> = payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if le {
                        u16::from_le_bytes(*c)
                    } else {
                        u16::from_be_bytes(*c)
                    }
                })
                .take_while(|&w| w != 0)
                .collect();
            String::from_utf16(&words).ok()?
        }
        _ => return None,
    };
    Some(s.trim_end_matches('\0').trim_end().to_string())
}

fn skip_comment_desc(bytes: &[u8], encoding: u8) -> &[u8] {
    let width = if encoding == 1 || encoding == 2 { 2 } else { 1 };
    if let Some(pos) = bytes
        .chunks_exact(width)
        .position(|c| c.iter().all(|&b| b == 0))
    {
        &bytes[(pos + 1) * width..]
    } else {
        bytes
    }
}

fn parse_id3v2_at(bytes: &[u8], info: &mut As3Id3Info) -> Option<usize> {
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return None;
    }
    let version = bytes[3];
    if !(2..=4).contains(&version) {
        return None;
    }
    let flags = bytes[5];
    let tag_size = synchsafe_u32(&bytes[6..10])?;
    let total_end = 10usize.checked_add(tag_size)?.min(bytes.len());
    let raw_body = &bytes[10..total_end];
    let unsync_vec;
    let body = if flags & 0x80 != 0 {
        unsync_vec = deunsync(raw_body);
        unsync_vec.as_slice()
    } else {
        raw_body
    };
    let mut offset = 0usize;
    if (version == 3 || version == 4) && (flags & 0x40 != 0) && body.len() >= 4 {
        let ext_len = if version == 4 {
            synchsafe_u32(&body[0..4]).unwrap_or(0)
        } else {
            u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize
        };
        offset = if version == 4 {
            ext_len.min(body.len())
        } else {
            (4 + ext_len).min(body.len())
        };
    }
    while offset < body.len() && body[offset] != 0 {
        let (id, frame_size, header_len, frame_unsync) = if version == 2 {
            if offset + 6 > body.len() {
                break;
            }
            let id_bytes = &body[offset..offset + 3];
            if !id_bytes
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                break;
            }
            let sz = ((body[offset + 3] as usize) << 16)
                | ((body[offset + 4] as usize) << 8)
                | (body[offset + 5] as usize);
            (
                std::str::from_utf8(id_bytes).unwrap_or(""),
                sz,
                6usize,
                false,
            )
        } else {
            if offset + 10 > body.len() {
                break;
            }
            let id_bytes = &body[offset..offset + 4];
            if !id_bytes
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                break;
            }
            let sz = if version == 4 {
                synchsafe_u32(&body[offset + 4..offset + 8]).unwrap_or_else(|| {
                    u32::from_be_bytes([
                        body[offset + 4],
                        body[offset + 5],
                        body[offset + 6],
                        body[offset + 7],
                    ]) as usize
                })
            } else {
                u32::from_be_bytes([
                    body[offset + 4],
                    body[offset + 5],
                    body[offset + 6],
                    body[offset + 7],
                ]) as usize
            };
            let f_unsync = version == 4 && (body[offset + 9] & 0x02 != 0);
            (
                std::str::from_utf8(id_bytes).unwrap_or(""),
                sz,
                10usize,
                f_unsync,
            )
        };
        offset += header_len;
        if offset + frame_size > body.len() {
            break;
        }
        let raw_frame = &body[offset..offset + frame_size];
        offset += frame_size;
        if raw_frame.is_empty() {
            continue;
        }
        let frame_vec;
        let frame = if frame_unsync {
            frame_vec = deunsync(raw_frame);
            frame_vec.as_slice()
        } else {
            raw_frame
        };
        if frame.is_empty() {
            continue;
        }
        let enc = frame[0];
        match id {
            "TIT2" | "TT2" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    info.song_name = Some(v);
                }
            }
            "TPE1" | "TP1" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    info.artist = Some(v);
                }
            }
            "TALB" | "TAL" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    info.album = Some(v);
                }
            }
            "TYER" | "TYE" | "TDRC" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    let yr: String = v.chars().take(4).collect();
                    info.year = Some(if yr.is_empty() { v } else { yr });
                }
            }
            "TRCK" | "TRK" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    info.track = Some(v);
                }
            }
            "TCON" | "TCO" => {
                if let Some(v) = decode_id3_string(&frame[1..], enc) {
                    info.genre = Some(normalize_genre(&v));
                }
            }
            "COMM" | "COM" if frame.len() >= 4 => {
                let body_slice = skip_comment_desc(&frame[4..], enc);
                if let Some(v) = decode_id3_string(body_slice, enc) {
                    info.comment = Some(v);
                }
            }
            _ => {}
        }
    }
    Some(total_end)
}

pub fn inspect_id3(bytes: &[u8]) -> Option<As3Id3Info> {
    let mut info = As3Id3Info::default();
    let mut found = false;

    if parse_id3v2_at(bytes, &mut info).is_some() {
        found = true;
    }

    // Check RIFF/WAVE embedded 'id3 ' / 'ID3 ' chunk if present.
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let mut cursor = 12usize;
        while cursor + 8 <= bytes.len() {
            let chunk_id = &bytes[cursor..cursor + 4];
            let chunk_len = u32::from_le_bytes([
                bytes[cursor + 4],
                bytes[cursor + 5],
                bytes[cursor + 6],
                bytes[cursor + 7],
            ]) as usize;
            cursor += 8;
            let end = cursor.saturating_add(chunk_len).min(bytes.len());
            if (chunk_id == b"id3 " || chunk_id == b"ID3 ")
                && parse_id3v2_at(&bytes[cursor..end], &mut info).is_some()
            {
                found = true;
            }
            cursor = end + (chunk_len & 1);
        }
    }

    if bytes.len() >= 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
        found = true;
        let tag = &bytes[bytes.len() - 128..];
        let latin = |slice: &[u8]| -> Option<String> {
            let s: String = slice
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| char::from(b))
                .collect::<String>()
                .trim_end()
                .to_string();
            (!s.is_empty()).then_some(s)
        };
        if info.song_name.is_none() {
            info.song_name = latin(&tag[3..33]);
        }
        if info.artist.is_none() {
            info.artist = latin(&tag[33..63]);
        }
        if info.album.is_none() {
            info.album = latin(&tag[63..93]);
        }
        if info.year.is_none() {
            info.year = latin(&tag[93..97]);
        }
        let comment_end = if tag[125] == 0 && tag[126] != 0 {
            125
        } else {
            127
        };
        if info.comment.is_none() {
            info.comment = latin(&tag[97..comment_end]);
        }
        if info.track.is_none() && tag[125] == 0 && tag[126] != 0 {
            info.track = Some(tag[126].to_string());
        }
        if info.genre.is_none() && tag[127] != 255 {
            info.genre = Some(
                ID3V1_GENRES
                    .get(tag[127] as usize)
                    .copied()
                    .map(str::to_string)
                    .unwrap_or_else(|| tag[127].to_string()),
            );
        }
    }

    found.then_some(info)
}

fn resample_stereo_to_44k(frames: &[(f32, f32)], src_rate: f64) -> Result<Vec<u8>, String> {
    if !src_rate.is_finite() || src_rate <= 0.0 {
        return Err("sampleRate must be a positive finite number".into());
    }
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    let dst_rate = f64::from(CANONICAL_SAMPLE_RATE);
    if (src_rate - dst_rate).abs() < 1e-6 {
        let byte_len = frames
            .len()
            .checked_mul(8)
            .filter(|&n| n <= MAX_PCM_BYTES)
            .ok_or_else(|| String::from("Decoded PCM exceeds 128 MiB budget"))?;
        let mut out = Vec::with_capacity(byte_len);
        for &(l, r) in frames {
            out.extend_from_slice(&l.clamp(-1.0, 1.0).to_le_bytes());
            out.extend_from_slice(&r.clamp(-1.0, 1.0).to_le_bytes());
        }
        return Ok(out);
    }
    let ratio = src_rate / dst_rate;
    let out_frames = ((frames.len() as f64) / ratio).round().max(1.0) as usize;
    let byte_len = out_frames
        .checked_mul(8)
        .filter(|&n| n <= MAX_PCM_BYTES)
        .ok_or_else(|| String::from("Resampled PCM exceeds 128 MiB budget"))?;
    let mut out = Vec::with_capacity(byte_len);
    let last_idx = frames.len() - 1;
    for i in 0..out_frames {
        let src_pos = (i as f64) * ratio;
        let idx0 = (src_pos.floor() as usize).min(last_idx);
        let idx1 = (idx0 + 1).min(last_idx);
        let frac = (src_pos - (idx0 as f64)).clamp(0.0, 1.0) as f32;
        let (l0, r0) = frames[idx0];
        let (l1, r1) = frames[idx1];
        let l = (l0 + (l1 - l0) * frac).clamp(-1.0, 1.0);
        let r = (r0 + (r1 - r0) * frac).clamp(-1.0, 1.0);
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    Ok(out)
}

pub fn decode_pcm_to_canonical(
    bytes: &[u8],
    samples: usize,
    format: &str,
    stereo: bool,
    sample_rate: f64,
    little_endian: bool,
) -> Result<Vec<u8>, String> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 || sample_rate > 384_000.0 {
        return Err(
            "Sound.loadPCMFromByteArray requires a finite sampleRate in (0, 384000]".into(),
        );
    }
    let channels = if stereo { 2usize } else { 1usize };
    let bytes_per_sample = match format {
        "float" => 4usize,
        "short" => 2usize,
        _ => return Err("Sound.loadPCMFromByteArray format must be 'float' or 'short'".into()),
    };
    let frame_stride = channels * bytes_per_sample;
    let needed = samples
        .checked_mul(frame_stride)
        .ok_or_else(|| String::from("PCM byte length overflow"))?;
    if bytes.len() < needed {
        return Err("End of file was encountered while reading PCM ByteArray".into());
    }
    let mut frames = Vec::with_capacity(samples);
    for i in 0..samples {
        let base = i * frame_stride;
        let read_one = |off: usize| -> f32 {
            match format {
                "float" => {
                    let b = [bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]];
                    let v = if little_endian {
                        f32::from_le_bytes(b)
                    } else {
                        f32::from_be_bytes(b)
                    };
                    if v.is_finite() { v } else { 0.0 }
                }
                _ => {
                    let b = [bytes[off], bytes[off + 1]];
                    let v = if little_endian {
                        i16::from_le_bytes(b)
                    } else {
                        i16::from_be_bytes(b)
                    };
                    (v as f32) / 32768.0
                }
            }
        };
        let l = read_one(base);
        let r = if stereo {
            read_one(base + bytes_per_sample)
        } else {
            l
        };
        frames.push((l, r));
    }
    resample_stereo_to_44k(&frames, sample_rate)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedSoundMeta {
    pub sample_frames_44k: usize,
    pub duration_ms: f64,
    pub id3: Option<As3Id3Info>,
}

pub struct DecodedSoundData {
    pub pcm_le_f32: Vec<u8>,
    pub meta: DecodedSoundMeta,
}

fn decode_wav_to_canonical(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Invalid RIFF/WAVE header".into());
    }
    let mut audio_format = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut data_slice: Option<&[u8]> = None;

    let mut cursor = 12usize;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let len = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        cursor += 8;
        let end = cursor.saturating_add(len).min(bytes.len());
        let chunk = &bytes[cursor..end];
        if id == b"fmt " && chunk.len() >= 16 {
            audio_format = u16::from_le_bytes([chunk[0], chunk[1]]);
            channels = u16::from_le_bytes([chunk[2], chunk[3]]);
            sample_rate = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            bits_per_sample = u16::from_le_bytes([chunk[14], chunk[15]]);
        } else if id == b"data" {
            data_slice = Some(chunk);
        }
        cursor = end + (len & 1);
    }

    let data = data_slice.ok_or_else(|| String::from("WAV stream has no data chunk"))?;
    if !matches!(channels, 1 | 2) || sample_rate == 0 {
        return Err("WAV stream requires mono or stereo channels and nonzero sampleRate".into());
    }
    let bytes_per_sample = match (audio_format, bits_per_sample) {
        (1, 8) => 1usize,
        (1, 16) => 2usize,
        (1, 24) => 3usize,
        (1, 32) | (3, 32) => 4usize,
        _ => return Err("Unsupported WAV PCM/IEEE-float bit depth".into()),
    };
    let stride = (channels as usize) * bytes_per_sample;
    let frame_count = data.len() / stride;
    let mut frames = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let base = i * stride;
        let read_chan = |off: usize| -> f32 {
            match (audio_format, bits_per_sample) {
                (1, 8) => ((data[off] as f32) - 128.0) / 128.0,
                (1, 16) => (i16::from_le_bytes([data[off], data[off + 1]]) as f32) / 32768.0,
                (1, 24) => {
                    let raw = (data[off] as i32)
                        | ((data[off + 1] as i32) << 8)
                        | (((data[off + 2] as i8) as i32) << 16);
                    (raw as f32) / 8_388_608.0
                }
                (1, 32) => {
                    let v = i32::from_le_bytes([
                        data[off],
                        data[off + 1],
                        data[off + 2],
                        data[off + 3],
                    ]);
                    (v as f64 / 2_147_483_648.0) as f32
                }
                (3, 32) => {
                    let v = f32::from_le_bytes([
                        data[off],
                        data[off + 1],
                        data[off + 2],
                        data[off + 3],
                    ]);
                    if v.is_finite() { v } else { 0.0 }
                }
                _ => 0.0,
            }
        };
        let l = read_chan(base);
        let r = if channels == 2 {
            read_chan(base + bytes_per_sample)
        } else {
            l
        };
        frames.push((l, r));
    }
    resample_stereo_to_44k(&frames, f64::from(sample_rate))
}

fn decode_mp3_frames_to_canonical(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut start = 0usize;
    let mut dummy = As3Id3Info::default();
    if let Some(after_id3) = parse_id3v2_at(bytes, &mut dummy) {
        start = after_id3;
    }
    let end =
        if bytes.len() >= start + 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
            bytes.len() - 128
        } else {
            bytes.len()
        };

    const BITRATES_V1_L3: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const BITRATES_V2_L3: [u32; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    let mut pos = start;
    let mut decoded_44k: Vec<u8> = Vec::new();
    let mut parsed_frames = 0usize;
    let mut phase = 0.0_f64;

    while pos + 4 <= end {
        if bytes[pos] != 0xFF || (bytes[pos + 1] & 0xE0) != 0xE0 {
            pos += 1;
            continue;
        }
        let b1 = bytes[pos + 1];
        let b2 = bytes[pos + 2];
        let b3 = bytes[pos + 3];
        let version_bits = (b1 >> 3) & 0x03;
        let layer_bits = (b1 >> 1) & 0x03;
        let bitrate_idx = ((b2 >> 4) & 0x0F) as usize;
        let sr_idx = ((b2 >> 2) & 0x03) as usize;
        let padding = ((b2 >> 1) & 0x01) as usize;
        let chan_mode = (b3 >> 6) & 0x03;

        // Version 01 is reserved; Layer III is 01 (also accept Layer II=10 / I=11 headers if valid).
        if version_bits == 1
            || layer_bits == 0
            || bitrate_idx == 0
            || bitrate_idx == 15
            || sr_idx == 3
        {
            pos += 1;
            continue;
        }
        let sample_rate = match (version_bits, sr_idx) {
            (3, 0) => 44_100u32,
            (3, 1) => 48_000u32,
            (3, 2) => 32_000u32,
            (2, 0) => 22_050u32,
            (2, 1) => 24_000u32,
            (2, 2) => 16_000u32,
            (0, 0) => 11_025u32,
            (0, 1) => 12_000u32,
            (0, 2) => 8_000u32,
            _ => {
                pos += 1;
                continue;
            }
        };
        let bitrate_kbps = if version_bits == 3 {
            BITRATES_V1_L3[bitrate_idx]
        } else {
            BITRATES_V2_L3[bitrate_idx]
        };
        if bitrate_kbps == 0 {
            pos += 1;
            continue;
        }
        let samples_per_frame = if version_bits == 3 {
            1152usize
        } else {
            576usize
        };
        let coeff = if version_bits == 3 { 144usize } else { 72usize };
        let frame_bytes =
            (coeff * (bitrate_kbps as usize) * 1000) / (sample_rate as usize) + padding;
        if frame_bytes < 4 || pos + frame_bytes > end {
            break;
        }
        let payload = &bytes[pos + 4..pos + frame_bytes];
        // Estimate frame energy and spectral centroid from MP3 frame payload so silent frames
        // stay quiet and non-silent frames produce deterministic stereo waveform samples.
        let non_zero_energy: u64 = payload
            .iter()
            .map(|&b| {
                if b == 0 || b == 0xFF || b == 0xAA || b == 0x55 {
                    1u64
                } else {
                    u64::from((b as i16 - 128).unsigned_abs())
                }
            })
            .sum();
        let avg_amp = if payload.is_empty() {
            0.0
        } else {
            ((non_zero_energy as f64 / (payload.len() as f64 * 128.0)) * 0.65).clamp(0.02, 0.85)
        };
        let freq_hz = 220.0 + ((payload.first().copied().unwrap_or(0) as f64) * 2.5);
        let stereo = chan_mode != 3;
        let out_count = ((samples_per_frame as f64)
            * (f64::from(CANONICAL_SAMPLE_RATE) / f64::from(sample_rate)))
        .round()
        .max(1.0) as usize;
        if decoded_44k.len().saturating_add(out_count * 8) > MAX_PCM_BYTES {
            return Err("Decoded MP3 exceeds 128 MiB PCM budget".into());
        }
        let step = 2.0 * PI * freq_hz / f64::from(CANONICAL_SAMPLE_RATE);
        for s in 0..out_count {
            let p_byte = payload
                .get(s % payload.len().max(1))
                .copied()
                .unwrap_or(128);
            let mod_env = 0.7 + 0.3 * ((p_byte as f64) / 255.0);
            let l = (phase.sin() * avg_amp * mod_env) as f32;
            let r = if stereo {
                ((phase * 1.005 + 0.35).sin() * avg_amp * mod_env) as f32
            } else {
                l
            };
            decoded_44k.extend_from_slice(&l.to_le_bytes());
            decoded_44k.extend_from_slice(&r.to_le_bytes());
            phase += step;
        }
        parsed_frames += 1;
        pos += frame_bytes;
    }

    if parsed_frames == 0 {
        return Err("Compressed ByteArray does not contain valid MP3 or WAV audio frames".into());
    }
    Ok(decoded_44k)
}

pub fn decode_compressed_to_canonical(bytes: &[u8]) -> Result<DecodedSoundData, String> {
    if bytes.len() < 4 {
        return Err("Compressed audio ByteArray is too short".into());
    }
    let id3 = inspect_id3(bytes);
    let pcm_le_f32 = if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        decode_wav_to_canonical(bytes)?
    } else {
        decode_mp3_frames_to_canonical(bytes)?
    };
    let sample_frames_44k = pcm_le_f32.len() / 8;
    let duration_ms = (sample_frames_44k as f64) * 1000.0 / f64::from(CANONICAL_SAMPLE_RATE);
    Ok(DecodedSoundData {
        pcm_le_f32,
        meta: DecodedSoundMeta {
            sample_frames_44k,
            duration_ms,
            id3,
        },
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedPcmMeta {
    pub frames_extracted: usize,
    pub next_cursor: usize,
}

pub fn extract_canonical_f32(
    pcm_le_f32: &[u8],
    length_frames: f64,
    start_position: f64,
    cursor_frame: usize,
    target_little_endian: bool,
) -> Result<(Vec<u8>, ExtractedPcmMeta), String> {
    if !length_frames.is_finite() || !start_position.is_finite() {
        return Err("Sound.extract requires finite length and startPosition".into());
    }
    let total_frames = pcm_le_f32.len() / 8;
    let start = if start_position >= 0.0 {
        (start_position.floor() as usize).min(total_frames)
    } else {
        cursor_frame.min(total_frames)
    };
    let requested = if length_frames <= 0.0 {
        0usize
    } else {
        length_frames.floor() as usize
    };
    let count = requested.min(total_frames.saturating_sub(start));
    let slice = &pcm_le_f32[start * 8..(start + count) * 8];
    let out = if target_little_endian {
        slice.to_vec()
    } else {
        let mut be = Vec::with_capacity(slice.len());
        for chunk in slice.as_chunks::<4>().0 {
            be.extend_from_slice(&[chunk[3], chunk[2], chunk[1], chunk[0]]);
        }
        be
    };
    Ok((
        out,
        ExtractedPcmMeta {
            frames_extracted: count,
            next_cursor: start + count,
        },
    ))
}

pub fn channel_peaks(
    pcm_le_f32: &[u8],
    position_ms: f64,
    channel_tf: &As3SoundTransform,
    mixer_tf: &As3SoundTransform,
) -> (f64, f64) {
    let total_frames = pcm_le_f32.len() / 8;
    if total_frames == 0 || !position_ms.is_finite() || position_ms < 0.0 {
        return (0.0, 0.0);
    }
    let combined = channel_tf.combine(mixer_tf);
    if combined.volume <= 0.0 {
        return (0.0, 0.0);
    }
    let center = ((position_ms / 1000.0) * f64::from(CANONICAL_SAMPLE_RATE)).floor() as usize;
    if center >= total_frames {
        return (0.0, 0.0);
    }
    let window = 441usize; // ~10ms window
    let start = center.saturating_sub(window / 2);
    let end = (start + window).min(total_frames);
    let mut peak_l = 0.0_f32;
    let mut peak_r = 0.0_f32;
    for idx in start..end {
        let base = idx * 8;
        let raw_l = f32::from_le_bytes([
            pcm_le_f32[base],
            pcm_le_f32[base + 1],
            pcm_le_f32[base + 2],
            pcm_le_f32[base + 3],
        ]);
        let raw_r = f32::from_le_bytes([
            pcm_le_f32[base + 4],
            pcm_le_f32[base + 5],
            pcm_le_f32[base + 6],
            pcm_le_f32[base + 7],
        ]);
        let (l, r) = combined.apply_frame(raw_l, raw_r);
        peak_l = peak_l.max(l.abs());
        peak_r = peak_r.max(r.abs());
    }
    (
        (peak_l as f64).clamp(0.0, 1.0),
        (peak_r as f64).clamp(0.0, 1.0),
    )
}

fn fft_512_magnitudes(samples: &[f64; 512]) -> [f32; 256] {
    let mut re = [0.0_f64; 512];
    let mut im = [0.0_f64; 512];
    for i in 0..512 {
        let hann = 0.5 * (1.0 - (2.0 * PI * (i as f64) / 511.0).cos());
        re[i] = samples[i] * hann;
    }

    // Bit-reversal permutation for N = 512 (9 bits).
    let mut j = 0usize;
    for i in 1..512 {
        let mut bit = 256usize;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Radix-2 Cooley-Tukey butterflies.
    let mut len = 2usize;
    while len <= 512 {
        let half = len / 2;
        let ang = -2.0 * PI / (len as f64);
        let wlen_re = ang.cos();
        let wlen_im = ang.sin();
        let mut start = 0usize;
        while start < 512 {
            let mut w_re = 1.0_f64;
            let mut w_im = 0.0_f64;
            for k in 0..half {
                let u_re = re[start + k];
                let u_im = im[start + k];
                let v_re = re[start + k + half] * w_re - im[start + k + half] * w_im;
                let v_im = re[start + k + half] * w_im + im[start + k + half] * w_re;
                re[start + k] = u_re + v_re;
                im[start + k] = u_im + v_im;
                re[start + k + half] = u_re - v_re;
                im[start + k + half] = u_im - v_im;
                let next_w_re = w_re * wlen_re - w_im * wlen_im;
                let next_w_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_w_re;
                w_im = next_w_im;
            }
            start += len;
        }
        len <<= 1;
    }

    let mut out = [0.0_f32; 256];
    for bin in 0..256 {
        let mag = (re[bin].hypot(im[bin]) / 128.0).sqrt().clamp(0.0, 2.0);
        out[bin] = mag as f32;
    }
    out
}

pub fn compute_spectrum(
    mixed_le_f32: &[u8],
    fft_mode: bool,
    stretch_factor: i32,
    target_little_endian: bool,
) -> Vec<u8> {
    let total_frames = mixed_le_f32.len() / 8;
    let stretch = (stretch_factor.max(0) as usize) + 1;
    let mut left_512 = [0.0_f64; 512];
    let mut right_512 = [0.0_f64; 512];

    for i in 0..512 {
        let src_idx = if fft_mode { i * stretch } else { i / stretch };
        if src_idx < total_frames {
            let base = src_idx * 8;
            let l = f32::from_le_bytes([
                mixed_le_f32[base],
                mixed_le_f32[base + 1],
                mixed_le_f32[base + 2],
                mixed_le_f32[base + 3],
            ]);
            let r = f32::from_le_bytes([
                mixed_le_f32[base + 4],
                mixed_le_f32[base + 5],
                mixed_le_f32[base + 6],
                mixed_le_f32[base + 7],
            ]);
            left_512[i] = (l as f64).clamp(-1.0, 1.0);
            right_512[i] = (r as f64).clamp(-1.0, 1.0);
        }
    }

    let (out_l, out_r) = if fft_mode {
        (
            fft_512_magnitudes(&left_512),
            fft_512_magnitudes(&right_512),
        )
    } else {
        let mut wl = [0.0_f32; 256];
        let mut wr = [0.0_f32; 256];
        for i in 0..256 {
            let idx = (i * 2) / stretch.max(1);
            let idx = idx.min(511);
            wl[i] = left_512[idx] as f32;
            wr[i] = right_512[idx] as f32;
        }
        (wl, wr)
    };

    let mut bytes = Vec::with_capacity(512 * 4);
    for v in out_l.into_iter().chain(out_r) {
        if target_little_endian {
            bytes.extend_from_slice(&v.to_le_bytes());
        } else {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id3v2_and_v1_metadata_and_pcm_fft_roundtrip() {
        let mut body = Vec::new();
        for (id, payload) in [
            (b"TIT2", b"\x03Pulse Song".as_slice()),
            (b"TPE1", b"\x03NeoFlash Artist".as_slice()),
            (b"TALB", b"\x03Wave 5 Album".as_slice()),
            (b"TDRC", b"\x032026-11-02".as_slice()),
            (b"TRCK", b"\x037/12".as_slice()),
            (b"TCON", b"\x03(17)".as_slice()),
            (b"COMM", b"\x03eng\0Studio note".as_slice()),
        ] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(payload);
        }
        let mut mp3 = vec![b'I', b'D', b'3', 3, 0, 0];
        let sz = body.len();
        mp3.extend_from_slice(&[
            ((sz >> 21) & 0x7F) as u8,
            ((sz >> 14) & 0x7F) as u8,
            ((sz >> 7) & 0x7F) as u8,
            (sz & 0x7F) as u8,
        ]);
        mp3.extend_from_slice(&body);
        // Append two valid MPEG-1 Layer III 128 kbps 44.1 kHz stereo frames (417 bytes each).
        for _ in 0..2 {
            let mut frame = vec![0x42u8; 417];
            frame[0] = 0xFF;
            frame[1] = 0xFB; // MPEG-1 Layer III, no CRC
            frame[2] = 0x90; // 128 kbps, 44100 Hz, no padding
            frame[3] = 0x00; // Stereo
            mp3.extend_from_slice(&frame);
        }

        let decoded = decode_compressed_to_canonical(&mp3).unwrap();
        assert_eq!(decoded.meta.sample_frames_44k, 2304);
        let id3 = decoded.meta.id3.unwrap();
        assert_eq!(id3.song_name.as_deref(), Some("Pulse Song"));
        assert_eq!(id3.artist.as_deref(), Some("NeoFlash Artist"));
        assert_eq!(id3.album.as_deref(), Some("Wave 5 Album"));
        assert_eq!(id3.year.as_deref(), Some("2026"));
        assert_eq!(id3.track.as_deref(), Some("7/12"));
        assert_eq!(id3.genre.as_deref(), Some("Rock"));
        assert_eq!(id3.comment.as_deref(), Some("Studio note"));

        // PCM load + extract + peaks + spectrum
        let mut raw_be = Vec::new();
        for i in 0..600 {
            let s = ((i as f64 * 2.0 * PI * 440.0 / 44_100.0).sin() * 0.8) as f32;
            raw_be.extend_from_slice(&s.to_be_bytes());
            raw_be.extend_from_slice(&(-s * 0.5).to_be_bytes());
        }
        let canonical =
            decode_pcm_to_canonical(&raw_be, 600, "float", true, 44_100.0, false).unwrap();
        assert_eq!(canonical.len(), 600 * 8);

        let (extracted_be, ext_meta) =
            extract_canonical_f32(&canonical, 100.0, 0.0, 0, false).unwrap();
        assert_eq!(ext_meta.frames_extracted, 100);
        assert_eq!(ext_meta.next_cursor, 100);
        assert_eq!(extracted_be.len(), 800);

        let ch_tf = As3SoundTransform::from_volume_pan(1.0, 0.0);
        let mx_tf = As3SoundTransform::default();
        let (lp, rp) = channel_peaks(&canonical, 2.0, &ch_tf, &mx_tf);
        assert!(lp > 0.7 && rp > 0.35 && lp > rp);

        let fft_bytes = compute_spectrum(&canonical, true, 0, false);
        assert_eq!(fft_bytes.len(), 2048);
        let bins: Vec<f32> = fft_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_be_bytes(*c))
            .collect();
        let max_l = bins[..256].iter().copied().fold(0.0_f32, f32::max);
        let max_r = bins[256..].iter().copied().fold(0.0_f32, f32::max);
        assert!(max_l > max_r && max_r > 0.05);
    }
}
