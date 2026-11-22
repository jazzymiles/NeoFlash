//! Integration tests for [T-461]: Binary FLA source-archive retention (`reconstruct_legacy_fla_bytes`
//! across Schema 2 raw, Schema 3 `"gzip"`, Schema 3 `"resource-delta-gzip"` multi-part, and
//! `"external"` SHA-256 provenance), pre-provenance lossless repeated-shape `Graphic` deduplication
//! preserving `cfb` pre-order symbol IDs, deterministic legacy MBCS/SBCS ANSI codepage decoding,
//! historical pre-R9 `CPicLayer` guide/mask/folder parent resolution, lossless `LoopFrom` graphic
//! modulo normalization, and multi-placement `CPicSwf` AVM1 component parameter retention.
//!
//! 100% synthetic binary FLA fixtures; zero private corpus bytes or filenames.

use neoflash_core::extensions::sha256_hex;
use neoflash_core::*;
use neoflash_import::*;
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t461-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i16b(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mfc_bytes(out: &mut Vec<u8>, raw: &[u8]) {
    assert!(raw.len() < 255);
    out.push(raw.len() as u8);
    out.extend_from_slice(raw);
}
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    mfc_bytes(out, s.as_bytes());
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    for ch in s.encode_utf16() {
        u16b(out, ch);
    }
}
fn push_mfc_new_class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xffff);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());
}
fn push_contents_header(out: &mut Vec<u8>, rev: u8) {
    out.push(rev);
    if rev >= 0x20 {
        u32b(out, 1);
    }
    u32b(out, 0);
    out.push(0);
    if rev >= 0x15 {
        out.push(0);
    }
    for gate in [0x21, 0x2b, 0x3b, 0x40, 0x44] {
        if rev >= gate {
            u32b(out, 0);
        }
    }
}
fn push_contents_settings_v3f(out: &mut Vec<u8>, media_counter: u16) {
    u16b(out, media_counter);
    u16b(out, 0);
    out.push(0);
    for word in [0_i32, 11000, 0, 8000] {
        i32b(out, word);
    }
    u32b(out, 20 * 65536);
    out.extend_from_slice(&[0; 5]);
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(&[0; 4]);
    i32b(out, 65536);
    u16b(out, 0);
    u16b(out, 0);
    u32b(out, 0xffff_ffff);
    u32b(out, 0);
    u32b(out, 24 * 65536);
    out.extend_from_slice(&[0; 3]);
    i32b(out, 20);
}

fn pseudo_random_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let word = state.wrapping_mul(0x2545_f491_4f6c_dd1d).to_le_bytes();
        let take = (len - out.len()).min(8);
        out.extend_from_slice(&word[..take]);
    }
    out
}

/// Build a valid Flash 8 (`0x3f`) `Contents` stream with optional library symbols
/// `(lib_id, stream_name, raw_name_bytes, symbol_type)` and optional PCM sound media
/// `(media_id, stream_name, name, sample_count, format_flags)`.
fn build_contents_v3f_with_sounds(
    symbols: &[(u32, &str, &[u8], u8)],
    sounds: &[(u16, &str, &str, u32, u16)],
) -> Vec<u8> {
    let mut out = Vec::new();
    push_contents_header(&mut out, 0x3f);

    push_mfc_new_class(&mut out, "CDocumentPage"); // ref 1 (class), ref 2 (Scene 1)
    let mut next_ref = 3_u16;
    out.push(5);
    mfc_str(&mut out, "Page 1");
    mfc_str(&mut out, "Scene 1");
    u32b(&mut out, 0);
    out.push(2);
    u32b(&mut out, 3);
    u32b(&mut out, 0);
    u32b(&mut out, 0);
    u32b(&mut out, 0);
    u16b(&mut out, 0); // end scenes

    u16b(&mut out, 1);
    u16b(&mut out, 1);
    u16b(&mut out, 2);

    for &(lib_id, stream_name, sym_name, sym_type) in symbols {
        u16b(&mut out, 0x8001);
        next_ref += 1;
        out.push(5);
        mfc_str(&mut out, stream_name);
        mfc_bytes(&mut out, sym_name);
        u32b(&mut out, lib_id);
        out.push(sym_type);
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
    }
    u16b(&mut out, 0); // end library
    u16b(&mut out, 100);

    let mut snd_cls: Option<u16> = None;
    for &(media_id, stream_name, snd_name, sample_count, format_flags) in sounds {
        if let Some(cid) = snd_cls {
            u16b(&mut out, 0x8000 | cid);
        } else {
            push_mfc_new_class(&mut out, "CMediaSound");
            snd_cls = Some(next_ref);
            next_ref += 1;
        }
        next_ref += 1;
        out.push(6); // common media descriptor rev 6 (UTF-16 stream name)
        compact_utf16(&mut out, stream_name);
        mfc_str(&mut out, snd_name);
        u16b(&mut out, media_id);
        mfc_str(&mut out, ""); // rev >= 2
        u32b(&mut out, 0); // rev >= 3
        // library_metadata rev 3:
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0); // rev >= 5
        // CMediaSound subtype rev 5 (no +4 offset on format_flags):
        out.push(5);
        u16b(&mut out, format_flags);
        u32b(&mut out, sample_count);
        i16b(&mut out, 0); // export_format (v >= 4)
        i16b(&mut out, 0); // export_bits (v >= 4)
    }
    u16b(&mut out, 0); // end media

    push_contents_settings_v3f(&mut out, (sounds.len() + 1) as u16);
    out
}

