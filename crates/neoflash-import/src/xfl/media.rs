//! Shared bounded media sniffing used by XFL import and export.

/// Only explicitly marked NeoFlash video-frame packages use these editor previews.
pub(super) fn video_preview_size(href: &str) -> Option<(u32, u32)> {
    let (size, filename) = href
        .strip_prefix("media/neoflash-video-frames/")?
        .split_once('/')?;
    if filename.is_empty() || filename.contains('/') {
        return None;
    }
    let (width, height) = size.split_once('x')?;
    let dimension = |text: &str| {
        text.parse::<u32>()
            .ok()
            .filter(|v| (1..=neoflash_core::MAX_BITMAP_DIMENSION).contains(v))
    };
    Some((dimension(width)?, dimension(height)?))
}

/// Decode one full frame at a time without caching it in the project's resident image arena.
pub(super) fn decode_video_preview(
    raw: &[u8],
    ext: &str,
    size: (u32, u32),
) -> image::ImageResult<Option<(Vec<u8>, [f64; 2])>> {
    use std::io::Cursor;
    let format = match ext {
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "png" => image::ImageFormat::Png,
        _ => return Ok(None),
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(raw), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(neoflash_core::MAX_BITMAP_DIMENSION);
    limits.max_image_height = Some(neoflash_core::MAX_BITMAP_DIMENSION);
    limits.max_alloc = Some(neoflash_core::MAX_BITMAP_BYTES as u64);
    reader.limits(limits);
    let full = reader.decode()?;
    let (w, h) = (full.width(), full.height());
    let (pw, ph) = size;
    // A renamed or externally replaced source must never be enlarged or distorted by a stale hint.
    if pw > w
        || ph > h
        || (pw, ph) == (w, h)
        || (i64::from(w) * i64::from(ph) - i64::from(h) * i64::from(pw)).abs()
            > i64::from(w) + i64::from(h)
    {
        return Ok(None);
    }
    let preview = full.thumbnail_exact(pw, ph);
    let mut encoded = Vec::new();
    if format == image::ImageFormat::Jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 92)
            .encode_image(&preview)?;
    } else {
        preview.write_to(&mut Cursor::new(&mut encoded), format)?;
    }
    Ok(Some((
        encoded,
        [f64::from(w) / f64::from(pw), f64::from(h) / f64::from(ph)],
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioInspection {
    pub extension: &'static str,
    pub duration_44: u64,
    pub is_mp3: bool,
}

/// Decoded Flash-compatible sound cache metadata and `bin/sound*.dat` / `M *` stream bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashSoundCache {
    pub format_flags: u16,
    pub format_label: String,
    pub sample_count: u32,
    pub data_length: Option<u32>,
    pub cache_bytes: Vec<u8>,
}

struct ParsedWav<'a> {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
    data: &'a [u8],
}

fn parse_wav_chunks(bytes: &[u8]) -> Option<ParsedWav<'_>> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt_info: Option<(u16, u16, u32, u16, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let tag = &bytes[pos..pos + 4];
        let chunk_len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.checked_add(chunk_len)?;
        if payload_end > bytes.len() {
            return None;
        }
        let payload = &bytes[payload_start..payload_end];
        if tag == b"fmt " {
            if payload.len() < 16 {
                return None;
            }
            let audio_format = u16::from_le_bytes(payload[0..2].try_into().ok()?);
            let channels = u16::from_le_bytes(payload[2..4].try_into().ok()?);
            let sample_rate = u32::from_le_bytes(payload[4..8].try_into().ok()?);
            let block_align = u16::from_le_bytes(payload[12..14].try_into().ok()?);
            let bits_per_sample = u16::from_le_bytes(payload[14..16].try_into().ok()?);
            if channels == 0 || sample_rate == 0 {
                return None;
            }
            fmt_info = Some((
                audio_format,
                channels,
                sample_rate,
                block_align,
                bits_per_sample,
            ));
        } else if tag == b"data" {
            data = Some(payload);
        }
        pos = payload_end.saturating_add(chunk_len & 1);
    }
    let (audio_format, channels, sample_rate, block_align, bits_per_sample) = fmt_info?;
    Some(ParsedWav {
        audio_format,
        channels,
        sample_rate,
        block_align,
        bits_per_sample,
        data: data?,
    })
}

pub(crate) fn inspect_audio_bytes(bytes: &[u8]) -> Option<AudioInspection> {
    if let Some(duration_44) = wav_duration_44(bytes) {
        return Some(AudioInspection {
            extension: "wav",
            duration_44,
            is_mp3: false,
        });
    }
    looks_like_mp3(bytes).then_some(AudioInspection {
        extension: "mp3",
        // XFL needs a bounded time base for normalized envelopes even when no
        // decoded MP3 sample count is available in the document model.
        duration_44: 44_100,
        is_mp3: true,
    })
}

pub(crate) fn wav_duration_44(bytes: &[u8]) -> Option<u64> {
    let wav = parse_wav_chunks(bytes)?;
    let derived_align =
        usize::from(wav.channels) * usize::from(wav.bits_per_sample.max(8)).div_ceil(8);
    let frame_bytes = usize::from(wav.block_align).max(derived_align).max(1);
    if wav.data.len() < frame_bytes {
        return None;
    }
    let sample_frames = (wav.data.len() / frame_bytes) as u64;
    Some((sample_frames * 44_100) / u64::from(wav.sample_rate))
}

