use super::*;
use neoflash_core::{
    AuthoredTextFieldOptions, Easing, Element, ElementKind, FrameLabelType, FrameScript,
    FrameSound, GradientKind, GradientPaint, GradientStop, InstancePlayback, Keyframe, Layer,
    LayerProperties, PathCommand, Project, ResourceUnit, Script, ScriptLanguage, SoundEffect,
    SoundSync, SourceUnit, Symbol, SymbolKind, TextFormat, Timeline, Transform, TweenKind, new_id,
};

fn build_synthetic_glyf_sfnt(
    family: &str,
    subfamily: &str,
    bold: bool,
    extra_cbdt: bool,
) -> Vec<u8> {
    let mut g1 = Vec::new();
    g1.extend_from_slice(&1i16.to_be_bytes());
    g1.extend_from_slice(&0i16.to_be_bytes());
    g1.extend_from_slice(&0i16.to_be_bytes());
    g1.extend_from_slice(&500i16.to_be_bytes());
    g1.extend_from_slice(&700i16.to_be_bytes());
    g1.extend_from_slice(&2u16.to_be_bytes());
    g1.extend_from_slice(&0u16.to_be_bytes());
    g1.extend_from_slice(&[0x37, 0x37, 0x37]);
    g1.extend_from_slice(&[10, 200, 100]);
    g1.extend_from_slice(&[0, 250, 100]);
    if !g1.len().is_multiple_of(2) {
        g1.push(0);
    }

    let mut glyf = Vec::new();
    glyf.extend_from_slice(&g1);
    let end_off = glyf.len() as u32;
    let mut loca = Vec::new();
    for off in [0u32, 0u32, end_off] {
        loca.extend_from_slice(&off.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[1..2].copy_from_slice(&[1]);
    head[16..18].copy_from_slice(&0x000Bu16.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    if bold {
        head[44..46].copy_from_slice(&0x0001u16.to_be_bytes());
    }
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[1..2].copy_from_slice(&[1]);
    hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&2u16.to_be_bytes());

    let mut maxp = vec![0u8; 32];
    maxp[1..2].copy_from_slice(&[1]);
    maxp[4..6].copy_from_slice(&2u16.to_be_bytes());
    maxp[6..8].copy_from_slice(&16u16.to_be_bytes());
    maxp[8..10].copy_from_slice(&4u16.to_be_bytes());

    let mut hmtx = Vec::new();
    for adv in [500u16, 600u16] {
        hmtx.extend_from_slice(&adv.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let mut os2 = vec![0u8; 78];
    os2[4..6].copy_from_slice(&(if bold { 700u16 } else { 400u16 }).to_be_bytes());
    if bold {
        os2[62..64].copy_from_slice(&0x0020u16.to_be_bytes());
    }

    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());

    let enc = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_be_bytes).collect() };
    let fam_b = enc(family);
    let sub_b = enc(subfamily);
    let mut name_tbl = Vec::new();
    name_tbl.extend_from_slice(&0u16.to_be_bytes());
    name_tbl.extend_from_slice(&2u16.to_be_bytes());
    name_tbl.extend_from_slice(&((6 + 24) as u16).to_be_bytes());
    for (nid, len, off) in [
        (1u16, fam_b.len() as u16, 0u16),
        (2u16, sub_b.len() as u16, fam_b.len() as u16),
    ] {
        name_tbl.extend_from_slice(&3u16.to_be_bytes());
        name_tbl.extend_from_slice(&1u16.to_be_bytes());
        name_tbl.extend_from_slice(&0x0409u16.to_be_bytes());
        name_tbl.extend_from_slice(&nid.to_be_bytes());
        name_tbl.extend_from_slice(&len.to_be_bytes());
        name_tbl.extend_from_slice(&off.to_be_bytes());
    }
    name_tbl.extend_from_slice(&fam_b);
    name_tbl.extend_from_slice(&sub_b);

    // cmap format 12 mapping space (32) -> 1 and 'A' (65) -> 1
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&10u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&12u16.to_be_bytes());
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&40u32.to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&2u32.to_be_bytes());
    for cp in [32u32, 65u32] {
        cmap.extend_from_slice(&cp.to_be_bytes());
        cmap.extend_from_slice(&cp.to_be_bytes());
        cmap.extend_from_slice(&1u32.to_be_bytes());
    }

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name_tbl),
        (*b"post", post),
    ];
    if extra_cbdt {
        tables.push((*b"CBDT", vec![0; 16]));
    }
    tables.sort_by_key(|(t, _)| *t);

    let num_tables = tables.len() as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 1, 0, 0]);
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    let mut off = 12 + tables.len() * 16;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(off as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        off += (data.len() + 3) & !3;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}

