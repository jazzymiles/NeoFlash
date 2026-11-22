//! `[T-471]` Track 2 — Embedded SWF Video (`DefineVideoStream` 60 + `VideoFrame` 61),
//! `StartSound` / `StartSound2` Trim Windows & Custom Stereo `SoundEnvelope`,
//! Rotated + Sheared Affine `Paint::Bitmap` Fills (`0x40..=0x43`),
//! AVM2 Multi-Interface + `super` Execution, & Upstream Flash Player `test.swf` Oracles.

use flate2::{Compression, write::ZlibEncoder};
use neoflash_core::{
    ElementKind, Player as CorePlayer, SoundCommand, SoundEffect, SoundSync, VideoSyncMode,
};
use neoflash_import::{SwfBytecodePlayer, import_project, inspect};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn find_ruffle_swfs_dir() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("NEOFLASH_RUFFLE_SWFS_DIR") {
        let p = PathBuf::from(explicit);
        if p.is_dir() {
            return Some(p);
        }
    }
    let root = workspace_root();
    let direct = root.join("target/internet-flash-corpus/ruffle/tests/tests/swfs");
    if direct.is_dir() {
        return Some(direct);
    }
    let dot_git = root.join(".git");
    if dot_git.is_file()
        && let Ok(contents) = std::fs::read_to_string(&dot_git)
        && let Some(gitdir_raw) = contents.lines().find_map(|l| l.strip_prefix("gitdir:"))
    {
        let gitdir = PathBuf::from(gitdir_raw.trim());
        if let Some(main_repo) = gitdir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
        {
            let shared = main_repo.join("target/internet-flash-corpus/ruffle/tests/tests/swfs");
            if shared.is_dir() {
                return Some(shared);
            }
        }
    }
    None
}

struct TempSwfFile(PathBuf);

impl TempSwfFile {
    fn write(label: &str, bytes: &[u8]) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-t471-{label}-{}-{}.swf",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).expect("write temp swf");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempSwfFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct SwfWriter {
    buf: Vec<u8>,
    bit_pos: u8,
    cur_byte: u8,
}

impl SwfWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            bit_pos: 0,
            cur_byte: 0,
        }
    }

    fn align(&mut self) {
        if self.bit_pos > 0 {
            self.buf.push(self.cur_byte);
            self.bit_pos = 0;
            self.cur_byte = 0;
        }
    }

    fn write_ub(&mut self, val: u32, nbits: u8) {
        for i in (0..nbits).rev() {
            let bit = ((val >> i) & 1) as u8;
            self.cur_byte |= bit << (7 - self.bit_pos);
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.buf.push(self.cur_byte);
                self.bit_pos = 0;
                self.cur_byte = 0;
            }
        }
    }

    fn write_sb(&mut self, val: i32, nbits: u8) {
        let mask = if nbits >= 32 {
            u32::MAX
        } else {
            (1u32 << nbits) - 1
        };
        self.write_ub((val as u32) & mask, nbits);
    }

    fn u8(&mut self, v: u8) {
        self.align();
        self.buf.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, slice: &[u8]) {
        self.align();
        self.buf.extend_from_slice(slice);
    }

    fn c_str(&mut self, s: &str) {
        self.align();
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    fn rect(&mut self, xmin: i32, xmax: i32, ymin: i32, ymax: i32) {
        self.align();
        self.write_ub(15, 5);
        self.write_sb(xmin, 15);
        self.write_sb(xmax, 15);
        self.write_sb(ymin, 15);
        self.write_sb(ymax, 15);
        self.align();
    }

    fn translate_matrix(&mut self, tx_twips: i32, ty_twips: i32) {
        self.align();
        self.write_ub(0, 1); // HasScale = 0
        self.write_ub(0, 1); // HasRotate = 0
        self.write_ub(14, 5);
        self.write_sb(tx_twips, 14);
        self.write_sb(ty_twips, 14);
        self.align();
    }

    fn full_matrix_fixed16(
        &mut self,
        scale_x: i32,
        scale_y: i32,
        rotate_skew0: i32,
        rotate_skew1: i32,
        tx_twips: i32,
        ty_twips: i32,
    ) {
        self.align();
        self.write_ub(1, 1); // HasScale = 1
        self.write_ub(24, 5);
        self.write_sb(scale_x, 24);
        self.write_sb(scale_y, 24);
        self.write_ub(1, 1); // HasRotate = 1
        self.write_ub(24, 5);
        self.write_sb(rotate_skew0, 24);
        self.write_sb(rotate_skew1, 24);
        self.write_ub(16, 5);
        self.write_sb(tx_twips, 16);
        self.write_sb(ty_twips, 16);
        self.align();
    }

    fn tag(&mut self, code: u16, payload: &[u8]) {
        self.align();
        if payload.len() < 63 {
            self.u16((code << 6) | (payload.len() as u16));
        } else {
            self.u16((code << 6) | 0x3F);
            self.u32(payload.len() as u32);
        }
        self.bytes(payload);
    }

    fn into_swf(mut self, version: u8, width_px: i32, height_px: i32, frames: u16) -> Vec<u8> {
        self.tag(0, &[]);
        let mut body = SwfWriter::new();
        body.rect(0, width_px * 20, 0, height_px * 20);
        body.u16(24 << 8); // 24 fps
        body.u16(frames);
        body.bytes(&self.buf);

        let total_len = (8 + body.buf.len()) as u32;
        let mut out = Vec::with_capacity(total_len as usize);
        out.extend_from_slice(b"FWS");
        out.push(version);
        out.extend_from_slice(&total_len.to_le_bytes());
        out.extend_from_slice(&body.buf);
        out
    }
}

fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(raw).expect("zlib write");
    enc.finish().expect("zlib finish")
}

/// Build a `SCREENVIDEOPACKET` (`codec_id == 3`) with `16x16` blocks (`block_w = 16, block_h = 16`).
/// `blocks` is ordered bottom-to-top (`br = 0..rows`), left-to-right (`bc = 0..cols`);
/// `None` encodes `DataSize = 0` (unchanged inter-frame delta block), `Some([r, g, b])` encodes
/// a solid `cur_w * cur_h` BGR24 zlib-compressed block.
fn encode_screen_video_solid_blocks_packet(
    width: u16,
    height: u16,
    blocks: &[Option<[u8; 3]>],
) -> Vec<u8> {
    let cols = usize::from(width).div_ceil(16);
    let rows = usize::from(height).div_ceil(16);
    assert_eq!(blocks.len(), cols * rows);
    let mut pkt = Vec::new();
    // BlockWidth = 0 ((0+1)*16 = 16), ImageWidth = width
    pkt.extend_from_slice(&(width & 0x0FFF).to_be_bytes());
    // BlockHeight = 0 ((0+1)*16 = 16), ImageHeight = height
    pkt.extend_from_slice(&(height & 0x0FFF).to_be_bytes());

    for br in 0..rows {
        let cur_h = 16usize.min(usize::from(height) - br * 16);
        for bc in 0..cols {
            let cur_w = 16usize.min(usize::from(width) - bc * 16);
            match blocks[br * cols + bc] {
                None => {
                    pkt.extend_from_slice(&0u16.to_be_bytes());
                }
                Some([r, g, b]) => {
                    let mut bgr = Vec::with_capacity(cur_w * cur_h * 3);
                    for _ in 0..(cur_w * cur_h) {
                        bgr.extend_from_slice(&[b, g, r]);
                    }
                    let z = zlib_compress(&bgr);
                    pkt.extend_from_slice(&(z.len() as u16).to_be_bytes());
                    pkt.extend_from_slice(&z);
                }
            }
        }
    }
    pkt
}

/// Build a `SCREEN2VIDEOPACKET` (`codec_id == 6`) single-block `16x16` BGR24 keyframe.
fn encode_screen_video_v2_solid_keyframe(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.extend_from_slice(&16u16.to_be_bytes());
    pkt.extend_from_slice(&16u16.to_be_bytes());
    pkt.push(0x02); // HasIframeImage = 1, HasPaletteInfo = 0
    let mut bgr = Vec::with_capacity(16 * 16 * 3);
    for _ in 0..(16 * 16) {
        bgr.extend_from_slice(&[b, g, r]);
    }
    let z = zlib_compress(&bgr);
    let block_len = (1 + z.len()) as u16;
    pkt.extend_from_slice(&block_len.to_be_bytes());
    pkt.push(0x00); // IMAGEBLOCKHEADER: 24-bit BGR, no diff, no zlib prime
    pkt.extend_from_slice(&z);
    pkt
}

fn sample_rgba(buf: &[u8], stride_w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride_w + x) * 4) as usize;
    [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
}

fn place_char(depth: u16, cid: u16, tx_px: i32, ty_px: i32, ratio: Option<u16>) -> Vec<u8> {
    let mut p = SwfWriter::new();
    let flags = 0x06 | if ratio.is_some() { 0x10 } else { 0 };
    p.u8(flags);
    p.u16(depth);
    p.u16(cid);
    p.translate_matrix(tx_px * 20, ty_px * 20);
    if let Some(r) = ratio {
        p.u16(r);
    }
    p.buf
}

