//! End-to-end CFB binary `.fla` container integration tests for T-408:
//! - Embedded audio extraction (`MP3` raw frames & `PCM` -> RIFF `audio/wav`) with
//!   library Sound ActionScript `linkage_id` (`Sound.attachSound`) and `Keyframe.sound`
//!   (`Start` sync, loop count, custom stereo `SoundEnvelopePoint`s).
//! - Embedded video extraction (`CMediaVideoStream` VP6 muxed into valid `FLV` `video/x-flv`
//!   `ResourceUnit` + `Keyframe.video` `TimelineVideoSpan` + `ElementKind::Video`,
//!   `videoType = 0` ActionScript-controlled empty Video viewport without false warnings,
//!   and `videoType = 5` VP6-alpha emitting honest `fla_media_not_imported`).
//! - Compiled-clip `CSwfPage` primary `SWF` extraction + `CPicSwf` placement.

use neoflash_core::{ElementKind, SoundEffect, SoundSync, as2_sound};
use neoflash_import::{import_project, inspect};
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
fn empty_shape(out: &mut Vec<u8>) {
    out.extend_from_slice(&[1, 0, 0, 0]);
    point_min(out);
    out.push(2);
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(out, v);
    }
    out.push(5);
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&[0; 4]);
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
        u32b(out, 0); // parent item_id = None
        u32b(out, 0); // item item_id = None
        u32b(out, 0); // attributes
        out.push(0); // linkage rev 0
        u32b(out, 1); // export for ActionScript flag
        string(out, id);
        string(out, "");
    } else {
        u32b(out, 3); // library_metadata rev 3
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
    }
    u32b(out, 0); // rev >= 5
}

fn build_cmedia_video_bytes(video_type: u32, width: i32, height: i32, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(2); // rev 2
    u32b(&mut b, video_type);
    b.extend_from_slice(&25.0_f64.to_le_bytes()); // frame_rate
    b.extend_from_slice(&44_100.0_f64.to_le_bytes()); // audio_sample_rate
    i32b(&mut b, 16); // audio_bits_per_sample
    i32b(&mut b, 1); // audio_channels
    i32b(&mut b, width);
    i32b(&mut b, height);
    b.extend_from_slice(&1.0_f64.to_le_bytes()); // duration_seconds
    u32b(&mut b, 0); // is_special
    b.push(0); // import_audio
    b.push(1); // sync_video
    u32b(&mut b, 0); // drop_ratio_index
    i32b(&mut b, 80); // quality
    b.extend_from_slice(&1.0_f64.to_le_bytes()); // scale_ratio
    u32b(&mut b, 12); // key_frame_interval

    // Track 0 (video)
    u32b(&mut b, 0);
    if !payload.is_empty() {
        b.push(1); // marker
        i32b(&mut b, 0); // offset
        i32b(&mut b, payload.len() as i32); // count
        i32b(&mut b, 1); // frame_count
        i32b(&mut b, 1); // keyframe flag
    }
    b.push(0); // end packet indices
    u32b(&mut b, payload.len() as u32);
    b.extend_from_slice(payload);

    // Track 1 (audio = empty)
    u32b(&mut b, 1);
    b.push(0);
    u32b(&mut b, 0);
    b
}