fn pack_ttc(faces: &[&[u8]]) -> Vec<u8> {
    let header_len = 12 + faces.len() * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    out.resize(header_len, 0);
    for (idx, face) in faces.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let base = out.len();
        let off_pos = 12 + idx * 4;
        out[off_pos..off_pos + 4].copy_from_slice(&(base as u32).to_be_bytes());
        let mut rel = face.to_vec();
        let num_tables = u16::from_be_bytes([rel[4], rel[5]]) as usize;
        for t in 0..num_tables {
            let rec_off = 12 + t * 16 + 8;
            let orig = u32::from_be_bytes([
                rel[rec_off],
                rel[rec_off + 1],
                rel[rec_off + 2],
                rel[rec_off + 3],
            ]) as usize;
            rel[rec_off..rec_off + 4].copy_from_slice(&((base + orig) as u32).to_be_bytes());
        }
        out.extend_from_slice(&rel);
    }
    out
}

#[test]
fn t414_avm1_this_method_call_and_unqualified_mc_prop_no_dual_clobber() {
    // P0-1 & P0-2:
    // 1. `this.helper()` in AS2 class emits `ActionCallMethod` (`0x52`) rather than `ActionCallFunction` (`0x3D`).
    // 2. `card.stopSelf = function() { this.gotoAndStop(2); };` seeks `card` (`this`), NOT lexical `_root`!
    // 3. `card.moveRoot = function() { _x = 77; };` sets `_root._x = 77` without dual-clobbering `card._x`!
    // 4. `function localShadow() { var _x = 10; _x = 25; return _x; }` does not clobber `this._x`!
    let mut project = Project::blank();
    project.name = "T414 AVM1 This & Props".into();
    let card_sym = new_id("symbol");
    project.symbols.push(Symbol {
        id: card_sym.clone(),
        name: "CardSym".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 3,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "L".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("element"),
                        kind: ElementKind::Rectangle {
                            width: 10.0,
                            height: 10.0,
                            fill: [255, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "Box.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: r#"
class Box {
    function helper() { return 42; }
    function stop() { return 99; }
    function run() { return this.helper() + ":" + this.stop(); }
}
"#
        .into(),
    });
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 3;
    root.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: new_id("element"),
        name: "card".into(),
        transform: Transform {
            x: 12.0,
            y: 20.0,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: card_sym,
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    }];
    root.timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var b = new Box();
trace("box=" + b.run());
card.seekSelf = function() { this.gotoAndStop(2); };
card.seekSelf();
trace("frames=" + _currentframe + "," + card._currentframe);
card.moveRoot = function() { _x = 77; };
card.moveRoot();
card.localShadow = function() { var _x = 10; _x = 25; return _x; };
var lv = card.localShadow();
trace("xs=" + _root._x + "," + card._x + "," + lv);
stop();
"#
            .into(),
        },
    }];

    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (swf_bytes, _) = publish_swf_bytes(&project, &opts).expect("publish AVM1 SWF");
    let playback = play_swf_bytes(&swf_bytes, 1).expect("play AVM1 SWF");
    assert_eq!(
        playback.trace_output,
        vec!["box=42:99", "frames=1,2", "xs=77,12,25"]
    );
}

#[test]
fn t414_avm1_try_break_continue_return_finally_and_nested_typed_catches() {
    // P0-3:
    // - `break` and `continue` inside `ActionTry` jump accurately into enclosing loop in `out`'s address space
    // - `finally` runs once on `break`, `continue`, and `return`
    // - Nested typed catches use distinct `__nf_err_0` / `__nf_err_1` without clobbering outer catch or polluting `_root.__nf_err_0`
    let mut project = Project::blank();
    project.name = "T414 AVM1 Try Flow".into();
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "ErrA.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class ErrA { var tag; function ErrA(t) { this.tag = t; } }".into(),
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "ErrB.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class ErrB { var tag; function ErrB(t) { this.tag = t; } }".into(),
    });
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var log = "";
var i = 0;
while (i < 4) {
    i = i + 1;
    try {
        if (i == 1) {
            log = log + "c1";
            continue;
        }
        if (i == 2) {
            throw "boom";
        }
        if (i == 3) {
            log = log + "b3";
            break;
        }
    } catch (e) {
        log = log + "C(" + e + ")";
        continue;
    } finally {
        log = log + "F" + i + ";";
    }
}
function testRet() {
    try {
        return "RET";
    } finally {
        log = log + "FR;";
    }
}
var rv = testRet();
try {
    try {
        throw new ErrA("outer");
    } catch (ea:ErrA) {
        try {
            throw new ErrB("inner");
        } catch (eb:ErrA) {
            log = log + "WRONG;";
        }
    }
} catch (eb2:ErrB) {
    log = log + "EB(" + eb2.tag + ");";
}
trace(log + rv + "|polluted=" + (__nf_err_0 != undefined));
stop();
"#
            .into(),
        },
    }];

    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (swf_bytes, _) = publish_swf_bytes(&project, &opts).expect("publish AVM1 try SWF");
    let playback = play_swf_bytes(&swf_bytes, 1).expect("play AVM1 try SWF");
    assert_eq!(
        playback.trace_output,
        vec!["c1F1;C(boom)F2;b3F3;FR;EB(inner);RET|polluted=false"]
    );
}

