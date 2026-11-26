//! End-to-end CFB binary `.fla` container integration tests for T-408 / T-414:
//! - Embedded audio extraction (`MP3` raw frames & `PCM` -> RIFF `audio/wav`) with
//!   library Sound ActionScript `linkage_id` (`Sound.attachSound`), `Keyframe.sound`
//!   (`Start` sync, loop count, custom stereo `SoundEnvelopePoint`s), AS3 sound linkage
//!   `fla_linkage_approximated`, and `.fla`/XFL `<DOMSoundItem>` linkage round-trip.
//! - Embedded video extraction (`CMediaVideoStream` On2 `VP6` (`videoType = 4`) and
//!   `VP6A` alpha (`videoType = 5`) muxed into playable `FLV` `video/x-flv` verified
//!   by real `ffmpeg` decoding + `neoflash_core::inspect_video_resource`, `videoType = 0`
//!   ActionScript-controlled empty Video viewport without false warnings, `CMediaVideo`
//!   `LinkedVideo` emitting `fla_media_not_imported` + enforcing `revision(0)`, and
//!   `.fla`/XFL `<DOMVideoItem>` + multi-part `legacy/source.fla.part00` round-trip).
//! - Compiled-clip `CSwfPage` genuine SWF vector + AVM1 script reconstruction via
//!   `swf::convert` without dead `legacy-swf-*` blobs, plus `fla_media_not_imported`
//!   when a `CSwfPage` has no reconstructable SWF artwork/scripts.
//! - Companion `#include` symlink traversal rejection (`#[cfg(unix)]`).

use neoflash_core::{
    ElementKind, ResourceUnit, as2_sound, inspect_video_resource,
    timeline_video::flv_has_vp6a_alpha,
};
use neoflash_import::{BinaryStreamRole, export_fla, import_project, inspect};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-import-fla-fidelity-{}-{}",
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

struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    used: u8,
}
impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            used: 0,
        }
    }
    fn push_bits(&mut self, val: u32, nbits: u8) {
        for i in (0..nbits).rev() {
            let bit = ((val >> i) & 1) as u8;
            self.cur = (self.cur << 1) | bit;
            self.used += 1;
            if self.used == 8 {
                self.bytes.push(self.cur);
                self.cur = 0;
                self.used = 0;
            }
        }
    }
    fn push_sbits(&mut self, val: i32, nbits: u8) {
        let mask = (1_u32 << nbits) - 1;
        self.push_bits((val as u32) & mask, nbits);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.bytes.push(self.cur << (8 - self.used));
        }
        self.bytes
    }
}

fn push_swf_tag(out: &mut Vec<u8>, code: u16, body: &[u8]) {
    if body.len() < 63 {
        let hdr = (code << 6) | (body.len() as u16);
        out.extend_from_slice(&hdr.to_le_bytes());
    } else {
        let hdr = (code << 6) | 0x3f;
        out.extend_from_slice(&hdr.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(body);
}

/// Builds a genuine uncompressed FWS SWF v8 containing a filled triangle `DefineShape`,
/// `PlaceObject2`, `DoAction` (`stop();`), `ShowFrame`, and `End`.
fn build_reconstructable_component_swf() -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0x00); // empty stage RECT (nbits = 0)
    body.extend_from_slice(&[0x00, 0x18]); // 24.0 fps
    body.extend_from_slice(&[0x01, 0x00]); // 1 frame

    // DefineShape (tag 2): character #1, filled triangle (0,0)->(40px,0)->(0,30px)->close
    let mut shape_tag = Vec::new();
    shape_tag.extend_from_slice(&1_u16.to_le_bytes()); // shape_id = 1
    shape_tag.push(0x00); // bounds RECT
    shape_tag.push(0x01); // 1 fill style
    shape_tag.extend_from_slice(&[0x00, 0x33, 0x88, 0xff]); // solid RGB
    shape_tag.push(0x00); // 0 line styles
    shape_tag.push(0x10); // 1 fill bit, 0 line bits
    let mut bw = BitWriter::new();
    // StyleChangeRecord: type=0 (1b), NewStyles=0, LineStyle=0, FillStyle1=1, FillStyle0=0, MoveTo=1 -> 0b0_00101
    bw.push_bits(0b000101, 6);
    bw.push_bits(12, 5); // move_bits = 12
    bw.push_sbits(0, 12);
    bw.push_sbits(0, 12);
    bw.push_bits(1, 1); // fill_style_1 = 1
    for (dx, dy) in [(800_i32, 0_i32), (-800, 600), (0, -600)] {
        bw.push_bits(0b11, 2); // StraightEdge
        bw.push_bits(10, 4); // nbits - 2 = 10 -> 12 bits
        bw.push_bits(1, 1); // general line
        bw.push_sbits(dx, 12);
        bw.push_sbits(dy, 12);
    }
    bw.push_bits(0, 6); // EndShapeRecord
    shape_tag.extend_from_slice(&bw.finish());
    push_swf_tag(&mut body, 2, &shape_tag);

    // PlaceObject2 (tag 26): flags = HasCharacter (0x02), depth = 1, char_id = 1
    let mut place_tag = vec![0x02];
    place_tag.extend_from_slice(&1_u16.to_le_bytes());
    place_tag.extend_from_slice(&1_u16.to_le_bytes());
    push_swf_tag(&mut body, 26, &place_tag);

    // DoAction (tag 12): ActionStop (0x07), ActionEnd (0x00)
    push_swf_tag(&mut body, 12, &[0x07, 0x00]);

    // ShowFrame (tag 1) + End (tag 0)
    push_swf_tag(&mut body, 1, &[]);
    push_swf_tag(&mut body, 0, &[]);

    let total_len = (8 + body.len()) as u32;
    let mut swf = Vec::with_capacity(total_len as usize);
    swf.extend_from_slice(b"FWS\x08");
    swf.extend_from_slice(&total_len.to_le_bytes());
    swf.extend_from_slice(&body);
    swf
}

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
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
    i32b(out, i32::MIN);
    i32b(out, i32::MIN);
}