struct StreamWriter {
    bytes: Vec<u8>,
    classes: std::collections::BTreeMap<&'static str, u16>,
    next_id: u16,
}
impl StreamWriter {
    fn new() -> Self {
        Self {
            bytes: vec![1],
            classes: std::collections::BTreeMap::new(),
            next_id: 1,
        }
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(&cid) = self.classes.get(class) {
            u16b(&mut self.bytes, 0x8000 | cid);
        } else {
            push_mfc_new_class(&mut self.bytes, class);
            self.classes.insert(class, self.next_id);
            self.next_id += 1;
        }
        let oid = self.next_id;
        self.next_id += 1;
        oid
    }
    fn base_start(&mut self) {
        self.bytes.push(1);
        self.bytes.push(3);
    }
    fn base_end(&mut self) {
        u16b(&mut self.bytes, 0);
        i32b(&mut self.bytes, i32::MIN);
        i32b(&mut self.bytes, i32::MIN);
    }
    fn empty_shape(&mut self) {
        self.bytes.push(2);
        for w in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, w);
        }
        self.bytes.push(5);
        self.bytes.extend_from_slice(&[0; 9]);
        u32b(&mut self.bytes, 0);
    }
    fn simple_rect_shape(&mut self, size_px: i32, rgba: u32) {
        self.bytes.push(2);
        for w in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, w);
        }
        self.bytes.push(2);
        u32b(&mut self.bytes, 0);
        u16b(&mut self.bytes, 1);
        u32b(&mut self.bytes, rgba);
        self.bytes.push(0);
        self.bytes.push(0);
        u16b(&mut self.bytes, 0);
        let s = size_px * 5120;
        let corners = [[0, 0], [s, 0], [s, s], [0, s], [0, 0]];
        for (ei, w) in corners.windows(2).enumerate() {
            let [x0, y0] = w[0];
            let [x1, y1] = w[1];
            if ei == 0 {
                self.bytes.push(0x80 | 0x40 | 0x20);
                self.bytes.extend_from_slice(&[0, 0, 1]);
            } else {
                self.bytes.push(0x20);
            }
            i32b(&mut self.bytes, x1 - x0);
            i32b(&mut self.bytes, y1 - y0);
        }
        self.bytes.push(0);
    }
    fn layer_tail_rev2(&mut self, name: &str) {
        self.bytes.push(2);
        mfc_str(&mut self.bytes, name);
        self.bytes.extend_from_slice(&[0, 0]); // 1-byte state_flags + layer_type=0
        u16b(&mut self.bytes, 0); // RelatedLayer = null
        self.bytes.push(2); // legacy_flag = 2 (visible)
    }
    fn page_tail(&mut self) {
        self.bytes.push(1);
        u16b(&mut self.bytes, 1);
    }
}

fn build_simple_page_stream() -> Vec<u8> {
    let mut w = StreamWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    w.base_end();
    w.simple_rect_shape(30, 0xff22_77cc);
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);
    w.bytes.push(0);
    w.base_end();
    w.layer_tail_rev2("Main");
    w.base_end();
    w.page_tail();
    w.bytes
}

fn write_cfb_fla(path: &std::path::Path, streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for &(name, data) in streams {
        cfb.create_stream(name).unwrap().write_all(data).unwrap();
    }
    let bytes = cfb.into_inner().into_inner();
    fs::write(path, &bytes).unwrap();
    bytes
}

fn read_provenance_json(project: &Project) -> serde_json::Value {
    let res = project
        .resource("fla-provenance")
        .expect("fla-provenance resource must exist");
    let raw = res.decoded_bytes().expect("decode fla-provenance");
    serde_json::from_slice(&raw).expect("valid fla-provenance JSON")
}

#[test]
fn reconstruct_legacy_fla_bytes_round_trips_schema2_raw_schema3_gzip_and_rejects_tampering() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f_with_sounds(&[], &[]);
    let page = build_simple_page_stream();

    // 1. Schema 2 raw archive (`source_len <= budget`).
    let raw_path = tmp.0.join("schema2_raw.fla");
    let raw_orig = write_cfb_fla(&raw_path, &[("Contents", &contents), ("Page 1", &page)]);
    let raw_proj = import_project(&raw_path).unwrap();
    raw_proj.validate().unwrap();
    let raw_prov = read_provenance_json(&raw_proj);
    assert_eq!(raw_prov["schema"], 2);
    assert_eq!(
        raw_prov["source_resources"],
        serde_json::json!(["fla-original"])
    );
    let raw_recovered = reconstruct_legacy_fla_bytes(&raw_proj)
        .expect("reconstruct schema 2")
        .expect("embedded schema 2 bytes");
    assert_eq!(sha256_hex(&raw_recovered), sha256_hex(&raw_orig));
    assert_eq!(raw_recovered, raw_orig);

    // 2. Schema 3 `"gzip"` archive: 66 MiB compressible CFB container (two 33 MiB CFB streams
    //    so each stream stays under the 64 MiB per-stream cap) -> ~100 KiB gzipped.
    let zero_aux = vec![0_u8; 33 * 1024 * 1024];
    let gz_path = tmp.0.join("schema3_gzip.fla");
    let gz_orig = write_cfb_fla(
        &gz_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("ZeroPadAux1", &zero_aux),
            ("ZeroPadAux2", &zero_aux),
        ],
    );
    let gz_rep = inspect(&gz_path).unwrap();
    assert!(
        gz_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_source_not_embedded"),
        "compressible 66 MiB FLA must embed via Schema 3 gzip: {:?}",
        gz_rep.diagnostics
    );
    let gz_proj = import_project(&gz_path).unwrap();
    gz_proj.validate().unwrap();
    let gz_prov = read_provenance_json(&gz_proj);
    assert_eq!(gz_prov["schema"], 3);
    assert_eq!(gz_prov["source_archive"]["encoding"], "gzip");
    assert_eq!(
        gz_prov["source_archive"]["source_sha256"],
        sha256_hex(&gz_orig)
    );
    assert_eq!(gz_prov["source_archive"]["source_byte_len"], gz_orig.len());
    let gz_recovered = reconstruct_legacy_fla_bytes(&gz_proj)
        .expect("reconstruct schema 3 gzip")
        .expect("embedded schema 3 gzip bytes");
    assert_eq!(sha256_hex(&gz_recovered), sha256_hex(&gz_orig));
    assert_eq!(gz_recovered, gz_orig);

    // 3. Adversarial tamper checks: digest mismatch & malformed NFD1 delta opcodes must fail cleanly.
    let mut bad_sha_proj = gz_proj.clone();
    let mut bad_prov = gz_prov.clone();
    bad_prov["source_sha256"] = serde_json::json!("0".repeat(64));
    bad_prov["source_archive"]["source_sha256"] = serde_json::json!("0".repeat(64));
    let slot = bad_sha_proj
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-provenance")
        .unwrap();
    *slot = ResourceUnit::from_bytes(
        "fla-provenance",
        "legacy/provenance.json",
        "application/json",
        &serde_json::to_vec(&bad_prov).unwrap(),
    )
    .unwrap();
    let err = reconstruct_legacy_fla_bytes(&bad_sha_proj).unwrap_err();
    assert!(
        err.to_string().contains("SHA-256"),
        "expected SHA-256 mismatch error, got {err}"
    );

    // Crafted NFD1 delta stream with out-of-bounds resource slice must be rejected.
    let mut bad_delta_raw = Vec::new();
    bad_delta_raw.extend_from_slice(b"NFD1");
    bad_delta_raw.extend_from_slice(&16_u64.to_le_bytes()); // expected_len = 16
    bad_delta_raw.extend_from_slice(&1_u16.to_le_bytes()); // 1 ref resource
    bad_delta_raw.push(1); // DELTA_OP_RESOURCE_SLICE = 0x01
    bad_delta_raw.extend_from_slice(&0_u16.to_le_bytes()); // ref 0
    bad_delta_raw.extend_from_slice(&9999_u32.to_le_bytes()); // out-of-bounds offset
    bad_delta_raw.extend_from_slice(&16_u32.to_le_bytes());
    bad_delta_raw.push(0xff); // DELTA_OP_END = 0xff
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&bad_delta_raw).unwrap();
    let bad_delta_gz = enc.finish().unwrap();

    let mut bad_delta_proj = raw_proj.clone();
    bad_delta_proj.resources.push(
        ResourceUnit::from_bytes("ref-0", "legacy/sounds/1/s.wav", "audio/wav", &[1, 2, 3, 4])
            .unwrap(),
    );
    *bad_delta_proj
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-original")
        .unwrap() = ResourceUnit::from_bytes(
        "fla-original",
        "legacy/source.fla.gz",
        "application/gzip",
        &bad_delta_gz,
    )
    .unwrap();
    let crafted_prov = serde_json::json!({
        "schema": 3,
        "source_bytes": 16,
        "source_fnv1a64": "0000000000000000",
        "source_archive": {
            "encoding": "resource-delta-gzip",
            "resources": ["fla-original"],
            "reference_resources": ["ref-0"],
            "source_byte_len": 16
        }
    });
    *bad_delta_proj
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-provenance")
        .unwrap() = ResourceUnit::from_bytes(
        "fla-provenance",
        "legacy/provenance.json",
        "application/json",
        &serde_json::to_vec(&crafted_prov).unwrap(),
    )
    .unwrap();
    let oob_err = reconstruct_legacy_fla_bytes(&bad_delta_proj).unwrap_err();
    assert!(
        oob_err.to_string().contains("out of bounds"),
        "expected out-of-bounds slice rejection, got {oob_err}"
    );
}

