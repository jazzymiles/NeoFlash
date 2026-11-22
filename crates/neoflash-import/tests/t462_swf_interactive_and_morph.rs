//! Track `T-462` verification suite:
//! 1. Nested `DefineSprite` (`MovieClip`) + grandchild raw-sound (`StartSound`, `StartSound2`)
//!    and stream-sound (`SoundStreamHead2` + `SoundStreamBlock`, `swf-stream-sym-<id>-<seq>`,
//!    `swf-clip-stream-<key>`) lifecycle (`StreamStart` while playing, `StreamStop` on `stop()`,
//!    stream span end, loop wrap, `RemoveObject2`, and `Move+HasCharacter`).
//! 2. `DefineButtonSound` (tag 17) all 4 transition slots (`0..3`) with full `SOUNDINFO`
//!    (`sync`, `in_point`, `out_point`, `loops`, `envelope`) across pointer transitions and
//!    imported `Button` symbol keyframes.
//! 3. `DefineMorphShape` (46) & `DefineMorphShape2` (84) morph gradients (`0x10` linear,
//!    `0x12` radial, `0x13` focal radial) import preserving distinct start vs end stops,
//!    matrices, spread/interpolation flags, and focal points (including collapsed end contours).
//! 4. Expanded upstream Ruffle Flash Player `test.swf` + `output.txt` 1:1 differential oracle suite.

use neoflash_core::{
    ElementKind, GradientKind, GradientSpread, Player as CorePlayer, SoundCommand, SoundSync,
    SymbolKind,
};
use neoflash_import::{SwfBytecodePlayer, import_project, inspect};
use std::collections::BTreeSet;
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
    // Resolve main checkout when running inside a git worktree (`.git` file with `gitdir: ...`)
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
            "neoflash-t462-{label}-{}-{}.swf",
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

    fn i16(&mut self, v: i16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u16(&mut self, v: u16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        self.align();
        self.buf.extend_from_slice(b);
    }

    fn c_str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
        self.u8(0);
    }

    fn rect(&mut self, xmin: i32, xmax: i32, ymin: i32, ymax: i32) {
        self.align();
        let nbits = 15u8;
        self.write_ub(u32::from(nbits), 5);
        self.write_sb(xmin, nbits);
        self.write_sb(xmax, nbits);
        self.write_sb(ymin, nbits);
        self.write_sb(ymax, nbits);
        self.align();
    }

    fn scale_translate_matrix(&mut self, sx: f64, sy: f64, tx_twips: i32, ty_twips: i32) {
        self.align();
        self.write_ub(1, 1); // HasScale = 1
        let sbits = 20u8;
        self.write_ub(u32::from(sbits), 5);
        self.write_sb((sx * 65536.0).round() as i32, sbits);
        self.write_sb((sy * 65536.0).round() as i32, sbits);
        self.write_ub(0, 1); // HasRotate = 0
        let tbits = 15u8;
        self.write_ub(u32::from(tbits), 5);
        self.write_sb(tx_twips, tbits);
        self.write_sb(ty_twips, tbits);
        self.align();
    }

    fn translate_matrix(&mut self, tx_twips: i32, ty_twips: i32) {
        self.align();
        self.write_ub(0, 1);
        self.write_ub(0, 1);
        let nbits = 14u8;
        self.write_ub(u32::from(nbits), 5);
        self.write_sb(tx_twips, nbits);
        self.write_sb(ty_twips, nbits);
        self.align();
    }

    fn tag(&mut self, code: u16, payload: &[u8]) {
        self.align();
        if payload.len() < 0x3f {
            let hdr = (code << 6) | (payload.len() as u16);
            self.u16(hdr);
        } else {
            let hdr = (code << 6) | 0x3f;
            self.u16(hdr);
            self.u32(payload.len() as u32);
        }
        self.bytes(payload);
    }

    fn into_swf(mut self, version: u8, width_px: i32, height_px: i32, frame_count: u16) -> Vec<u8> {
        self.tag(0, &[]); // End
        let mut hdr = SwfWriter::new();
        hdr.rect(0, width_px * 20, 0, height_px * 20);
        hdr.u16(24 << 8); // 24 fps
        hdr.u16(frame_count);
        hdr.align();

        let total_len = (8 + hdr.buf.len() + self.buf.len()) as u32;
        let mut out = Vec::with_capacity(total_len as usize);
        out.extend_from_slice(b"FWS");
        out.push(version);
        out.extend_from_slice(&total_len.to_le_bytes());
        out.extend_from_slice(&hdr.buf);
        out.extend_from_slice(&self.buf);
        out
    }
}

fn make_pcm_define_sound_payload(sound_id: u16, sample_count: u32) -> Vec<u8> {
    let mut w = SwfWriter::new();
    w.u16(sound_id);
    w.u8(0x3e); // Uncompressed LE PCM, 44.1 kHz, 16-bit, mono
    w.u32(sample_count);
    let mut pcm = vec![0u8; (sample_count as usize) * 2];
    for (i, chunk) in pcm.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        *chunk = ((i as i16).wrapping_mul(97)).to_le_bytes();
    }
    w.bytes(&pcm);
    w.buf
}

