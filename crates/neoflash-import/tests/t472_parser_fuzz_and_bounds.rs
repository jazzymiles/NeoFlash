//! `[T-472]` Phase 2 — Deterministic Seeded Fuzzing & Boundary Hardening Suite
//! across all binary, archive, vector, and project container parsers (`< 5s` debug runtime).
//!
//! Fixed PRNG seed: `0x4E46_3437_3200` (`0` external crates).
//! Exercises prefix truncations, boundary length/count corruptions (`u16::MAX`, `u32::MAX`,
//! `i32::MIN`, `u64::MAX`, negative counts, cyclic offsets, invalid UTF-8, unpaired UTF-16LE
//! surrogates), and structure-aware inner-stream mutations across:
//! 1. `Project::from_nflash_bytes` & `Project::from_json` (`NFLASHB1` + JSON)
//! 2. `OversizedFlaBundle` & `reconstruct_legacy_fla_bytes` (`NFD1`)
//! 3. Binary `.fla` (`neoflash_import::inspect` & `import_project` on raw CFB + mutated
//!    `Contents` / `Page 1` (`CPicShape`, `CPicText`, `ActionRecord`) / `CMediaBits` / `CMediaSound`)
//! 4. `.swf` (`FWS` / `CWS` / `ZWS` + mutated `DefineShape4`, `DefineMorphShape2`,
//!    `DefineVideoStream`, `VideoFrame` `ScreenVideo` v1/v2, `DoAction`, `DoABC2` in both
//!    `import_project` and `SwfBytecodePlayer::from_bytes` + `step_frame`)
//! 5. `.fig` (`inspect_figma_bytes`, `import_figma_bytes`, `import_figma_clipboard_bytes`
//!    on `fig-kiwi` Deflate + Zstd v106, `.fig` ZIP, and `.figma.json`)
//! 6. SVG, FXG, PDF/AI/EPS, and PSD importers (`import_vector_bytes_with_options`,
//!    `import_svg_with_options`, `import_fxg_str`, `import_pdf_ai_with_options`,
//!    `import_pdf_clipboard`, `import_eps_clipboard`, `parse_psd_document`, `import_psd_bytes`)

use flate2::{
    Compression,
    write::{DeflateEncoder, GzEncoder, ZlibEncoder},
};
use neoflash_core::extensions::sha256_hex;
use neoflash_core::{NflashBundleEntry, NflashBundleManifest, Project, ResourceUnit};
use neoflash_import::{
    FigmaImportOptions, OversizedChunkRole, OversizedDeltaReference, OversizedFlaBundle,
    OversizedMediaChunk, OversizedSourceArchive, PsdImportOptions, SwfBytecodePlayer,
    VectorImportOptions, encode_packbits_channel, import_act_palette, import_ase_palette,
    import_clr_palette, import_eps_clipboard, import_figma_bytes, import_figma_clipboard_bytes,
    import_fxg_str, import_legacy_fla_oversized_bundle, import_pdf_ai_with_options,
    import_pdf_clipboard, import_project, import_psd_bytes, import_svg_with_options,
    import_swc_bytes, import_vector_bytes_with_options, inspect, inspect_figma_bytes,
    parse_psd_document, reconstruct_legacy_fla_bytes,
};
use std::io::{Cursor, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const FUZZ_SEED: u64 = 0x4E46_3437_3200;

/// Deterministic zero-dependency SplitMix64 PRNG.
#[derive(Clone)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next_u64() as usize) % upper
        }
    }

    fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    /// Deterministically corrupt `base` with bit-flips, boundary integers (`u16::MAX`, `u32::MAX`,
    /// `i32::MIN`, `u64::MAX`, negative LE/BE counts), invalid UTF-8 (`0xFF`, `0xC0 0xAF`), and
    /// unpaired UTF-16 surrogates (`0x00 0xD8`, `0xFF 0xDF`).
    fn mutate(&mut self, base: &[u8]) -> Vec<u8> {
        let mut buf = base.to_vec();
        if buf.is_empty() {
            return vec![self.next_u8()];
        }
        let ops = 1 + self.next_usize(5);
        for _ in 0..ops {
            let mode = self.next_usize(10);
            let pos = self.next_usize(buf.len());
            match mode {
                0 => {
                    // Single bit flip
                    buf[pos] ^= 1u8 << (self.next_usize(8) as u8);
                }
                1 => {
                    // Random byte overwrite
                    buf[pos] = self.next_u8();
                }
                2 if pos + 2 <= buf.len() => {
                    let pat: [u8; 2] = match self.next_usize(4) {
                        0 => u16::MAX.to_le_bytes(),
                        1 => i16::MIN.to_le_bytes(),
                        2 => [0x00, 0xD8], // UTF-16LE unpaired high surrogate
                        _ => [0xFF, 0xDF], // UTF-16LE unpaired low surrogate
                    };
                    buf[pos..pos + 2].copy_from_slice(&pat);
                }
                3 if pos + 4 <= buf.len() => {
                    let pat: [u8; 4] = match self.next_usize(6) {
                        0 => u32::MAX.to_le_bytes(),
                        1 => u32::MAX.to_be_bytes(),
                        2 => i32::MIN.to_le_bytes(),
                        3 => i32::MIN.to_be_bytes(),
                        4 => (-1i32).to_le_bytes(),
                        _ => 0x7FFF_FFFFu32.to_le_bytes(),
                    };
                    buf[pos..pos + 4].copy_from_slice(&pat);
                }
                4 if pos + 8 <= buf.len() => {
                    let pat: [u8; 8] = match self.next_usize(4) {
                        0 => u64::MAX.to_le_bytes(),
                        1 => u64::MAX.to_be_bytes(),
                        2 => i64::MIN.to_le_bytes(),
                        _ => (513u64 * 1024 * 1024).to_le_bytes(),
                    };
                    buf[pos..pos + 8].copy_from_slice(&pat);
                }
                5 => {
                    // Invalid UTF-8 sequences (overlong + lone continuation + 0xFF)
                    let bad = [0xC0, 0xAF, 0xED, 0xA0, 0x80, 0xFF];
                    let take = bad.len().min(buf.len() - pos);
                    buf[pos..pos + take].copy_from_slice(&bad[..take]);
                }
                6 => {
                    // Delete slice
                    let del = 1 + self.next_usize((buf.len() - pos).min(16));
                    buf.drain(pos..pos + del);
                    if buf.is_empty() {
                        buf.push(0);
                    }
                }
                7 if buf.len() < 65_536 => {
                    // Duplicate slice / splice
                    let len = 1 + self.next_usize((buf.len() - pos).min(16));
                    let slice = buf[pos..pos + len].to_vec();
                    let dst = self.next_usize(buf.len() + 1);
                    buf.splice(dst..dst, slice);
                }
                8 => {
                    // Zero-fill small window (triggers divide-by-zero / zero-stride bugs)
                    let len = 1 + self.next_usize((buf.len() - pos).min(12));
                    buf[pos..pos + len].fill(0);
                }
                _ => {
                    // 0xFF-fill small window
                    let len = 1 + self.next_usize((buf.len() - pos).min(12));
                    buf[pos..pos + len].fill(0xFF);
                }
            }
        }
        buf
    }
}

/// Generate deterministic prefix truncation lengths covering `0..=36`, tail boundaries, and stepped prefixes.
fn truncation_lengths(total_len: usize, max_mid_samples: usize) -> Vec<usize> {
    let mut lens = Vec::new();
    for l in 0..=total_len.min(36) {
        lens.push(l);
    }
    if total_len > 36 {
        let span = total_len - 36;
        let step = (span / max_mid_samples.max(1)).max(1);
        let mut cur = 37;
        while cur < total_len {
            lens.push(cur);
            cur += step;
        }
        for tail in [
            total_len.saturating_sub(4),
            total_len.saturating_sub(2),
            total_len.saturating_sub(1),
            total_len,
        ] {
            lens.push(tail);
        }
    }
    lens.sort_unstable();
    lens.dedup();
    lens
}

fn assert_no_panic<F: FnOnce()>(context: &str, f: F) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_owned()
        };
        panic!("PANIC caught during fuzzing [{context}]: {msg}");
    }
}

struct TempScratchDir(PathBuf);

impl TempScratchDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-t472-fuzz-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp scratch dir");
        Self(path)
    }

    fn write_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).expect("write temp fuzz input");
        p
    }
}

impl Drop for TempScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// =========================================================================
// Family 1: `Project::from_nflash_bytes` & `Project::from_json` (`NFLASHB1` + JSON)
// =========================================================================