#[test]
fn t471_embedded_screen_video_flv_mux_stage_projection_seek_restart_and_honest_warnings() {
    // 1. Build a 5-frame 32x32 ScreenVideo SWF (2x2 = 4 blocks of 16x16):
    //    - Frame 0: intra keyframe (BL=Blue, BR=Yellow, TL=Red, TR=Green)
    //    - Frame 1: inter-frame delta (only TR changes to Magenta; 3 blocks DataSize=0)
    //    - Frame 2: inter-frame delta (only BL changes to Cyan; 3 blocks DataSize=0)
    //    - Frame 3: second intra keyframe (all 4 blocks = Orange)
    //    - Frame 4: inter-frame delta (only BR changes to White; 3 blocks DataSize=0)
    // Plus a second embedded ScreenVideo v2 (codec_id = 6) stream (#11) and Sorenson H.263 (#12)
    // to verify multi-codec FLV v1 muxing with zero warnings!
    let mut swf = SwfWriter::new();

    // DefineVideoStream #10: 5 frames, 32x32, smoothing=0, codec_id=3 (ScreenVideo)
    let mut def10 = SwfWriter::new();
    def10.u16(10);
    def10.u16(5);
    def10.u16(32);
    def10.u16(32);
    def10.u8(0x02); // deblocking=1, smoothing=0
    def10.u8(3);
    swf.tag(60, &def10.buf);

    // DefineVideoStream #11: 1 frame, 16x16, smoothing=1, codec_id=6 (ScreenVideo v2)
    let mut def11 = SwfWriter::new();
    def11.u16(11);
    def11.u16(1);
    def11.u16(16);
    def11.u16(16);
    def11.u8(0x01); // smoothing=1
    def11.u8(6);
    swf.tag(60, &def11.buf);

    // DefineVideoStream #12: 1 frame, 64x48, codec_id=2 (Sorenson H.263 with valid PictureStartCode)
    let mut def12 = SwfWriter::new();
    def12.u16(12);
    def12.u16(1);
    def12.u16(64);
    def12.u16(48);
    def12.u8(0x00);
    def12.u8(2);
    swf.tag(60, &def12.buf);

    let blue = [0u8, 64, 255];
    let yellow = [255u8, 220, 0];
    let red = [240u8, 20, 40];
    let green = [20u8, 210, 60];
    let magenta = [220u8, 30, 210];
    let cyan = [0u8, 215, 235];
    let orange = [250u8, 130, 15];
    let white = [250u8, 250, 250];
    let teal_v2 = [10u8, 180, 170];

    let pkt0 = encode_screen_video_solid_blocks_packet(
        32,
        32,
        &[Some(blue), Some(yellow), Some(red), Some(green)],
    );
    let pkt1 = encode_screen_video_solid_blocks_packet(32, 32, &[None, None, None, Some(magenta)]);
    let pkt2 = encode_screen_video_solid_blocks_packet(32, 32, &[Some(cyan), None, None, None]);
    let pkt3 = encode_screen_video_solid_blocks_packet(
        32,
        32,
        &[Some(orange), Some(orange), Some(orange), Some(orange)],
    );
    let pkt4 = encode_screen_video_solid_blocks_packet(32, 32, &[None, Some(white), None, None]);

    let push_vf = |w: &mut SwfWriter, stream_id: u16, frame_num: u16, data: &[u8]| {
        let mut vf = SwfWriter::new();
        vf.u16(stream_id);
        vf.u16(frame_num);
        vf.bytes(data);
        w.tag(61, &vf.buf);
    };

    // Place #10 at (10, 10) depth 1, #11 at (50, 10) depth 2, #12 at (80, 10) depth 3
    let move_ratio = |depth: u16, ratio: u16| -> Vec<u8> {
        let mut p = SwfWriter::new();
        p.u8(0x11); // HasRatio(0x10) | Move(0x01)
        p.u16(depth);
        p.u16(ratio);
        p.buf
    };

    // Valid Sorenson H.263 intra packet header: PictureStartCode(17b=1), Version(5b=1), TempRef(8b=0),
    // PictureSize(3b=0 => 8b width=64, 8b height=48), PictureType(2b=0 intra)
    let mut h263 = SwfWriter::new();
    h263.write_ub(1, 17);
    h263.write_ub(1, 5);
    h263.write_ub(0, 8);
    h263.write_ub(0, 3);
    h263.write_ub(64, 8);
    h263.write_ub(48, 8);
    h263.write_ub(0, 2);
    h263.write_ub(0, 5);
    h263.align();

    // Frame 0
    swf.tag(26, &place_char(1, 10, 10, 10, Some(0)));
    swf.tag(26, &place_char(2, 11, 50, 10, Some(0)));
    swf.tag(26, &place_char(3, 12, 80, 10, Some(0)));
    push_vf(&mut swf, 10, 0, &pkt0);
    push_vf(
        &mut swf,
        11,
        0,
        &encode_screen_video_v2_solid_keyframe(teal_v2[0], teal_v2[1], teal_v2[2]),
    );
    push_vf(&mut swf, 12, 0, &h263.buf);
    swf.tag(1, &[]);

    // Frames 1..=4
    for (f, pkt) in [(1u16, &pkt1), (2, &pkt2), (3, &pkt3), (4, &pkt4)] {
        swf.tag(26, &move_ratio(1, f));
        push_vf(&mut swf, 10, f, pkt);
        swf.tag(1, &[]);
    }

    let swf_bytes = swf.into_swf(8, 160, 80, 5);
    let tmp = TempSwfFile::write("screenvideo", &swf_bytes);

    let report = inspect(tmp.path()).expect("inspect embedded video SWF");
    assert!(
        report.diagnostics.iter().all(|d| {
            d.code != "swf_video_not_imported"
                && d.code != "swf_video_viewport"
                && d.code != "swf_media_budget"
        }),
        "valid embedded SWF video streams must import without video approximation warnings, got: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import embedded video SWF");
    project
        .validate()
        .expect("imported video project validates");

    for vid in [10u16, 11, 12] {
        let res_id = format!("swf-video-{vid}");
        let res = project
            .resources
            .iter()
            .find(|r| r.id == res_id)
            .unwrap_or_else(|| panic!("missing FLV resource {res_id}"));
        assert_eq!(res.media_type, "video/x-flv");
        let flv_bytes = res.decoded_bytes().expect("decode FLV resource bytes");
        assert!(
            flv_bytes.len() > 24 && &flv_bytes[0..5] == b"FLV\x01\x01",
            "{res_id} must be a valid FLV v1 container"
        );
    }

    // Verify coalesced timeline span + ElementKind::Video + TimelineVideoSpan trim metadata on root
    let root_sym = project.symbol(&project.root).expect("root symbol");
    let d1_layer = &root_sym.timeline.layers[0];
    assert_eq!(
        d1_layer.keyframes.len(),
        1,
        "sequential +1 ratio steps on stationary InlineVideo must coalesce into 1 continuous span"
    );
    assert_eq!(
        d1_layer.keyframes[0].elements[0].kind,
        ElementKind::Video {
            width: 32.0,
            height: 32.0
        }
    );
    let vspan = d1_layer.keyframes[0]
        .video
        .as_ref()
        .expect("Keyframe.video TimelineVideoSpan attached");
    assert_eq!(vspan.resource, "swf-video-10");
    assert_eq!(vspan.sync, VideoSyncMode::Stream);
    assert_eq!(vspan.start_offset_ms, 0);
    assert_eq!(vspan.end_offset_ms, Some(208)); // round(5 * 1000 / 24) = 208 ms

    // Verify SwfBytecodePlayer live ScreenVideo decoding & Stage projection across step, seek, restart!
    let expected_quadrants = [
        // (TL, TR, BL, BR)
        (red, green, blue, yellow),
        (red, magenta, blue, yellow),
        (red, magenta, cyan, yellow),
        (orange, orange, orange, orange),
        (orange, orange, orange, white),
    ];

    let mut player =
        SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer::from_bytes");
    let check_stage_at = |player: &SwfBytecodePlayer, frame_idx: usize| {
        let (tl, tr, bl, br) = expected_quadrants[frame_idx];
        let (vw, vh, vrgba) = player
            .video_frame_rgba(10)
            .expect("decoded ScreenVideo #10 RGBA available");
        assert_eq!((vw, vh), (32, 32));
        assert_eq!(&sample_rgba(vrgba, 32, 4, 4)[..3], &tl);
        assert_eq!(&sample_rgba(vrgba, 32, 24, 4)[..3], &tr);
        assert_eq!(&sample_rgba(vrgba, 32, 4, 24)[..3], &bl);
        assert_eq!(&sample_rgba(vrgba, 32, 24, 24)[..3], &br);

        // Check ScreenVideo v2 (#11) on frame 0+
        let (v2w, v2h, v2rgba) = player
            .video_frame_rgba(11)
            .expect("decoded ScreenVideo v2 #11 RGBA available");
        assert_eq!((v2w, v2h), (16, 16));
        assert_eq!(&sample_rgba(v2rgba, 16, 8, 8)[..3], &teal_v2);

        // Render full Stage snapshot through project_with_display_list_overlay + CorePlayer!
        let overlay =
            player.project_with_display_list_overlay(&project, &project.root, frame_idx as u32);
        overlay
            .validate()
            .expect("projected video overlay validates");
        let mut core_player = CorePlayer::new(&overlay);
        core_player.seek(&overlay, frame_idx as u32);
        let stage_rgba = core_player
            .render_snapshot_rgba(&overlay, 160, 80)
            .expect("render stage snapshot");
        // Video #10 is placed at (10, 10): TL=(14,14), TR=(34,14), BL=(14,34), BR=(34,34)
        assert_eq!(
            &sample_rgba(&stage_rgba, 160, 14, 14)[..3],
            &tl,
            "stage TL mismatch at frame {frame_idx}"
        );
        assert_eq!(
            &sample_rgba(&stage_rgba, 160, 34, 14)[..3],
            &tr,
            "stage TR mismatch at frame {frame_idx}"
        );
        assert_eq!(
            &sample_rgba(&stage_rgba, 160, 14, 34)[..3],
            &bl,
            "stage BL mismatch at frame {frame_idx}"
        );
        assert_eq!(
            &sample_rgba(&stage_rgba, 160, 34, 34)[..3],
            &br,
            "stage BR mismatch at frame {frame_idx}"
        );
        // Video #11 (ScreenVideo v2) is placed at (50, 10) -> sample (58, 18)
        assert_eq!(
            &sample_rgba(&stage_rgba, 160, 58, 18)[..3],
            &teal_v2,
            "stage ScreenVideo v2 mismatch at frame {frame_idx}"
        );

        // Also check evaluate_stage_scene (non-cloning Stage path)
        let scene = player.evaluate_stage_scene(&project, &project.root, frame_idx as u32, true);
        assert!(
            !scene.draws.is_empty(),
            "evaluate_stage_scene must project non-empty video draw items at frame {frame_idx}"
        );
    };

    for f in 0..5 {
        let st = player.step_frame();
        assert_eq!(st.frame, f as u32);
        check_stage_at(&player, f);
    }

    // Backward seek to delta frame 2 (replays from keyframe 0 + deltas 1..=2)
    let s2 = player.seek_to_frame(2);
    assert_eq!(s2.frame, 2);
    check_stage_at(&player, 2);

    // Forward seek to delta frame 4 (replays through second keyframe 3 + delta 4)
    let s4 = player.seek_to_frame(4);
    assert_eq!(s4.frame, 4);
    check_stage_at(&player, 4);

    // Backward seek to delta frame 1 (replays from keyframe 0 + delta 1, clearing keyframe 3's orange!)
    let s1 = player.seek_to_frame(1);
    assert_eq!(s1.frame, 1);
    check_stage_at(&player, 1);

    // Restart back to frame 0
    let s0 = player.restart();
    assert_eq!(s0.frame, 0);
    check_stage_at(&player, 0);

    // 2a. Pure ActionScript `new Video(w, h)` / `FLVPlayback` viewport (`num_frames == 0 && codec_id == 0`)
    // is losslessly represented by `ElementKind::Video { width, height }` and must emit 0 warnings!
    let mut pure_vp_swf = SwfWriter::new();
    let mut pure_vp_def = SwfWriter::new();
    pure_vp_def.u16(29);
    pure_vp_def.u16(0); // num_frames = 0
    pure_vp_def.u16(320);
    pure_vp_def.u16(180);
    pure_vp_def.u8(0);
    pure_vp_def.u8(0); // codec_id = 0
    pure_vp_swf.tag(60, &pure_vp_def.buf);
    pure_vp_swf.tag(26, &place_char(1, 29, 0, 0, None));
    pure_vp_swf.tag(1, &[]);
    let pure_vp_tmp = TempSwfFile::write("pure-video-vp", &pure_vp_swf.into_swf(8, 320, 180, 1));
    let pure_vp_report = inspect(pure_vp_tmp.path()).expect("inspect pure AS Video viewport SWF");
    assert!(
        pure_vp_report
            .diagnostics
            .iter()
            .all(|d| d.code != "swf_video_viewport"),
        "pure AS Video(w, h) / FLVPlayback viewport (num_frames=0, codec_id=0) must emit 0 swf_video_viewport warnings, got: {:?}",
        pure_vp_report.diagnostics
    );

    // 2b. Verify honest located warning on 0-VideoFrame DefineVideoStream with codec_id != 0 (external/unembedded stream)
    let mut ext_swf = SwfWriter::new();
    let mut ext_def = SwfWriter::new();
    ext_def.u16(30);
    ext_def.u16(0); // 0 embedded VideoFrame tags
    ext_def.u16(320);
    ext_def.u16(180);
    ext_def.u8(0);
    ext_def.u8(2); // Sorenson H.263 declared, 0 VideoFrame tags present
    ext_swf.tag(60, &ext_def.buf);
    ext_swf.tag(26, &place_char(1, 30, 0, 0, None));
    ext_swf.tag(1, &[]);
    let ext_tmp = TempSwfFile::write("ext-video", &ext_swf.into_swf(8, 320, 180, 1));
    let ext_report = inspect(ext_tmp.path()).expect("inspect 0-frame video SWF");
    let ext_proj = import_project(ext_tmp.path()).expect("import 0-frame video SWF");
    ext_proj
        .validate()
        .expect("0-frame video project validates");
    let vp_warns: Vec<_> = ext_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_video_viewport")
        .collect();
    assert_eq!(
        vp_warns.len(),
        1,
        "0-frame codec_id!=0 DefineVideoStream must emit exactly 1 honest located warning: {vp_warns:?}"
    );
    assert!(
        vp_warns[0].location.contains("#tag=60@") && vp_warns[0].location.contains("swf-sym-30"),
        "warning must carry exact tag & symbol location: {:?}",
        vp_warns[0]
    );

    // 3. Unknown codec_id (99) with non-empty VideoFrame also emits honest located warning without FLV muxing
    let mut unk_swf = SwfWriter::new();
    let mut unk_def = SwfWriter::new();
    unk_def.u16(31);
    unk_def.u16(1);
    unk_def.u16(64);
    unk_def.u16(64);
    unk_def.u8(0);
    unk_def.u8(99); // unknown codec_id
    unk_swf.tag(60, &unk_def.buf);
    let mut unk_vf = SwfWriter::new();
    unk_vf.u16(31);
    unk_vf.u16(0);
    unk_vf.bytes(&[0xAA, 0xBB, 0xCC, 0xDD]);
    unk_swf.tag(61, &unk_vf.buf);
    unk_swf.tag(26, &place_char(1, 31, 0, 0, None));
    unk_swf.tag(1, &[]);
    let unk_tmp = TempSwfFile::write("unk-video", &unk_swf.into_swf(8, 64, 64, 1));
    let unk_report = inspect(unk_tmp.path()).expect("inspect unknown codec video SWF");
    let unk_proj = import_project(unk_tmp.path()).expect("import unknown codec video SWF");
    unk_proj
        .validate()
        .expect("unknown codec video project validates");
    assert!(
        unk_proj.resource("swf-video-31").is_none(),
        "unknown codec_id must not create an invalid FLV resource"
    );
    assert!(
        unk_report
            .diagnostics
            .iter()
            .any(|d| d.code == "swf_video_viewport" && d.location.contains("swf-sym-31")),
        "unknown codec_id must emit honest located swf_video_viewport warning: {:?}",
        unk_report.diagnostics
    );
}