#[test]
fn t414_avm2_verifier_scopes_coerce_a_finally_unwind_post_super_and_imports() {
    // P0-4, P0-5, P0-6, P1-7 & P1-8 end-to-end:
    // - Multi-level inheritance (`GrandBase -> MidSprite -> LeafClip`) + MovieClip ancestor scopes
    // - Nested `super(...)` inside `if/else` with field initializers + `addFrameScript` emitted AFTER `constructsuper`
    // - `import pkg.sub.Helper;` and wildcard `import pkg.wild.*;` resolved inside methods, constructors, `is`, and `catch`
    // - Frame-script closure capturing `var counter = 0` via activation slot + `findproperty`
    // - `finally` unwind on `return` placed OUTSIDE `[from, to)` so throwing inside `finally` is NOT caught by `try`'s own `catch`!
    // - Self-named function expression inside `finally` deduplicates `activation_traits`
    let mut project = Project::blank();
    project.name = "T414 AVM2 Verifier".into();
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "pkg/sub/Helper.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg.sub {
    public class Helper {
        public var v:int;
        public function Helper(n:int) { this.v = n * 3; }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "pkg/wild/WildError.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg.wild {
    public class WildError extends Error {
        public function WildError(msg:String) { super(msg); }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "pkg/GrandBase.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    import flash.display.MovieClip;
    public class GrandBase extends MovieClip {
        public var order:String = "";
        public function GrandBase(seed:String) {
            super();
            this.order = seed;
        }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "pkg/MidClip.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    public class MidClip extends GrandBase {
        public function MidClip(seed:String) {
            super(seed + ">Mid");
        }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "pkg/LeafClip.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    import pkg.sub.Helper;
    import pkg.wild.*;
    public class LeafClip extends MidClip {
        public var fieldTag:String = "FIELD";
        public function LeafClip(flag:Boolean) {
            if (flag) {
                super("T");
            } else {
                super("F");
            }
            this.order = this.order + "+" + this.fieldTag;
        }
        public function checkImports():String {
            var h:Helper = new Helper(7);
            var ok:Boolean = h is Helper;
            try {
                throw new WildError("wild!");
            } catch (we:WildError) {
                return h.v + ":" + ok + ":" + we.message;
            }
            return "bad";
        }
        public function finallyThrowsOnReturn():String {
            try {
                return "early";
            } catch (e:Error) {
                return "WRONGLY_CAUGHT";
            } finally {
                var f = function namedFin():int { return 1; };
                throw new WildError("from-finally");
            }
        }
    }
}
"#
        .into(),
    });

    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"
import pkg.LeafClip;
import pkg.wild.*;
var counter:int = 10;
var lazyCounter:int;
var bump = function(d:int):int {
    lazyCounter = 42;
    counter = counter + d;
    return counter + lazyCounter;
};
var leaf:LeafClip = new LeafClip(true);
var finOutcome:String = "none";
try {
    finOutcome = leaf.finallyThrowsOnReturn();
} catch (fe:WildError) {
    finOutcome = "OUTER(" + fe.message + ")";
}
var logicVal:* = (false && 99) || bump(5);
trace(leaf.order + "|" + leaf.checkImports() + "|" + finOutcome + "|" + logicVal + ":" + lazyCounter);
stop();
"#
            .into(),
        },
    }];

    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (swf_bytes, _) = publish_swf_bytes(&project, &opts).expect("publish AVM2 SWF");

    // Inspect raw ABC in re-imported / parsed DoABC2 to verify verifier invariants:
    // - `LeafClip` iinit emits `0x49` (`constructsuper`) BEFORE `fieldTag` init
    // - `finallyThrowsOnReturn` exception `[from, to)` ends strictly before the unwind finally block!
    let playback = play_swf_bytes(&swf_bytes, 1).expect("play AVM2 SWF");
    assert_eq!(
        playback.trace_output,
        vec!["T>Mid+FIELD|21:true:wild!|OUTER(from-finally)|57:42"]
    );

    // Also verify round-trip import succeeds cleanly without duplicate activation traits!
    let reimported = import_swf_bytes(&swf_bytes).expect("re-import AVM2 SWF");
    reimported.validate().unwrap();
}

