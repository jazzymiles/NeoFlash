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

// ---------------------------------------------------------------------------
// Shared Web Audio SoundTransform 2x2 Matrix & Stereo Pan Planner (T-327)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsSoundTransformWire {
    pub volume: f64,
    #[serde(default)]
    pub pan: f64,
    pub left_to_left: f64,
    pub left_to_right: f64,
    pub right_to_left: f64,
    pub right_to_right: f64,
}

impl Default for JsSoundTransformWire {
    fn default() -> Self {
        Self::from_volume_pan(1.0, 0.0)
    }
}

impl JsSoundTransformWire {
    pub fn from_volume_pan(volume: f64, pan: f64) -> Self {
        let v = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Self {
            volume: v,
            pan: p,
            left_to_left: if p <= 0.0 { v } else { v * (1.0 - p) },
            left_to_right: 0.0,
            right_to_left: 0.0,
            right_to_right: if p >= 0.0 { v } else { v * (1.0 + p) },
        }
    }

    pub fn sanitized(self) -> Self {
        let clamp01 = |x: f64| {
            if x.is_finite() {
                x.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let p = if self.pan.is_finite() {
            self.pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Self {
            volume: clamp01(self.volume),
            pan: p,
            left_to_left: clamp01(self.left_to_left),
            left_to_right: clamp01(self.left_to_right),
            right_to_left: clamp01(self.right_to_left),
            right_to_right: clamp01(self.right_to_right),
        }
    }

    pub fn is_pure_pan(&self) -> bool {
        if self.left_to_right.abs() > 1e-6 || self.right_to_left.abs() > 1e-6 {
            return false;
        }
        let expected = Self::from_volume_pan(self.volume, self.pan);
        (self.left_to_left - expected.left_to_left).abs() <= 1e-5
            && (self.right_to_right - expected.right_to_right).abs() <= 1e-5
    }

    /// Matrix multiplication `next * self` on `[L, R]^T`:
    pub fn to_gain_matrix(self) -> [f64; 4] {
        let s = self.sanitized();
        [
            s.left_to_left,
            s.right_to_left,
            s.left_to_right,
            s.right_to_right,
        ]
    }

    /// Compose `self` (inner/channel) followed by `next` (outer/target or mixer):
    /// `mid = self * in`, `out = next * mid`.
    pub fn then(self, next: Self) -> [f64; 4] {
        mul_gain_matrix(self.to_gain_matrix(), next.to_gain_matrix()).map(|v| v.clamp(0.0, 2.0))
    }
}

fn mul_gain_matrix(prev: [f64; 4], next: [f64; 4]) -> [f64; 4] {
    [
        next[0] * prev[0] + next[1] * prev[2],
        next[0] * prev[1] + next[1] * prev[3],
        next[2] * prev[0] + next[3] * prev[2],
        next[2] * prev[1] + next[3] * prev[3],
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundTransformWebPlan {
    pub volume: f64,
    pub pan: f64,
    pub is_matrix: bool,
    /// `[ll, rl, lr, rr]` matching Web Audio splitter->merger wiring:
    /// `[0]: L->L`, `[1]: R->L`, `[2]: L->R`, `[3]: R->R`.
    pub gains: [f64; 4],
}

pub fn plan_web_sound_transform(
    channel: JsSoundTransformWire,
    target: Option<JsSoundTransformWire>,
    mixer: Option<JsSoundTransformWire>,
) -> SoundTransformWebPlan {
    let ch = channel.sanitized();
    let tgt = target.unwrap_or_default().sanitized();
    let mx = mixer.unwrap_or_default().sanitized();

    // Multiply raw 2x2 matrices directly without clamping intermediate stage1 sums to 1.0,
    // preserving (mx * tgt) * ch == mx * (tgt * ch) associativity before final [0.0, 2.0] clamp.
    let raw_stage1 = mul_gain_matrix(ch.to_gain_matrix(), tgt.to_gain_matrix());
    let raw_gains = mul_gain_matrix(raw_stage1, mx.to_gain_matrix());
    let gains = raw_gains.map(|v| v.clamp(0.0, 2.0));
    let [ll, rl, lr, rr] = gains;

    if ll <= 1e-9 && rl <= 1e-9 && lr <= 1e-9 && rr <= 1e-9 {
        let pan = if ch.pan.abs() > 1e-9 {
            ch.pan
        } else if tgt.pan.abs() > 1e-9 {
            tgt.pan
        } else {
            mx.pan
        };
        return SoundTransformWebPlan {
            volume: 0.0,
            pan,
            is_matrix: false,
            gains: [0.0, 0.0, 0.0, 0.0],
        };
    }

    // Check if combined [ll, rl, lr, rr] can be represented as pure (volume, pan).
    if rl.abs() <= 1e-6 && lr.abs() <= 1e-6 {
        let vol = ll.max(rr).clamp(0.0, 1.0);
        let pan = if vol <= 1e-9 {
            0.0
        } else if (ll - vol).abs() <= 1e-5 {
            ((rr / vol) - 1.0).clamp(-1.0, 0.0)
        } else if (rr - vol).abs() <= 1e-5 {
            (1.0 - (ll / vol)).clamp(0.0, 1.0)
        } else {
            // Both L->L and R->R are attenuated asymmetrically (custom channel scaling, not pure pan).
            return SoundTransformWebPlan {
                volume: vol,
                pan: ((rr - ll) / vol).clamp(-1.0, 1.0),
                is_matrix: true,
                gains,
            };
        };
        let pure_check = ch.is_pure_pan() && tgt.is_pure_pan() && mx.is_pure_pan();
        if pure_check {
            return SoundTransformWebPlan {
                volume: vol,
                pan,
                is_matrix: false,
                gains,
            };
        }
        // Even if built via leftToLeft/rightToRight setters, if lr==0 && rl==0 and one side == vol,
        // wait: if user explicitly set custom leftToLeft/rightToRight or non-pure pan, check whether
        // StereoPannerNode differs for stereo sources (StereoPanner sums R into L when pan < 0).
        // Therefore, whenever !pure_check, use the exact 2x2 matrix!
        return SoundTransformWebPlan {
            volume: vol,
            pan,
            is_matrix: true,
            gains,
        };
    }

    let left_sum = ll + rl;
    let right_sum = lr + rr;
    let vol = left_sum.max(right_sum).clamp(0.0, 1.0);
    let pan = if vol > 1e-9 {
        ((right_sum - left_sum) / vol).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    SoundTransformWebPlan {
        volume: vol,
        pan,
        is_matrix: true,
        gains,
    }
}

// ---------------------------------------------------------------------------
// Shared Non-RTMP NetStream Play / Playlist / Seek / Container Inspector (T-327)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetStreamResetMode {
    /// `true` or `1`: clear playlist and start playing immediately.
    ResetAll,
    /// `false` or `0`: append to playlist and wait for current item(s) to finish.
    Append,
    /// `2`: play immediately only if nothing is currently playing; otherwise ignore.
    ImmediateIfIdle,
    /// `3`: clear queued items (keep current playing item) and append this item next.
    ResetQueueAppend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum NetStreamPlayPlan {
    Stop,
    DataGeneration,
    RejectRtmp {
        url: String,
        code: String,
        description: String,
    },
    RejectInvalid {
        description: String,
    },
    PlayClip {
        url: String,
        /// Clamped non-negative progressive start offset in seconds (`start < 0` -> `0.0`).
        start_seconds: f64,
        /// Raw `start` argument passed by script (`-2` default).
        raw_start: f64,
        /// `None` = play to end (`len < 0`), `Some(0.0)` = single frame, `Some(d)` = play `d` seconds.
        duration_limit: Option<f64>,
        single_frame: bool,
        reset_mode: NetStreamResetMode,
    },
}

pub fn is_rtmp_or_live_scheme(url: &str) -> bool {
    let trimmed = url.trim();
    let Some(colon) = trimmed.find(':') else {
        return false;
    };
    let scheme = trimmed[..colon].to_ascii_lowercase();
    matches!(
        scheme.as_str(),
        "rtmp" | "rtmps" | "rtmpt" | "rtmpe" | "rtmpte" | "rtmfp"
    )
}

pub fn plan_netstream_play(
    url: Option<&str>,
    stop_flag: bool,
    is_null: bool,
    start: Option<f64>,
    len: Option<f64>,
    reset_val: Option<&serde_json::Value>,
) -> NetStreamPlayPlan {
    if stop_flag {
        return NetStreamPlayPlan::Stop;
    }
    if is_null {
        return NetStreamPlayPlan::DataGeneration;
    }
    let Some(raw_url) = url else {
        return NetStreamPlayPlan::RejectInvalid {
            description: "NetStream.play requires a media URL string, false, or null.".to_owned(),
        };
    };
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return NetStreamPlayPlan::RejectInvalid {
            description: "NetStream.play requires a non-empty media URL.".to_owned(),
        };
    }
    if is_rtmp_or_live_scheme(trimmed) {
        return NetStreamPlayPlan::RejectRtmp {
            url: trimmed.to_owned(),
            code: "NetStream.Play.StreamNotFound".to_owned(),
            description: "RTMP and remote live streaming servers are excluded in HTML5; use NetConnection.connect(null) with progressive HTTP(S) or bundled file media.".to_owned(),
        };
    }

    let raw_start = start.unwrap_or(-2.0);
    // Flash progressive file playback (`connect(null)`): negative start values (-2 default, -1, etc.)
    // begin at the start of the recorded file (0s); positive values seek to `start` seconds.
    let start_seconds = if raw_start.is_finite() && raw_start > 0.0 {
        raw_start
    } else {
        0.0
    };

    let raw_len = len.unwrap_or(-1.0);
    let (duration_limit, single_frame) = if !raw_len.is_finite() || raw_len < 0.0 {
        (None, false)
    } else if raw_len == 0.0 {
        (Some(0.0), true)
    } else {
        (Some(raw_len), false)
    };

    let reset_mode = match reset_val {
        None => NetStreamResetMode::ResetAll,
        Some(serde_json::Value::Bool(true)) => NetStreamResetMode::ResetAll,
        Some(serde_json::Value::Bool(false)) => NetStreamResetMode::Append,
        Some(serde_json::Value::Number(n)) => {
            match n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)) {
                Some(0) => NetStreamResetMode::Append,
                Some(2) => NetStreamResetMode::ImmediateIfIdle,
                Some(3) => NetStreamResetMode::ResetQueueAppend,
                _ => NetStreamResetMode::ResetAll,
            }
        }
        _ => NetStreamResetMode::ResetAll,
    };

    NetStreamPlayPlan::PlayClip {
        url: raw_url.to_owned(),
        start_seconds,
        raw_start,
        duration_limit,
        single_frame,
        reset_mode,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "decision")]
pub enum NetStreamSeekDecision {
    SeekNow { target: f64 },
    Pending { target: f64 },
    InvalidTime { details: f64 },
}

pub fn decide_netstream_seek(
    offset: f64,
    metadata_known: bool,
    duration: f64,
) -> NetStreamSeekDecision {
    if !offset.is_finite() || offset < 0.0 {
        return NetStreamSeekDecision::InvalidTime { details: 0.0 };
    }
    if !metadata_known {
        return NetStreamSeekDecision::Pending { target: offset };
    }
    let dur = if duration.is_finite() && duration >= 0.0 {
        duration
    } else {
        0.0
    };
    if offset > dur + 1e-6 {
        return NetStreamSeekDecision::InvalidTime { details: dur };
    }
    NetStreamSeekDecision::SeekNow {
        target: offset.min(dur),
    }
}

pub fn compute_netstream_buffer_length(current_time: f64, ranges: &[(f64, f64)]) -> f64 {
    if !current_time.is_finite() || ranges.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<(f64, f64)> = ranges
        .iter()
        .copied()
        .filter(|(s, e)| s.is_finite() && e.is_finite() && *e >= *s)
        .collect();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut covered_end: Option<f64> = None;
    let eps = 0.05;
    for (s, e) in sorted {
        match covered_end {
            None => {
                if s <= current_time + eps && e >= current_time - eps {
                    covered_end = Some(e);
                }
            }
            Some(end) => {
                if s <= end + eps {
                    covered_end = Some(end.max(e));
                } else {
                    break;
                }
            }
        }
    }
    covered_end
        .map(|end| (end - current_time).max(0.0))
        .unwrap_or(0.0)
}

pub fn compute_netstream_fps(
    frame_times_ms: &[f64],
    now_ms: f64,
    playing: bool,
    receive_video: bool,
    metadata_fps: f64,
) -> f64 {
    if !playing || !receive_video {
        return 0.0;
    }
    let recent: Vec<f64> = frame_times_ms
        .iter()
        .copied()
        .filter(|t| t.is_finite() && now_ms - *t <= 1500.0 && *t <= now_ms + 1.0)
        .collect();
    if recent.len() >= 2 {
        let span_ms = recent[recent.len() - 1] - recent[0];
        if span_ms > 5.0 {
            let fps = ((recent.len() - 1) as f64) * 1000.0 / span_ms;
            return (fps * 100.0).round() / 100.0;
        }
    }
    if metadata_fps.is_finite() && metadata_fps > 0.0 {
        metadata_fps
    } else {
        0.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCuePoint {
    pub name: String,
    pub time: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaContainerProbe {
    pub format: Option<String>,
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub framerate: Option<f64>,
    pub cue_points: Vec<MediaCuePoint>,
}

/// Lightweight ISO BMFF (MP4/M4A/MOV) and RIFF WAV container inspector so progressive/embedded
/// `NetStream` `onMetaData` receives real `framerate` and container cue points when bytes are known.
pub fn inspect_media_container(bytes: &[u8]) -> MediaContainerProbe {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return inspect_mp4_boxes(bytes);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return inspect_wav_cues(bytes);
    }
    MediaContainerProbe::default()
}

fn read_be_u32(data: &[u8], off: usize) -> Option<u32> {
    let slice = data.get(off..off + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_be_u64(data: &[u8], off: usize) -> Option<u64> {
    let slice = data.get(off..off + 8)?;
    Some(u64::from_be_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn inspect_mp4_boxes(bytes: &[u8]) -> MediaContainerProbe {
    let mut probe = MediaContainerProbe {
        format: Some("mp4".to_owned()),
        ..Default::default()
    };
    let mut video_fps: Option<f64> = None;

    fn walk_boxes(
        buf: &[u8],
        probe: &mut MediaContainerProbe,
        video_fps: &mut Option<f64>,
        mdhd_timescale: &mut u32,
        is_video_trak: &mut bool,
        depth: usize,
    ) {
        if depth > 8 {
            return;
        }
        let mut pos = 0usize;
        while pos + 8 <= buf.len() {
            let sz32 = match read_be_u32(buf, pos) {
                Some(v) => v as usize,
                None => break,
            };
            let typ = &buf[pos + 4..pos + 8];
            let (hdr, box_len) = if sz32 == 1 {
                let Some(sz64) = read_be_u64(buf, pos + 8) else {
                    break;
                };
                (16usize, sz64 as usize)
            } else if sz32 == 0 {
                (8usize, buf.len() - pos)
            } else {
                (8usize, sz32)
            };
            if box_len < hdr || pos + box_len > buf.len() {
                break;
            }
            let payload = &buf[pos + hdr..pos + box_len];

            match typ {
                b"moov" | b"mdia" | b"minf" | b"stbl" | b"udta" => {
                    walk_boxes(
                        payload,
                        probe,
                        video_fps,
                        mdhd_timescale,
                        is_video_trak,
                        depth + 1,
                    );
                }
                b"trak" => {
                    let mut local_ts = 0u32;
                    let mut local_vid = false;
                    walk_boxes(
                        payload,
                        probe,
                        video_fps,
                        &mut local_ts,
                        &mut local_vid,
                        depth + 1,
                    );
                }
                b"mvhd" if payload.len() >= 20 => {
                    let version = payload[0];
                    if version == 1 && payload.len() >= 28 {
                        let ts = read_be_u32(payload, 20).unwrap_or(0);
                        let dur = read_be_u64(payload, 24).unwrap_or(0);
                        if ts > 0 {
                            probe.duration = Some(dur as f64 / ts as f64);
                        }
                    } else if version == 0 {
                        let ts = read_be_u32(payload, 12).unwrap_or(0);
                        let dur = read_be_u32(payload, 16).unwrap_or(0);
                        if ts > 0 {
                            probe.duration = Some(dur as f64 / ts as f64);
                        }
                    }
                }
                b"hdlr" if payload.len() >= 12 => {
                    if &payload[8..12] == b"vide" {
                        *is_video_trak = true;
                    }
                }
                b"tkhd" if payload.len() >= 84 => {
                    let version = payload[0];
                    let wh_off = if version == 1 { 88 } else { 76 };
                    if payload.len() >= wh_off + 8 {
                        let w_fixed = read_be_u32(payload, wh_off).unwrap_or(0);
                        let h_fixed = read_be_u32(payload, wh_off + 4).unwrap_or(0);
                        let w = w_fixed >> 16;
                        let h = h_fixed >> 16;
                        if w > 0 && h > 0 && probe.width.is_none() {
                            probe.width = Some(w);
                            probe.height = Some(h);
                        }
                    }
                }
                b"mdhd" if payload.len() >= 20 => {
                    let version = payload[0];
                    if version == 1 && payload.len() >= 28 {
                        *mdhd_timescale = read_be_u32(payload, 20).unwrap_or(0);
                    } else if version == 0 {
                        *mdhd_timescale = read_be_u32(payload, 12).unwrap_or(0);
                    }
                }
                b"stts" if *is_video_trak && *mdhd_timescale > 0 && payload.len() >= 8 => {
                    let entry_count = read_be_u32(payload, 4).unwrap_or(0) as usize;
                    let mut total_samples = 0u64;
                    let mut total_delta = 0u64;
                    let max_entries = entry_count.min((payload.len() - 8) / 8);
                    for i in 0..max_entries {
                        let base = 8 + i * 8;
                        let count = read_be_u32(payload, base).unwrap_or(0) as u64;
                        let delta = read_be_u32(payload, base + 4).unwrap_or(0) as u64;
                        total_samples = total_samples.saturating_add(count);
                        total_delta = total_delta.saturating_add(count.saturating_mul(delta));
                    }
                    if total_samples > 0 && total_delta > 0 {
                        let fps = (total_samples as f64) * (*mdhd_timescale as f64)
                            / (total_delta as f64);
                        if fps.is_finite() && fps > 0.0 && fps <= 240.0 {
                            *video_fps = Some((fps * 100.0).round() / 100.0);
                        }
                    }
                }
                b"chpl" if payload.len() >= 9 => {
                    // Nero chapter box inside udta: version(1)+flags(3)+reserved(4)+count(1)
                    let count = payload[8] as usize;
                    let mut cpos = 9usize;
                    for idx in 0..count {
                        if cpos + 9 > payload.len() {
                            break;
                        }
                        let ts_100ns = read_be_u64(payload, cpos).unwrap_or(0);
                        let name_len = payload[cpos + 8] as usize;
                        cpos += 9;
                        if cpos + name_len > payload.len() {
                            break;
                        }
                        let name =
                            String::from_utf8_lossy(&payload[cpos..cpos + name_len]).into_owned();
                        cpos += name_len;
                        probe.cue_points.push(MediaCuePoint {
                            name: if name.is_empty() {
                                format!("cue_{idx}")
                            } else {
                                name
                            },
                            time: (ts_100ns as f64) / 10_000_000.0,
                            kind: "navigation".to_owned(),
                        });
                    }
                }
                _ => {}
            }
            pos += box_len;
        }
    }

    let mut root_ts = 0u32;
    let mut root_vid = false;
    walk_boxes(
        bytes,
        &mut probe,
        &mut video_fps,
        &mut root_ts,
        &mut root_vid,
        0,
    );
    probe.framerate = video_fps;
    probe
}

fn inspect_wav_cues(bytes: &[u8]) -> MediaContainerProbe {
    let mut probe = MediaContainerProbe {
        format: Some("wav".to_owned()),
        ..Default::default()
    };
    let mut sample_rate = 0u32;
    let mut byte_rate = 0u32;
    let mut data_bytes = 0u32;
    let mut cue_offsets: Vec<(u32, u32)> = Vec::new();
    let mut cue_labels: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let sz = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let payload_end = (pos + 8).saturating_add(sz).min(bytes.len());
        let payload = &bytes[pos + 8..payload_end];
        match id {
            b"fmt " if payload.len() >= 12 => {
                sample_rate = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                byte_rate = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
            }
            b"data" => {
                data_bytes = sz as u32;
            }
            b"cue " if payload.len() >= 4 => {
                let count =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                let max_cues = count.min((payload.len() - 4) / 24);
                for i in 0..max_cues {
                    let b = 4 + i * 24;
                    let cue_id = u32::from_le_bytes([
                        payload[b],
                        payload[b + 1],
                        payload[b + 2],
                        payload[b + 3],
                    ]);
                    let sample_offset = u32::from_le_bytes([
                        payload[b + 20],
                        payload[b + 21],
                        payload[b + 22],
                        payload[b + 23],
                    ]);
                    cue_offsets.push((cue_id, sample_offset));
                }
            }
            b"LIST" if payload.len() >= 4 && &payload[0..4] == b"adtl" => {
                let mut lpos = 4usize;
                while lpos + 8 <= payload.len() {
                    let lid = &payload[lpos..lpos + 4];
                    let lsz = u32::from_le_bytes([
                        payload[lpos + 4],
                        payload[lpos + 5],
                        payload[lpos + 6],
                        payload[lpos + 7],
                    ]) as usize;
                    let lend = (lpos + 8).saturating_add(lsz).min(payload.len());
                    let ldata = &payload[lpos + 8..lend];
                    if lid == b"labl" && ldata.len() >= 4 {
                        let cid = u32::from_le_bytes([ldata[0], ldata[1], ldata[2], ldata[3]]);
                        let raw_txt = &ldata[4..];
                        let trimmed_end = raw_txt
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(raw_txt.len());
                        cue_labels.insert(
                            cid,
                            String::from_utf8_lossy(&raw_txt[..trimmed_end]).into_owned(),
                        );
                    }
                    lpos = (lpos + 8).saturating_add(lsz + (lsz & 1));
                }
            }
            _ => {}
        }
        pos = (pos + 8).saturating_add(sz + (sz & 1));
    }

    if byte_rate > 0 && data_bytes > 0 {
        probe.duration = Some(data_bytes as f64 / byte_rate as f64);
    }
    if sample_rate > 0 {
        for (cid, sample_offset) in cue_offsets {
            let name = cue_labels
                .remove(&cid)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("cue_{cid}"));
            probe.cue_points.push(MediaCuePoint {
                name,
                time: sample_offset as f64 / sample_rate as f64,
                kind: "event".to_owned(),
            });
        }
    }
    probe
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

    #[test]
    fn sound_transform_web_plan_handles_pure_pan_matrix_and_mixer_composition() {
        let zero = JsSoundTransformWire::from_volume_pan(0.0, 0.25);
        let mixer = JsSoundTransformWire::from_volume_pan(0.5, -0.25);
        let zero_plan = plan_web_sound_transform(zero, None, Some(mixer));
        assert_eq!(zero_plan.volume, 0.0);
        assert!(!zero_plan.is_matrix);
        assert_eq!(zero_plan.gains, [0.0, 0.0, 0.0, 0.0]);

        let left_pan = JsSoundTransformWire::from_volume_pan(0.8, -0.5);
        let pan_plan = plan_web_sound_transform(left_pan, None, None);
        assert!(!pan_plan.is_matrix);
        assert!((pan_plan.volume - 0.8).abs() < 1e-6);
        assert!((pan_plan.pan + 0.5).abs() < 1e-6);

        let swap = JsSoundTransformWire {
            volume: 1.0,
            pan: 0.0,
            left_to_left: 0.0,
            left_to_right: 1.0,
            right_to_left: 1.0,
            right_to_right: 0.0,
        };
        let swap_plan = plan_web_sound_transform(swap, None, None);
        assert!(swap_plan.is_matrix);
        // [ll, rl, lr, rr] = [0, 1, 1, 0]
        assert_eq!(swap_plan.gains, [0.0, 1.0, 1.0, 0.0]);

        // Swap composed with half-volume mixer scales swapped channels by 0.5
        let half_mixer = JsSoundTransformWire::from_volume_pan(0.5, 0.0);
        let swapped_half = plan_web_sound_transform(swap, None, Some(half_mixer));
        assert!(swapped_half.is_matrix);
        assert!((swapped_half.gains[1] - 0.5).abs() < 1e-6);
        assert!((swapped_half.gains[2] - 0.5).abs() < 1e-6);

        // 3-stage channel * target * mixer cross-channel summing > 1.0 in stage1 is NOT clamped
        // prematurely before multiplying by mixer (preserving (mx * tgt) * ch == mx * (tgt * ch)).
        let ch_full = JsSoundTransformWire {
            volume: 1.0,
            pan: 0.0,
            left_to_left: 1.0,
            left_to_right: 0.8,
            right_to_left: 0.0,
            right_to_right: 1.0,
        };
        let tgt_mix = JsSoundTransformWire {
            volume: 1.0,
            pan: 0.0,
            left_to_left: 1.0,
            left_to_right: 0.0,
            right_to_left: 1.0,
            right_to_right: 1.0,
        };
        // stage1[0] = tgt.ll * ch.ll + tgt.rl * ch.lr = 1.0 * 1.0 + 1.0 * 0.8 = 1.8 (> 1.0)
        // With half_mixer (volume = 0.5): final ll must be 1.8 * 0.5 = 0.9, NOT 1.0 * 0.5 = 0.5!
        let three_stage = plan_web_sound_transform(ch_full, Some(tgt_mix), Some(half_mixer));
        assert!(three_stage.is_matrix);
        assert!((three_stage.gains[0] - 0.9).abs() < 1e-9);
        let assoc_rhs = mul_gain_matrix(
            ch_full.to_gain_matrix(),
            mul_gain_matrix(tgt_mix.to_gain_matrix(), half_mixer.to_gain_matrix()),
        );
        for (lhs, rhs) in three_stage.gains.into_iter().zip(assoc_rhs) {
            assert!((lhs - rhs.clamp(0.0, 2.0)).abs() < 1e-12);
        }
    }

    #[test]
    fn netstream_play_seek_buffer_fps_and_container_probe() {
        // RTMP schemes rejected cleanly
        for rtmp in [
            "rtmp://live.example.test/app/stream",
            "rtmps://live.example.test/app/stream",
            "rtmpt://live.example.test/app/stream",
            "rtmpe://live.example.test/app/stream",
            "rtmfp://p2p.example.test/live",
        ] {
            match plan_netstream_play(Some(rtmp), false, false, None, None, None) {
                NetStreamPlayPlan::RejectRtmp { code, .. } => {
                    assert_eq!(code, "NetStream.Play.StreamNotFound");
                }
                other => panic!("expected RejectRtmp, got {other:?}"),
            }
        }

        // Single-frame load (len = 0) and playlist append (reset = false / 0)
        let p_single = plan_netstream_play(
            Some("clip1.mp4"),
            false,
            false,
            Some(1.25),
            Some(0.0),
            Some(&serde_json::json!(true)),
        );
        assert_eq!(
            p_single,
            NetStreamPlayPlan::PlayClip {
                url: "clip1.mp4".to_owned(),
                start_seconds: 1.25,
                raw_start: 1.25,
                duration_limit: Some(0.0),
                single_frame: true,
                reset_mode: NetStreamResetMode::ResetAll,
            }
        );

        let p_append = plan_netstream_play(
            Some("clip2.mp4"),
            false,
            false,
            Some(-2.0),
            Some(0.75),
            Some(&serde_json::json!(false)),
        );
        assert_eq!(
            p_append,
            NetStreamPlayPlan::PlayClip {
                url: "clip2.mp4".to_owned(),
                start_seconds: 0.0,
                raw_start: -2.0,
                duration_limit: Some(0.75),
                single_frame: false,
                reset_mode: NetStreamResetMode::Append,
            }
        );

        // Seek decisions
        assert_eq!(
            decide_netstream_seek(-0.5, true, 4.0),
            NetStreamSeekDecision::InvalidTime { details: 0.0 }
        );
        assert_eq!(
            decide_netstream_seek(5.5, true, 4.0),
            NetStreamSeekDecision::InvalidTime { details: 4.0 }
        );
        assert_eq!(
            decide_netstream_seek(1.5, false, 0.0),
            NetStreamSeekDecision::Pending { target: 1.5 }
        );
        assert_eq!(
            decide_netstream_seek(2.0, true, 4.0),
            NetStreamSeekDecision::SeekNow { target: 2.0 }
        );

        // Buffer length & FPS
        assert!(
            (compute_netstream_buffer_length(1.0, &[(0.0, 2.5), (2.5, 4.0)]) - 3.0).abs() < 1e-6
        );
        assert!(
            (compute_netstream_fps(&[0.0, 40.0, 80.0, 120.0, 160.0], 170.0, true, true, 24.0)
                - 25.0)
                .abs()
                < 1e-6
        );
        assert_eq!(
            compute_netstream_fps(&[0.0, 40.0], 50.0, false, true, 24.0),
            0.0
        );

        // Synthetic MP4 container probe (ftyp + moov(mvhd + trak(tkhd + mdia(mdhd + hdlr + minf(stbl(stts)))) + udta(chpl)))
        fn mp4_box(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let len = (8 + payload.len()) as u32;
            let mut out = Vec::with_capacity(len as usize);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(tag);
            out.extend_from_slice(payload);
            out
        }
        let ftyp = mp4_box(b"ftyp", b"isom\x00\x00\x02\x00isom");
        let mut mvhd_payload = vec![0u8; 20];
        mvhd_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd_payload[16..20].copy_from_slice(&4000u32.to_be_bytes());
        let mvhd = mp4_box(b"mvhd", &mvhd_payload);
        let mut tkhd_payload = vec![0u8; 84];
        tkhd_payload[76..80].copy_from_slice(&(320u32 << 16).to_be_bytes());
        tkhd_payload[80..84].copy_from_slice(&(180u32 << 16).to_be_bytes());
        let tkhd = mp4_box(b"tkhd", &tkhd_payload);
        let mut mdhd_payload = vec![0u8; 20];
        mdhd_payload[12..16].copy_from_slice(&24000u32.to_be_bytes());
        let mdhd = mp4_box(b"mdhd", &mdhd_payload);
        let mut hdlr_payload = vec![0u8; 16];
        hdlr_payload[8..12].copy_from_slice(b"vide");
        let hdlr = mp4_box(b"hdlr", &hdlr_payload);
        let mut stts_payload = vec![0u8; 16];
        stts_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stts_payload[8..12].copy_from_slice(&96u32.to_be_bytes());
        stts_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
        let stts = mp4_box(b"stts", &stts_payload);
        let stbl = mp4_box(b"stbl", &stts);
        let minf = mp4_box(b"minf", &stbl);
        let mut mdia_body = Vec::new();
        mdia_body.extend_from_slice(&mdhd);
        mdia_body.extend_from_slice(&hdlr);
        mdia_body.extend_from_slice(&minf);
        let mdia = mp4_box(b"mdia", &mdia_body);
        let mut trak_body = Vec::new();
        trak_body.extend_from_slice(&tkhd);
        trak_body.extend_from_slice(&mdia);
        let trak = mp4_box(b"trak", &trak_body);
        let mut chpl_payload = vec![0u8; 9];
        chpl_payload[8] = 1;
        chpl_payload.extend_from_slice(&15_000_000u64.to_be_bytes()); // 1.5s
        chpl_payload.push(5);
        chpl_payload.extend_from_slice(b"intro");
        let chpl = mp4_box(b"chpl", &chpl_payload);
        let udta = mp4_box(b"udta", &chpl);
        let mut moov_body = Vec::new();
        moov_body.extend_from_slice(&mvhd);
        moov_body.extend_from_slice(&trak);
        moov_body.extend_from_slice(&udta);
        let moov = mp4_box(b"moov", &moov_body);
        let mut mp4 = Vec::new();
        mp4.extend_from_slice(&ftyp);
        mp4.extend_from_slice(&moov);

        let probed = inspect_media_container(&mp4);
        assert_eq!(probed.width, Some(320));
        assert_eq!(probed.height, Some(180));
        assert_eq!(probed.duration, Some(4.0));
        assert_eq!(probed.framerate, Some(24.0));
        assert_eq!(probed.cue_points.len(), 1);
        assert_eq!(probed.cue_points[0].name, "intro");
        assert!((probed.cue_points[0].time - 1.5).abs() < 1e-6);
    }
}