fn make_solid_rect_shape_payload(char_id: u16, w_px: i32, h_px: i32, rgba: [u8; 4]) -> Vec<u8> {
    let w = w_px * 20;
    let h = h_px * 20;
    let mut sw = SwfWriter::new();
    sw.u16(char_id);
    sw.rect(0, w, 0, h);
    sw.u8(1); // 1 fill style
    sw.u8(0x00); // solid
    sw.bytes(&rgba);
    sw.u8(0); // 0 line styles
    sw.write_ub(1, 4); // numFillBits = 1
    sw.write_ub(0, 4); // numLineBits = 0
    sw.write_ub(0, 1); // non-edge
    sw.write_ub(0, 1); // StateNewStyles
    sw.write_ub(0, 1); // StateLineStyle
    sw.write_ub(1, 1); // StateFillStyle1
    sw.write_ub(0, 1); // StateFillStyle0
    sw.write_ub(1, 1); // StateMoveTo
    sw.write_ub(1, 5);
    sw.write_sb(0, 1);
    sw.write_sb(0, 1);
    sw.write_ub(1, 1); // fillStyle1 = 1
    for (dx, dy) in [(w, 0), (0, h), (-w, 0), (0, -h)] {
        sw.write_ub(1, 1);
        sw.write_ub(1, 1);
        let nbits = 13u8;
        sw.write_ub(u32::from(nbits), 4);
        sw.write_ub(1, 1);
        sw.write_sb(dx, nbits + 2);
        sw.write_sb(dy, nbits + 2);
    }
    sw.write_ub(0, 6);
    sw.align();
    sw.buf
}

fn place_char_payload(depth: u16, char_id: u16, tx_px: i32, ty_px: i32) -> Vec<u8> {
    let mut p = SwfWriter::new();
    p.u8(0x06); // HasMatrix | HasCharacter
    p.u16(depth);
    p.u16(char_id);
    p.translate_matrix(tx_px * 20, ty_px * 20);
    p.buf
}

fn define_sprite_payload(sprite_id: u16, frame_count: u16, inner_tags: &[u8]) -> Vec<u8> {
    let mut sp = SwfWriter::new();
    sp.u16(sprite_id);
    sp.u16(frame_count);
    sp.bytes(inner_tags);
    sp.tag(0, &[]); // End tag inside DefineSprite
    sp.buf
}

