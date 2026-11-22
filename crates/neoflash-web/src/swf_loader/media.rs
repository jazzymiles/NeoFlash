//! Audio (`DefineSound`, `StartSound`, `StartSound2`, `SoundStreamHead`/`SoundStreamHead2`,
//! `SoundStreamBlock`) and video (`DefineVideoStream`, `VideoFrame`, ScreenVideo raster +
//! FLV v1 container assembly) decoding for SWF `Loader.load` / `Loader.loadBytes`.

use super::{
    BitReader, MAX_BITMAP_RAW_BYTES, MAX_SWF_FRAMES, SwfDecodeCtx, encode_rgba_png,
    make_default_element,
};
use neoflash_core::{
    ElementKind, FrameSound, Keyframe, Layer, LayerProperties, ResourceUnit, SoundEffect,
    SoundSync, Symbol, SymbolKind, Timeline, TimelineVideoSpan,
};
use std::collections::BTreeMap;

const ADPCM_INDEX_2: [i32; 2] = [-1, 2];
const ADPCM_INDEX_3: [i32; 4] = [-1, -1, 2, 4];
const ADPCM_INDEX_4: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
const ADPCM_INDEX_5: [i32; 16] = [-1, -1, -1, -1, -1, -1, -1, -1, 1, 2, 4, 6, 8, 10, 13, 16];

const ADPCM_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

#[derive(Clone, Debug)]
pub(crate) struct SwfSoundAsset {
    pub(super) resource_id: String,
    pub(super) duration_44k: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct SwfVideoStreamDef {
    pub(super) stream_id: u16,
    pub(super) num_frames: u16,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) codec_id: u8,
    pub(super) frames: BTreeMap<u16, Vec<u8>>,
}

#[derive(Clone, Copy, Debug)]
struct StreamHeadSpec {
    compression: u8,
    sample_rate: u32,
    is_16_bit: bool,
    stereo: bool,
}

type StreamChunkEntry = (u32, Vec<u8>);
type StreamFinishedSegment = (StreamHeadSpec, u32, u32, Vec<StreamChunkEntry>);

#[derive(Default)]
pub(super) struct TimelineStreamCollector {
    head: Option<StreamHeadSpec>,
    active_start: Option<u32>,
    active_end: u32,
    active_chunks: Vec<StreamChunkEntry>,
    finished_segments: Vec<StreamFinishedSegment>,
}

impl TimelineStreamCollector {
    pub(super) fn on_head(&mut self, data: &[u8]) {
        self.flush_active();
        if data.len() < 4 {
            return;
        }
        let b1 = data[1];
        let compression = (b1 >> 4) & 0x0F;
        let rate_code = (b1 >> 2) & 0x03;
        let sample_rate = swf_rate_hz(rate_code);
        let is_16_bit = ((b1 >> 1) & 1) != 0;
        let stereo = (b1 & 1) != 0;
        self.head = Some(StreamHeadSpec {
            compression,
            sample_rate,
            is_16_bit,
            stereo,
        });
    }

    pub(super) fn on_block(&mut self, frame_idx: u32, payload: &[u8]) {
        if self.head.is_none() || payload.is_empty() {
            return;
        }
        if let Some(_start) = self.active_start {
            if frame_idx > self.active_end + 1 {
                self.flush_active();
                self.active_start = Some(frame_idx);
            }
        } else {
            self.active_start = Some(frame_idx);
        }
        self.active_end = frame_idx + 1;
        if self
            .active_chunks
            .iter()
            .map(|(_, b)| b.len())
            .sum::<usize>()
            + payload.len()
            <= MAX_BITMAP_RAW_BYTES
        {
            self.active_chunks.push((frame_idx, payload.to_vec()));
        }
    }

    fn flush_active(&mut self) {
        if let (Some(spec), Some(start)) = (self.head, self.active_start.take())
            && !self.active_chunks.is_empty()
        {
            let chunks = std::mem::take(&mut self.active_chunks);
            self.finished_segments
                .push((spec, start, self.active_end.max(start + 1), chunks));
        }
        self.active_chunks.clear();
    }