fn push_contents_header_and_scene(contents: &mut Vec<u8>) {
    contents.push(0x3f);
    u32b(contents, 1);
    u32b(contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(contents, 0);
    }
    class(contents, "CDocumentPage"); // ref 1 (class), ref 2 (object)
    contents.push(23);
    compact_utf16(contents, "Page 1");
    string(contents, "Scene 1");
    u32b(contents, 7);
    contents.push(0);
    string(contents, "");
    u32b(contents, 0);
    u32b(contents, 6);
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    contents.push(2);
    u32b(contents, 0);
    string(contents, "");
    string(contents, "");
    contents.push(0);
    u32b(contents, 1);
    string(contents, "");
    string(contents, "");
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    contents.push(0);
    u32b(contents, 0);
    string(contents, "");
    string(contents, "");
    contents.push(1);
    u32b(contents, 0);
    u32b(contents, 0);
    contents.push(7);
    u32b(contents, 0);
    string(contents, "");
    string(contents, "");
    string(contents, "");
    contents.push(0);
    u32b(contents, 1);
    string(contents, "");
    string(contents, "");
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    contents.push(0);
    string(contents, "");
    contents.push(8);
    u32b(contents, 0);
    string(contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(contents, "");
    for _ in 0..3 {
        contents.push(2);
        string(contents, "");
        u32b(contents, 0);
        u32b(contents, 0);
    }
    string(contents, "");
    contents.push(1);
    u32b(contents, 0);
    string(contents, "");
    u32b(contents, u32::MAX);
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
    string(contents, "");
    contents.extend_from_slice(&[0; 8]);
    contents.push(3);
    string(contents, "");
    u32b(contents, 0);
    u32b(contents, 0);
    string(contents, "");
    string(contents, "");
    u32b(contents, 0);
    string(contents, "");
    contents.extend_from_slice(&[0; 20]);

    u16b(contents, 0); // end scenes
    u16b(contents, 1);
    u16b(contents, 0);
    u16b(contents, 2);
    u16b(contents, 0); // end symbols
    u16b(contents, 0);
}

fn push_contents_settings_tail(contents: &mut Vec<u8>, media_counter: u16) {
    u16b(contents, 0); // end media list
    u16b(contents, media_counter);
    u16b(contents, 2); // selected_scene = ref 2
    contents.push(0);
    for word in [0_i32, 11000, 0, 8000] {
        i32b(contents, word);
    }
    u32b(contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    u32b(contents, 65536);
    u16b(contents, 0);
    u16b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0xffffffff);
    u32b(contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    u32b(contents, 20);
}

fn push_media_descriptor_prefix(
    out: &mut Vec<u8>,
    stream_name: &str,
    item_name: &str,
    media_id: u16,
    linkage_id: Option<&str>,
) {
    out.push(6); // descriptor rev 6
    compact_utf16(out, stream_name);
    string(out, item_name);
    u16b(out, media_id);
    string(out, ""); // rev >= 2 source path
    u32b(out, 0); // rev >= 3 timestamp
    if let Some(id) = linkage_id {
        u32b(out, 5); // library_metadata rev 5 (has linkage)
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        out.push(0); // linkage rev 0
        u32b(out, 1); // export for ActionScript flag
        string(out, id);
        string(out, "");
    } else {
        u32b(out, 3);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
    }
    u32b(out, 0);
}

fn build_cmedia_video_bytes(video_type: u32, width: i32, height: i32, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(2); // rev 2
    u32b(&mut b, video_type);
    b.extend_from_slice(&25.0_f64.to_le_bytes()); // frame_rate
    b.extend_from_slice(&44_100.0_f64.to_le_bytes()); // audio_sample_rate
    i32b(&mut b, 16);
    i32b(&mut b, 1);
    i32b(&mut b, width);
    i32b(&mut b, height);
    b.extend_from_slice(&1.0_f64.to_le_bytes());
    u32b(&mut b, 0);
    b.push(0); // import_audio
    b.push(1); // sync_video
    u32b(&mut b, 0);
    i32b(&mut b, 80);
    b.extend_from_slice(&1.0_f64.to_le_bytes());
    u32b(&mut b, 12);

    // Track 0 (video)
    u32b(&mut b, 0);
    if !payload.is_empty() {
        b.push(1);
        i32b(&mut b, 0);
        i32b(&mut b, payload.len() as i32);
        i32b(&mut b, 1);
        i32b(&mut b, 1);
    }
    b.push(0);
    u32b(&mut b, payload.len() as u32);
    b.extend_from_slice(payload);

    // Track 1 (audio = empty)
    u32b(&mut b, 1);
    b.push(0);
    u32b(&mut b, 0);
    b
}

fn find_ffmpeg() -> Option<String> {
    for candidate in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        "ffmpeg",
        "ffmpeg.exe",
    ] {
        if Command::new(candidate)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn assert_ffmpeg_decodes_flv_cleanly(ffmpeg: &str, dir: &std::path::Path, name: &str, flv: &[u8]) {
    let flv_path = dir.join(name);
    fs::write(&flv_path, flv).unwrap();
    let out = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-i",
            flv_path.to_str().unwrap(),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .output()
        .expect("run ffmpeg on muxed FLV");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stderr.trim().is_empty(),
        "ffmpeg failed to decode {name} (status={:?}, stderr={stderr:?})",
        out.status.code()
    );
    assert_eq!(
        out.stdout.len(),
        16 * 16 * 4,
        "expected decoded 16x16 RGBA frame from {name}"
    );
}

#[test]
fn binary_fla_vp6_vp6a_ffmpeg_decode_compiled_clip_reconstruction_and_xfl_roundtrip() {
    let mut mp3_frame = vec![0_u8; 417];
    mp3_frame[0] = 0xff;
    mp3_frame[1] = 0xfb;
    mp3_frame[2] = 0x90;
    mp3_frame[3] = 0xc4;

    let pcm_bytes: Vec<u8> = (0..64_i16).flat_map(|i| (i * 256).to_le_bytes()).collect();

    // Synthetic 16x16 On2 VP6 keyframe bitstream (`VP6FLVVideoPacket` = `[0x00]` + 72-byte VP6 frame).
    let mut vp6_raw = vec![0x41_u8, 0x46, 0x00, 0x20, 1, 1, 1, 1];
    vp6_raw.extend_from_slice(&[0_u8; 64]);
    let mut vp6_pkt = vec![0x00_u8]; // HorizontalAdjustment:0 | VerticalAdjustment:0
    vp6_pkt.extend_from_slice(&vp6_raw);

    // Synthetic 16x16 On2 VP6A (alpha) keyframe packet (`[0x00, 0x00, 0x00, 0x48]` + YUV + Alpha).
    let mut vp6a_pkt = vec![0x00_u8, 0x00, 0x00, vp6_raw.len() as u8];
    vp6a_pkt.extend_from_slice(&vp6_raw);
    vp6a_pkt.extend_from_slice(&vp6_raw);

    let vp6_media = build_cmedia_video_bytes(4, 16, 16, &vp6_pkt);
    let empty_video_media = build_cmedia_video_bytes(0, 160, 120, &[]);
    let vp6a_media = build_cmedia_video_bytes(5, 16, 16, &vp6a_pkt);

    let real_swf = build_reconstructable_component_swf();
    let empty_swf: [u8; 10] = [b'F', b'W', b'S', 8, 10, 0, 0, 0, 0x00, 0x00];

    let mut contents = Vec::new();
    push_contents_header_and_scene(&mut contents);

    // Media 1: MP3 Sound with ActionScript linkage "sfx_click".
    class(&mut contents, "CMediaSound"); // class ref 3, obj ref 4
    push_media_descriptor_prefix(&mut contents, "Media 1", "Click.mp3", 1, Some("sfx_click"));
    contents.push(8);
    u16b(&mut contents, (2 << 4) | (3 << 2) | 2);
    u32b(&mut contents, 1152);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, mp3_frame.len() as u32);
    contents.push(0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);

    // Media 2: Raw 16-bit PCM Sound wrapped into RIFF WAV.
    u16b(&mut contents, 0x8003);
    push_media_descriptor_prefix(&mut contents, "Media 2", "Tone.wav", 2, None);
    contents.push(8);
    u16b(&mut contents, (3 << 2) | 2);
    u32b(&mut contents, 64);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, pcm_bytes.len() as u32);
    contents.push(0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);

    // Media 3: Embedded On2 VP6 video (CMediaVideoStream, videoType = 4).
    class(&mut contents, "CMediaVideoStream"); // class ref 6
    push_media_descriptor_prefix(&mut contents, "Media 3", "IntroVideo", 3, None);

    // Media 4: Empty ActionScript-controlled Video (videoType = 0).
    u16b(&mut contents, 0x8006);
    push_media_descriptor_prefix(&mut contents, "Media 4", "NetStreamView", 4, None);

    // Media 5: Embedded On2 VP6A alpha video (videoType = 5 -> playable FLV CodecID 5!).
    u16b(&mut contents, 0x8006);
    push_media_descriptor_prefix(&mut contents, "Media 5", "AlphaClip", 5, None);

    // Media 6: Compiled clip CSwfPage with genuine shape + stop() SWF (reconstructed!).
    class(&mut contents, "CSwfPage"); // class ref 9 (wait: class ref index depends on object count!)
    // Use explicit `class(...)` if needed — note `0x8000 | ref` uses object/class table index;
    // above: 1=CDocumentPage(class), 2=Page1(obj), 3=CMediaSound(class), 4=Media1(obj), 5=Media2(obj),
    // 6=CMediaVideoStream(class), 7=Media3(obj), 8=Media4(obj), 9=Media5(obj), 10=CSwfPage(class), 11=Media6(obj)!
    push_media_descriptor_prefix(&mut contents, "Media 6", "PlayerWidget", 6, None);
    contents.push(0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(&mut contents, "");
    contents.push(1);
    string(&mut contents, "PlayerWidget.swf");
    u32b(&mut contents, real_swf.len() as u32);
    contents.extend_from_slice(&real_swf);
    contents.push(0);
    contents.push(0);
    contents.push(2);
    string(&mut contents, "PlayerWidget.swc");
    for w in [4800_i32, 0, 3600, 0] {
        i32b(&mut contents, w);
    }

    // Media 7: Compiled clip CSwfPage with empty SWF (no artwork/scripts -> honest fla_media_not_imported!).
    u16b(&mut contents, 0x800a); // CSwfPage class ref 10, obj ref 12
    push_media_descriptor_prefix(&mut contents, "Media 7", "EmptyWidget", 7, None);
    contents.push(0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(&mut contents, "");
    contents.push(1);
    string(&mut contents, "EmptyWidget.swf");
    u32b(&mut contents, empty_swf.len() as u32);
    contents.extend_from_slice(&empty_swf);
    contents.push(0);
    contents.push(0);
    contents.push(2);
    string(&mut contents, "EmptyWidget.swc");
    for w in [2000_i32, 0, 1200, 0] {
        i32b(&mut contents, w);
    }

    // Media 8: External LinkedVideo (`CMediaVideo`, rev 0 -> honest fla_media_not_imported!).
    class(&mut contents, "CMediaVideo"); // class ref 13, obj ref 14
    push_media_descriptor_prefix(&mut contents, "Media 8", "ExternalQuickTime.mov", 8, None);
    contents.push(0); // CMediaVideo subtype revision 0

    push_contents_settings_tail(&mut contents, 8);

    // Page 1 timeline:
    // - CPicVideoStream referencing Media 3 (16x16 VP6 FLV)
    // - CPicVideoStream referencing Media 4 (160x120 empty AS Video)
    // - CPicSwf referencing Media 6 (reconstructed SWF clip)
    // - CPicSwf referencing Media 7 (placeholder fallback clip)
    // - CPicVideo referencing Media 8 (external LinkedVideo, rev 0)
    let mut page = vec![1];
    class(&mut page, "CPicPage"); // ref 1, obj 2
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer"); // ref 3, obj 4
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame"); // ref 5, obj 6
    page.extend_from_slice(&[1, 0]);

    // Child 1: CPicVideoStream (Media 3)
    class(&mut page, "CPicVideoStream"); // ref 7, obj 8
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3);
    for w in [65536_i32, 0, 0, 65536, 400, 600] {
        i32b(&mut page, w);
    }
    for w in [0_i32, 6400, 0, 4800] {
        i32b(&mut page, w);
    }
    page.push(0);
    string(&mut page, "");
    string(&mut page, "intro_vid");
    u32b(&mut page, 1);
    u16b(&mut page, 3);

    // Child 2: CPicVideoStream (Media 4, empty AS viewport)
    u16b(&mut page, 0x8007); // obj 9
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3);
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page, w);
    }
    for w in [i32::MIN, 0, 0, 0] {
        i32b(&mut page, w);
    }
    page.push(0);
    string(&mut page, "");
    string(&mut page, "net_vid");
    u32b(&mut page, 1);
    u16b(&mut page, 4);

    // Child 3: CPicSwf (Media 6, reconstructed compiled clip)
    class(&mut page, "CPicSwf"); // ref 10, obj 11
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3);
    for w in [65536_i32, 0, 0, 65536, 800, 1000] {
        i32b(&mut page, w);
    }
    u32b(&mut page, 1);
    u16b(&mut page, 6);
    u32b(&mut page, 0);
    u32b(&mut page, 1);
    string(&mut page, "");
    string(&mut page, "widget_mc");

    // Child 4: CPicSwf (Media 7, empty SWF fallback compiled clip)
    u16b(&mut page, 0x800a); // obj 12
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3);
    for w in [65536_i32, 0, 0, 65536, 1200, 1000] {
        i32b(&mut page, w);
    }
    u32b(&mut page, 1);
    u16b(&mut page, 7);
    u32b(&mut page, 0);
    u32b(&mut page, 1);
    string(&mut page, "");
    string(&mut page, "empty_widget_mc");

    // Child 5: CPicVideo (Media 8, external LinkedVideo rev 0)
    class(&mut page, "CPicVideo"); // ref 13, obj 14
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(0); // CPicVideo rev 0
    for w in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(&mut page, w);
    }
    for w in [0_i32, 3200, 0, 2400] {
        i32b(&mut page, w);
    }
    u32b(&mut page, 1); // media_present
    u16b(&mut page, 8); // media_id = 8

    u16b(&mut page, 0); // end Frame children
    point_min(&mut page);
    out_empty_shape_body(&mut page);
    page.push(21);
    u16b(&mut page, 5);
    u16b(&mut page, 0);
    u16b(&mut page, 0);
    u16b(&mut page, 1); // sound_media_id = 1 ("sfx_click")
    u16b(&mut page, 2);
    for (mark44, l, r) in [(0_u32, 32768_u16, 16384_u16), (11025, 16384, 32768)] {
        u32b(&mut page, mark44);
        u16b(&mut page, l);
        u16b(&mut page, r);
    }
    u16b(&mut page, 2);
    page.push(1);
    u32b(&mut page, 0);
    u32b(&mut page, 0);
    u16b(&mut page, 0);
    string(&mut page, "");
    u32b(&mut page, 0);
    u32b(&mut page, 1);
    string(&mut page, "");
    u32b(&mut page, 0);
    u32b(&mut page, 0);
    u32b(&mut page, 0);
    u16b(&mut page, 0);
    u32b(&mut page, 0);
    u16b(&mut page, 0);
    string(&mut page, "");
    u32b(&mut page, 0);
    u32b(&mut page, 7);

    u16b(&mut page, 0); // end Layer children
    point_min(&mut page);
    page.push(8);
    string(&mut page, "MediaLayer");
    page.extend_from_slice(&[0; 3]);
    page.extend_from_slice(&[0; 16]);
    page.push(0);
    page.extend_from_slice(&[0; 5]);

    u16b(&mut page, 0); // end Page children
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
    for (idx, bytes) in [
        mp3_frame.as_slice(),
        pcm_bytes.as_slice(),
        vp6_media.as_slice(),
        empty_video_media.as_slice(),
        vp6a_media.as_slice(),
        &[][..],
        &[][..],
        &[][..],
    ]
    .into_iter()
    .enumerate()
    {
        compound
            .create_stream(format!("Media {}", idx + 1))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    let fla_bytes = compound.into_inner().into_inner();

    let dir = TempDir::new();
    let path = dir.0.join("media_fidelity.fla");
    fs::write(&path, &fla_bytes).unwrap();

    let report = inspect(&path).expect("inspect synthetic media binary FLA");
    let mut project = import_project(&path).expect("import synthetic media binary FLA");
    project.validate().expect("imported project must validate");

    // 1. Both Media 3 (VP6) and Media 5 (VP6A) import as playable FLV resources!
    //    Media 7 (empty CSwfPage) and Media 8 (external LinkedVideo) emit honest fla_media_not_imported!
    let media_warnings: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_media_not_imported")
        .collect();
    assert_eq!(
        media_warnings.len(),
        2,
        "expected exactly Media 7 (empty CSwfPage) & Media 8 (LinkedVideo) warnings, got {:?}",
        report.diagnostics
    );
    assert!(
        media_warnings
            .iter()
            .any(|d| d.location.contains("media 7"))
    );
    assert!(
        media_warnings
            .iter()
            .any(|d| d.location.contains("media 8"))
    );

    let vp6_res = project
        .resource("legacy-video-3")
        .expect("VP6 FLV resource");
    let vp6a_res = project
        .resource("legacy-video-5")
        .expect("VP6A FLV resource");
    let vp6_flv = vp6_res.decoded_bytes().unwrap();
    let vp6a_flv = vp6a_res.decoded_bytes().unwrap();
    assert!(!flv_has_vp6a_alpha(&vp6_flv));
    assert!(flv_has_vp6a_alpha(&vp6a_flv));

    // Verify neoflash_core::inspect_video_resource accepts both FLV streams with zero errors!
    let vp6_diags = inspect_video_resource("legacy-video-3", "video/x-flv", &vp6_flv, None);
    let vp6a_diags = inspect_video_resource("legacy-video-5", "video/x-flv", &vp6a_flv, None);
    assert!(!vp6_diags.iter().any(|d| d.is_error), "{vp6_diags:?}");
    assert!(!vp6a_diags.iter().any(|d| d.is_error), "{vp6a_diags:?}");
    assert!(
        vp6_diags
            .iter()
            .any(|d| d.code == "video-flv-transcode-on-publish")
    );

    // Real ffmpeg decode test on BOTH muxed VP6 and VP6A FLV streams when ffmpeg is installed!
    if let Some(ffmpeg) = find_ffmpeg() {
        assert_ffmpeg_decodes_flv_cleanly(&ffmpeg, &dir.0, "muxed_vp6.flv", &vp6_flv);
        assert_ffmpeg_decodes_flv_cleanly(&ffmpeg, &dir.0, "muxed_vp6a.flv", &vp6a_flv);
    }

    // 2. No dead unreferenced `legacy-swf-*` resource exists; `fla-compiled-clip-6` is genuinely
    //    reconstructed with vector geometry AND decompiled `stop()` frame script!
    assert!(
        !project
            .resources
            .iter()
            .any(|r| r.id.starts_with("legacy-swf-")),
        "dead legacy-swf-* blobs must not be emitted"
    );
    let cc6 = project
        .symbol("fla-compiled-clip-6")
        .expect("reconstructed compiled clip symbol");
    assert!(
        cc6.timeline
            .scripts
            .iter()
            .any(|fs| fs.script.source.contains("stop()")),
        "expected decompiled stop() script on reconstructed compiled clip: {:?}",
        cc6.timeline.scripts
    );
    let cc6_has_vector = cc6
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|el| match &el.kind {
            ElementKind::PaintedPath { commands, .. } | ElementKind::Path { commands, .. } => {
                !commands.is_empty()
            }
            ElementKind::Instance { symbol_id, .. } => {
                project.symbol(symbol_id).is_some_and(|child| {
                    child
                        .timeline
                        .layers
                        .iter()
                        .flat_map(|l| &l.keyframes)
                        .flat_map(|k| &k.elements)
                        .any(|ce| {
                            matches!(
                                &ce.kind,
                                ElementKind::PaintedPath { commands, .. }
                                    | ElementKind::Path { commands, .. }
                                    if !commands.is_empty()
                            )
                        })
                })
            }
            _ => false,
        });
    assert!(
        cc6_has_vector,
        "expected reconstructed SWF vector shape inside fla-compiled-clip-6"
    );

    // 3. Sound linkage "sfx_click" resolves AND round-trips through `.fla` / XFL export
    //    alongside embedded FLV video and multi-part `legacy/source.fla.part00` provenance!
    assert_eq!(
        as2_sound::resource_path(&project, "sfx_click")
            .expect("resolve sfx_click")
            .as_deref(),
        Some("legacy/sounds/1/Click.mp3")
    );
    project.resources.retain(|r| r.path != "legacy/source.fla");
    project.resources.push(
        ResourceUnit::from_bytes(
            "fla-original-0",
            "legacy/source.fla.part00",
            "application/octet-stream",
            b"part0",
        )
        .unwrap(),
    );

    let exported_fla_path = dir.0.join("roundtrip_video_sound.fla");
    export_fla(&project, &exported_fla_path)
        .expect("export_fla must succeed on embedded FLV video + linked sound + part00 provenance");
    let reimported =
        import_project(&exported_fla_path).expect("reimport exported XFL .fla with video & sound");
    reimported.validate().expect("reimported project validates");
    assert!(
        as2_sound::resource_path(&reimported, "sfx_click")
            .expect("resolve sfx_click after XFL round-trip")
            .is_some(),
        "DOMSoundItem linkageIdentifier must survive XFL round-trip"
    );
    assert!(
        reimported
            .resources
            .iter()
            .filter(|r| r.media_type == "video/x-flv")
            .count()
            >= 2,
        "embedded VP6 & VP6A FLV video resources must survive XFL round-trip"
    );
    let re_root = reimported.symbol(&reimported.root).unwrap();
    assert!(
        re_root.timeline.layers[0].keyframes[0].video.is_some(),
        "Keyframe.video span must be restored from DOMVideoItem + DOMVideoInstance"
    );
}