#[test]
fn resource_delta_gzip_embeds_wav_media_heavy_fla_multipart_and_external_records_sha256() {
    let tmp = TempDir::new();

    // Three 12 MiB 16-bit 44.1 kHz stereo PCM streams (36 MiB total incompressible audio).
    // format_flags = (3 << 2) | 2 | 1 = 15 (codec 0 PCM, 44.1 kHz, 16-bit, stereo = 4 bytes/sample).
    let pcm_len = 12 * 1024 * 1024;
    let sample_count = (pcm_len / 4) as u32;
    let pcm_1 = pseudo_random_bytes(0x1111_2222_3333_4444, pcm_len);
    let pcm_2 = pseudo_random_bytes(0x5555_6666_7777_8888, pcm_len);
    let pcm_3 = pseudo_random_bytes(0x9999_aaaa_bbbb_cccc, pcm_len);

    let contents = build_contents_v3f_with_sounds(
        &[],
        &[
            (1, "Media 1", "TrackA", sample_count, 15),
            (2, "Media 2", "TrackB", sample_count, 15),
            (3, "Media 3", "TrackC", sample_count, 15),
        ],
    );
    let page = build_simple_page_stream();

    // Case A: +20 MiB incompressible CFB stream -> ~56.1 MiB FLA, ~56.1 MiB plain gzip,
    // 36 MiB WAV in project.resources -> remaining budget ~27.9 MiB.
    // Plain gzip (56.1 MiB > 27.9 MiB) FAILS, whereas resource-delta-gzip deduplicates
    // the 36 MiB PCM slices against legacy-sound-1..=3 and fits (~20.1 MiB across 2 parts <= 27.9 MiB)!
    let aux_20m = pseudo_random_bytes(0xdddd_eeee_ffff_1234, 20 * 1024 * 1024);
    let delta_path = tmp.0.join("wav_delta_embedded.fla");
    let delta_orig = write_cfb_fla(
        &delta_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("Media 1", &pcm_1),
            ("Media 2", &pcm_2),
            ("Media 3", &pcm_3),
            ("AuxBlob", &aux_20m),
        ],
    );

    let delta_rep = inspect(&delta_path).unwrap();
    assert!(delta_rep.can_import, "{:?}", delta_rep.diagnostics);
    assert!(
        delta_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_source_not_embedded" && d.code != "fla_media_not_imported"),
        "56 MiB WAV-heavy FLA must embed via resource-delta-gzip without dropping media: {:?}",
        delta_rep.diagnostics
    );

    let delta_proj = import_project(&delta_path).unwrap();
    delta_proj.validate().unwrap();
    let delta_prov = read_provenance_json(&delta_proj);
    assert_eq!(delta_prov["schema"], 3);
    assert_eq!(
        delta_prov["source_archive"]["encoding"],
        "resource-delta-gzip"
    );
    assert_eq!(
        delta_prov["source_archive"]["source_sha256"],
        sha256_hex(&delta_orig)
    );
    assert_eq!(
        delta_prov["source_archive"]["source_byte_len"],
        delta_orig.len()
    );
    let archive_parts = delta_prov["source_archive"]["resources"]
        .as_array()
        .expect("source_archive.resources array");
    assert!(
        archive_parts.len() >= 2,
        "20+ MiB delta stream should span multiple <=16 MiB resource parts: {archive_parts:?}"
    );
    let ref_res: Vec<&str> = delta_prov["source_archive"]["reference_resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ref_res,
        vec!["legacy-sound-1", "legacy-sound-2", "legacy-sound-3"]
    );

    let delta_recovered = reconstruct_legacy_fla_bytes(&delta_proj)
        .expect("reconstruct resource-delta-gzip")
        .expect("embedded resource-delta-gzip bytes");
    assert_eq!(sha256_hex(&delta_recovered), sha256_hex(&delta_orig));
    assert_eq!(delta_recovered, delta_orig);

    // Editable WAV media remains intact for XFL export:
    for id in ["legacy-sound-1", "legacy-sound-2", "legacy-sound-3"] {
        let wav = delta_proj
            .resource(id)
            .expect("editable WAV resource retained");
        assert_eq!(wav.media_type, "audio/wav");
        assert!(wav.byte_len() > pcm_len);
    }
    let xfl_zip = export_fla_bytes(&delta_proj).expect("export valid XFL archive");
    assert!(!xfl_zip.is_empty());

    // Case B: +34 MiB incompressible CFB stream -> 36 MiB WAV + 34 MiB unmatched > 64 MiB resource cap.
    // Must preserve all 3 editable WAV resources, emit fla_source_not_embedded, and record
    // deterministic external provenance metadata (`encoding: "external"`, `source_sha256`, `source_byte_len`).
    let aux_34m = pseudo_random_bytes(0x7777_8888_9999_0001, 34 * 1024 * 1024);
    let ext_path = tmp.0.join("wav_truly_external.fla");
    let ext_orig = write_cfb_fla(
        &ext_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("Media 1", &pcm_1),
            ("Media 2", &pcm_2),
            ("Media 3", &pcm_3),
            ("AuxBlob", &aux_34m),
        ],
    );

    let ext_rep = inspect(&ext_path).unwrap();
    assert!(ext_rep.can_import, "{:?}", ext_rep.diagnostics);
    assert!(
        ext_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_source_not_embedded"),
        "genuinely >64 MiB incompressible WAV+aux FLA must report fla_source_not_embedded"
    );
    assert!(
        ext_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_media_not_imported"),
        "editable WAV media must be preserved for SWF/XFL export: {:?}",
        ext_rep.diagnostics
    );

    let ext_proj = import_project(&ext_path).unwrap();
    ext_proj.validate().unwrap();
    for id in ["legacy-sound-1", "legacy-sound-2", "legacy-sound-3"] {
        assert!(
            ext_proj.resource(id).is_some(),
            "editable WAV resource {id} must be preserved when source is external"
        );
    }
    let ext_prov = read_provenance_json(&ext_proj);
    let expected_sha = sha256_hex(&ext_orig);
    assert_eq!(ext_prov["source_sha256"], expected_sha);
    assert_eq!(ext_prov["source_byte_len"], ext_orig.len());
    assert_eq!(ext_prov["source_archive"]["encoding"], "external");
    assert_eq!(
        ext_prov["source_archive"]["resources"],
        serde_json::json!([])
    );
    assert_eq!(ext_prov["source_archive"]["source_sha256"], expected_sha);
    assert_eq!(
        ext_prov["source_archive"]["source_byte_len"],
        ext_orig.len()
    );
    assert!(
        reconstruct_legacy_fla_bytes(&ext_proj)
            .expect("external provenance returns Ok(None)")
            .is_none()
    );
}