fn build_valid_nflashb1_bundle() -> (Vec<u8>, Vec<u8>) {
    let mut proj = Project::blank();
    proj.name = "T472 Bundle Seed".into();
    let res = ResourceUnit::from_bytes(
        "res_1",
        "assets/tone.bin",
        "application/octet-stream",
        b"NEOFLASH_RESOURCE_PAYLOAD_0123456789",
    )
    .unwrap();
    proj.resources.push(res);
    proj.upgrade_format_for_features();
    let json_bytes = proj.to_json().unwrap().into_bytes();

    // Also build an explicit `NFLASHB1` binary container for `proj`.
    let mut skeleton = proj.clone();
    skeleton.resources[0].data_base64 = "".into();
    let skel_json = serde_json::to_vec(&skeleton).unwrap();
    let raw_blob = b"NEOFLASH_RESOURCE_PAYLOAD_0123456789";
    let manifest = NflashBundleManifest {
        version: 1,
        resources: vec![NflashBundleEntry {
            id: "res_1".into(),
            path: "assets/tone.bin".into(),
            media_type: "application/octet-stream".into(),
            offset: 0,
            len: raw_blob.len() as u64,
            sha256: sha256_hex(raw_blob),
        }],
    };
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let mut bundle = Vec::new();
    bundle.extend_from_slice(b"NFLASHB1");
    bundle.extend_from_slice(&1u32.to_le_bytes());
    bundle.extend_from_slice(&(skel_json.len() as u64).to_le_bytes());
    bundle.extend_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
    bundle.extend_from_slice(&skel_json);
    bundle.extend_from_slice(&manifest_bytes);
    bundle.extend_from_slice(raw_blob);

    let decoded = Project::from_nflash_bytes(&bundle).expect("valid NFLASHB1 seed must decode");
    decoded.validate().expect("decoded NFLASHB1 must validate");
    (json_bytes, bundle)
}

#[test]
fn t472_fuzz_family_1_project_nflashb1_and_json_bounds() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x01);
    let (valid_json, valid_bundle) = build_valid_nflashb1_bundle();
    let mut cases = 0usize;

    // 1A. Prefix truncations across both JSON and NFLASHB1
    for (label, seed) in [("json", &valid_json), ("nflashb1", &valid_bundle)] {
        for len in truncation_lengths(seed.len(), 48) {
            let slice = &seed[..len];
            cases += 1;
            assert_no_panic(&format!("{label}_trunc_{len}"), || {
                if let Ok(p) = Project::from_nflash_bytes(slice) {
                    p.validate().expect("any Ok Project must pass validate()");
                }
                if let Ok(s) = std::str::from_utf8(slice)
                    && let Ok(p) = Project::from_json(s)
                {
                    p.validate()
                        .expect("any Ok Project::from_json must pass validate()");
                }
            });
        }
    }

    // 1B. Adversarial NFLASHB1 headers (u64::MAX, huge json_len/manifest_len, overlapping/OOB offsets)
    let skel_only = serde_json::to_vec(&Project::blank()).unwrap();
    for (tag, ver, json_len, man_len, extra) in [
        ("ver_0", 0u32, 0u64, 0u64, &[][..]),
        ("ver_max", u32::MAX, 0, 0, &[]),
        ("json_u64_max", 1, u64::MAX, 1, b"{}"),
        ("man_u64_max", 1, 2, u64::MAX, b"{}"),
        ("sum_overflow", 1, u64::MAX - 10, 32, b"{}"),
        ("huge_json_len", 1, 200 * 1024 * 1024, 2, b"{}"),
        ("huge_man_len", 1, 2, 32 * 1024 * 1024, b"{}"),
        ("oob_slice", 1, 4096, 4096, b"{}"),
    ] {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"NFLASHB1");
        raw.extend_from_slice(&ver.to_le_bytes());
        raw.extend_from_slice(&json_len.to_le_bytes());
        raw.extend_from_slice(&man_len.to_le_bytes());
        raw.extend_from_slice(extra);
        cases += 1;
        assert_no_panic(&format!("nflashb1_adv_{tag}"), || {
            assert!(Project::from_nflash_bytes(&raw).is_err());
        });
    }

    // Manifest with u64::MAX offset + length wraparound
    for (offset, len) in [(u64::MAX, 1u64), (10, u64::MAX), (0, 1024 * 1024)] {
        let bad_man = serde_json::to_vec(&NflashBundleManifest {
            version: 1,
            resources: vec![NflashBundleEntry {
                id: "r".into(),
                path: "../evil.bin".into(),
                media_type: "application/octet-stream".into(),
                offset,
                len,
                sha256: "0".repeat(64),
            }],
        })
        .unwrap();
        let mut raw = Vec::new();
        raw.extend_from_slice(b"NFLASHB1");
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&(skel_only.len() as u64).to_le_bytes());
        raw.extend_from_slice(&(bad_man.len() as u64).to_le_bytes());
        raw.extend_from_slice(&skel_only);
        raw.extend_from_slice(&bad_man);
        cases += 1;
        assert_no_panic(&format!("nflashb1_man_{offset}_{len}"), || {
            assert!(Project::from_nflash_bytes(&raw).is_err());
        });
    }

    // 1C. Deterministic PRNG mutations across JSON & NFLASHB1 + deeply nested JSON brackets
    let deep_json = format!("{}{}{}", "[".repeat(512), "0", "]".repeat(512));
    cases += 1;
    assert_no_panic("deep_json_512", || {
        let _ = Project::from_json(&deep_json);
        let _ = Project::from_nflash_bytes(deep_json.as_bytes());
    });

    for iter in 0..180 {
        let base = if iter % 2 == 0 {
            &valid_bundle
        } else {
            &valid_json
        };
        let mutated = rng.mutate(base);
        cases += 1;
        assert_no_panic(&format!("nflash_mut_{iter}"), || {
            if let Ok(p) = Project::from_nflash_bytes(&mutated) {
                p.validate().expect("validated");
            }
            let lossy = String::from_utf8_lossy(&mutated);
            if let Ok(p) = Project::from_json(&lossy) {
                p.validate().expect("validated");
            }
        });
    }

    assert!(
        cases >= 280,
        "expected >= 280 cases in family 1, got {cases}"
    );
}

// =========================================================================
// Family 2 & 3: Binary `.fla` (CFB + MFC + Contents + Shape + Text + Action)
//               and `OversizedFlaBundle` + `reconstruct_legacy_fla_bytes` (`NFD1`)
// =========================================================================

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.push(units.len() as u8);
    for ch in units {
        u16b(out, ch);
    }
}
fn push_mfc_new_class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xFFFF);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());
}

fn build_synthetic_binary_fla_streams() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    // 1. Contents stream (rev 0x3F with CDocumentPage + CMediaBits)
    let mut contents = Vec::new();
    contents.push(0x3F);
    u32b(&mut contents, 1);
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2B, 0x3B] {
        u32b(&mut contents, 0);
    }
    push_mfc_new_class(&mut contents, "CDocumentPage");
    contents.push(5);
    mfc_str(&mut contents, "Page 1");
    mfc_str(&mut contents, "Scene 1");
    u32b(&mut contents, 0);
    contents.push(2);
    u32b(&mut contents, 3);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u16b(&mut contents, 0); // end scenes
    u16b(&mut contents, 1);
    u16b(&mut contents, 1);
    u16b(&mut contents, 2);
    u16b(&mut contents, 0); // end library
    u16b(&mut contents, 100);

    push_mfc_new_class(&mut contents, "CMediaBits");
    contents.push(6);
    compact_utf16(&mut contents, "M 1");
    mfc_str(&mut contents, "fuzz.png");
    u16b(&mut contents, 1);
    mfc_str(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 3);
    for _ in 0..4 {
        u32b(&mut contents, 0);
    }
    contents.extend_from_slice(&[2, 1, 80, 1]);
    u16b(&mut contents, 0); // end media

    // Settings tail
    u16b(&mut contents, 2);
    u16b(&mut contents, 0);
    contents.push(0);
    for w in [0i32, 11000, 0, 8000] {
        i32b(&mut contents, w);
    }
    u32b(&mut contents, 20 * 65536);
    contents.extend_from_slice(&[0; 25]);
    i32b(&mut contents, 65536);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0xFFFF_FFFF);
    u32b(&mut contents, 0);
    u32b(&mut contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    i32b(&mut contents, 20);

    // 2. Page 1 stream (CPicPage -> CPicLayer -> CPicFrame with ActionScript + CPicShape)
    let mut page = vec![1u8];
    push_mfc_new_class(&mut page, "CPicPage"); // cid 1, oid 2
    page.extend_from_slice(&[1, 3]);
    push_mfc_new_class(&mut page, "CPicLayer"); // cid 3, oid 4
    page.extend_from_slice(&[1, 3]);
    push_mfc_new_class(&mut page, "CPicFrame"); // cid 5, oid 6
    page.extend_from_slice(&[1, 3]);
    push_mfc_new_class(&mut page, "CPicShape"); // cid 7, oid 8
    page.extend_from_slice(&[1, 3]);
    u16b(&mut page, 0);
    i32b(&mut page, i32::MIN);
    i32b(&mut page, i32::MIN);
    // Shape payload
    page.push(2);
    for w in [65536i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page, w);
    }
    page.push(5);
    page.extend_from_slice(&[0; 9]);
    u32b(&mut page, 0);
    // End frame children
    u16b(&mut page, 0);
    i32b(&mut page, i32::MIN);
    i32b(&mut page, i32::MIN);
    // Frame tail (rev 2 with action script)
    page.push(2);
    mfc_str(&mut page, "start");
    page.push(1); // duration = 1
    page.push(0); // tween = none
    page.push(0); // ease = 0
    u16b(&mut page, 0); // sound = null
    // ActionRecord rev 1: 1 action ("stop();")
    page.push(1);
    u16b(&mut page, 1);
    page.push(0);
    mfc_str(&mut page, "stop();");
    // End layer children
    u16b(&mut page, 0);
    i32b(&mut page, i32::MIN);
    i32b(&mut page, i32::MIN);
    page.push(2);
    mfc_str(&mut page, "Layer 1");
    page.extend_from_slice(&[0, 0]);
    u16b(&mut page, 0);
    page.push(2);
    // End page children
    u16b(&mut page, 0);
    i32b(&mut page, i32::MIN);
    i32b(&mut page, i32::MIN);
    page.push(1);
    u16b(&mut page, 1);

    // 3. M 1 bitmap stream (4x4 32-bit ARGB uncompressed)
    let mut media = Vec::new();
    media.extend_from_slice(&[3, 5]);
    u16b(&mut media, 16);
    u16b(&mut media, 4);
    u16b(&mut media, 4);
    i32b(&mut media, 0);
    i32b(&mut media, 80);
    i32b(&mut media, 0);
    i32b(&mut media, 80);
    media.extend_from_slice(&[1, 0]);
    media.extend(std::iter::repeat_n([255u8, 40, 120, 220], 16).flatten());

    (contents, page, media)
}