#[test]
fn t471_large_320x240_screen_video_stage_scene_downsampling_and_sv2_flags() {
    // 1. 320x240 (76,800 px > 16,384 MAX_SCANLINE_VIDEO_PIXELS):
    //    20 cols x 15 rows of 16x16 blocks: top half Emerald [16, 185, 129], bottom half Coral [244, 63, 94].
    let emerald = [16u8, 185, 129];
    let coral = [244u8, 63, 94];
    let mut blocks = Vec::with_capacity(20 * 15);
    for br in 0..15 {
        // br = 0..7 is bottom half (Coral); br = 7..15 is top half (Emerald)
        let color = if br < 7 { coral } else { emerald };
        for _ in 0..20 {
            blocks.push(Some(color));
        }
    }
    let pkt_320x240 = encode_screen_video_solid_blocks_packet(320, 240, &blocks);

    let mut swf = SwfWriter::new();
    let mut def = SwfWriter::new();
    def.u16(40);
    def.u16(1);
    def.u16(320);
    def.u16(240);
    def.u8(0);
    def.u8(3); // ScreenVideo v1
    swf.tag(60, &def.buf);

    let mut vf = SwfWriter::new();
    vf.u16(40);
    vf.u16(0);
    vf.bytes(&pkt_320x240);
    swf.tag(61, &vf.buf);
    swf.tag(26, &place_char(1, 40, 0, 0, None));
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 320, 240, 1);
    let tmp = TempSwfFile::write("sv-320x240", &swf_bytes);
    let project = import_project(tmp.path()).expect("import 320x240 ScreenVideo");
    project.validate().expect("320x240 project validates");

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("player");
    player.step_frame();
    let (vw, vh, vrgba) = player.video_frame_rgba(40).expect("320x240 RGBA decoded");
    assert_eq!((vw, vh), (320, 240));
    assert_eq!(&sample_rgba(vrgba, 320, 160, 40)[..3], &emerald);
    assert_eq!(&sample_rgba(vrgba, 320, 160, 200)[..3], &coral);

    // Live Editor Stage path (`evaluate_stage_scene`) must downsample into <= 16,384 non-empty runs!
    let scene = player.evaluate_stage_scene(&project, &project.root, 0, true);
    assert!(
        !scene.draws.is_empty() && scene.draws.len() <= 16_384,
        "expected non-empty bounded Stage draws for 320x240 ScreenVideo, got {} draws",
        scene.draws.len()
    );

    // 2. ScreenVideo v2 (`codec_id == 6`) IMAGEBLOCKV2 flags byte validation:
    //    `0x00` (BGR24, no diff, no zlib prime) must decode; unsupported diff/prime/color-depth/reserved bits must reject cleanly!
    let make_sv2_swf_with_block_flags = |block_flags: u8| -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(&16u16.to_be_bytes());
        pkt.extend_from_slice(&16u16.to_be_bytes());
        pkt.push(0x02); // HasIframeImage = 1, HasPaletteInfo = 0
        let mut bgr = Vec::with_capacity(16 * 16 * 3);
        for _ in 0..(16 * 16) {
            bgr.extend_from_slice(&[200, 100, 50]);
        }
        let z = zlib_compress(&bgr);
        let block_len = (1 + z.len()) as u16;
        pkt.extend_from_slice(&block_len.to_be_bytes());
        pkt.push(block_flags);
        pkt.extend_from_slice(&z);

        let mut s = SwfWriter::new();
        let mut d = SwfWriter::new();
        d.u16(50);
        d.u16(1);
        d.u16(16);
        d.u16(16);
        d.u8(0);
        d.u8(6); // ScreenVideo v2
        s.tag(60, &d.buf);
        let mut v = SwfWriter::new();
        v.u16(50);
        v.u16(0);
        v.bytes(&pkt);
        s.tag(61, &v.buf);
        s.tag(26, &place_char(1, 50, 0, 0, None));
        s.tag(1, &[]);
        s.into_swf(8, 16, 16, 1)
    };

    let mut ok_p = SwfBytecodePlayer::from_bytes(&make_sv2_swf_with_block_flags(0x00)).unwrap();
    ok_p.step_frame();
    let (_, _, ok_rgba) = ok_p
        .video_frame_rgba(50)
        .expect("BGR24 0x00 ScreenVideo v2 block must decode");
    assert_eq!(&sample_rgba(ok_rgba, 16, 8, 8)[..3], &[50, 100, 200]);

    for bad_flag in [
        0x01u8, // ZlibPrimeCompressPrevious
        0x02,   // ZlibPrimeCompressCurrent
        0x04,   // HasDiffBlocks
        0x08,   // ColorDepth = 1 (non-BGR24)
        0x10,   // ColorDepth = 2 (non-BGR24)
        0x20,   // Reserved bit
    ] {
        let mut bad_p =
            SwfBytecodePlayer::from_bytes(&make_sv2_swf_with_block_flags(bad_flag)).unwrap();
        bad_p.step_frame();
        assert!(
            bad_p.video_frame_rgba(50).is_none(),
            "ScreenVideo v2 block with unsupported IMAGEBLOCKV2 flag 0x{bad_flag:02X} must fail closed"
        );
    }
}