    pub(super) fn into_layers(
        mut self,
        ctx: &mut SwfDecodeCtx,
        scope_prefix: &str,
        total_frames: u32,
    ) -> Vec<Layer> {
        self.flush_active();
        let mut layers = Vec::new();
        for (seg_idx, (spec, start_frame, end_frame, chunks)) in
            self.finished_segments.into_iter().enumerate()
        {
            if start_frame >= total_frames || ctx.resources.len() >= 960 {
                continue;
            }
            let res_id = format!("swf-{}-{scope_prefix}-strm-{seg_idx}", ctx.mount_seq);
            let encoded = match spec.compression {
                2 => {
                    let mut mp3_bytes = Vec::new();
                    for (_, blk) in chunks {
                        if blk.len() > 4 {
                            mp3_bytes.extend_from_slice(&blk[4..]);
                        }
                    }
                    if mp3_bytes.is_empty() {
                        None
                    } else {
                        Some(("audio/mpeg", "mp3", mp3_bytes))
                    }
                }
                0 | 3 => {
                    let mut pcm_bytes = Vec::new();
                    for (_, blk) in chunks {
                        pcm_bytes.extend_from_slice(&blk);
                    }
                    if pcm_bytes.is_empty() {
                        None
                    } else {
                        let wav = build_wav_bytes(
                            &pcm_bytes,
                            spec.sample_rate,
                            if spec.stereo { 2 } else { 1 },
                            if spec.is_16_bit { 16 } else { 8 },
                        );
                        Some(("audio/wav", "wav", wav))
                    }
                }
                1 => {
                    let mut samples_i16 = Vec::new();
                    for (_, blk) in chunks {
                        if let Ok(decoded) = decode_swf_adpcm(&blk, None, spec.stereo) {
                            samples_i16.extend_from_slice(&decoded);
                        }
                    }
                    if samples_i16.is_empty() {
                        None
                    } else {
                        let mut pcm_le = Vec::with_capacity(samples_i16.len() * 2);
                        for s in samples_i16 {
                            pcm_le.extend_from_slice(&s.to_le_bytes());
                        }
                        let wav = build_wav_bytes(
                            &pcm_le,
                            spec.sample_rate,
                            if spec.stereo { 2 } else { 1 },
                            16,
                        );
                        Some(("audio/wav", "wav", wav))
                    }
                }
                other => {
                    ctx.warn(format!(
                        "SWF {scope_prefix} SoundStream uses unsupported compression {other}; skipped."
                    ));
                    None
                }
            };

            let Some((mime, ext, bytes)) = encoded else {
                continue;
            };
            let path = format!(
                "swf_stream_{}_{scope_prefix}_{seg_idx}.{ext}",
                ctx.mount_seq
            );
            let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), path, mime, &bytes) else {
                continue;
            };
            ctx.resources.push(unit);

            let mut keyframes = Vec::new();
            if start_frame > 0 {
                keyframes.push(Keyframe {
                    frame: 0,
                    ..Default::default()
                });
            }
            let mut snd = FrameSound::new(res_id);
            snd.sync = SoundSync::Stream;
            keyframes.push(Keyframe {
                frame: start_frame,
                sound: Some(snd),
                ..Default::default()
            });
            if end_frame < total_frames {
                keyframes.push(Keyframe {
                    frame: end_frame,
                    ..Default::default()
                });
            }
            layers.push(Layer {
                id: format!("swf-{}-{scope_prefix}-strmlyr-{seg_idx}", ctx.mount_seq),
                name: format!("Stream Sound {}", seg_idx + 1),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
        }
        layers
    }
}

fn swf_rate_hz(rate_code: u8) -> u32 {
    match rate_code & 0x03 {
        0 => 5512,
        1 => 11025,
        2 => 22050,
        _ => 44100,
    }
}

fn build_wav_bytes(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let block_align = channels.saturating_mul(bits_per_sample / 8).max(1);
    let byte_rate = sample_rate.saturating_mul(u32::from(block_align));
    let data_len = pcm.len().min(u32::MAX as usize - 44) as u32;
    let riff_len = 36u32.saturating_add(data_len);
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&pcm[..data_len as usize]);
    out
}