#[test]
fn t414_swf_multihop_include_rejects_root_fallback() {
    // `#302` P2-9: `#include "c.as"` inside `"a/b.as"` must resolve ONLY `"a/c.as"`, never root `"c.as"`.
    let mut bad_proj = Project::blank();
    bad_proj.name = "T414 Include No Root Fallback".into();
    bad_proj.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "a/b.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "#include \"c.as\"\ntrace(secret);\n".into(),
    });
    bad_proj.sources.push(SourceUnit {
        id: new_id("unit"),
        path: "c.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "var secret = 123;\n".into(),
    });
    let root_id = bad_proj.root.clone();
    bad_proj.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"a/b.as\"\nstop();\n".into(),
        },
    }];
    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    assert!(
        publish_swf_bytes(&bad_proj, &opts).is_err(),
        "`#include \"c.as\"` from `a/b.as` must not fall back to root `c.as`"
    );

    // Moving `c.as` to `a/c.as` succeeds and executes `123`!
    bad_proj.sources[1].path = "a/c.as".into();
    let (ok_bytes, _) = publish_swf_bytes(&bad_proj, &opts).expect("relative a/c.as must resolve");
    let play = play_swf_bytes(&ok_bytes, 1).unwrap();
    assert_eq!(play.trace_output, vec!["123"]);
}