#[cfg(unix)]
#[test]
fn companion_include_rejects_leaf_and_directory_symlinks_while_embedding_regular_as_files() {
    use std::os::unix::fs::symlink;

    let outer = TempDir::new();
    let base_dir = outer.0.join("project");
    let outside_dir = outer.0.join("outside");
    fs::create_dir_all(&base_dir).unwrap();
    fs::create_dir_all(&outside_dir).unwrap();

    let secret_file = outside_dir.join("secret.as");
    fs::write(
        &secret_file,
        "var SECRET_TOKEN = \"TOP_SECRET_DO_NOT_READ\";\n",
    )
    .unwrap();
    fs::write(base_dir.join("legit.as"), "var LEGIT_OK = 42;\n").unwrap();
    fs::write(base_dir.join("helper.asi"), "var HELPER_ASI = 7;\n").unwrap();

    symlink(&secret_file, base_dir.join("leak.as")).unwrap();
    symlink(&outside_dir, base_dir.join("sub")).unwrap();

    let mut contents = Vec::new();
    push_contents_header_and_scene(&mut contents);
    push_contents_settings_tail(&mut contents, 0);

    let script_text = "#include \"leak.as\"\n#include \"sub/secret.as\"\n#include \"legit.as\"\n#include \"helper.asi\"\n";
    let mut page = vec![1];
    class(&mut page, "CPicPage");
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer");
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame");
    page.extend_from_slice(&[1, 0]);
    u16b(&mut page, 0); // no display children
    point_min(&mut page);
    out_empty_shape_body(&mut page);
    page.push(21);
    u16b(&mut page, 1); // duration = 1
    u16b(&mut page, 0);
    u16b(&mut page, 0);
    u16b(&mut page, 0); // sound_media_id = 0
    u16b(&mut page, 0); // sound_envelope_count = 0
    u16b(&mut page, 1); // sound_loop_count = 1
    page.push(0); // sound_sync = 0
    u32b(&mut page, 0); // sound_in_point
    u32b(&mut page, 0); // sound_out_point
    u16b(&mut page, 0);
    string(&mut page, ""); // frame label
    u32b(&mut page, 0); // actions rev 0
    u32b(&mut page, 1); // actions mode 1 (source)
    string(&mut page, script_text);
    u32b(&mut page, 0);
    u32b(&mut page, 0);
    u32b(&mut page, 0);
    u16b(&mut page, 0);
    u32b(&mut page, 0);
    u16b(&mut page, 0);
    string(&mut page, "");
    u32b(&mut page, 0);
    u32b(&mut page, 0);

    u16b(&mut page, 0); // end Layer children
    point_min(&mut page);
    page.push(8);
    string(&mut page, "Actions");
    page.extend_from_slice(&[0; 3]);
    page.extend_from_slice(&[0; 16]);
    page.push(0);
    page.extend_from_slice(&[0; 5]);

    u16b(&mut page, 0); // end Page children
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
    let fla_path = base_dir.join("evil.fla");
    fs::write(&fla_path, compound.into_inner().into_inner()).unwrap();

    let project = import_project(&fla_path).expect("import binary FLA with companion includes");
    let include_paths: Vec<&str> = project
        .resources
        .iter()
        .filter(|r| r.id.starts_with("fla-include-"))
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(include_paths, vec!["legit.as", "helper.asi"]);
    for res in &project.resources {
        if let Ok(bytes) = res.decoded_bytes() {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains("TOP_SECRET_DO_NOT_READ"),
                "symlink target leaked into resource {}",
                res.path
            );
        }
    }

    // Importing via a symlinked project directory (e.g. `/tmp -> private/tmp`) must still
    // resolve legitimate `.as`/`.asi` includes while rejecting inner symlinks.
    let linked_project_dir = outer.0.join("project_link");
    symlink(&base_dir, &linked_project_dir).unwrap();
    let linked_project = import_project(&linked_project_dir.join("evil.fla"))
        .expect("import binary FLA via symlinked project directory");
    let linked_include_paths: Vec<&str> = linked_project
        .resources
        .iter()
        .filter(|r| r.id.starts_with("fla-include-"))
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(linked_include_paths, vec!["legit.as", "helper.asi"]);
}