fn decode_swf_adpcm(
    data: &[u8],
    max_samples: Option<u32>,
    stereo: bool,
) -> Result<Vec<i16>, String> {
    let mut br = BitReader::new(data, 0);
    let code_bits = (br.read_ubits(2)? + 2) as usize;
    let idx_table: &[i32] = match code_bits {
        2 => &ADPCM_INDEX_2,
        3 => &ADPCM_INDEX_3,
        4 => &ADPCM_INDEX_4,
        5 => &ADPCM_INDEX_5,
        _ => return Err("Invalid SWF ADPCM code size".into()),
    };
    let channels = if stereo { 2usize } else { 1usize };
    let cap_frames = max_samples.unwrap_or(262_144).min(2_097_152) as usize;
    let mut out = Vec::with_capacity(cap_frames.min(65_536) * channels);
    let mut decoded_frames = 0usize;

    while decoded_frames < cap_frames && br.remaining_bytes() > 0 {
        let mut val = [0i32; 2];
        let mut step_idx = [0usize; 2];
        let mut header_ok = true;
        for ch in 0..channels {
            let Ok(init_sample) = br.read_sbits(16) else {
                header_ok = false;
                break;
            };
            let Ok(init_idx) = br.read_ubits(6) else {
                header_ok = false;
                break;
            };
            val[ch] = init_sample.clamp(-32768, 32767);
            step_idx[ch] = (init_idx as usize).min(88);
        }
        if !header_ok {
            break;
        }
        for &sample_val in val.iter().take(channels) {
            out.push(sample_val as i16);
        }
        decoded_frames += 1;

        let block_rem = (cap_frames - decoded_frames).min(4095);
        let sign_mask = 1u32 << (code_bits - 1);
        let mag_mask = sign_mask - 1;
        let mut eof = false;
        for _ in 0..block_rem {
            for ch in 0..channels {
                let Ok(delta) = br.read_ubits(code_bits) else {
                    eof = true;
                    break;
                };
                let sign = (delta & sign_mask) != 0;
                let mag = delta & mag_mask;
                let step = ADPCM_STEP_TABLE[step_idx[ch]];
                let mut vpdiff = step >> (code_bits - 1);
                for b in 0..(code_bits - 1) {
                    if (mag & (1 << (code_bits - 2 - b))) != 0 {
                        vpdiff += step >> b;
                    }
                }
                if sign {
                    val[ch] -= vpdiff;
                } else {
                    val[ch] += vpdiff;
                }
                val[ch] = val[ch].clamp(-32768, 32767);
                step_idx[ch] =
                    (step_idx[ch] as i32 + idx_table[mag as usize]).clamp(0, 88) as usize;
                out.push(val[ch] as i16);
            }
            if eof {
                break;
            }
            decoded_frames += 1;
            if out.len() * 2 >= MAX_BITMAP_RAW_BYTES {
                return Ok(out);
            }
        }
        if eof {
            break;
        }
    }
    Ok(out)
}

pub(super) fn decode_define_sound_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    if data.len() < 7 || ctx.resources.len() >= 960 {
        return Ok(());
    }
    let sound_id = u16::from_le_bytes([data[0], data[1]]);
    let flags = data[2];
    let format = (flags >> 4) & 0x0F;
    let rate_code = (flags >> 2) & 0x03;
    let sample_rate = swf_rate_hz(rate_code);
    let is_16_bit = ((flags >> 1) & 1) != 0;
    let stereo = (flags & 1) != 0;
    let sample_count = u32::from_le_bytes([data[3], data[4], data[5], data[6]]);
    let payload = &data[7..];

    let encoded = match format {
        2 => {
            if payload.len() <= 2 {
                return Ok(());
            }
            ("audio/mpeg", "mp3", payload[2..].to_vec())
        }
        0 | 3 => {
            if payload.is_empty() {
                return Ok(());
            }
            let capped = &payload[..payload.len().min(MAX_BITMAP_RAW_BYTES)];
            let wav = build_wav_bytes(
                capped,
                sample_rate,
                if stereo { 2 } else { 1 },
                if is_16_bit { 16 } else { 8 },
            );
            ("audio/wav", "wav", wav)
        }
        1 => {
            let samples = decode_swf_adpcm(payload, Some(sample_count.max(1)), stereo)?;
            if samples.is_empty() {
                return Ok(());
            }
            let mut pcm_le = Vec::with_capacity(samples.len() * 2);
            for s in samples {
                pcm_le.extend_from_slice(&s.to_le_bytes());
            }
            let wav = build_wav_bytes(&pcm_le, sample_rate, if stereo { 2 } else { 1 }, 16);
            ("audio/wav", "wav", wav)
        }
        other => {
            ctx.warn(format!(
                "SWF DefineSound {sound_id} uses unsupported codec format {other}; skipped."
            ));
            return Ok(());
        }
    };

    let res_id = format!("swf-{}-snd-{sound_id}", ctx.mount_seq);
    let path = format!("swf_snd_{}_{sound_id}.{}", ctx.mount_seq, encoded.1);
    let unit = ResourceUnit::from_bytes(res_id.clone(), path, encoded.0, &encoded.2)
        .map_err(|e| e.to_string())?;
    ctx.resources.push(unit);
    let duration_44k = (u64::from(sample_count.max(1)) * 44_100) / u64::from(sample_rate.max(1));
    ctx.char_sounds.insert(
        sound_id,
        SwfSoundAsset {
            resource_id: res_id,
            duration_44k: duration_44k.max(1),
        },
    );
    Ok(())
}