#[test]
fn t471_start_sound_and_start_sound2_trim_windows_and_custom_stereo_envelope_round_trip() {
    let mut swf = SwfWriter::new();

    // DefineSound #5: format=3 (uncompressed LE PCM), rate=3 (44.1 kHz), 16-bit, stereo (0x3F)
    // 44,100 stereo sample frames = 1.0s = 176,400 bytes
    let mut snd5 = SwfWriter::new();
    snd5.u16(5);
    snd5.u8(0x3F);
    snd5.u32(44_100);
    snd5.bytes(&vec![0u8; 44_100 * 4]);
    swf.tag(14, &snd5.buf);

    // DefineSound #6 for StartSound2 class trigger (`audio.StereoCue`)
    let mut snd6 = SwfWriter::new();
    snd6.u16(6);
    snd6.u8(0x3F);
    snd6.u32(44_100);
    snd6.bytes(&vec![0u8; 44_100 * 4]);
    swf.tag(14, &snd6.buf);

    let mut sym_cls = SwfWriter::new();
    sym_cls.u16(1);
    sym_cls.u16(6);
    sym_cls.c_str("audio.StereoCue");
    swf.tag(76, &sym_cls.buf);

    // StartSound (tag 15) on frame 0:
    // flags = 0x1F (SyncNoMultiple=Start | HasEnvelope | HasLoops | HasOutPoint | HasInPoint)
    // in_point = 11,025 (0.25s), out_point = 33,075 (0.75s) -> active window = 22,050 samples (0.50s)
    // 4 envelope points anchored at [11_025, 16_537, 27_562, 33_075] -> normalized [0.0, ~0.25, ~0.75, 1.0]
    let mut start15 = SwfWriter::new();
    start15.u16(5);
    start15.u8(0x1F);
    start15.u32(11_025); // in_point = 0.25s
    start15.u32(33_075); // out_point = 0.75s
    start15.u16(2); // loops = 2
    start15.u8(4); // 4 custom stereo envelope points
    for (pos44, left, right) in [
        (11_025u32, 8_192u16, 32_768u16),  // pos 0.00: L=0.25, R=1.00
        (16_537u32, 16_384u16, 24_576u16), // pos 0.25: L=0.50, R=0.75
        (27_562u32, 24_576u16, 8_192u16),  // pos 0.75: L=0.75, R=0.25
        (33_075u32, 32_768u16, 0u16),      // pos 1.00: L=1.00, R=0.00
    ] {
        start15.u32(pos44);
        start15.u16(left);
        start15.u16(right);
    }
    swf.tag(15, &start15.buf);
    swf.tag(1, &[]);

    // Frame 1: StartSound2 (tag 89) referencing `audio.StereoCue` with trim [0.10s .. 0.60s] + 3-point envelope
    let mut start89 = SwfWriter::new();
    start89.c_str("audio.StereoCue");
    start89.u8(0x0B); // Event sync | HasEnvelope(0x08) | HasOutPoint(0x02) | HasInPoint(0x01)
    start89.u32(4_410); // in_point = 0.10s
    start89.u32(26_460); // out_point = 0.60s (window = 22,050 samples)
    start89.u8(3);
    for (pos44, left, right) in [
        (4_410u32, 0u16, 32_768u16),
        (15_435u32, 16_384u16, 16_384u16),
        (26_460u32, 32_768u16, 4_096u16),
    ] {
        start89.u32(pos44);
        start89.u16(left);
        start89.u16(right);
    }
    swf.tag(89, &start89.buf);
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(9, 100, 100, 2);
    let tmp = TempSwfFile::write("sound-envelope", &swf_bytes);

    let report = inspect(tmp.path()).expect("inspect sound SWF");
    assert!(
        report.diagnostics.iter().all(|d| {
            d.code != "swf_frame_sound"
                && d.code != "swf_sound_approximated"
                && d.code != "swf_media_not_imported"
        }),
        "representable StartSound/StartSound2 trim + stereo envelope must emit zero warnings, got: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import sound SWF");
    project
        .validate()
        .expect("project with format-7 sound envelope validates");
    assert!(project.format_version >= 7);

    let root = project.symbol(&project.root).expect("root");
    let find_sound_at = |frame: u32, res: &str| {
        root.timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .filter(|k| k.frame == frame)
            .filter_map(|k| k.sound.as_ref())
            .find(|s| s.resource == res)
            .unwrap_or_else(|| panic!("missing FrameSound {res} at frame {frame}"))
    };

    let fs5 = find_sound_at(0, "swf-sound-5");
    assert_eq!(fs5.sync, SoundSync::Start);
    assert_eq!(fs5.loops, 2);
    assert_eq!(fs5.in_point, Some(0.25));
    assert_eq!(fs5.out_point, Some(0.75));
    assert_eq!(fs5.effect, SoundEffect::Custom);
    let env5 = fs5.envelope.as_ref().expect("custom envelope present");
    assert_eq!(env5.len(), 4);
    assert!(
        (env5[0][0] - 0.0).abs() < 1e-4
            && (env5[0][1] - 0.25).abs() < 1e-4
            && (env5[0][2] - 1.0).abs() < 1e-4
    );
    assert!(
        (env5[1][0] - 0.25).abs() < 1e-3
            && (env5[1][1] - 0.50).abs() < 1e-4
            && (env5[1][2] - 0.75).abs() < 1e-4
    );
    assert!(
        (env5[2][0] - 0.75).abs() < 1e-3
            && (env5[2][1] - 0.75).abs() < 1e-4
            && (env5[2][2] - 0.25).abs() < 1e-4
    );
    assert!(
        (env5[3][0] - 1.0).abs() < 1e-4
            && (env5[3][1] - 1.0).abs() < 1e-4
            && (env5[3][2] - 0.0).abs() < 1e-4
    );

    let fs6 = find_sound_at(1, "swf-sound-6");
    assert_eq!(fs6.sync, SoundSync::Event);
    assert_eq!(fs6.in_point, Some(0.1));
    assert_eq!(fs6.out_point, Some(0.6));
    assert_eq!(fs6.effect, SoundEffect::Custom);
    let env6 = fs6
        .envelope
        .as_ref()
        .expect("StartSound2 custom envelope present");
    assert_eq!(env6.len(), 3);
    assert!(
        (env6[0][0] - 0.0).abs() < 1e-4
            && (env6[0][1] - 0.0).abs() < 1e-4
            && (env6[0][2] - 1.0).abs() < 1e-4
    );
    assert!(
        (env6[1][0] - 0.5).abs() < 1e-4
            && (env6[1][1] - 0.5).abs() < 1e-4
            && (env6[1][2] - 0.5).abs() < 1e-4
    );
    assert!(
        (env6[2][0] - 1.0).abs() < 1e-4
            && (env6[2][1] - 1.0).abs() < 1e-4
            && (env6[2][2] - 0.125).abs() < 1e-4
    );

    // Also verify SwfBytecodePlayer emits matching SoundCommand::Play on frames 0 and 1!
    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("player");
    player.step_frame();
    let cmds0 = player.drain_sound_commands(&project);
    assert!(
        matches!(
            cmds0.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                loops: 2,
                in_point: Some(inp),
                out_point: Some(outp),
                envelope,
                ..
            }] if resource == "swf-sound-5"
                && (*inp - 0.25).abs() < 1e-6
                && (*outp - 0.75).abs() < 1e-6
                && envelope.len() == 4
        ),
        "unexpected frame 0 sound commands: {cmds0:?}"
    );
    player.step_frame();
    let cmds1 = player.drain_sound_commands(&project);
    assert!(
        matches!(
            cmds1.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Event,
                in_point: Some(inp),
                out_point: Some(outp),
                envelope,
                ..
            }] if resource == "swf-sound-6"
                && (*inp - 0.1).abs() < 1e-6
                && (*outp - 0.6).abs() < 1e-6
                && envelope.len() == 3
        ),
        "unexpected frame 1 sound commands: {cmds1:?}"
    );
}