#[test]
fn t462_nested_sprite_and_grandchild_event_and_stream_sounds_lifecycle() {
    let mut swf = SwfWriter::new();

    // Sound #10 (direct StartSound) and #11 (StartSound2 bound via late SymbolClass)
    swf.tag(14, &make_pcm_define_sound_payload(10, 4410));
    swf.tag(14, &make_pcm_define_sound_payload(11, 4410));

    // Replacement static shape #30 for Move+HasCharacter test
    swf.tag(
        32,
        &make_solid_rect_shape_payload(30, 20, 20, [200, 60, 60, 255]),
    );

    // Grandchild DefineSprite #20 (3 frames):
    // - Frame 0: StartSound(#10, Start, in=0.01, out=0.05, loops=2, env=1pt) + SoundStreamHead2 + SoundStreamBlock
    // - Frame 1: SoundStreamBlock (continues stream)
    // - Frame 2: StartSound2("sfx.GrandChime", Event, loops=1) — NO SoundStreamBlock (stream span ends!)
    let mut gc = SwfWriter::new();
    let mut start10 = SwfWriter::new();
    start10.u16(10);
    start10.u8(0x1f); // SyncNoMultiple(0x10) | HasEnvelope(0x08) | HasLoops(0x04) | HasOut(0x02) | HasIn(0x01)
    start10.u32(441); // in_point = 0.01s
    start10.u32(2205); // out_point = 0.05s
    start10.u16(2); // loops = 2
    start10.u8(1); // 1 envelope point
    start10.u32(0);
    start10.u16(32768); // left = 1.0
    start10.u16(16384); // right = 0.5
    gc.tag(15, &start10.buf);
    gc.tag(45, &[0x0e, 0x3e, 2, 0]); // SoundStreamHead2
    gc.tag(19, &[11, 0, 12, 0]); // SoundStreamBlock f0
    gc.tag(1, &[]); // ShowFrame 0

    gc.tag(19, &[21, 0, 22, 0]); // SoundStreamBlock f1
    gc.tag(1, &[]); // ShowFrame 1

    let mut start_cls = SwfWriter::new();
    start_cls.c_str("sfx.GrandChime");
    start_cls.u8(0x04); // Event + HasLoops
    start_cls.u16(2);
    gc.tag(89, &start_cls.buf); // StartSound2 on f2 (stream span ended on f1)
    gc.tag(1, &[]); // ShowFrame 2
    swf.tag(39, &define_sprite_payload(20, 3, &gc.buf));

    // Parent DefineSprite #21 (5 frames):
    // Places Grandchild #20 at depth 1 and plays its own stream across all 5 frames.
    let mut parent = SwfWriter::new();
    parent.tag(26, &place_char_payload(1, 20, 4, 4));
    parent.tag(45, &[0x0e, 0x3e, 2, 0]);
    for f in 0..5u8 {
        parent.tag(19, &[f + 1, 0, f + 1, 0]);
        parent.tag(1, &[]);
    }
    swf.tag(39, &define_sprite_payload(21, 5, &parent.buf));

    // Stoppable DefineSprite #22 (3 frames):
    // Stream plays on f0, then f1 executes DoAction `stop();` -> must emit StreamStop!
    let mut stoppable = SwfWriter::new();
    stoppable.tag(45, &[0x0e, 0x3e, 2, 0]);
    stoppable.tag(19, &[31, 0, 32, 0]);
    stoppable.tag(1, &[]);
    stoppable.tag(12, &[0x07, 0x00]); // ActionStop + End
    stoppable.tag(19, &[33, 0, 34, 0]);
    stoppable.tag(1, &[]);
    stoppable.tag(19, &[35, 0, 36, 0]);
    stoppable.tag(1, &[]);
    swf.tag(39, &define_sprite_payload(22, 3, &stoppable.buf));

    // Looping DefineSprite #23 (2 frames):
    // Stream plays on f0 and f1, then wraps 1 -> 0 on step 2 -> must emit StreamStop + StreamStart!
    let mut looper = SwfWriter::new();
    looper.tag(45, &[0x0e, 0x3e, 2, 0]);
    looper.tag(19, &[41, 0, 42, 0]);
    looper.tag(1, &[]);
    looper.tag(19, &[43, 0, 44, 0]);
    looper.tag(1, &[]);
    swf.tag(39, &define_sprite_payload(23, 2, &looper.buf));

    // Root timeline (5 frames):
    // Frame 0: Place #21 (depth 1), #22 (depth 2), #23 (depth 3) + late SymbolClass(11 -> "sfx.GrandChime")
    swf.tag(26, &place_char_payload(1, 21, 10, 10));
    swf.tag(26, &place_char_payload(2, 22, 30, 10));
    swf.tag(26, &place_char_payload(3, 23, 50, 10));
    let mut sym_cls = SwfWriter::new();
    sym_cls.u16(1);
    sym_cls.u16(11);
    sym_cls.c_str("sfx.GrandChime");
    swf.tag(76, &sym_cls.buf);
    swf.tag(1, &[]); // Root frame 0

    swf.tag(1, &[]); // Root frame 1
    swf.tag(1, &[]); // Root frame 2

    // Root frame 3: RemoveObject2 on depth 3 (removes Looping #23)
    let mut rem3 = SwfWriter::new();
    rem3.u16(3);
    swf.tag(28, &rem3.buf);
    swf.tag(1, &[]); // Root frame 3

    // Root frame 4: Move + HasCharacter on depth 1 replacing Parent #21 (and Grandchild #20) with Shape #30
    let mut replace1 = SwfWriter::new();
    replace1.u8(0x03); // HasCharacter(0x02) | Move(0x01)
    replace1.u16(1); // depth 1
    replace1.u16(30); // shape #30
    swf.tag(26, &replace1.buf);
    swf.tag(1, &[]); // Root frame 4

    let swf_bytes = swf.into_swf(9, 200, 150, 5);
    let tmp = TempSwfFile::write("nested-sprite-sounds", &swf_bytes);
    let report = inspect(tmp.path()).expect("inspect nested sprite sound SWF");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "swf_sound_approximated"),
        "unexpected sound approximation diagnostics: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import nested sprite sound SWF");
    project
        .validate()
        .expect("imported nested sprite sound project must validate");

    // Verify per-sprite `swf-stream-sym-<sprite_id>-<seq>` resources exist in imported Project
    let res_ids: BTreeSet<&str> = project.resources.iter().map(|r| r.id.as_str()).collect();
    for expected_res in [
        "swf-sound-10",
        "swf-sound-11",
        "swf-stream-sym-20-1",
        "swf-stream-sym-21-1",
        "swf-stream-sym-22-1",
        "swf-stream-sym-23-1",
    ] {
        assert!(
            res_ids.contains(expected_res),
            "missing expected audio resource `{expected_res}` in {res_ids:?}"
        );
    }

    // Verify Grandchild Sprite #20 timeline imported both StartSound (#10) and StartSound2 (#11) + stream
    let gc_sym = project
        .symbols
        .iter()
        .find(|s| s.id == "swf-sym-20")
        .expect("swf-sym-20 imported");
    let gc_sounds: Vec<_> = gc_sym
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .filter_map(|k| k.sound.as_ref().map(|s| (k.frame, s)))
        .collect();
    assert!(
        gc_sounds.iter().any(|(f, s)| *f == 0
            && s.resource == "swf-sound-10"
            && s.sync == SoundSync::Start
            && s.loops == 2
            && s.in_point == Some(0.01)
            && s.out_point == Some(0.05)),
        "StartSound on grandchild frame 0 missing or incomplete: {gc_sounds:?}"
    );
    assert!(
        gc_sounds.iter().any(|(f, s)| *f == 0
            && s.resource == "swf-stream-sym-20-1"
            && s.sync == SoundSync::Stream),
        "Stream sound on grandchild frame 0 missing: {gc_sounds:?}"
    );
    assert!(
        gc_sounds.iter().any(|(f, s)| *f == 2
            && s.resource == "swf-sound-11"
            && s.sync == SoundSync::Event
            && s.loops == 2),
        "StartSound2 on grandchild frame 2 missing: {gc_sounds:?}"
    );

    // Execute SwfBytecodePlayer across all 5 root frames and verify exact sound command lifecycle!
    let mut player =
        SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer::from_bytes");

    // Step 0 (Root frame 0):
    // - Grandchild #20 starts `swf-sound-10` + stream `swf-stream-sym-20-1`
    // - Parent #21 starts stream `swf-stream-sym-21-1`
    // - Stoppable #22 starts stream `swf-stream-sym-22-1`
    // - Looper #23 starts stream `swf-stream-sym-23-1`
    player.step_frame();
    assert!(
        player.unsupported_diagnostics().is_empty(),
        "unexpected unsupported diagnostics on f0: {:?}",
        player.unsupported_diagnostics()
    );
    let cmds0 = player.drain_sound_commands(&project);
    assert!(
        cmds0.iter().any(|c| matches!(
            c,
            SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                loops: 2,
                in_point: Some(0.01),
                out_point: Some(0.05),
                envelope,
                ..
            } if resource == "swf-sound-10" && envelope.len() == 1 && (envelope[0][1] - 1.0).abs() < 1e-3 && (envelope[0][2] - 0.5).abs() < 1e-3
        )),
        "expected grandchild StartSound #10 Play on f0, got {cmds0:?}"
    );
    let find_stream_channel = |cmds: &[SoundCommand], target_res: &str| -> String {
        cmds.iter()
            .find_map(|c| match c {
                SoundCommand::StreamStart {
                    channel,
                    resource,
                    offset,
                    ..
                } if resource == target_res && offset.abs() < 1e-6 => Some(channel.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected StreamStart for {target_res} in {cmds:?}"))
    };
    let ch_gc = find_stream_channel(&cmds0, "swf-stream-sym-20-1");
    let ch_parent = find_stream_channel(&cmds0, "swf-stream-sym-21-1");
    let ch_stop = find_stream_channel(&cmds0, "swf-stream-sym-22-1");
    let ch_loop = find_stream_channel(&cmds0, "swf-stream-sym-23-1");
    for ch in [&ch_gc, &ch_parent, &ch_stop, &ch_loop] {
        assert!(
            ch.starts_with("swf-clip-stream-"),
            "clip stream channel must use `swf-clip-stream-<key>` prefix, got {ch}"
        );
    }
    let unique_channels: BTreeSet<&str> = [
        ch_gc.as_str(),
        ch_parent.as_str(),
        ch_stop.as_str(),
        ch_loop.as_str(),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        unique_channels.len(),
        4,
        "each live clip instance must own a distinct stream channel"
    );

    // Step 1 (Root frame 1):
    // - #20, #21, #23 advance consecutively (0 -> 1) without restarting
    // - #22 runs `stop();` on frame 1 -> emits `StreamStop { channel: ch_stop }`!
    player.step_frame();
    let cmds1 = player.drain_sound_commands(&project);
    assert_eq!(
        cmds1,
        vec![SoundCommand::StreamStop {
            channel: ch_stop.clone()
        }],
        "only Stoppable #22 should emit StreamStop on f1; consecutive streams must keep playing: {cmds1:?}"
    );

    // Step 2 (Root frame 2):
    // - Grandchild #20 reaches frame 2: fires StartSound2("sfx.GrandChime" -> swf-sound-11) AND ends stream span -> StreamStop(ch_gc)!
    // - Looper #23 wraps 1 -> 0: emits StreamStop(ch_loop) + StreamStart(ch_loop, swf-stream-sym-23-1)!
    // - Parent #21 advances 1 -> 2 consecutively.
    player.step_frame();
    let cmds2 = player.drain_sound_commands(&project);
    assert!(
        cmds2.iter().any(|c| matches!(
            c,
            SoundCommand::Play {
                resource,
                sync: SoundSync::Event,
                loops: 2,
                ..
            } if resource == "swf-sound-11"
        )),
        "expected grandchild StartSound2 (`sfx.GrandChime` -> swf-sound-11) on f2: {cmds2:?}"
    );
    assert!(
        cmds2.contains(&SoundCommand::StreamStop {
            channel: ch_gc.clone()
        }),
        "expected StreamStop for grandchild #20 when its stream span ended on f2: {cmds2:?}"
    );
    let stop_loop_pos = cmds2
        .iter()
        .position(|c| {
            c == &SoundCommand::StreamStop {
                channel: ch_loop.clone(),
            }
        })
        .unwrap_or_else(|| panic!("expected StreamStop on loop wrap for {ch_loop}: {cmds2:?}"));
    let restart_loop_pos = cmds2
        .iter()
        .position(|c| {
            matches!(
                c,
                SoundCommand::StreamStart {
                    channel,
                    resource,
                    offset,
                    ..
                } if channel == &ch_loop && resource == "swf-stream-sym-23-1" && offset.abs() < 1e-6
            )
        })
        .unwrap_or_else(|| panic!("expected StreamStart after loop wrap for {ch_loop}: {cmds2:?}"));
    assert!(
        stop_loop_pos < restart_loop_pos,
        "loop wrap must emit StreamStop before restarting StreamStart: {cmds2:?}"
    );

    // Step 3 (Root frame 3):
    // - RemoveObject2(depth 3) removes Looper #23 -> emits StreamStop(ch_loop)!
    // - Grandchild #20 wraps 2 -> 0 -> re-fires StartSound(#10) + StreamStart(ch_gc, swf-stream-sym-20-1)!
    player.step_frame();
    let cmds3 = player.drain_sound_commands(&project);
    assert!(
        cmds3.contains(&SoundCommand::StreamStop {
            channel: ch_loop.clone()
        }),
        "expected StreamStop when Looper #23 was removed by RemoveObject2: {cmds3:?}"
    );
    assert!(
        cmds3.iter().any(|c| matches!(
            c,
            SoundCommand::StreamStart {
                channel,
                resource,
                ..
            } if channel == &ch_gc && resource == "swf-stream-sym-20-1"
        )),
        "expected grandchild #20 stream to restart when wrapping 2 -> 0: {cmds3:?}"
    );

    // Step 4 (Root frame 4):
    // - Move + HasCharacter replaces Parent #21 (and its Grandchild #20) at depth 1 with Shape #30
    // -> must emit StreamStop for BOTH ch_parent and ch_gc!
    player.step_frame();
    let cmds4 = player.drain_sound_commands(&project);
    assert!(
        cmds4.contains(&SoundCommand::StreamStop {
            channel: ch_parent.clone()
        }),
        "Move+HasCharacter replacement must emit StreamStop for replaced parent clip: {cmds4:?}"
    );
    assert!(
        cmds4.contains(&SoundCommand::StreamStop {
            channel: ch_gc.clone()
        }),
        "Move+HasCharacter replacement must emit StreamStop for replaced grandchild clip: {cmds4:?}"
    );
}

#[test]
fn t462_define_button_sound_all_four_transitions_and_imported_button_keyframes() {
    let mut swf = SwfWriter::new();

    for sid in 1..=4u16 {
        swf.tag(14, &make_pcm_define_sound_payload(sid, 4410));
    }
    swf.tag(
        32,
        &make_solid_rect_shape_payload(5, 40, 20, [70, 150, 240, 255]),
    );

    // DefineButton2 (tag 34, button_id = 10)
    let mut btn = SwfWriter::new();
    btn.u16(10);
    btn.u8(0x00); // push button
    btn.u16(0); // action_offset = 0 (no cond actions)
    btn.u8(0x0F); // Hit | Down | Over | Up
    btn.u16(5); // shape #5
    btn.u16(1); // depth 1
    btn.translate_matrix(0, 0);
    btn.u8(0x00); // identity CXFORMWITHALPHA (0 bits)
    btn.u8(0x00); // end of button records
    swf.tag(34, &btn.buf);

    // DefineButtonSound (tag 17) on button #10 with all 4 transition slots:
    // - Slot 0 (OverUpToIdle / rollOut -> Up frame 0): sound #1, Start, in=0.01, out=0.05, loops=2, 1 env pt
    // - Slot 1 (IdleToOverUp / rollOver -> Over frame 1): sound #2, Event, loops=3
    // - Slot 2 (OverUpToOverDown / press -> Down frame 2): sound #3, Start, in=0.02, loops=1
    // - Slot 3 (OverDownToOverUp / release -> Hit frame 3): sound #4, Stop
    let mut bs = SwfWriter::new();
    bs.u16(10);
    // Slot 0
    bs.u16(1);
    bs.u8(0x1f); // Start | Env | Loops | Out | In
    bs.u32(441); // 0.01s
    bs.u32(2205); // 0.05s
    bs.u16(2);
    bs.u8(1);
    bs.u32(0);
    bs.u16(32768);
    bs.u16(16384);
    // Slot 1
    bs.u16(2);
    bs.u8(0x04); // Event | Loops
    bs.u16(3);
    // Slot 2
    bs.u16(3);
    bs.u8(0x15); // Start | Loops | In
    bs.u32(882); // 0.02s
    bs.u16(1);
    // Slot 3
    bs.u16(4);
    bs.u8(0x20); // SyncStop
    swf.tag(17, &bs.buf);

    swf.tag(26, &place_char_payload(1, 10, 20, 20));
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 200, 120, 1);
    let tmp = TempSwfFile::write("button-sound-4slots", &swf_bytes);
    let report = inspect(tmp.path()).expect("inspect DefineButtonSound SWF");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "swf_sound_approximated"),
        "unexpected button sound diagnostics: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import DefineButtonSound SWF");
    project
        .validate()
        .expect("imported Button sound project must validate");

    let btn_sym = project
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Button)
        .expect("imported Button symbol");
    assert_eq!(btn_sym.timeline.frame_count, 4);

    let sound_for_frame = |frame: u32| {
        btn_sym
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find_map(|k| (k.frame == frame).then_some(k.sound.as_ref()).flatten())
            .unwrap_or_else(|| panic!("missing FrameSound on Button frame {frame}"))
    };
    let up_snd = sound_for_frame(0);
    assert_eq!(up_snd.resource, "swf-sound-1");
    assert_eq!(up_snd.sync, SoundSync::Start);
    assert_eq!(up_snd.loops, 2);
    assert_eq!(up_snd.in_point, Some(0.01));
    assert_eq!(up_snd.out_point, Some(0.05));
    assert_eq!(up_snd.envelope_points().len(), 1);

    let over_snd = sound_for_frame(1);
    assert_eq!(over_snd.resource, "swf-sound-2");
    assert_eq!(over_snd.sync, SoundSync::Event);
    assert_eq!(over_snd.loops, 3);

    let down_snd = sound_for_frame(2);
    assert_eq!(down_snd.resource, "swf-sound-3");
    assert_eq!(down_snd.sync, SoundSync::Start);
    assert_eq!(down_snd.in_point, Some(0.02));
    assert_eq!(down_snd.loops, 1);

    let hit_snd = sound_for_frame(3);
    assert_eq!(hit_snd.resource, "swf-sound-4");
    assert_eq!(hit_snd.sync, SoundSync::Stop);

    // Verify SwfBytecodePlayer pointer transitions fire exact SOUNDINFO commands for all 4 slots!
    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer");
    player.step_frame();
    assert!(
        player.drain_sound_commands(&project).is_empty(),
        "button placement alone must not fire transition sounds"
    );

    // Slot 1: IdleToOverUp (rollOver)
    player.trigger_pointer_roll_over();
    let over_cmds = player.drain_sound_commands(&project);
    assert!(
        matches!(
            over_cmds.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Event,
                loops: 3,
                ..
            }] if resource == "swf-sound-2"
        ),
        "rollOver must fire slot 1 (swf-sound-2): {over_cmds:?}"
    );

    // Slot 0: OverUpToIdle (rollOut)
    player.trigger_pointer_roll_out();
    let out_cmds = player.drain_sound_commands(&project);
    assert!(
        matches!(
            out_cmds.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                loops: 2,
                in_point: Some(0.01),
                out_point: Some(0.05),
                envelope,
                ..
            }] if resource == "swf-sound-1"
                && envelope.len() == 1
                && (envelope[0][1] - 1.0).abs() < 1e-3
                && (envelope[0][2] - 0.5).abs() < 1e-3
        ),
        "rollOut must fire slot 0 (swf-sound-1) with full SOUNDINFO: {out_cmds:?}"
    );

    // Slot 2 (press) + Slot 3 (release) via click
    player.trigger_pointer_click();
    let click_cmds = player.drain_sound_commands(&project);
    assert!(
        matches!(
            click_cmds.as_slice(),
            [
                SoundCommand::Play {
                    resource: r_press,
                    sync: SoundSync::Start,
                    loops: 1,
                    in_point: Some(0.02),
                    ..
                },
                SoundCommand::Stop {
                    resource: r_release,
                }
            ] if r_press == "swf-sound-3" && r_release == "swf-sound-4"
        ),
        "click must fire slot 2 (press) then slot 3 (release): {click_cmds:?}"
    );

    // dragOver -> slot 2
    player.trigger_pointer_drag_over();
    let drag_over_cmds = player.drain_sound_commands(&project);
    assert!(
        matches!(
            drag_over_cmds.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                in_point: Some(0.02),
                ..
            }] if resource == "swf-sound-3"
        ),
        "dragOver must fire slot 2: {drag_over_cmds:?}"
    );

    // dragOut -> slot 0
    player.trigger_pointer_drag_out();
    let drag_out_cmds = player.drain_sound_commands(&project);
    assert!(
        matches!(
            drag_out_cmds.as_slice(),
            [SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                loops: 2,
                ..
            }] if resource == "swf-sound-1"
        ),
        "dragOut must fire slot 0: {drag_out_cmds:?}"
    );
}

