//! `[T-472]` Track D — `avm2_runtime` Submodule Decomposition, AVM1/AVM2 Opcode & Builtin
//! Hardening, and `158 -> 310` (`+152` Strictly Disjoint Non-AIR) Upstream Ruffle Flash Player
//! `test.swf` + `output.txt` 1:1 Differential Oracles.

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
            "neoflash-t472-{label}-{}-{}.swf",
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

fn push_u30(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

fn push_s24(out: &mut Vec<u8>, val: i32) {
    let bytes = val.to_le_bytes();
    out.extend_from_slice(&bytes[..3]);
}

fn avm1_push_str(code: &mut Vec<u8>, s: &str) {
    code.push(0x96);
    let len = (1 + s.len() + 1) as u16;
    code.extend_from_slice(&len.to_le_bytes());
    code.push(0x00); // String
    code.extend_from_slice(s.as_bytes());
    code.push(0x00);
}

fn avm1_push_i32(code: &mut Vec<u8>, n: i32) {
    code.push(0x96);
    code.extend_from_slice(&5u16.to_le_bytes());
    code.push(0x07); // Integer (i32)
    code.extend_from_slice(&n.to_le_bytes());
}

fn avm1_push_undef(code: &mut Vec<u8>) {
    code.push(0x96);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(0x03); // Undefined
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read_dir");
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

#[test]
fn t472_avm2_runtime_decomposition_and_player_line_budget_guard() {
    let swf_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/swf");
    let player_root = swf_dir.join("player.rs");
    let player_dir = swf_dir.join("player");
    let avm2_rt_root = player_dir.join("avm2_runtime.rs");
    let avm2_rt_dir = player_dir.join("avm2_runtime");

    assert!(player_root.is_file(), "expected player.rs to exist");
    assert!(avm2_rt_root.is_file(), "expected avm2_runtime.rs to exist");
    assert!(
        avm2_rt_dir.is_dir(),
        "expected avm2_runtime/ submodule directory"
    );

    let mut all_player_files = vec![player_root];
    collect_rs_files(&player_dir, &mut all_player_files);
    all_player_files.sort();

    let mut avm2_submods = 0usize;
    for path in &all_player_files {
        let lines = std::fs::read_to_string(path)
            .expect("read source file")
            .lines()
            .count();
        assert!(
            lines <= 2_700,
            "{} exceeds 2,700-line SWF player budget: {lines} lines",
            path.display()
        );
        if path == &avm2_rt_root || path.starts_with(&avm2_rt_dir) {
            if path.starts_with(&avm2_rt_dir) {
                avm2_submods += 1;
            }
            assert!(
                lines <= 1_800,
                "{} exceeds 1,800-line avm2_runtime decomposition budget: {lines} lines",
                path.display()
            );
        }
    }
    assert!(
        avm2_submods >= 6,
        "expected at least 6 domain submodules in avm2_runtime/, got {avm2_submods}"
    );
}

#[test]
fn t472_avm1_to_integer_value_of_escape_unescape_is_finite_and_sparse_array_slice() {
    let mut code = Vec::new();

    // 1. var box = { valueOf: function() { return 42; } }; trace(ActionToInteger(box)); trace(int(box));
    avm1_push_str(&mut code, "box");
    avm1_push_i32(&mut code, 0);
    code.push(0x43); // ActionInitObject -> {}
    code.push(0x1D); // SetVariable("box", {})

    avm1_push_str(&mut code, "box");
    code.push(0x1C); // GetVariable("box")
    avm1_push_str(&mut code, "valueOf");
    // ActionDefineFunction("", 0 params): push 42, ActionReturn (0x3E)
    let mut fn_body = Vec::new();
    avm1_push_i32(&mut fn_body, 42);
    fn_body.push(0x3E);
    code.push(0x9B); // ActionDefineFunction
    let fn_hdr_len = (1 + 2 + 2) as u16;
    code.extend_from_slice(&fn_hdr_len.to_le_bytes());
    code.push(0x00); // empty name
    code.extend_from_slice(&0u16.to_le_bytes()); // 0 params
    code.extend_from_slice(&(fn_body.len() as u16).to_le_bytes());
    code.extend_from_slice(&fn_body);
    code.push(0x4F); // ActionSetMember (box.valueOf = fn)

    // ActionToInteger(box)
    avm1_push_str(&mut code, "box");
    code.push(0x1C);
    code.push(0x18); // ActionToInteger
    code.push(0x26); // ActionTrace -> "42"

    // int(box)
    avm1_push_str(&mut code, "box");
    code.push(0x1C);
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "int");
    code.push(0x3D); // ActionCallFunction
    code.push(0x26); // ActionTrace -> "42"

    // 2. isFinite("  5  ") -> false, isFinite(" 5") -> true, isFinite("0x1999999981ffffff") -> true, isFinite() -> false
    for s in ["  5  ", " 5", "0x1999999981ffffff", "0xhello"] {
        avm1_push_str(&mut code, s);
        avm1_push_i32(&mut code, 1);
        avm1_push_str(&mut code, "isFinite");
        code.push(0x3D);
        code.push(0x26);
    }
    avm1_push_i32(&mut code, 0);
    avm1_push_str(&mut code, "isFinite");
    code.push(0x3D);
    code.push(0x26); // false

    // 3. escape("AZ09@ ") -> "AZ09%40%20", escape("abc\0def") -> "abc", escape() -> "undefined", unescape("a%40%20b") -> "a@ b"
    avm1_push_str(&mut code, "AZ09@ ");
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "escape");
    code.push(0x3D);
    code.push(0x26);

    // Push string containing embedded NUL? In SWF Push(String) C-strings end at 0x00, so concat "abc" + String.fromCharCode(0) + "def":
    avm1_push_i32(&mut code, 0);
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "String");
    code.push(0x1C);
    avm1_push_str(&mut code, "fromCharCode");
    code.push(0x52); // CallMethod -> "\0"
    avm1_push_str(&mut code, "abc");
    code.push(0x4D); // ActionStackSwap -> "abc", "\0"
    code.push(0x47); // ActionAdd2 -> "abc\0"
    avm1_push_str(&mut code, "def");
    code.push(0x47); // ActionAdd2 -> "abc\0def"
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "escape");
    code.push(0x3D);
    code.push(0x26); // "abc"

    avm1_push_i32(&mut code, 0);
    avm1_push_str(&mut code, "escape");
    code.push(0x3D);
    code.push(0x26); // "undefined"

    avm1_push_str(&mut code, "a%40%20b");
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "unescape");
    code.push(0x3D);
    code.push(0x26); // "a@ b"

    // 4. Sparse new Array(3), hasOwnProperty(), slice(1, undefined)
    avm1_push_str(&mut code, "arr");
    avm1_push_i32(&mut code, 3);
    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "Array");
    code.push(0x40); // ActionNewObject -> new Array(3)
    code.push(0x1D); // SetVariable("arr", ...)

    avm1_push_str(&mut code, "arr");
    code.push(0x1C);
    code.push(0x26); // SWF7 sparse array trace -> "undefined,undefined,undefined"

    avm1_push_i32(&mut code, 0);
    avm1_push_str(&mut code, "box");
    code.push(0x1C);
    avm1_push_str(&mut code, "hasOwnProperty");
    code.push(0x52); // box.hasOwnProperty() (0 args) -> false
    code.push(0x26);

    // [10, 20, 30, 40].slice(1, undefined) -> "20,30,40"
    avm1_push_undef(&mut code);
    avm1_push_i32(&mut code, 1);
    avm1_push_i32(&mut code, 2); // 2 args: (1, undefined)
    avm1_push_i32(&mut code, 40);
    avm1_push_i32(&mut code, 30);
    avm1_push_i32(&mut code, 20);
    avm1_push_i32(&mut code, 10);
    avm1_push_i32(&mut code, 4);
    code.push(0x42); // ActionInitArray -> [10, 20, 30, 40]
    avm1_push_str(&mut code, "slice");
    code.push(0x52); // CallMethod("slice")
    code.push(0x26); // "20,30,40"

    // 5. tellTarget scope: _global.shared = "from_global"; _root.shared = "from_root"; createEmptyMovieClip("child", 1); tellTarget("child") { trace(shared); }
    avm1_push_str(&mut code, "_global");
    code.push(0x1C);
    avm1_push_str(&mut code, "shared");
    avm1_push_str(&mut code, "from_global");
    code.push(0x4F); // _global.shared = "from_global"

    avm1_push_str(&mut code, "shared");
    avm1_push_str(&mut code, "from_root");
    code.push(0x1D); // _root.shared = "from_root"

    avm1_push_i32(&mut code, 1);
    avm1_push_str(&mut code, "child");
    avm1_push_i32(&mut code, 2);
    avm1_push_str(&mut code, "createEmptyMovieClip");
    code.push(0x3D);
    code.push(0x17); // Pop

    code.push(0x8B); // ActionSetTarget("child")
    code.extend_from_slice(&6u16.to_le_bytes());
    code.extend_from_slice(b"child\0");
    avm1_push_str(&mut code, "shared");
    code.push(0x1C);
    code.push(0x26); // "from_global"
    code.push(0x8B); // ActionSetTarget("")
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(0x00);

    code.push(0x00); // End action

    let mut sw = SwfWriter::new();
    sw.tag(12, &code); // DoAction
    sw.tag(1, &[]); // ShowFrame
    let swf_bytes = sw.into_swf(7, 320, 240, 1);

    let tmp = TempSwfFile::write("avm1-coercions-escape-slice", &swf_bytes);
    let mut player = SwfBytecodePlayer::from_path(tmp.path()).expect("load AVM1 SWF");
    let step = player.step_frame();
    assert!(
        step.unsupported_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        step.unsupported_diagnostics
    );
    assert_eq!(
        step.traces,
        vec![
            "42",
            "42",
            "false",
            "true",
            "true",
            "false",
            "false",
            "AZ09%40%20",
            "abc",
            "undefined",
            "a@ b",
            "undefined,undefined,undefined",
            "false",
            "20,30,40",
            "from_global",
        ]
    );

    let report = inspect(tmp.path()).expect("inspect AVM1 SWF");
    assert!(report.can_import);
    let project = import_project(tmp.path()).expect("import AVM1 SWF");
    project.validate().expect("validate AVM1 SWF");
}

