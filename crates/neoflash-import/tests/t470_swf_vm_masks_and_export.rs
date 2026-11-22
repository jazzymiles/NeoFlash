//! `[T-470]` SWF VM Execution, `CSwfPage` Clip Actions, & Time-Varying `clip_depth` Masks Suite:
//! 1. `CSwfPage` compiled-clip `CLIPACTIONS` (`ClipEventConstruct` `0x0004_0000`, `ClipEventInitialize`
//!    `0x0000_0200`, `ClipEventKeyPress` `0x0002_0000` with keyCode byte, `ClipEventDragOver`/`DragOut`,
//!    and multi-flag `CLIPACTIONRECORD` entries) mapped cleanly without avoidable `swf_clip_event_approximated`
//!    warnings while keeping honest warnings on procedural `construct` scripts.
//! 2. Time-varying `clip_depth` mask spans (`PlaceObject2` `is_move`, mid-timeline starting/ending/changing
//!    `clip_depth`, nested `DefineSprite` masks, empty-target stencil hiding, deterministic `seek_to_frame`
//!    and `restart`, and static `import_project` layer splitting).
//! 3. `SwfBytecodePlayer` AVM1 `ActionTry` (`0x8F` catch-in-register & catch-in-variable, `return` + `finally`,
//!    `ActionThrow` `0x2A`, bounded `MAX_VM_STEPS_PER_FRAME` guard + `seek`/`restart`), `ActionInstanceOf`
//!    (`0x54`), `ActionImplementsOp` (`0x2C`), and `ActionCastOp` (`0x2B`).
//! 4. `SwfBytecodePlayer` AVM2 `EXCEPTION_INFO` unwinding (`throw` `0x03`, `newcatch` `0x5A`, typed catches +
//!    catch-all, bounded step cap + `seek`/`restart`), `Vector.<int|uint|Number|*>` (`applytype` `0x53`),
//!    `flash.utils.Dictionary`, and E4X (`XML`/`XMLList`, `@attr`, `0x06` `dxns`, `0x07` `dxnslate`,
//!    `0x59` `getdescendants`, `0x71` `esc_xelem`, `0x72` `esc_xattr`, `0x78` `checkfilter`).
//! 5. Upstream Ruffle Flash Player `test.swf` + `output.txt` 1:1 differential oracle suite (>= 30 strictly
//!    additional non-AIR reference movies).

use neoflash_core::{LayerRole, Player as CorePlayer, Project};
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
            "neoflash-t470-{label}-{}-{}.swf",
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
        self.tag(0, &[]);
        let mut hdr = SwfWriter::new();
        hdr.rect(0, width_px * 20, 0, height_px * 20);
        hdr.u16(24 << 8);
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

fn make_solid_rect_shape3_payload(char_id: u16, w_px: i32, h_px: i32, rgba: [u8; 4]) -> Vec<u8> {
    let w = w_px * 20;
    let h = h_px * 20;
    let mut sw = SwfWriter::new();
    sw.u16(char_id);
    sw.rect(0, w, 0, h);
    sw.u8(1);
    sw.u8(0x00);
    sw.bytes(&rgba);
    sw.u8(0);
    sw.write_ub(1, 4);
    sw.write_ub(0, 4);
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

fn avm1_push_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let len = (1 + b.len() + 1) as u16;
    out.push(0x96);
    out.extend_from_slice(&len.to_le_bytes());
    out.push(0x00);
    out.extend_from_slice(b);
    out.push(0x00);
}

fn avm1_push_i32(out: &mut Vec<u8>, v: i32) {
    out.push(0x96);
    out.extend_from_slice(&5u16.to_le_bytes());
    out.push(0x07);
    out.extend_from_slice(&v.to_le_bytes());
}

fn avm1_push_bool(out: &mut Vec<u8>, v: bool) {
    out.push(0x96);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.push(0x05);
    out.push(u8::from(v));
}

fn avm1_push_reg(out: &mut Vec<u8>, r: u8) {
    out.push(0x96);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.push(0x04);
    out.push(r);
}