fn encode_cfb_bytes(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for &(name, data) in streams {
        cfb.create_stream(name).unwrap().write_all(data).unwrap();
    }
    cfb.into_inner().into_inner()
}

fn gzip_bytes(raw: &[u8]) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(raw).unwrap();
    gz.finish().unwrap()
}

#[test]
fn t472_fuzz_family_2_and_3_fla_cfb_streams_and_nfd1_oversized_bundle() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x02);
    let tmp = TempScratchDir::new("fla");
    let (contents, page, media) = build_synthetic_binary_fla_streams();
    let valid_fla =
        encode_cfb_bytes(&[("Contents", &contents), ("Page 1", &page), ("M 1", &media)]);
    let valid_path = tmp.write_file("seed.fla", &valid_fla);
    let baseline_proj = import_project(&valid_path).expect("seed binary FLA must import");
    baseline_proj.validate().expect("baseline FLA validates");
    let restored = reconstruct_legacy_fla_bytes(&baseline_proj)
        .expect("reconstruct ok")
        .expect("embedded provenance");
    assert_eq!(restored, valid_fla);

    let base_bundle =
        import_legacy_fla_oversized_bundle(&valid_path).expect("oversized bundle seed");
    let bundle_dir = tmp.0.join("valid-bundle");
    base_bundle.save_to_dir(&bundle_dir).expect("save bundle");
    let loaded_bundle = OversizedFlaBundle::load_from_dir(&bundle_dir).expect("load bundle");
    assert_eq!(loaded_bundle.reconstruct_fla_bytes().unwrap(), valid_fla);

    let mut cases = 0usize;

    // 2A. Fuzz `NFD1` delta stream opcodes, reference slices, lengths, and truncations
    let ref_blob = b"0123456789ABCDEF_REFERENCE_MEDIA_SLICE".to_vec();
    let mut valid_nfd1 = Vec::new();
    valid_nfd1.extend_from_slice(b"NFD1");
    valid_nfd1.extend_from_slice(&20u64.to_le_bytes()); // expected_len = 20
    valid_nfd1.extend_from_slice(&1u16.to_le_bytes()); // 1 ref
    valid_nfd1.push(0x00); // LITERAL 4 bytes
    valid_nfd1.extend_from_slice(&4u32.to_le_bytes());
    valid_nfd1.extend_from_slice(b"HEAD");
    valid_nfd1.push(0x01); // REF_SLICE ref=0, off=0, len=16
    valid_nfd1.extend_from_slice(&0u16.to_le_bytes());
    valid_nfd1.extend_from_slice(&0u64.to_le_bytes());
    valid_nfd1.extend_from_slice(&16u32.to_le_bytes());

    let mut adversarial_nfd1_streams: Vec<Vec<u8>> = Vec::new();
    for len in truncation_lengths(valid_nfd1.len(), 32) {
        adversarial_nfd1_streams.push(valid_nfd1[..len].to_vec());
    }
    // Bad opcode, u64::MAX expected_len, ref_idx OOB, offset+len u64::MAX wraparound, 512 MiB literal claim
    for bad in [
        {
            let mut b = b"NFD1".to_vec();
            b.extend_from_slice(&u64::MAX.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
            b
        },
        {
            let mut b = b"NFD1".to_vec();
            b.extend_from_slice(&16u64.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.push(0x01); // REF_SLICE with offset = u64::MAX, len = 16 -> overflow!
            b.extend_from_slice(&0u16.to_le_bytes());
            b.extend_from_slice(&u64::MAX.to_le_bytes());
            b.extend_from_slice(&16u32.to_le_bytes());
            b
        },
        {
            let mut b = b"NFD1".to_vec();
            b.extend_from_slice(&16u64.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.push(0x01); // REF_SLICE with ref_idx = u16::MAX
            b.extend_from_slice(&u16::MAX.to_le_bytes());
            b.extend_from_slice(&0u64.to_le_bytes());
            b.extend_from_slice(&4u32.to_le_bytes());
            b
        },
        {
            let mut b = b"NFD1".to_vec();
            b.extend_from_slice(&16u64.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
            b.push(0xFF); // unknown opcode
            b
        },
    ] {
        adversarial_nfd1_streams.push(bad);
    }
    for _ in 0..80 {
        adversarial_nfd1_streams.push(rng.mutate(&valid_nfd1));
    }

    let ref_sha = sha256_hex(&ref_blob);
    for (idx, raw_nfd1) in adversarial_nfd1_streams.iter().enumerate() {
        let gz = gzip_bytes(raw_nfd1);
        let gz_sha = sha256_hex(&gz);
        cases += 1;
        assert_no_panic(&format!("nfd1_fuzz_{idx}"), || {
            let mut b = base_bundle.clone();
            b.chunks = vec![
                OversizedMediaChunk {
                    sha256: ref_sha.clone(),
                    byte_len: ref_blob.len(),
                    media_type: "application/octet-stream".into(),
                    role: OversizedChunkRole::FullResolutionMedia,
                    data: ref_blob.clone(),
                },
                OversizedMediaChunk {
                    sha256: gz_sha.clone(),
                    byte_len: gz.len(),
                    media_type: "application/x-neoflash-nfd1+gzip".into(),
                    role: OversizedChunkRole::SourceDeltaGzip,
                    data: gz.clone(),
                },
            ];
            b.source_archive = OversizedSourceArchive::BundleDeltaGzip {
                source_bytes: 20,
                source_fnv1a64: "0000000000000000".into(),
                source_sha256: "a".repeat(64),
                delta_chunk_sha256s: vec![gz_sha],
                references: vec![OversizedDeltaReference::BundleChunk {
                    sha256: ref_sha.clone(),
                }],
            };
            let _ = b.reconstruct_fla_bytes();

            // Also exercise inline `reconstruct_legacy_fla_bytes`
            let mut p = baseline_proj.clone();
            p.resources.retain(|r| !r.id.starts_with("fla-"));
            p.resources.push(
                ResourceUnit::from_bytes("ref_0", "r.bin", "application/octet-stream", &ref_blob)
                    .unwrap(),
            );
            p.resources.push(
                ResourceUnit::from_bytes(
                    "fla-original",
                    "legacy/source.fla.delta.gz",
                    "application/octet-stream",
                    &gz,
                )
                .unwrap(),
            );
            let prov = serde_json::json!({
                "schema": 3,
                "source_bytes": 20,
                "source_fnv1a64": "0000000000000000",
                "source_sha256": "a".repeat(64),
                "source_archive": {
                    "encoding": "resource-delta-gzip",
                    "resources": ["fla-original"],
                    "reference_resources": ["ref_0"],
                    "source_byte_len": 20,
                    "source_sha256": "a".repeat(64),
                }
            });
            p.resources.push(
                ResourceUnit::from_bytes(
                    "fla-provenance",
                    "legacy/provenance.json",
                    "application/json",
                    serde_json::to_vec(&prov).unwrap().as_slice(),
                )
                .unwrap(),
            );
            let _ = reconstruct_legacy_fla_bytes(&p);
        });
    }

    // Corrupt bundle-manifest.json in OversizedFlaBundle directory
    let bad_bundle_dir = tmp.0.join("corrupt-bundle");
    base_bundle.save_to_dir(&bad_bundle_dir).unwrap();
    let manifest_path = bad_bundle_dir.join("bundle-manifest.json");
    let orig_manifest = std::fs::read(&manifest_path).unwrap();
    for iter in 0..30 {
        let mut_man = rng.mutate(&orig_manifest);
        std::fs::write(&manifest_path, &mut_man).unwrap();
        cases += 1;
        assert_no_panic(&format!("oversized_manifest_mut_{iter}"), || {
            let _ = OversizedFlaBundle::load_from_dir(&bad_bundle_dir);
        });
    }

    // 3A. Raw CFB container prefix truncations + bit-flips
    for len in truncation_lengths(valid_fla.len(), 32) {
        let p = tmp.write_file("trunc.fla", &valid_fla[..len]);
        cases += 1;
        assert_no_panic(&format!("fla_cfb_trunc_{len}"), || {
            let _ = inspect(&p);
            let _ = import_project(&p);
        });
    }
    for iter in 0..60 {
        let mutated_cfb = rng.mutate(&valid_fla);
        let p = tmp.write_file("mut_cfb.fla", &mutated_cfb);
        cases += 1;
        assert_no_panic(&format!("fla_cfb_mut_{iter}"), || {
            let _ = inspect(&p);
            let _ = import_project(&p);
        });
    }

    // 3B. Structure-preserving CFB inner stream fuzzing (Contents, Page 1 shape/text/action, M 1 bitmap)
    for len in truncation_lengths(contents.len(), 24) {
        let cfb_bytes = encode_cfb_bytes(&[("Contents", &contents[..len]), ("Page 1", &page)]);
        let p = tmp.write_file("trunc_contents.fla", &cfb_bytes);
        cases += 1;
        assert_no_panic(&format!("fla_contents_trunc_{len}"), || {
            let _ = inspect(&p);
            if let Ok(proj) = import_project(&p) {
                proj.validate().expect("imported FLA must validate");
            }
        });
    }
    for len in truncation_lengths(page.len(), 24) {
        let cfb_bytes = encode_cfb_bytes(&[("Contents", &contents), ("Page 1", &page[..len])]);
        let p = tmp.write_file("trunc_page.fla", &cfb_bytes);
        cases += 1;
        assert_no_panic(&format!("fla_page_trunc_{len}"), || {
            let _ = inspect(&p);
            if let Ok(proj) = import_project(&p) {
                proj.validate().expect("imported FLA must validate");
            }
        });
    }
    for iter in 0..120 {
        let mut_c = if iter % 3 == 0 {
            rng.mutate(&contents)
        } else {
            contents.clone()
        };
        let mut_p = if iter % 3 == 1 {
            rng.mutate(&page)
        } else {
            page.clone()
        };
        let mut_m = if iter % 3 == 2 {
            rng.mutate(&media)
        } else {
            media.clone()
        };
        let cfb_bytes =
            encode_cfb_bytes(&[("Contents", &mut_c), ("Page 1", &mut_p), ("M 1", &mut_m)]);
        let p = tmp.write_file("mut_streams.fla", &cfb_bytes);
        cases += 1;
        assert_no_panic(&format!("fla_inner_mut_{iter}"), || {
            let _ = inspect(&p);
            if let Ok(proj) = import_project(&p) {
                proj.validate().expect("imported FLA must validate");
            }
        });
    }

    assert!(
        cases >= 400,
        "expected >= 400 cases in family 2 & 3, got {cases}"
    );
}

// =========================================================================
// Family 4: `.swf` (`FWS` / `CWS` / `ZWS` + `DefineShape4`, `DefineMorphShape2`,
//           `DefineVideoStream`, `ScreenVideo`, `DoAction`, `DoABC2`)
// =========================================================================

fn push_swf_tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
    if payload.len() < 63 {
        out.extend_from_slice(&((code << 6) | (payload.len() as u16)).to_le_bytes());
    } else {
        out.extend_from_slice(&((code << 6) | 0x3F).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn wrap_fws(version: u8, tags: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    // RECT(0..4000, 0..3000) -> Nbits=14: 5 + 56 = 61 bits = 8 bytes
    body.extend_from_slice(&[0x70, 0x00, 0x0F, 0xA0, 0x00, 0x00, 0xBB, 0x80]);
    body.extend_from_slice(&(24u16 << 8).to_le_bytes()); // 24 fps
    body.extend_from_slice(&2u16.to_le_bytes()); // 2 frames
    body.extend_from_slice(tags);
    push_swf_tag(&mut body, 0, &[]); // End tag

    let total_len = (8 + body.len()) as u32;
    let mut fws = Vec::with_capacity(total_len as usize);
    fws.extend_from_slice(b"FWS");
    fws.push(version);
    fws.extend_from_slice(&total_len.to_le_bytes());
    fws.extend_from_slice(&body);
    fws
}

fn wrap_cws_from_fws(fws: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(&fws[8..]).unwrap();
    let z = enc.finish().unwrap();
    let mut cws = Vec::with_capacity(8 + z.len());
    cws.extend_from_slice(b"CWS");
    cws.push(fws[3]);
    cws.extend_from_slice(&fws[4..8]);
    cws.extend_from_slice(&z);
    cws
}

fn build_rich_swf_tags() -> Vec<(u16, Vec<u8>)> {
    // 1. DefineShape4 (tag 83): id=1, shape_bounds, edge_bounds, flags=0, 1 solid fill, 1 line style2, 1 straight edge
    let mut shape4 = Vec::new();
    shape4.extend_from_slice(&1u16.to_le_bytes());
    shape4.extend_from_slice(&[0x70, 0x00, 0x07, 0xD0, 0x00, 0x00, 0x07, 0xD0]); // bounds
    shape4.extend_from_slice(&[0x70, 0x00, 0x07, 0xD0, 0x00, 0x00, 0x07, 0xD0]); // edge bounds
    shape4.push(0x00); // flags
    shape4.push(1); // 1 fill style
    shape4.extend_from_slice(&[0x00, 255, 60, 80, 255]); // solid RGBA
    shape4.push(1); // 1 line style2
    shape4.extend_from_slice(&20u16.to_le_bytes()); // width
    shape4.extend_from_slice(&[0x00, 0x00]); // flags
    shape4.extend_from_slice(&[20, 30, 40, 255]); // RGBA
    shape4.push(0x11); // numFillBits=1, numLineBits=1
    shape4.extend_from_slice(&[0x25, 0x8F, 0x20, 0x00]); // StyleChange + StraightEdge + EndShape

    // 2. DefineMorphShape2 (tag 84): id=2
    let mut morph2 = Vec::new();
    morph2.extend_from_slice(&2u16.to_le_bytes());
    for _ in 0..4 {
        morph2.extend_from_slice(&[0x70, 0x00, 0x07, 0xD0, 0x00, 0x00, 0x07, 0xD0]);
    }
    morph2.push(0x00); // flags
    morph2.extend_from_slice(&14u32.to_le_bytes()); // offset to EndEdges
    morph2.push(1); // 1 morph fill style (solid)
    morph2.extend_from_slice(&[0x00, 255, 0, 0, 255, 0, 0, 255, 255]);
    morph2.push(0); // 0 morph line styles
    morph2.extend_from_slice(&[0x10, 0x00]); // start edges (empty)
    morph2.extend_from_slice(&[0x00, 0x00]); // end edges (empty)

    // 3. DefineVideoStream (tag 60) + VideoFrame (tag 61) ScreenVideo v1 (codec 3) & v2 (codec 6)
    let mut vid_def = Vec::new();
    vid_def.extend_from_slice(&3u16.to_le_bytes()); // cid = 3
    vid_def.extend_from_slice(&2u16.to_le_bytes()); // num_frames = 2
    vid_def.extend_from_slice(&16u16.to_le_bytes()); // width = 16
    vid_def.extend_from_slice(&16u16.to_le_bytes()); // height = 16
    vid_def.push(0); // flags
    vid_def.push(3); // codec_id = 3 (ScreenVideo v1)

    let mut z_enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    z_enc.write_all(&[120u8; 16 * 16 * 3]).unwrap();
    let z_block = z_enc.finish().unwrap();
    let mut vid_frame = Vec::new();
    vid_frame.extend_from_slice(&3u16.to_le_bytes()); // stream_id = 3
    vid_frame.extend_from_slice(&0u16.to_le_bytes()); // frame_num = 0
    vid_frame.extend_from_slice(&16u16.to_be_bytes()); // block_w=16, img_w=16
    vid_frame.extend_from_slice(&16u16.to_be_bytes()); // block_h=16, img_h=16
    vid_frame.extend_from_slice(&(z_block.len() as u16).to_be_bytes());
    vid_frame.extend_from_slice(&z_block);

    // 4. PlaceObject2 (tag 26)
    let mut place = Vec::new();
    place.push(0x06); // HasCharacter + HasMatrix
    place.extend_from_slice(&1u16.to_le_bytes()); // depth 1
    place.extend_from_slice(&3u16.to_le_bytes()); // cid 3
    place.push(0x00); // identity matrix

    // 5. DoAction (tag 12): ActionPush("ok"), ActionTrace, ActionStop, End
    let do_action = vec![0x96, 4, 0, 0, b'o', b'k', 0, 0x26, 0x07, 0x00];

    // 6. DoABC2 (tag 82): minimal valid ABC header + 1 method + 1 script + 1 body (`returnvoid`)
    let do_abc2 = vec![
        1, 0, 0, 0, // flags = 1 (lazy)
        b'f', b'z', 0, // name = "fz"
        16, 0, 46, 0, // minor=16, major=46
        0, 0, 0, 0, 0, 0, 0, // constant pool counts = 0
        1, // method_count = 1
        0, 0, 0, 0, // param_count=0, return_type=0, name=0, flags=0
        0, // metadata_count = 0
        0, // class_count = 0
        1, // script_count = 1
        0, 0, // init=0, trait_count=0
        1, // method_body_count = 1
        0, 1, 1, 0, 1, // method=0, max_stack=1, local_count=1, init_scope=0, max_scope=1
        1, 0x47, // code_len=1, returnvoid (0x47)
        0, 0, // exception_count=0, trait_count=0
    ];

    vec![
        (69, vec![0x08, 0, 0, 0]), // FileAttributes (AS3 bit)
        (83, shape4),
        (84, morph2),
        (60, vid_def),
        (61, vid_frame),
        (26, place),
        (12, do_action),
        (82, do_abc2),
        (1, Vec::new()), // ShowFrame
    ]
}

#[test]
fn t472_fuzz_family_4_swf_fws_cws_zws_tags_and_bytecode_player() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x04);
    let tmp = TempScratchDir::new("swf");
    let base_tags = build_rich_swf_tags();

    let mut serialized_tags = Vec::new();
    for (code, payload) in &base_tags {
        push_swf_tag(&mut serialized_tags, *code, payload);
    }
    let seed_fws = wrap_fws(10, &serialized_tags);
    let seed_cws = wrap_cws_from_fws(&seed_fws);

    // Verify seed works cleanly
    {
        let p = tmp.write_file("seed.swf", &seed_fws);
        let _ = inspect(&p).expect("inspect seed SWF");
        let proj = import_project(&p).expect("import seed SWF");
        proj.validate().expect("seed SWF validates");
        let mut player = SwfBytecodePlayer::from_bytes(&seed_fws).expect("player seed SWF");
        let _ = player.step_frame();
    }

    let mut cases = 0usize;

    let exercise_swf_bytes = |label: &str, bytes: &[u8], tmp_dir: &TempScratchDir| {
        assert_no_panic(label, || {
            let p = tmp_dir.write_file("case.swf", bytes);
            let _ = inspect(&p);
            if let Ok(proj) = import_project(&p) {
                proj.validate().expect("imported SWF project must validate");
            }
            if let Ok(mut player) = SwfBytecodePlayer::from_bytes(bytes) {
                let _ = player.step_frame();
                let _ = player.step_frame();
            }
        });
    };

    // 4A. Prefix truncations across FWS & CWS
    for (kind, seed) in [("fws", &seed_fws), ("cws", &seed_cws)] {
        for len in truncation_lengths(seed.len(), 36) {
            cases += 1;
            exercise_swf_bytes(&format!("swf_{kind}_trunc_{len}"), &seed[..len], &tmp);
        }
    }

    // 4B. Adversarial ZWS (LZMA SWF 13+) frames hitting `decompress_zws` directly!
    // Header: b"ZWS" + version(1) + declared_uncompressed_len(4 LE) + compressed_len(4 LE) + props(5) + lzma_stream
    for iter in 0..90 {
        let mut lzma_body = vec![0x00u8]; // valid initial range-coder byte = 0x00!
        let stream_len = 4 + rng.next_usize(48);
        for _ in 0..stream_len {
            lzma_body.push(rng.next_u8());
        }
        if iter == 0 {
            // Adversarial initial code = 0xFFFF_FFFF, props[0] = 44 (lc=8, lp=4)
            lzma_body[1..5].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        }
        let props: [u8; 5] = match iter % 6 {
            0 => [0x5D, 0x00, 0x10, 0x00, 0x00], // standard lc=3, lp=0, pb=2
            1 => [0x00, 0x00, 0x00, 0x00, 0x00], // lc=0, lp=0, pb=0, dict=0
            2 => [224, 0xFF, 0xFF, 0xFF, 0xFF], // max valid props[0]=224 (lc=8, lp=4, pb=4), dict=u32::MAX
            3 => [225, 0, 0, 0, 0],             // invalid props[0]=225
            4 => [8, 1, 0, 0, 0],               // lc=8, lp=0, pb=0
            _ => [rng.next_u8(), rng.next_u8(), rng.next_u8(), 0, 0],
        };
        let declared_uncompressed: u32 = match iter % 5 {
            0 => 64,
            1 => 8,
            2 => 7, // < 8
            3 => u32::MAX,
            _ => 16 + (rng.next_usize(512) as u32),
        };
        let compressed_len = lzma_body.len() as u32;
        let mut zws = Vec::with_capacity(17 + lzma_body.len());
        zws.extend_from_slice(b"ZWS");
        zws.push(15); // SWF 15
        zws.extend_from_slice(&declared_uncompressed.to_le_bytes());
        zws.extend_from_slice(&compressed_len.to_le_bytes());
        zws.extend_from_slice(&props);
        zws.extend_from_slice(&lzma_body);
        cases += 1;
        exercise_swf_bytes(&format!("swf_zws_fuzz_{iter}"), &zws, &tmp);
    }

    // 4C. Structure-aware SWF tag payload mutations (`DefineShape4`, `DefineMorphShape2`,
    //     `DefineVideoStream` + `ScreenVideo` v1/v2, `DoAction` infinite backward jump, `DoABC2` corrupt u30/code)
    // Explicit infinite backward jump in DoAction (`0x99 0x02 0x00 0xFB 0xFF` -> jump -5 to self!)
    {
        let mut inf_tags = Vec::new();
        push_swf_tag(&mut inf_tags, 12, &[0x99, 0x02, 0x00, 0xFB, 0xFF, 0x00]);
        push_swf_tag(&mut inf_tags, 1, &[]);
        let inf_swf = wrap_fws(8, &inf_tags);
        cases += 1;
        exercise_swf_bytes("swf_doaction_self_jump", &inf_swf, &tmp);
    }
    // Explicit infinite backward jump in DoABC2 (`0x10 0xFC 0xFF 0xFF` -> jump -4 to self!)
    {
        let mut abc_inf = base_tags[7].1.clone();
        // Replace `code_len=1, 0x47` with `code_len=5, label(0x09), jump -5 (0x10, 0xFB, 0xFF, 0xFF)`
        let len_pos = abc_inf.len() - 4;
        abc_inf.splice(len_pos..len_pos + 2, [5u8, 0x09, 0x10, 0xFB, 0xFF, 0xFF]);
        let mut inf_tags = Vec::new();
        push_swf_tag(&mut inf_tags, 69, &[0x08, 0, 0, 0]);
        push_swf_tag(&mut inf_tags, 82, &abc_inf);
        push_swf_tag(&mut inf_tags, 1, &[]);
        let inf_swf = wrap_fws(10, &inf_tags);
        cases += 1;
        exercise_swf_bytes("swf_doabc2_self_jump", &inf_swf, &tmp);
    }

    for iter in 0..160 {
        let mut tags_clone = base_tags.clone();
        let target_idx = iter % tags_clone.len();
        tags_clone[target_idx].1 = rng.mutate(&tags_clone[target_idx].1);
        // Also test ScreenVideo v2 (`codec_id = 6`) on odd video mutations
        if iter % 9 == 0 {
            tags_clone[3].1 = vec![3, 0, 2, 0, 16, 0, 16, 0, 0, 6]; // codec_id = 6 (ScreenVideo v2)
        }
        let mut tag_buf = Vec::new();
        for (c, p) in &tags_clone {
            push_swf_tag(&mut tag_buf, *c, p);
        }
        let fws = wrap_fws(10, &tag_buf);
        cases += 1;
        if iter % 4 == 0 {
            let cws = wrap_cws_from_fws(&fws);
            exercise_swf_bytes(&format!("swf_tag_cws_mut_{iter}"), &cws, &tmp);
        } else {
            exercise_swf_bytes(&format!("swf_tag_fws_mut_{iter}"), &fws, &tmp);
        }
    }

    // Also raw container header/tag-length bit-flips with fixed `declared_file_length`
    for iter in 0..50 {
        let mut raw = rng.mutate(&seed_fws);
        if raw.len() >= 8 {
            let actual_len = raw.len() as u32;
            raw[0..3].copy_from_slice(b"FWS");
            raw[4..8].copy_from_slice(&actual_len.to_le_bytes());
        }
        cases += 1;
        exercise_swf_bytes(&format!("swf_raw_len_synced_{iter}"), &raw, &tmp);
    }

    // [A-05] 28-step exponential `a = a.concat(a)` loop in AVM2 `DoABC2` must cap at 65,536 without OOM!
    {
        // Script: `var a = [1]; for (var i = 0; i < 28; i = i + 1) { a = a.concat(a); } trace(a.length);`
        let code: Vec<u8> = vec![
            0xD0, 0x30, // getlocal_0, pushscope
            0x24, 1, 0x56, 1, 0xD5, // pushbyte 1, newarray 1, setlocal_1 (`a = [1]`)
            0x24, 0, 0xD6, // pushbyte 0, setlocal_2 (`i = 0`)
            0x09, // label (pc = 10)
            0xD2, 0x24, 28, 0x18, 15, 0,
            0, // getlocal_2, pushbyte 28, ifge (0x18) +15 (to pc = 33)
            0xD1, 0xD1, 0x46, 1, 1, 0xD5, // a = a.concat(a) (6 bytes)
            0xD2, 0x24, 1, 0xA0, 0xD6, // i = i + 1 (5 bytes)
            0x10, 0xE9, 0xFF, 0xFF, // jump -23 (from pc = 33 back to pc = 10)
            0x5E, 3, 0xD1, 0x66, 2, 0x4F, 3,
            1,    // findpropstrict("trace"), a.length, callpropvoid("trace", 1)
            0x47, // returnvoid
        ];
        let mut abc_concat = vec![
            0,
            0,
            0,
            0, // flags = 0 (eagerly execute script 0!)
            b'c',
            b'c',
            0, // name = "cc"
            16,
            0,
            46,
            0, // minor=16, major=46
            0,
            0,
            0, // int=0, uint=0, double=0
            5, // string_count = 5 (1..=4)
            0, // [1] ""
            6,
            b'c',
            b'o',
            b'n',
            b'c',
            b'a',
            b't', // [2] "concat"
            6,
            b'l',
            b'e',
            b'n',
            b'g',
            b't',
            b'h', // [3] "length"
            5,
            b't',
            b'r',
            b'a',
            b'c',
            b'e', // [4] "trace"
            2,    // namespace_count = 2 (1..=1)
            0x16,
            1, // [1] PackageNamespace("")
            0, // ns_set_count = 0
            4, // multiname_count = 4 (1..=3)
            0x07,
            1,
            2, // [1] QName("", "concat")
            0x07,
            1,
            3, // [2] QName("", "length")
            0x07,
            1,
            4, // [3] QName("", "trace")
            1, // method_count = 1
            0,
            0,
            0,
            0, // method 0
            0, // metadata_count = 0
            0, // class_count = 0
            1, // script_count = 1
            0,
            0, // init=0, traits=0
            1, // method_body_count = 1
            0,
            4,
            3,
            0,
            2, // method=0, max_stack=4, local_count=3, init_scope=0, max_scope=2
            code.len() as u8,
        ];
        abc_concat.extend_from_slice(&code);
        abc_concat.extend_from_slice(&[0, 0]); // exceptions=0, traits=0

        let mut concat_tags = Vec::new();
        push_swf_tag(&mut concat_tags, 69, &[0x08, 0, 0, 0]);
        push_swf_tag(&mut concat_tags, 82, &abc_concat);
        push_swf_tag(&mut concat_tags, 1, &[]);
        let concat_swf = wrap_fws(10, &concat_tags);
        cases += 1;
        let mut concat_player =
            SwfBytecodePlayer::from_bytes(&concat_swf).expect("parse AVM2 concat SWF");
        let _ = concat_player.step_frame();
        assert_eq!(
            concat_player.trace_output(),
            &["65536".to_owned()],
            "28-step a = a.concat(a) must cap cleanly at 65,536 elements without OOM"
        );
    }

    assert!(
        cases >= 350,
        "expected >= 350 cases in family 4, got {cases}"
    );
}

// =========================================================================
// Family 5: `.fig` (`inspect_figma_bytes` / `import_figma_bytes` Kiwi + Zstd + ZIP + JSON)
// =========================================================================

fn kiwi_var_uint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        } else {
            out.push(byte | 0x80);
        }
    }
}

