//! Integration tests for binary FLA Flash/SWF ADPCM audio decoding (2/3/4/5-bit,
//! mono/stereo, multi-block >4096 frames, short-read truncation) and format-7
//! custom multi-point stereo sound envelopes.

use neoflash_core::{FrameSound, SoundEffect, SoundSync};
use neoflash_import::{ImportFormat, import_project, inspect};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-import-adpcm-env-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

fn index_table(bits: usize) -> &'static [i32] {
    match bits {
        2 => &[-1, 2],
        3 => &[-1, -1, 2, 4],
        4 => &[-1, -1, -1, -1, 2, 4, 6, 8],
        5 => &[-1, -1, -1, -1, -1, -1, -1, -1, 1, 2, 4, 6, 8, 10, 13, 16],
        _ => panic!("unsupported ADPCM bit width {bits}"),
    }
}

struct MsbBitWriter {
    bytes: Vec<u8>,
    bit_len: usize,
}
impl MsbBitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_len: 0,
        }
    }
    fn write_bits(&mut self, value: u32, count: usize) {
        for i in (0..count).rev() {
            let bit = ((value >> i) & 1) as u8;
            if (self.bit_len & 7) == 0 {
                self.bytes.push(0);
            }
            let byte_idx = self.bit_len >> 3;
            let shift = 7 - (self.bit_len & 7);
            self.bytes[byte_idx] |= bit << shift;
            self.bit_len += 1;
        }
    }
}

/// Encodes interleaved PCM samples into Flash/SWF ADPCM (Adobe SWF v19 §10)
/// and returns `(encoded_bytes, expected_decoded_interleaved_pcm)`.
fn encode_flash_adpcm(adpcm_bits: usize, channels: usize, input: &[i16]) -> (Vec<u8>, Vec<i16>) {
    assert!((2..=5).contains(&adpcm_bits));
    assert!(channels == 1 || channels == 2);
    assert_eq!(input.len() % channels, 0);
    let total_frames = input.len() / channels;
    assert!(total_frames > 0);

    let idx_table = index_table(adpcm_bits);
    let sign_mask = 1usize << (adpcm_bits - 1);
    let mut writer = MsbBitWriter::new();
    writer.write_bits((adpcm_bits - 2) as u32, 2);

    let mut expected = Vec::with_capacity(input.len());
    let mut frame = 0usize;
    while frame < total_frames {
        let block_end = (frame + 4096).min(total_frames);
        let mut predictor = [0_i32; 2];
        let mut step_index = [0_i32; 2];
        for ch in 0..channels {
            let initial = input[frame * channels + ch];
            let init_idx = ((frame / 4096) * 17 + ch * 23) as i32 % 89;
            predictor[ch] = i32::from(initial);
            step_index[ch] = init_idx;
            writer.write_bits((initial as u16) as u32, 16);
            writer.write_bits(init_idx as u32, 6);
            expected.push(initial);
        }
        for f in (frame + 1)..block_end {
            for ch in 0..channels {
                let target = i32::from(input[f * channels + ch]);
                let delta = target - predictor[ch];
                let step = STEP_TABLE[step_index[ch] as usize];
                let mut rem = delta.abs();
                let mut mag = 0usize;
                let mut diff = step >> (adpcm_bits - 1);
                for bit in 0..(adpcm_bits - 1) {
                    let s = step >> bit;
                    if rem >= s {
                        mag |= 1 << (adpcm_bits - 2 - bit);
                        rem -= s;
                        diff += s;
                    }
                }
                let code = if delta < 0 { sign_mask | mag } else { mag };
                if (code & sign_mask) != 0 {
                    predictor[ch] = (predictor[ch] - diff).clamp(-32768, 32767);
                } else {
                    predictor[ch] = (predictor[ch] + diff).clamp(-32768, 32767);
                }
                step_index[ch] = (step_index[ch] + idx_table[mag]).clamp(0, 88);
                writer.write_bits(code as u32, adpcm_bits);
                expected.push(predictor[ch] as i16);
            }
        }
        frame = block_end;
    }

    (writer.bytes, expected)
}

struct MediaSpec<'a> {
    item_name: &'a str,
    sound_flags: u8,
    sample_count: u32,
    encoded_byte_count: u32,
    bytes: &'a [u8],
}