#[test]
fn t471_rotated_and_sheared_affine_bitmap_fills_0x40_to_0x43_import_and_render_pixels() {
    let mut swf = SwfWriter::new();
    // SetBackgroundColor = white (255, 255, 255)
    swf.tag(9, &[255, 255, 255]);

    // DefineBitsLossless2 (tag 36, bitmap #1): 16x16 ARGB (premultiplied, opaque alpha=255):
    //   u in 0..8,  v in 0..8  (TL): Pure Red   [255, 230, 20, 20]
    //   u in 8..16, v in 0..8  (TR): Pure Green [255, 20, 210, 40]
    //   u in 0..8,  v in 8..16 (BL): Pure Blue  [255, 25, 60, 240]
    //   u in 8..16, v in 8..16 (BR): Pure Gold  [255, 245, 200, 10]
    let mut raw_argb = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let [r, g, b] = match (x >= 8, y >= 8) {
                (false, false) => [230u8, 20, 20],
                (true, false) => [20u8, 210, 40],
                (false, true) => [25u8, 60, 240],
                (true, true) => [245u8, 200, 10],
            };
            raw_argb.extend_from_slice(&[255, r, g, b]);
        }
    }
    let mut lossless = SwfWriter::new();
    lossless.u16(1); // bitmap_id = 1
    lossless.u8(5); // 32-bit ARGB
    lossless.u16(16);
    lossless.u16(16);
    lossless.bytes(&zlib_compress(&raw_argb));
    swf.tag(36, &lossless.buf);

    // Rotated + sheared SWF bitmap matrix (in 16.16 fixed-point twips-per-pixel):
    // Desired pixel-space affine matrix M_px ([a, b, c, d, tx, ty] mapping bitmap (u,v) -> shape (x,y)):
    //   a =  2.0,  b =  0.5   (rotate_skew0 = 0.5 * 20 * 65536 != 0)
    //   c = -0.75, d =  1.5   (rotate_skew1 = -0.75 * 20 * 65536 != 0; note b != -c => true shear + rotation!)
    //   tx = 12.0, ty = 4.0
    let sx_fp = 2 * 20 * 65536; // a_twips = 40.0 -> a_px = 2.0
    let sy_fp = 30 * 65536; // d_twips = 30.0 -> d_px = 1.5
    let sk0_fp = 10 * 65536; // b_twips = 10.0 -> b_px = 0.5
    let sk1_fp = -15 * 65536; // c_twips = -15.0 -> c_px = -0.75
    let tx_tw = 12 * 20;
    let ty_tw = 4 * 20;

    // Build 4 DefineShape3 tags (#2..=#5) for fill types 0x40, 0x41, 0x42, 0x43
    // Using a 6-vertex clipped polygon (non-quad!) spanning [0..60] x [0..50] px (0..1200 x 0..1000 twips).
    for (idx, fill_type) in [0x40u8, 0x41, 0x42, 0x43].into_iter().enumerate() {
        let shape_id = (2 + idx) as u16;
        let mut sh = SwfWriter::new();
        sh.u16(shape_id);
        sh.rect(0, 1200, 0, 1000);
        sh.u8(1); // 1 fill style
        sh.u8(fill_type);
        sh.u16(1); // bitmap_id = 1
        sh.full_matrix_fixed16(sx_fp, sy_fp, sk0_fp, sk1_fp, tx_tw, ty_tw);
        sh.u8(0); // 0 line styles
        sh.write_ub(1, 4); // num_fill_bits = 1
        sh.write_ub(0, 4); // num_line_bits = 0

        // StyleChangeRecord: MoveTo(100, 0), fillStyle1 = 1
        sh.write_ub(0, 1);
        sh.write_ub(0x05, 5); // StateFillStyle1(0x04) | StateMoveTo(0x01)
        sh.write_ub(12, 5);
        sh.write_sb(100, 12);
        sh.write_sb(0, 12);
        sh.write_ub(1, 1); // fs1 = 1

        // 6-edge hexagon: (100,0)->(1100,0)->(1200,200)->(1200,1000)->(100,1000)->(0,800)->(100,0)
        let deltas = [
            (1000, 0),
            (100, 200),
            (0, 800),
            (-1100, 0),
            (-100, -200),
            (100, -800),
        ];
        for (dx, dy) in deltas {
            sh.write_ub(1, 1); // edge
            sh.write_ub(1, 1); // straight
            sh.write_ub(11, 4); // nbits = 13
            sh.write_ub(1, 1); // general line
            sh.write_sb(dx, 13);
            sh.write_sb(dy, 13);
        }
        sh.write_ub(0, 1);
        sh.write_ub(0, 5); // EndShapeRecord
        sh.align();
        swf.tag(32, &sh.buf);

        // Place each shape side-by-side at x = idx * 70 px, y = 5 px
        let mut pl = SwfWriter::new();
        pl.u8(0x06);
        pl.u16((idx + 1) as u16);
        pl.u16(shape_id);
        pl.translate_matrix((idx as i32) * 70 * 20, 5 * 20);
        swf.tag(26, &pl.buf);
    }
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 290, 70, 1);
    let tmp = TempSwfFile::write("sheared-bitmap-fills", &swf_bytes);

    let report = inspect(tmp.path()).expect("inspect sheared bitmap fills");
    assert!(
        report.diagnostics.iter().all(|d| {
            d.code != "swf_bitmap_fill_approximated"
                && d.code != "swf_shape_approximated"
                && d.code != "swf_bitmap_skipped"
        }),
        "sheared + rotated 0x40..=0x43 bitmap fills must import with zero approximation warnings: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import sheared bitmap fills");
    project
        .validate()
        .expect("sheared bitmap fill project validates");

    let expected_modes = [
        (2u16, true, true),   // 0x40: repeat=true,  smoothing=true
        (3u16, false, true),  // 0x41: repeat=false, smoothing=true
        (4u16, true, false),  // 0x42: repeat=true,  smoothing=false
        (5u16, false, false), // 0x43: repeat=false, smoothing=false
    ];
    for (shape_id, exp_repeat, exp_smooth) in expected_modes {
        let sym = project
            .symbol(&format!("swf-sym-{shape_id}"))
            .expect("shape symbol");
        let kf = &sym.timeline.layers[0].keyframes[0];
        assert_eq!(
            kf.bitmap_fills.len(),
            1,
            "shape #{shape_id} must carry AuthoredBitmapFill"
        );
        let bf = &kf.bitmap_fills[0];
        assert_eq!(
            bf.repeat, exp_repeat,
            "repeat mismatch on shape #{shape_id}"
        );
        assert_eq!(
            bf.smoothing, exp_smooth,
            "smoothing mismatch on shape #{shape_id}"
        );
        for (got, want) in bf.matrix.iter().zip([2.0, 0.5, -0.75, 1.5, 12.0, 4.0]) {
            assert!(
                (got - want).abs() < 1e-4,
                "matrix mismatch on shape #{shape_id}: got {:?}, expected [2.0, 0.5, -0.75, 1.5, 12.0, 4.0]",
                bf.matrix
            );
        }
    }

    // Verify rendered pixels in BOTH static CorePlayer AND SwfBytecodePlayer Stage overlay!
    // Forward transform M_px(u, v) = (2.0*u - 0.75*v + 12.0, 0.5*u + 1.5*v + 4.0):
    // - Pick (u=3.0, v=3.0) [TL Red quadrant]:
    //     local_x = 6.0 - 2.25 + 12.0 = 15.75 -> 16, local_y = 1.5 + 4.5 + 4.0 = 10.0 -> 10
    // - Pick (u=12.0, v=3.0) [TR Green quadrant]:
    //     local_x = 24.0 - 2.25 + 12.0 = 33.75 -> 34, local_y = 6.0 + 4.5 + 4.0 = 14.5 -> 15
    // - Pick (u=12.0, v=12.0) [BR Gold quadrant]:
    //     local_x = 24.0 - 9.0 + 12.0 = 27.0 -> 27, local_y = 6.0 + 18.0 + 4.0 = 28.0 -> 28
    // - Pick (u=20.0, v=3.0) [wrapped UV -> u mod 16 = 4.0 (TL Red) when repeat=true, clamped TR Green when repeat=false!]:
    //     local_x = 40.0 - 2.25 + 12.0 = 49.75 -> 50, local_y = 10.0 + 4.5 + 4.0 = 18.5 -> 19
    let static_player = CorePlayer::new(&project);
    let static_rgba = static_player
        .render_snapshot_rgba(&project, 290, 70)
        .expect("render static bitmap fills");
    let mut vm_player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("vm player");
    vm_player.step_frame();
    let vm_overlay = vm_player.project_with_display_list_overlay(&project, &project.root, 0);
    let vm_core = CorePlayer::new(&vm_overlay);
    let vm_rgba = vm_core
        .render_snapshot_rgba(&vm_overlay, 290, 70)
        .expect("render vm bitmap fills");

    for (label, rgba) in [("static", &static_rgba), ("swf_vm", &vm_rgba)] {
        for idx in 0..4u32 {
            let ox = idx * 70;
            let oy = 5;
            let px_red = sample_rgba(rgba, 290, ox + 16, oy + 10);
            assert!(
                px_red[0] > 180 && px_red[1] < 60 && px_red[2] < 60,
                "{label} shape #{idx} TL Red mismatch: {px_red:?}"
            );
            let px_green = sample_rgba(rgba, 290, ox + 34, oy + 15);
            assert!(
                px_green[1] > 160 && px_green[0] < 70 && px_green[2] < 80,
                "{label} shape #{idx} TR Green mismatch: {px_green:?}"
            );
            let px_gold = sample_rgba(rgba, 290, ox + 27, oy + 28);
            assert!(
                px_gold[0] > 200 && px_gold[1] > 160 && px_gold[2] < 60,
                "{label} shape #{idx} BR Gold mismatch: {px_gold:?}"
            );
        }
        // Check repeat (idx=0 -> 0x40, idx=2 -> 0x42 wraps u=20 -> u=4 Red) vs clipped (idx=1 -> 0x41, idx=3 -> 0x43 clamps u=20 -> u=15 Green)!
        for rep_idx in [0u32, 2] {
            let px_wrap = sample_rgba(rgba, 290, rep_idx * 70 + 50, 5 + 19);
            assert!(
                px_wrap[0] > 180 && px_wrap[1] < 60,
                "{label} repeating fill #{rep_idx} must tile to Red at u=20, got {px_wrap:?}"
            );
        }
        for clip_idx in [1u32, 3] {
            let px_clamp = sample_rgba(rgba, 290, clip_idx * 70 + 50, 5 + 19);
            assert!(
                px_clamp[1] > 160 && px_clamp[0] < 70,
                "{label} clipped fill #{clip_idx} must clamp edge texel Green at u=20, got {px_clamp:?}"
            );
        }
    }
}