#[test]
fn t470_cswfpage_clip_actions_pure_component_params_vs_procedural_warnings() {
    // Build an empty DefineSprite (id = 1) to attach SWF7 CSwfPage-style CLIPACTIONS onto.
    let mut sprite_payload = SwfWriter::new();
    sprite_payload.u16(1);
    sprite_payload.u16(1);
    sprite_payload.tag(1, &[]); // ShowFrame
    sprite_payload.tag(0, &[]); // End

    // Pure component parameter assignments on Initialize (0x0200) and Construct (0x0004_0000):
    let mut init_body = Vec::new();
    avm1_push_str(&mut init_body, "label");
    avm1_push_str(&mut init_body, "Submit");
    init_body.push(0x1D); // SetVariable
    avm1_push_str(&mut init_body, "toggle");
    avm1_push_bool(&mut init_body, true);
    init_body.push(0x1D);
    init_body.push(0x00);

    let mut construct_body = Vec::new();
    avm1_push_str(&mut construct_body, "width");
    avm1_push_i32(&mut construct_body, 140);
    construct_body.push(0x1D);
    avm1_push_str(&mut construct_body, "a");
    avm1_push_str(&mut construct_body, "b");
    avm1_push_i32(&mut construct_body, 2);
    construct_body.push(0x42); // InitArray -> ["b", "a"]
    avm1_push_str(&mut construct_body, "items");
    construct_body.push(0x4D); // StackSwap
    construct_body.push(0x1D); // SetVariable
    construct_body.push(0x00);

    // Interactive KeyPress ('k' = 107), DragOver | DragOut multi-flag, and Press | Release:
    let mut kp_body = Vec::new();
    avm1_push_str(&mut kp_body, "kp:k");
    kp_body.push(0x26); // Trace
    kp_body.push(0x00);

    let mut drag_body = Vec::new();
    avm1_push_str(&mut drag_body, "drag-io");
    drag_body.push(0x26);
    drag_body.push(0x00);

    let mut click_body = Vec::new();
    avm1_push_str(&mut click_body, "label");
    click_body.push(0x1C); // GetVariable
    click_body.push(0x26); // Trace
    click_body.push(0x00);

    let all_flags: u32 = 0x0000_0200 | 0x0004_0000 | 0x0002_0000 | 0x0001_8000 | 0x0000_0C00;
    let mut place_pure = SwfWriter::new();
    place_pure.u8(0x80 | 0x20 | 0x04 | 0x02); // HasClipActions | HasName | HasMatrix | HasCharacter
    place_pure.u16(1); // depth 1
    place_pure.u16(1); // char 1
    place_pure.translate_matrix(0, 0);
    place_pure.c_str("compBtn");
    place_pure.u16(0); // reserved
    place_pure.u32(all_flags);
    // Record 1: ClipEventInitialize (0x0200)
    place_pure.u32(0x0000_0200);
    place_pure.u32(init_body.len() as u32);
    place_pure.bytes(&init_body);
    // Record 2: ClipEventConstruct (0x0004_0000)
    place_pure.u32(0x0004_0000);
    place_pure.u32(construct_body.len() as u32);
    place_pure.bytes(&construct_body);
    // Record 3: ClipEventKeyPress (0x0002_0000) -> 1-byte keyCode + actions!
    place_pure.u32(0x0002_0000);
    place_pure.u32((1 + kp_body.len()) as u32);
    place_pure.u8(b'k');
    place_pure.bytes(&kp_body);
    // Record 4: Multi-flag DragOver | DragOut (0x8000 | 0x0001_0000)
    place_pure.u32(0x0001_8000);
    place_pure.u32(drag_body.len() as u32);
    place_pure.bytes(&drag_body);
    // Record 5: Multi-flag Press | Release (0x0400 | 0x0800)
    place_pure.u32(0x0000_0C00);
    place_pure.u32(click_body.len() as u32);
    place_pure.bytes(&click_body);
    place_pure.u32(0); // end CLIPACTIONRECORDs

    let mut swf_pure = SwfWriter::new();
    swf_pure.tag(39, &sprite_payload.buf);
    swf_pure.tag(26, &place_pure.buf);
    swf_pure.tag(1, &[]);
    let pure_bytes = swf_pure.into_swf(7, 200, 120, 1);
    let pure_tmp = TempSwfFile::write("cswfpage-pure", &pure_bytes);

    let pure_report = inspect(pure_tmp.path()).expect("inspect pure CSwfPage clip");
    let approx_count = pure_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_clip_event_approximated")
        .count();
    assert_eq!(
        approx_count, 0,
        "pure CSwfPage component parameter clip actions must emit 0 swf_clip_event_approximated warnings: {:?}",
        pure_report.diagnostics
    );

    let pure_project = import_project(pure_tmp.path()).expect("import pure CSwfPage clip");
    pure_project
        .validate()
        .expect("validate pure CSwfPage project");
    let root = pure_project.symbol(&pure_project.root).unwrap();
    let scripts = &root.timeline.layers[0].keyframes[0].elements[0].scripts;
    assert_eq!(
        scripts.len(),
        5,
        "initialize, construct, keyPress, dragOver+dragOut, and press+release scripts must be preserved: {scripts:?}"
    );
    assert!(scripts[0].script.source.contains("label = \"Submit\""));
    assert!(
        scripts[1]
            .script
            .source
            .contains("// onClipEvent(construct)")
    );
    assert!(
        scripts
            .iter()
            .any(|s| s.script.source.contains("on(keyPress \"k\")"))
    );
    assert!(
        scripts
            .iter()
            .any(|s| s.script.source.contains("on(dragOver, dragOut)"))
    );
    assert!(
        scripts
            .iter()
            .any(|s| s.script.source.contains("on(press, release)"))
    );

    let mut player = SwfBytecodePlayer::from_bytes(&pure_bytes).expect("player");
    player.step_frame();
    player.trigger_key_press(b'k');
    player.trigger_pointer_drag_over();
    player.trigger_pointer_click();
    assert_eq!(
        player.trace_output(),
        &["kp:k", "drag-io", "Submit", "Submit"]
    );

    // Procedural Construct + Load MUST still emit an honest `swf_clip_event_approximated` warning!
    let mut proc_construct = Vec::new();
    avm1_push_str(&mut proc_construct, "side-effect");
    proc_construct.push(0x26); // Trace (procedural!)
    proc_construct.push(0x00);

    let mut place_proc = SwfWriter::new();
    place_proc.u8(0x80 | 0x04 | 0x02);
    place_proc.u16(1);
    place_proc.u16(1);
    place_proc.translate_matrix(0, 0);
    place_proc.u16(0);
    place_proc.u32(0x0004_0000);
    place_proc.u32(0x0004_0000);
    place_proc.u32(proc_construct.len() as u32);
    place_proc.bytes(&proc_construct);
    place_proc.u32(0);

    let mut swf_proc = SwfWriter::new();
    swf_proc.tag(39, &sprite_payload.buf);
    swf_proc.tag(26, &place_proc.buf);
    swf_proc.tag(1, &[]);
    let proc_bytes = swf_proc.into_swf(7, 200, 120, 1);
    let proc_tmp = TempSwfFile::write("cswfpage-proc", &proc_bytes);
    let proc_report = inspect(proc_tmp.path()).expect("inspect procedural construct clip");
    let proc_approx = proc_report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_clip_event_approximated")
        .count();
    assert_eq!(
        proc_approx, 1,
        "procedural ClipEventConstruct must retain honest swf_clip_event_approximated warning"
    );
}