#[allow(clippy::too_many_arguments)]
fn build_morph_gradient_tag_payload(
    is_morph2: bool,
    char_id: u16,
    fill_type: u8,
    spread_code: u8,
    linear_rgb: bool,
    start_matrix: (f64, f64, i32, i32),
    end_matrix: (f64, f64, i32, i32),
    stops: &[(u8, [u8; 4], u8, [u8; 4])],
    focal_points: Option<(f64, f64)>,
    collapsed_end_edges: bool,
) -> Vec<u8> {
    let mut start_edges = SwfWriter::new();
    start_edges.write_ub(1, 4); // numFillBits = 1
    start_edges.write_ub(0, 4); // numLineBits = 0
    start_edges.write_ub(0, 1); // non-edge
    start_edges.write_ub(0, 1);
    start_edges.write_ub(0, 1);
    start_edges.write_ub(1, 1); // StateFillStyle1
    start_edges.write_ub(0, 1);
    start_edges.write_ub(1, 1); // StateMoveTo
    start_edges.write_ub(1, 5);
    start_edges.write_sb(0, 1);
    start_edges.write_sb(0, 1);
    start_edges.write_ub(1, 1); // fillStyle1 = 1
    let w = 60 * 20;
    let h = 60 * 20;
    for (dx, dy) in [(w, 0), (0, h), (-w, 0), (0, -h)] {
        start_edges.write_ub(1, 1);
        start_edges.write_ub(1, 1);
        let nbits = 13u8;
        start_edges.write_ub(u32::from(nbits), 4);
        start_edges.write_ub(1, 1);
        start_edges.write_sb(dx, nbits + 2);
        start_edges.write_sb(dy, nbits + 2);
    }
    start_edges.write_ub(0, 6);
    start_edges.align();

    let mut end_edges = SwfWriter::new();
    if collapsed_end_edges {
        // Degenerate EndEdges with 0 edges (numFillBits=0, numLineBits=0, EndShapeRecord)
        end_edges.u8(0x00);
        end_edges.u8(0x00);
    } else {
        // SWF EndEdges omits style changes and relies on 1:1 edge correspondence with StartEdges!
        end_edges.write_ub(0, 4); // numFillBits = 0
        end_edges.write_ub(0, 4); // numLineBits = 0
        end_edges.write_ub(0, 1); // non-edge MoveTo(20px, 10px)
        end_edges.write_ub(0, 4);
        end_edges.write_ub(1, 1); // StateMoveTo
        end_edges.write_ub(12, 5);
        end_edges.write_sb(20 * 20, 12);
        end_edges.write_sb(10 * 20, 12);
        let ew = 80 * 20;
        let eh = 80 * 20;
        for (dx, dy) in [(ew, 0), (0, eh), (-ew, 0), (0, -eh)] {
            end_edges.write_ub(1, 1);
            end_edges.write_ub(1, 1);
            let nbits = 13u8;
            end_edges.write_ub(u32::from(nbits), 4);
            end_edges.write_ub(1, 1);
            end_edges.write_sb(dx, nbits + 2);
            end_edges.write_sb(dy, nbits + 2);
        }
        end_edges.write_ub(0, 6);
        end_edges.align();
    }

    let mut styles = SwfWriter::new();
    styles.u8(1); // 1 MorphFillStyle
    styles.u8(fill_type);
    styles.scale_translate_matrix(
        start_matrix.0,
        start_matrix.1,
        start_matrix.2,
        start_matrix.3,
    );
    styles.scale_translate_matrix(end_matrix.0, end_matrix.1, end_matrix.2, end_matrix.3);
    let grad_hdr =
        ((spread_code & 0x03) << 6) | (if linear_rgb { 1 << 4 } else { 0 }) | (stops.len() as u8);
    styles.u8(grad_hdr);
    for &(sr, sc, er, ec) in stops {
        styles.u8(sr);
        styles.bytes(&sc);
        styles.u8(er);
        styles.bytes(&ec);
    }
    if fill_type == 0x13 {
        let (sf, ef) = focal_points.unwrap_or((0.0, 0.0));
        styles.i16((sf * 256.0).round() as i16);
        styles.i16((ef * 256.0).round() as i16);
    }
    styles.u8(0); // 0 MorphLineStyles

    let offset = (styles.buf.len() + start_edges.buf.len()) as u32;
    let mut tag = SwfWriter::new();
    tag.u16(char_id);
    tag.rect(0, 60 * 20, 0, 60 * 20);
    tag.rect(0, 100 * 20, 0, 100 * 20);
    if is_morph2 {
        tag.rect(0, 60 * 20, 0, 60 * 20); // StartEdgeBounds
        tag.rect(0, 100 * 20, 0, 100 * 20); // EndEdgeBounds
        tag.u8(0x01); // UsesNonScalingStrokes = 0, UsesScalingStrokes = 1
    }
    tag.u32(offset);
    tag.bytes(&styles.buf);
    tag.bytes(&start_edges.buf);
    tag.bytes(&end_edges.buf);
    tag.buf
}