fn kiwi_var_int(out: &mut Vec<u8>, v: i64) {
    let zigzag = ((v << 1) ^ (v >> 63)) as u64;
    kiwi_var_uint(out, zigzag);
}

fn kiwi_var_float(out: &mut Vec<u8>, f: f32) {
    if f == 0.0 {
        out.push(0);
    } else {
        out.extend_from_slice(&f.to_bits().rotate_left(9).to_le_bytes());
    }
}

fn kiwi_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

fn build_zstd_raw_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(16 + payload.len());
    frame.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]); // Zstandard magic
    frame.push(0xA0); // FCS_Field_Size_Flag=2 (4 bytes), Single_Segment=1, no checksum
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    let hdr = ((payload.len() as u32) << 3) | 1; // Raw_Block (type=0), Last_Block=1
    let hb = hdr.to_le_bytes();
    frame.extend_from_slice(&[hb[0], hb[1], hb[2]]);
    frame.extend_from_slice(payload);
    frame
}

fn build_seed_fig_kiwi_and_zip() -> [Vec<u8>; 5] {
    let mut schema = Vec::new();
    kiwi_var_uint(&mut schema, 5);
    // 0: GUID
    kiwi_cstr(&mut schema, "GUID");
    schema.push(1);
    kiwi_var_uint(&mut schema, 2);
    kiwi_cstr(&mut schema, "sessionID");
    kiwi_var_int(&mut schema, -4);
    schema.push(0);
    kiwi_var_uint(&mut schema, 1);
    kiwi_cstr(&mut schema, "localID");
    kiwi_var_int(&mut schema, -4);
    schema.push(0);
    kiwi_var_uint(&mut schema, 2);
    // 1: ParentIndex
    kiwi_cstr(&mut schema, "ParentIndex");
    schema.push(1);
    kiwi_var_uint(&mut schema, 2);
    kiwi_cstr(&mut schema, "guid");
    kiwi_var_int(&mut schema, 0);
    schema.push(0);
    kiwi_var_uint(&mut schema, 1);
    kiwi_cstr(&mut schema, "position");
    kiwi_var_int(&mut schema, -6);
    schema.push(0);
    kiwi_var_uint(&mut schema, 2);
    // 2: Vector
    kiwi_cstr(&mut schema, "Vector");
    schema.push(1);
    kiwi_var_uint(&mut schema, 2);
    kiwi_cstr(&mut schema, "x");
    kiwi_var_int(&mut schema, -5);
    schema.push(0);
    kiwi_var_uint(&mut schema, 1);
    kiwi_cstr(&mut schema, "y");
    kiwi_var_int(&mut schema, -5);
    schema.push(0);
    kiwi_var_uint(&mut schema, 2);
    // 3: NodeChange
    kiwi_cstr(&mut schema, "NodeChange");
    schema.push(2);
    kiwi_var_uint(&mut schema, 5);
    for (tag, fname, tid) in [
        (1u64, "guid", 0i64),
        (2, "parentIndex", 1),
        (3, "type", -6),
        (4, "name", -6),
        (5, "size", 2),
    ] {
        kiwi_cstr(&mut schema, fname);
        kiwi_var_int(&mut schema, tid);
        schema.push(0);
        kiwi_var_uint(&mut schema, tag);
    }
    // 4: Message
    kiwi_cstr(&mut schema, "Message");
    schema.push(2);
    kiwi_var_uint(&mut schema, 1);
    kiwi_cstr(&mut schema, "nodeChanges");
    kiwi_var_int(&mut schema, 3);
    schema.push(1);
    kiwi_var_uint(&mut schema, 1);

    let mut msg = Vec::new();
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 3);
    // DOCUMENT 0:0
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 3);
    kiwi_cstr(&mut msg, "DOCUMENT");
    kiwi_var_uint(&mut msg, 4);
    kiwi_cstr(&mut msg, "FuzzDoc");
    kiwi_var_uint(&mut msg, 0);
    // CANVAS 0:1
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 2);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 0);
    kiwi_cstr(&mut msg, "!");
    kiwi_var_uint(&mut msg, 3);
    kiwi_cstr(&mut msg, "CANVAS");
    kiwi_var_uint(&mut msg, 4);
    kiwi_cstr(&mut msg, "Page 1");
    kiwi_var_uint(&mut msg, 0);
    // FRAME 1:1
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 1);
    kiwi_var_uint(&mut msg, 2);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 1);
    kiwi_cstr(&mut msg, "!");
    kiwi_var_uint(&mut msg, 3);
    kiwi_cstr(&mut msg, "FRAME");
    kiwi_var_uint(&mut msg, 4);
    kiwi_cstr(&mut msg, "Hero");
    kiwi_var_uint(&mut msg, 5);
    kiwi_var_float(&mut msg, 640.0);
    kiwi_var_float(&mut msg, 480.0);
    kiwi_var_uint(&mut msg, 0);
    kiwi_var_uint(&mut msg, 0); // end Message

    let deflate = |raw: &[u8]| -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    };
    let c0 = deflate(&schema);
    let c1_zstd = build_zstd_raw_frame(&msg);

    let mut canvas_zstd = Vec::new();
    canvas_zstd.extend_from_slice(b"fig-kiwi");
    canvas_zstd.extend_from_slice(&106u32.to_le_bytes());
    canvas_zstd.extend_from_slice(&(c0.len() as u32).to_le_bytes());
    canvas_zstd.extend_from_slice(&c0);
    canvas_zstd.extend_from_slice(&(c1_zstd.len() as u32).to_le_bytes());
    canvas_zstd.extend_from_slice(&c1_zstd);

    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zip_buf);
        let zopts = zip::write::SimpleFileOptions::default();
        zw.start_file("meta.json", zopts).unwrap();
        zw.write_all(br#"{"file_name":"Fuzz v106"}"#).unwrap();
        zw.start_file("canvas.fig", zopts).unwrap();
        zw.write_all(&canvas_zstd).unwrap();
        zw.finish().unwrap();
    }
    let fig_zip = zip_buf.into_inner();
    let fig_json = br#"{"name":"JSON Fig","document":{"id":"0:0","type":"DOCUMENT","children":[{"id":"0:1","name":"Page 1","type":"CANVAS","children":[{"id":"1:1","name":"Card","type":"RECTANGLE","absoluteBoundingBox":{"x":0,"y":0,"width":200,"height":100},"fills":[{"type":"SOLID","color":{"r":0.2,"g":0.5,"b":0.8,"a":1.0}}]}]}]}}"#.to_vec();

    [schema, msg, canvas_zstd, fig_zip, fig_json]
}