#[test]
fn t472_avm2_finddef_sparse_array_proto_substr_negative_dictionary_delete_and_type_methods() {
    let mut abc = Vec::new();
    abc.extend_from_slice(&16u16.to_le_bytes());
    abc.extend_from_slice(&46u16.to_le_bytes());

    // ints: 1=4, 2=2, 3=1, 4=-4, 5=-2, 6=9, 7=-9, 8=32, 9=5
    let ints = [4i32, 2, 1, -4, -2, 9, -9, 32, 5];
    push_u30(&mut abc, (ints.len() + 1) as u32);
    for v in ints {
        // signed s32 leb128
        let mut val = v;
        loop {
            let mut byte = (val & 0x7f) as u8;
            val >>= 7;
            let done = (val == 0 && (byte & 0x40) == 0) || (val == -1 && (byte & 0x40) != 0);
            if !done {
                byte |= 0x80;
            }
            abc.push(byte);
            if done {
                break;
            }
        }
    }
    push_u30(&mut abc, 0); // uints
    push_u30(&mut abc, 2); // doubles: 1 = -Infinity
    abc.extend_from_slice(&f64::NEG_INFINITY.to_le_bytes());

    let strings = [
        "",                     // 1
        "Object",               // 2
        "Box",                  // 3
        "trace",                // 4
        "Array",                // 5
        "prototype",            // 6
        "2",                    // 7
        "proto2",               // 8
        "hasOwnProperty",       // 9
        "1",                    // 10
        "shift",                // 11
        "unshift",              // 12
        "head",                 // 13
        "lastIndexOf",          // 14
        "toLocaleString",       // 15
        "String",               // 16
        "abcdefg",              // 17
        "substr",               // 18
        "\u{FF61}",             // 19
        "\u{10402}",            // 20
        "toString",             // 21
        "valueOf",              // 22
        "Dictionary",           // 23
        "absent",               // 24
        "XML",                  // 25
        "<r a='1'/>",           // 26
        "propertyIsEnumerable", // 27
        "parseInt",             // 28
        "undefined",            // 29
        "getTimer",             // 30
        "ApplicationDomain",    // 31
        "currentDomain",        // 32
        "hasDefinition",        // 33
        "getDefinition",        // 34
        "some.pkg.Nope",        // 35
        "MissingSym",           // 36
        "Event",                // 37
        "ping",                 // 38
        "call",                 // 39
    ];
    push_u30(&mut abc, (strings.len() + 1) as u32);
    for s in strings {
        push_u30(&mut abc, s.len() as u32);
        abc.extend_from_slice(s.as_bytes());
    }

    // namespaces: 1 = PackageNamespace("")
    push_u30(&mut abc, 2);
    abc.push(0x16);
    push_u30(&mut abc, 1);

    push_u30(&mut abc, 0); // ns_sets

    // multinames (1..=38): QName(1, strings[idx]) for strings 2..=39 -> mn_idx = str_idx - 1
    push_u30(&mut abc, 39);
    for str_idx in 2u32..=39 {
        abc.push(0x07);
        push_u30(&mut abc, 1);
        push_u30(&mut abc, str_idx);
    }

    // Methods: 0 = Box.iinit, 1 = Box.cinit, 2 = sinit
    push_u30(&mut abc, 3);
    for _ in 0..3 {
        push_u30(&mut abc, 0);
        push_u30(&mut abc, 0);
        push_u30(&mut abc, 0);
        abc.push(0);
    }
    push_u30(&mut abc, 0); // metadata

    // Class 0: Box extends Object
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 2); // mn#2 = "Box"
    push_u30(&mut abc, 1); // mn#1 = "Object"
    abc.push(0x01); // Sealed
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0); // iinit = 0
    push_u30(&mut abc, 0); // 0 instance traits
    // ClassInfo 0: cinit = 1
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0);

    // Script 0: sinit = 2, 1 trait: Box (mn#2 -> cls 0)
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 2);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 2);
    abc.push(0x04);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);

    let mn = |s_idx_1based: u32| -> u8 { (s_idx_1based - 1) as u8 };

    // Build sinit bytecode:
    let mut s = Vec::new();
    s.extend_from_slice(&[0xD0, 0x30]); // getlocal0, pushscope

    // (a) finddef Box (mn(3)=2) -> trace("[object global]")
    s.extend_from_slice(&[0x5D, mn(4), 0x5F, mn(3), 0x4F, mn(4), 0x01]);

    // (b) try { finddef MissingSym (mn(36)=35) } catch (e:*) { trace(e) }
    let try1_from = s.len();
    s.extend_from_slice(&[0x5F, mn(36), 0x29]); // finddef MissingSym, pop
    let try1_to = s.len();
    s.push(0x10); // jump over catch1
    let jump1_patch = s.len();
    push_s24(&mut s, 0);
    let catch1_target = s.len();
    // catch: setlocal1, findpropstrict trace, getlocal1, callpropvoid trace 1
    s.extend_from_slice(&[0xD5, 0x5D, mn(4), 0xD1, 0x4F, mn(4), 0x01]);
    let after_catch1 = s.len();
    let rel1 = (after_catch1 - (jump1_patch + 3)) as i32;
    s[jump1_patch..jump1_patch + 3].copy_from_slice(&rel1.to_le_bytes()[..3]);

    // (c) Array.prototype["2"] = "proto2"; var a = new Array(4);
    // getlex Array(mn(5)), getproperty prototype(mn(6)), pushstring "proto2"(8), setproperty "2"(mn(7))
    s.extend_from_slice(&[0x60, mn(5), 0x66, mn(6), 0x2C, 8, 0x61, mn(7)]);
    // findpropstrict Array, pushint #1(4), constructprop Array(mn(5)), 1, setlocal2
    s.extend_from_slice(&[0x5D, mn(5), 0x2D, 1, 0x4A, mn(5), 0x01, 0xD6]);
    // trace(a["2"]) -> "proto2"
    s.extend_from_slice(&[0x5D, mn(4), 0xD2, 0x66, mn(7), 0x4F, mn(4), 0x01]);
    // trace(a.hasOwnProperty("1")) -> "false"
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0xD2,
        0x2C,
        10,
        0x46,
        mn(9),
        0x01,
        0x4F,
        mn(4),
        0x01,
    ]);
    // a.shift(); trace(a["1"]) -> hole shifted from 2 to 1, still inherits Array.prototype["2"]? No, index 1 is a hole so undefined, while a["2"] is STILL a hole so inherits "proto2"!
    s.extend_from_slice(&[0xD2, 0x4F, mn(11), 0x00]); // a.shift()
    s.extend_from_slice(&[0x5D, mn(4), 0xD2, 0x66, mn(7), 0x4F, mn(4), 0x01]); // trace(a["2"]) -> "proto2"
    // [5, 1].lastIndexOf(5, -9) -> 0
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2D,
        9, // 5
        0x2D,
        3, // 1
        0x56,
        0x02, // newarray [5, 1]
        0x2D,
        9, // 5
        0x24,
        (-9i8) as u8, // -9
        0x46,
        mn(14),
        0x02, // callproperty lastIndexOf(5, -9)
        0x4F,
        mn(4),
        0x01,
    ]);
    // ["x", "y"] -> [5, 1].toLocaleString() -> "5,1"
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2D,
        9,
        0x2D,
        3,
        0x56,
        0x02,
        0x46,
        mn(15),
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);

    // (d) var cls = String; trace(cls()) -> ""
    // getlex String(mn(16)), pushnull, call 0 -> ""
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x60,
        mn(16),
        0x20,
        0x41,
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);

    // (e) "abcdefg".substr(1, -4) -> "bcd", "abcdefg".substr(2, -2) -> "", "abcdefg".substr(9, -Infinity) -> ""
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2C,
        17,
        0x2D,
        3,
        0x24,
        (-4i8) as u8,
        0x46,
        mn(18),
        0x02,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2C,
        17,
        0x2D,
        2,
        0x24,
        (-2i8) as u8,
        0x46,
        mn(18),
        0x02,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2C,
        17,
        0x2D,
        6,
        0x2F,
        1, // pushdouble -Infinity
        0x46,
        mn(18),
        0x02,
        0x4F,
        mn(4),
        0x01,
    ]);

    // (f) "\u{FF61}" < "\u{10402}" -> false (UTF-16 code unit order: 0xFF61 > 0xD800)
    s.extend_from_slice(&[0x5D, mn(4), 0x2C, 19, 0x2C, 20, 0xAD, 0x4F, mn(4), 0x01]);

    // (g) Box.toString(), Box.toLocaleString(), Box.valueOf() === Box, ({}).toLocaleString(), Object.prototype.toString.call(new Event("ping"))
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x60,
        mn(3),
        0x46,
        mn(21),
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x60,
        mn(3),
        0x46,
        mn(15),
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x60,
        mn(3),
        0x46,
        mn(22),
        0x00,
        0x60,
        mn(3),
        0xAC, // strictequals
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x55,
        0x00,
        0x46,
        mn(15),
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);
    // Object.prototype.toString.call(new Event("ping")) -> "[object Event]"
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x60,
        mn(2), // Object
        0x66,
        mn(6), // .prototype
        0x66,
        mn(21), // .toString
        0x5D,
        mn(37), // Event
        0x2C,
        38, // "ping"
        0x4A,
        mn(37),
        0x01, // new Event("ping")
        0x46,
        mn(39),
        0x01, // .call(...)
        0x4F,
        mn(4),
        0x01,
    ]);

    // (h) delete (new Dictionary())["absent"] -> true
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x5D,
        mn(23),
        0x4A,
        mn(23),
        0x00, // new Dictionary()
        0x6A,
        mn(24), // deleteproperty "absent"
        0x4F,
        mn(4),
        0x01,
    ]);

    // (i) "propertyIsEnumerable" in new XML("<r a='1'/>") -> true
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x2C,
        27, // "propertyIsEnumerable"
        0x5D,
        mn(25),
        0x2C,
        26,
        0x4A,
        mn(25),
        0x01, // new XML("<r a='1'/>")
        0xB4, // in
        0x4F,
        mn(4),
        0x01,
    ]);

    // (j) parseInt(undefined, 32) -> 785077, parseInt("undefined", 32) -> 33790067563981, getTimer() -> 0
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x5D,
        mn(28),
        0x21, // pushundefined
        0x2D,
        8, // 32
        0x46,
        mn(28),
        0x02,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x5D,
        mn(28),
        0x2C,
        29, // "undefined"
        0x2D,
        8, // 32
        0x46,
        mn(28),
        0x02,
        0x4F,
        mn(4),
        0x01,
    ]);
    s.extend_from_slice(&[
        0x5D,
        mn(4),
        0x5D,
        mn(30),
        0x46,
        mn(30),
        0x00,
        0x4F,
        mn(4),
        0x01,
    ]);

    // (k) try { ApplicationDomain.currentDomain.hasDefinition(null) } catch (e:*) { trace(e) }
    let try2_from = s.len();
    s.extend_from_slice(&[0x60, mn(31), 0x66, mn(32), 0x20, 0x46, mn(33), 0x01, 0x29]);
    let try2_to = s.len();
    s.push(0x10);
    let jump2_patch = s.len();
    push_s24(&mut s, 0);
    let catch2_target = s.len();
    s.extend_from_slice(&[0xD5, 0x5D, mn(4), 0xD1, 0x4F, mn(4), 0x01]);
    let after_catch2 = s.len();
    let rel2 = (after_catch2 - (jump2_patch + 3)) as i32;
    s[jump2_patch..jump2_patch + 3].copy_from_slice(&rel2.to_le_bytes()[..3]);

    // (l) try { ApplicationDomain.currentDomain.getDefinition("some.pkg.Nope") } catch (e:*) { trace(e) }
    let try3_from = s.len();
    s.extend_from_slice(&[
        0x60,
        mn(31),
        0x66,
        mn(32),
        0x2C,
        35,
        0x46,
        mn(34),
        0x01,
        0x29,
    ]);
    let try3_to = s.len();
    s.push(0x10);
    let jump3_patch = s.len();
    push_s24(&mut s, 0);
    let catch3_target = s.len();
    s.extend_from_slice(&[0xD5, 0x5D, mn(4), 0xD1, 0x4F, mn(4), 0x01]);
    let after_catch3 = s.len();
    let rel3 = (after_catch3 - (jump3_patch + 3)) as i32;
    s[jump3_patch..jump3_patch + 3].copy_from_slice(&rel3.to_le_bytes()[..3]);

    s.push(0x47); // returnvoid

    // Method bodies (3): 0 (Box.iinit), 1 (Box.cinit), 2 (sinit with 3 exception ranges)
    push_u30(&mut abc, 3);
    // Body 0: Box.iinit
    let box_iinit = [0xD0u8, 0x30, 0xD0, 0x49, 0x00, 0x47];
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 4);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 2);
    push_u30(&mut abc, box_iinit.len() as u32);
    abc.extend_from_slice(&box_iinit);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);

    // Body 1: Box.cinit
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 2);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);
    abc.push(0x47);
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 0);

    // Body 2: sinit
    push_u30(&mut abc, 2);
    push_u30(&mut abc, 8); // max_stack
    push_u30(&mut abc, 4); // local_count
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 3);
    push_u30(&mut abc, s.len() as u32);
    abc.extend_from_slice(&s);
    push_u30(&mut abc, 3); // 3 exception handlers
    for (from, to, target) in [
        (try1_from, try1_to, catch1_target),
        (try2_from, try2_to, catch2_target),
        (try3_from, try3_to, catch3_target),
    ] {
        push_u30(&mut abc, from as u32);
        push_u30(&mut abc, to as u32);
        push_u30(&mut abc, target as u32);
        push_u30(&mut abc, 0); // catch-all (*)
        push_u30(&mut abc, 0);
    }
    push_u30(&mut abc, 0); // 0 traits

    let mut doabc2 = Vec::new();
    doabc2.extend_from_slice(&0u32.to_le_bytes()); // flags = eager
    doabc2.extend_from_slice(b"frame1\0");
    doabc2.extend_from_slice(&abc);

    let mut sw = SwfWriter::new();
    sw.tag(69, &[0x08, 0x00, 0x00, 0x00]); // FileAttributes (ActionScript3 = 1 << 3)
    sw.tag(82, &doabc2); // DoABC2
    sw.tag(1, &[]); // ShowFrame
    let swf_bytes = sw.into_swf(10, 320, 240, 1);

    let tmp = TempSwfFile::write("avm2-finddef-sparse-substr-dict", &swf_bytes);
    let mut player = SwfBytecodePlayer::from_path(tmp.path()).expect("load AVM2 SWF");
    let step = player.step_frame();
    assert!(
        step.unsupported_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        step.unsupported_diagnostics
    );
    assert_eq!(
        step.traces,
        vec![
            "[object global]",
            "ReferenceError: Error #1065: Variable MissingSym is not defined.",
            "proto2",
            "false",
            "proto2",
            "0",
            "5,1",
            "",
            "bcd",
            "",
            "",
            "false",
            "[class Box]",
            "[class Box]",
            "true",
            "[object Object]",
            "[object Event]",
            "true",
            "true",
            "785077",
            "33790067563981",
            "0",
            "TypeError: Error #2007: Parameter name must be non-null.",
            "ReferenceError: Error #1065: Variable Nope is not defined.",
        ]
    );

    let report = inspect(tmp.path()).expect("inspect AVM2 SWF");
    assert!(report.can_import);
    let project = import_project(tmp.path()).expect("import AVM2 SWF");
    project.validate().expect("validate AVM2 SWF");
}