struct FrameSoundSpec<'a> {
    duration: u16,
    media_id: u16,
    sync: u8,
    loops: u16,
    sound_effect: u32,
    envelope: &'a [(u32, u16, u16)],
}

fn build_sound_fla(media: &[MediaSpec<'_>], frames: &[FrameSoundSpec<'_>]) -> Vec<u8> {
    fn u16b(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn u32b(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn string(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        out.extend_from_slice(s.as_bytes());
    }
    fn compact_utf16(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        for b in s.bytes() {
            u16b(out, u16::from(b));
        }
    }
    fn class(out: &mut Vec<u8>, name: &str) {
        u16b(out, 0xffff);
        u16b(out, 1);
        u16b(out, name.len() as u16);
        out.extend_from_slice(name.as_bytes());
    }
    fn point_min(out: &mut Vec<u8>) {
        out.extend_from_slice(&i32::MIN.to_le_bytes());
        out.extend_from_slice(&i32::MIN.to_le_bytes());
    }
    fn empty_shape(out: &mut Vec<u8>) {
        out.extend_from_slice(&[1, 0, 0, 0]);
        point_min(out);
        out.push(2);
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.push(5);
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&[0; 4]);
    }
    fn sound_frame(out: &mut Vec<u8>, spec: &FrameSoundSpec<'_>) {
        empty_shape(out);
        out.push(21); // CPicFrame rev 21
        u16b(out, spec.duration);
        u16b(out, 0); // key_mode
        u16b(out, 0); // acceleration
        u16b(out, spec.media_id);
        u16b(out, spec.envelope.len() as u16);
        for &(mark44, left, right) in spec.envelope {
            u32b(out, mark44);
            u16b(out, left);
            u16b(out, right);
        }
        u16b(out, spec.loops);
        out.push(spec.sync);
        u32b(out, 0); // in_point
        u32b(out, 0); // out_point
        u16b(out, 0); // sound_zoom
        string(out, ""); // label
        u32b(out, 0); // actions rev = 0
        u32b(out, 1); // actions mode = 1 (source)
        string(out, ""); // empty actions source
        u32b(out, 0); // rotation_mode
        u32b(out, 0); // rotation_count
        u32b(out, 0); // label_type
        u16b(out, 0); // morph_shape = null
        u32b(out, 0); // shape_tween_blend
        u16b(out, 0); // sound_settings = null
        string(out, ""); // extension
        u32b(out, 0); // reserved
        u32b(out, spec.sound_effect);
    }
    fn cmedia_sound(out: &mut Vec<u8>, stream_name: &str, media_id: u16, spec: &MediaSpec<'_>) {
        out.push(6);
        compact_utf16(out, stream_name);
        string(out, spec.item_name);
        u16b(out, media_id);
        string(out, "");
        u32b(out, 0);
        u32b(out, 3);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        out.push(8);
        u16b(out, u16::from(spec.sound_flags));
        u32b(out, spec.sample_count);
        out.extend_from_slice(&0_i16.to_le_bytes());
        out.extend_from_slice(&0_i16.to_le_bytes());
        u32b(out, spec.encoded_byte_count);
        out.push(0);
        u16b(out, 0);
        u32b(out, 0);
    }

    let mut contents = vec![0x3f];
    u32b(&mut contents, 1);
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut contents, 0);
    }
    class(&mut contents, "CDocumentPage");
    contents.push(23);
    compact_utf16(&mut contents, "Page 1");
    string(&mut contents, "Scene 1");
    u32b(&mut contents, 7);
    contents.push(0);
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 6);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(2);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(0);
    u32b(&mut contents, 1);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(1);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(7);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(0);
    u32b(&mut contents, 1);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(0);
    string(&mut contents, "");
    contents.push(8);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(&mut contents, "");
    for _ in 0..3 {
        contents.push(2);
        string(&mut contents, "");
        u32b(&mut contents, 0);
        u32b(&mut contents, 0);
    }
    string(&mut contents, "");
    contents.push(1);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    u32b(&mut contents, u32::MAX);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 8]);
    contents.push(3);
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 20]);

    u16b(&mut contents, 0); // end scenes
    u16b(&mut contents, 1);
    u16b(&mut contents, 0);
    u16b(&mut contents, 2);
    u16b(&mut contents, 0); // end symbols
    u16b(&mut contents, 0);

    for (idx, m) in media.iter().enumerate() {
        if idx == 0 {
            class(&mut contents, "CMediaSound"); // class index 3
        } else {
            u16b(&mut contents, 0x8003);
        }
        let stream_name = format!("Media {}", idx + 1);
        cmedia_sound(&mut contents, &stream_name, (idx + 1) as u16, m);
    }
    u16b(&mut contents, 0); // end media list
    u16b(&mut contents, media.len() as u16);

    u16b(&mut contents, 2);
    contents.push(0);
    for word in [0_i32, 11000, 0, 8000] {
        contents.extend_from_slice(&word.to_le_bytes());
    }
    u32b(&mut contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    u32b(&mut contents, 65536);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0xffffffff);
    u32b(&mut contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    u32b(&mut contents, 20);

    let mut page = vec![1];
    class(&mut page, "CPicPage"); // 1
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer"); // 3
    page.extend_from_slice(&[1, 0]);
    for (idx, f) in frames.iter().enumerate() {
        if idx == 0 {
            class(&mut page, "CPicFrame"); // 5
        } else {
            u16b(&mut page, 0x8005);
        }
        sound_frame(&mut page, f);
    }
    u16b(&mut page, 0); // end layer children
    point_min(&mut page);
    page.push(8);
    string(&mut page, "Audio");
    page.extend_from_slice(&[0; 3]);
    page.extend_from_slice(&[0; 16]);
    page.push(0);
    page.extend_from_slice(&[0; 5]);
    u16b(&mut page, 0); // end page children
    point_min(&mut page);
    page.push(3);
    u16b(&mut page, 1);
    page.extend_from_slice(&[0; 4]);

    let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    compound
        .create_stream("Contents")
        .unwrap()
        .write_all(&contents)
        .unwrap();
    compound
        .create_stream("Page 1")
        .unwrap()
        .write_all(&page)
        .unwrap();
    for (idx, m) in media.iter().enumerate() {
        let stream_name = format!("Media {}", idx + 1);
        compound
            .create_stream(&stream_name)
            .unwrap()
            .write_all(m.bytes)
            .unwrap();
    }
    compound.into_inner().into_inner()
}