fn place_obj2_payload(
    is_move: bool,
    depth: u16,
    char_id: Option<u16>,
    pos_px: Option<(i32, i32)>,
    clip_depth: Option<u16>,
) -> Vec<u8> {
    let mut flags = 0u8;
    if is_move {
        flags |= 0x01;
    }
    if char_id.is_some() {
        flags |= 0x02;
    }
    if pos_px.is_some() {
        flags |= 0x04;
    }
    if clip_depth.is_some() {
        flags |= 0x40;
    }
    let mut w = SwfWriter::new();
    w.u8(flags);
    w.u16(depth);
    if let Some(cid) = char_id {
        w.u16(cid);
    }
    if let Some((x, y)) = pos_px {
        w.translate_matrix(x * 20, y * 20);
    }
    if let Some(cd) = clip_depth {
        w.u16(cd);
    }
    w.buf
}

fn render_swf_overlay_rgba(
    player: &SwfBytecodePlayer,
    base_project: &Project,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let f = player.presented_frame();
    let overlay = player.project_with_display_list_overlay(base_project, &base_project.root, f);
    let mut cp = CorePlayer::new(&overlay);
    cp.seek(&overlay, f);
    cp.render_snapshot_rgba(&overlay, width, height)
        .expect("render overlay")
}

fn px_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let off = ((y * width + x) * 4) as usize;
    [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
}

