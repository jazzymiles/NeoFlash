//! Track 2 (`T-451`) verification suite for `neoflash-import`:
//! - Exact Flash Player `output.txt` trace-oracle match across all upstream Ruffle AVM1/AVM2 SWFs
//! - Visual & structural verification of upstream MX SWFs (`DefineButton-MX`, `DefineMorphShape-MX`, `DefineText2-MX`)
//! - Full AVM1 `CLIPACTIONS` lifecycle (`Initialize` -> `Construct` -> `Load` -> `EnterFrame` -> recursive `Unload` on `RemoveObject2` & `Move+HasCharacter`) + SWF 6/7+ interactive clip events
//! - `InlineText` (`DefineText2` & `DefineEditText`) non-opaque `TextColor` alpha × `PlaceObject2` `CXFORMWITHALPHA` multiplication
//! - `DefineVideoStream` `ElementKind::Video` viewport sizing (`0x0` -> `160x120`, `1..=8192` clamp) and accurate `swf_video_viewport` diagnostics

use neoflash_core::{ElementKind, Player as CorePlayer};
use neoflash_import::{SwfBytecodePlayer, import_project, inspect};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
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

    fn translate_matrix(&mut self, tx_twips: i32, ty_twips: i32) {
        self.align();
        self.write_ub(0, 1); // HasScale = 0
        self.write_ub(0, 1); // HasRotate = 0
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

fn avm1_trace_bytes(msg: &str) -> Vec<u8> {
    let mut b = Vec::new();
    let str_bytes = msg.as_bytes();
    let len = (1 + str_bytes.len() + 1) as u16;
    b.push(0x96); // ActionPush
    b.extend_from_slice(&len.to_le_bytes());
    b.push(0x00); // String
    b.extend_from_slice(str_bytes);
    b.push(0x00);
    b.push(0x26); // ActionTrace
    b.push(0x00); // ActionEndFlag
    b
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
    // StyleChangeRecord: moveTo(0, 0) + fillStyle1 = 1
    sw.write_ub(0, 1);
    sw.write_ub(0, 1);
    sw.write_ub(0, 1);
    sw.write_ub(1, 1);
    sw.write_ub(0, 1);
    sw.write_ub(1, 1);
    sw.write_ub(1, 5);
    sw.write_sb(0, 1);
    sw.write_sb(0, 1);
    sw.write_ub(1, 1);
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

fn count_non_bg_pixels(rgba: &[u8], bg: [u8; 4]) -> usize {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|&&px| px != bg)
        .count()
}

#[test]
fn t451_ruffle_flash_oracle_suite_exact_trace_and_visual_mx_swfs() {
    let ruffle_dir = repo_root().join("examples/fla-corpus/upstream/ruffle");

    let oracle_cases: [(&str, usize, usize); 5] = [
        ("avm1-frame-labels", 4, 17),
        ("avm1-clip-events", 4, 19),
        ("avm1-nested-button-text", 1, 0),
        ("avm2-button-frame-scripts", 2, 28),
        ("avm2-nested-construction", 3, 18),
    ];

    for (name, ticks, expected_line_count) in oracle_cases {
        let swf_path = ruffle_dir.join(name).join("test.swf");
        let out_path = ruffle_dir.join(name).join("output.txt");
        let swf_bytes =
            std::fs::read(&swf_path).unwrap_or_else(|e| panic!("read {}: {e}", swf_path.display()));
        let expected_raw = std::fs::read_to_string(&out_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", out_path.display()));
        let expected_lines: Vec<String> = expected_raw.lines().map(str::to_owned).collect();
        assert_eq!(
            expected_lines.len(),
            expected_line_count,
            "unexpected oracle line count for {name}"
        );

        let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes)
            .unwrap_or_else(|e| panic!("SwfBytecodePlayer::from_bytes({name}): {e}"));
        let mut all_traces = Vec::new();
        for _ in 0..ticks {
            let step = player.step_frame();
            assert!(
                step.unsupported_diagnostics.is_empty(),
                "{name} emitted unsupported opcode diagnostics: {:?}",
                step.unsupported_diagnostics
            );
            all_traces.extend(step.traces);
        }
        assert_eq!(
            all_traces, expected_lines,
            "Flash Player trace oracle mismatch on {name}"
        );

        let report = inspect(&swf_path).unwrap_or_else(|e| panic!("inspect({name}): {e}"));
        assert!(
            report.can_import,
            "{name} inspect can_import failed: {report:?}"
        );
        let project =
            import_project(&swf_path).unwrap_or_else(|e| panic!("import_project({name}): {e}"));
        project
            .validate()
            .unwrap_or_else(|e| panic!("{name} imported project failed validate(): {e}"));

        if name == "avm1-clip-events" {
            // Only the 5 SWF-internal `initialize` (3) & `construct` (2) records emit
            // `swf_clip_event_approximated`; all standard SWF 6/7+ clip/button events
            // (`unload`, `enterFrame`, `load`, `rollOver`, `rollOut`, etc.) decompile natively!
            let approx_clip_warnings: Vec<_> = report
                .diagnostics
                .iter()
                .filter(|d| d.code == "swf_clip_event_approximated")
                .collect();
            assert_eq!(
                approx_clip_warnings.len(),
                5,
                "avm1-clip-events must only warn on the 5 SWF-internal initialize/construct records (not 10): {approx_clip_warnings:?}"
            );
        }

        if name == "avm1-nested-button-text" {
            let overlay = player.project_with_display_list_overlay(&project, &project.root, 0);
            overlay.validate().expect("nested button overlay validates");
            let core_player = CorePlayer::new(&overlay);
            let rgba = core_player
                .render_snapshot_rgba(&overlay, overlay.width, overlay.height)
                .expect("render nested button RGBA");
            assert!(
                count_non_bg_pixels(&rgba, overlay.background) > 0,
                "avm1-nested-button-text must render visible button/text pixels"
            );
        }
    }

    // Visual MX SWF fixtures: DefineButton-MX, DefineMorphShape-MX, DefineText2-MX
    let mx_cases = [
        ("mx-button/DefineButton-MX.swf", 1usize),
        ("mx-shape-tween/DefineMorphShape-MX.swf", 6usize),
        ("mx-static-text/DefineText2-MX.swf", 1usize),
    ];
    for (rel, sample_frames) in mx_cases {
        let swf_path = ruffle_dir.join(rel);
        let swf_bytes = std::fs::read(&swf_path).expect("read MX SWF");
        let project = import_project(&swf_path).expect("import MX SWF");
        project.validate().expect("MX SWF project validates");

        let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("MX SwfBytecodePlayer");
        let mut frame_hashes = Vec::new();
        for f in 0..sample_frames {
            let st = player.step_frame();
            assert!(
                !st.display_list.is_empty(),
                "{rel} frame {f} display list empty"
            );
            let overlay =
                player.project_with_display_list_overlay(&project, &project.root, st.frame);
            overlay.validate().expect("MX overlay validates");
            let core_player = CorePlayer::new(&overlay);
            let rgba = core_player
                .render_snapshot_rgba(&overlay, overlay.width, overlay.height)
                .expect("render MX RGBA");
            assert!(
                count_non_bg_pixels(&rgba, overlay.background) > 0,
                "{rel} frame {f} rendered zero foreground pixels"
            );
            frame_hashes.push(rgba);
        }
        if rel.contains("DefineMorphShape") {
            assert_ne!(
                frame_hashes.first(),
                frame_hashes.last(),
                "DefineMorphShape-MX must visibly morph RGBA pixels across frames"
            );
        }
    }
}

#[test]
fn t451_avm1_clipactions_lifecycle_recursive_unload_and_swf7_events() {
    // Exact SWF ClipEventFlags (32-bit LE u32):
    const CLIP_LOAD: u32 = 0x0000_0001;
    const CLIP_ENTER_FRAME: u32 = 0x0000_0002;
    const CLIP_UNLOAD: u32 = 0x0000_0004;
    const CLIP_MOUSE_MOVE: u32 = 0x0000_0008;
    const CLIP_DATA: u32 = 0x0000_0100;
    const CLIP_INITIALIZE: u32 = 0x0000_0200;
    const CLIP_RELEASE_OUTSIDE: u32 = 0x0000_1000;
    const CLIP_ROLL_OVER: u32 = 0x0000_2000;
    const CLIP_ROLL_OUT: u32 = 0x0000_4000;
    const CLIP_DRAG_OVER: u32 = 0x0000_8000;
    const CLIP_DRAG_OUT: u32 = 0x0001_0000;
    const CLIP_KEY_PRESS: u32 = 0x0002_0000;
    const CLIP_CONSTRUCT: u32 = 0x0004_0000;

    let encode_clip_actions_raw = |records: &[(u32, Option<u8>, Vec<u8>)]| -> Vec<u8> {
        let mut w = SwfWriter::new();
        w.u16(0); // reserved
        let all_flags = records.iter().fold(0u32, |acc, (f, _, _)| acc | *f);
        w.u32(all_flags);
        for (flags, key_code, code) in records {
            w.u32(*flags);
            let rec_size = code.len() + usize::from(key_code.is_some());
            w.u32(rec_size as u32);
            if let Some(kc) = key_code {
                w.u8(*kc);
            }
            w.bytes(code);
        }
        w.u32(0); // end of clip action records (u32 for SWF 6+)
        w.buf
    };

    // Part A: Pure native SWF 6/7+ clip & pointer/key events -> MUST produce ZERO `swf_clip_event_approximated` warnings!
    let mut native_swf = SwfWriter::new();
    let shape1 = make_solid_rect_shape_payload(1, 24, 24, [220, 60, 80, 255]);
    native_swf.tag(32, &shape1);

    let mut sp_native = SwfWriter::new();
    sp_native.u16(2);
    sp_native.u16(1);
    let mut sp_po = SwfWriter::new();
    sp_po.u8(0x06);
    sp_po.u16(1);
    sp_po.u16(1);
    sp_po.translate_matrix(0, 0);
    sp_native.tag(26, &sp_po.buf);
    sp_native.tag(1, &[]);
    sp_native.tag(0, &[]);
    native_swf.tag(39, &sp_native.buf);

    let native_records = encode_clip_actions_raw(&[
        (CLIP_LOAD, None, avm1_trace_bytes("n:load")),
        (CLIP_ENTER_FRAME, None, avm1_trace_bytes("n:enter")),
        (CLIP_UNLOAD, None, avm1_trace_bytes("n:unload")),
        (CLIP_MOUSE_MOVE, None, avm1_trace_bytes("n:move")),
        (CLIP_DATA, None, avm1_trace_bytes("n:data")),
        (CLIP_ROLL_OVER, None, avm1_trace_bytes("n:rollOver")),
        (CLIP_ROLL_OUT, None, avm1_trace_bytes("n:rollOut")),
        (CLIP_DRAG_OVER, None, avm1_trace_bytes("n:dragOver")),
        (CLIP_DRAG_OUT, None, avm1_trace_bytes("n:dragOut")),
        (
            CLIP_RELEASE_OUTSIDE,
            None,
            avm1_trace_bytes("n:releaseOutside"),
        ),
        (CLIP_KEY_PRESS, Some(32), avm1_trace_bytes("n:keySpace")),
    ]);
    let mut n_po = SwfWriter::new();
    n_po.u8(0xA6);
    n_po.u16(1);
    n_po.u16(2);
    n_po.translate_matrix(0, 0);
    n_po.c_str("btnClip");
    n_po.bytes(&native_records);
    native_swf.tag(26, &n_po.buf);
    native_swf.tag(1, &[]);
    let native_bytes = native_swf.into_swf(7, 100, 100, 1);

    let tmp_native =
        std::env::temp_dir().join(format!("t451-native-clip-{}.swf", std::process::id()));
    std::fs::write(&tmp_native, &native_bytes).expect("write temp native swf");
    let native_report = inspect(&tmp_native).expect("inspect native SWF 7 clip events");
    let native_proj = import_project(&tmp_native).expect("import native SWF 7 clip events");
    let _ = std::fs::remove_file(&tmp_native);
    native_proj
        .validate()
        .expect("native SWF 7 project validates");

    let approx_native: Vec<_> = native_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_clip_event_approximated")
        .collect();
    assert!(
        approx_native.is_empty(),
        "All SWF 6/7+ native clip/pointer/key events must import with 0 swf_clip_event_approximated warnings: {approx_native:?}"
    );
    let combined_sources: String = native_proj
        .symbol(&native_proj.root)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes[0]
        .elements[0]
        .scripts
        .iter()
        .map(|s| s.script.source.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for snippet in [
        "onClipEvent(unload)",
        "onClipEvent(mouseMove)",
        "onClipEvent(data)",
        "on(rollOver)",
        "on(rollOut)",
        "on(dragOver)",
        "on(dragOut)",
        "on(releaseOutside)",
        "on(keyPress \"<Space>\")",
    ] {
        assert!(
            combined_sources.contains(snippet),
            "missing native decompiled handler `{snippet}` in:\n{combined_sources}"
        );
    }

    // Part B: Full parent + nested child Initialize -> Construct -> Load -> interactive triggers -> Move+HasCharacter recursive Unload
    let mut swf = SwfWriter::new();
    swf.tag(32, &shape1);
    swf.tag(39, &sp_native.buf); // sprite 2 = leaf child

    let child_records = encode_clip_actions_raw(&[
        (CLIP_INITIALIZE, None, avm1_trace_bytes("child:initialize")),
        (CLIP_CONSTRUCT, None, avm1_trace_bytes("child:construct")),
        (CLIP_LOAD, None, avm1_trace_bytes("child:load")),
        (CLIP_UNLOAD, None, avm1_trace_bytes("child:unload")),
    ]);
    let mut parent_tl = SwfWriter::new();
    parent_tl.u16(3); // sprite 3 = parent containing child sprite 2
    parent_tl.u16(1);
    let mut parent_po = SwfWriter::new();
    parent_po.u8(0xA6);
    parent_po.u16(1);
    parent_po.u16(2);
    parent_po.translate_matrix(0, 0);
    parent_po.c_str("inner");
    parent_po.bytes(&child_records);
    parent_tl.tag(26, &parent_po.buf);
    parent_tl.tag(1, &[]);
    parent_tl.tag(0, &[]);
    swf.tag(39, &parent_tl.buf);

    // Replacement Sprite 4 on root depth 1 (tests Move + HasCharacter replacement Unload!)
    let mut repl_tl = SwfWriter::new();
    repl_tl.u16(4);
    repl_tl.u16(1);
    repl_tl.tag(26, &sp_po.buf);
    repl_tl.tag(1, &[]);
    repl_tl.tag(0, &[]);
    swf.tag(39, &repl_tl.buf);

    let parent_records = encode_clip_actions_raw(&[
        (CLIP_INITIALIZE, None, avm1_trace_bytes("parent:initialize")),
        (CLIP_CONSTRUCT, None, avm1_trace_bytes("parent:construct")),
        (CLIP_LOAD, None, avm1_trace_bytes("parent:load")),
        (
            CLIP_ENTER_FRAME,
            None,
            avm1_trace_bytes("parent:enterFrame"),
        ),
        (CLIP_ROLL_OVER, None, avm1_trace_bytes("parent:rollOver")),
        (CLIP_ROLL_OUT, None, avm1_trace_bytes("parent:rollOut")),
        (CLIP_DRAG_OVER, None, avm1_trace_bytes("parent:dragOver")),
        (CLIP_DRAG_OUT, None, avm1_trace_bytes("parent:dragOut")),
        (
            CLIP_RELEASE_OUTSIDE,
            None,
            avm1_trace_bytes("parent:releaseOutside"),
        ),
        (
            CLIP_KEY_PRESS,
            Some(32),
            avm1_trace_bytes("parent:keyPressSpace"),
        ),
        (CLIP_DATA, None, avm1_trace_bytes("parent:data")),
        (CLIP_UNLOAD, None, avm1_trace_bytes("parent:unload")),
    ]);
    let mut root_po0 = SwfWriter::new();
    root_po0.u8(0xA6);
    root_po0.u16(1);
    root_po0.u16(3);
    root_po0.translate_matrix(0, 0);
    root_po0.c_str("outer");
    root_po0.bytes(&parent_records);
    swf.tag(26, &root_po0.buf);
    swf.tag(12, &avm1_trace_bytes("root:f0"));
    swf.tag(1, &[]);

    // Frame 1: Move + HasCharacter replacement at depth 1 with Sprite 4 -> recursive Unload!
    let mut root_po1 = SwfWriter::new();
    root_po1.u8(0x07); // HasMatrix | HasCharacter | Move
    root_po1.u16(1);
    root_po1.u16(4);
    root_po1.translate_matrix(0, 0);
    swf.tag(26, &root_po1.buf);
    swf.tag(12, &avm1_trace_bytes("root:f1"));
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(7, 100, 100, 2);
    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer");
    let f0 = player.step_frame();
    assert_eq!(
        f0.traces,
        vec![
            "parent:initialize",
            "child:initialize",
            "parent:construct",
            "child:construct",
            "parent:load",
            "parent:enterFrame",
            "child:load",
            "root:f0",
        ]
    );

    let base_len = player.trace_output().len();
    player.trigger_pointer_roll_over();
    player.trigger_pointer_roll_out();
    player.trigger_pointer_drag_over();
    player.trigger_pointer_drag_out();
    player.trigger_pointer_release_outside();
    player.trigger_key_press(32);
    player.trigger_data_event();
    assert_eq!(
        &player.trace_output()[base_len..],
        &[
            "parent:rollOver",
            "parent:rollOut",
            "parent:dragOver",
            "parent:dragOut",
            "parent:releaseOutside",
            "parent:keyPressSpace",
            "parent:data",
        ]
    );

    let f1 = player.step_frame();
    assert!(
        f1.traces.contains(&"child:unload".to_owned())
            && f1.traces.contains(&"parent:unload".to_owned()),
        "Move+HasCharacter replacement must fire recursive Unload on child and parent: {:?}",
        f1.traces
    );
    let parent_unload_idx = f1.traces.iter().position(|s| s == "parent:unload").unwrap();
    let child_unload_idx = f1.traces.iter().position(|s| s == "child:unload").unwrap();
    let root_f1_idx = f1.traces.iter().position(|s| s == "root:f1").unwrap();
    assert!(
        parent_unload_idx < root_f1_idx && child_unload_idx < root_f1_idx,
        "Unload must precede frame DoAction: {:?}",
        f1.traces
    );
}

#[test]
fn button_transition_flags_distinguish_drag_release_and_keypress() {
    let mut movie = SwfWriter::new();
    for (id, menu) in [(1u16, false), (2, true)] {
        let mut button = SwfWriter::new();
        button.u16(id);
        button.u8(u8::from(menu));
        button.u16(3);
        button.u8(0); // Empty display record list.
        for (condition, message) in [
            (0x0010u16, "push-drag-out"),
            (0x0020, "push-drag-over"),
            (0x0040, "push-release-outside"),
            (0x0080, "menu-drag-over"),
            (0x0100, "menu-drag-out"),
            (0x0008, "release"),
            (0x0004, "press"),
            (u16::from(b'k') << 9, "key"),
        ] {
            let actions = avm1_trace_bytes(&format!("{id}-{message}"));
            button.u16(if message == "key" {
                0
            } else {
                (actions.len() + 4) as u16
            });
            button.u16(condition);
            button.bytes(&actions);
        }
        movie.tag(34, &button.buf);
        let mut place = SwfWriter::new();
        place.u8(0x02);
        place.u16(id);
        place.u16(id);
        movie.tag(26, &place.buf);
    }
    movie.tag(1, &[]);
    let mut player = SwfBytecodePlayer::from_bytes(&movie.into_swf(8, 100, 100, 1)).unwrap();
    player.step_frame();
    player.trigger_pointer_drag_out();
    player.trigger_pointer_drag_over();
    player.trigger_pointer_release_outside();
    player.trigger_pointer_click();
    player.trigger_key_down(Some(b'k'));
    player.trigger_key_up(Some(b'k'));
    player.trigger_key_press(b'k');
    assert_eq!(
        player.trace_output(),
        [
            "1-push-drag-out",
            "2-menu-drag-out",
            "1-push-drag-over",
            "2-menu-drag-over",
            "1-push-release-outside",
            "1-press",
            "2-press",
            "1-release",
            "2-release",
            "1-key",
            "2-key"
        ]
    );
}

#[test]
fn native_stream_uses_its_own_start_and_event_audio_preserves_controls() {
    use neoflash_core::{SoundCommand, SoundSync};
    let mut film = SwfWriter::new();
    let mut sound = SwfWriter::new();
    sound.u16(4);
    sound.u8(0x3e); // 44.1 kHz, mono, 16-bit little-endian PCM.
    sound.u32(4410);
    sound.bytes(&vec![0; 8820]);
    film.tag(14, &sound.buf);
    film.tag(45, &[0x0e, 0x3e, 2, 0]);
    film.tag(19, &[1, 0, 1, 0]);
    film.tag(1, &[]);
    film.tag(19, &[2, 0, 2, 0]);
    film.tag(1, &[]);
    film.tag(45, &[0x0e, 0x3e, 2, 0]);
    film.tag(1, &[]);
    let mut start = SwfWriter::new();
    start.u16(4);
    start.u8(0x1f); // no multiple, envelope, loops, out and in
    start.u32(441);
    start.u32(2205);
    start.u16(3);
    start.u8(1);
    start.u32(0);
    start.u16(32768);
    start.u16(0);
    let mut by_class = SwfWriter::new();
    by_class.c_str("clips.Bell");
    by_class.bytes(&start.buf[2..]);
    film.tag(89, &by_class.buf);
    // The class binding appears after the trigger in this same SWF frame.
    let mut binding = SwfWriter::new();
    binding.u16(1);
    binding.u16(4);
    binding.c_str("clips.Bell");
    film.tag(76, &binding.buf);
    film.tag(19, &[3, 0, 3, 0]);
    film.tag(1, &[]);
    film.tag(19, &[4, 0, 4, 0]);
    film.tag(1, &[]);
    let bytes = film.into_swf(9, 100, 100, 5);
    let path = std::env::temp_dir().join(format!(
        "neoflash-stream-preview-{}.swf",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();
    let project = import_project(&path).unwrap();
    let report = inspect(&path).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "swf_sound_approximated")
    );
    let authored = project
        .symbol(&project.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .filter_map(|layer| {
            layer
                .keyframes
                .iter()
                .find(|key| key.frame == 3)?
                .sound
                .as_ref()
        })
        .find(|sound| sound.resource == "swf-sound-4")
        .unwrap();
    assert_eq!(
        (authored.loops, authored.in_point, authored.out_point),
        (3, Some(0.01), Some(0.05))
    );
    std::fs::remove_file(&path).unwrap();
    let mut player = SwfBytecodePlayer::from_bytes(&bytes).unwrap();
    player.step_frame();
    assert!(
        matches!(player.drain_sound_commands(&project).as_slice(), [SoundCommand::StreamStart { resource, offset: 0.0, .. }] if resource=="swf-stream-root-1")
    );
    player.step_frame();
    assert!(player.drain_sound_commands(&project).is_empty());
    player.step_frame();
    assert!(matches!(player.drain_sound_commands(&project).as_slice(),
        [SoundCommand::StreamStop { channel }] if channel == "swf-root-stream"));
    player.step_frame();
    let commands = player.drain_sound_commands(&project);
    assert!(
        matches!(&commands[..], [SoundCommand::Play {resource, sync: SoundSync::Start, loops: 3, envelope, in_point: Some(0.01), out_point: Some(0.05), .. }, SoundCommand::StreamStart {resource: stream,offset: 0.0,..}] if resource=="swf-sound-4" && envelope.iter().all(|point|point[1]==1.0 && point[2]==0.0) && !envelope.is_empty() && stream=="swf-stream-root-2"),
        "{commands:?}"
    );
    player.seek_to_frame(4);
    let commands = player.drain_sound_commands(&project);
    assert!(
        matches!(&commands[..],[SoundCommand::StreamStart {resource,offset,..}] if resource=="swf-stream-root-2" && (*offset - 1.0/24.0).abs()<1e-6),
        "{commands:?}"
    );
}

#[test]
fn t451_inline_text_alpha_multiplication_and_define_video_stream_viewport_fidelity() {
    let mut swf = SwfWriter::new();

    // Minimal DefineFont3 (tag 75, font_id = 1, 1 glyph 'A' = 65)
    let mut f3 = SwfWriter::new();
    f3.u16(1); // font_id = 1
    f3.u8(0x04); // FontFlagsWideCodes = 1
    f3.u8(1); // LanguageCode = Latin
    f3.u8(4); // FontNameLen
    f3.bytes(b"Sans");
    f3.u16(1); // NumGlyphs = 1
    f3.u16(4); // OffsetTable[0] = 4 (2 u16s)
    f3.u16(6); // CodeTableOffset = 6
    f3.u8(0x00);
    f3.u8(0x00);
    f3.u16(65); // 'A'
    swf.tag(75, &f3.buf);

    // DefineText2 (tag 33, char_id = 10): RGBA color [20, 100, 200, 128] (~0.502 alpha)
    let mut dt2 = SwfWriter::new();
    dt2.u16(10);
    dt2.rect(0, 1200, 0, 400);
    dt2.translate_matrix(0, 0);
    dt2.u8(1); // GlyphBits = 1
    dt2.u8(8); // AdvanceBits = 8
    dt2.u8(0x8C); // StyleFlagsReserved(1000) | HasFont(1) | HasColor(1)
    dt2.u16(1); // FontID = 1
    dt2.bytes(&[20, 100, 200, 128]); // TextColor RGBA with non-opaque alpha 128!
    dt2.u16(240); // TextHeight = 12px
    dt2.u8(1); // GlyphCount = 1
    dt2.write_ub(0, 1);
    dt2.write_sb(120, 8);
    dt2.align();
    dt2.u8(0);
    swf.tag(33, &dt2.buf);

    // DefineEditText (tag 37, char_id = 11): RGBA color [200, 50, 30, 64] (~0.251 alpha)
    let mut det = SwfWriter::new();
    det.u16(11);
    det.rect(0, 1600, 0, 400);
    det.u8(0x8D); // HasText(0x80) | ReadOnly(0x08) | HasTextColor(0x04) | HasFont(0x01)
    det.u8(0x00);
    det.u16(1);
    det.u16(240);
    det.bytes(&[200, 50, 30, 64]); // RGBA with alpha = 64
    det.c_str("");
    det.c_str("EditAlpha");
    swf.tag(37, &det.buf);

    // DefineVideoStream 20: empty new Video() placeholder (NumFrames=0, Width=0, Height=0, CodecID=0)
    let mut vs_empty = SwfWriter::new();
    vs_empty.u16(20);
    vs_empty.u16(0);
    vs_empty.u16(0);
    vs_empty.u16(0);
    vs_empty.u8(0);
    vs_empty.u8(0);
    swf.tag(60, &vs_empty.buf);

    // DefineVideoStream 21: embedded Sorenson stream (NumFrames=5, Width=320, Height=240, CodecID=2)
    let mut vs_stream = SwfWriter::new();
    vs_stream.u16(21);
    vs_stream.u16(5);
    vs_stream.u16(320);
    vs_stream.u16(240);
    vs_stream.u8(0);
    vs_stream.u8(2);
    swf.tag(60, &vs_stream.buf);

    let place_with_half_alpha = |depth: u16, cid: u16, tx_px: i32, ty_px: i32| -> Vec<u8> {
        let mut p = SwfWriter::new();
        p.u8(0x0E); // HasColorTransform(0x08) | HasMatrix(0x04) | HasCharacter(0x02)
        p.u16(depth);
        p.u16(cid);
        p.translate_matrix(tx_px * 20, ty_px * 20);
        p.write_ub(0, 1); // HasAddTerms = 0
        p.write_ub(1, 1); // HasMultTerms = 1
        p.write_ub(10, 4); // Nbits = 10
        p.write_sb(256, 10);
        p.write_sb(256, 10);
        p.write_sb(256, 10);
        p.write_sb(128, 10); // AlphaMultTerm = 128/256 = 0.5
        p.align();
        p.buf
    };

    swf.tag(26, &place_with_half_alpha(1, 10, 10, 10));
    swf.tag(26, &place_with_half_alpha(2, 11, 10, 40));

    let mut place_v20 = SwfWriter::new();
    place_v20.u8(0x06);
    place_v20.u16(3);
    place_v20.u16(20);
    place_v20.translate_matrix(20 * 20, 80 * 20);
    swf.tag(26, &place_v20.buf);

    let mut place_v21 = SwfWriter::new();
    place_v21.u8(0x06);
    place_v21.u16(4);
    place_v21.u16(21);
    place_v21.translate_matrix(40 * 20, 80 * 20);
    swf.tag(26, &place_v21.buf);

    swf.tag(61, &[21, 0, 0, 0, 0xAA]);
    swf.tag(61, &[21, 0, 1, 0, 0xBB]);
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 400, 360, 1);
    let tmp = std::env::temp_dir().join(format!("t451-text-video-{}.swf", std::process::id()));
    std::fs::write(&tmp, &swf_bytes).expect("write temp swf");
    let report = inspect(&tmp).expect("inspect text+video SWF");
    let proj = import_project(&tmp).expect("import text+video SWF");
    let _ = std::fs::remove_file(&tmp);

    proj.validate().expect("project validates");
    let root_sym = proj.symbol(&proj.root).unwrap();

    // Layer 0 (depth 1): DefineText2 (alpha 128/255) * PlaceObject2 (128/256 = 0.5) ≈ 0.251
    let el_dt2 = &root_sym.timeline.layers[0].keyframes[0].elements[0];
    let expected_dt2_alpha = (128.0 / 255.0) * 0.5;
    assert!(
        (el_dt2.transform.alpha - expected_dt2_alpha).abs() < 0.015,
        "DefineText2 placement alpha must multiply (got {}, expected ~{})",
        el_dt2.transform.alpha,
        expected_dt2_alpha
    );
    if let ElementKind::TextBox { format, .. } = &el_dt2.kind {
        assert_eq!(
            format.color, 0x1464C8,
            "local TextBox format.color preserves RGB [20, 100, 200] while alpha is multiplied into transform.alpha"
        );
    } else {
        panic!(
            "expected InlineText ElementKind::TextBox on depth 1, got {:?}",
            el_dt2.kind
        );
    }

    // Layer 1 (depth 2): DefineEditText (alpha 64/255) * PlaceObject2 (0.5) ≈ 0.1255
    let el_det = &root_sym.timeline.layers[1].keyframes[0].elements[0];
    let expected_det_alpha = (64.0 / 255.0) * 0.5;
    assert!(
        (el_det.transform.alpha - expected_det_alpha).abs() < 0.015,
        "DefineEditText placement alpha must multiply (got {}, expected ~{})",
        el_det.transform.alpha,
        expected_det_alpha
    );

    // Layer 2 (depth 3): DefineVideoStream 20 (0x0 -> 160x120)
    let el_v20 = &root_sym.timeline.layers[2].keyframes[0].elements[0];
    assert_eq!(
        el_v20.kind,
        ElementKind::Video {
            width: 160.0,
            height: 120.0
        }
    );

    // Layer 3 (depth 4): DefineVideoStream 21 (320x240)
    let el_v21 = &root_sym.timeline.layers[3].keyframes[0].elements[0];
    assert_eq!(
        el_v21.kind,
        ElementKind::Video {
            width: 320.0,
            height: 240.0
        }
    );

    let video_warnings: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|i| i.code == "swf_video_viewport")
        .collect();
    assert_eq!(
        video_warnings.len(),
        1,
        "expected exactly 1 swf_video_viewport warning (for vid 21 only), got: {video_warnings:?}"
    );
    assert!(
        video_warnings[0].location.contains("swf-sym-21"),
        "swf_video_viewport warning must cite swf-sym-21: {:?}",
        video_warnings[0]
    );
}