fn synthetic_waveform(frames: usize, channels: usize) -> Vec<i16> {
    let mut out = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        let t = i as f64;
        let l = ((t * 0.19).sin() * 22_000.0 + (t * 0.037).cos() * 8_000.0)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        out.push(l);
        if channels == 2 {
            let r = ((t * 0.13).cos() * 20_000.0 - (t * 0.051).sin() * 9_500.0)
                .round()
                .clamp(-32768.0, 32767.0) as i16;
            out.push(r);
        }
    }
    out
}

fn parse_wav_pcm16(wav: &[u8]) -> (u16, u32, Vec<i16>) {
    assert!(wav.len() >= 44);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1); // PCM format tag
    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let sample_rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
    let bits_per_sample = u16::from_le_bytes([wav[34], wav[35]]);
    assert_eq!(bits_per_sample, 16);
    assert_eq!(&wav[36..40], b"data");
    let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
    assert_eq!(wav.len(), 44 + data_len);
    let samples = wav[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(i16::from_le_bytes)
        .collect();
    (channels, sample_rate, samples)
}

#[test]
fn binary_fla_decodes_2_3_4_5_bit_adpcm_mono_and_stereo_including_multiblock() {
    let temp = TempDir::new();
    let cases = [
        // (adpcm_bits, channels, frames, rate_code, expected_hz)
        (2usize, 1usize, 64usize, 0u8, 5_512u32),
        (2, 2, 48, 1, 11_025),
        (3, 1, 96, 2, 22_050),
        (3, 2, 72, 3, 44_100),
        (4, 1, 4_250, 2, 22_050), // multi-block (> 4096 sample frames)
        (4, 2, 128, 3, 44_100),
        (5, 1, 80, 1, 11_025),
        (5, 2, 4_120, 3, 44_100), // multi-block stereo (> 4096 sample frames)
    ];

    let mut encoded_blobs = Vec::new();
    let mut expected_pcms = Vec::new();
    for &(bits, ch, frames, _, _) in &cases {
        let src = synthetic_waveform(frames, ch);
        let (adpcm, expected_pcm) = encode_flash_adpcm(bits, ch, &src);
        encoded_blobs.push(adpcm);
        expected_pcms.push(expected_pcm);
    }

    let media_specs: Vec<MediaSpec<'_>> = cases
        .iter()
        .enumerate()
        .map(|(idx, &(_, ch, frames, rate_code, _))| {
            let flags = (1_u8 << 4) | (rate_code << 2) | 0x02 | u8::from(ch == 2);
            MediaSpec {
                item_name: if ch == 2 { "Stereo.wav" } else { "Mono.wav" },
                sound_flags: flags,
                sample_count: frames as u32,
                encoded_byte_count: encoded_blobs[idx].len() as u32,
                bytes: &encoded_blobs[idx],
            }
        })
        .collect();

    let frame_specs: Vec<FrameSoundSpec<'_>> = (0..cases.len())
        .map(|idx| FrameSoundSpec {
            duration: 1,
            media_id: (idx + 1) as u16,
            sync: 0,
            loops: 1,
            sound_effect: 0,
            envelope: &[],
        })
        .collect();

    let fla_path = temp.0.join("adpcm_matrix.fla");
    fs::write(&fla_path, build_sound_fla(&media_specs, &frame_specs)).unwrap();

    let report = inspect(&fla_path).unwrap();
    assert_eq!(report.format, ImportFormat::LegacyBinaryFla);
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report.diagnostics.iter().all(|d| !matches!(
            d.code.as_str(),
            "fla_media_not_imported" | "fla_sound_short_read" | "fla_frame_sound"
        )),
        "unexpected audio diagnostics: {:?}",
        report.diagnostics
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    assert_eq!(project.format_version, 6);

    for (idx, &(bits, ch, frames, _, expected_hz)) in cases.iter().enumerate() {
        let res_id = format!("legacy-sound-{}", idx + 1);
        let res = project
            .resources
            .iter()
            .find(|r| r.id == res_id)
            .unwrap_or_else(|| panic!("missing resource {res_id}"));
        assert_eq!(res.media_type, "audio/wav");
        let wav_bytes = res.decoded_bytes().unwrap();
        let (wav_ch, wav_hz, samples) = parse_wav_pcm16(&wav_bytes);
        assert_eq!(usize::from(wav_ch), ch, "case {idx} ({bits}-bit)");
        assert_eq!(wav_hz, expected_hz, "case {idx} ({bits}-bit)");
        assert_eq!(samples.len(), frames * ch, "case {idx} ({bits}-bit)");
        assert_eq!(
            samples, expected_pcms[idx],
            "PCM mismatch for {bits}-bit {ch}ch"
        );
    }
}