#[test]
fn t471_located_avm1_and_avm2_unsupported_opcode_diagnostics_carry_tag_and_offset() {
    // 1. AVM1 unsupported opcode 0x7D inside DoAction (tag 12)
    let mut avm1_swf = SwfWriter::new();
    avm1_swf.tag(12, &[0x7D, 0x00]);
    avm1_swf.tag(1, &[]);
    let mut p1 = SwfBytecodePlayer::from_bytes(&avm1_swf.into_swf(7, 100, 100, 1)).unwrap();
    let st1 = p1.step_frame();
    assert!(
        st1.unsupported_diagnostics.iter().any(|d| {
            d.contains("0x7D")
                && d.contains("#tag=12:")
                && d.contains("target=_root")
                && d.contains("offset=0x0")
        }),
        "expected located #tag=12 AVM1 diagnostic, got: {:?}",
        st1.unsupported_diagnostics
    );

    // 2. AVM2 unsupported opcode 0xFF inside DoABC2 (tag 82)
    let mut abc = Vec::new();
    let push_u30 = |buf: &mut Vec<u8>, mut val: u32| {
        loop {
            let mut b = (val & 0x7F) as u8;
            val >>= 7;
            if val != 0 {
                b |= 0x80;
            }
            buf.push(b);
            if val == 0 {
                break;
            }
        }
    };
    abc.extend_from_slice(&16u16.to_le_bytes()); // minor = 16
    abc.extend_from_slice(&46u16.to_le_bytes()); // major = 46
    push_u30(&mut abc, 0); // ints
    push_u30(&mut abc, 0); // uints
    push_u30(&mut abc, 0); // doubles
    push_u30(&mut abc, 1); // strings (0 unused -> count=1)
    push_u30(&mut abc, 1); // namespaces (0 unused -> count=1)
    push_u30(&mut abc, 1); // ns_sets
    push_u30(&mut abc, 1); // multinames
    // 1 method: sinit (0 params, return *, flags=0)
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0); // param_count
    push_u30(&mut abc, 0); // return_type
    push_u30(&mut abc, 0); // name
    abc.push(0); // flags
    push_u30(&mut abc, 0); // metadata_count
    push_u30(&mut abc, 0); // class_count
    // 1 script: init=0, 0 traits
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);
    // 1 method body for method 0: code = [0xD0 (getlocal0), 0x30 (pushscope), 0xFF (invalid), 0x47 (returnvoid)]
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0); // method = 0
    push_u30(&mut abc, 2); // max_stack
    push_u30(&mut abc, 1); // local_count
    push_u30(&mut abc, 0); // init_scope_depth
    push_u30(&mut abc, 1); // max_scope_depth
    push_u30(&mut abc, 4); // code_length
    abc.extend_from_slice(&[0xD0, 0x30, 0xFF, 0x47]);
    push_u30(&mut abc, 0); // exception_count
    push_u30(&mut abc, 0); // trait_count

    let mut do_abc2 = SwfWriter::new();
    do_abc2.u32(0); // flags (eager)
    do_abc2.c_str("frame1");
    do_abc2.bytes(&abc);

    let mut avm2_swf = SwfWriter::new();
    // FileAttributes tag 69 with ActionScript3 (0x08)
    avm2_swf.tag(69, &[0x08, 0x00, 0x00, 0x00]);
    avm2_swf.tag(82, &do_abc2.buf);
    avm2_swf.tag(1, &[]);
    let mut p2 = SwfBytecodePlayer::from_bytes(&avm2_swf.into_swf(9, 100, 100, 1)).unwrap();
    let st2 = p2.step_frame();
    assert!(
        st2.unsupported_diagnostics
            .iter()
            .any(|d| { d.contains("0xFF") && d.contains("#tag=82:") && d.contains("offset=0x2") }),
        "expected located #tag=82 AVM2 diagnostic, got: {:?}",
        st2.unsupported_diagnostics
    );
}