#[test]
fn binary_fla_imports_sound_linkage_embedded_flv_video_compiled_clip_and_honest_vp6a_warning() {
    // Valid MPEG-1 Layer III 128kbps 44.1kHz mono frame (417 bytes).
    let mut mp3_frame = vec![0_u8; 417];
    mp3_frame[0] = 0xff;
    mp3_frame[1] = 0xfb;
    mp3_frame[2] = 0x90;
    mp3_frame[3] = 0xc4;

    // 16-bit mono 44.1kHz PCM samples.
    let pcm_bytes: Vec<u8> = (0..64_i16).flat_map(|i| (i * 256).to_le_bytes()).collect();

    // Embedded VP6 video stream (320x240), empty ActionScript video (160x120),
    // and raw VP6-alpha stream (videoType = 5 -> honest fla_media_not_imported).
    let vp6_media = build_cmedia_video_bytes(4, 320, 240, &[0x14, 0x00, 0x11, 0x22, 0x33]);
    let empty_video_media = build_cmedia_video_bytes(0, 160, 120, &[]);
    let vp6a_media = build_cmedia_video_bytes(5, 200, 150, &[0x15, 0x00, 0x44]);

    // Minimal valid uncompressed FWS SWF v8 (header + End tag = 10 bytes).
    let mini_swf: [u8; 10] = [
        b'F', b'W', b'S', 8, 10, 0, 0, 0, // FWS v8, len = 10
        0x00, 0x00, // rect 0 bits + End
    ];

    let mut contents = Vec::new();
    push_contents_header_and_scene(&mut contents);

    // Media 1: MP3 Sound with ActionScript linkage "sfx_click".
    class(&mut contents, "CMediaSound"); // class ref 3, obj ref 4
    push_media_descriptor_prefix(&mut contents, "Media 1", "Click.mp3", 1, Some("sfx_click"));
    contents.push(8); // sound subtype rev 8
    u16b(&mut contents, (2 << 4) | (3 << 2) | 2); // codec 2 (MP3), 44kHz, 16-bit mono
    u32b(&mut contents, 1152);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, mp3_frame.len() as u32);
    contents.push(0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);

    // Media 2: Raw 16-bit PCM Sound wrapped into RIFF WAV.
    u16b(&mut contents, 0x8003); // CMediaSound, obj ref 5
    push_media_descriptor_prefix(&mut contents, "Media 2", "Tone.wav", 2, None);
    contents.push(8);
    u16b(&mut contents, (3 << 2) | 2); // 44kHz, 16-bit, mono PCM
    u32b(&mut contents, 64);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, pcm_bytes.len() as u32);
    contents.push(0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);

    // Media 3: Embedded VP6 video (CMediaVideoStream).
    class(&mut contents, "CMediaVideoStream"); // class ref 6, obj ref 7
    push_media_descriptor_prefix(&mut contents, "Media 3", "IntroVideo", 3, None);

    // Media 4: Empty ActionScript-controlled Video (videoType = 0).
    u16b(&mut contents, 0x8006); // CMediaVideoStream, obj ref 8
    push_media_descriptor_prefix(&mut contents, "Media 4", "NetStreamView", 4, None);

    // Media 5: Raw VP6 with alpha (videoType = 5 -> honest fla_media_not_imported).
    u16b(&mut contents, 0x8006); // CMediaVideoStream, obj ref 9
    push_media_descriptor_prefix(&mut contents, "Media 5", "AlphaClip", 5, None);

    // Media 6: Compiled clip CSwfPage with primary_swf.
    class(&mut contents, "CSwfPage"); // class ref 10, obj ref 11
    push_media_descriptor_prefix(&mut contents, "Media 6", "PlayerWidget", 6, None);
    contents.push(0); // component rev 0
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(&mut contents, "");
    // smart_movie[0] (primary_swf)
    contents.push(1);
    string(&mut contents, "PlayerWidget.swf");
    u32b(&mut contents, mini_swf.len() as u32);
    contents.extend_from_slice(&mini_swf);
    // smart_movie[1] & [2] empty
    contents.push(0);
    contents.push(0);
    // CSwfPage subtype rev 2 (source name + bounds_words: right, left, bottom, top in twips)
    contents.push(2);
    string(&mut contents, "PlayerWidget.swc");
    for w in [4800_i32, 0, 3600, 0] {
        i32b(&mut contents, w);
    }

    push_contents_settings_tail(&mut contents, 6);

    // Build Page 1 timeline with 1 layer, 1 frame (duration 5, FrameSound referencing media 1
    // with Start sync, 2 loops, custom envelope), containing:
    // - CPicVideoStream referencing Media 3 (320x240 VP6 FLV)
    // - CPicVideoStream referencing Media 4 (160x120 empty ActionScript Video)
    // - CPicSwf referencing Media 6 (240x180 compiled clip)
    let mut page = vec![1];
    class(&mut page, "CPicPage"); // ref 1, obj 2
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer"); // ref 3, obj 4
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame"); // ref 5, obj 6
    page.extend_from_slice(&[1, 0]);

    // Child 1 of Frame: CPicVideoStream (Media 3)
    class(&mut page, "CPicVideoStream"); // ref 7, obj 8
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3); // CPicVideoStream rev 3
    for w in [65536_i32, 0, 0, 65536, 400, 600] {
        i32b(&mut page, w);
    }
    for w in [0_i32, 6400, 0, 4800] {
        i32b(&mut page, w);
    }
    page.push(0);
    string(&mut page, "");
    string(&mut page, "intro_vid");
    u32b(&mut page, 1); // media_present
    u16b(&mut page, 3); // media_id = 3

    // Child 2 of Frame: CPicVideoStream (Media 4, empty AS viewport)
    u16b(&mut page, 0x8007); // CPicVideoStream, obj 9
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
    u16b(&mut page, 4); // media_id = 4

    // Child 3 of Frame: CPicSwf (Media 6, compiled clip)
    class(&mut page, "CPicSwf"); // ref 10, obj 11
    page.extend_from_slice(&[1, 0, 0, 0]);
    point_min(&mut page);
    page.push(3); // CPicSwf rev 3
    for w in [65536_i32, 0, 0, 65536, 800, 1000] {
        i32b(&mut page, w);
    }
    u32b(&mut page, 1); // media_present
    u16b(&mut page, 6); // media_id = 6
    u32b(&mut page, 0); // actions rev 0
    u32b(&mut page, 1); // actions mode 1
    string(&mut page, "");
    string(&mut page, "widget_mc");

    u16b(&mut page, 0); // end Frame children
    point_min(&mut page);
    // Frame shape + CPicFrame rev 21 body
    out_empty_shape_body(&mut page);
    page.push(21);
    u16b(&mut page, 5); // duration = 5 frames
    u16b(&mut page, 0);
    u16b(&mut page, 0);
    u16b(&mut page, 1); // sound_media_id = 1 ("sfx_click")
    u16b(&mut page, 2); // 2 custom envelope points
    for (mark44, l, r) in [(0_u32, 32768_u16, 16384_u16), (11025, 16384, 32768)] {
        u32b(&mut page, mark44);
        u16b(&mut page, l);
        u16b(&mut page, r);
    }
    u16b(&mut page, 2); // loops = 2
    page.push(1); // sync = Start
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
    u32b(&mut page, 7); // custom envelope sound_effect

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
    let project = import_project(&path).expect("import synthetic media binary FLA");
    project.validate().expect("imported project must validate");

    // 1. Only Media 5 (VP6-alpha) should emit fla_media_not_imported!
    let media_warnings: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_media_not_imported")
        .collect();
    assert_eq!(media_warnings.len(), 1, "{:?}", report.diagnostics);
    assert!(
        media_warnings[0].location.contains("media 5"),
        "expected media 5 VP6-alpha warning, got {:?}",
        media_warnings[0]
    );

    // 2. Sound linkage "sfx_click" resolves for ActionScript Sound.attachSound AND Keyframe.sound!
    assert_eq!(
        as2_sound::resource_path(&project, "sfx_click")
            .expect("resolve sfx_click")
            .as_deref(),
        Some("legacy/sounds/1/Click.mp3")
    );
    assert!(
        project
            .resources
            .iter()
            .any(|r| r.id == "legacy-sound-2" && r.media_type == "audio/wav")
    );
    let root = project
        .symbols
        .iter()
        .find(|s| s.id == project.root)
        .unwrap();
    let kf = &root.timeline.layers[0].keyframes[0];
    let sound = kf.sound.as_ref().expect("Keyframe.sound wired");
    assert_eq!(sound.resource, "sfx_click");
    assert_eq!(sound.sync, SoundSync::Start);
    assert_eq!(sound.loops, 2);
    assert_eq!(sound.effect, SoundEffect::Custom);
    assert_eq!(sound.envelope.as_ref().expect("custom envelope").len(), 2);

    // 3. Embedded VP6 video extracted to FLV resource + Keyframe.video + Video viewports!
    assert!(
        project
            .resources
            .iter()
            .any(|r| r.id == "legacy-video-3" && r.media_type == "video/x-flv")
    );
    let vid_span = kf.video.as_ref().expect("Keyframe.video wired");
    assert_eq!(vid_span.resource, "legacy-video-3");
    assert!(
        kf.elements.iter().any(|e| {
            e.name == "intro_vid"
                && matches!(e.kind, ElementKind::Video { width, height } if width == 320.0 && height == 240.0)
        }),
        "expected intro_vid 320x240 Video element: {:?}",
        kf.elements
    );
    assert!(
        kf.elements.iter().any(|e| {
            e.name == "net_vid"
                && matches!(e.kind, ElementKind::Video { width, height } if width == 160.0 && height == 120.0)
        }),
        "expected net_vid 160x120 empty Video viewport: {:?}",
        kf.elements
    );

    // 4. Compiled clip CSwfPage extracted to application/x-shockwave-flash resource + placed instance!
    assert!(
        project
            .resources
            .iter()
            .any(|r| { r.id == "legacy-swf-6" && r.media_type == "application/x-shockwave-flash" })
    );
    assert!(kf.elements.iter().any(|e| {
        e.name == "widget_mc"
            && matches!(&e.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == "fla-compiled-clip-6")
    }));
}

fn out_empty_shape_body(out: &mut Vec<u8>) {
    let _ = empty_shape;
    out.push(2);
    for v in [65536_i32, 0, 0, 65536, 0, 0] {
        i32b(out, v);
    }
    out.push(5);
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&[0; 4]);
}