#[test]
fn pre_provenance_vector_dedup_retains_embedded_source_and_preserves_preorder_symbol_ids() {
    let tmp = TempDir::new();

    // Two library symbols whose cfb pre-order stream IDs (`fla-symbol-2`, `fla-symbol-3`) MUST stay invariant.
    let contents = build_contents_v3f_with_sounds(
        &[
            (10, "Symbol 1", b"FirstLibSym", 1),
            (11, "Symbol 2", b"SecondLibSym", 2),
        ],
        &[],
    );
    let sym1 = build_simple_page_stream();
    let sym2 = build_simple_page_stream();

    // Build a complex single-fill polygon shape with 240 straight segments (~10 KiB JSON per element)
    // repeated across 12,000 keyframes (< 100,000 MAX_AUTHORED_ELEMENTS!) -> ~120 MiB raw JSON!
    // Combine with a 15 MiB incompressible CFB stream (~20 MiB base64 in JSON):
    // Without pre-provenance shape deduplication, `120 MiB + 20 MiB = 140 MiB > 128 MiB`,
    // which used to drop the embedded `.fla` (`fla_source_not_embedded`)!
    let mut complex_shape = Vec::new();
    complex_shape.push(2);
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut complex_shape, w);
    }
    complex_shape.push(2);
    u32b(&mut complex_shape, 0);
    u16b(&mut complex_shape, 1);
    u32b(&mut complex_shape, 0xff33_66cc);
    complex_shape.push(0);
    complex_shape.push(0);
    u16b(&mut complex_shape, 0);

    let n_pts = 240_usize;
    let mut pts = Vec::with_capacity(n_pts + 1);
    for i in 0..n_pts {
        let angle = (i as f64) * std::f64::consts::TAU / (n_pts as f64);
        let r = if i % 2 == 0 { 80_000.0 } else { 52_345.0 };
        pts.push([
            (r * angle.cos()).round() as i32,
            (r * angle.sin()).round() as i32,
        ]);
    }
    pts.push(pts[0]);
    let mut prev = [0_i32, 0];
    for (ei, pair) in pts.windows(2).enumerate() {
        let [x0, y0] = pair[0];
        let [x1, y1] = pair[1];
        let flags = (if ei == 0 { 0x80 | 0x40 } else { 0 }) | 0x20 | 0x02;
        complex_shape.push(flags);
        if ei == 0 {
            complex_shape.extend_from_slice(&[0, 0, 1]);
        }
        i32b(&mut complex_shape, x0 - prev[0]);
        i32b(&mut complex_shape, y0 - prev[1]);
        i32b(&mut complex_shape, x1 - x0);
        i32b(&mut complex_shape, y1 - y0);
        prev = [x1, y1];
    }
    complex_shape.push(0);

    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    for _ in 0..12_000 {
        page.begin("CPicFrame");
        page.base_start();
        page.base_end();
        page.bytes.extend_from_slice(&complex_shape);
        page.bytes.push(1);
        u16b(&mut page.bytes, 1);
        page.bytes.push(0);
    }
    page.base_end();
    page.layer_tail_rev2("HeavyVectorFrames");
    page.base_end();
    page.page_tail();

    let aux_15m = pseudo_random_bytes(0x4242_1337_9001_cafe, 15 * 1024 * 1024);
    let fla_path = tmp.0.join("vector_json_compaction.fla");
    let orig_bytes = write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Symbol 1", &sym1),
            ("Symbol 2", &sym2),
            ("IncompressiblePayload", &aux_15m),
        ],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_source_not_embedded" && d.code != "fla_shape_approximated"),
        "pre-provenance Graphic shape dedup must keep JSON + embedded FLA well under 128 MiB: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();

    // Verify cfb pre-order symbol IDs remain 100% invariant and shared shape symbol is appended after!
    let sym_ids_and_names: Vec<(&str, &str)> = proj
        .symbols
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    let first_idx = sym_ids_and_names
        .iter()
        .position(|(_, name)| *name == "FirstLibSym")
        .expect("FirstLibSym imported");
    let second_idx = sym_ids_and_names
        .iter()
        .position(|(_, name)| *name == "SecondLibSym")
        .expect("SecondLibSym imported");
    let shared_idx = sym_ids_and_names
        .iter()
        .position(|(id, _)| id.starts_with("fla-shared-shape-"))
        .expect("fla-shared-shape-* created");
    assert!(proj.symbols[first_idx].id.starts_with("fla-symbol-"));
    assert!(proj.symbols[second_idx].id.starts_with("fla-symbol-"));
    assert!(
        shared_idx > first_idx && shared_idx > second_idx,
        "shared Graphic symbols must be appended after all stream symbols: {sym_ids_and_names:?}"
    );

    let recovered = reconstruct_legacy_fla_bytes(&proj)
        .expect("reconstruct deduplicated FLA")
        .expect("source FLA embedded");
    assert_eq!(sha256_hex(&recovered), sha256_hex(&orig_bytes));
}