#[test]
fn t472_fuzz_family_5_figma_kiwi_zstd_zip_and_json() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x05);
    let [schema, msg, canvas_zstd, fig_zip, fig_json] = build_seed_fig_kiwi_and_zip();
    let opts = FigmaImportOptions::default();

    // Verify seeds succeed
    let _ = inspect_figma_bytes(&canvas_zstd, "seed").expect("inspect zstd fig-kiwi");
    let (p1, _) = import_figma_bytes(&canvas_zstd, "seed", &opts).expect("import zstd fig-kiwi");
    p1.validate().unwrap();
    let (p2, _) = import_figma_bytes(&fig_zip, "seed", &opts).expect("import zip fig");
    p2.validate().unwrap();

    let mut cases = 0usize;
    let exercise_fig = |label: &str, bytes: &[u8]| {
        assert_no_panic(label, || {
            let _ = inspect_figma_bytes(bytes, "fuzz");
            if let Ok((proj, _)) = import_figma_bytes(bytes, "fuzz", &opts) {
                proj.validate()
                    .expect("imported Figma project must validate");
            }
            if let Ok(out) = import_figma_clipboard_bytes(bytes, None) {
                out.project
                    .validate()
                    .expect("imported Figma clipboard project must validate");
            }
        });
    };

    // 5A. Prefix truncations across `fig-kiwi` (Zstd), `.fig` ZIP, and `.figma.json`
    for (kind, seed) in [
        ("kiwi_zstd", &canvas_zstd),
        ("fig_zip", &fig_zip),
        ("fig_json", &fig_json),
    ] {
        for len in truncation_lengths(seed.len(), 28) {
            cases += 1;
            exercise_fig(&format!("fig_{kind}_trunc_{len}"), &seed[..len]);
        }
    }

    // 5B. Mutate inner Kiwi schema + message + raw Zstd compressed blocks (including RLE / Compressed_Block type=1,2,3)
    let deflate = |raw: &[u8]| -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    };
    let c0_valid = deflate(&schema);

    for iter in 0..140 {
        let payload = match iter % 4 {
            0 => {
                // Mutate Kiwi schema inside valid Deflate chunk 0
                let ms = rng.mutate(&schema);
                let mc0 = deflate(&ms);
                let mc1 = build_zstd_raw_frame(&msg);
                let mut b = b"fig-kiwi".to_vec();
                b.extend_from_slice(&106u32.to_le_bytes());
                b.extend_from_slice(&(mc0.len() as u32).to_le_bytes());
                b.extend_from_slice(&mc0);
                b.extend_from_slice(&(mc1.len() as u32).to_le_bytes());
                b.extend_from_slice(&mc1);
                b
            }
            1 => {
                // Mutate Kiwi message inside valid Zstd frame
                let mm = rng.mutate(&msg);
                let mc1 = build_zstd_raw_frame(&mm);
                let mut b = b"fig-kiwi".to_vec();
                b.extend_from_slice(&106u32.to_le_bytes());
                b.extend_from_slice(&(c0_valid.len() as u32).to_le_bytes());
                b.extend_from_slice(&c0_valid);
                b.extend_from_slice(&(mc1.len() as u32).to_le_bytes());
                b.extend_from_slice(&mc1);
                b
            }
            2 => {
                // Mutate raw Zstd frame bytes directly (fuzzing RFC 8878 Zstd FSE/Huffman/LZ77 decoder!)
                let mut z = build_zstd_raw_frame(&msg);
                z = rng.mutate(&z);
                if z.len() >= 4 {
                    z[0..4].copy_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]);
                }
                let mut b = b"fig-kiwi".to_vec();
                b.extend_from_slice(&106u32.to_le_bytes());
                b.extend_from_slice(&(c0_valid.len() as u32).to_le_bytes());
                b.extend_from_slice(&c0_valid);
                b.extend_from_slice(&(z.len() as u32).to_le_bytes());
                b.extend_from_slice(&z);
                b
            }
            _ => rng.mutate(&canvas_zstd),
        };
        cases += 1;
        exercise_fig(&format!("fig_inner_mut_{iter}"), &payload);
    }

    assert!(
        cases >= 280,
        "expected >= 280 cases in family 5, got {cases}"
    );
}