pub(super) fn decode_start_sound_tag(
    ctx: &SwfDecodeCtx,
    data: &[u8],
    is_start_sound2: bool,
) -> Option<FrameSound> {
    let mut r = BitReader::new(data, 0);
    let sound_id = if is_start_sound2 {
        let cls_name = r.read_c_string().ok()?;
        ctx.symbol_classes
            .iter()
            .find_map(|(&cid, nm)| (nm == &cls_name).then_some(cid))?
    } else {
        r.read_u16().ok()?
    };
    let asset = ctx.char_sounds.get(&sound_id)?;
    let flags = r.read_u8().ok()?;
    let sync_stop = (flags & 0x20) != 0;
    let sync_no_multiple = (flags & 0x10) != 0;
    let has_envelope = (flags & 0x08) != 0;
    let has_loops = (flags & 0x04) != 0;
    let has_out_point = (flags & 0x02) != 0;
    let has_in_point = (flags & 0x01) != 0;

    let in_44 = if has_in_point {
        Some(r.read_u32().ok()?)
    } else {
        None
    };
    let out_44 = if has_out_point {
        Some(r.read_u32().ok()?)
    } else {
        None
    };
    let loop_raw = if has_loops { r.read_u16().ok()? } else { 1 };

    let mut sound = FrameSound::new(asset.resource_id.clone());
    sound.sync = if sync_stop {
        SoundSync::Stop
    } else if sync_no_multiple {
        SoundSync::Start
    } else {
        SoundSync::Event
    };

    if !sync_stop {
        if loop_raw >= 0x7fff {
            sound.loop_forever = true;
            sound.loops = 1;
        } else {
            sound.loops = u32::from(loop_raw).clamp(1, FrameSound::MAX_LOOPS);
        }

        if let Some(in_s) = in_44 {
            let sec = (f64::from(in_s) / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS);
            if sec > 0.0 {
                sound.in_point = Some(sec);
            }
        }
        if let Some(out_s) = out_44 {
            let sec = (f64::from(out_s) / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS);
            if sound.in_point.is_none_or(|inp| sec > inp) && sec > 0.0 {
                sound.out_point = Some(sec);
            }
        }

        if has_envelope && let Ok(env_cnt) = r.read_u8() {
            let count = usize::from(env_cnt).min(FrameSound::MAX_ENVELOPE_POINTS);
            let start_44 = u64::from(in_44.unwrap_or(0));
            let end_44 = out_44
                .map(u64::from)
                .unwrap_or(asset.duration_44k)
                .max(start_44 + 1);
            let span_44 = (end_44 - start_44).max(1) as f64;
            let mut pts = Vec::with_capacity(count);
            for _ in 0..count {
                let (Ok(pos44), Ok(ll), Ok(rl)) = (r.read_u32(), r.read_u16(), r.read_u16()) else {
                    break;
                };
                let rel = (f64::from(pos44) - start_44 as f64).max(0.0);
                let norm_pos = (rel / span_44).clamp(0.0, 1.0);
                let left = (f64::from(ll) / 32768.0).clamp(0.0, 1.0);
                let right = (f64::from(rl) / 32768.0).clamp(0.0, 1.0);
                pts.push([norm_pos, left, right]);
            }
            if !pts.is_empty() {
                pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
                sound.effect = SoundEffect::Custom;
                sound.envelope = Some(pts);
            }
        }
    }
    Some(sound)
}