fn push_cpic_text_rev4_run(w: &mut StreamWriter, char_set: u8, raw_text: &[u8]) {
    w.begin("CPicText");
    w.base_start();
    w.base_end();
    w.bytes.push(4); // CPicText rev 4 (Flash 5)
    for m in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut w.bytes, m);
    }
    for b in [0_i32, 4000, 0, 800] {
        i32b(&mut w.bytes, b);
    }
    w.bytes.push(0); // auto_expand
    w.bytes.push(0); // reserved (rev > 2)
    u16b(&mut w.bytes, 0); // flags (u16 for rev 4)
    u16b(&mut w.bytes, 0); // max_characters
    mfc_str(&mut w.bytes, ""); // variable_name

    // 1 run:
    u16b(&mut w.bytes, raw_text.len() as u16);
    // text_style rev 4 (8-bit compact style strings):
    w.bytes.push(4);
    u16b(&mut w.bytes, 240); // 12pt
    w.bytes.push(5);
    w.bytes.extend_from_slice(b"Arial");
    u32b(&mut w.bytes, 0xff00_0000);
    w.bytes.push(0); // pitch_and_family
    w.bytes.push(char_set); // LOGFONT.lfCharSet
    w.bytes.extend_from_slice(&[0, 0, 1, 1, 0, 0]); // bold, italic, screen, kern, pos, align
    for _ in 0..5 {
        i16b(&mut w.bytes, 0);
    }
    w.bytes.extend_from_slice(raw_text);
    u16b(&mut w.bytes, 0); // end runs
}

fn build_ole_summary_with_codepage(codepage: u16) -> Vec<u8> {
    let mut sum_info = Vec::new();
    u16b(&mut sum_info, 0xfffe);
    u16b(&mut sum_info, 0);
    u32b(&mut sum_info, 0x0002_0005);
    sum_info.extend_from_slice(&[0; 16]);
    u32b(&mut sum_info, 1); // 1 section
    sum_info.extend_from_slice(&[
        0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3,
        0xd9,
    ]);
    u32b(&mut sum_info, 48); // section offset = 48
    u32b(&mut sum_info, 24); // section byte len = 24
    u32b(&mut sum_info, 1); // 1 property
    u32b(&mut sum_info, 1); // PIDSI_CODEPAGE = 1
    u32b(&mut sum_info, 16); // value offset = 16
    u32b(&mut sum_info, 0x0002); // VT_I2
    u16b(&mut sum_info, codepage);
    u16b(&mut sum_info, 0);
    sum_info
}

#[test]
fn deterministic_ansi_charset_and_ole_codepage_decode_cleanly_while_ambiguous_warns() {
    let tmp = TempDir::new();

    // 1. Six explicit GDI lfCharSet text runs in one timeline (`0` ANSI_CHARSET = Windows-1252,
    //    `204` RUSSIAN_CHARSET = Windows-1251, `128` SHIFTJIS_CHARSET = Shift_JIS,
    //    `134` GB2312_CHARSET = GBK, `129` HANGUL_CHARSET = EUC-KR, `136` CHINESEBIG5_CHARSET = Big5).
    let contents_multi = build_contents_v3f_with_sounds(&[], &[]);
    let mut page_multi = StreamWriter::new();
    page_multi.begin("CPicPage");
    page_multi.base_start();
    page_multi.begin("CPicLayer");
    page_multi.base_start();
    page_multi.begin("CPicFrame");
    page_multi.base_start();
    push_cpic_text_rev4_run(&mut page_multi, 0, &[0xf1]); // 'ñ' in Windows-1252 (was misread as 'с'!)
    push_cpic_text_rev4_run(&mut page_multi, 204, &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2]); // "Привет"
    push_cpic_text_rev4_run(&mut page_multi, 128, &[0x93, 0xfa, 0x96, 0x7b]); // "日本"
    push_cpic_text_rev4_run(&mut page_multi, 134, &[0xd6, 0xd0, 0xce, 0xc4]); // "中文"
    push_cpic_text_rev4_run(&mut page_multi, 129, &[0xc7, 0xd1, 0xb1, 0xdb]); // "한글"
    push_cpic_text_rev4_run(&mut page_multi, 136, &[0xb6, 0x7d, 0xb5, 0x6f]); // "開發"
    page_multi.base_end();
    page_multi.empty_shape();
    page_multi.bytes.push(1);
    u16b(&mut page_multi.bytes, 1);
    page_multi.bytes.push(0);
    page_multi.base_end();
    page_multi.layer_tail_rev2("Texts");
    page_multi.base_end();
    page_multi.page_tail();

    let multi_path = tmp.0.join("gdi_charsets.fla");
    write_cfb_fla(
        &multi_path,
        &[("Contents", &contents_multi), ("Page 1", &page_multi.bytes)],
    );
    let multi_rep = inspect(&multi_path).unwrap();
    assert!(
        multi_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_ansi_code_page"),
        "explicit GDI lfCharSet runs must decode without fla_ansi_code_page: {:?}",
        multi_rep.diagnostics
    );
    let multi_proj = import_project(&multi_path).unwrap();
    multi_proj.validate().unwrap();
    let elems =
        &multi_proj.symbol(&multi_proj.root).unwrap().timeline.layers[0].keyframes[0].elements;
    let decoded_texts: Vec<&str> = elems
        .iter()
        .map(|e| match &e.kind {
            ElementKind::TextBox { text, .. } => text.as_str(),
            other => panic!("expected TextBox, got {other:?}"),
        })
        .collect();
    assert_eq!(
        decoded_texts,
        vec!["ñ", "Привет", "日本", "中文", "한글", "開發"]
    );

    // 2. Document-level inference: 8-bit Contents symbol name `[0xf1]` ("ñ") with unanimous
    //    `ANSI_CHARSET` (`0`) text run in Symbol 1 -> decodes symbol name as "ñ" with 0 warnings!
    let contents_es = build_contents_v3f_with_sounds(&[(10, "Symbol 1", &[0xf1], 2)], &[]);
    let mut sym_es = StreamWriter::new();
    sym_es.begin("CPicPage");
    sym_es.base_start();
    sym_es.begin("CPicLayer");
    sym_es.base_start();
    sym_es.begin("CPicFrame");
    sym_es.base_start();
    push_cpic_text_rev4_run(&mut sym_es, 0, &[0xf1]);
    sym_es.base_end();
    sym_es.empty_shape();
    sym_es.bytes.push(1);
    u16b(&mut sym_es.bytes, 1);
    sym_es.bytes.push(0);
    sym_es.base_end();
    sym_es.layer_tail_rev2("Letra");
    sym_es.base_end();
    sym_es.page_tail();

    let es_path = tmp.0.join("spanish_tilde_n.fla");
    write_cfb_fla(
        &es_path,
        &[
            ("Contents", &contents_es),
            ("Page 1", &build_simple_page_stream()),
            ("Symbol 1", &sym_es.bytes),
        ],
    );
    let es_rep = inspect(&es_path).unwrap();
    assert!(
        es_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_ansi_code_page"),
        "unanimous ANSI_CHARSET (0) must infer Windows-1252 for 8-bit Contents symbol name: {:?}",
        es_rep.diagnostics
    );
    let es_proj = import_project(&es_path).unwrap();
    assert!(
        es_proj.symbols.iter().any(|s| s.name == "ñ"),
        "symbol name [0xf1] must decode as 'ñ', got {:?}",
        es_proj.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    // 3. [MS-OLEPS] SummaryInformation PIDSI_CODEPAGE = 932 (Shift_JIS) decodes 8-bit symbol name!
    let contents_sjis =
        build_contents_v3f_with_sounds(&[(10, "Symbol 1", &[0x93, 0xfa, 0x96, 0x7b], 1)], &[]);
    let sum_932 = build_ole_summary_with_codepage(932);
    let sjis_path = tmp.0.join("ole_cp932.fla");
    write_cfb_fla(
        &sjis_path,
        &[
            ("Contents", &contents_sjis),
            ("Page 1", &build_simple_page_stream()),
            ("Symbol 1", &build_simple_page_stream()),
            ("\u{0005}SummaryInformation", &sum_932),
        ],
    );
    let sjis_rep = inspect(&sjis_path).unwrap();
    assert!(
        sjis_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_ansi_code_page"),
        "PIDSI_CODEPAGE=932 must deterministically decode Shift_JIS symbol name: {:?}",
        sjis_rep.diagnostics
    );
    let sjis_proj = import_project(&sjis_path).unwrap();
    assert!(sjis_proj.symbols.iter().any(|s| s.name == "日本"));

    // 4. Genuinely ambiguous 8-bit symbol name `[0xf1]` with NO SummaryInformation and NO charset tags
    //    MUST still emit `fla_ansi_code_page`!
    let ambig_path = tmp.0.join("ambiguous_ansi.fla");
    write_cfb_fla(
        &ambig_path,
        &[
            ("Contents", &contents_es),
            ("Page 1", &build_simple_page_stream()),
            ("Symbol 1", &build_simple_page_stream()),
        ],
    );
    let ambig_rep = inspect(&ambig_path).unwrap();
    assert!(
        ambig_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_ansi_code_page"),
        "untagged non-ASCII 8-bit string must keep fla_ansi_code_page warning"
    );
}