#[test]
fn binary_fla_adpcm_truncation_emits_short_read_warning_and_preserves_decoded_prefix() {
    let temp = TempDir::new();
    let src = synthetic_waveform(64, 1);
    let (full_adpcm, expected_pcm) = encode_flash_adpcm(4, 1, &src);
    // Keep 8 bytes = 64 bits: 2-bit code size + 22-bit header (1 frame) + 10 * 4-bit deltas = 11 frames.
    let truncated_adpcm = &full_adpcm[..8];

    let media = [
        // Media 1: stream shorter than declared encoded_byte_count AND sample_count (11 of 64 frames)
        MediaSpec {
            item_name: "TruncatedContainer.wav",
            sound_flags: 0x1a, // ADPCM, 22.05 kHz, 16-bit, mono
            sample_count: 64,
            encoded_byte_count: full_adpcm.len() as u32,
            bytes: truncated_adpcm,
        },
        // Media 2: encoded_byte_count matches byte length, but bitstream ends at 11 of 64 frames
        MediaSpec {
            item_name: "TruncatedBitstream.wav",
            sound_flags: 0x1a,
            sample_count: 64,
            encoded_byte_count: truncated_adpcm.len() as u32,
            bytes: truncated_adpcm,
        },
        // Media 3: truncated before first 22-bit block header (0 frames -> fla_sound_truncated)
        MediaSpec {
            item_name: "HeaderTruncated.wav",
            sound_flags: 0x1a,
            sample_count: 16,
            encoded_byte_count: 2,
            bytes: &full_adpcm[..2],
        },
    ];
    let frames = [
        FrameSoundSpec {
            duration: 2,
            media_id: 1,
            sync: 0,
            loops: 1,
            sound_effect: 0,
            envelope: &[],
        },
        FrameSoundSpec {
            duration: 2,
            media_id: 2,
            sync: 0,
            loops: 1,
            sound_effect: 0,
            envelope: &[],
        },
    ];

    let fla_path = temp.0.join("adpcm_truncated.fla");
    fs::write(&fla_path, build_sound_fla(&media, &frames)).unwrap();

    let report = inspect(&fla_path).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    let short_reads: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_sound_short_read")
        .collect();
    assert_eq!(
        short_reads.len(),
        2,
        "expected 2 fla_sound_short_read warnings, got {:?}",
        report.diagnostics
    );
    let not_imported: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_media_not_imported")
        .collect();
    assert_eq!(not_imported.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(not_imported[0].location, "media 3");
    assert!(
        not_imported[0].message.contains("fla_sound_truncated"),
        "{:?}",
        not_imported[0]
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    for res_id in ["legacy-sound-1", "legacy-sound-2"] {
        let res = project.resources.iter().find(|r| r.id == res_id).unwrap();
        let (ch, hz, samples) = parse_wav_pcm16(&res.decoded_bytes().unwrap());
        assert_eq!(ch, 1);
        assert_eq!(hz, 22_050);
        assert_eq!(samples, &expected_pcm[..11]);
    }
}

#[test]
fn binary_fla_imports_format_7_custom_multipoint_stereo_sound_envelopes() {
    let temp = TempDir::new();
    // 400 stereo frames at 44.1 kHz (duration_44 = 400):
    let pcm_bytes = vec![0_u8; 400 * 4];
    let media = [MediaSpec {
        item_name: "Bed.wav",
        sound_flags: 0x0f, // codec 0 (PCM), 44.1 kHz (3<<2), 16-bit (2), stereo (1)
        sample_count: 400,
        encoded_byte_count: pcm_bytes.len() as u32,
        bytes: &pcm_bytes,
    }];

    // Frame 0: 4-point custom stereo envelope (sound_effect = 7)
    let env_4pt = [
        (0_u32, 0_u16, 0x8000_u16), // pos 0.00: L=0.0, R=1.0
        (100, 0x4000, 0x4000),      // pos 0.25: L=0.5, R=0.5
        (300, 0x8000, 0x2000),      // pos 0.75: L=1.0, R=0.25
        (400, 0, 0),                // pos 1.00: L=0.0, R=0.0
    ];
    // Frame 1: 2-point partial-duration fade-in ending at 25% of duration (mark44 = 100 of 400)
    // stored with sound_effect = 0 — must be preserved as custom format-7 envelope, not stretched!
    let env_partial_fade = [(0_u32, 0_u16, 0_u16), (100, 0x8000, 0x8000)];

    let frames = [
        FrameSoundSpec {
            duration: 2,
            media_id: 1,
            sync: 1, // Start
            loops: 1,
            sound_effect: 7,
            envelope: &env_4pt,
        },
        FrameSoundSpec {
            duration: 2,
            media_id: 1,
            sync: 0, // Event
            loops: 1,
            sound_effect: 0,
            envelope: &env_partial_fade,
        },
    ];

    let fla_path = temp.0.join("custom_envelope.fla");
    fs::write(&fla_path, build_sound_fla(&media, &frames)).unwrap();

    let report = inspect(&fla_path).unwrap();
    assert_eq!(report.format, ImportFormat::LegacyBinaryFla);
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_frame_sound"),
        "expected zero fla_frame_sound warnings, got {:?}",
        report.diagnostics
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    assert_eq!(project.format_version, 7);

    let root = project.symbol(&project.root).unwrap();
    let kfs = &root.timeline.layers[0].keyframes;
    assert_eq!(kfs.len(), 2);

    let s0 = kfs[0].sound.as_ref().unwrap();
    assert_eq!(s0.resource, "legacy-sound-1");
    assert_eq!(s0.sync, SoundSync::Start);
    assert_eq!(s0.effect, SoundEffect::Custom);
    let expected_pts = vec![
        [0.0, 0.0, 1.0],
        [0.25, 0.5, 0.5],
        [0.75, 1.0, 0.25],
        [1.0, 0.0, 0.0],
    ];
    assert_eq!(s0.envelope, Some(expected_pts));

    let assert_gains = |sound: &FrameSound, t: f64, exp_l: f64, exp_r: f64| {
        let (l, r) = sound.gains_at(t);
        assert!(
            (l - exp_l).abs() < 1e-9 && (r - exp_r).abs() < 1e-9,
            "at t={t}: got ({l}, {r}), expected ({exp_l}, {exp_r})"
        );
    };
    assert_gains(s0, 0.0, 0.0, 1.0);
    assert_gains(s0, 0.125, 0.25, 0.75);
    assert_gains(s0, 0.25, 0.5, 0.5);
    assert_gains(s0, 0.5, 0.75, 0.375);
    assert_gains(s0, 0.75, 1.0, 0.25);
    assert_gains(s0, 0.875, 0.5, 0.125);
    assert_gains(s0, 1.0, 0.0, 0.0);

    let s1 = kfs[1].sound.as_ref().unwrap();
    assert_eq!(s1.effect, SoundEffect::Custom);
    assert_eq!(s1.envelope, Some(vec![[0.0, 0.0, 0.0], [0.25, 1.0, 1.0]]));
    assert_gains(s1, 0.125, 0.5, 0.5);
    assert_gains(s1, 0.5, 1.0, 1.0);
}

#[test]
fn binary_fla_envelope_preserves_format_6_full_span_presets_and_clamps_overflow_envelopes() {
    let temp = TempDir::new();
    let pcm_bytes = vec![0_u8; 400 * 2];
    let media = [MediaSpec {
        item_name: "Tone.wav",
        sound_flags: 0x0e, // PCM, 44.1 kHz, 16-bit, mono (duration_44 = 400)
        sample_count: 400,
        encoded_byte_count: pcm_bytes.len() as u32,
        bytes: &pcm_bytes,
    }];

    // 1) Exact full-span 2-point FadeIn with sound_effect = 7 stays format-6 preset:
    let full_fade_frames = [FrameSoundSpec {
        duration: 2,
        media_id: 1,
        sync: 0,
        loops: 1,
        sound_effect: 7,
        envelope: &[(0, 0, 0), (400, 0x8000, 0x8000)],
    }];
    let preset_fla = temp.0.join("preset_envelope.fla");
    fs::write(&preset_fla, build_sound_fla(&media, &full_fade_frames)).unwrap();
    let preset_proj = import_project(&preset_fla).unwrap();
    preset_proj.validate().unwrap();
    assert_eq!(preset_proj.format_version, 6);
    let preset_sound = preset_proj
        .symbol(&preset_proj.root)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes[0]
        .sound
        .as_ref()
        .unwrap();
    assert_eq!(preset_sound.effect, SoundEffect::FadeIn);
    assert_eq!(preset_sound.envelope, None);

    // 2) Out-of-order points, gain > 0x8000, and > 32 points clamp, sort, warn, and validate:
    let mut big_env: Vec<(u32, u16, u16)> = (0..35_u32)
        .map(|i| (i * 10, 0x4000, if i == 2 { 0xc000 } else { 0x4000 }))
        .collect();
    big_env.swap(1, 4); // introduce out-of-order mark44
    let clamp_frames = [FrameSoundSpec {
        duration: 2,
        media_id: 1,
        sync: 0,
        loops: 1,
        sound_effect: 7,
        envelope: &big_env,
    }];
    let clamp_fla = temp.0.join("clamped_envelope.fla");
    fs::write(&clamp_fla, build_sound_fla(&media, &clamp_frames)).unwrap();
    let clamp_report = inspect(&clamp_fla).unwrap();
    assert!(clamp_report.can_import);
    let env_warns: Vec<_> = clamp_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_frame_sound")
        .collect();
    assert_eq!(env_warns.len(), 2, "{:?}", clamp_report.diagnostics);

    let clamp_proj = import_project(&clamp_fla).unwrap();
    clamp_proj.validate().unwrap();
    assert_eq!(clamp_proj.format_version, 7);
    let clamped_sound = clamp_proj.symbol(&clamp_proj.root).unwrap().timeline.layers[0].keyframes
        [0]
    .sound
    .as_ref()
    .unwrap();
    assert_eq!(clamped_sound.effect, SoundEffect::Custom);
    let pts = clamped_sound.envelope.as_ref().unwrap();
    assert_eq!(pts.len(), FrameSound::MAX_ENVELOPE_POINTS);
    assert_eq!(pts[2][2], 1.0);
}