pub(super) fn build_event_sound_layers(
    ctx: &SwfDecodeCtx,
    scope_prefix: &str,
    events: Vec<(u32, FrameSound)>,
    total_frames: u32,
) -> Vec<Layer> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut tracks: Vec<BTreeMap<u32, FrameSound>> = Vec::new();
    for (frame, snd) in events {
        if frame >= total_frames {
            continue;
        }
        if let Some(slot) = tracks.iter_mut().find(|t| !t.contains_key(&frame)) {
            slot.insert(frame, snd);
        } else if tracks.len() < 16 {
            let mut map = BTreeMap::new();
            map.insert(frame, snd);
            tracks.push(map);
        }
    }
    let mut out = Vec::with_capacity(tracks.len());
    for (t_idx, map) in tracks.into_iter().enumerate() {
        let mut keyframes = Vec::with_capacity(map.len() * 2 + 1);
        if !map.contains_key(&0) {
            keyframes.push(Keyframe {
                frame: 0,
                ..Default::default()
            });
        }
        for (frame, snd) in &map {
            keyframes.push(Keyframe {
                frame: *frame,
                sound: Some(snd.clone()),
                ..Default::default()
            });
            let next_f = frame + 1;
            if next_f < total_frames && !map.contains_key(&next_f) {
                keyframes.push(Keyframe {
                    frame: next_f,
                    ..Default::default()
                });
            }
        }
        out.push(Layer {
            id: format!("swf-{}-{scope_prefix}-evsnd-{t_idx}", ctx.mount_seq),
            name: format!("Event Sound {}", t_idx + 1),
            visible: true,
            locked: false,
            keyframes,
            properties: LayerProperties::default(),
        });
    }
    out
}

pub(super) fn decode_define_video_stream_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) {
    if data.len() < 10 {
        return;
    }
    let stream_id = u16::from_le_bytes([data[0], data[1]]);
    let num_frames = u16::from_le_bytes([data[2], data[3]]);
    let width = u16::from_le_bytes([data[4], data[5]]).clamp(1, 4096);
    let height = u16::from_le_bytes([data[6], data[7]]).clamp(1, 4096);
    let codec_id = data[9];
    ctx.video_streams.insert(
        stream_id,
        SwfVideoStreamDef {
            stream_id,
            num_frames,
            width,
            height,
            codec_id,
            frames: BTreeMap::new(),
        },
    );
}

pub(super) fn decode_video_frame_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) {
    if data.len() < 4 {
        return;
    }
    let stream_id = u16::from_le_bytes([data[0], data[1]]);
    let frame_num = u16::from_le_bytes([data[2], data[3]]);
    if let Some(def) = ctx.video_streams.get_mut(&stream_id)
        && def.frames.len() < MAX_SWF_FRAMES as usize
    {
        let cur_total: usize = def.frames.values().map(Vec::len).sum();
        let payload = &data[4..];
        if cur_total + payload.len() <= MAX_BITMAP_RAW_BYTES {
            def.frames.insert(frame_num, payload.to_vec());
        }
    }
}

