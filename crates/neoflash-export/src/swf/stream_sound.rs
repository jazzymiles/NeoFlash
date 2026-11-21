//! SWF timeline `SoundStreamHead2` (tag 45) and per-frame `SoundStreamBlock` (tag 19) encoder.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct PreparedSwfStreamSound {
    pub(super) info: SwfSoundInfo,
    pub(super) audio_bytes: Vec<u8>,
    pub(super) mp3_frames: Vec<(usize, usize, u16)>,
}

pub(super) struct TimelineStreamPlan {
    pub(super) head_by_frame: BTreeMap<u32, Vec<u8>>,
    pub(super) block_by_frame: BTreeMap<u32, Vec<u8>>,
    pub(super) fallback_event_frames: HashSet<(usize, u32)>,
}

fn parse_mp3_frame_slices(audio_bytes: &[u8]) -> Vec<(usize, usize, u16)> {
    const BITRATES_V1_L3: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const BITRATES_V2_L3: [u32; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    let mut frames = Vec::new();
    let mut pos = 0usize;
    let end = audio_bytes.len();
    while pos + 4 <= end {
        if audio_bytes[pos] != 0xff || (audio_bytes[pos + 1] & 0xe0) != 0xe0 {
            break;
        }
        let b1 = audio_bytes[pos + 1];
        let b2 = audio_bytes[pos + 2];
        let version_bits = (b1 >> 3) & 0x03;
        let layer_bits = (b1 >> 1) & 0x03;
        let bitrate_index = usize::from((b2 >> 4) & 0x0f);
        let sample_rate_index = usize::from((b2 >> 2) & 0x03);
        let padding = usize::from((b2 >> 1) & 0x01);
        if version_bits == 1 || layer_bits != 1 || bitrate_index == 0 || bitrate_index == 15 {
            break;
        }
        let rate = match (version_bits, sample_rate_index) {
            (3, 0) => 44_100usize,
            (3, 1) => 48_000,
            (3, 2) => 32_000,
            (2, 0) => 22_050,
            (2, 1) => 24_000,
            (2, 2) => 16_000,
            (0, 0) => 11_025,
            (0, 1) => 12_000,
            (0, 2) => 8_000,
            _ => break,
        };
        let bitrate_kbps = if version_bits == 3 {
            BITRATES_V1_L3[bitrate_index] as usize
        } else {
            BITRATES_V2_L3[bitrate_index] as usize
        };
        let samples: u16 = if version_bits == 3 { 1152 } else { 576 };
        let coeff = if version_bits == 3 { 144usize } else { 72usize };
        let frame_len = (coeff * bitrate_kbps * 1000) / rate + padding;
        if frame_len < 4 || pos + frame_len > end {
            break;
        }
        frames.push((pos, pos + frame_len, samples));
        pos += frame_len;
    }
    frames
}

pub(super) fn prepare_swf_stream_sound(
    media_type: &str,
    raw: &[u8],
) -> Result<PreparedSwfStreamSound, String> {
    let info = analyze_swf_sound_resource(media_type, raw)?;
    let audio_bytes = raw[info.data_start..info.data_end].to_vec();
    let mp3_frames = if info.mp3 {
        parse_mp3_frame_slices(&audio_bytes)
    } else {
        Vec::new()
    };
    Ok(PreparedSwfStreamSound {
        info,
        audio_bytes,
        mp3_frames,
    })
}

pub(super) fn build_sound_stream_head2_payload(info: &SwfSoundInfo, fps: u32) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    let playback_flags: u8 = (info.rate_code << 2) | 0x02 | u8::from(info.stereo);
    let stream_flags: u8 = (info.sound_format << 4)
        | (info.rate_code << 2)
        | (u8::from(info.is_16_bit) << 1)
        | u8::from(info.stereo);
    let avg_samples = (f64::from(info.sample_rate) / f64::from(fps.max(1)))
        .round()
        .clamp(1.0, 65535.0) as u16;
    w.u8(playback_flags);
    w.u8(stream_flags);
    w.u16(avg_samples);
    if info.mp3 {
        w.i16(0); // LatencySeek = 0
    }
    w.buf
}