#[test]
fn t471_avm2_multi_interface_and_super_subclass_execution() {
    // Hand-assembled DoABC2 (matching both mxmlc and NeoFlash's SWF AS3 exporter conventions):
    //   interface IRoot {}
    //   interface IAlpha extends IRoot {}
    //   interface IBeta {}
    //   class Base {
    //       public var count:Number;
    //       public function Base(n:Number) { this.count = n; }
    //       public function bump(delta:Number):void { this.count = this.count + delta; }
    //       public function describe():String { return "base:" + this.count; }
    //   }
    //   class Widget extends Base implements IAlpha, IBeta {
    //       public function Widget(n:Number) { super(n + 10); }
    //       override public function describe():String {
    //           super.bump(5);
    //           super.count = super.count + 2;
    //           return super.describe() + ":widget";
    //       }
    //   }
    let push_u30 = |buf: &mut Vec<u8>, mut val: u32| {
        loop {
            let mut b = (val & 0x7F) as u8;
            val >>= 7;
            if val != 0 {
                b |= 0x80;
            }
            buf.push(b);
            if val == 0 {
                break;
            }
        }
    };
    let mut abc = Vec::new();
    abc.extend_from_slice(&16u16.to_le_bytes());
    abc.extend_from_slice(&46u16.to_le_bytes());

    // ints: [0(unused), 10, 5, 2, 7]
    push_u30(&mut abc, 5);
    for v in [10u32, 5, 2, 7] {
        push_u30(&mut abc, v);
    }
    push_u30(&mut abc, 0); // uints
    push_u30(&mut abc, 0); // doubles

    // strings (1-based):
    let strings = [
        "",         // 1
        "Object",   // 2
        "IRoot",    // 3
        "IAlpha",   // 4
        "IBeta",    // 5
        "Base",     // 6
        "Widget",   // 7
        "count",    // 8
        "bump",     // 9
        "describe", // 10
        "trace",    // 11
        "base:",    // 12
        ":widget",  // 13
    ];
    push_u30(&mut abc, (strings.len() + 1) as u32);
    for s in strings {
        push_u30(&mut abc, s.len() as u32);
        abc.extend_from_slice(s.as_bytes());
    }

    // namespaces (1-based): 1 = PackageNamespace("")
    push_u30(&mut abc, 2);
    abc.push(0x16); // CONSTANT_PackageNamespace
    push_u30(&mut abc, 1); // ""

    // ns_sets (1-based): 1 = [ns#1] (used for CONSTANT_Multiname interface refs!)
    push_u30(&mut abc, 2);
    abc.push(1);
    push_u30(&mut abc, 1);

    // multinames (1-based):
    // 1: QName(1, "Object")
    // 2: QName(1, "IRoot")
    // 3: QName(1, "IAlpha")
    // 4: QName(1, "IBeta")
    // 5: QName(1, "Base")
    // 6: QName(1, "Widget")
    // 7: QName(1, "count")
    // 8: QName(1, "bump")
    // 9: QName(1, "describe")
    // 10: QName(1, "trace")
    // 11: Multiname("IRoot", ns_set=1)  (0x09 CONSTANT_Multiname interface link!)
    // 12: Multiname("IAlpha", ns_set=1) (0x09 CONSTANT_Multiname interface link!)
    push_u30(&mut abc, 13);
    for str_idx in 2u32..=11 {
        abc.push(0x07); // QName
        push_u30(&mut abc, 1);
        push_u30(&mut abc, str_idx);
    }
    for str_idx in [3u32, 4] {
        abc.push(0x09); // Multiname
        push_u30(&mut abc, str_idx);
        push_u30(&mut abc, 1); // ns_set = 1
    }

    // Methods (0..=11):
    // 0: IRoot.iinit (0 params) - no body
    // 1: IRoot.cinit (0 params)
    // 2: IAlpha.iinit (0 params) - no body
    // 3: IAlpha.cinit (0 params)
    // 4: IBeta.iinit (0 params) - no body
    // 5: IBeta.cinit (0 params)
    // 6: Base.iinit (1 param)
    // 7: Base.cinit (0 params)
    // 8: Base.bump (1 param)
    // 9: Base.describe (0 params)
    // 10: Widget.iinit (1 param)
    // 11: Widget.cinit (0 params)
    // 12: Widget.describe (0 params)
    // 13: script0.sinit (0 params)
    let method_param_counts = [0u32, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0];
    push_u30(&mut abc, method_param_counts.len() as u32);
    for pc in method_param_counts {
        push_u30(&mut abc, pc);
        push_u30(&mut abc, 0); // return *
        for _ in 0..pc {
            push_u30(&mut abc, 0); // param *
        }
        push_u30(&mut abc, 0); // name
        abc.push(0); // flags
    }
    push_u30(&mut abc, 0); // metadata

    // Classes (5): 0=IRoot, 1=IAlpha, 2=IBeta, 3=Base, 4=Widget
    push_u30(&mut abc, 5);
    // 0: IRoot (interface, flags=0x05)
    push_u30(&mut abc, 2); // name = IRoot
    push_u30(&mut abc, 0); // super_name = 0
    abc.push(0x05); // Sealed | Interface
    push_u30(&mut abc, 0); // 0 interfaces
    push_u30(&mut abc, 0); // iinit = 0
    push_u30(&mut abc, 0); // 0 traits
    // 1: IAlpha extends IRoot (interface, flags=0x05, interfaces=[Multiname#11(IRoot)])
    push_u30(&mut abc, 3); // name = IAlpha
    push_u30(&mut abc, 0); // super_name = 0
    abc.push(0x05);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 11); // Multiname("IRoot")
    push_u30(&mut abc, 2); // iinit = 2
    push_u30(&mut abc, 0);
    // 2: IBeta (interface, flags=0x05)
    push_u30(&mut abc, 4); // name = IBeta
    push_u30(&mut abc, 0);
    abc.push(0x05);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 4); // iinit = 4
    push_u30(&mut abc, 0);
    // 3: Base extends Object (flags=0x01)
    push_u30(&mut abc, 5); // name = Base
    push_u30(&mut abc, 1); // super = Object
    abc.push(0x01); // Sealed
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 6); // iinit = 6
    push_u30(&mut abc, 3); // 3 instance traits: slot `count`, method `bump`, method `describe`
    // Trait 0: Slot count (mn=7)
    push_u30(&mut abc, 7);
    abc.push(0x00); // Trait_Slot
    push_u30(&mut abc, 0); // slot_id auto
    push_u30(&mut abc, 0); // type *
    push_u30(&mut abc, 0); // vindex 0
    // Trait 1: Method bump (mn=8, method=8)
    push_u30(&mut abc, 8);
    abc.push(0x01); // Trait_Method
    push_u30(&mut abc, 0); // disp_id
    push_u30(&mut abc, 8); // method 8
    // Trait 2: Method describe (mn=9, method=9)
    push_u30(&mut abc, 9);
    abc.push(0x01);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 9);
    // 4: Widget extends Base implements IAlpha (Multiname#12), IBeta (QName#4)
    push_u30(&mut abc, 6); // name = Widget
    push_u30(&mut abc, 5); // super = Base
    abc.push(0x01); // Sealed
    push_u30(&mut abc, 2); // 2 interfaces!
    push_u30(&mut abc, 12); // Multiname("IAlpha")
    push_u30(&mut abc, 4); // QName("IBeta")
    push_u30(&mut abc, 10); // iinit = 10
    push_u30(&mut abc, 1); // 1 trait: override describe (method=12)
    push_u30(&mut abc, 9);
    abc.push(0x21); // ATTR_Override(0x20) | Trait_Method(0x01)
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 12);

    // ClassInfos (5): cinit = 1, 3, 5, 7, 11 (0 static traits each)
    for cinit in [1u32, 3, 5, 7, 11] {
        push_u30(&mut abc, cinit);
        push_u30(&mut abc, 0);
    }

    // ScriptInfo (1): init = 13, 5 class traits (IRoot..Widget)
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 13);
    push_u30(&mut abc, 5);
    for (cls_idx, mn_idx) in [(0u32, 2u32), (1, 3), (2, 4), (3, 5), (4, 6)] {
        push_u30(&mut abc, mn_idx);
        abc.push(0x04); // Trait_Class
        push_u30(&mut abc, 0); // slot_id
        push_u30(&mut abc, cls_idx);
    }

    // Method bodies (11 bodies: cinits 1,3,5,7,11 + Base.iinit(6), Base.bump(8), Base.describe(9), Widget.iinit(10), Widget.describe(12), sinit(13))
    let retvoid = vec![0x47u8];
    // Base.iinit(n): getlocal0, pushscope, getlocal0, constructsuper(0), getlocal0, getlocal1, initproperty count(7), returnvoid
    let base_iinit = vec![0xD0, 0x30, 0xD0, 0x49, 0x00, 0xD0, 0xD1, 0x68, 0x07, 0x47];
    // Base.bump(delta): getlocal0, pushscope, getlocal0, getlocal0, getproperty count(7), getlocal1, add, setproperty count(7), returnvoid
    let base_bump = vec![
        0xD0, 0x30, 0xD0, 0xD0, 0x66, 0x07, 0xD1, 0xA0, 0x61, 0x07, 0x47,
    ];
    // Base.describe(): getlocal0, pushscope, pushstring "base:"(12), getlocal0, getproperty count(7), add, returnvalue
    let base_describe = vec![0xD0, 0x30, 0x2C, 0x0C, 0xD0, 0x66, 0x07, 0xA0, 0x48];
    // Widget.iinit(n): getlocal0, pushscope, getlocal0, getlocal1, pushint #1(10), add, constructsuper(1), returnvoid
    let widget_iinit = vec![0xD0, 0x30, 0xD0, 0xD1, 0x2D, 0x01, 0xA0, 0x49, 0x01, 0x47];
    // Widget.describe():
    //   getlocal0, pushscope,
    //   getlocal0, pushint #2(5), callsupervoid bump(8), 1,
    //   getlocal0, getlocal0, getsuper count(7), pushint #3(2), add, setsuper count(7),
    //   getlocal0, callsuper describe(9), 0, pushstring ":widget"(13), add, returnvalue
    let widget_describe = vec![
        0xD0, 0x30, // prologue
        0xD0, 0x2D, 0x02, 0x4E, 0x08, 0x01, // super.bump(5)
        0xD0, 0xD0, 0x04, 0x07, 0x2D, 0x03, 0xA0, 0x05, 0x07, // super.count = super.count + 2
        0xD0, 0x45, 0x09, 0x00, 0x2C, 0x0D, 0xA0, 0x48, // return super.describe() + ":widget"
    ];
    // sinit (locals: 0=global, 1=w):
    //   getlocal0, pushscope
    //   init IRoot(0), IAlpha(1), IBeta(2) with pushnull + newclass:
    //   init Base(3): getlex Object(1), pushscope, getlex Object(1), newclass 3, popscope, initproperty Base(5)
    //   init Widget(4): getlex Object(1), pushscope, getlex Base(5), pushscope, getlex Base(5), newclass 4, popscope, popscope, initproperty Widget(6)
    //   w = new Widget(7) -> count = 7 + 10 = 17
    //   trace(w.describe()) -> bump(5)->22, +2->24 -> "base:24:widget"
    //   trace(w is IAlpha) [istype 3], trace(w is IRoot) [istypelate], trace(w is IBeta) [istype 4]
    //   trace((w as IRoot) === w) [astype 2], trace((w as IBeta) === w) [astypelate]
    //   trace(w instanceof Base), trace(w instanceof IAlpha)
    let mut sinit = vec![0xD0, 0x30];
    for (cls_idx, mn_idx) in [(0u8, 2u8), (1, 3), (2, 4)] {
        sinit.extend_from_slice(&[0x65, 0x00, 0x20, 0x58, cls_idx, 0x68, mn_idx]);
    }
    // Base
    sinit.extend_from_slice(&[
        0x65, 0x00, 0x60, 0x01, 0x30, 0x60, 0x01, 0x58, 0x03, 0x1D, 0x68, 0x05,
    ]);
    // Widget
    sinit.extend_from_slice(&[
        0x65, 0x00, 0x60, 0x01, 0x30, 0x60, 0x05, 0x30, 0x60, 0x05, 0x58, 0x04, 0x1D, 0x1D, 0x68,
        0x06,
    ]);
    // local1 = new Widget(7)
    sinit.extend_from_slice(&[0x60, 0x06, 0x2D, 0x04, 0x42, 0x01, 0xD5]);
    // trace(w.describe())
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0x46, 0x09, 0x00, 0x4F, 0x0A, 0x01]);
    // trace(w is IAlpha) via istype(3)
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0xB2, 0x03, 0x4F, 0x0A, 0x01]);
    // trace(w is IRoot) via istypelate
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0x60, 0x02, 0xB3, 0x4F, 0x0A, 0x01]);
    // trace(w is IBeta) via istype(4)
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0xB2, 0x04, 0x4F, 0x0A, 0x01]);
    // trace((w as IRoot) === w) via astype(2)
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0x86, 0x02, 0xD1, 0xAC, 0x4F, 0x0A, 0x01]);
    // trace((w as IBeta) === w) via astypelate
    sinit.extend_from_slice(&[
        0x5E, 0x0A, 0xD1, 0x60, 0x04, 0x87, 0xD1, 0xAC, 0x4F, 0x0A, 0x01,
    ]);
    // trace(w instanceof Base) -> true
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0x60, 0x05, 0xB1, 0x4F, 0x0A, 0x01]);
    // trace(w instanceof IAlpha) -> false (AS3 instanceof is false for interfaces!)
    sinit.extend_from_slice(&[0x5E, 0x0A, 0xD1, 0x60, 0x03, 0xB1, 0x4F, 0x0A, 0x01]);
    sinit.push(0x47);

    let bodies: [(u32, u32, u32, &[u8]); 11] = [
        (1, 1, 1, &retvoid),
        (3, 1, 1, &retvoid),
        (5, 1, 1, &retvoid),
        (6, 4, 2, &base_iinit),
        (7, 1, 1, &retvoid),
        (8, 4, 2, &base_bump),
        (9, 4, 1, &base_describe),
        (10, 4, 2, &widget_iinit),
        (11, 1, 1, &retvoid),
        (12, 5, 1, &widget_describe),
        (13, 6, 2, &sinit),
    ];
    push_u30(&mut abc, bodies.len() as u32);
    for (midx, max_stack, local_cnt, code) in bodies {
        push_u30(&mut abc, midx);
        push_u30(&mut abc, max_stack);
        push_u30(&mut abc, local_cnt);
        push_u30(&mut abc, 0);
        push_u30(&mut abc, 4);
        push_u30(&mut abc, code.len() as u32);
        abc.extend_from_slice(code);
        push_u30(&mut abc, 0);
        push_u30(&mut abc, 0);
    }

    let mut do_abc2 = SwfWriter::new();
    do_abc2.u32(0);
    do_abc2.c_str("oop_test");
    do_abc2.bytes(&abc);

    let mut swf = SwfWriter::new();
    swf.tag(69, &[0x08, 0x00, 0x00, 0x00]);
    swf.tag(82, &do_abc2.buf);
    swf.tag(1, &[]);
    let swf_bytes = swf.into_swf(9, 100, 100, 1);

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("player");
    let step = player.step_frame();
    assert!(
        step.unsupported_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        step.unsupported_diagnostics
    );
    assert_eq!(
        step.traces,
        vec![
            "base:24:widget",
            "true",
            "true",
            "true",
            "true",
            "true",
            "true",
            "false",
        ]
    );

    // Also verify static import_project + validate() cleanly decompiles multi-interface + super!
    let tmp = TempSwfFile::write("oop-multi-interface", &swf_bytes);
    let project = import_project(tmp.path()).expect("import OOP SWF");
    project.validate().expect("imported OOP SWF validates");
}

