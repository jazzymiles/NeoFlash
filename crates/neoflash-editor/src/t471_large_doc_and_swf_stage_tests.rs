//! `[T-471]` Track 4 verification:
//! 1. Self-contained large-document `.nflash` binary bundle (`NFLASHB1`, `80 MiB` resources
//!    including a `24 MiB` resource `> MAX_RESOURCE_BYTES`) round-trip across `neoflash-core`,
//!    `Editor` open/save/revert/reopen, and `--headless inspect`/`validate`, plus SHA-256 tamper
//!    and truncation rejection, ordinary UTF-8 JSON preservation, and `128 MiB + 1` non-bundle
//!    rejection.
//! 2. Deterministic `Editor` Stage SWF projection across `seek`, `step`, `restart`, `trace()`
//!    routing, `SwfAudioEvent` / `SoundCommand` (`Start`, `Stop`, `StreamStart`, `StreamStop`,
//!    `StopAll`) synchronization, and unreconstructible-SWF `SwfBytecodePlayer` Stage fallback
//!    preview leaving the active authored document and Undo history untouched.

use crate::app::Editor;
use crate::headless::{
    detect_headless_mode, load_project_lenient, load_project_valid, run_cli_with_io,
};
use eframe::egui;
use neoflash_core::{
    ElementKind, MAX_BUNDLE_RESOURCE_BYTES, MAX_BUNDLE_RESOURCE_TOTAL_BYTES, MAX_BUNDLE_RESOURCES,
    MAX_NFLASH_BUNDLE_BYTES, MAX_PROJECT_JSON_BYTES, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES,
    NFLASH_BUNDLE_MAGIC, Project, ResourceUnit, SoundCommand, SoundSync, is_nflash_bundle_bytes,
    new_id,
};
use serde_json::Value;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t471-{label}-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_headless(args: &[&str]) -> (i32, String, String) {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let detection = detect_headless_mode(&owned, None);
    let mut stdin = Cursor::new(b"".as_slice());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_cli_with_io(
        &owned,
        detection.explicit_headless,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8(stdout).expect("utf8 stdout"),
        String::from_utf8(stderr).expect("utf8 stderr"),
    )
}