fn place_morph_with_ratio(depth: u16, char_id: u16, ratio: u16, is_move: bool) -> Vec<u8> {
    let mut p = SwfWriter::new();
    if is_move {
        p.u8(0x11); // HasRatio(0x10) | Move(0x01)
        p.u16(depth);
        p.u16(ratio);
    } else {
        p.u8(0x16); // HasRatio(0x10) | HasMatrix(0x04) | HasCharacter(0x02)
        p.u16(depth);
        p.u16(char_id);
        p.translate_matrix(4 * 20, 4 * 20);
        p.u16(ratio);
    }
    p.buf
}

#[test]
fn t462_morph_shape_and_morph_shape2_linear_radial_and_focal_gradients_import() {
    let mut swf = SwfWriter::new();

    // #1: DefineMorphShape (tag 46) with 0x10 Linear gradient (Reflect, linear_rgb=true, 3 stops)
    let linear_stops = [
        (0, [255, 20, 40, 255], 16, [20, 220, 255, 255]),
        (120, [240, 240, 10, 200], 180, [180, 40, 240, 220]),
        (255, [10, 40, 200, 255], 255, [40, 250, 90, 128]),
    ];
    swf.tag(
        46,
        &build_morph_gradient_tag_payload(
            false,
            1,
            0x10,
            1, // Reflect
            true,
            (0.04, 0.04, 30 * 20, 30 * 20),
            (0.08, 0.06, 60 * 20, 50 * 20),
            &linear_stops,
            None,
            false,
        ),
    );

    // #2: DefineMorphShape (tag 46) with 0x12 Radial gradient (Extend, linear_rgb=false, 2 stops)
    let radial_stops = [
        (0, [255, 255, 255, 255], 0, [255, 120, 0, 255]),
        (255, [0, 80, 200, 255], 255, [20, 0, 60, 180]),
    ];
    swf.tag(
        46,
        &build_morph_gradient_tag_payload(
            false,
            2,
            0x12,
            0, // Extend
            false,
            (0.05, 0.05, 30 * 20, 30 * 20),
            (0.09, 0.09, 55 * 20, 55 * 20),
            &radial_stops,
            None,
            false,
        ),
    );

    // #3: DefineMorphShape2 (tag 84) with 0x13 Focal Radial gradient (Repeat, linear_rgb=true,
    // StartFocalPoint = -0.5 -> EndFocalPoint = +0.75)
    let focal_stops = [
        (0, [255, 240, 180, 255], 10, [120, 255, 220, 255]),
        (255, [90, 10, 160, 255], 245, [10, 90, 220, 255]),
    ];
    swf.tag(
        84,
        &build_morph_gradient_tag_payload(
            true,
            3,
            0x13,
            2, // Repeat
            true,
            (0.045, 0.045, 28 * 20, 28 * 20),
            (0.075, 0.075, 58 * 20, 52 * 20),
            &focal_stops,
            Some((-0.5, 0.75)),
            false,
        ),
    );

    // #4: DefineMorphShape2 (tag 84) with 0x13 Focal Radial gradient AND collapsed EndEdges!
    swf.tag(
        84,
        &build_morph_gradient_tag_payload(
            true,
            4,
            0x13,
            1, // Reflect
            true,
            (0.04, 0.04, 25 * 20, 25 * 20),
            (0.07, 0.07, 50 * 20, 45 * 20),
            &focal_stops,
            Some((-0.25, 0.625)),
            true,
        ),
    );

    // Place all 4 morphs across 3 frames (ratio 0 -> 32768 -> 65535)
    for (depth, cid) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        swf.tag(26, &place_morph_with_ratio(depth, cid, 0, false));
    }
    swf.tag(1, &[]);
    for (depth, cid) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        swf.tag(26, &place_morph_with_ratio(depth, cid, 32768, true));
    }
    swf.tag(1, &[]);
    for (depth, cid) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        swf.tag(26, &place_morph_with_ratio(depth, cid, 65535, true));
    }
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 160, 160, 3);
    let tmp = TempSwfFile::write("morph-gradients-46-84", &swf_bytes);
    let project = import_project(tmp.path()).expect("import morph gradients SWF");
    project
        .validate()
        .expect("imported morph gradients project must validate");

    let extract_start_end_gradients = |sym_id: &str| {
        let sym = project
            .symbol(sym_id)
            .unwrap_or_else(|| panic!("missing morph symbol {sym_id}"));
        let kfs = &sym.timeline.layers[0].keyframes;
        assert_eq!(
            kfs.len(),
            2,
            "{sym_id} must have start and end shape-tween keyframes"
        );
        let g0 = match &kfs[0].elements[0].kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } => g,
            other => panic!("expected PaintedPath fill_gradient on {sym_id} start, got {other:?}"),
        };
        let g1 = match &kfs[1].elements[0].kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } => g,
            other => panic!("expected PaintedPath fill_gradient on {sym_id} end, got {other:?}"),
        };
        (g0, g1)
    };

    // Check #1 (0x10 Linear, Reflect, linear_rgb=true)
    let (lin0, lin1) = extract_start_end_gradients("swf-sym-1");
    assert_eq!(lin0.kind, GradientKind::Linear);
    assert_eq!(lin1.kind, GradientKind::Linear);
    assert_eq!(lin0.spread, GradientSpread::Reflect);
    assert_eq!(lin1.spread, GradientSpread::Reflect);
    assert!(lin0.linear_rgb && lin1.linear_rgb);
    assert_eq!(lin0.stops.len(), 3);
    assert_eq!(lin1.stops.len(), 3);
    assert_eq!(lin0.stops[0].ratio, 0);
    assert_eq!(lin1.stops[0].ratio, 16);
    assert_eq!(lin0.stops[1].color, [240, 240, 10, 200]);
    assert_eq!(lin1.stops[1].color, [180, 40, 240, 220]);
    assert_eq!(lin1.stops[2].color, [40, 250, 90, 128]);
    assert!((lin0.matrix[4] - 30.0).abs() < 0.1);
    assert!((lin1.matrix[4] - 60.0).abs() < 0.1);
    assert!((lin1.matrix[5] - 50.0).abs() < 0.1);

    // Check #2 (0x12 Radial, Extend, linear_rgb=false)
    let (rad0, rad1) = extract_start_end_gradients("swf-sym-2");
    assert_eq!(rad0.kind, GradientKind::Radial);
    assert_eq!(rad1.kind, GradientKind::Radial);
    assert_eq!(rad0.spread, GradientSpread::Extend);
    assert!(!rad0.linear_rgb && !rad1.linear_rgb);
    assert_eq!(rad0.stops[1].color, [0, 80, 200, 255]);
    assert_eq!(rad1.stops[1].color, [20, 0, 60, 180]);
    assert!((rad0.matrix[4] - 30.0).abs() < 0.1);
    assert!((rad1.matrix[4] - 55.0).abs() < 0.1);

    // Check #3 (DefineMorphShape2 0x13 Focal Radial, Repeat, linear_rgb=true, -0.5 -> +0.75)
    let (foc0, foc1) = extract_start_end_gradients("swf-sym-3");
    assert_eq!(foc0.kind, GradientKind::Radial);
    assert_eq!(foc1.kind, GradientKind::Radial);
    assert_eq!(foc0.spread, GradientSpread::Repeat);
    assert_eq!(foc1.spread, GradientSpread::Repeat);
    assert!(foc0.linear_rgb && foc1.linear_rgb);
    assert!((foc0.focal_point_ratio - (-0.5)).abs() < 1e-3);
    assert!((foc1.focal_point_ratio - 0.75).abs() < 1e-3);
    assert!((foc0.matrix[4] - 28.0).abs() < 0.1);
    assert!((foc1.matrix[4] - 58.0).abs() < 0.1);

    // Check #4 (Collapsed EndEdges must STILL preserve end stops, matrix, and focal point!)
    let (col0, col1) = extract_start_end_gradients("swf-sym-4");
    assert!((col0.focal_point_ratio - (-0.25)).abs() < 1e-3);
    assert!((col1.focal_point_ratio - 0.625).abs() < 1e-3);
    assert_eq!(col0.stops[0].color, [255, 240, 180, 255]);
    assert_eq!(col1.stops[0].color, [120, 255, 220, 255]);
    assert!((col1.matrix[4] - 50.0).abs() < 0.1);

    // Midpoint (frame 1, ratio = 32768) must render visible interpolated pixels cleanly via CorePlayer
    let mut core_player = CorePlayer::new(&project);
    core_player.seek(&project, 1);
    let midpoint_rgba = core_player
        .render_snapshot_rgba(&project, 160, 160)
        .expect("render midpoint morph gradients");
    let non_bg = midpoint_rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|&&px| px != project.background)
        .count();
    assert!(
        non_bg > 500,
        "midpoint morph gradient render must draw visible pixels (got {non_bg})"
    );
}