#[test]
fn t470_time_varying_clip_depth_masks_dynamic_player_static_import_and_seek() {
    // Shapes:
    // 1 = Left-half stencil (50x40, white)
    // 2 = Full-width blue bar (100x40, [20, 100, 240, 255])
    // 3 = Right-half orange bar (50x40 placed at x=50, [250, 130, 20, 255])
    // 4 = Nested stencil (20x20, white)
    // 5 = Nested green bar (40x20, [10, 210, 80, 255])
    let mut swf = SwfWriter::new();
    swf.tag(
        32,
        &make_solid_rect_shape3_payload(1, 50, 40, [255, 255, 255, 255]),
    );
    swf.tag(
        32,
        &make_solid_rect_shape3_payload(2, 100, 40, [20, 100, 240, 255]),
    );
    swf.tag(
        32,
        &make_solid_rect_shape3_payload(3, 50, 40, [250, 130, 20, 255]),
    );
    swf.tag(
        32,
        &make_solid_rect_shape3_payload(4, 20, 20, [255, 255, 255, 255]),
    );
    swf.tag(
        32,
        &make_solid_rect_shape3_payload(5, 40, 20, [10, 210, 80, 255]),
    );

    // DefineSprite 6 (2-frame nested timeline where depth 1 mask turns ON on frame 1 only):
    let mut child = SwfWriter::new();
    child.u16(6);
    child.u16(2);
    // Child Frame 0: depth 2 = green bar (unmasked!)
    child.tag(
        26,
        &place_obj2_payload(false, 2, Some(5), Some((0, 0)), None),
    );
    child.tag(1, &[]);
    // Child Frame 1: depth 1 = stencil with clip_depth = 2!
    child.tag(
        26,
        &place_obj2_payload(false, 1, Some(4), Some((0, 0)), Some(2)),
    );
    child.tag(1, &[]);
    child.tag(0, &[]);
    swf.tag(39, &child.buf);

    // Root Frame 0: depth 2 = blue bar (unmasked), depth 10 = nested sprite 6 at (0, 50)
    swf.tag(
        26,
        &place_obj2_payload(false, 2, Some(2), Some((0, 0)), None),
    );
    swf.tag(
        26,
        &place_obj2_payload(false, 10, Some(6), Some((0, 50)), None),
    );
    swf.tag(1, &[]);

    // Root Frame 1: newly place depth 1 stencil with clip_depth = 2 (masks blue bar to left half!)
    swf.tag(
        26,
        &place_obj2_payload(false, 1, Some(1), Some((0, 0)), Some(2)),
    );
    swf.tag(1, &[]);

    // Root Frame 2: place depth 3 orange bar at (50, 0) AND expand depth 1 clip_depth = 3 via is_move!
    swf.tag(
        26,
        &place_obj2_payload(false, 3, Some(3), Some((50, 0)), None),
    );
    swf.tag(26, &place_obj2_payload(true, 1, None, None, Some(3)));
    swf.tag(1, &[]);

    // Root Frame 3: clear depth 1 mask via RemoveObject2(1) -> blue & orange bars both unmasked!
    swf.tag(28, &1u16.to_le_bytes());
    swf.tag(1, &[]);

    // Root Frame 4: re-place depth 1 with clip_depth = 2 (masks ONLY depth 2 blue bar; depth 3 orange stays unmasked!)
    swf.tag(
        26,
        &place_obj2_payload(false, 1, Some(1), Some((0, 0)), Some(2)),
    );
    swf.tag(1, &[]);

    let swf_bytes = swf.into_swf(8, 120, 80, 5);
    let tmp = TempSwfFile::write("time-varying-masks", &swf_bytes);

    let report = inspect(tmp.path()).expect("inspect time-varying masks SWF");
    let mask_warns = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_mask_approximated")
        .count();
    assert_eq!(
        mask_warns, 0,
        "cleanly representable time-varying vector clip_depth spans must emit 0 swf_mask_approximated warnings: {:?}",
        report.diagnostics
    );

    let project = import_project(tmp.path()).expect("import time-varying masks SWF");
    project
        .validate()
        .expect("imported split mask/masked/normal layers must pass Project::validate()");
    let root_sym = project.symbol(&project.root).unwrap();
    assert!(
        root_sym
            .timeline
            .layers
            .iter()
            .any(|l| l.properties.role == LayerRole::Mask),
        "expected reconstructed LayerRole::Mask layer"
    );

    let mut swf_player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer");
    let mut static_player = CorePlayer::new(&project);
    let mut forward_frames = Vec::with_capacity(5);

    for f in 0..5u32 {
        if f > 0 {
            static_player.tick(&project);
        }
        swf_player.step_frame();
        let swf_rgba = render_swf_overlay_rgba(&swf_player, &project, 120, 80);
        let static_rgba = static_player
            .render_snapshot_rgba(&project, 120, 80)
            .expect("static CorePlayer render");

        let left_top = px_at(&swf_rgba, 120, 25, 20);
        let right_top = px_at(&swf_rgba, 120, 75, 20);
        match f {
            0 => {
                assert_eq!(left_top, [20, 100, 240, 255], "f0 left unmasked blue");
                assert_eq!(right_top, [20, 100, 240, 255], "f0 right unmasked blue");
            }
            1 => {
                assert_eq!(left_top, [20, 100, 240, 255], "f1 left inside mask");
                assert_eq!(right_top, [0, 0, 0, 0], "f1 right clipped by clip_depth=2");
            }
            2 => {
                assert_eq!(left_top, [20, 100, 240, 255], "f2 left inside mask");
                assert_eq!(
                    right_top,
                    [0, 0, 0, 0],
                    "f2 orange bar at depth 3 clipped after is_move expanded clip_depth=3"
                );
            }
            3 => {
                assert_eq!(left_top, [20, 100, 240, 255], "f3 left unmasked");
                assert_eq!(right_top, [250, 130, 20, 255], "f3 right orange unmasked");
            }
            4 => {
                assert_eq!(left_top, [20, 100, 240, 255], "f4 left blue masked");
                assert_eq!(
                    right_top,
                    [250, 130, 20, 255],
                    "f4 right orange at depth 3 ABOVE clip_depth=2 stays visible!"
                );
            }
            _ => unreachable!(),
        }

        // Nested DefineSprite 6 loops 0, 1, 0, 1, 0:
        let child_left = px_at(&swf_rgba, 120, 10, 60);
        let child_right = px_at(&swf_rgba, 120, 30, 60);
        assert_eq!(child_left, [10, 210, 80, 255]);
        if f % 2 == 0 {
            assert_eq!(child_right, [10, 210, 80, 255], "nested f{f} unmasked");
        } else {
            assert_eq!(child_right, [0, 0, 0, 0], "nested f{f} masked");
        }

        assert_eq!(
            swf_rgba, static_rgba,
            "SwfBytecodePlayer vs static imported Project RGBA mismatch on frame {f}"
        );
        forward_frames.push(swf_rgba);
    }

    // Deterministic backward & non-sequential seek_to_frame + restart!
    for target in [4u32, 1, 3, 0, 2] {
        swf_player.seek_to_frame(target);
        let seek_rgba = render_swf_overlay_rgba(&swf_player, &project, 120, 80);
        assert_eq!(
            seek_rgba, forward_frames[target as usize],
            "seek_to_frame({target}) mismatch"
        );
    }
    swf_player.restart();
    assert_eq!(
        render_swf_overlay_rgba(&swf_player, &project, 120, 80),
        forward_frames[0],
        "restart() mismatch"
    );
}

fn build_avm1_action_try(
    catch_in_reg: Option<u8>,
    catch_var: Option<&str>,
    try_body: &[u8],
    catch_body: Option<&[u8]>,
    finally_body: Option<&[u8]>,
) -> Vec<u8> {
    let mut flags = 0u8;
    if catch_body.is_some() {
        flags |= 0x01;
    }
    if finally_body.is_some() {
        flags |= 0x02;
    }
    if catch_in_reg.is_some() {
        flags |= 0x04;
    }
    let mut hdr = Vec::new();
    hdr.push(flags);
    hdr.extend_from_slice(&(try_body.len() as u16).to_le_bytes());
    hdr.extend_from_slice(&(catch_body.map_or(0, <[u8]>::len) as u16).to_le_bytes());
    hdr.extend_from_slice(&(finally_body.map_or(0, <[u8]>::len) as u16).to_le_bytes());
    if let Some(reg) = catch_in_reg {
        hdr.push(reg);
    } else {
        hdr.extend_from_slice(catch_var.unwrap_or("").as_bytes());
        hdr.push(0);
    }
    let mut out = Vec::new();
    out.push(0x8F);
    out.extend_from_slice(&(hdr.len() as u16).to_le_bytes());
    out.extend_from_slice(&hdr);
    out.extend_from_slice(try_body);
    if let Some(cb) = catch_body {
        out.extend_from_slice(cb);
    }
    if let Some(fb) = finally_body {
        out.extend_from_slice(fb);
    }
    out
}