fn swf_tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
    if payload.len() < 0x3f {
        let hdr = (code << 6) | (payload.len() as u16);
        out.extend_from_slice(&hdr.to_le_bytes());
    } else {
        let hdr = (code << 6) | 0x3f;
        out.extend_from_slice(&hdr.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn build_swf(version: u8, frame_count: u16, tags: &[u8]) -> Vec<u8> {
    // 400 x 300 px stage (0..8000, 0..6000 twips), 24 fps
    let rect = [0x78, 0x00, 0x03, 0xE8, 0x00, 0x00, 0x0B, 0xB8, 0x00];
    let mut body = Vec::new();
    body.extend_from_slice(&rect);
    body.extend_from_slice(&[0x00, 24]);
    body.extend_from_slice(&frame_count.to_le_bytes());
    body.extend_from_slice(tags);
    body.extend_from_slice(&0_u16.to_le_bytes());
    let file_len = (8 + body.len()) as u32;
    let mut out = vec![b'F', b'W', b'S', version];
    out.extend_from_slice(&file_len.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn minimal_define_shape_payload(shape_id: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(28);
    payload.extend_from_slice(&shape_id.to_le_bytes());
    payload.extend_from_slice(&[
        0x70, 0x00, 0x0C, 0x80, 0x00, 0x01, 0x90, 0x00, // 0..400, 0..400 twips
        0x01, // 1 fill style
        0x00, 0xE0, 0x40, 0x30, // solid RGB
        0x00, // 0 line styles
        0x10, // num_fill_bits=1, num_line_bits=0
        0x14, 0x25, 0x9F, 0x20, 0x32, 0x02, 0x58, 0xCF, 0xFF, 0x9C, 0xE6, 0x7F, 0x00,
    ]);
    payload
}

fn push_avm1_set_prop(buf: &mut Vec<u8>, target: &str, prop_idx: i32, val: i32) {
    buf.push(0x96); // ActionPush
    let mut p = Vec::new();
    p.push(0x00);
    p.extend_from_slice(target.as_bytes());
    p.push(0);
    p.push(0x07);
    p.extend_from_slice(&prop_idx.to_le_bytes());
    p.push(0x07);
    p.extend_from_slice(&val.to_le_bytes());
    buf.extend_from_slice(&(p.len() as u16).to_le_bytes());
    buf.extend_from_slice(&p);
    buf.push(0x23); // ActionSetProperty
}

fn push_avm1_trace(buf: &mut Vec<u8>, msg: &str) {
    buf.push(0x96); // ActionPush
    let mut p = Vec::new();
    p.push(0x00);
    p.extend_from_slice(msg.as_bytes());
    p.push(0);
    buf.extend_from_slice(&(p.len() as u16).to_le_bytes());
    buf.extend_from_slice(&p);
    buf.push(0x26); // ActionTrace
}

#[test]
fn t471_large_nflash_bundle_80mib_round_trips_editor_and_headless_while_preserving_inline_limits() {
    assert_eq!(MAX_RESOURCE_BYTES, 16 * 1024 * 1024);
    assert_eq!(MAX_RESOURCE_TOTAL_BYTES, 64 * 1024 * 1024);
    assert_eq!(MAX_PROJECT_JSON_BYTES, 128 * 1024 * 1024);
    assert_eq!(MAX_BUNDLE_RESOURCE_BYTES, 128 * 1024 * 1024);
    assert_eq!(MAX_BUNDLE_RESOURCE_TOTAL_BYTES, 512 * 1024 * 1024);
    assert_eq!(MAX_BUNDLE_RESOURCES, 4096);
    assert_eq!(MAX_NFLASH_BUNDLE_BYTES, 640 * 1024 * 1024);

    let tmp = TempDirGuard::new("large-bundle");

    // 1. Ordinary projects still emit plain UTF-8 JSON byte-for-byte identical to `to_json()`.
    let ordinary = Project::demo();
    let ordinary_bytes = ordinary
        .to_nflash_bytes()
        .expect("ordinary to_nflash_bytes");
    assert!(!is_nflash_bundle_bytes(&ordinary_bytes));
    assert_eq!(ordinary_bytes, ordinary.to_json().unwrap().into_bytes());
    assert_eq!(
        Project::from_nflash_bytes(&ordinary_bytes).unwrap(),
        ordinary
    );

    // 2. Non-bundle oversized file (`128 MiB + 1`) is STILL rejected by headless loaders.
    let oversized_raw_path = tmp.0.join("oversized_plain.nflash");
    {
        let f = fs::File::create(&oversized_raw_path).unwrap();
        f.set_len(MAX_PROJECT_JSON_BYTES as u64 + 1).unwrap();
    }
    for res in [
        load_project_lenient(&oversized_raw_path),
        load_project_valid(&oversized_raw_path),
    ] {
        let err = res.expect_err("128 MiB + 1 non-bundle input must fail");
        assert_eq!(err.exit_code, 1);
        assert_eq!(err.code, "resource_limit_exceeded");
    }

    // 3. Build a `large_bundle` Project with 80 MiB of binary resources, including a 24 MiB resource
    //    (> `MAX_RESOURCE_BYTES` = 16 MiB) and 80 MiB total (> `MAX_RESOURCE_TOTAL_BYTES` = 64 MiB).
    let make_patterned_blob = |mib: usize, seed: u8| -> Vec<u8> {
        let len = mib * 1024 * 1024;
        let mut buf = vec![seed; len];
        // Stamp deterministic header, midpoint, and trailer markers so truncation/swap is caught.
        buf[0..4].copy_from_slice(&[seed, 0xA5, 0x5A, (mib as u8)]);
        let mid = len / 2;
        buf[mid..mid + 4].copy_from_slice(&[0xDE, 0xAD, seed, 0xBE]);
        buf[len - 4..len].copy_from_slice(&[0xCA, 0xFE, (mib as u8), seed]);
        buf
    };
    let blob_24m = make_patterned_blob(24, 0x11);
    let blob_20m_a = make_patterned_blob(20, 0x22);
    let blob_20m_b = make_patterned_blob(20, 0x33);
    let blob_16m = make_patterned_blob(16, 0x44);

    // Standard `ResourceUnit::from_bytes` still rejects > 16 MiB!
    assert!(
        ResourceUnit::from_bytes(
            "too-big-inline",
            "media/chunk24.bin",
            "application/octet-stream",
            &blob_24m,
        )
        .is_err()
    );

    let mut large_project = Project::blank();
    large_project.name = "LargeBundle80MiB".into();
    large_project.resources.push(
        ResourceUnit::new_large(
            "res-24m",
            "media/video_24m.bin",
            "application/octet-stream",
            &blob_24m,
        )
        .expect("24 MiB resource via new_large"),
    );
    large_project.resources.push(
        ResourceUnit::new_large(
            "res-20m-a",
            "media/audio_20m_a.bin",
            "application/octet-stream",
            &blob_20m_a,
        )
        .unwrap(),
    );
    large_project.resources.push(
        ResourceUnit::new_large(
            "res-20m-b",
            "media/audio_20m_b.bin",
            "application/octet-stream",
            &blob_20m_b,
        )
        .unwrap(),
    );
    large_project.resources.push(
        ResourceUnit::new_large(
            "res-16m",
            "media/pack_16m.bin",
            "application/octet-stream",
            &blob_16m,
        )
        .unwrap(),
    );
    large_project.format_version = 3;

    // Without `large_bundle`, inline `validate()` still rejects 80 MiB / 24 MiB!
    assert!(
        large_project.validate().is_err(),
        "without large_bundle enabled, inline validate() must enforce 16 MiB / 64 MiB"
    );

    // Enabling `large_bundle` passes `project.validate()` while `to_json()` still enforces 128 MiB.
    large_project.set_large_bundle(true);
    assert!(large_project.is_large_bundle());
    large_project
        .validate()
        .expect("large_bundle project with 80 MiB resources must validate");

    // 4. Round-trip through `to_nflash_bytes()` / `from_nflash_bytes()` (`NFLASHB1`).
    let bundle_bytes = large_project
        .to_nflash_bytes()
        .expect("serialize 80 MiB project to NFLASHB1");
    assert!(bundle_bytes.starts_with(NFLASH_BUNDLE_MAGIC));
    assert!(
        bundle_bytes.len() < 82 * 1024 * 1024,
        "NFLASHB1 stores raw binary blobs without base64 bloat, got {} bytes",
        bundle_bytes.len()
    );

    let decoded = Project::from_nflash_bytes(&bundle_bytes).expect("decode 80 MiB NFLASHB1");
    assert_eq!(decoded, large_project);
    assert!(decoded.is_large_bundle());
    decoded.validate().expect("decoded bundle validates");
    assert_eq!(
        decoded
            .resources
            .iter()
            .find(|r| r.id == "res-24m")
            .unwrap()
            .decoded_bundle_bytes()
            .unwrap(),
        blob_24m
    );

    // 5. Reject corrupted SHA-256 and truncated `NFLASHB1` headers/sections cleanly.
    let mut tampered = bundle_bytes.clone();
    let flip_idx = tampered.len() - 17;
    tampered[flip_idx] ^= 0x80;
    let sha_err = Project::from_nflash_bytes(&tampered)
        .expect_err("corrupted blob byte must fail SHA-256 verification")
        .to_string();
    assert!(sha_err.contains("SHA-256 mismatch"), "got: {sha_err}");

    let tampered_path = tmp.0.join("corrupted.nflash");
    fs::write(&tampered_path, &tampered).unwrap();
    drop(tampered);

    let (bad_code, bad_out, _) = run_headless(&[
        "--headless",
        "validate",
        "--project",
        tampered_path.to_str().unwrap(),
    ]);
    assert_eq!(bad_code, 1);
    let bad_json: Value = serde_json::from_str(&bad_out).unwrap();
    assert_eq!(bad_json["ok"], false);
    assert_eq!(bad_json["error"]["code"], "invalid_bundle");

    for trunc_len in [4usize, 8, 20, 27, 128] {
        assert!(
            Project::from_nflash_bytes(&bundle_bytes[..trunc_len]).is_err(),
            "truncated NFLASHB1 ({trunc_len} bytes) must be rejected"
        );
    }
    let trunc_path = tmp.0.join("truncated_hdr.nflash");
    fs::write(&trunc_path, &bundle_bytes[..12]).unwrap();
    let (trunc_code, _, _) = run_headless(&[
        "--headless",
        "inspect",
        "--project",
        trunc_path.to_str().unwrap(),
    ]);
    assert_eq!(trunc_code, 1);

    // 6. Editor open / save / revert / reopen + `--headless inspect` and `--headless validate`!
    let bundle_path = tmp.0.join("large_scene.nflash");
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, large_project.clone());
    assert!(editor.save_to_path(bundle_path.clone()));
    assert!(!editor.dirty());
    let disk_head = {
        let mut f = fs::File::open(&bundle_path).unwrap();
        let mut hdr = [0u8; 8];
        std::io::Read::read_exact(&mut f, &mut hdr).unwrap();
        hdr
    };
    assert_eq!(&disk_head, NFLASH_BUNDLE_MAGIC);

    // Mutate in memory, verify `revert_to_saved` reloads the `NFLASHB1` bundle cleanly.
    editor.transact(|ed| {
        ed.project.name = "MutatedUnsaved".into();
    });
    assert!(editor.dirty());
    assert!(editor.revert_to_saved(&ctx));
    assert!(!editor.dirty());
    assert_eq!(editor.project.name, "LargeBundle80MiB");

    // Reopen in a fresh Editor instance.
    let mut reopened = Editor::from_project(&ctx, Project::blank());
    reopened.open_path(bundle_path.clone(), &ctx);
    assert!(!reopened.dirty());
    assert!(!reopened.history.can_undo());
    assert_eq!(reopened.project, large_project);

    // `--headless inspect` and `--headless validate` on the saved 80 MiB `.nflash` bundle.
    let (insp_code, insp_out, insp_err) = run_headless(&[
        "--headless",
        "inspect",
        "--project",
        bundle_path.to_str().unwrap(),
    ]);
    assert_eq!(insp_code, 0, "stderr: {insp_err}, stdout: {insp_out}");
    let insp_val: Value = serde_json::from_str(&insp_out).unwrap();
    assert_eq!(insp_val["ok"], true);
    assert_eq!(insp_val["metadata"]["name"], "LargeBundle80MiB");
    assert_eq!(insp_val["validation"]["valid"], true);
    assert!(
        insp_val["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "res-24m" && r["byte_len"] == 24 * 1024 * 1024)
    );

    let (val_code, val_out, val_err) = run_headless(&[
        "--headless",
        "validate",
        "--project",
        bundle_path.to_str().unwrap(),
    ]);
    assert_eq!(val_code, 0, "stderr: {val_err}, stdout: {val_out}");
    let val_json: Value = serde_json::from_str(&val_out).unwrap();
    assert_eq!(val_json["ok"], true);
    assert_eq!(val_json["valid"], true);
}

#[test]
fn t471_swf_stage_seek_step_restart_audio_sync_and_unreconstructible_fallback() {
    let tmp = TempDirGuard::new("swf-stage-fallback");
    let ctx = egui::Context::default();
    let shape1 = minimal_define_shape_payload(1);
    let translate_zero_matrix = [0x00_u8];

    // -----------------------------------------------------------------------
    // Part A: Interactive 4-frame AVM1 SWF with mask, runtime pose mutations,
    //         `trace()`, and all 5 audio command transitions (`Start`, `Stop`,
    //         `StreamStart`, `StreamStop`, `StopAll`).
    // -----------------------------------------------------------------------
    let mut avm1_tags = Vec::new();
    swf_tag(&mut avm1_tags, 2, &shape1);

    // DefineSound (id = 10): uncompressed LE 16-bit mono 22 kHz, 64 samples (128 zero bytes)
    let mut def_sound = Vec::new();
    def_sound.extend_from_slice(&10_u16.to_le_bytes());
    def_sound.push(0x3A); // format=3 (LE PCM), rate=2 (22kHz), 16-bit=1, stereo=0
    def_sound.extend_from_slice(&64_u32.to_le_bytes());
    def_sound.extend_from_slice(&[0u8; 128]);
    swf_tag(&mut avm1_tags, 14, &def_sound);

    // DefineSprite 2 ("hero" MovieClip placing shape 1)
    let mut sprite2 = Vec::new();
    sprite2.extend_from_slice(&2_u16.to_le_bytes());
    sprite2.extend_from_slice(&1_u16.to_le_bytes());
    let mut po2_inner = vec![0x06];
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&1_u16.to_le_bytes());
    po2_inner.extend_from_slice(&translate_zero_matrix);
    swf_tag(&mut sprite2, 26, &po2_inner);
    swf_tag(&mut sprite2, 1, &[]);
    swf_tag(&mut sprite2, 0, &[]);
    swf_tag(&mut avm1_tags, 39, &sprite2);

    // Frame 0: Mask at depth 1 (clip_depth=2), "hero" at depth 2, StartSound(10, Start),
    //          SoundStreamHead2 + SoundStreamBlock, hero._x = 60, trace("avm1-f0")
    let mut po2_mask = vec![0x46]; // HasClipDepth | HasMatrix | HasCharacter
    po2_mask.extend_from_slice(&1_u16.to_le_bytes());
    po2_mask.extend_from_slice(&1_u16.to_le_bytes());
    po2_mask.extend_from_slice(&translate_zero_matrix);
    po2_mask.extend_from_slice(&2_u16.to_le_bytes());
    swf_tag(&mut avm1_tags, 26, &po2_mask);

    let mut po2_hero = vec![0x26]; // HasName | HasMatrix | HasCharacter
    po2_hero.extend_from_slice(&2_u16.to_le_bytes());
    po2_hero.extend_from_slice(&2_u16.to_le_bytes());
    po2_hero.extend_from_slice(&translate_zero_matrix);
    po2_hero.extend_from_slice(b"hero\0");
    swf_tag(&mut avm1_tags, 26, &po2_hero);

    let mut start_snd = Vec::new();
    start_snd.extend_from_slice(&10_u16.to_le_bytes());
    start_snd.push(0x10); // sync_no_multiple -> SoundSync::Start
    swf_tag(&mut avm1_tags, 15, &start_snd);

    swf_tag(&mut avm1_tags, 45, &[0x0A, 0x3A, 64, 0]); // SoundStreamHead2 (seq 1)
    swf_tag(&mut avm1_tags, 19, &[0u8; 128]); // SoundStreamBlock frame 0

    let mut act0 = Vec::new();
    push_avm1_set_prop(&mut act0, "hero", 0, 60); // _x = 60
    push_avm1_set_prop(&mut act0, "hero", 6, 50); // _alpha = 50
    push_avm1_trace(&mut act0, "avm1-f0");
    act0.push(0x00);
    swf_tag(&mut avm1_tags, 12, &act0);
    swf_tag(&mut avm1_tags, 1, &[]); // ShowFrame 0

    // Frame 1: StartSound(10, Stop), consecutive SoundStreamBlock, hero._x = 120, hero._visible = 0
    let mut stop_snd = Vec::new();
    stop_snd.extend_from_slice(&10_u16.to_le_bytes());
    stop_snd.push(0x20); // sync_stop -> SoundSync::Stop
    swf_tag(&mut avm1_tags, 15, &stop_snd);
    swf_tag(&mut avm1_tags, 19, &[0u8; 128]);

    let mut act1 = Vec::new();
    push_avm1_set_prop(&mut act1, "hero", 0, 120);
    push_avm1_set_prop(&mut act1, "hero", 7, 0); // _visible = false
    push_avm1_trace(&mut act1, "avm1-f1");
    act1.push(0x00);
    swf_tag(&mut avm1_tags, 12, &act1);
    swf_tag(&mut avm1_tags, 1, &[]); // ShowFrame 1

    // Frame 2: New SoundStreamHead2 (triggers StreamStop on previous root stream + StreamStart seq 2),
    //          hero._visible = 1, hero._x = 190, hero._alpha = 100
    swf_tag(&mut avm1_tags, 45, &[0x0A, 0x3A, 64, 0]); // SoundStreamHead2 (seq 2)
    swf_tag(&mut avm1_tags, 19, &[0u8; 128]);
    let mut act2 = Vec::new();
    push_avm1_set_prop(&mut act2, "hero", 7, 1);
    push_avm1_set_prop(&mut act2, "hero", 0, 190);
    push_avm1_set_prop(&mut act2, "hero", 6, 100);
    push_avm1_trace(&mut act2, "avm1-f2");
    act2.push(0x00);
    swf_tag(&mut avm1_tags, 12, &act2);
    swf_tag(&mut avm1_tags, 1, &[]); // ShowFrame 2

    // Frame 3: ActionStopSounds (0x09 -> StopAll) + ActionStop (0x07) + trace("avm1-f3-halt")
    let mut act3 = Vec::new();
    act3.push(0x09); // ActionStopSounds
    act3.push(0x07); // ActionStop
    push_avm1_trace(&mut act3, "avm1-f3-halt");
    act3.push(0x00);
    swf_tag(&mut avm1_tags, 12, &act3);
    swf_tag(&mut avm1_tags, 1, &[]); // ShowFrame 3

    let avm1_path = tmp.0.join("interactive_audio_avm1.swf");
    fs::write(&avm1_path, build_swf(8, 4, &avm1_tags)).unwrap();

    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.open_path(avm1_path, &ctx);
    let project_snapshot = editor.project.clone();
    let saved_snapshot = editor.saved_project.clone();
    let dirty_snapshot = editor.dirty();
    assert!(!editor.history.can_undo());

    // Frame 0: mask + hero at x=60, alpha=0.5, Start sound + StreamStart synced
    let s0 = editor.current_authored_scene(true);
    assert_eq!(s0.draws.len(), 1);
    assert!(
        !s0.clips.is_empty(),
        "mask clip_depth must project onto Stage"
    );
    assert!((s0.draws[0].item.transform[4] - 60.0).abs() < 1e-3);
    assert!((s0.draws[0].item.alpha - 0.5).abs() < 1e-3);
    assert!(
        editor.timeline_audio.history.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                ..
            } if resource == "swf-sound-10"
        )),
        "expected Start sound on frame 0, got: {:?}",
        editor.timeline_audio.history
    );
    assert!(
        editor.timeline_audio.history.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::StreamStart { channel, resource, .. }
                if channel == "swf-root-stream" && resource == "swf-stream-root-1"
        )),
        "expected StreamStart on frame 0, got: {:?}",
        editor.timeline_audio.history
    );

    // Step forward to Frame 1 (`step_swf_preview(1)`): hero hidden, SoundSync::Stop emitted!
    editor.timeline_audio.history.clear();
    let st1 = editor.step_swf_preview(1).expect("step to frame 1");
    assert_eq!(st1.frame, 1);
    assert!(editor.current_authored_scene(true).draws.is_empty());
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("[trace] avm1-f1"))
    );
    assert!(
        editor.timeline_audio.history.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::Stop { resource } if resource == "swf-sound-10"
        )),
        "expected Stop command on frame 1, got: {:?}",
        editor.timeline_audio.history
    );

    // Step forward to Frame 2 (`step_swf_preview(1)`): StreamStop + new StreamStart, hero visible at x=190!
    editor.timeline_audio.history.clear();
    let st2 = editor.step_swf_preview(1).expect("step to frame 2");
    assert_eq!(st2.frame, 2);
    let s2 = editor.current_authored_scene(true);
    assert_eq!(s2.draws.len(), 1);
    assert!((s2.draws[0].item.transform[4] - 190.0).abs() < 1e-3);
    assert!((s2.draws[0].item.alpha - 1.0).abs() < 1e-3);
    assert!(
        editor.timeline_audio.history.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::StreamStop { channel } if channel == "swf-root-stream"
        )),
        "expected StreamStop on stream replacement at frame 2, got: {:?}",
        editor.timeline_audio.history
    );
    assert!(
        editor.timeline_audio.history.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::StreamStart { channel, resource, .. }
                if channel == "swf-root-stream" && resource == "swf-stream-root-2"
        )),
        "expected StreamStart seq 2 at frame 2, got: {:?}",
        editor.timeline_audio.history
    );

    // Step forward to Frame 3 (`step_swf_preview(1)`): ActionStopSounds emits `StopAll`!
    editor.timeline_audio.history.clear();
    let st3 = editor.step_swf_preview(1).expect("step to frame 3");
    assert_eq!(st3.frame, 3);
    assert!(!st3.playing);
    assert!(
        editor
            .timeline_audio
            .history
            .contains(&SoundCommand::StopAll),
        "expected StopAll on frame 3, got: {:?}",
        editor.timeline_audio.history
    );

    // Seek backward to Frame 2 (`seek_swf_preview(2)`): resets cleanly, silences prior audio, re-seeks stream!
    editor.timeline_audio.history.clear();
    editor
        .seek_swf_preview(2)
        .expect("backward seek to frame 2");
    assert_eq!(editor.frame, 2);
    let s2_back = editor.current_authored_scene(true);
    assert_eq!(s2_back.draws.len(), 1);
    assert!((s2_back.draws[0].item.transform[4] - 190.0).abs() < 1e-3);
    assert_eq!(
        editor.timeline_audio.history.first(),
        Some(&SoundCommand::StopAll)
    );

    // Restart (`restart_swf_preview()`): deterministically matches initial Frame 0 pose & traces without Undo/dirty!
    editor.output.clear();
    editor.restart_swf_preview().expect("restart SWF preview");
    assert_eq!(editor.frame, 0);
    let s0_restart = editor.presented_authored_scene(true);
    assert_eq!(s0_restart.draws.len(), 1);
    assert_eq!(
        s0_restart.draws[0].item.transform,
        s0.draws[0].item.transform
    );
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("[trace] avm1-f0")),
        "restart must re-route frame 0 trace to Output"
    );
    assert_eq!(editor.project, project_snapshot);
    assert_eq!(editor.saved_project, saved_snapshot);
    assert_eq!(editor.dirty(), dirty_snapshot);
    assert!(!editor.history.can_undo());

    // -----------------------------------------------------------------------
    // Part B: Interactive AVM2 SWF (`DoABC2` display mutation + backward step + restart)
    // -----------------------------------------------------------------------
    let mut avm2_tags = Vec::new();
    swf_tag(&mut avm2_tags, 69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes AS3
    swf_tag(&mut avm2_tags, 2, &shape1);
    let mut po2_badge = vec![0x26];
    po2_badge.extend_from_slice(&1_u16.to_le_bytes());
    po2_badge.extend_from_slice(&1_u16.to_le_bytes());
    po2_badge.extend_from_slice(&translate_zero_matrix);
    po2_badge.extend_from_slice(b"badge\0");
    swf_tag(&mut avm2_tags, 26, &po2_badge);

    let mut abc = Vec::new();
    abc.extend_from_slice(&0_u32.to_le_bytes());
    abc.push(0);
    abc.extend_from_slice(&16_u16.to_le_bytes());
    abc.extend_from_slice(&46_u16.to_le_bytes());
    abc.extend_from_slice(&[1, 1, 1, 6]);
    abc.push(0);
    abc.extend_from_slice(b"\x05trace\x0bt471_avm2_0\x05badge\x01x");
    abc.extend_from_slice(&[2, 0x16, 1, 1]);
    abc.push(4);
    abc.extend_from_slice(&[0x07, 1, 2, 0x07, 1, 4, 0x07, 1, 5]);
    abc.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 4, 1, 0, 1]);
    let avm2_code = [
        0x5D, 0x02, 0x66, 0x02, 0x24, 125, 0x61, 0x03, // badge.x = 125
        0x5D, 0x01, 0x2C, 0x03, 0x4F, 0x01, 0x01, // trace("t471_avm2_0")
        0x47,
    ];
    abc.push(avm2_code.len() as u8);
    abc.extend_from_slice(&avm2_code);
    abc.extend_from_slice(&[0, 0]);
    swf_tag(&mut avm2_tags, 82, &abc);
    swf_tag(&mut avm2_tags, 1, &[]);
    swf_tag(&mut avm2_tags, 28, &1_u16.to_le_bytes()); // RemoveObject2 depth 1 on frame 1
    swf_tag(&mut avm2_tags, 1, &[]);

    let avm2_path = tmp.0.join("interactive_avm2.swf");
    fs::write(&avm2_path, build_swf(10, 2, &avm2_tags)).unwrap();

    let mut avm2_ed = Editor::from_project(&ctx, Project::blank());
    avm2_ed.open_path(avm2_path, &ctx);
    let avm2_proj_before = avm2_ed.project.clone();
    assert!((avm2_ed.current_authored_scene(true).draws[0].item.transform[4] - 125.0).abs() < 1e-3);
    assert!(
        avm2_ed
            .output
            .iter()
            .any(|l| l.contains("[trace] t471_avm2_0"))
    );
    avm2_ed.step_swf_preview(1).unwrap();
    assert!(avm2_ed.current_authored_scene(true).draws.is_empty());
    avm2_ed.step_swf_preview(-1).unwrap();
    assert!((avm2_ed.current_authored_scene(true).draws[0].item.transform[4] - 125.0).abs() < 1e-3);
    assert_eq!(avm2_ed.project, avm2_proj_before);
    assert!(!avm2_ed.history.can_undo());

    // -----------------------------------------------------------------------
    // Part C: Unreconstructible SWF (`import_project` fails static validation due to an
    //         unreferenced cyclic `DefineSprite(id=99)` while `SwfBytecodePlayer::from_bytes`
    //         succeeds!) — verifies bounded Stage fallback preview leaves the active authored
    //         document & Undo history 100% untouched!
    // -----------------------------------------------------------------------
    let mut fallback_tags = Vec::new();
    swf_tag(&mut fallback_tags, 2, &shape1);
    swf_tag(&mut fallback_tags, 14, &def_sound);

    // Redefined + cyclic DefineSprite(id=99) placing character_id=99 inside itself ->
    // fails static `Project::validate()` (`swf_invalid_project`) while `SwfBytecodePlayer` plays cleanly!
    let mut stub_sprite_99 = Vec::new();
    stub_sprite_99.extend_from_slice(&99_u16.to_le_bytes());
    stub_sprite_99.extend_from_slice(&1_u16.to_le_bytes());
    swf_tag(&mut stub_sprite_99, 1, &[]);
    swf_tag(&mut stub_sprite_99, 0, &[]);
    swf_tag(&mut fallback_tags, 39, &stub_sprite_99);

    let mut cyclic_sprite = Vec::new();
    cyclic_sprite.extend_from_slice(&99_u16.to_le_bytes());
    cyclic_sprite.extend_from_slice(&1_u16.to_le_bytes());
    let mut po2_self = vec![0x06];
    po2_self.extend_from_slice(&1_u16.to_le_bytes());
    po2_self.extend_from_slice(&99_u16.to_le_bytes());
    po2_self.extend_from_slice(&translate_zero_matrix);
    swf_tag(&mut cyclic_sprite, 26, &po2_self);
    swf_tag(&mut cyclic_sprite, 1, &[]);
    swf_tag(&mut cyclic_sprite, 0, &[]);
    swf_tag(&mut fallback_tags, 39, &cyclic_sprite);

    // Root timeline (3 frames) placing DefineSprite 2 ("orb") and mutating it across frames 0..=2!
    swf_tag(&mut fallback_tags, 39, &sprite2);
    let mut po2_orb = vec![0x26];
    po2_orb.extend_from_slice(&1_u16.to_le_bytes());
    po2_orb.extend_from_slice(&2_u16.to_le_bytes());
    po2_orb.extend_from_slice(&translate_zero_matrix);
    po2_orb.extend_from_slice(b"orb\0");
    swf_tag(&mut fallback_tags, 26, &po2_orb);
    swf_tag(&mut fallback_tags, 15, &start_snd);

    let mut fb_act0 = Vec::new();
    push_avm1_set_prop(&mut fb_act0, "orb", 0, 45);
    push_avm1_trace(&mut fb_act0, "fallback-f0");
    fb_act0.push(0x00);
    swf_tag(&mut fallback_tags, 12, &fb_act0);
    swf_tag(&mut fallback_tags, 1, &[]); // Frame 0

    let mut fb_act1 = Vec::new();
    push_avm1_set_prop(&mut fb_act1, "orb", 0, 135);
    push_avm1_trace(&mut fb_act1, "fallback-f1");
    fb_act1.push(0x00);
    swf_tag(&mut fallback_tags, 12, &fb_act1);
    swf_tag(&mut fallback_tags, 1, &[]); // Frame 1

    let mut fb_act2 = Vec::new();
    push_avm1_set_prop(&mut fb_act2, "orb", 7, 0); // _visible = false
    fb_act2.push(0x09); // ActionStopSounds
    fb_act2.push(0x07); // ActionStop
    push_avm1_trace(&mut fb_act2, "fallback-f2-stop");
    fb_act2.push(0x00);
    swf_tag(&mut fallback_tags, 12, &fb_act2);
    swf_tag(&mut fallback_tags, 1, &[]); // Frame 2

    let unreconstructible_path = tmp.0.join("unreconstructible_cyclic.swf");
    fs::write(&unreconstructible_path, build_swf(8, 3, &fallback_tags)).unwrap();

    // Confirm static `import_project` genuinely fails while `SwfBytecodePlayer` succeeds!
    assert!(
        neoflash_import::import_project(&unreconstructible_path).is_err(),
        "unreconstructible SWF fixture must fail static import_project"
    );

    // Prepare an active authored document with existing artwork and 1 Undo step.
    let mut authored_proj = Project::blank();
    authored_proj.name = "AuthoredProtectedScene".into();
    let mut host_editor = Editor::from_project(&ctx, authored_proj);
    host_editor.add_element(
        ElementKind::Rectangle {
            width: 90.0,
            height: 50.0,
            fill: [30, 200, 110, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.0,
        },
        12.0,
        24.0,
    );
    let authored_scene_baseline = host_editor.current_authored_scene(true);
    assert_eq!(authored_scene_baseline.draws.len(), 1);
    let authored_doc_before = host_editor.project.clone();
    let saved_doc_before = host_editor.saved_project.clone();
    let undo_len_before = host_editor.history.undo_len();
    let redo_len_before = host_editor.history.redo_len();
    let rev_before = host_editor.history.revision();
    let session_count_before = host_editor.session_count();

    // Attach bounded fallback preview via `open_swf_with_bounded_preview_fallback`!
    let fallback_res = host_editor
        .open_swf_with_bounded_preview_fallback(&unreconstructible_path, &ctx)
        .expect("bounded SwfBytecodePlayer fallback must succeed");
    assert_eq!(fallback_res.total_frames, 3);
    assert!(
        host_editor
            .output
            .iter()
            .any(|l| l.contains("[swf-fallback]") && l.contains("authored document unchanged")),
        "expected explicit [swf-fallback] diagnostic in Output, got: {:?}",
        host_editor.output
    );
    assert!(
        host_editor
            .output
            .iter()
            .any(|l| l.contains("[trace] fallback-f0"))
    );

    // Frame 0 of fallback SWF projects onto Stage (`orb._x == 45.0`) even though `host_editor.project`
    // has only 1 frame and NO `swf-sym-*` symbols!
    let fb_s0 = host_editor.current_authored_scene(true);
    assert_eq!(fb_s0.draws.len(), 1);
    assert!((fb_s0.draws[0].item.transform[4] - 45.0).abs() < 1e-3);

    // Step to Frame 1 (`orb._x == 135.0`), even though `host_editor.project` has `frame_count == 1`!
    let fb_st1 = host_editor
        .step_swf_preview(1)
        .expect("step standalone fallback SWF to frame 1");
    assert_eq!(fb_st1.frame, 1);
    assert_eq!(host_editor.frame, 1);
    let fb_s1 = host_editor.current_authored_scene(true);
    assert_eq!(fb_s1.draws.len(), 1);
    assert!((fb_s1.draws[0].item.transform[4] - 135.0).abs() < 1e-3);
    assert!(
        host_editor
            .output
            .iter()
            .any(|l| l.contains("[trace] fallback-f1"))
    );

    // Seek to Frame 2 (`orb._visible == false`, `StopAll` audio command):
    host_editor.timeline_audio.history.clear();
    host_editor
        .seek_swf_preview(2)
        .expect("seek standalone fallback SWF to frame 2");
    assert_eq!(host_editor.frame, 2);
    assert!(host_editor.current_authored_scene(true).draws.is_empty());
    assert!(
        host_editor
            .timeline_audio
            .history
            .contains(&SoundCommand::StopAll)
    );

    // Step backward to Frame 1 (`orb._x == 135.0`) and restart to Frame 0 (`orb._x == 45.0`):
    host_editor.step_swf_preview(-1).unwrap();
    assert_eq!(host_editor.frame, 1);
    assert!(
        (host_editor.current_authored_scene(true).draws[0]
            .item
            .transform[4]
            - 135.0)
            .abs()
            < 1e-3
    );

    host_editor.restart_swf_preview().unwrap();
    assert_eq!(host_editor.frame, 0);
    assert!(
        (host_editor.current_authored_scene(true).draws[0]
            .item
            .transform[4]
            - 45.0)
            .abs()
            < 1e-3
    );

    // Active authored document, sessions, saved_project, path, import_source, and Undo history
    // are 100% UNCHANGED!
    assert_eq!(host_editor.project, authored_doc_before);
    assert_eq!(host_editor.saved_project, saved_doc_before);
    assert_eq!(host_editor.path, None);
    assert_eq!(host_editor.import_source, None);
    assert_eq!(host_editor.session_count(), session_count_before);
    assert_eq!(host_editor.history.undo_len(), undo_len_before);
    assert_eq!(host_editor.history.redo_len(), redo_len_before);
    assert_eq!(host_editor.history.revision(), rev_before);

    // Exiting preview immediately restores the original authored Stage artwork!
    host_editor.reset_authored_preview();
    let restored_authored_scene = host_editor.current_authored_scene(true);
    assert_eq!(restored_authored_scene.draws.len(), 1);
    assert_eq!(
        restored_authored_scene.draws[0].item.transform,
        authored_scene_baseline.draws[0].item.transform
    );
}