#[test]
fn t472_ruffle_flash_player_differential_oracle_suite() {
    let Some(swfs_dir) = find_ruffle_swfs_dir() else {
        return;
    };

    // Complete set of 158 Ruffle reference movies already covered across T-451, T-465, T-462, T-470, and T-471:
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
        // T-471 (40)
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
        "avm1/closure_scope",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        previously_covered.len(),
        158,
        "expected exact 158 baseline movies from T-451/T-465/T-462/T-470/T-471"
    );

    let reference_movies = [
        // AVM1 coercions, globals, arrays, OOP, scope chains, & timeline lifecycle (42)
        "avm1/action_to_integer",
        "avm1/array_concat",
        "avm1/array_slice",
        "avm1/as1_constructor_v7",
        "avm1/as2_oop",
        "avm1/attach_movie_stop",
        "avm1/call_method_empty_name",
        "avm1/conflicting_instance_names",
        "avm1/custom_clip_methods",
        "avm1/define_function_case_sensitive",
        "avm1/escape",
        "avm1/global_is_bare",
        "avm1/is_finite",
        "avm1/is_finite_swf6",
        "avm1/issue_1086",
        "avm1/issue_2166",
        "avm1/issue_3446",
        "avm1/issue_4377",
        "avm1/logical_ops_swf4",
        "avm1/logical_ops_swf8",
        "avm1/looping_real_1_declared_1",
        "avm1/looping_real_1_declared_2",
        "avm1/looping_real_2_declared_1",
        "avm1/looping_real_2_declared_2",
        "avm1/movieclip_init_object",
        "avm1/new_object_enumerate",
        "avm1/root_global_parent",
        "avm1/set_variable_scope",
        "avm1/shared_object_serialize_typed_objects",
        "avm1/single_frame",
        "avm1/stage_object_enumerate",
        "avm1/stage_object_properties_get_var",
        "avm1/string_relational_compare",
        "avm1/swf4_actions_bool",
        "avm1/swf4_bool",
        "avm1/target_clip_removed",
        "avm1/target_path",
        "avm1/timeline_function_def",
        "avm1/try_catch_stack",
        "avm1/try_finally_simple",
        "avm1/unload_clip_event",
        "avm1/variable_args",
        // AVM2 opcodes, sparse arrays, strings, Dictionary, E4X, OOP, & DisplayObjectContainer (110)
        "avm2/application_domain",
        "avm2/array_enumeration",
        "avm2/array_hasownproperty",
        "avm2/array_holes",
        "avm2/array_lastindexof",
        "avm2/array_push",
        "avm2/array_shift",
        "avm2/array_storage",
        "avm2/array_unshift",
        "avm2/callproplex_class",
        "avm2/checkfilter",
        "avm2/class_init_interpreter_mode",
        "avm2/class_to_locale_string",
        "avm2/class_to_string",
        "avm2/class_value_of",
        "avm2/constructor_call",
        "avm2/constructors_vs_timeline",
        "avm2/control_flow_bool",
        "avm2/control_flow_stricteq",
        "avm2/declocal",
        "avm2/declocal_i",
        "avm2/dictionary_delete",
        "avm2/dictionary_namespaces",
        "avm2/displayobject_alpha",
        "avm2/displayobject_parent",
        "avm2/displayobject_root",
        "avm2/displayobject_visible",
        "avm2/displayobjectcontainer_addchild",
        "avm2/displayobjectcontainer_addchild_lazy_sprite",
        "avm2/displayobjectcontainer_addchild_timelinepull0",
        "avm2/displayobjectcontainer_addchild_timelinepull1",
        "avm2/displayobjectcontainer_addchild_timelinepull2",
        "avm2/displayobjectcontainer_contains",
        "avm2/displayobjectcontainer_getchildat",
        "avm2/displayobjectcontainer_getchildbyname",
        "avm2/displayobjectcontainer_getchildbyname_wrongcase",
        "avm2/displayobjectcontainer_getchildindex",
        "avm2/displayobjectcontainer_removechild",
        "avm2/displayobjectcontainer_removechild_timelinemanip_remove1",
        "avm2/doabc_is_eager",
        "avm2/documentclass",
        "avm2/duplicate_defs",
        "avm2/eager_init",
        "avm2/event_bubbles",
        "avm2/event_cancelable",
        "avm2/event_type",
        "avm2/event_valueof_tostring",
        "avm2/eventdispatcher_interface_invoke",
        "avm2/falsiness",
        "avm2/finddef",
        "avm2/function_call_via_apply",
        "avm2/function_to_locale_string",
        "avm2/function_to_string",
        "avm2/function_type",
        "avm2/function_value_of",
        "avm2/get_timer",
        "avm2/goto_button_nested_framescript",
        "avm2/goto_in_scene_last_frame",
        "avm2/goto_nested_construct_sibling",
        "avm2/hasownproperty_namespaces",
        "avm2/hello_world",
        "avm2/inclocal",
        "avm2/inclocal_i",
        "avm2/indexof_xml",
        "avm2/is_finite",
        "avm2/is_nan",
        "avm2/issue_14901",
        "avm2/issue_5292",
        "avm2/movieclip_addframescript",
        "avm2/movieclip_gotoandstop_children",
        "avm2/movieclip_gotoandstop_framescripts2",
        "avm2/movieclip_next_frame",
        "avm2/movieclip_play",
        "avm2/movieclip_prev_frame",
        "avm2/movieclip_queued_noop_goto_swf9",
        "avm2/movieclip_stop",
        "avm2/movieclip_super_is_symbol",
        "avm2/movieclip_symbol_constr",
        "avm2/newactivation_in_script_init",
        "avm2/number_autoconv_array_sort_32bit",
        "avm2/object_to_locale_string",
        "avm2/op_escxattr",
        "avm2/op_escxelem",
        "avm2/op_lookupswitch",
        "avm2/optimize_coerce",
        "avm2/param_default_value_has_zero_cpool_index",
        "avm2/parent_early_access_child",
        "avm2/property_priority_chained",
        "avm2/property_priority_three_level",
        "avm2/qname_as_lazy_name_attribute_multiname",
        "avm2/soundmixer_stopall",
        "avm2/stage_access",
        "avm2/stored_properties",
        "avm2/string_call",
        "avm2/string_relational_compare",
        "avm2/string_substr_negative",
        "avm2/string_substr_weird",
        "avm2/subclass_superclass_linked_symbol",
        "avm2/swf8",
        "avm2/timeline_scripts",
        "avm2/unchecked_function",
        "avm2/vector_reborrow_bug",
        "avm2/versioned_isplaying",
        "avm2/virtual_properties",
        "avm2/xml_as_attribute",
        "avm2/xml_equals_namespace_check",
        "avm2/xml_explicit_use_namespace",
        "avm2/xml_has_property_via_in",
        "avm2/xml_list_as_attribute",
        "avm2/xml_list_delete_clear_parent",
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
            "{rel} overlaps earlier T-451..T-471 suite; keep T-472 movies strictly disjoint"
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
            !toml_str.contains("air = true") && !toml_str.contains("runtime = \"AIR\""),
            "{rel} must be a non-AIR Flash Player reference movie"
        );
        assert!(
            !toml_str.contains("known_failure = true"),
            "{rel} must not be marked known_failure"
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
        passed.len() >= 110,
        "expected at least 110 strictly disjoint Ruffle reference movies verified 1:1, got {}",
        passed.len()
    );
    assert!(
        previously_covered.len() + passed.len() >= 263,
        "expected cumulative Ruffle oracle coverage >= 263, got {}",
        previously_covered.len() + passed.len()
    );
    eprintln!(
        "[T-472] Verified {}/{} strictly disjoint non-AIR Ruffle test.swf + output.txt movies 1:1 ({total_verified_lines} trace lines; {} cumulative across T-451..T-472)",
        passed.len(),
        reference_movies.len(),
        previously_covered.len() + passed.len()
    );
}