pub(super) fn plan_timeline_stream_sounds(
    timeline: &Timeline,
    total_frames: u32,
    fps: u32,
    prepared: &BTreeMap<String, PreparedSwfStreamSound>,
) -> TimelineStreamPlan {
    let mut head_by_frame = BTreeMap::new();
    let mut block_by_frame = BTreeMap::new();
    let mut fallback_event_frames = HashSet::new();
    let mut occupied_frames: HashSet<u32> = HashSet::new();
    let eff_fps = u64::from(fps.max(1));

    for (l_idx, layer) in timeline.layers.iter().enumerate() {
        if !layer.visible {
            continue;
        }
        let effective_end = layer
            .properties
            .end_frame
            .unwrap_or(total_frames)
            .min(total_frames);
        for (k_idx, kf) in layer.keyframes.iter().enumerate() {
            if kf.frame >= effective_end {
                break;
            }
            let Some(snd) = &kf.sound else {
                continue;
            };
            if snd.sync != SoundSync::Stream {
                continue;
            }
            let Some(stream_res) = prepared.get(&snd.resource) else {
                continue;
            };
            let span_end = layer.keyframes[k_idx + 1..]
                .iter()
                .find(|nk| nk.sound.is_some())
                .map_or(effective_end, |nk| nk.frame.min(effective_end));
            if span_end <= kf.frame {
                continue;
            }
            if (kf.frame..span_end).any(|f| occupied_frames.contains(&f)) {
                fallback_event_frames.insert((l_idx, kf.frame));
                continue;
            }

            let info = &stream_res.info;
            let rate = u64::from(info.sample_rate.max(1));
            let in_sample = snd
                .in_point
                .filter(|t| t.is_finite() && *t > 0.0)
                .map_or(0usize, |t| {
                    ((t * (rate as f64)).round().max(0.0) as usize).min(info.sample_count as usize)
                });
            let out_sample = snd
                .out_point
                .filter(|t| t.is_finite() && *t > 0.0)
                .map_or(info.sample_count as usize, |t| {
                    ((t * (rate as f64)).round().max(0.0) as usize).min(info.sample_count as usize)
                })
                .max(in_sample);
            let repeat_count: usize = if snd.loop_forever {
                (span_end - kf.frame) as usize + 1
            } else {
                snd.loops.clamp(1, 32767) as usize
            };

            let mut produced_any = false;
            if info.mp3 {
                // Collect trimmed single-pass MPEG frames
                let mut single_pass: Vec<(usize, usize, u16)> = Vec::new();
                let mut cur_s = 0usize;
                for &(bs, be, f_samples) in &stream_res.mp3_frames {
                    let next_s = cur_s + usize::from(f_samples);
                    if next_s > in_sample && cur_s < out_sample {
                        single_pass.push((bs, be, f_samples));
                    }
                    cur_s = next_s;
                }
                if !single_pass.is_empty() {
                    let max_total_frames =
                        ((span_end - kf.frame) as usize).saturating_mul(8).max(8);
                    let mut all_frames: Vec<(usize, usize, u16)> = Vec::new();
                    for _ in 0..repeat_count {
                        for item in &single_pass {
                            all_frames.push(*item);
                            if all_frames.len() >= max_total_frames {
                                break;
                            }
                        }
                        if all_frames.len() >= max_total_frames {
                            break;
                        }
                    }

                    let mut m_idx = 0usize;
                    let mut emitted_samples = 0u64;
                    for f in kf.frame..span_end {
                        if m_idx >= all_frames.len() {
                            break;
                        }
                        let offset = u64::from(f - kf.frame + 1);
                        let target_samples = (offset * rate) / eff_fps;
                        let mut block_samples: u32 = 0;
                        let mut block_mp3 = Vec::new();
                        while m_idx < all_frames.len()
                            && (emitted_samples < target_samples
                                || block_samples == 0 && offset == 1)
                        {
                            let (bs, be, fsamp) = all_frames[m_idx];
                            if block_samples.saturating_add(u32::from(fsamp)) > u32::from(u16::MAX)
                            {
                                break;
                            }
                            block_samples += u32::from(fsamp);
                            emitted_samples += u64::from(fsamp);
                            block_mp3.extend_from_slice(&stream_res.audio_bytes[bs..be]);
                            m_idx += 1;
                        }
                        if block_samples > 0 && !block_mp3.is_empty() {
                            let mut bw = SwfBitWriter::new();
                            bw.u16(block_samples as u16);
                            bw.i16(0); // SeekSamples = 0
                            bw.bytes(&block_mp3);
                            block_by_frame.insert(f, bw.buf);
                            occupied_frames.insert(f);
                            produced_any = true;
                        }
                    }
                }
            } else {
                let bps = (if info.is_16_bit { 2usize } else { 1 })
                    * (if info.stereo { 2usize } else { 1 });
                let total_avail_samples = stream_res.audio_bytes.len() / bps.max(1);
                let s_start = in_sample.min(total_avail_samples);
                let s_end = out_sample.min(total_avail_samples).max(s_start);
                let pass_samples = s_end - s_start;
                if pass_samples > 0 {
                    let max_needed_samples =
                        (((u64::from(span_end - kf.frame) + 1) * rate) / eff_fps) as usize + 16;
                    let total_virtual_samples = pass_samples
                        .saturating_mul(repeat_count)
                        .min(max_needed_samples);
                    for f in kf.frame..span_end {
                        let off0 = u64::from(f - kf.frame);
                        let off1 = off0 + 1;
                        let p0 = ((off0 * rate) / eff_fps) as usize;
                        let p1 = (((off1 * rate) / eff_fps) as usize).min(total_virtual_samples);
                        if p1 <= p0 {
                            if p0 >= total_virtual_samples {
                                break;
                            }
                            continue;
                        }
                        let mut pcm_chunk = Vec::with_capacity((p1 - p0) * bps);
                        let mut cur = p0;
                        while cur < p1 {
                            let in_pass = cur % pass_samples;
                            let take = (pass_samples - in_pass).min(p1 - cur);
                            let byte_s = (s_start + in_pass) * bps;
                            let byte_e = byte_s + take * bps;
                            pcm_chunk.extend_from_slice(&stream_res.audio_bytes[byte_s..byte_e]);
                            cur += take;
                        }
                        if !pcm_chunk.is_empty() {
                            block_by_frame.insert(f, pcm_chunk);
                            occupied_frames.insert(f);
                            produced_any = true;
                        }
                    }
                }
            }

            if produced_any {
                head_by_frame.insert(kf.frame, build_sound_stream_head2_payload(info, fps));
            }
        }
    }

    TimelineStreamPlan {
        head_by_frame,
        block_by_frame,
        fallback_event_frames,
    }
}
