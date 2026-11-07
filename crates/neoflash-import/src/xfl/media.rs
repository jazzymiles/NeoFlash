//! Shared bounded media sniffing used by XFL import and export.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioInspection {
    pub extension: &'static str,
    pub duration_44: u64,
    pub is_mp3: bool,
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
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt_info: Option<(u32, usize)> = None;
    let mut data_len: Option<usize> = None;
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
            let channels = u16::from_le_bytes(payload[2..4].try_into().ok()?);
            let sample_rate = u32::from_le_bytes(payload[4..8].try_into().ok()?);
            let block_align = u16::from_le_bytes(payload[12..14].try_into().ok()?);
            let bits = u16::from_le_bytes(payload[14..16].try_into().ok()?);
            if channels == 0 || sample_rate == 0 {
                return None;
            }
            let derived_align = usize::from(channels) * usize::from(bits.max(8)).div_ceil(8);
            let frame_bytes = usize::from(block_align).max(derived_align).max(1);
            fmt_info = Some((sample_rate, frame_bytes));
        } else if tag == b"data" {
            data_len = Some(payload.len());
        }
        pos = payload_end.saturating_add(chunk_len & 1);
    }
    let (sample_rate, frame_bytes) = fmt_info?;
    let data_bytes = data_len?;
    if data_bytes < frame_bytes {
        return None;
    }
    let sample_frames = (data_bytes / frame_bytes) as u64;
    Some((sample_frames * 44_100) / u64::from(sample_rate))
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