#[test]
fn cmedia_video_and_cpic_video_enforce_revision_zero_boundary() {
    // Verify CMediaVideo with revision = 1 stops Contents before trailing garbage is misread.
    let mut contents = Vec::new();
    push_contents_header_and_scene(&mut contents);
    class(&mut contents, "CMediaVideo");
    push_media_descriptor_prefix(&mut contents, "Media 1", "FutureVid.mov", 1, None);
    contents.push(1); // unproven revision 1 -> must stop sequentially!
    contents.extend_from_slice(&[0xff; 32]);

    let mut page = vec![1];
    class(&mut page, "CPicPage");
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer");
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame");
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicVideo");
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(1); // unproven CPicVideo revision 1 -> must stop sequentially!
    page.extend_from_slice(&[0xaa; 32]);

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
    let dir = TempDir::new();
    let fla_path = dir.0.join("rev_boundary.fla");
    fs::write(&fla_path, compound.into_inner().into_inner()).unwrap();

    let report = inspect(&fla_path).expect("inspect unproven revision FLA");
    let binary = report.binary_fla.as_ref().expect("binary_fla inspection");
    let contents_stream = binary
        .streams
        .iter()
        .find(|s| s.role == BinaryStreamRole::Contents)
        .expect("Contents stream");
    assert!(
        contents_stream.stop_code.is_some() && !contents_stream.structure_complete,
        "CMediaVideo rev 1 must stop Contents sequentially: {contents_stream:?}"
    );
    let page_stream = binary
        .streams
        .iter()
        .find(|s| s.root_class.as_deref() == Some("CPicPage"))
        .expect("Page 1 stream");
    assert!(
        page_stream.stop_code.is_some() && !page_stream.structure_complete,
        "CPicVideo rev 1 must stop Page 1 sequentially: {page_stream:?}"
    );
}