#[test]
fn legacy_pre_r9_guide_mask_and_r8_folder_layer_relations_convert_while_broken_link_warns() {
    let tmp = TempDir::new();
    let contents = build_contents_v3f_with_sounds(&[], &[]);

    // Build a timeline stream containing:
    // - Layer A (rev 3 Guide `layer_type = 1`) <-> Layer B (rev 3 Guided `layer_type = 2`) reciprocal `RelatedLayer`
    // - Layer C (rev 3 Mask `layer_type = 0`, `legacyMask = 1`) <-> Layer D (rev 3 Masked `layer_type = 0`, `legacyMask = 0`) reciprocal `RelatedLayer`
    // - Layer E (rev 8 Folder `layer_type = 3`) + Layer F (rev 8 Normal `layer_type = 0`, `RelatedLayer = null`, `LegacyRelatedLayer = Layer E`)
    let mut page = StreamWriter::new();
    page.begin("CPicPage"); // oid 2
    page.base_start();

    let add_one_frame = |w: &mut StreamWriter| {
        w.begin("CPicFrame");
        w.base_start();
        w.base_end();
        w.simple_rect_shape(20, 0xff44_88aa);
        w.bytes.push(1);
        u16b(&mut w.bytes, 1);
        w.bytes.push(0);
    };

    // Layer A (Guide, rev 3) introduces Layer B inline via RelatedLayer, then Layer B's RelatedLayer
    // references Layer A! And Page's child list also references Layer B via back-reference `oid_b`.
    let oid_a = page.begin("CPicLayer");
    page.base_start();
    add_one_frame(&mut page);
    page.base_end();
    page.bytes.push(3); // CPicLayer rev 3
    mfc_str(&mut page.bytes, "GuideR3");
    page.bytes.push(0); // 1-byte state_flags for rev <= 3
    page.bytes.push(1); // layer_type = 1 (Guide)
    // RelatedLayer inline introduces Layer B:
    let oid_b = page.begin("CPicLayer");
    page.base_start();
    add_one_frame(&mut page);
    page.base_end();
    page.bytes.push(3); // CPicLayer rev 3
    mfc_str(&mut page.bytes, "GuidedR3");
    page.bytes.push(0);
    page.bytes.push(2); // layer_type = 2 (Guided)
    u16b(&mut page.bytes, oid_a); // Layer B RelatedLayer -> Layer A!
    page.bytes.push(2); // legacy_flag (visible)
    page.bytes.push(0); // legacyMask = 0
    // Finish Layer A after its RelatedLayer object:
    page.bytes.push(2); // legacy_flag (visible)
    page.bytes.push(0); // legacyMask = 0

    // Include Layer B in Page's children list as an object back-reference:
    u16b(&mut page.bytes, oid_b);

    // Layer C (Mask, rev 3: layer_type = 0, legacyMask = 1) <-> Layer D (Masked, rev 3)
    let oid_c = page.begin("CPicLayer");
    page.base_start();
    add_one_frame(&mut page);
    page.base_end();
    page.bytes.push(3);
    mfc_str(&mut page.bytes, "MaskR3");
    page.bytes.push(0);
    page.bytes.push(0); // layer_type = 0 before R9!
    let oid_d = page.begin("CPicLayer");
    page.base_start();
    add_one_frame(&mut page);
    page.base_end();
    page.bytes.push(3);
    mfc_str(&mut page.bytes, "MaskedR3");
    page.bytes.push(0);
    page.bytes.push(0);
    u16b(&mut page.bytes, oid_c); // Layer D RelatedLayer -> Layer C (Mask)!
    page.bytes.push(2);
    page.bytes.push(0); // legacyMask = 0
    // Finish Layer C:
    page.bytes.push(2);
    page.bytes.push(1); // legacyMask = 1 (Mask layer!)

    u16b(&mut page.bytes, oid_d);

    // Layer E (Folder, rev 8) + Layer F (Normal, rev 8 with LegacyRelatedLayer -> Layer E)
    let oid_e = page.begin("CPicLayer");
    page.base_start();
    page.base_end();
    page.bytes.push(8); // CPicLayer rev 8 (Flash MX)
    mfc_str(&mut page.bytes, "FolderR8");
    page.bytes.extend_from_slice(&[0, 0, 0]);
    u32b(&mut page.bytes, 100); // height_literal (rev >= 5)
    u32b(&mut page.bytes, 0x0033_6699); // outline_color (rev >= 6)
    u32b(&mut page.bytes, 0); // outline
    u32b(&mut page.bytes, 100); // height_multiplier (rev >= 8)
    page.bytes.push(3); // layer_type = 3 (Folder)
    u16b(&mut page.bytes, 0); // RelatedLayer = null
    u16b(&mut page.bytes, 0); // LegacyRelatedLayer = null
    page.bytes.push(0); // legacyMask = 0 (NOT collapsed!)

    let _oid_f = page.begin("CPicLayer");
    page.base_start();
    add_one_frame(&mut page);
    page.base_end();
    page.bytes.push(8);
    mfc_str(&mut page.bytes, "InFolderR8");
    page.bytes.extend_from_slice(&[0, 0, 0]);
    u32b(&mut page.bytes, 100);
    u32b(&mut page.bytes, 0x0033_6699);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 100);
    page.bytes.push(0); // Normal
    u16b(&mut page.bytes, 0); // RelatedLayer = null!
    u16b(&mut page.bytes, oid_e); // LegacyRelatedLayer -> FolderR8!
    page.bytes.push(0); // legacyMask = 0

    page.base_end();
    page.page_tail();

    let ok_path = tmp.0.join("legacy_layer_relations.fla");
    write_cfb_fla(
        &ok_path,
        &[("Contents", &contents), ("Page 1", &page.bytes)],
    );
    let ok_rep = inspect(&ok_path).unwrap();
    assert!(
        ok_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_layer_relation"),
        "valid pre-R9 Guide/Mask and R8 LegacyRelatedLayer links must resolve without fla_layer_relation: {:?}",
        ok_rep.diagnostics
    );
    let ok_proj = import_project(&ok_path).unwrap();
    ok_proj.validate().unwrap();
    let layers = &ok_proj.symbol(&ok_proj.root).unwrap().timeline.layers;
    let by_name = |n: &str| layers.iter().find(|l| l.name == n).unwrap();
    let guide = by_name("GuideR3");
    let guided = by_name("GuidedR3");
    assert_eq!(guide.properties.role, LayerRole::Guide);
    assert_eq!(guided.properties.role, LayerRole::Normal);
    assert_eq!(
        guided.properties.motion_guide.as_deref(),
        Some(guide.id.as_str())
    );

    let mask = by_name("MaskR3");
    let masked = by_name("MaskedR3");
    assert_eq!(mask.properties.role, LayerRole::Mask);
    assert_eq!(masked.properties.mask.as_deref(), Some(mask.id.as_str()));

    let folder = by_name("FolderR8");
    let in_folder = by_name("InFolderR8");
    assert_eq!(folder.properties.role, LayerRole::Folder);
    assert!(
        !folder.properties.collapsed,
        "pre-R9 legacyMask=0 on Folder must not mark Folder collapsed"
    );
    assert_eq!(
        in_folder.properties.parent.as_deref(),
        Some(folder.id.as_str())
    );

    // Self-referential / broken RelatedLayer on rev 3 Guide MUST still emit `fla_layer_relation`:
    let mut bad = StreamWriter::new();
    bad.begin("CPicPage");
    bad.base_start();
    let self_oid = bad.begin("CPicLayer");
    bad.base_start();
    add_one_frame(&mut bad);
    bad.base_end();
    bad.bytes.push(3);
    mfc_str(&mut bad.bytes, "SelfCycleGuide");
    bad.bytes.push(0);
    bad.bytes.push(1); // Guide
    u16b(&mut bad.bytes, self_oid); // points to ITSELF!
    bad.bytes.push(2);
    bad.bytes.push(0);
    bad.base_end();
    bad.page_tail();

    let bad_path = tmp.0.join("broken_layer_relation.fla");
    write_cfb_fla(
        &bad_path,
        &[("Contents", &contents), ("Page 1", &bad.bytes)],
    );
    let bad_rep = inspect(&bad_path).unwrap();
    assert!(
        bad_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_layer_relation"),
        "self-pointing RelatedLayer must still emit fla_layer_relation"
    );
}