#[test]
fn t470_avm1_action_try_catch_register_finally_return_step_cap_and_oop_opcodes() {
    // Frame 0:
    // 1) DefineFunction2 `fnRegCatch()` catching in register 2 and returning from catch while running finally!
    let mut try_b = Vec::new();
    avm1_push_str(&mut try_b, "reg-boom");
    try_b.push(0x2A); // ActionThrow

    let mut catch_b = Vec::new();
    avm1_push_str(&mut catch_b, "caught-r2:");
    avm1_push_reg(&mut catch_b, 2);
    catch_b.push(0x47); // ActionAdd2
    catch_b.push(0x3E); // ActionReturn (must run finally_b first!)

    let mut fin_b = Vec::new();
    avm1_push_str(&mut fin_b, "fin-after-reg-return");
    fin_b.push(0x26); // ActionTrace

    let fn_body = build_avm1_action_try(Some(2), None, &try_b, Some(&catch_b), Some(&fin_b));

    let mut frame0_actions = Vec::new();
    // DefineFunction2 "fnRegCatch" (0 params, reg_count = 4, flags = 0)
    let mut df2_hdr = Vec::new();
    df2_hdr.extend_from_slice(b"fnRegCatch\0");
    df2_hdr.extend_from_slice(&0u16.to_le_bytes()); // numParams = 0
    df2_hdr.push(4); // registerCount = 4
    df2_hdr.extend_from_slice(&0u16.to_le_bytes()); // flags = 0
    df2_hdr.extend_from_slice(&(fn_body.len() as u16).to_le_bytes());
    frame0_actions.push(0x8E);
    frame0_actions.extend_from_slice(&(df2_hdr.len() as u16).to_le_bytes());
    frame0_actions.extend_from_slice(&df2_hdr);
    frame0_actions.extend_from_slice(&fn_body);

    // Call fnRegCatch() and trace return value:
    avm1_push_i32(&mut frame0_actions, 0);
    avm1_push_str(&mut frame0_actions, "fnRegCatch");
    frame0_actions.push(0x3D); // ActionCallFunction
    frame0_actions.push(0x26); // ActionTrace

    // 2) ActionImplementsOp (0x2C), ActionInstanceOf (0x54), and ActionCastOp (0x2B):
    // DefineFunction "IFoo" () {}, DefineFunction "IBar" () {} (`IBar implements IFoo`),
    // DefineFunction "Widget" () {} (`Widget implements IBar`)
    for cname in ["IFoo", "IBar", "Widget", "Other"] {
        let mut df = Vec::new();
        df.extend_from_slice(cname.as_bytes());
        df.push(0);
        df.extend_from_slice(&0u16.to_le_bytes());
        df.extend_from_slice(&0u16.to_le_bytes());
        frame0_actions.push(0x9B);
        frame0_actions.extend_from_slice(&(df.len() as u16).to_le_bytes());
        frame0_actions.extend_from_slice(&df);
    }
    // IBar implements IFoo (stack bottom->top: [IFoo, 1, IBar] -> ActionImplementsOp 0x2C)
    avm1_push_str(&mut frame0_actions, "IFoo");
    frame0_actions.push(0x1C); // GetVariable
    avm1_push_i32(&mut frame0_actions, 1);
    avm1_push_str(&mut frame0_actions, "IBar");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x2C); // ActionImplementsOp

    // Widget implements IBar
    avm1_push_str(&mut frame0_actions, "IBar");
    frame0_actions.push(0x1C);
    avm1_push_i32(&mut frame0_actions, 1);
    avm1_push_str(&mut frame0_actions, "Widget");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x2C); // ActionImplementsOp

    // var w = new Widget();
    avm1_push_str(&mut frame0_actions, "w");
    avm1_push_i32(&mut frame0_actions, 0);
    avm1_push_str(&mut frame0_actions, "Widget");
    frame0_actions.push(0x40); // ActionNewObject
    frame0_actions.push(0x1D); // SetVariable

    // Trace (w instanceof IFoo) -> true (transitive interface!)
    avm1_push_str(&mut frame0_actions, "w");
    frame0_actions.push(0x1C);
    avm1_push_str(&mut frame0_actions, "IFoo");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x54); // ActionInstanceOf
    frame0_actions.push(0x26); // Trace

    // ActionCastOp (0x2B): stack [Constructor, Instance] -> returns Instance if match else null!
    // Cast(IFoo, w) !== null -> true; Cast(Other, w) === null -> true
    avm1_push_str(&mut frame0_actions, "IFoo");
    frame0_actions.push(0x1C);
    avm1_push_str(&mut frame0_actions, "w");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x2B); // ActionCastOp -> w
    avm1_push_str(&mut frame0_actions, "w");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x66); // ActionStrictEquals
    frame0_actions.push(0x26); // Trace

    avm1_push_str(&mut frame0_actions, "Other");
    frame0_actions.push(0x1C);
    avm1_push_str(&mut frame0_actions, "w");
    frame0_actions.push(0x1C);
    frame0_actions.push(0x2B); // ActionCastOp -> null
    frame0_actions.push(0x96);
    frame0_actions.extend_from_slice(&1u16.to_le_bytes());
    frame0_actions.push(0x02); // Null
    frame0_actions.push(0x66); // ActionStrictEquals
    frame0_actions.push(0x26); // Trace
    frame0_actions.push(0x00);

    // Frame 1: Infinite loop inside ActionTry (`Jump -5`) to verify MAX_VM_STEPS_PER_FRAME cap!
    let inf_loop: [u8; 5] = [0x99, 0x02, 0x00, 0xFB, 0xFF]; // Jump -5 (back to itself)
    let mut frame1_actions =
        build_avm1_action_try(None, Some("e"), &inf_loop, None, Some(&inf_loop));
    frame1_actions.push(0x00);

    let mut swf = SwfWriter::new();
    swf.tag(12, &frame0_actions);
    swf.tag(1, &[]);
    swf.tag(12, &frame1_actions);
    swf.tag(1, &[]);
    let swf_bytes = swf.into_swf(8, 100, 100, 2);

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("AVM1 player");
    let f0 = player.step_frame();
    assert!(f0.unsupported_diagnostics.is_empty());
    assert_eq!(
        f0.traces,
        vec![
            "fin-after-reg-return",
            "caught-r2:reg-boom",
            "true",
            "true",
            "true"
        ]
    );

    // Step frame 1 (bounded infinite loop inside ActionTry + finally must terminate deterministically!)
    let _f1 = player.step_frame();

    // Verify seek_to_frame(0) and restart() cleanly reset all VM state!
    let seek0 = player.seek_to_frame(0);
    assert_eq!(
        seek0.traces,
        vec![
            "fin-after-reg-return",
            "caught-r2:reg-boom",
            "true",
            "true",
            "true"
        ]
    );
    let restarted = player.restart();
    assert_eq!(
        restarted.traces,
        vec![
            "fin-after-reg-return",
            "caught-r2:reg-boom",
            "true",
            "true",
            "true"
        ]
    );
}

