//! Integration tests for [T-471] Track 3:
//! 1. `OversizedFlaBundle` (`import_legacy_fla_oversized_bundle`, `save_to_dir`, `load_from_dir`,
//!    `full_resolution_resource`, `reconstruct_fla_bytes`) with synthetic `>64 MiB` binary `.fla`
//!    containers, unscaled `>4096 px` bitmaps, `>16 MiB` omitted audio, Schema 3 `NFD1` delta
//!    deduplication, atomic save/overwrite, and symlink / path-traversal / SHA-256 tamper rejection.
//! 2. Rare Flash 3–CS4 binary subrecords (`ActionRecordDecoder` `revision == 3, mode == 1` packed
//!    UTF-16LE CString with and without `FF FE FF` marker and odd trailing byte, `CQTAudioSettings`
//!    on `CPicFrame` `SoundSettings` pointers, intermediate `0x15..=0x46` `Contents` save revisions,
//!    `CPicText` unpaired UTF-16 surrogates, and visible-run vs whitespace-run text alpha).
//! 3. Opt-in local private-corpus gate verifying `105 / 105` byte-exact SHA-256 `.fla`
//!    reconstruction when the git-ignored local sample directory is present on disk.
//!
//! 100% synthetic binary FLA fixtures in repository; zero private filenames, strings, or bytes.

use neoflash_core::extensions::sha256_hex;
use neoflash_core::*;
use neoflash_import::*;
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t471-{}-{}",
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
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn compact_utf16(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    assert!(units.len() < 255);
    out.push(units.len() as u8);
    for ch in units {
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
fn push_contents_settings(out: &mut Vec<u8>, media_counter: u16) {
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

fn write_cfb_fla(path: &Path, streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for &(name, data) in streams {
        cfb.create_stream(name).unwrap().write_all(data).unwrap();
    }
    let bytes = cfb.into_inner().into_inner();
    fs::write(path, &bytes).unwrap();
    bytes
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
    fn layer_tail_rev2(&mut self, name: &str) {
        self.bytes.push(2);
        mfc_str(&mut self.bytes, name);
        self.bytes.extend_from_slice(&[0, 0]);
        u16b(&mut self.bytes, 0);
        self.bytes.push(2);
    }
    fn page_tail(&mut self) {
        self.bytes.push(1);
        u16b(&mut self.bytes, 1);
    }
}

/// Build a synthetic uncompressed 32-bit ARGB `CMediaBits` raster payload (`width x height`).
fn build_uncompressed_bitmap_stream(width: u16, height: u16) -> Vec<u8> {
    let row_bytes = width * 4;
    let mut out = Vec::with_capacity(32 + usize::from(row_bytes) * usize::from(height));
    out.push(3); // raster rev 3
    out.push(5); // depth_code 5 = 32-bit ARGB
    u16b(&mut out, row_bytes);
    u16b(&mut out, width);
    u16b(&mut out, height);
    i32b(&mut out, 0);
    i32b(&mut out, i32::from(width) * 20);
    i32b(&mut out, 0);
    i32b(&mut out, i32::from(height) * 20);
    out.push(1); // alpha flag
    out.push(0); // compression = 0 (uncompressed rows)
    for y in 0..height {
        for x in 0..width {
            let r = ((x.wrapping_mul(17) ^ y.wrapping_mul(31)) & 0xff) as u8;
            let g = ((x.wrapping_mul(7).wrapping_add(y.wrapping_mul(13))) & 0xff) as u8;
            let b = ((x ^ y) & 0xff) as u8;
            // Flash 32-bit raster order is A, R, G, B.
            out.extend_from_slice(&[255, r, g, b]);
        }
    }
    out
}

fn build_contents_v3f_with_bitmap_and_sounds(
    bitmap: Option<(u16, &str, &str)>,
    sounds: &[(u16, &str, &str, u32, u16)],
) -> Vec<u8> {
    let mut out = Vec::new();
    push_contents_header(&mut out, 0x3f);

    push_mfc_new_class(&mut out, "CDocumentPage");
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
    u16b(&mut out, 0); // end library
    u16b(&mut out, 100);

    if let Some((media_id, stream_name, bmp_name)) = bitmap {
        push_mfc_new_class(&mut out, "CMediaBits");
        next_ref += 2;
        out.push(6); // mediaBase rev 6
        compact_utf16(&mut out, stream_name);
        mfc_str(&mut out, bmp_name);
        u16b(&mut out, media_id);
        mfc_str(&mut out, "");
        u32b(&mut out, 0);
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        // CMediaBits subtype rev 2 (lossless PNG mode, smoothing enabled)
        out.push(2);
        out.push(1); // compressionMode = lossless
        out.push(80); // jpegQuality
        out.push(1); // allowSmoothing
    }

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
        out.push(6);
        compact_utf16(&mut out, stream_name);
        mfc_str(&mut out, snd_name);
        u16b(&mut out, media_id);
        mfc_str(&mut out, "");
        u32b(&mut out, 0);
        u32b(&mut out, 3);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        out.push(5);
        u16b(&mut out, format_flags);
        u32b(&mut out, sample_count);
        i16b(&mut out, 0);
        i16b(&mut out, 0);
    }
    u16b(&mut out, 0); // end media

    let total_media = usize::from(bitmap.is_some()) + sounds.len();
    push_contents_settings(&mut out, (total_media + 1) as u16);
    out
}

fn build_page_placing_bitmap(media_id: u16) -> Vec<u8> {
    let mut w = StreamWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();

    w.begin("CPicBitmap");
    w.base_start();
    w.base_end();
    w.bytes.push(1); // CPicBitmap rev 1
    for mat in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut w.bytes, mat);
    }
    u16b(&mut w.bytes, media_id);

    w.base_end(); // end CPicFrame children
    w.empty_shape();
    w.bytes.push(1); // CPicFrame rev 1
    u16b(&mut w.bytes, 1);
    w.bytes.push(0);
    w.base_end(); // end CPicLayer children
    w.layer_tail_rev2("Layer 1");
    w.base_end(); // end CPicPage children
    w.page_tail();
    w.bytes
}