fn build_minimal_avm1_swf() -> Vec<u8> {
    // Minimal uncompressed SWF7 (`FWS`) with 15-bit RECT 0..2000 x 0..2000 twips, 24 fps, 1 frame,
    // a `DoAction` tag (tag 12, ActionStop `0x07`, end `0x00`), `ShowFrame` (tag 1), `End` (tag 0).
    let mut body = vec![
        0x78, 0x00, 0x03, 0xe8, 0x00, 0x00, 0x0f, 0xa0,
        0x00, // RECT(15 bits): 0..2000, 0..2000
        0x00, 0x18, // 24.0 fps
        0x01, 0x00, // 1 frame
    ];
    // DoAction (tag 12, len 2): (12 << 6) | 2 = 0x0302
    u16b(&mut body, (12 << 6) | 2);
    body.extend_from_slice(&[0x07, 0x00]); // stop();
    // ShowFrame (tag 1, len 0): 1 << 6 = 0x0040
    u16b(&mut body, 1 << 6);
    // End (tag 0, len 0)
    u16b(&mut body, 0);

    let total_len = (8 + body.len()) as u32;
    let mut swf = Vec::with_capacity(total_len as usize);
    swf.extend_from_slice(b"FWS");
    swf.push(7); // SWF 7 (AVM1)
    u32b(&mut swf, total_len);
    swf.extend_from_slice(&body);
    swf
}