fn push_u30(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut b = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            b |= 0x80;
        }
        out.push(b);
        if val == 0 {
            break;
        }
    }
}

fn push_s24(out: &mut Vec<u8>, val: i32) {
    let bytes = val.to_le_bytes();
    out.push(bytes[0]);
    out.push(bytes[1]);
    out.push(bytes[2]);
}

#[test]
fn t470_avm2_exception_unwinding_vector_dictionary_and_e4x_opcodes_and_step_cap() {
    // Construct a compact SWF 10 DoABC2 testing:
    // - 0x53 applytype (Vector.<int>), fixed RangeError #1126 caught via EXCEPTION_INFO + 0x5A newcatch
    // - flash.utils.Dictionary with distinct object identity keys + 0xB4 `in` + 0x32 `hasnext2`
    // - E4X: XML("<a><b id='7'>hi</b><b id='9'>ok</b></a>"), 0x06 dxns, 0x07 dxnslate,
    //   0x59 getdescendants (`..b`), `@id` QNameA (0x0D), 0x71 esc_xelem, 0x72 esc_xattr,
    //   and 0x78 checkfilter (passing on XML and throwing TypeError #1123 on int, caught cleanly!)
    let strings = [
        "",                                        // 1
        "trace",                                   // 2
        "__AS3__.vec",                             // 3
        "Vector",                                  // 4
        "int",                                     // 5
        "fixed",                                   // 6
        "push",                                    // 7
        "RangeError",                              // 8
        "TypeError",                               // 9
        "err",                                     // 10
        "flash.utils",                             // 11
        "Dictionary",                              // 12
        "XML",                                     // 13
        "<a><b id='7'>hi</b><b id='9'>ok</b></a>", // 14
        "b",                                       // 15
        "id",                                      // 16
        "http://example.com/ns",                   // 17
        "a<b&c\"d",                                // 18
        "vec-caught",                              // 19
        "filter-caught",                           // 20
    ];

    let mut abc = Vec::new();
    abc.extend_from_slice(&16u16.to_le_bytes());
    abc.extend_from_slice(&46u16.to_le_bytes());
    push_u30(&mut abc, 0); // ints
    push_u30(&mut abc, 0); // uints
    push_u30(&mut abc, 0); // doubles
    push_u30(&mut abc, (strings.len() + 1) as u32);
    for s in strings {
        push_u30(&mut abc, s.len() as u32);
        abc.extend_from_slice(s.as_bytes());
    }
    // Namespaces:
    // [1] PackageNamespace("") -> str 1
    // [2] PackageNamespace("__AS3__.vec") -> str 3
    // [3] PackageNamespace("flash.utils") -> str 11
    push_u30(&mut abc, 4);
    abc.extend_from_slice(&[0x16, 1, 0x16, 3, 0x16, 11]);
    // NsSets: [1] = {1}
    push_u30(&mut abc, 2);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);

    // Multinames:
    // [1] QName(1, "trace"=2)
    // [2] QName(2, "Vector"=4)
    // [3] QName(1, "int"=5)
    // [4] QName(1, "fixed"=6)
    // [5] QName(1, "push"=7)
    // [6] QName(1, "RangeError"=8)
    // [7] QName(1, "TypeError"=9)
    // [8] QName(1, "err"=10)
    // [9] QName(3, "Dictionary"=12)
    // [10] MultinameL(ns_set=1) (0x1B)
    // [11] QName(1, "XML"=13)
    // [12] QName(1, "b"=15)
    // [13] QNameA(1, "id"=16) (0x0D — attribute multiname `@id`!)
    push_u30(&mut abc, 14);
    for (kind, a, b) in [
        (0x07u8, 1u32, 2u32),
        (0x07, 2, 4),
        (0x07, 1, 5),
        (0x07, 1, 6),
        (0x07, 1, 7),
        (0x07, 1, 8),
        (0x07, 1, 9),
        (0x07, 1, 10),
        (0x07, 3, 12),
    ] {
        abc.push(kind);
        push_u30(&mut abc, a);
        push_u30(&mut abc, b);
    }
    // [10] MultinameL(ns_set=1)
    abc.push(0x1B);
    push_u30(&mut abc, 1);
    // [11] QName(1, "XML"=13)
    abc.extend_from_slice(&[0x07, 1, 13]);
    // [12] QName(1, "b"=15)
    abc.extend_from_slice(&[0x07, 1, 15]);
    // [13] QNameA(1, "id"=16)
    abc.extend_from_slice(&[0x0D, 1, 16]);

    // 1 method (script init, 0 params)
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);
    abc.push(0);
    push_u30(&mut abc, 0); // metadata
    push_u30(&mut abc, 0); // classes
    push_u30(&mut abc, 1); // 1 script
    push_u30(&mut abc, 0); // sinit = 0
    push_u30(&mut abc, 0); // 0 traits

    let mut code = Vec::new();
    code.extend_from_slice(&[0xD0, 0x30]); // getlocal_0, pushscope

    // 1. Vector.<int> via 0x53 applytype:
    // getlex Vector(2), getlex int(3), applytype(1), pushbyte 2, pushtrue, construct(2) -> local 1
    code.extend_from_slice(&[0x60, 2, 0x60, 3, 0x53, 1, 0x24, 2, 0x26, 0x42, 2, 0xD5]);
    // Try pushing onto fixed Vector -> must throw RangeError #1126 caught by catch 0!
    let try1_from = code.len() as u32;
    code.extend_from_slice(&[0xD1, 0x24, 99, 0x4F, 5, 1]); // getlocal_1, pushbyte 99, callpropvoid push(1)
    let jmp1_pos = code.len();
    code.push(0x10);
    push_s24(&mut code, 0);
    let try1_to = code.len() as u32;
    let catch1_target = code.len() as u32;
    // Catch 0 (RangeError=6, var err=8): getlocal_0, pushscope, newcatch 0, dup, pushscope, swap, setslot 1, popscope
    code.extend_from_slice(&[0xD0, 0x30, 0x5A, 0, 0x2A, 0x30, 0x2B, 0x6D, 1, 0x1D]);
    code.extend_from_slice(&[0x5D, 1, 0x2C, 19, 0x4F, 1, 1]); // trace("vec-caught")
    let after1 = code.len();
    let rel1 = (after1 as i32) - ((jmp1_pos + 4) as i32);
    code[jmp1_pos + 1..jmp1_pos + 4].copy_from_slice(&rel1.to_le_bytes()[..3]);

    // 2. Dictionary with two distinct `{}` object keys (local 2 and local 3):
    // findpropstrict Dictionary(9), constructprop Dictionary(9, 0) -> local 4 (`63 04`)
    code.extend_from_slice(&[0x5D, 9, 0x4A, 9, 0, 0x63, 4]);
    code.extend_from_slice(&[0x55, 0, 0xD6]); // newobject 0 -> local 2
    code.extend_from_slice(&[0x55, 0, 0xD7]); // newobject 0 -> local 3
    // dict[k1] = 11; dict[k2] = 22;
    code.extend_from_slice(&[0x62, 4, 0xD2, 0x24, 11, 0x61, 10]); // setproperty MultinameL(10)
    code.extend_from_slice(&[0x62, 4, 0xD3, 0x24, 22, 0x61, 10]);
    // trace((k1 in dict) + ":" + dict[k1] + ":" + dict[k2]) -> "true:11:22"
    code.extend_from_slice(&[
        0x5D, 1, // findpropstrict trace
        0xD2, 0x62, 4, 0xB4, // k1 in dict
        0x2C, 1,    // ""
        0xA0, // +
        0x62, 4, 0xD2, 0x66, 10,   // dict[k1]
        0xA0, // +
        0x62, 4, 0xD3, 0x66, 10,   // dict[k2]
        0xA0, // +
        0x4F, 1, 1, // callpropvoid trace(1)
    ]);

    // 3. E4X: dxns("http://example.com/ns"), dxnslate, XML("<a>...</a>"), checkfilter(0x78),
    //    getdescendants(0x59, b=12), QNameA(`@id`=13), esc_xelem(0x71), esc_xattr(0x72):
    code.extend_from_slice(&[0x06, 17]); // dxns "http://example.com/ns"
    code.extend_from_slice(&[0x2C, 1, 0x07]); // pushstring "", dxnslate
    // findpropstrict XML(11), pushstring 14, constructprop XML(11, 1), checkfilter(0x78) -> local 5
    code.extend_from_slice(&[0x5D, 11, 0x2C, 14, 0x4A, 11, 1, 0x78, 0x63, 5]);
    // trace(xml..b.@id) -> getlocal 5, getdescendants b(12), getproperty @id(13) -> "79"
    code.extend_from_slice(&[0x5D, 1, 0x62, 5, 0x59, 12, 0x66, 13, 0x4F, 1, 1]);
    // trace(esc_xelem("a<b&c\"d") + "|" + esc_xattr("a<b&c\"d"))
    code.extend_from_slice(&[0x5D, 1, 0x2C, 18, 0x71, 0x2C, 18, 0x72, 0xA0, 0x4F, 1, 1]);

    // checkfilter(0x78) on integer 42 must throw TypeError #1123 caught by catch 1!
    let try2_from = code.len() as u32;
    code.extend_from_slice(&[0x24, 42, 0x78, 0x29]); // pushbyte 42, checkfilter, pop
    let jmp2_pos = code.len();
    code.push(0x10);
    push_s24(&mut code, 0);
    let try2_to = code.len() as u32;
    let catch2_target = code.len() as u32;
    code.extend_from_slice(&[0xD0, 0x30, 0x5A, 1, 0x2A, 0x30, 0x2B, 0x6D, 1, 0x1D]);
    code.extend_from_slice(&[0x5D, 1, 0x2C, 20, 0x4F, 1, 1]); // trace("filter-caught")
    let after2 = code.len();
    let rel2 = (after2 as i32) - ((jmp2_pos + 4) as i32);
    code[jmp2_pos + 1..jmp2_pos + 4].copy_from_slice(&rel2.to_le_bytes()[..3]);

    code.push(0x47); // returnvoid

    // Method body 0:
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0); // method 0
    push_u30(&mut abc, 8); // max_stack
    push_u30(&mut abc, 8); // local_count
    push_u30(&mut abc, 1); // init_scope_depth
    push_u30(&mut abc, 4); // max_scope_depth
    push_u30(&mut abc, code.len() as u32);
    abc.extend_from_slice(&code);
    push_u30(&mut abc, 2); // 2 exceptions
    // Exception 0: RangeError(6), var err(8)
    push_u30(&mut abc, try1_from);
    push_u30(&mut abc, try1_to);
    push_u30(&mut abc, catch1_target);
    push_u30(&mut abc, 6);
    push_u30(&mut abc, 8);
    // Exception 1: TypeError(7), var err(8)
    push_u30(&mut abc, try2_from);
    push_u30(&mut abc, try2_to);
    push_u30(&mut abc, catch2_target);
    push_u30(&mut abc, 7);
    push_u30(&mut abc, 8);
    push_u30(&mut abc, 0); // 0 body traits

    let mut do_abc2 = Vec::new();
    do_abc2.extend_from_slice(&1u32.to_le_bytes());
    do_abc2.extend_from_slice(b"t470\0");
    do_abc2.extend_from_slice(&abc);

    let mut swf = SwfWriter::new();
    swf.tag(69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes: ActionScript3 (UseABC)
    swf.tag(82, &do_abc2);
    swf.tag(1, &[]);
    let swf_bytes = swf.into_swf(10, 100, 100, 1);
    let tmp = TempSwfFile::write("avm2-opcodes", &swf_bytes);

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("AVM2 player");
    let step = player.step_frame();
    assert!(
        step.unsupported_diagnostics.is_empty(),
        "unexpected unsupported diagnostics: {:?}",
        step.unsupported_diagnostics
    );
    assert_eq!(
        step.traces,
        vec![
            "vec-caught",
            "true1122",
            "79",
            "a&lt;b&amp;c\"da&lt;b&amp;c&quot;d",
            "filter-caught",
        ]
    );

    // Deterministic restart() and import_project() validation!
    let restarted = player.restart();
    assert_eq!(restarted.traces, step.traces);
    let imported = import_project(tmp.path()).expect("import AVM2 opcodes SWF");
    imported
        .validate()
        .expect("imported AVM2 project must validate");
}