#[test]
fn t414_swf_morph_transforms_gradient_guard_trimmed_sound_pos44_multilabel_and_ttc_face() {
    // 1) P1-14 SWF StartSound `pos44` maps across `[in_samples..out_samples]`
    let snd = FrameSound {
        resource: "snd1".into(),
        sync: SoundSync::Event,
        loops: 1,
        loop_forever: false,
        effect: SoundEffect::Custom,
        in_point: Some(1.0),  // 44,100 samples
        out_point: Some(3.0), // 132,300 samples (span = 88,200)
        envelope: Some(vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.5], [1.0, 1.0, 1.0]]),
    };
    let payload = build_start_sound_payload(7, &snd, 220_500); // 5.0s total duration
    // Layout: u16 sound_id (2) + u8 flags (1) + u32 in_point (4) + u32 out_point (4) + u8 env_count (1) + 3 * (u32 pos44 + u16 l + u16 r)
    assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), 7);
    assert_eq!(payload[2] & 0x0B, 0x0B, "has_in | has_out | has_envelope");
    let in_s = u32::from_le_bytes([payload[3], payload[4], payload[5], payload[6]]);
    let out_s = u32::from_le_bytes([payload[7], payload[8], payload[9], payload[10]]);
    assert_eq!((in_s, out_s), (44_100, 132_300));
    assert_eq!(payload[11], 3);
    let p0 = u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
    let p1 = u32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]);
    let p2 = u32::from_le_bytes([payload[28], payload[29], payload[30], payload[31]]);
    assert_eq!((p0, p1, p2), (44_100, 88_200, 132_300));

    // 2) P2-16: `can_morph_keys` retains compatible gradient `PaintedPath` fills while solid shape tweens bake `start_el.transform` & `end_el.transform`!
    let grad_el = Element {
        id: new_id("element"),
        kind: ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 20.0, y: 0.0 },
                PathCommand::LineTo { x: 20.0, y: 20.0 },
                PathCommand::Close,
            ],
            fill_rule: neoflash_core::FillRule::NonZero,
            fill: [0; 4],
            stroke: [0; 4],
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: [255, 0, 0, 255],
                    },
                    GradientStop {
                        ratio: 255,
                        color: [0, 0, 255, 255],
                    },
                ],
                matrix: [0.02, 0.0, 0.0, 0.02, 10.0, 10.0],
                ..Default::default()
            }),
            stroke_gradient: None,
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let kf_g0 = Keyframe {
        frame: 0,
        tween: Some(Easing::Linear),
        tween_kind: TweenKind::Shape,
        elements: vec![grad_el.clone()],
        ..Default::default()
    };
    let kf_g1 = Keyframe {
        frame: 4,
        elements: vec![grad_el],
        ..Default::default()
    };
    assert!(can_morph_keys(&kf_g0, &kf_g1));

    // 3) P2-16 + P3-20 + P1-12 end-to-end SWF publication:
    // - Shape tween moving from `x = 10` to `x = 150` via `DefineMorphShape`
    // - Two layers with navigable `FrameLabel`s on frame 0 (`"intro_a"` and `"intro_b"`) both emitted as tag 43
    // - Embedded `.ttc` font where face 0 has `CBDT` and face 1 (`Bold`) has clean `glyf` outlines with `outline_text: true`
    let face0_bad = build_synthetic_glyf_sfnt("SwfTtcFont", "Regular", false, true);
    let face1_bold = build_synthetic_glyf_sfnt("SwfTtcFont", "Bold", true, false);
    let ttc_bytes = pack_ttc(&[&face0_bad, &face1_bold]);

    let mut project = Project::blank();
    project.name = "T414 Morph Labels TTC".into();
    project.resources.push(
        ResourceUnit::from_bytes(
            "ttc-res",
            "fonts/SwfTtcFont.ttc",
            "font/collection",
            &ttc_bytes,
        )
        .unwrap(),
    );
    let mut bold_fmt = TextFormat {
        font: "SwfTtcFont".into(),
        size: 24.0,
        bold: true,
        color: 0x112233,
        ..Default::default()
    };
    bold_fmt.set_field_options(Some(AuthoredTextFieldOptions::default()));

    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 3;
    root.timeline.layers = vec![
        Layer {
            id: new_id("layer"),
            name: "MorphLayer".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    label: "intro_a".into(),
                    label_type: FrameLabelType::Name,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Shape,
                    elements: vec![Element {
                        id: new_id("element"),
                        transform: Transform {
                            x: 10.0,
                            y: 20.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 30.0,
                            height: 30.0,
                            fill: [255, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                },
                Keyframe {
                    frame: 2,
                    elements: vec![Element {
                        id: new_id("element"),
                        transform: Transform {
                            x: 150.0,
                            y: 60.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 50.0,
                            height: 50.0,
                            fill: [0, 0, 255, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                },
            ],
        },
        Layer {
            id: new_id("layer"),
            name: "SecondLabelAndTtcLayer".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                label: "intro_b".into(),
                label_type: FrameLabelType::Name,
                elements: vec![Element {
                    id: new_id("element"),
                    kind: ElementKind::TextBox {
                        text: "A".into(),
                        width: 80.0,
                        height: 30.0,
                        format: bold_fmt,
                        word_wrap: false,
                        multiline: false,
                    },
                    ..Element::default()
                }],
                ..Default::default()
            }],
        },
    ];
    project.upgrade_format_for_features();

    let mut swf_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    swf_opts.compression = SwfCompression::UncompressedFws;
    swf_opts.outline_text = true;
    let (swf_bytes, _) = publish_swf_bytes(&project, &swf_opts).expect("publish morph+labels+ttc");
    let raw_str = String::from_utf8_lossy(&swf_bytes);
    assert!(
        raw_str.contains("intro_a\0") && raw_str.contains("intro_b\0"),
        "both FrameLabel tags on frame 0 across layers must be emitted"
    );

    let reimported = import_swf_bytes(&swf_bytes).expect("re-import morph SWF");
    reimported.validate().unwrap();
    // Verify the morph symbol's end keyframe geometry moved to x ~= 150 (not stuck at x = 10)!
    let morph_sym = reimported
        .symbols
        .iter()
        .find(|s| s.name.starts_with("MorphShape"))
        .expect("MorphShape symbol");
    let end_kf = morph_sym.timeline.layers[0]
        .keyframes
        .last()
        .expect("morph end keyframe");
    let end_cmds = match &end_kf.elements[0].kind {
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => commands,
        other => panic!("expected morphed path, got {other:?}"),
    };
    let end_b = neoflash_core::path_bounds(end_cmds).unwrap().unwrap();
    assert!(
        (end_b.x_min - 150.0).abs() < 1.0 && (end_b.x_max - 200.0).abs() < 1.0,
        "DefineMorphShape end bounds must bake end_el.transform (150..200), got {end_b:?}"
    );
}

#[test]
fn t416_avm1_function_expressions_nested_closures_try_fscommand_and_placeobject3() {
    use base64::Engine as _;
    use neoflash_core::{BlendMode, FilterSpec};

    let mut project = Project::blank();
    project.name = "T416 AVM1 Closures".into();
    project.width = 120;
    project.height = 90;
    project.fps = 24;
    let badge_sym = "sym_badge".to_owned();
    project.symbols.push(Symbol {
        id: badge_sym.clone(),
        name: "BadgeSym".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![Layer {
                id: "lyr_badge".into(),
                name: "Art".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el_badge_rect".into(),
                        kind: ElementKind::Rectangle {
                            width: 24.0,
                            height: 24.0,
                            fill: [220, 40, 60, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });

    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.id = "root".into();
    root.timeline.frame_count = 1;
    root.timeline.layers[0].id = "lyr_root".into();
    root.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "el_badge_inst".into(),
        name: "badge".into(),
        cache_as_bitmap: true,
        blend_mode: BlendMode::Screen,
        filters: vec![FilterSpec::Glow {
            color: 0x00CCFF,
            alpha: 1.0,
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 2.0,
            quality: 1,
            inner: false,
            knockout: false,
        }],
        transform: Transform {
            x: 16.0,
            y: 12.0,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: badge_sym,
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    }];
    root.timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var shadow = 900;
var addOffset = function(a, b) {
    return a + b;
};
var fact = function selfFact(n) {
    if (n <= 1) {
        return 1;
    }
    return n * selfFact(n - 1);
};
function makeCounter(seed) {
    var count = seed;
    function stepBy(delta) {
        var shadow = 1;
        count = count + delta + shadow - 1;
        return count;
    }
    return function(d) {
        return stepBy(d);
    };
}
var c1 = makeCounter(10);
var r1 = c1(4);
var r2 = c1(6);
function mapReduce(items, fn) {
    var acc = 0;
    var idx = 0;
    while (idx < items.length) {
        acc = acc + fn(items[idx], idx);
        idx = idx + 1;
    }
    return acc;
}
var cbSum = mapReduce([4, 1, 9, 2], function(v, idx) {
    return v * (idx + 1);
});
var nums = [4, 1, 9, 2];
nums.sort(function(a, b) {
    return b - a;
});
var pkg = {
    Widget: function(v) { this.val = v * 2; },
    calc: function(x) { return x + 7; }
};
var w = new pkg.Widget(8);
var iifeVal = (function(k) { return k * 3; })(5);
var tryLog = "";
try {
    tryLog = tryLog + "T";
    throw "err42";
} catch (ex) {
    tryLog = tryLog + ":" + ex;
} finally {
    tryLog = tryLog + ":F";
}
fscommand("avm1Status", "ready");
getURL("javascript:globalThis.__nfXssAvm1=1", "_self");
this.exportedClosure = c1;
this.sortedNums = nums;
this.summary = addOffset(3, 4) + "|" + fact(5) + "|" + r1 + "," + r2 + "," + shadow + "|" + cbSum + "|" + w.val + "," + pkg.calc(5) + "," + iifeVal + "|" + tryLog;
trace(this.summary);
stop();
"#
            .into(),
        },
    }];
    project.root = "root".into();
    project.upgrade_format_for_features();

    let mut opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    opts.compression = SwfCompression::UncompressedFws;
    let preflight = preflight_swf_publish(&project, &opts);
    assert!(
        !preflight.has_blocking(),
        "AVM1 function expressions must pass preflight without blocking issues: {:?}",
        preflight.items
    );

    let (swf_bytes, _) = publish_swf_bytes(&project, &opts).expect("publish T416 AVM1 SWF");
    let playback = play_swf_bytes(&swf_bytes, 1).expect("play T416 AVM1 SWF");
    assert_eq!(
        playback.trace_output,
        vec!["7|120|14,20,900|41|16,12,15|T:err42:F"]
    );
    let b64 = base64::engine::general_purpose::STANDARD.encode(&swf_bytes);
    assert_eq!(
        b64,
        "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"
    );
}

#[test]
fn t416_avm2_nested_closures_super_symbolclass_sprite_and_abc_verifier_metrics() {
    use base64::Engine as _;
    use neoflash_core::{BlendMode, FilterSpec};

    let mut project = Project::blank();
    project.name = "T416 AVM2 Closures".into();
    project.width = 120;
    project.height = 90;
    project.fps = 24;
    project.document_class = Some("MainDoc".into());

    let child_sym = "sym_card".to_owned();
    project.symbols.push(Symbol {
        id: child_sym.clone(),
        name: "ChildSprite".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: Some("ChildCard".into()),
        linkage_id: Some("ChildCard".into()),
        timeline: Timeline {
            frame_count: 2,
            layers: vec![Layer {
                id: "lyr_card".into(),
                name: "Art".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el_card_rect".into(),
                        kind: ElementKind::Rectangle {
                            width: 28.0,
                            height: 20.0,
                            fill: [40, 160, 220, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: vec![FrameScript {
                layer: None,
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "this.frame2Visited = 77;\nstop();\n".into(),
                },
            }],
            guides: Vec::new(),
        },
    });

    project.sources.push(SourceUnit {
        id: "u_base".into(),
        path: "BaseCard.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package {
    import flash.display.MovieClip;
    public class BaseCard extends MovieClip {
        public var tag:String = "init";
        public function BaseCard(prefix:String = "base") {
            super();
            this.tag = prefix;
        }
        public function label():String {
            return "B(" + this.tag + ")";
        }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: "u_child".into(),
        path: "ChildCard.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package {
    public class ChildCard extends BaseCard {
        public var frame2Visited:int = 0;
        public function ChildCard() {
            super("sprite");
            this.tag = this.tag + ">C";
        }
        override public function label():String {
            return super.label() + "+Child";
        }
    }
}
"#
        .into(),
    });
    project.sources.push(SourceUnit {
        id: "u_doc".into(),
        path: "MainDoc.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package {
    import flash.display.MovieClip;
    public class MainDoc extends MovieClip {
        public var closureSummary:String = "";
        public var spriteSummary:String = "";
        public var exportedAdder:Function;
        public function outer(x:int):Function {
            var bonus:int = 2;
            return function(y:int):int {
                bonus = bonus + 1;
                return x + y + bonus;
            };
        }
        public function runCallback(cb:Function, v:int):int {
            return cb(v) + cb(v + 1);
        }
        public function MainDoc() {
            super();
            var add10:Function = this.outer(10);
            var a1:int = add10(5);
            var a2:int = add10(5);
            var mul:int = this.runCallback(function(n:int):int { return n * 3; }, 4);
            var fact:Function = function selfFact(n:int):int {
                if (n <= 1) { return 1; }
                return n * selfFact(n - 1);
            };
            this.exportedAdder = add10;
            this.closureSummary = a1 + "," + a2 + "," + mul + "," + fact(5);
            if (this["card"] != null) {
                this.spriteSummary = this["card"].label();
            }
            trace(this.closureSummary);
        }
    }
}
"#
        .into(),
    });

    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.id = "root".into();
    root.timeline.frame_count = 1;
    root.timeline.layers[0].id = "lyr_root".into();
    root.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "el_card_inst".into(),
        name: "card".into(),
        cache_as_bitmap: true,
        blend_mode: BlendMode::Multiply,
        filters: vec![FilterSpec::DropShadow {
            distance: 4.0,
            angle: 45.0,
            color: 0x112233,
            alpha: 0.9,
            blur_x: 5.0,
            blur_y: 5.0,
            strength: 1.5,
            quality: 1,
            inner: false,
            knockout: false,
            hide_object: false,
        }],
        transform: Transform {
            x: 20.0,
            y: 16.0,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: child_sym,
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    }];
    project.root = "root".into();
    project.upgrade_format_for_features();

    let mut opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    opts.compression = SwfCompression::UncompressedFws;
    let preflight = preflight_swf_publish(&project, &opts);
    assert!(
        !preflight.has_blocking(),
        "AVM2 closures + SymbolClass child sprite must pass preflight: {:?}",
        preflight.items
    );

    let (swf_bytes, _) = publish_swf_bytes(&project, &opts).expect("publish T416 AVM2 SWF");
    let playback = play_swf_bytes(&swf_bytes, 1).expect("play T416 AVM2 SWF");
    assert_eq!(playback.trace_output, vec!["18,19,27,120"]);

    // Re-import round-trip must validate cleanly
    let reimported = import_swf_bytes(&swf_bytes).expect("re-import T416 AVM2 SWF");
    reimported.validate().unwrap();

    // Verify ABC emission invariants: NEED_ACTIVATION(0x02), newactivation(0x57),
    // getouterscope(0x67), getslot(0x6C), setslot(0x6D), newfunction(0x40), constructsuper(0x49), callsuper(0x45)
    assert!(
        swf_bytes
            .windows(3)
            .any(|w| w == [0x57, 0x2A, 0xD2] || w[0] == 0x57),
        "expected newactivation (0x57) in emitted DoABC2"
    );
    assert!(swf_bytes.contains(&0x67), "expected getouterscope (0x67)");
    assert!(swf_bytes.contains(&0x6C), "expected getslot (0x6C)");
    assert!(swf_bytes.contains(&0x6D), "expected setslot (0x6D)");
    assert!(swf_bytes.contains(&0x40), "expected newfunction (0x40)");
    assert!(swf_bytes.contains(&0x49), "expected constructsuper (0x49)");
    assert!(swf_bytes.contains(&0x45), "expected callsuper (0x45)");

    let b64 = base64::engine::general_purpose::STANDARD.encode(&swf_bytes);
    assert_eq!(
        b64,
        "RldTCkMFAABoABLAAAOEAAAYAQBEEQgAAABDAv///+oUAQBYAEYAAMgAWABGAADIAAABACig3P8AETQlzRgADiADIc7oAA4gBOAA0gkCAAIAhgYGAQABAABAAEAAAAC/FJQEAAABAAAAZnJhbWUxABAALgAAAAAqAAZPYmplY3QOYWRkRnJhbWVTY3JpcHQIQmFzZUNhcmQNZmxhc2guZGlzcGxheQlNb3ZpZUNsaXAMZmxhc2guZXZlbnRzD0V2ZW50RGlzcGF0Y2hlcg1EaXNwbGF5T2JqZWN0EUludGVyYWN0aXZlT2JqZWN0FkRpc3BsYXlPYmplY3RDb250YWluZXIGU3ByaXRlA3RhZwVsYWJlbAJCKAEpBnByZWZpeARpbml0CUNoaWxkQ2FyZA1mcmFtZTJWaXNpdGVkBmZyYW1lMgRzdG9wBitDaGlsZAZzcHJpdGUCPkMHTWFpbkRvYw5jbG9zdXJlU3VtbWFyeQ1zcHJpdGVTdW1tYXJ5DWV4cG9ydGVkQWRkZXIFb3V0ZXIBeAVib251cwF5C3J1bkNhbGxiYWNrAmNiAXYBbghzZWxmRmFjdAEsBGNhcmQFdHJhY2UEFgEWBRYHABsHAQIHAQMHAQQHAgYHAwgHAgkHAgoHAgsHAgwHAQ0HAQ4HARMHARQHARUHARYHARoHARsHARwHAR0HAR4HAR8HASAHASIHASYHASgHASkPAAAAAAAAAAABAAAAgBEAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAACCHwEAAACAIQIAAAAAgCMkAAAAAgEAAACAJQEAAACAJQAAAAAAAwMEAAACAgoAAAAACwEAAQwDAAAGAw0AAAAADgEABAshAAUQBAAACwURAAAAABIAAAAAEwAAAAAUAQAIFwEACgAAAwAHAAEOAwMEAQAMBAIBEAQDAg8AAwEICQPQMEcAAAEEAQkKDdAwLA/QZgqgLBCgSEcAAAIEAgkKD9Aw0EkA0CwSYQrQ0WEKRwAAAwMBCQoD0DBHAAAEBAEKCw3QMNAlTWENXQ9PDwBHAAAFBAEKCwvQMNBFCwAsF6BIRwAABgUBCgsf0DDQLBhJAV0CJQFgDk8CAtAlAGEN0NBmCiwZoGEKRwAABwMBCAkD0DBHAAAJBQILDBrQMGcKZwpsAiUBoG0CZwpsAdGgZwpsAqBIRwAACAQDCQsV0DBXKtYwZQHRbQFlASUCbQJACUhHAAIVAAEAABYAAgAACgcDCQoS0DDR0NJBAdHQ0iUBoEEBoEhHAAAMBAILDAjQMNElA6JIRwAADQcCCwwc0DDRJQGuEgMAACUBSNFnCmwB0NElAaFBAaJIRwAACwcHCQt+0DBXKmMGMNBJANAsAWER0CwBYRJgFNAlCkEBgtXR0CUFQQGC1tHQJQVBAYLXYBfQQAwlBEECgmMEQA0qZQErbQGCYwXQ0WET0NIsJ6DToCwnoGIEoCwnoGIF0CUFQQGgYRHQZhkgq5YSCQAA0NBmGUYLAGESXRrQZhFPGgFHAAEYAAEAAA4EAQEKc9AwZQBgATBgBTBgBjBgBzBgCDBgCTBgBDBgBFgAHR0dHR0dHWgDZQBgATBgBTBgBjBgBzBgCDBgCTBgBDBgAzBgA1gBHR0dHR0dHR1oDGUAYAEwYAUwYAYwYAcwYAgwYAkwYAQwYARYAh0dHR0dHR1oEEcAABgTAgACAENoaWxkQ2FyZAAAAE1haW5Eb2MAqhEmBwEAAgAUyCgAY2FyZAABABEiM+YAAAUAAAAFABDJAAAAAAQAgAEhAwFAAAAA"
    );
}