#[test]
fn t462_ruffle_flash_player_differential_oracle_suite() {
    let Some(swfs_dir) = find_ruffle_swfs_dir() else {
        // Optional external checkout (`target/internet-flash-corpus/ruffle`) is absent in this environment.
        return;
    };

    // Exclude the 11 arithmetic fixtures already covered in `T-465` and the fixtures in `T-451`.
    let already_covered: BTreeSet<&str> = [
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
        "avm1/goto_label",
        "avm1/clip_events",
    ]
    .into_iter()
    .collect();

    // Playable non-AIR Flash Player reference movies across AVM2 and AVM1 (61 additional movies):
    // bitwise, modulo/divide, relational & equality, branches, coercions, constructors, closures,
    // call/throw/with, arrays, AVM1 goto/execution-order/doInitAction, SWF6-8 bitwise & strict-eq,
    // typeof, delete, looping, and with-return.
    let reference_movies = [
        // AVM2 (42 movies)
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
        "avm2/function_call_default",
        "avm2/function_call_via_call",
        "avm2/throw",
        "avm2/with",
        "avm2/array_literal",
        "avm2/array_length",
        "avm2/array_join",
        // AVM1 (19 movies)
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
    ];

    let mut verified_count = 0usize;
    let mut total_verified_lines = 0usize;

    for rel in reference_movies {
        assert!(
            !already_covered.contains(rel),
            "{rel} overlaps earlier suite; keep T-462 movies strictly additional"
        );
        let dir = swfs_dir.join(rel);
        let swf_path = dir.join("test.swf");
        let out_path = dir.join("output.txt");
        let toml_path = dir.join("test.toml");
        assert!(
            swf_path.is_file() && out_path.is_file(),
            "expected upstream Ruffle fixture files for {rel} under {}",
            dir.display()
        );
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

        let expected_raw = std::fs::read_to_string(&out_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", out_path.display()));
        let expected: Vec<String> = expected_raw.lines().map(str::to_owned).collect();

        let mut player = SwfBytecodePlayer::from_path(&swf_path)
            .unwrap_or_else(|e| panic!("SwfBytecodePlayer::from_path({rel}): {e}"));
        let mut actual = Vec::new();
        for _ in 0..ticks {
            let step = player.step_frame();
            assert!(
                step.unsupported_diagnostics.is_empty(),
                "{rel} emitted unsupported diagnostics: {:?}",
                step.unsupported_diagnostics
            );
            actual.extend(step.traces);
        }
        assert_eq!(
            actual, expected,
            "Flash Player 1:1 output.txt mismatch on {rel} (ticks={ticks})"
        );

        let report = inspect(&swf_path).unwrap_or_else(|e| panic!("inspect({rel}): {e}"));
        assert!(
            report.can_import,
            "{rel} inspect can_import failed: {report:?}"
        );
        let project =
            import_project(&swf_path).unwrap_or_else(|e| panic!("import_project({rel}): {e}"));
        project
            .validate()
            .unwrap_or_else(|e| panic!("{rel} imported project failed validate(): {e}"));

        verified_count += 1;
        total_verified_lines += expected.len();
    }

    assert!(
        verified_count >= 60,
        "expected at least 20+ (60+) additional Ruffle reference movies verified 1:1, got {verified_count}"
    );
    assert!(
        total_verified_lines >= 10_000,
        "expected >10,000 Flash Player trace lines verified 1:1, got {total_verified_lines}"
    );
}
