//! SWF timeline `SoundStreamHead2` (tag 45) and per-frame `SoundStreamBlock` (tag 19) encoder.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct PreparedSwfStreamSound {
    pub(super) info: SwfSoundInfo,
    pub(super) audio_bytes: Vec<u8>,
    pub(super) mp3_frames: Vec<Mp3Frame>,
}

pub(super) struct TimelineStreamPlan {
    pub(super) head_by_frame: BTreeMap<u32, Vec<u8>>,
    pub(super) block_by_frame: BTreeMap<u32, Vec<u8>>,
    pub(super) fallback_event_frames: BTreeSet<(usize, u32)>,
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
    let mut fallback_event_frames = BTreeSet::new();
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
            let span_samples = u64::from(span_end - kf.frame) * rate / eff_fps;
            let repeats = u64::from(snd.loops.max(1));

            let mut produced_any = false;
            if info.mp3 {
                // MPEG frames are indivisible. Select the frames intersecting the trim once,
                // then cycle their descriptors on demand until the sound or timeline ends.
                let frames = &stream_res.mp3_frames;
                let first = frames.partition_point(|frame| frame.end_sample as usize <= in_sample);
                let end = frames.partition_point(|frame| {
                    (frame.end_sample - u32::from(frame.samples)) < out_sample as u32
                });
                let pass = &frames[first..end];
                if !pass.is_empty() && in_sample < out_sample {
                    let frame_limit = if snd.loop_forever {
                        u64::MAX
                    } else {
                        repeats * pass.len() as u64
                    };
                    let mut cursor = 0u64;
                    let mut emitted_samples = 0u64;
                    for f in kf.frame..span_end {
                        if cursor >= frame_limit {
                            break;
                        }
                        let offset = u64::from(f - kf.frame + 1);
                        let target_samples = (offset * rate) / eff_fps;
                        let mut block_samples: u32 = 0;
                        let mut block_mp3 = Vec::new();
                        while cursor < frame_limit
                            && (emitted_samples < target_samples
                                || block_samples == 0 && offset == 1)
                        {
                            let frame = pass[(cursor % pass.len() as u64) as usize];
                            if block_samples.saturating_add(u32::from(frame.samples))
                                > u32::from(u16::MAX)
                            {
                                break;
                            }
                            block_samples += u32::from(frame.samples);
                            emitted_samples += u64::from(frame.samples);
                            block_mp3
                                .extend_from_slice(&stream_res.audio_bytes[frame.start..frame.end]);
                            cursor += 1;
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
                    let total_virtual_samples = if snd.loop_forever {
                        span_samples
                    } else {
                        (pass_samples as u64 * repeats).min(span_samples)
                    } as usize;
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