#[test]
#[ignore = "uses private local FLA corpus via NEOFLASH_LEGACY_SAMPLE_DIR"]
fn private_corpus_flv_streams_decode_in_ffmpeg_and_video_compiled_clips_export_to_xfl() {
    let sample_dir = std::env::var("NEOFLASH_LEGACY_SAMPLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("examples/old flash files"));
    assert!(
        sample_dir.is_dir(),
        "missing sample dir {}",
        sample_dir.display()
    );
    let ffmpeg = find_ffmpeg().expect("ffmpeg required for corpus FLV verification");
    let tmp = TempDir::new();

    let mut stack = vec![sample_dir];
    let mut fla_files = Vec::new();
    while let Some(cur) = stack.pop() {
        for entry in fs::read_dir(&cur).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
            {
                fla_files.push(p);
            }
        }
    }
    fla_files.sort();

    let mut decoded_flv_streams = 0usize;
    let mut exported_video_or_cc_docs = 0usize;
    for (doc_idx, fla_path) in fla_files.iter().enumerate() {
        let Ok(project) = import_project(fla_path) else {
            continue;
        };
        project
            .validate()
            .expect("imported corpus project must validate");

        let mut has_video_or_cc = project
            .symbols
            .iter()
            .any(|s| s.id.starts_with("fla-compiled-clip-"));
        for res in &project.resources {
            if res.id.starts_with("legacy-video-") {
                has_video_or_cc = true;
                let raw = res.decoded_bytes().expect("decode legacy-video bytes");
                let diags = inspect_video_resource(&res.id, &res.media_type, &raw, None);
                assert!(
                    !diags.iter().any(|d| d.is_error),
                    "inspect_video_resource failed on doc #{doc_idx} {}: {diags:?}",
                    res.id
                );
                let staged = tmp.0.join(format!("corpus_{doc_idx}_{}.flv", res.id));
                fs::write(&staged, &raw).unwrap();
                let out = Command::new(&ffmpeg)
                    .args([
                        "-v",
                        "error",
                        "-i",
                        staged.to_str().unwrap(),
                        "-f",
                        "null",
                        "-",
                    ])
                    .output()
                    .expect("run ffmpeg on corpus video stream");
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert!(
                    out.status.success() && stderr.trim().is_empty(),
                    "ffmpeg failed on doc #{doc_idx} {} (rc={:?}, stderr={stderr:?})",
                    res.id,
                    out.status.code()
                );
                decoded_flv_streams += 1;
            }
        }
        if has_video_or_cc {
            let out_fla = tmp.0.join(format!("roundtrip_{doc_idx}.fla"));
            export_fla(&project, &out_fla).unwrap_or_else(|err| {
                panic!("export_fla failed on video/compiled-clip doc #{doc_idx}: {err}")
            });
            let reimported = import_project(&out_fla).unwrap_or_else(|err| {
                panic!("re-importing exported XFL failed on doc #{doc_idx}: {err}")
            });
            reimported
                .validate()
                .expect("reimported XFL project must validate");
            exported_video_or_cc_docs += 1;
        }
    }
    assert!(
        decoded_flv_streams >= 23,
        "expected >= 23 decoded corpus video streams, got {decoded_flv_streams}"
    );
    assert!(
        exported_video_or_cc_docs >= 10,
        "expected >= 10 video/compiled-clip corpus FLAs exported cleanly, got {exported_video_or_cc_docs}"
    );
}

fn out_empty_shape_body(out: &mut Vec<u8>) {
    out.push(2);
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(out, v);
    }
    out.push(5);
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&[0; 4]);
}