fn build_flv_from_video_frames(def: &SwfVideoStreamDef, fps: f64) -> Vec<u8> {
    let mut flv = Vec::new();
    // FLV v1 header: signature "FLV", version 1, flags 0x01 (video), header size 9, PreviousTagSize0 = 0
    flv.extend_from_slice(&[b'F', b'L', b'V', 0x01, 0x01, 0x00, 0x00, 0x00, 0x09]);
    flv.extend_from_slice(&0u32.to_be_bytes());
    let eff_fps = fps.clamp(1.0, 120.0);

    for (idx, (&frame_num, raw)) in def.frames.iter().enumerate() {
        let ts_ms = ((f64::from(frame_num) * 1000.0) / eff_fps).round().max(0.0) as u32;
        let already_prefixed = raw.len() >= 5
            && (raw[0] & 0x0F) == def.codec_id
            && matches!(raw[0] >> 4, 1..=3)
            && (def.codec_id != 3
                || ((u16::from_be_bytes([raw[1], raw[2]]) & 0x0FFF) == def.width
                    && (u16::from_be_bytes([raw[3], raw[4]]) & 0x0FFF) == def.height));

        let data_size = if already_prefixed {
            raw.len()
        } else if def.codec_id == 7 {
            raw.len() + 5 // AVC: header byte + AVCPacketType(1) + CompositionTime(3)
        } else {
            raw.len() + 1
        } as u32;

        // TagType = 9 (video)
        flv.push(0x09);
        // DataSize (UI24 BE)
        flv.push(((data_size >> 16) & 0xFF) as u8);
        flv.push(((data_size >> 8) & 0xFF) as u8);
        flv.push((data_size & 0xFF) as u8);
        // Timestamp (UI24 BE) + TimestampExtended (UI8)
        flv.push(((ts_ms >> 16) & 0xFF) as u8);
        flv.push(((ts_ms >> 8) & 0xFF) as u8);
        flv.push((ts_ms & 0xFF) as u8);
        flv.push(((ts_ms >> 24) & 0xFF) as u8);
        // StreamID (UI24 = 0)
        flv.extend_from_slice(&[0, 0, 0]);

        if already_prefixed {
            flv.extend_from_slice(raw);
        } else {
            let frame_type: u8 = if idx == 0 { 1 } else { 2 };
            flv.push((frame_type << 4) | (def.codec_id & 0x0F));
            if def.codec_id == 7 {
                flv.extend_from_slice(&[1, 0, 0, 0]); // AVC NALU
            }
            flv.extend_from_slice(raw);
        }
        let prev_tag_size = 11u32.saturating_add(data_size);
        flv.extend_from_slice(&prev_tag_size.to_be_bytes());
    }
    flv
}

fn decode_screen_video_layer(ctx: &mut SwfDecodeCtx, def: &SwfVideoStreamDef) -> Option<Layer> {
    let w = usize::from(def.width);
    let h = usize::from(def.height);
    if w == 0 || h == 0 || w * h * 4 > MAX_BITMAP_RAW_BYTES {
        return None;
    }
    let mut canvas_rgba = vec![0u8; w * h * 4];
    for px in canvas_rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let mut keyframes = Vec::new();
    let mut image_budget = ctx.resources.iter().filter(|r| r.is_image()).count();

    for (&frame_num, raw) in &def.frames {
        if image_budget >= 220 || ctx.resources.len() >= 960 {
            break;
        }
        let pkt = if raw.len() >= 5
            && (raw[0] & 0x0F) == 3
            && (u16::from_be_bytes([raw[1], raw[2]]) & 0x0FFF) == def.width
            && (u16::from_be_bytes([raw[3], raw[4]]) & 0x0FFF) == def.height
        {
            &raw[1..]
        } else {
            raw.as_slice()
        };
        if pkt.len() < 4 {
            continue;
        }
        let h0 = u16::from_be_bytes([pkt[0], pkt[1]]);
        let h1 = u16::from_be_bytes([pkt[2], pkt[3]]);
        let block_w = usize::from(((h0 >> 12) & 0x0F) + 1) * 16;
        let img_w = usize::from(h0 & 0x0FFF);
        let block_h = usize::from(((h1 >> 12) & 0x0F) + 1) * 16;
        let img_h = usize::from(h1 & 0x0FFF);
        if img_w != w || img_h != h || block_w == 0 || block_h == 0 {
            continue;
        }

        let cols = w.div_ceil(block_w);
        let rows = h.div_ceil(block_h);
        let mut pos = 4usize;
        let mut decoded_any = false;

        for br in 0..rows {
            let cur_h = block_h.min(h - br * block_h);
            for bc in 0..cols {
                if pos + 2 > pkt.len() {
                    break;
                }
                let cur_w = block_w.min(w - bc * block_w);
                let data_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
                pos += 2;
                if data_size == 0 {
                    decoded_any = true;
                    continue;
                }
                if pos + data_size > pkt.len() {
                    break;
                }
                let zslice = &pkt[pos..pos + data_size];
                pos += data_size;
                if let Ok(bgr) = neoflash_core::as_binary::binary_transform(
                    "uncompress",
                    zslice,
                    "",
                    "zlib",
                    cur_w * cur_h * 3 + 64,
                ) {
                    decoded_any = true;
                    for by in 0..cur_h {
                        // ScreenVideo blocks and scanlines inside each block are bottom-up
                        let bottom_y = br * block_h + by;
                        let top_y = h - 1 - bottom_y;
                        for bx in 0..cur_w {
                            let si = (by * cur_w + bx) * 3;
                            if si + 3 <= bgr.len() {
                                let dst_x = bc * block_w + bx;
                                let di = (top_y * w + dst_x) * 4;
                                canvas_rgba[di] = bgr[si + 2];
                                canvas_rgba[di + 1] = bgr[si + 1];
                                canvas_rgba[di + 2] = bgr[si];
                                canvas_rgba[di + 3] = 255;
                            }
                        }
                    }
                }
            }
        }

        if !decoded_any {
            continue;
        }
        let Ok(png_bytes) =
            encode_rgba_png(u32::from(def.width), u32::from(def.height), &canvas_rgba)
        else {
            continue;
        };
        let res_id = format!("swf-{}-svbmp-{}-{frame_num}", ctx.mount_seq, def.stream_id);
        let path = format!("swf_sv_{}_{}_{frame_num}.png", ctx.mount_seq, def.stream_id);
        let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), path, "image/png", &png_bytes)
        else {
            continue;
        };
        ctx.resources.push(unit);
        image_budget += 1;

        if keyframes.is_empty() && frame_num > 0 {
            keyframes.push(Keyframe {
                frame: 0,
                ..Default::default()
            });
        }
        let mut kf = Keyframe {
            frame: u32::from(frame_num),
            ..Default::default()
        };
        kf.elements.push(make_default_element(
            format!("swf-{}-svel-{}-{frame_num}", ctx.mount_seq, def.stream_id),
            ElementKind::Bitmap {
                resource_id: res_id,
                width: u32::from(def.width),
                height: u32::from(def.height),
                smoothing: false,
            },
            0.0,
            0.0,
        ));
        keyframes.push(kf);
    }

    if keyframes.is_empty() {
        return None;
    }
    Some(Layer {
        id: format!("swf-{}-svlyr-{}", ctx.mount_seq, def.stream_id),
        name: "ScreenVideo Frames".into(),
        visible: true,
        locked: false,
        keyframes,
        properties: LayerProperties::default(),
    })
}