// =========================================================================
// Family 6: SVG, FXG, PDF/AI/EPS, PSD, SWC & Palette Importers
// =========================================================================

fn build_seed_psd() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&4u16.to_be_bytes()); // 4 channels
    out.extend_from_slice(&8u32.to_be_bytes()); // h = 8
    out.extend_from_slice(&8u32.to_be_bytes()); // w = 8
    out.extend_from_slice(&8u16.to_be_bytes()); // depth = 8
    out.extend_from_slice(&3u16.to_be_bytes()); // RGB = 3
    out.extend_from_slice(&0u32.to_be_bytes()); // color mode data
    out.extend_from_slice(&0u32.to_be_bytes()); // image resources

    let rle = encode_packbits_channel(&[180u8; 16], 4, 4);
    let mut layer_info = Vec::new();
    layer_info.extend_from_slice(&1i16.to_be_bytes()); // 1 layer
    for v in [0i32, 0, 4, 4] {
        layer_info.extend_from_slice(&v.to_be_bytes());
    }
    layer_info.extend_from_slice(&4u16.to_be_bytes());
    for cid in [0i16, 1, 2, -1] {
        layer_info.extend_from_slice(&cid.to_be_bytes());
        layer_info.extend_from_slice(&(rle.len() as u32).to_be_bytes());
    }
    layer_info.extend_from_slice(b"8BIMnorm");
    layer_info.extend_from_slice(&[255, 0, 0, 0]);
    let extra = [0u8, 0, 0, 0, 0, 0, 0, 0, 4, b'B', b'a', b's', b'e', 0, 0, 0];
    layer_info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
    layer_info.extend_from_slice(&extra);
    for _ in 0..4 {
        layer_info.extend_from_slice(&rle);
    }
    if layer_info.len() % 2 != 0 {
        layer_info.push(0);
    }
    let lm_len = (4 + layer_info.len()) as u32;
    out.extend_from_slice(&lm_len.to_be_bytes());
    out.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
    out.extend_from_slice(&layer_info);
    out.extend_from_slice(&0u16.to_be_bytes()); // merged raw
    out.extend(std::iter::repeat_n(200u8, 8 * 8 * 4));
    out
}