#[test]
fn t470_ruffle_flash_player_differential_oracle_suite() {
    let Some(swfs_dir) = find_ruffle_swfs_dir() else {
        // Optional external checkout (`target/internet-flash-corpus/ruffle`) is absent in CI.
        return;
    };

    // Complete set of 77 Ruffle fixtures already covered in T-451, T-465, and T-462:
    let already_covered: BTreeSet<&str> = [
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
    ]
    .into_iter()
    .collect();

    let reference_movies = [
        // AVM1 (5): ActionTry, ActionThrow, catch-in-register, ActionInstanceOf, OOP & DefineFunction2
        "avm1/try_catch_finally",
        "avm1/catch_references_registers",
        "avm1/primitive_instanceof",
        "avm1/extends_chain",
        "avm1/define_function2",
        // AVM2 (36): try/catch, superinterface instanceof, Vector, Dictionary, E4X XML/XMLList, QName
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
            !already_covered.contains(rel),
            "{rel} overlaps earlier suite; keep T-470 movies strictly additional"
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
        passed.len() >= 40,
        "expected at least 40 additional Ruffle reference movies verified 1:1, got {}",
        passed.len()
    );
    eprintln!(
        "[T-470] Verified {}/{} additional non-AIR upstream Ruffle test.swf + output.txt movies 1:1 ({total_verified_lines} trace lines)",
        passed.len(),
        reference_movies.len()
    );
}