pub(super) fn finalize_video_streams(ctx: &mut SwfDecodeCtx, frame_rate: f64) {
    let defs: Vec<SwfVideoStreamDef> = std::mem::take(&mut ctx.video_streams)
        .into_values()
        .collect();
    for def in defs {
        let stream_id = def.stream_id;
        if def.frames.is_empty() {
            ctx.char_videos
                .insert(stream_id, (f64::from(def.width), f64::from(def.height)));
            continue;
        }
        if ctx.resources.len() >= 960 {
            continue;
        }
        let flv_bytes = build_flv_from_video_frames(&def, frame_rate);
        let res_id = format!("swf-{}-vid-{stream_id}", ctx.mount_seq);
        let path = format!("swf_video_{}_{stream_id}.flv", ctx.mount_seq);
        let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), path, "video/x-flv", &flv_bytes)
        else {
            continue;
        };
        ctx.resources.push(unit);

        let max_frame = def.frames.keys().next_back().copied().unwrap_or(0);
        let frame_count =
            u32::from(def.num_frames.max(max_frame.saturating_add(1))).clamp(1, MAX_SWF_FRAMES);

        let mut vid_kf = Keyframe {
            frame: 0,
            video: Some(TimelineVideoSpan::embedded(res_id)),
            ..Default::default()
        };
        vid_kf.elements.push(make_default_element(
            format!("swf-{}-videl-{stream_id}", ctx.mount_seq),
            ElementKind::Video {
                width: f64::from(def.width),
                height: f64::from(def.height),
            },
            0.0,
            0.0,
        ));
        let mut layers = vec![Layer {
            id: format!("swf-{}-vidlyr-{stream_id}", ctx.mount_seq),
            name: "Video Stream".into(),
            visible: true,
            locked: false,
            keyframes: vec![vid_kf],
            properties: LayerProperties::default(),
        }];

        if def.codec_id == 3
            && let Some(sv_layer) = decode_screen_video_layer(ctx, &def)
        {
            layers.push(sv_layer);
        }

        let sym_id = format!("swf-{}-char-{stream_id}", ctx.mount_seq);
        ctx.symbols.push(Symbol {
            id: sym_id.clone(),
            name: format!("Video_{stream_id}"),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        ctx.char_symbols.insert(stream_id, sym_id);
    }
}