fn push_cpic_swf_rev4_with_props(
    w: &mut StreamWriter,
    media_id: u16,
    inst_name: &str,
    tx_twips: i32,
    auto_play: bool,
    volume: f64,
) {
    w.begin("CPicSwf");
    w.base_start();
    w.base_end();
    w.bytes.push(4); // CPicSwf rev 4
    for m in [65536_i32, 0, 0, 65536, tx_twips, 0] {
        i32b(&mut w.bytes, m);
    }
    u32b(&mut w.bytes, 1); // media_present = 1
    u16b(&mut w.bytes, media_id);
    // empty actions (rev >= 2 -> frame_revision 19): rev = 0 (u32), mode = 2 (u32 => Empty)
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 2);
    // instance_name (rev >= 3):
    mfc_str(&mut w.bytes, inst_name);
    // component_properties (rev >= 4): revision = 0, count = 2 (u32)
    w.bytes.push(0);
    u32b(&mut w.bytes, 2);

    // Property 0: `autoPlay` Boolean (`legacy_type = 7`)
    u32b(&mut w.bytes, 7);
    mfc_str(&mut w.bytes, "autoPlay");
    mfc_str(&mut w.bytes, if auto_play { "true" } else { "false" });

    // Property 1: `volume` Number (`legacy_type = 6`)
    u32b(&mut w.bytes, 6);
    mfc_str(&mut w.bytes, "volume");
    mfc_str(&mut w.bytes, &format!("{volume}"));
}

#[test]
fn multiple_cpicswf_placements_of_same_avm1_compiled_clip_preserve_component_props_on_all_instances()
 {
    let tmp = TempDir::new();
    let swf = build_minimal_avm1_swf();

    // Build Contents v3f with 1 CSwfPage media descriptor (`media_id = 1`, `CSwfPage`).
    let mut contents = Vec::new();
    push_contents_header(&mut contents, 0x3f);
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
    u16b(&mut contents, 0);
    u16b(&mut contents, 1);
    u16b(&mut contents, 1);
    u16b(&mut contents, 2);
    u16b(&mut contents, 0); // library empty
    u16b(&mut contents, 100);

    // Media list: 1 CSwfPage (`media_id = 1`)
    push_mfc_new_class(&mut contents, "CSwfPage");
    contents.push(6); // descriptor rev 6
    compact_utf16(&mut contents, "Media 1");
    mfc_str(&mut contents, "FLVPlaybackClip");
    u16b(&mut contents, 1); // media_id = 1
    mfc_str(&mut contents, "");
    u32b(&mut contents, 0); // rev >= 3
    u32b(&mut contents, 3); // library_metadata rev 3
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0); // rev >= 5
    // component() rev 0:
    contents.push(0);
    u32b(&mut contents, 0);
    mfc_str(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    mfc_str(&mut contents, "");
    // primary_swf = smart_movie rev 1:
    contents.push(1);
    mfc_str(&mut contents, "clip.swf");
    u32b(&mut contents, swf.len() as u32);
    contents.extend_from_slice(&swf);
    // 2 empty smart_movies (rev 0):
    contents.push(0);
    contents.push(0);
    // CSwfPage subtype rev 2:
    contents.push(2);
    mfc_str(&mut contents, "FLVPlayback");
    for w in [2000_i32, 0, 2000, 0] {
        i32b(&mut contents, w);
    }
    u16b(&mut contents, 0); // end media
    push_contents_settings_v3f(&mut contents, 2);

    // Page 1 places TWO CPicSwf instances referencing `media_id = 1`!
    let mut page = StreamWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    push_cpic_swf_rev4_with_props(&mut page, 1, "video_mc_1", 0, true, 0.75);
    push_cpic_swf_rev4_with_props(&mut page, 1, "video_mc_2", 4000, false, 0.25);
    page.base_end();
    page.empty_shape();
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    page.base_end();
    page.layer_tail_rev2("Players");
    page.base_end();
    page.page_tail();

    let fla_path = tmp.0.join("two_cpicswf_instances.fla");
    write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Media 1", &swf),
        ],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_instance_settings"),
        "2nd+ CPicSwf placement of same AVM1 compiled clip must retain component props without fla_instance_settings: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).unwrap();
    proj.validate().unwrap();
    let elems = &proj.symbol(&proj.root).unwrap().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elems.len(), 2);
    assert_eq!(elems[0].name, "video_mc_1");
    assert_eq!(elems[1].name, "video_mc_2");
    assert_eq!(elems[0].scripts.len(), 1);
    assert_eq!(elems[1].scripts.len(), 1);
    assert!(
        elems[0].scripts[0]
            .script
            .source
            .contains("this.autoPlay = true;")
    );
    assert!(
        elems[0].scripts[0]
            .script
            .source
            .contains("this.volume = 0.75;")
    );
    assert!(
        elems[1].scripts[0]
            .script
            .source
            .contains("this.autoPlay = false;")
    );
    assert!(
        elems[1].scripts[0]
            .script
            .source
            .contains("this.volume = 0.25;")
    );
}

#[test]
#[ignore = "reads local unversioned NEOFLASH_LEGACY_SAMPLE_DIR corpus when present"]
fn private_corpus_embedded_sources_reconstruct_byte_exact_sha256_and_external_records_metadata() {
    let dir = std::env::var_os("NEOFLASH_LEGACY_SAMPLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/old flash files")
        });
    assert!(dir.is_dir(), "missing sample directory {}", dir.display());

    fn walk(d: &std::path::Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = fs::read_dir(d)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fla")) {
                out.push(p);
            }
        }
    }

    let mut files = Vec::new();
    walk(&dir, &mut files);
    let mut imported = 0usize;
    let mut embedded = 0usize;
    let mut external = 0usize;

    for path in &files {
        let Ok(rep) = inspect(path) else {
            continue;
        };
        if !rep.can_import {
            continue;
        }
        imported += 1;
        let orig = fs::read(path).unwrap();
        let orig_sha = sha256_hex(&orig);
        let proj = import_project(path).unwrap();
        proj.validate().unwrap();
        let prov = read_provenance_json(&proj);
        match reconstruct_legacy_fla_bytes(&proj).expect("reconstruct must not fail") {
            Some(rebuilt) => {
                assert_eq!(sha256_hex(&rebuilt), orig_sha);
                assert_eq!(rebuilt.len(), orig.len());
                embedded += 1;
            }
            None => {
                assert_eq!(prov["source_archive"]["encoding"], "external");
                assert_eq!(prov["source_sha256"], orig_sha);
                assert_eq!(prov["source_byte_len"], orig.len());
                external += 1;
            }
        }
    }
    assert!(
        imported >= 105 && embedded >= 103 && external <= 2,
        "unexpected corpus archival counts: imported={imported}, embedded={embedded}, external={external}"
    );
}