#[test]
fn t471_ruffle_flash_player_differential_oracle_suite() {
    let Some(swfs_dir) = find_ruffle_swfs_dir() else {
        return;
    };

    // Complete set of 118 Ruffle reference movies already covered across T-451, T-465, T-462, and T-470:
    let previously_covered: BTreeSet<&str> = [
        // T-465 (11)
        "avm2/increment",
        "avm2/increment_i",
        "avm2/decrement",
        "avm2/decrement_i",
        "avm2/multiply",
        "avm2/subtract",
        "avm2/add",
        "avm2/negate",
        "avm2/bitand",
        "avm2/rshift",
        "avm2/urshift",
        // T-451 (5)
        "avm1/goto_label",
        "avm1/clip_events",
        "avm1/frame_labels",
        "avm2/button_frame_scripts",
        "avm2/nested_construction",
        // T-462 (61)
        "avm2/bitnot",
        "avm2/bitor",
        "avm2/bitxor",
        "avm2/lshift",
        "avm2/modulo",
        "avm2/divide",
        "avm2/equals",
        "avm2/strict_equality",
        "avm2/lessthan",
        "avm2/lessequals",
        "avm2/greaterthan",
        "avm2/greaterequals",
        "avm2/boolean_negation",
        "avm2/boolean_constr",
        "avm2/boolean_tostring",
        "avm2/if_eq",
        "avm2/if_ne",
        "avm2/if_lt",
        "avm2/if_lte",
        "avm2/if_gt",
        "avm2/if_gte",
        "avm2/if_stricteq",
        "avm2/if_strictne",
        "avm2/convert_boolean",
        "avm2/convert_integer",
        "avm2/convert_number",
        "avm2/convert_uinteger",
        "avm2/coerce_string",
        "avm2/coerce_return_void",
        "avm2/coerce_return_type",
        "avm2/int_constr",
        "avm2/uint_constr",
        "avm2/number_constr",
        "avm2/string_constr",
        "avm2/closures",
        "avm2/function_call_via_call",
        "avm2/function_call_default",
        "avm2/throw",
        "avm2/with",
        "avm2/array_literal",
        "avm2/array_length",
        "avm2/array_join",
        "avm1/goto_advance2",
        "avm1/goto_rewind2",
        "avm1/goto_execution_order",
        "avm1/goto_execution_order2",
        "avm1/execution_order1",
        "avm1/execution_order3",
        "avm1/do_init_action",
        "avm1/strictly_equals",
        "avm1/strictequals_swf6",
        "avm1/bitand",
        "avm1/bitor",
        "avm1/bitxor",
        "avm1/biturshift",
        "avm1/biturshift_swf8",
        "avm1/greaterthan_swf5",
        "avm1/typeof_globals",
        "avm1/delete",
        "avm1/looping",
        "avm1/with_return",
        // T-470 (41)
        "avm1/try_catch_finally",
        "avm1/catch_references_registers",
        "avm1/primitive_instanceof",
        "avm1/extends_chain",
        "avm1/define_function2",
        "avm2/try_catch",
        "avm2/superinterface_instanceof",
        "avm2/truthiness",
        "avm2/vector_constr",
        "avm2/vector_enumeration",
        "avm2/vector_int_access",
        "avm2/vector_pushpop",
        "avm2/vector_shiftunshift",
        "avm2/vector_slice",
        "avm2/vector_join",
        "avm2/vector_reverse",
        "avm2/vector_indexof",
        "avm2/vector_lastindexof",
        "avm2/vector_insertat",
        "avm2/vector_every",
        "avm2/vector_filter",
        "avm2/vector_map",
        "avm2/vector_object_toString",
        "avm2/dictionary_access",
        "avm2/dictionary_hasownproperty",
        "avm2/dictionary_in",
        "avm2/xml_child",
        "avm2/xml_childindex",
        "avm2/xml_elements",
        "avm2/xml_text",
        "avm2/xml_nodekind",
        "avm2/xml_length",
        "avm2/xml_tostring",
        "avm2/xml_simple_complex_content",
        "avm2/xml_wildcard",
        "avm2/xml_class_call",
        "avm2/xml_hasownproperty",
        "avm2/xml_no_namespace",
        "avm2/xml_settings",
        "avm2/qname_tostring",
        "avm2/qname_valueof",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        previously_covered.len(),
        118,
        "expected exact 118 baseline movies from T-451/T-465/T-462/T-470"
    );

    let reference_movies = [
        // AVM2 OOP interfaces, inheritance, super, istype/astype/instanceof, & class cast calls (13)
        "avm2/es4_interfaces",
        "avm2/es4_inheritance",
        "avm2/construct_interface",
        "avm2/super_get_call",
        "avm2/superinterface_call",
        "avm2/istype",
        "avm2/istypelate_coerce",
        "avm2/astype",
        "avm2/astypelate",
        "avm2/astypelate_propagates",
        "avm2/class_cast_call",
        "avm2/class_is",
        "avm2/class_methods",
        // AVM2 arrays, strings, objects, functions, slots, E4X, & QName/Namespace (26)
        "avm2/array_constr",
        "avm2/array_delete",
        "avm2/array_every",
        "avm2/array_filter",
        "avm2/array_foreach",
        "avm2/array_indexof",
        "avm2/array_map",
        "avm2/array_some",
        "avm2/array_tostring",
        "avm2/array_valueof",
        "avm2/string_char_at",
        "avm2/string_char_code_at",
        "avm2/string_length",
        "avm2/object_to_string",
        "avm2/object_value_of",
        "avm2/object_enumeration",
        "avm2/function_call_types",
        "avm2/default_values",
        "avm2/static_var_with_this_in_ctor",
        "avm2/slots_force_autoassigned",
        "avm2/slot_disp_id_shared_numbering",
        "avm2/namespace_constr_args",
        "avm2/qname_constr",
        "avm2/xml_copy",
        "avm2/xml_delete",
        "avm2/xml_list_concat",
        // AVM1 lexical closure scope (1)
        "avm1/closure_scope",
    ];

    let mut passed = Vec::new();
    let mut failures = Vec::new();
    let mut total_verified_lines = 0usize;
    let mut seen = BTreeSet::new();

    for rel in reference_movies {
        assert!(
            seen.insert(rel),
            "duplicate Ruffle reference movie entry: {rel}"
        );
        assert!(
            !previously_covered.contains(rel),
            "{rel} overlaps earlier T-451/T-465/T-462/T-470 suite; keep T-471 movies strictly disjoint"
        );
        let dir = swfs_dir.join(rel);
        let swf_path = dir.join("test.swf");
        let out_path = dir.join("output.txt");
        let toml_path = dir.join("test.toml");
        if !swf_path.is_file() || !out_path.is_file() {
            failures.push(format!("{rel}: missing test.swf or output.txt"));
            continue;
        }
        let toml_str = std::fs::read_to_string(&toml_path).unwrap_or_default();
        assert!(
            !toml_str.contains("air = true"),
            "{rel} must be a non-AIR Flash Player reference movie"
        );
        let ticks = toml_str
            .lines()
            .find_map(|l| {
                let l = l.trim();
                l.strip_prefix("num_frames =")
                    .or_else(|| l.strip_prefix("num_ticks ="))
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(1);

        let expected_raw = std::fs::read_to_string(&out_path).expect("read output.txt");
        let expected: Vec<String> = expected_raw.lines().map(str::to_owned).collect();

        let mut player = SwfBytecodePlayer::from_path(&swf_path)
            .unwrap_or_else(|e| panic!("SwfBytecodePlayer::from_path({rel}): {e}"));
        let mut actual = Vec::new();
        let mut diag_err = None;
        for _ in 0..ticks {
            let step = player.step_frame();
            if !step.unsupported_diagnostics.is_empty() {
                diag_err = Some(format!("{:?}", step.unsupported_diagnostics));
                break;
            }
            for t in step.traces {
                for line in t.split('\n') {
                    actual.push(line.to_owned());
                }
            }
        }
        if let Some(d) = diag_err {
            failures.push(format!("{rel}: unsupported diagnostics {d}"));
            continue;
        }
        if actual != expected {
            let diff_idx = actual
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| actual.len().min(expected.len()));
            failures.push(format!(
                "{rel}: line {diff_idx} mismatch (actual len={} vs expected len={}): got {:?}, expected {:?}",
                actual.len(),
                expected.len(),
                actual.get(diff_idx),
                expected.get(diff_idx)
            ));
            continue;
        }

        let report = inspect(&swf_path).unwrap_or_else(|e| panic!("inspect({rel}): {e}"));
        assert!(report.can_import, "{rel} inspect can_import failed");
        let project =
            import_project(&swf_path).unwrap_or_else(|e| panic!("import_project({rel}): {e}"));
        project
            .validate()
            .unwrap_or_else(|e| panic!("{rel} imported project failed validate(): {e}"));

        passed.push(rel);
        total_verified_lines += expected.len();
    }

    assert!(
        failures.is_empty(),
        "passed {}/{} ({total_verified_lines} lines); failures:\n{}",
        passed.len(),
        reference_movies.len(),
        failures.join("\n")
    );
    assert_eq!(passed.len(), reference_movies.len());
    assert!(
        passed.len() >= 35,
        "expected at least 35 strictly disjoint Ruffle reference movies verified 1:1, got {}",
        passed.len()
    );
    eprintln!(
        "[T-471] Verified {}/{} strictly disjoint non-AIR Ruffle test.swf + output.txt movies 1:1 ({total_verified_lines} trace lines; {} cumulative across T-451..T-471)",
        passed.len(),
        reference_movies.len(),
        118 + passed.len()
    );
}