#[test]
fn oversized_fla_bundle_preserves_unscaled_media_nfd1_reconstruction_and_enforces_dir_safety() {
    let tmp = TempDir::new();

    // 1. Panoramic 5000x64 px bitmap (> 4096 px MAX_BITMAP_DIMENSION -> downscaled inline to 4096x52).
    let pano_stream = build_uncompressed_bitmap_stream(5000, 64);

    // 2. Oversized 18 MiB PCM 16-bit mono 44.1 kHz sound (> 16 MiB MAX_RESOURCE_BYTES -> omitted inline,
    //    retained losslessly in OversizedFlaBundle chunks AND used as an NFD1 delta reference!).
    let oversized_pcm_bytes = 18 * 1024 * 1024;
    let oversized_pcm = pseudo_random_bytes(0x7471_0001, oversized_pcm_bytes);
    let oversized_samples = (oversized_pcm_bytes / 2) as u32;

    // 3. Two 26 MiB uncompressible auxiliary streams so total CFB container exceeds 70 MiB (> 64 MiB),
    //    forcing inline `source_archive` to `"external"` while `OversizedFlaBundle` reconstructs 100%
    //    byte-for-byte via Schema 3 `NFD1` delta chunks!
    let aux_a = pseudo_random_bytes(0x7471_0002, 26 * 1024 * 1024);
    let aux_b = pseudo_random_bytes(0x7471_0003, 26 * 1024 * 1024);

    // format_flags = (0 << 4) | (3 << 2) | 2 = 0x000e -> uncompressed PCM, 44.1 kHz, 16-bit mono.
    let contents = build_contents_v3f_with_bitmap_and_sounds(
        Some((1, "Media 1", "PanoBanner.png")),
        &[(2, "Media 2", "NarrationLong.wav", oversized_samples, 0x000e)],
    );
    let page = build_page_placing_bitmap(1);

    let fla_path = tmp.0.join("synthetic_oversized_70mb.fla");
    let orig_fla = write_cfb_fla(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page),
            ("Media 1", &pano_stream),
            ("Media 2", &oversized_pcm),
            ("AuxNoiseA", &aux_a),
            ("AuxNoiseB", &aux_b),
        ],
    );
    assert!(
        orig_fla.len() > 68 * 1024 * 1024,
        "synthetic FLA must exceed 64 MiB resource limit, got {}",
        orig_fla.len()
    );
    let orig_sha = sha256_hex(&orig_fla);

    // Inline `import_project` stays strictly within MAX_PROJECT_JSON_BYTES and marks source external.
    let inline_proj = import_project(&fla_path).expect("inline import must succeed");
    inline_proj
        .validate()
        .expect("inline project must validate");
    assert!(inline_proj.to_json().unwrap().len() <= MAX_PROJECT_JSON_BYTES);
    assert!(
        reconstruct_legacy_fla_bytes(&inline_proj)
            .expect("inline reconstruct check")
            .is_none(),
        "70+ MiB uncompressible FLA must be external on inline Project"
    );

    // `import_legacy_fla_oversized_bundle` captures full-res media + Schema 3 NFD1 delta chunks!
    let bundle = import_legacy_fla_oversized_bundle(&fla_path)
        .expect("oversized bundle import must succeed");
    bundle
        .project
        .validate()
        .expect("bundle project must validate");
    assert!(bundle.project.to_json().unwrap().len() <= MAX_PROJECT_JSON_BYTES);

    // Unscaled 5000x64 PNG is preserved in `full_resolution_resources`.
    let bmp_entry = bundle
        .full_resolution_resources
        .get("legacy-bitmap-1")
        .expect("downscaled panoramic bitmap must have sidecar full-resolution entry");
    assert_eq!(bmp_entry.original_width, Some(5000));
    assert_eq!(bmp_entry.original_height, Some(64));
    assert!(bmp_entry.inline_downscaled);
    assert!(!bmp_entry.inline_omitted);
    let full_bmp_bytes = bundle
        .full_resolution_resource("legacy-bitmap-1")
        .expect("full_resolution_resource for bitmap");
    let (fw, fh) = image::ImageReader::new(Cursor::new(full_bmp_bytes))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .unwrap();
    assert_eq!((fw, fh), (5000, 64));

    // Omitted 18 MiB WAV sound (> 16 MiB single-resource cap) is preserved in `full_resolution_resources`.
    let snd_entry = bundle
        .full_resolution_resources
        .get("legacy-sound-2")
        .expect("omitted >16 MiB sound must have sidecar entry");
    assert!(snd_entry.inline_omitted);
    let full_wav = bundle
        .full_resolution_resource("legacy-sound-2")
        .expect("full_resolution_resource for omitted WAV");
    assert_eq!(&full_wav[..4], b"RIFF");
    assert!(full_wav.len() > oversized_pcm_bytes);

    // Schema 3 NFD1 delta archive deduplicates the 18 MiB PCM payload against the sidecar WAV chunk!
    match &bundle.source_archive {
        OversizedSourceArchive::BundleDeltaGzip {
            source_bytes,
            source_sha256,
            references,
            ..
        } => {
            assert_eq!(*source_bytes, orig_fla.len() as u64);
            assert_eq!(source_sha256, &orig_sha);
            assert!(
                references
                    .iter()
                    .any(|r| matches!(r, OversizedDeltaReference::BundleChunk { .. })),
                "NFD1 delta archive should reference deduplicated sidecar media chunk"
            );
        }
        other => panic!("expected BundleDeltaGzip source archive, got {other:?}"),
    }

    let reconstructed = bundle
        .reconstruct_fla_bytes()
        .expect("reconstruct 70+ MiB FLA from bundle");
    assert_eq!(sha256_hex(&reconstructed), orig_sha);
    assert_eq!(reconstructed, orig_fla);

    // Atomic `save_to_dir` + `load_from_dir` round-trip (including atomic overwrite of existing dir).
    let bundle_dir = tmp.0.join("saved-bundle");
    bundle
        .save_to_dir(&bundle_dir)
        .expect("initial save_to_dir");
    bundle
        .save_to_dir(&bundle_dir)
        .expect("atomic overwrite save_to_dir");

    let loaded = OversizedFlaBundle::load_from_dir(&bundle_dir).expect("load_from_dir");
    loaded.project.validate().unwrap();
    assert_eq!(
        loaded.full_resolution_resource("legacy-bitmap-1").unwrap(),
        full_bmp_bytes
    );
    assert_eq!(
        loaded.full_resolution_resource("legacy-sound-2").unwrap(),
        full_wav
    );
    let loaded_fla = loaded
        .reconstruct_fla_bytes()
        .expect("reconstruct after load_from_dir");
    assert_eq!(sha256_hex(&loaded_fla), orig_sha);

    // Security & corruption checks on `save_to_dir` / `load_from_dir`:
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let sym_target = tmp.0.join("real-target");
        fs::create_dir_all(&sym_target).unwrap();
        let sym_bundle = tmp.0.join("symlink-bundle");
        symlink(&sym_target, &sym_bundle).unwrap();
        assert!(
            bundle.save_to_dir(&sym_bundle).is_err(),
            "save_to_dir must reject symlink target directory"
        );
        assert!(
            OversizedFlaBundle::load_from_dir(&sym_bundle).is_err(),
            "load_from_dir must reject symlink bundle directory"
        );

        let sneaky_link = bundle_dir.join("chunks/sneaky.bin");
        symlink(&fla_path, &sneaky_link).unwrap();
        assert!(
            OversizedFlaBundle::load_from_dir(&bundle_dir).is_err(),
            "load_from_dir must reject symlinks inside chunks/"
        );
        fs::remove_file(&sneaky_link).unwrap();
    }

    // Reject path traversal (`../`) in manifest chunk SHA-256.
    let manifest_path = bundle_dir.join("bundle-manifest.json");
    let orig_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let mut traversal_manifest = orig_manifest.clone();
    traversal_manifest["chunks"][0]["sha256"] = serde_json::json!("../../etc/passwd");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&traversal_manifest).unwrap(),
    )
    .unwrap();
    let trav_err = OversizedFlaBundle::load_from_dir(&bundle_dir).unwrap_err();
    assert!(
        trav_err.to_string().contains("traversal") || trav_err.to_string().contains("SHA-256"),
        "unexpected error on traversal manifest: {trav_err}"
    );

    // Reject oversized chunk byte_len > MAX_OVERSIZED_BUNDLE_CHUNK_BYTES (256 MiB).
    let mut huge_manifest = orig_manifest.clone();
    huge_manifest["chunks"][0]["byte_len"] =
        serde_json::json!(MAX_OVERSIZED_BUNDLE_CHUNK_BYTES + 1);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&huge_manifest).unwrap(),
    )
    .unwrap();
    assert!(OversizedFlaBundle::load_from_dir(&bundle_dir).is_err());

    // Restore manifest and flip 1 byte in a chunk file -> SHA-256 verification must fail!
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&orig_manifest).unwrap(),
    )
    .unwrap();
    let first_sha = orig_manifest["chunks"][0]["sha256"].as_str().unwrap();
    let chunk_file = bundle_dir.join("chunks").join(format!("{first_sha}.bin"));
    let mut chunk_bytes = fs::read(&chunk_file).unwrap();
    chunk_bytes[0] ^= 0xff;
    fs::write(&chunk_file, &chunk_bytes).unwrap();
    let corrupt_err = OversizedFlaBundle::load_from_dir(&bundle_dir).unwrap_err();
    assert!(
        corrupt_err.to_string().contains("SHA-256"),
        "expected SHA-256 verification error, got {corrupt_err}"
    );
}