fn build_seed_pdf() -> Vec<u8> {
    let stream = b"q 0.2 0.4 0.8 rg 10 20 120 80 re f BT /F1 16 Tf 20 40 Td (Hello PDF) Tj ET Q";
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let o1 = pdf.len();
    pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    let o2 = pdf.len();
    pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    let o3 = pdf.len();
    pdf.extend_from_slice(
        b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Contents 4 0 R >> endobj\n",
    );
    let o4 = pdf.len();
    pdf.extend_from_slice(format!("4 0 obj << /Length {} >>\nstream\n", stream.len()).as_bytes());
    pdf.extend_from_slice(stream);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in [o1, o2, o3, o4] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer << /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

#[test]
fn t472_fuzz_family_6_svg_fxg_pdf_ai_eps_psd_and_swc() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x06);
    let vec_opts = VectorImportOptions::default();
    let psd_opts = PsdImportOptions::default();

    let seed_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 240"><defs><linearGradient id="g1"><stop offset="0%" stop-color="#ff3366"/><stop offset="100%" stop-color="#3366ff"/></linearGradient></defs><g id="layer1" transform="translate(10,15) rotate(12)"><path d="M10 10 C 40 20, 65 20, 95 10 S 150 0, 180 40 A 25 25 0 1 1 120 90 Z" fill="url(#g1)" stroke="#112233" stroke-width="2"/><text x="20" y="60" font-size="18">SVG Fuzz</text></g></svg>"##.to_vec();
    let seed_fxg = br##"<?xml version="1.0" encoding="utf-8"?><Graphic version="2.0" viewHeight="240" viewWidth="320" xmlns="http://ns.adobe.com/fxg/2008"><Library><Definition name="Badge"><Group><Rect width="40" height="40"><fill><SolidColor color="#FF4422"/></fill></Rect></Group></Definition></Library><Group d:type="layer" xmlns:d="http://ns.adobe.com/fxg/2008/dt"><Path data="M 0 0 L 100 20 Q 120 80 60 90 Z"><fill><SolidColor color="#2288EE" alpha="0.9"/></fill></Path><RichText x="12" y="24"><content><p>FXG Text</p></content></RichText></Group></Graphic>"##.to_vec();
    let seed_pdf = build_seed_pdf();
    let seed_eps = br#"%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: 0 0 300 200