/// Extract Adobe Flash-compatible cached sound bytes (`bin/sound*.dat` / legacy `M *` stream)
/// and format metadata for valid PCM WAV (8/16-bit mono/stereo) or MP3 audio resources.
pub fn build_flash_sound_cache(bytes: &[u8]) -> Option<FlashSoundCache> {
    if let Some(wav) = parse_wav_chunks(bytes) {
        if wav.audio_format != 1
            || !matches!(wav.bits_per_sample, 8 | 16)
            || !matches!(wav.channels, 1 | 2)
        {
            return None;
        }
        let (rate_code, rate_label): (u16, &str) = if wav.sample_rate <= 6000 {
            (0, "5kHz")
        } else if wav.sample_rate <= 12000 {
            (1, "11kHz")
        } else if wav.sample_rate <= 24000 {
            (2, "22kHz")
        } else {
            (3, "44kHz")
        };
        let size_bit = u16::from(wav.bits_per_sample >= 16);
        let stereo_bit = u16::from(wav.channels >= 2);
        let format_flags = (rate_code << 2) | (size_bit << 1) | stereo_bit;
        let frame_bytes = usize::from(1 + stereo_bit) * usize::from(1 + size_bit);
        let sample_count = (wav.data.len() / frame_bytes.max(1)) as u32;
        if sample_count == 0 {
            return None;
        }
        let exact_pcm_len = (sample_count as usize).checked_mul(frame_bytes)?;
        let bits_label = if size_bit == 1 { "16bit" } else { "8bit" };
        let ch_label = if stereo_bit == 1 { "Stereo" } else { "Mono" };
        return Some(FlashSoundCache {
            format_flags,
            format_label: format!("{rate_label} {bits_label} {ch_label}"),
            sample_count,
            data_length: None,
            cache_bytes: wav.data[..exact_pcm_len].to_vec(),
        });
    }
    if looks_like_mp3(bytes) && !bytes.is_empty() {
        let len_u32 = u32::try_from(bytes.len()).ok()?;
        let mut cache_bytes = Vec::with_capacity(bytes.len() + 1);
        cache_bytes.extend_from_slice(bytes);
        // Binary/XFL sound media payloads follow compressed frames with a 1-byte
        // secondary PCM marker (0 = no secondary decoded cache).
        cache_bytes.push(0);
        return Some(FlashSoundCache {
            format_flags: 0x002f,
            format_label: "44kHz 16bit Stereo".to_owned(),
            sample_count: len_u32.saturating_mul(8).max(576),
            data_length: Some(len_u32),
            cache_bytes,
        });
    }
    None
}

pub(crate) fn looks_like_mp3(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    if bytes.len() >= 10
        && &bytes[..3] == b"ID3"
        && (bytes[6] | bytes[7] | bytes[8] | bytes[9]) & 0x80 == 0
    {
        let tag_len = ((bytes[6] as usize) << 21)
            | ((bytes[7] as usize) << 14)
            | ((bytes[8] as usize) << 7)
            | (bytes[9] as usize);
        let footer = if (bytes[5] & 0x10) != 0 { 10 } else { 0 };
        start = (10 + tag_len + footer).min(bytes.len());
    }
    let end = bytes
        .len()
        .saturating_sub(3)
        .min(start.saturating_add(4096));
    (start..end).any(|i| {
        let h = &bytes[i..i + 4];
        h[0] == 0xff
            && (h[1] & 0xe0) == 0xe0
            && ((h[1] >> 3) & 3) != 1
            && ((h[1] >> 1) & 3) != 0
            && ((h[2] >> 4) & 0xf) != 0xf
            && ((h[2] >> 2) & 3) != 3
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm_wav(sample_rate: u32, channels: u16, frames: usize, odd_junk: bool) -> Vec<u8> {
        let block_align = channels * 2;
        let data_len = frames * usize::from(block_align);
        let junk_len = usize::from(odd_junk);
        let riff_len = 4 + (8 + 16) + (if odd_junk { 8 + junk_len + 1 } else { 0 }) + 8 + data_len;
        let mut out = Vec::with_capacity(8 + riff_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(riff_len as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        if odd_junk {
            out.extend_from_slice(b"JUNK");
            out.extend_from_slice(&(junk_len as u32).to_le_bytes());
            out.push(0x5a);
            out.push(0);
        }
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        out.resize(out.len() + data_len, 0);
        out
    }

    #[test]
    fn wav_duration_handles_odd_chunks_and_rejects_truncation() {
        let ordinary = pcm_wav(44_100, 2, 22_050, false);
        let odd_chunk = pcm_wav(22_050, 1, 22_050, true);
        assert_eq!(wav_duration_44(&ordinary), Some(22_050));
        assert_eq!(wav_duration_44(&odd_chunk), Some(44_100));

        let mut truncated = odd_chunk;
        truncated.pop();
        assert_eq!(wav_duration_44(&truncated), None);
        assert_eq!(wav_duration_44(b"RIFF\0\0\0\0WAVE"), None);
    }

    #[test]
    fn mp3_sniffing_skips_id3_and_rejects_reserved_headers() {
        let frame = [0xff, 0xfb, 0x90, 0x64];
        assert!(looks_like_mp3(&frame));

        let mut tagged = b"ID3\x04\0\0\0\0\0\x03abc".to_vec();
        tagged.extend_from_slice(&frame);
        assert!(looks_like_mp3(&tagged));
        assert_eq!(
            inspect_audio_bytes(&tagged),
            Some(AudioInspection {
                extension: "mp3",
                duration_44: 44_100,
                is_mp3: true,
            })
        );

        assert!(!looks_like_mp3(&[0xff, 0xeb, 0x90, 0x64]));
        assert!(!looks_like_mp3(&[0xff, 0xfb, 0xf0, 0x64]));
        assert!(!looks_like_mp3(b"ID3\x04\0\0\x80\0\0\0"));
    }
}