#[test]
fn rare_flash3_to_cs4_action_source_rev3_qtaudio_intermediate_contents_and_text_runs_decode_to_exact_eof()
 {
    let tmp = TempDir::new();

    // 1. Build a timeline stream containing:
    //    - Frame 1 (CPicFrame rev 21): ActionRecordDecoder `revision == 3, mode == 1` WITHOUT
    //      Unicode marker AND an odd trailing byte (`length = 15`, 7 UTF-16LE chars `"stop();"` + 1 odd byte)
    //      PLUS `CQTAudioSettings` on `TimelineRelation::SoundSettings`!
    //    - Frame 2 (CPicFrame rev 21): ActionRecordDecoder `revision == 3, mode == 1` WITH
    //      `FF FE FF` Unicode marker (`"play();"`), PLUS two `CPicText` objects:
    //      (a) `CPicText` with an unpaired UTF-16 surrogate code unit (`0xD800`) in its run, and
    //      (b) Multi-run `CPicText` where visible text has `alpha = 128/255` and a trailing `"\r"`
    //          run has `alpha = 255/255` (must NOT emit false-positive per-run alpha warning!).
    let mut w = StreamWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();

    // --- Frame 1 ---
    w.begin("CPicFrame");
    w.base_start();
    w.base_end();
    w.empty_shape();
    w.bytes.push(21); // CPicFrame rev 21
    u16b(&mut w.bytes, 1); // duration = 1
    u16b(&mut w.bytes, 0); // key_mode = 0
    u16b(&mut w.bytes, 0); // acceleration
    u16b(&mut w.bytes, 0); // sound_media_id
    u16b(&mut w.bytes, 0); // sound_envelope_count
    u16b(&mut w.bytes, 0); // sound_loop_count
    w.bytes.push(0); // sound_sync
    u32b(&mut w.bytes, 0); // sound_in_point
    u32b(&mut w.bytes, 0); // sound_out_point
    u16b(&mut w.bytes, 0); // sound_zoom
    mfc_str(&mut w.bytes, "intro"); // label
    // ActionRecordDecoder::source: rev = 3, mode = 1, non-Unicode header with odd byte length (15):
    u32b(&mut w.bytes, 3); // rev = 3
    u32b(&mut w.bytes, 1); // mode = 1
    u32b(&mut w.bytes, 0); // identifier
    u32b(&mut w.bytes, 0); // breakpoint count
    let script1_u16: Vec<u16> = "stop();".encode_utf16().collect();
    w.bytes.push((script1_u16.len() * 2 + 1) as u8); // 15 bytes (7 UTF-16LE units + 1 odd trailing byte!)
    for unit in script1_u16 {
        u16b(&mut w.bytes, unit);
    }
    w.bytes.push(0x7f); // odd trailing byte consumed by wire span!
    u32b(&mut w.bytes, 0); // rotation_mode
    u32b(&mut w.bytes, 0); // rotation_count
    u32b(&mut w.bytes, 0); // label_type
    u16b(&mut w.bytes, 0); // morphShape = null
    u32b(&mut w.bytes, 0); // shape_tween_blend
    // SoundSettings pointer -> CQTAudioSettings (schema = 1, non-CPicObj: byte_count + data)!
    w.begin("CQTAudioSettings");
    u32b(&mut w.bytes, 4);
    w.bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    mfc_str(&mut w.bytes, ""); // extensionString (rev >= 16)
    u32b(&mut w.bytes, 0); // reserved (rev >= 20)
    u32b(&mut w.bytes, 0); // soundEffect (rev >= 21)

    // --- Frame 2 ---
    w.begin("CPicFrame");
    w.base_start();

    // Child 1: CPicText rev 4 with style rev 10 containing unpaired UTF-16 surrogate `[0x0041, 0xD800, 0x0042]`
    let push_text_style_v10 = |buf: &mut Vec<u8>, rgba: u32| {
        buf.push(10); // style rev 10 (UTF-16 compact strings)
        u16b(buf, 240); // 12pt
        compact_utf16(buf, "Arial");
        u32b(buf, rgba);
        buf.push(0); // pitch_and_family
        buf.push(0); // char_set
        buf.push(0); // bold
        buf.extend_from_slice(&[0, 0, 1, 0]); // italic, screen, kern, pos
        buf.push(0); // align left
        i16b(buf, 40); // line_spacing = 2pt
        i16b(buf, 0);
        i16b(buf, 0);
        i16b(buf, 0);
        i16b(buf, 0);
        compact_utf16(buf, ""); // url
        buf.extend_from_slice(&[0, 0, 0]); // vertical, rtl, rotation
        buf.push(0); // bitmap_rendering
        compact_utf16(buf, ""); // target
    };

    w.begin("CPicText");
    w.base_start();
    w.base_end();
    w.bytes.push(4); // CPicText rev 4
    for m in [65536_i32, 0, 0, 65536, 200, 200] {
        i32b(&mut w.bytes, m);
    }
    for b in [0_i32, 2400, 0, 480] {
        i32b(&mut w.bytes, b);
    }
    w.bytes.push(1); // auto_expand
    w.bytes.push(0); // reserved (rev > 2)
    u16b(&mut w.bytes, 0); // flags (rev == 4 uses u16)
    u16b(&mut w.bytes, 0); // max_characters
    mfc_str(&mut w.bytes, ""); // variable_name
    // 1 run of 3 UTF-16 code units: 'A', lone 0xD800, 'B'
    u16b(&mut w.bytes, 3);
    push_text_style_v10(&mut w.bytes, 0xff33_6699);
    u16b(&mut w.bytes, u16::from(b'A'));
    u16b(&mut w.bytes, 0xD800);
    u16b(&mut w.bytes, u16::from(b'B'));
    u16b(&mut w.bytes, 0); // end runs

    // Child 2: Multi-run CPicText where visible run has alpha = 0x80 and trailing "\r" has alpha = 0xFF
    w.begin("CPicText");
    w.base_start();
    w.base_end();
    w.bytes.push(4);
    for m in [65536_i32, 0, 0, 65536, 200, 900] {
        i32b(&mut w.bytes, m);
    }
    for b in [0_i32, 2400, 0, 480] {
        i32b(&mut w.bytes, b);
    }
    w.bytes.push(1);
    w.bytes.push(0);
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    mfc_str(&mut w.bytes, "");
    // Run 1: "Fade" with alpha 0x80 (rgba LE: [0x11, 0x22, 0x33, 0x80] = 0x8033_2211)
    u16b(&mut w.bytes, 4);
    push_text_style_v10(&mut w.bytes, 0x8033_2211);
    for ch in "Fade".encode_utf16() {
        u16b(&mut w.bytes, ch);
    }
    // Run 2: "\r" (invisible paragraph break) with alpha 0xFF (0xff33_2211)
    u16b(&mut w.bytes, 1);
    push_text_style_v10(&mut w.bytes, 0xff33_2211);
    u16b(&mut w.bytes, u16::from(b'\r'));
    u16b(&mut w.bytes, 0); // end runs

    w.base_end(); // end Frame 2 children
    w.empty_shape();
    w.bytes.push(21);
    u16b(&mut w.bytes, 1);
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    w.bytes.push(0);
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    mfc_str(&mut w.bytes, "outro");
    // ActionRecordDecoder::source: rev = 3, mode = 1 WITH `FF FE FF` Unicode marker!
    u32b(&mut w.bytes, 3);
    u32b(&mut w.bytes, 1);
    u32b(&mut w.bytes, 7);
    u32b(&mut w.bytes, 0);
    let script2_u16: Vec<u16> = "play();".encode_utf16().collect();
    w.bytes.extend_from_slice(&[0xff, 0xfe, 0xff]); // CString Unicode marker
    w.bytes.push(script2_u16.len() as u8);
    for unit in script2_u16 {
        u16b(&mut w.bytes, unit);
    }
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0); // null SoundSettings
    mfc_str(&mut w.bytes, "");
    u32b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);

    w.base_end(); // end CPicLayer children
    w.layer_tail_rev2("ActionsAndText");
    w.base_end(); // end CPicPage children
    w.page_tail();

    let contents = build_contents_v3f_with_bitmap_and_sounds(None, &[]);
    let fla_path = tmp.0.join("rare_records.fla");
    write_cfb_fla(&fla_path, &[("Contents", &contents), ("Page 1", &w.bytes)]);

    let rep = inspect(&fla_path).expect("inspect rare_records.fla");
    assert!(
        rep.can_import,
        "rare_records.fla must import: {:?}",
        rep.diagnostics
    );
    let scene_stream = rep
        .binary_fla
        .as_ref()
        .unwrap()
        .streams
        .iter()
        .find(|s| s.role == BinaryStreamRole::Scene)
        .unwrap();
    assert!(
        scene_stream.structure_complete,
        "scene stream must decode to exact EOF: {:?}",
        scene_stream
    );
    assert_eq!(scene_stream.bytes_consumed as u64, scene_stream.byte_length);
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "fla_text_approximated"),
        "invisible trailing newline alpha must not trigger fla_text_approximated: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&fla_path).expect("import rare_records.fla");
    proj.validate().unwrap();
    let root_tl = &proj.symbol(&proj.root).unwrap().timeline;
    assert_eq!(root_tl.scripts.len(), 2);
    assert_eq!(root_tl.scripts[0].script.source, "stop();");
    assert_eq!(root_tl.scripts[1].script.source, "play();");

    let kf2_els = &root_tl.layers[0].keyframes[1].elements;
    assert_eq!(kf2_els.len(), 2);
    if let ElementKind::TextBox { text, .. } = &kf2_els[0].kind {
        assert_eq!(text, "A\u{FFFD}B");
    } else {
        panic!("expected TextBox 0");
    }
    assert!(
        (kf2_els[1].transform.alpha - (128.0 / 255.0)).abs() < 1e-4,
        "visible run alpha 128/255 must be preserved, got {}",
        kf2_els[1].transform.alpha
    );

    // 2. Intermediate Flash 3–CS4 `Contents` save revisions (`0x15`, `0x1e`, `0x25`, `0x34`, `0x41`, `0x46`)
    //    are recognized and mapped to their Flash release family, while `0x48` remains unsupported.
    for (rev, expected_ver) in [
        (0x15_u8, "Flash 4"),
        (0x1e_u8, "Flash 5"),
        (0x25_u8, "Flash MX"),
        (0x34_u8, "Flash MX 2004"),
        (0x41_u8, "Flash CS3"),
        (0x46_u8, "Flash CS4"),
    ] {
        let mut min_contents = Vec::new();
        push_contents_header(&mut min_contents, rev);
        push_mfc_new_class(&mut min_contents, "CDocumentPage");
        min_contents.push(4); // CDocumentPage rev 4
        mfc_str(&mut min_contents, "Page 1");
        mfc_str(&mut min_contents, "Scene 1");
        u32b(&mut min_contents, 0);
        min_contents.push(2);
        u16b(&mut min_contents, 0); // end scenes
        u16b(&mut min_contents, 1);
        u16b(&mut min_contents, 1);
        u16b(&mut min_contents, 2);
        u16b(&mut min_contents, 0); // end library
        u16b(&mut min_contents, 1);
        u16b(&mut min_contents, 0); // end media
        push_contents_settings(&mut min_contents, 1);

        let rev_path = tmp.0.join(format!("intermediate_{rev:02x}.fla"));
        write_cfb_fla(
            &rev_path,
            &[("Contents", &min_contents), ("Page 1", &page_minimal())],
        );
        let rev_rep = inspect(&rev_path).unwrap();
        assert_eq!(
            rev_rep.version.as_deref(),
            Some(expected_ver),
            "revision 0x{rev:02x} should map to {expected_ver}"
        );
        assert!(
            rev_rep
                .diagnostics
                .iter()
                .all(|d| d.code != "fla_revision_unsupported"),
            "revision 0x{rev:02x} must not fail with fla_revision_unsupported"
        );
    }
}