gsave
0.1 0.6 0.9 setrgbcolor
10 20 moveto 140 20 lineto 140 110 lineto closepath fill
grestore
showpage
%%EOF
"#
    .to_vec();
    let seed_psd = build_seed_psd();

    // Verify all 5 baseline seeds import cleanly and validate!
    let (p_svg, _) = import_vector_bytes_with_options(&seed_svg, "seed.svg", &vec_opts).unwrap();
    p_svg.validate().unwrap();
    let (p_fxg, _) = import_fxg_str(std::str::from_utf8(&seed_fxg).unwrap(), "seed.fxg").unwrap();
    p_fxg.validate().unwrap();
    let (p_pdf, _) = import_pdf_ai_with_options(&seed_pdf, "seed.pdf", &vec_opts).unwrap();
    p_pdf.validate().unwrap();
    let (p_eps, _) = import_vector_bytes_with_options(&seed_eps, "seed.eps", &vec_opts).unwrap();
    p_eps.validate().unwrap();
    let (p_psd, _) = import_psd_bytes(&seed_psd, "seed.psd", &psd_opts).unwrap();
    p_psd.validate().unwrap();

    let mut cases = 0usize;

    // 6A. Prefix truncations across SVG, FXG, PDF/AI, EPS, and PSD
    for (kind, seed) in [
        ("svg", &seed_svg),
        ("fxg", &seed_fxg),
        ("pdf", &seed_pdf),
        ("eps", &seed_eps),
        ("psd", &seed_psd),
    ] {
        for len in truncation_lengths(seed.len(), 22) {
            let slice = &seed[..len];
            cases += 1;
            assert_no_panic(&format!("{kind}_trunc_{len}"), || match kind {
                "svg" => {
                    let _ = import_vector_bytes_with_options(slice, "t.svg", &vec_opts);
                    if let Ok(s) = std::str::from_utf8(slice)
                        && let Ok((p, _)) = import_svg_with_options(s, "t.svg", &vec_opts)
                    {
                        p.validate().unwrap();
                    }
                }
                "fxg" => {
                    if let Ok(s) = std::str::from_utf8(slice)
                        && let Ok((p, _)) = import_fxg_str(s, "t.fxg")
                    {
                        p.validate().unwrap();
                    }
                }
                "pdf" => {
                    if let Ok((p, _)) = import_pdf_ai_with_options(slice, "t.pdf", &vec_opts) {
                        p.validate().unwrap();
                    }
                    let _ = import_pdf_clipboard(slice, "t.pdf");
                }
                "eps" => {
                    if let Ok((p, _)) = import_vector_bytes_with_options(slice, "t.eps", &vec_opts)
                    {
                        p.validate().unwrap();
                    }
                    let _ = import_eps_clipboard(slice, "t.eps");
                }
                "psd" => {
                    let _ = parse_psd_document(slice);
                    if let Ok((p, _)) = import_psd_bytes(slice, "t.psd", &psd_opts) {
                        p.validate().unwrap();
                    }
                }
                _ => unreachable!(),
            });
        }
    }

    // 6B. Adversarial PSD headers (u32::MAX dimensions, negative layer counts, corrupt TySh/lfx2/PackBits)
    for (w, h, channels, depth, layer_count) in [
        (u32::MAX, 16u32, 4u16, 8u16, 1i16),
        (16, u32::MAX, 4, 8, 1),
        (30_000, 30_000, 4, 8, 1),
        (0, 0, 0, 0, -32768),
        (16, 16, u16::MAX, 16, i16::MIN),
    ] {
        let mut adv_psd = seed_psd.clone();
        adv_psd[12..14].copy_from_slice(&channels.to_be_bytes());
        adv_psd[14..18].copy_from_slice(&h.to_be_bytes());
        adv_psd[18..22].copy_from_slice(&w.to_be_bytes());
        adv_psd[22..24].copy_from_slice(&depth.to_be_bytes());
        if adv_psd.len() >= 44 {
            adv_psd[42..44].copy_from_slice(&layer_count.to_be_bytes());
        }
        cases += 1;
        assert_no_panic(&format!("psd_adv_{w}_{h}_{channels}"), || {
            let _ = parse_psd_document(&adv_psd);
            let _ = import_psd_bytes(&adv_psd, "adv.psd", &psd_opts);
        });
    }

    // 6C. Deeply nested SVG `<g>` & FXG `<Group>` & cyclic `<use xlink:href="#a"/>`
    {
        let deep_svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><g id=\"c\"><use xlink:href=\"#c\"/></g>{}<rect width=\"10\" height=\"10\"/>{}</svg>",
            "<g>".repeat(300),
            "</g>".repeat(300)
        );
        let deep_fxg = format!(
            "<Graphic version=\"2.0\" xmlns=\"http://ns.adobe.com/fxg/2008\">{}<Rect width=\"10\" height=\"10\"/>{}</Graphic>",
            "<Group>".repeat(300),
            "</Group>".repeat(300)
        );
        cases += 2;
        assert_no_panic("svg_deep_and_cyclic_use", || {
            let _ = import_svg_with_options(&deep_svg, "deep.svg", &vec_opts);
        });
        assert_no_panic("fxg_deep_300", || {
            let _ = import_fxg_str(&deep_fxg, "deep.fxg");
        });
    }

    // 6D. Deterministic PRNG mutations across SVG, FXG, PDF/AI, EPS, PSD, SWC, and ACT/CLR/ASE palettes
    for iter in 0..220 {
        cases += 1;
        match iter % 6 {
            0 => {
                let m = rng.mutate(&seed_svg);
                assert_no_panic(&format!("svg_mut_{iter}"), || {
                    let _ = import_vector_bytes_with_options(&m, "m.svg", &vec_opts);
                    let lossy = String::from_utf8_lossy(&m);
                    if let Ok((p, _)) = import_svg_with_options(&lossy, "m.svg", &vec_opts) {
                        p.validate().unwrap();
                    }
                });
            }
            1 => {
                let m = rng.mutate(&seed_fxg);
                assert_no_panic(&format!("fxg_mut_{iter}"), || {
                    let lossy = String::from_utf8_lossy(&m);
                    if let Ok((p, _)) = import_fxg_str(&lossy, "m.fxg") {
                        p.validate().unwrap();
                    }
                });
            }
            2 => {
                let m = rng.mutate(&seed_pdf);
                assert_no_panic(&format!("pdf_mut_{iter}"), || {
                    if let Ok((p, _)) = import_pdf_ai_with_options(&m, "m.pdf", &vec_opts) {
                        p.validate().unwrap();
                    }
                    let _ = import_pdf_clipboard(&m, "m.pdf");
                });
            }
            3 => {
                let m = rng.mutate(&seed_eps);
                assert_no_panic(&format!("eps_mut_{iter}"), || {
                    if let Ok((p, _)) = import_vector_bytes_with_options(&m, "m.eps", &vec_opts) {
                        p.validate().unwrap();
                    }
                    let _ = import_eps_clipboard(&m, "m.eps");
                });
            }
            4 => {
                let m = rng.mutate(&seed_psd);
                assert_no_panic(&format!("psd_mut_{iter}"), || {
                    let _ = parse_psd_document(&m);
                    if let Ok((p, _)) = import_psd_bytes(&m, "m.psd", &psd_opts) {
                        p.validate().unwrap();
                    }
                });
            }
            _ => {
                let m = rng.mutate(&seed_psd);
                assert_no_panic(&format!("swc_and_palette_mut_{iter}"), || {
                    let _ = import_swc_bytes("m.swc", &m);
                    let _ = import_act_palette(&m, "fuzz");
                    let _ = import_clr_palette(&m, "fuzz");
                    let _ = import_ase_palette(&m);
                });
            }
        }
    }

    assert!(
        cases >= 450,
        "expected >= 450 cases in family 6, got {cases}"
    );
}

#[allow(dead_code)]
fn _keep_path_import_check(_: &Path) {}