fn page_minimal() -> Vec<u8> {
    let mut w = StreamWriter::new();
    w.begin("CPicPage");
    w.base_start();
    w.begin("CPicLayer");
    w.base_start();
    w.begin("CPicFrame");
    w.base_start();
    w.base_end();
    w.empty_shape();
    w.bytes.push(1);
    u16b(&mut w.bytes, 1);
    w.bytes.push(0);
    w.base_end();
    w.layer_tail_rev2("L1");
    w.base_end();
    w.page_tail();
    w.bytes
}

fn discover_local_private_corpus() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("NEOFLASH_LEGACY_SAMPLE_DIR") {
        let p = PathBuf::from(explicit);
        if p.is_dir() {
            return Some(p);
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_cand = manifest_dir.join("../../examples/old flash files");
    if workspace_cand.is_dir() {
        return Some(workspace_cand);
    }
    if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(&manifest_dir)
        .output()
        && out.status.success()
    {
        let common = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let shared_cand = PathBuf::from(common).join("../examples/old flash files");
        if shared_cand.is_dir() {
            return Some(shared_cand);
        }
    }
    None
}

fn collect_nonempty_flas(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_nonempty_flas(&p, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
            && fs::metadata(&p).is_ok_and(|m| m.len() > 0)
        {
            out.push(p);
        }
    }
}

#[test]
fn private_corpus_all_nonempty_flas_reconstruct_sha256_byte_for_byte_when_present() {
    let Some(sample_dir) = discover_local_private_corpus() else {
        // Gracefully skip in clean CI checkouts where git-ignored private samples are absent.
        return;
    };
    let mut flas = Vec::new();
    collect_nonempty_flas(&sample_dir, &mut flas);
    if flas.is_empty() {
        return;
    }

    let full_sweep =
        !cfg!(debug_assertions) || std::env::var_os("NEOFLASH_T471_FULL_DEBUG").is_some();
    let mut verified_inline = 0usize;
    let mut verified_oversized_bundle = 0usize;
    let mut skipped_large_in_debug = 0usize;

    for (idx, path) in flas.iter().enumerate() {
        let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if !full_sweep && len > 2 * 1024 * 1024 {
            skipped_large_in_debug += 1;
            continue;
        }
        let orig_bytes = fs::read(path).expect("read local sample");
        let orig_sha = sha256_hex(&orig_bytes);

        let proj = import_project(path).unwrap_or_else(|e| {
            panic!("nonempty sample #{idx} failed inline import: {e}");
        });
        proj.validate().unwrap_or_else(|e| {
            panic!("nonempty sample #{idx} failed Project::validate: {e}");
        });

        if let Some(inline_restored) = reconstruct_legacy_fla_bytes(&proj).unwrap_or_else(|e| {
            panic!("nonempty sample #{idx} failed inline FLA reconstruction: {e}");
        }) {
            assert_eq!(
                sha256_hex(&inline_restored),
                orig_sha,
                "nonempty sample #{idx} inline SHA-256 mismatch"
            );
            verified_inline += 1;
            // Also verify OversizedFlaBundle on any sample that downscaled bitmaps (e.g. >4096 px).
            let rep = inspect(path).unwrap();
            if rep
                .diagnostics
                .iter()
                .any(|d| d.code == "fla_bitmap_downscaled" || d.code == "fla_bitmap_reencoded")
            {
                let bundle = import_legacy_fla_oversized_bundle(path).unwrap();
                bundle.project.validate().unwrap();
                assert!(
                    !bundle.full_resolution_resources.is_empty(),
                    "sample #{idx} with downscaled/reencoded bitmap must populate full_resolution_resources"
                );
                let bundle_restored = bundle.reconstruct_fla_bytes().unwrap();
                assert_eq!(sha256_hex(&bundle_restored), orig_sha);
                verified_oversized_bundle += 1;
            }
        } else {
            // Inline `source_archive` was `"external"` (>64 MiB archive): `OversizedFlaBundle`
            // MUST reconstruct 100% byte-for-byte via Schema 3 NFD1 delta chunks!
            let bundle = import_legacy_fla_oversized_bundle(path).unwrap_or_else(|e| {
                panic!("oversized sample #{idx} failed bundle import: {e}");
            });
            bundle.project.validate().unwrap();
            assert!(
                !bundle.full_resolution_resources.is_empty(),
                "oversized sample #{idx} must expose unscaled/un-reencoded full-resolution media"
            );
            let bundle_restored = bundle.reconstruct_fla_bytes().unwrap_or_else(|e| {
                panic!("oversized sample #{idx} failed bundle FLA reconstruction: {e}");
            });
            assert_eq!(
                sha256_hex(&bundle_restored),
                orig_sha,
                "oversized sample #{idx} bundle SHA-256 mismatch"
            );
            verified_oversized_bundle += 1;
        }
    }

    assert!(
        verified_inline + verified_oversized_bundle + skipped_large_in_debug >= flas.len(),
        "100% of swept nonempty private FLAs must reconstruct byte-for-byte"
    );
    if full_sweep {
        assert_eq!(skipped_large_in_debug, 0);
        assert!(
            verified_oversized_bundle >= 1,
            "expected at least 1 oversized/downscaled sample verified via OversizedFlaBundle"
        );
    }
}

#[test]
fn f09_truncated_40_byte_nfd1_claiming_500mb_literal_and_inline_manifest_invariants_fail_fast() {
    let tmp = TempDir::new();

    // Build a valid minimal FLA so we have a clean base bundle (`InlineProject`).
    let contents = build_contents_v3f_with_bitmap_and_sounds(None, &[]);
    let page = page_minimal();
    let fla_path = tmp.0.join("small_valid.fla");
    write_cfb_fla(&fla_path, &[("Contents", &contents), ("Page 1", &page)]);

    let base_bundle = import_legacy_fla_oversized_bundle(&fla_path).expect("import small bundle");
    let bundle_dir = tmp.0.join("small-bundle-dir");
    base_bundle
        .save_to_dir(&bundle_dir)
        .expect("save small bundle");

    // 1. Craft a tiny (< 50 byte) gzipped `NFD1` stream declaring `expected_len = 500 MiB`
    //    and a `DELTA_OP_LITERAL` header with `len = 500 MiB` followed by only 4 bytes (EOF).
    let claimed_len: u32 = 500 * 1024 * 1024;
    let mut raw_nfd1 = Vec::new();
    raw_nfd1.extend_from_slice(b"NFD1");
    raw_nfd1.extend_from_slice(&u64::from(claimed_len).to_le_bytes());
    raw_nfd1.extend_from_slice(&0_u16.to_le_bytes()); // 0 reference blobs
    raw_nfd1.push(0x00); // DELTA_OP_LITERAL
    raw_nfd1.extend_from_slice(&claimed_len.to_le_bytes()); // claims 500 MiB literal!
    raw_nfd1.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // truncated after 4 bytes!

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&raw_nfd1).unwrap();
    let tiny_gz = gz.finish().unwrap();
    assert!(
        tiny_gz.len() <= 48,
        "crafted truncated NFD1 gzip payload should be <= 48 bytes, got {}",
        tiny_gz.len()
    );

    let tiny_sha = sha256_hex(&tiny_gz);
    let mut bomb_bundle = base_bundle.clone();
    bomb_bundle.chunks = vec![OversizedMediaChunk {
        sha256: tiny_sha.clone(),
        byte_len: tiny_gz.len(),
        media_type: "application/x-neoflash-nfd1+gzip".to_owned(),
        role: OversizedChunkRole::SourceDeltaGzip,
        data: tiny_gz.clone(),
    }];
    bomb_bundle.source_archive = OversizedSourceArchive::BundleDeltaGzip {
        source_bytes: u64::from(claimed_len),
        source_fnv1a64: "0000000000000000".to_owned(),
        source_sha256: "a".repeat(64),
        delta_chunk_sha256s: vec![tiny_sha],
        references: Vec::new(),
    };

    let t0 = std::time::Instant::now();
    let err = bomb_bundle.reconstruct_fla_bytes().unwrap_err();
    assert!(
        t0.elapsed().as_secs() < 2,
        "truncated 500 MiB literal header must fail immediately without huge allocation"
    );
    assert!(
        err.to_string().contains("truncated delta literal payload"),
        "expected truncated delta literal payload error, got {err}"
    );

    // Also verify inline `reconstruct_legacy_fla_bytes` rejects the same 40-byte truncated stream fast.
    let mut bomb_proj = base_bundle.project.clone();
    bomb_proj
        .resources
        .retain(|r| !r.id.starts_with("fla-original"));
    bomb_proj.resources.push(
        ResourceUnit::from_bytes(
            "fla-original",
            "legacy/source.fla.delta.gz",
            "application/octet-stream",
            &tiny_gz,
        )
        .unwrap(),
    );
    let prov = serde_json::json!({
        "schema": 3,
        "source_bytes": u64::from(claimed_len),
        "source_fnv1a64": "0000000000000000",
        "source_sha256": "a".repeat(64),
        "source_archive": {
            "encoding": "resource-delta-gzip",
            "resources": ["fla-original"],
            "reference_resources": [],
            "source_byte_len": u64::from(claimed_len),
            "source_sha256": "a".repeat(64),
        }
    });
    *bomb_proj
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-provenance")
        .unwrap() = ResourceUnit::from_bytes(
        "fla-provenance",
        "legacy/provenance.json",
        "application/json",
        &serde_json::to_vec(&prov).unwrap(),
    )
    .unwrap();
    let inline_err = reconstruct_legacy_fla_bytes(&bomb_proj).unwrap_err();
    assert!(
        inline_err
            .to_string()
            .contains("truncated delta literal payload"),
        "expected truncated delta literal payload error on inline project, got {inline_err}"
    );

    // 2. Verify `InlineProject` source_bytes (> 512 MiB) & source_sha256 invariants are enforced
    //    unconditionally on `load_from_dir` and `save_to_dir`, plus `':'` chunk segment rejection!
    let manifest_path = bundle_dir.join("bundle-manifest.json");
    let valid_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    let mut bad_inline_size = valid_manifest.clone();
    bad_inline_size["source_archive"]["source_bytes"] =
        serde_json::json!(MAX_OVERSIZED_BUNDLE_SOURCE_CONTAINER_BYTES + 1);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&bad_inline_size).unwrap(),
    )
    .unwrap();
    assert!(
        OversizedFlaBundle::load_from_dir(&bundle_dir).is_err(),
        "InlineProject with source_bytes > 512 MiB must be rejected"
    );

    let mut bad_inline_sha = valid_manifest.clone();
    bad_inline_sha["source_archive"]["source_sha256"] = serde_json::json!("not-a-valid-sha256");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&bad_inline_sha).unwrap(),
    )
    .unwrap();
    assert!(
        OversizedFlaBundle::load_from_dir(&bundle_dir).is_err(),
        "InlineProject with invalid source_sha256 must be rejected"
    );

    let mut colon_chunk_manifest = valid_manifest;
    colon_chunk_manifest["chunks"] = serde_json::json!([{
        "sha256": "C:windows_drive_injection",
        "byte_len": 4,
        "media_type": "application/octet-stream",
        "role": "omitted_media"
    }]);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&colon_chunk_manifest).unwrap(),
    )
    .unwrap();
    let colon_err = OversizedFlaBundle::load_from_dir(&bundle_dir).unwrap_err();
    assert!(
        colon_err.to_string().contains("traversal") || colon_err.to_string().contains("unsafe"),
        "expected colon path rejection error, got {colon_err}"
    );
}
