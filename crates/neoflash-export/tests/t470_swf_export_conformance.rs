//! `[T-470]` SWF Export Conformance & Round-Trip Verification Suite:
//! 1. FP8 AVM1 (`ActionTry` `0x8F` + `ActionThrow` `0x2A` + `ActionInstanceOf` `0x54`) compilation,
//!    raw bytecode opcode verification, `SwfBytecodePlayer` execution, and `import_project` round-trip.
//! 2. FP9 & FP10 AVM2 (`METHOD_BODY_INFO` `exception` table + `throw` `0x03` + `newcatch` `0x5A` +
//!    `finally` unwinding across fallthrough/`return`/`break`/`continue`/`throw` + nested catch scopes +
//!    `instanceof` `0xB1` + static `istype` `0xB2` / dynamic `istypelate` `0xB3` + static `astype` `0x86` /
//!    dynamic `astypelate` `0x87`) compilation, ABC bytecode inspection, `SwfBytecodePlayer` execution,
//!    and `import_project` round-trip.
//! 3. Mid-timeline starting and ending `LayerRole::Mask` layers (`clip_depth` placed only on active mask
//!    frames and removed via `RemoveObject2` on inactive frames), verified against `CorePlayer`,
//!    `SwfBytecodePlayer` (forward + backward `seek_to_frame` + `restart`), and `import_project`.

use neoflash_core::{
    Element, ElementKind, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties,
    LayerRole, Player as CorePlayer, Project, Script, ScriptLanguage, SourceUnit, Symbol,
    SymbolKind, Timeline, Transform,
};
use neoflash_export::{
    SwfCompression, SwfPublishOptions, SwfPublishProfile, SwfVmKind, import_swf_bytes,
    play_swf_bytes, preflight_swf_publish, publish_swf_bytes,
};
use neoflash_import::SwfBytecodePlayer;

fn collect_uncompressed_swf_tags(swf_bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    assert_eq!(&swf_bytes[0..3], b"FWS", "expected uncompressed FWS header");
    // Skip 8-byte header + RECT + frame_rate(2) + frame_count(2)
    let nbits = (swf_bytes[8] >> 3) as usize;
    let rect_bits = 5 + nbits * 4;
    let rect_bytes = rect_bits.div_ceil(8);
    let mut pos = 8 + rect_bytes + 4;
    let mut out = Vec::new();
    while pos + 2 <= swf_bytes.len() {
        let hdr = u16::from_le_bytes([swf_bytes[pos], swf_bytes[pos + 1]]);
        pos += 2;
        let code = hdr >> 6;
        let mut len = usize::from(hdr & 0x3f);
        if len == 0x3f {
            assert!(pos + 4 <= swf_bytes.len());
            len = u32::from_le_bytes([
                swf_bytes[pos],
                swf_bytes[pos + 1],
                swf_bytes[pos + 2],
                swf_bytes[pos + 3],
            ]) as usize;
            pos += 4;
        }
        assert!(pos + len <= swf_bytes.len());
        out.push((code, swf_bytes[pos..pos + len].to_vec()));
        pos += len;
        if code == 0 {
            break;
        }
    }
    out
}

fn scan_avm1_action_opcodes(mut actions: &[u8], seen: &mut Vec<u8>) {
    while let Some(&op) = actions.first() {
        actions = &actions[1..];
        if op == 0 {
            break;
        }
        seen.push(op);
        if op >= 0x80 {
            if actions.len() < 2 {
                break;
            }
            let len = u16::from_le_bytes([actions[0], actions[1]]) as usize;
            actions = &actions[2..];
            if actions.len() < len {
                break;
            }
            let payload = &actions[..len];
            actions = &actions[len..];
            // Recurse into DefineFunction (0x9B), DefineFunction2 (0x8E), and ActionTry (0x8F) bodies,
            // whose bytecode streams follow immediately after their header record in `actions`!
            if matches!(op, 0x9B | 0x8E) && payload.len() >= 2 {
                let code_size =
                    u16::from_le_bytes([payload[payload.len() - 2], payload[payload.len() - 1]])
                        as usize;
                if actions.len() >= code_size {
                    scan_avm1_action_opcodes(&actions[..code_size], seen);
                    actions = &actions[code_size..];
                }
            } else if op == 0x8F && payload.len() >= 7 {
                let try_size = u16::from_le_bytes([payload[1], payload[2]]) as usize;
                let catch_size = u16::from_le_bytes([payload[3], payload[4]]) as usize;
                let fin_size = u16::from_le_bytes([payload[5], payload[6]]) as usize;
                let total_try = try_size + catch_size + fin_size;
                if actions.len() >= total_try {
                    scan_avm1_action_opcodes(&actions[..try_size], seen);
                    scan_avm1_action_opcodes(&actions[try_size..try_size + catch_size], seen);
                    scan_avm1_action_opcodes(&actions[try_size + catch_size..total_try], seen);
                    actions = &actions[total_try..];
                }
            }
        }
    }
}

fn read_u30(buf: &[u8], pos: &mut usize) -> u32 {
    let mut out = 0u32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let b = buf[*pos];
        *pos += 1;
        out |= u32::from(b & 0x7f) << shift;
        if (b & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    out
}

struct ParsedAbcBodySummary {
    init_scope_depth: u32,
    max_scope_depth: u32,
    exception_count: u32,
    opcodes: Vec<u8>,
}

fn extract_abc_method_bodies(abc: &[u8]) -> Vec<ParsedAbcBodySummary> {
    let mut pos = 4usize; // minor(2) + major(2)
    let int_cnt = read_u30(abc, &mut pos);
    for _ in 1..int_cnt {
        let _ = read_u30(abc, &mut pos);
    }
    let uint_cnt = read_u30(abc, &mut pos);
    for _ in 1..uint_cnt {
        let _ = read_u30(abc, &mut pos);
    }
    let dbl_cnt = read_u30(abc, &mut pos);
    if dbl_cnt > 1 {
        pos += ((dbl_cnt - 1) as usize) * 8;
    }
    let str_cnt = read_u30(abc, &mut pos);
    for _ in 1..str_cnt {
        let slen = read_u30(abc, &mut pos) as usize;
        pos += slen;
    }
    let ns_cnt = read_u30(abc, &mut pos);
    for _ in 1..ns_cnt {
        pos += 1;
        let _ = read_u30(abc, &mut pos);
    }
    let nss_cnt = read_u30(abc, &mut pos);
    for _ in 1..nss_cnt {
        let c = read_u30(abc, &mut pos);
        for _ in 0..c {
            let _ = read_u30(abc, &mut pos);
        }
    }
    let mut multiname_l_indices = std::collections::HashSet::new();
    let mn_cnt = read_u30(abc, &mut pos);
    for idx in 1..mn_cnt {
        let kind = abc[pos];
        pos += 1;
        match kind {
            0x07 | 0x0D => {
                let _ = read_u30(abc, &mut pos);
                let _ = read_u30(abc, &mut pos);
            }
            0x0F | 0x10 => {
                let _ = read_u30(abc, &mut pos);
            }
            0x11 | 0x12 => {}
            0x09 | 0x0E => {
                let _ = read_u30(abc, &mut pos);
                let _ = read_u30(abc, &mut pos);
            }
            0x1B | 0x1C => {
                let _ = read_u30(abc, &mut pos);
                multiname_l_indices.insert(idx);
            }
            0x1D => {
                let _ = read_u30(abc, &mut pos);
                let tc = read_u30(abc, &mut pos);
                for _ in 0..tc {
                    let _ = read_u30(abc, &mut pos);
                }
            }
            other => panic!("unexpected multiname kind 0x{other:02X}"),
        }
    }

    let method_cnt = read_u30(abc, &mut pos);
    for _ in 0..method_cnt {
        let param_cnt = read_u30(abc, &mut pos);
        let _ret_type = read_u30(abc, &mut pos);
        for _ in 0..param_cnt {
            let _ = read_u30(abc, &mut pos);
        }
        let _name = read_u30(abc, &mut pos);
        let flags = abc[pos];
        pos += 1;
        if (flags & 0x08) != 0 {
            let opt_cnt = read_u30(abc, &mut pos);
            for _ in 0..opt_cnt {
                let _ = read_u30(abc, &mut pos);
                pos += 1;
            }
        }
        if (flags & 0x80) != 0 {
            for _ in 0..param_cnt {
                let _ = read_u30(abc, &mut pos);
            }
        }
    }

    let meta_cnt = read_u30(abc, &mut pos);
    for _ in 0..meta_cnt {
        let _ = read_u30(abc, &mut pos);
        let ic = read_u30(abc, &mut pos);
        for _ in 0..ic {
            let _ = read_u30(abc, &mut pos);
            let _ = read_u30(abc, &mut pos);
        }
    }

    let skip_traits = |abc: &[u8], pos: &mut usize| {
        let tcnt = read_u30(abc, pos);
        for _ in 0..tcnt {
            let _name = read_u30(abc, pos);
            let kind_byte = abc[*pos];
            *pos += 1;
            let kind = kind_byte & 0x0f;
            let attr = kind_byte >> 4;
            match kind {
                0 | 6 => {
                    let _slot_id = read_u30(abc, pos);
                    let _type_name = read_u30(abc, pos);
                    let vindex = read_u30(abc, pos);
                    if vindex != 0 {
                        *pos += 1;
                    }
                }
                1..=3 | 5 => {
                    let _disp_id = read_u30(abc, pos);
                    let _method = read_u30(abc, pos);
                }
                4 => {
                    let _slot_id = read_u30(abc, pos);
                    let _classi = read_u30(abc, pos);
                }
                other => panic!("unexpected trait kind {other}"),
            }
            if (attr & 0x04) != 0 {
                let mc = read_u30(abc, pos);
                for _ in 0..mc {
                    let _ = read_u30(abc, pos);
                }
            }
        }
    };

    let class_cnt = read_u30(abc, &mut pos);
    for _ in 0..class_cnt {
        let _name = read_u30(abc, &mut pos);
        let _super = read_u30(abc, &mut pos);
        let flags = abc[pos];
        pos += 1;
        if (flags & 0x08) != 0 {
            let _ = read_u30(abc, &mut pos);
        }
        let intf_cnt = read_u30(abc, &mut pos);
        for _ in 0..intf_cnt {
            let _ = read_u30(abc, &mut pos);
        }
        let _iinit = read_u30(abc, &mut pos);
        skip_traits(abc, &mut pos);
    }
    for _ in 0..class_cnt {
        let _cinit = read_u30(abc, &mut pos);
        skip_traits(abc, &mut pos);
    }
    let script_cnt = read_u30(abc, &mut pos);
    for _ in 0..script_cnt {
        let _init = read_u30(abc, &mut pos);
        skip_traits(abc, &mut pos);
    }

    let body_cnt = read_u30(abc, &mut pos);
    let mut out = Vec::with_capacity(body_cnt as usize);
    for _ in 0..body_cnt {
        let _method = read_u30(abc, &mut pos);
        let _max_stack = read_u30(abc, &mut pos);
        let _local_cnt = read_u30(abc, &mut pos);
        let init_scope_depth = read_u30(abc, &mut pos);
        let max_scope_depth = read_u30(abc, &mut pos);
        let code_len = read_u30(abc, &mut pos) as usize;
        let code = &abc[pos..pos + code_len];
        pos += code_len;

        let mut opcodes = Vec::new();
        let mut cpos = 0usize;
        while cpos < code.len() {
            let op = code[cpos];
            cpos += 1;
            opcodes.push(op);
            match op {
                0x0C..=0x1A => cpos += 3,
                0x1B => {
                    cpos += 3;
                    let cnt = read_u30(code, &mut cpos) as usize;
                    cpos += (cnt + 1) * 3;
                }
                0x24 | 0x65 => cpos += 1,
                0x04..=0x06
                | 0x08
                | 0x25
                | 0x2C..=0x2F
                | 0x31
                | 0x40..=0x42
                | 0x49
                | 0x53
                | 0x55
                | 0x56
                | 0x58..=0x63
                | 0x66..=0x68
                | 0x6A
                | 0x6C..=0x6F
                | 0x80
                | 0x86
                | 0x92
                | 0x94
                | 0xB2
                | 0xC2
                | 0xC3
                | 0xF0..=0xF2 => {
                    let _ = read_u30(code, &mut cpos);
                }
                0x32 | 0x43..=0x46 | 0x4A | 0x4C | 0x4E | 0x4F => {
                    let _ = read_u30(code, &mut cpos);
                    let _ = read_u30(code, &mut cpos);
                }
                0xEF => {
                    cpos += 1;
                    let _ = read_u30(code, &mut cpos);
                    cpos += 1;
                    let _ = read_u30(code, &mut cpos);
                }
                _ => {}
            }
        }

        let exception_count = read_u30(abc, &mut pos);
        for _ in 0..exception_count {
            let from = read_u30(abc, &mut pos);
            let to = read_u30(abc, &mut pos);
            let target = read_u30(abc, &mut pos);
            let _exc_type = read_u30(abc, &mut pos);
            let _var_name = read_u30(abc, &mut pos);
            assert!(
                from < to && target >= to && (target as usize) < code.len(),
                "verifier-valid ABC exception bounds required: from={from}, to={to}, target={target}, code_len={}",
                code.len()
            );
        }
        skip_traits(abc, &mut pos);
        out.push(ParsedAbcBodySummary {
            init_scope_depth,
            max_scope_depth,
            exception_count,
            opcodes,
        });
    }
    out
}

#[test]
fn t470_avm1_try_catch_finally_throw_and_instanceof_fp8_roundtrip() {
    let mut project = Project::blank();
    project.name = "T470 FP8 AVM1 Try Throw InstanceOf".to_owned();
    project.sources.push(SourceUnit {
        id: "src_base".to_owned(),
        path: "model/BaseItem.as".to_owned(),
        language: ScriptLanguage::ActionScript2,
        source: r#"
class model.BaseItem {
    var kind:String;
    function BaseItem(k:String) {
        this.kind = k;
    }
}
"#
        .to_owned(),
    });
    project.sources.push(SourceUnit {
        id: "src_special".to_owned(),
        path: "model/SpecialItem.as".to_owned(),
        language: ScriptLanguage::ActionScript2,
        source: r#"
class model.SpecialItem extends model.BaseItem {
    function SpecialItem(k:String) {
        super("special:" + k);
    }
}
"#
        .to_owned(),
    });

    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var log = [];
function runReturnFromTry(shouldThrow) {
    try {
        log.push("try-start");
        if (shouldThrow) {
            throw "boom-from-try";
        }
        return "ret-ok";
    } catch (err) {
        log.push("caught:" + err);
        return "ret-caught:" + err;
    } finally {
        log.push("fin-once");
    }
}

var r1 = runReturnFromTry(false);
var r2 = runReturnFromTry(true);

for (var i = 0; i < 3; i = i + 1) {
    try {
        if (i == 1) {
            continue;
        }
        if (i == 2) {
            break;
        }
        log.push("loop:" + i);
    } finally {
        log.push("loop-fin:" + i);
    }
}

try {
    try {
        throw "inner-err";
    } finally {
        log.push("inner-fin");
    }
} catch (outerErr) {
    log.push("outer-catch:" + outerErr);
}

function InlineBase(label) {
    this.label = label;
}
function InlineChild(label) {
    this.label = "child:" + label;
}
InlineChild.prototype = new InlineBase("");

var sp = new SpecialItem("alpha");
var bs = new BaseItem("beta");
var ic = new InlineChild("gamma");
var ib = new InlineBase("delta");
var classFlags = [
    sp instanceof SpecialItem,
    sp instanceof BaseItem,
    bs instanceof SpecialItem
];
var protoFlags = [
    ic instanceof InlineChild,
    ic instanceof InlineBase,
    ic instanceof Object,
    ib instanceof InlineChild,
    log instanceof Array
];
trace(r1 + "|" + r2);
trace(log.join(","));
trace(ic.label + "|" + protoFlags.join(","));
if (sp.kind != undefined) {
    trace(sp.kind + "|" + classFlags.join(","));
}
stop();
"#
            .to_owned(),
        },
    }];

    let opts = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer8Avm1,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };
    let preflight = preflight_swf_publish(&project, &opts);
    assert!(
        preflight.can_proceed(),
        "FP8 AVM1 preflight blocked: {:?}",
        preflight.blocking_items()
    );
    let (swf_bytes, report) = publish_swf_bytes(&project, &opts).expect("publish FP8 AVM1 SWF");
    assert!(report.can_proceed());

    // Inspect raw AVM1 bytecode in DoAction (12) / DoInitAction (59) to confirm 0x8F, 0x2A, 0x54!
    let tags = collect_uncompressed_swf_tags(&swf_bytes);
    let mut seen_ops = Vec::new();
    for (code, payload) in &tags {
        if *code == 12 {
            scan_avm1_action_opcodes(payload, &mut seen_ops);
        } else if *code == 59 && payload.len() > 2 {
            scan_avm1_action_opcodes(&payload[2..], &mut seen_ops);
        }
    }
    assert!(
        seen_ops.contains(&0x8F),
        "expected ActionTry (0x8F) in FP8 AVM1 bytecode"
    );
    assert!(
        seen_ops.contains(&0x2A),
        "expected ActionThrow (0x2A) in FP8 AVM1 bytecode"
    );
    assert!(
        seen_ops.contains(&0x54),
        "expected ActionInstanceOf (0x54) in FP8 AVM1 bytecode"
    );

    let expected_common_traces = vec![
        "ret-ok|ret-caught:boom-from-try".to_owned(),
        "try-start,fin-once,try-start,caught:boom-from-try,fin-once,loop:0,loop-fin:0,loop-fin:1,loop-fin:2,inner-fin,outer-catch:inner-err".to_owned(),
        "child:gamma|true,true,true,false,true".to_owned(),
    ];
    let mut expected_initial_traces = expected_common_traces.clone();
    expected_initial_traces.push("special:alpha|true,true,false".to_owned());

    let play = play_swf_bytes(&swf_bytes, 1).expect("play FP8 AVM1 SWF");
    assert_eq!(play.vm_kind, SwfVmKind::Avm1);
    assert_eq!(play.trace_output, expected_initial_traces);

    // Round-trip through neoflash_import::import_project -> validate -> re-export -> play!
    let reimported = import_swf_bytes(&swf_bytes).expect("re-import FP8 AVM1 SWF");
    reimported
        .validate()
        .expect("re-imported FP8 AVM1 project must validate");
    let (re_swf_bytes, _) =
        publish_swf_bytes(&reimported, &opts).expect("re-publish re-imported FP8 AVM1 SWF");
    let replay = play_swf_bytes(&re_swf_bytes, 1).expect("replay re-exported FP8 AVM1 SWF");
    assert_eq!(&replay.trace_output[..3], expected_common_traces.as_slice());
}

#[test]
fn t470_avm2_exception_table_newcatch_finally_and_type_operators_fp9_fp10_roundtrip() {
    let mut project = Project::blank();
    project.name = "T470 FP9 FP10 AVM2 Exceptions & Type Ops".to_owned();
    project.sources.push(SourceUnit {
        id: "src_base_node".to_owned(),
        path: "pkg/BaseNode.as".to_owned(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    public class BaseNode {
        public var label:String;
        public function BaseNode(lbl:String) {
            this.label = lbl;
        }
    }
}
"#
        .to_owned(),
    });
    project.sources.push(SourceUnit {
        id: "src_leaf_node".to_owned(),
        path: "pkg/LeafNode.as".to_owned(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    public class LeafNode extends BaseNode {
        public function LeafNode(lbl:String) {
            super("leaf:" + lbl);
        }
    }
}
"#
        .to_owned(),
    });
    project.sources.push(SourceUnit {
        id: "src_custom_fault".to_owned(),
        path: "pkg/CustomFault.as".to_owned(),
        language: ScriptLanguage::ActionScript3,
        source: r#"
package pkg {
    public class CustomFault extends Error {
        public function CustomFault(msg:String) {
            super(msg);
        }
    }
}
"#
        .to_owned(),
    });

    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"
import pkg.BaseNode;
import pkg.LeafNode;
import pkg.CustomFault;

var events:Array = [];
function runTypedTry(mode:int):String {
    try {
        try {
            if (mode == 0) {
                return "ok";
            }
            if (mode == 1) {
                throw new TypeError("bad-type");
            }
            if (mode == 2) {
                throw new CustomFault("custom-boom");
            }
            throw "raw-str";
        } catch (te:TypeError) {
            try {
                throw "nested-in-catch";
            } catch (ne:*) {
                events.push("nested:" + ne);
            }
            return "TE:" + te.message;
        } catch (cf:CustomFault) {
            var readCaught = function():String { return cf.message; };
            return "CF:" + readCaught();
        } catch (anyVal:*) {
            return "ANY:" + anyVal;
        }
    } finally {
        events.push("fin:" + mode);
    }
    return "unreachable";
}

var res:Array = [
    runTypedTry(0),
    runTypedTry(1),
    runTypedTry(2),
    runTypedTry(3)
];

for (var k:int = 0; k < 2; k = k + 1) {
    try {
        throw "loop-err-" + k;
    } catch (le:*) {
        events.push("lc:" + le);
        if (k == 0) {
            continue;
        }
        break;
    } finally {
        events.push("lf:" + k);
    }
}

var leaf:LeafNode = new LeafNode("n1");
var base:BaseNode = new BaseNode("b0");
var dynBase:* = BaseNode;
var dynLeaf:* = LeafNode;
var staticChecks:Array = [
    leaf instanceof LeafNode,
    leaf instanceof BaseNode,
    base instanceof LeafNode,
    leaf is LeafNode,
    leaf is BaseNode,
    base is LeafNode,
    42 is int,
    -5 is uint,
    leaf is *,
    (leaf as BaseNode) === leaf,
    (base as LeafNode) === null
];
var dynamicChecks:Array = [
    leaf is dynBase,
    base is dynLeaf,
    (leaf as dynBase) === leaf,
    (base as dynLeaf) === null
];

trace(res.join("|"));
trace(events.join(","));
trace(staticChecks.join(","));
trace(dynamicChecks.join(","));
stop();
"#
            .to_owned(),
        },
    }];

    let expected_traces = vec![
        "ok|TE:bad-type|CF:custom-boom|ANY:raw-str".to_owned(),
        "fin:0,nested:nested-in-catch,fin:1,fin:2,fin:3,lc:loop-err-0,lf:0,lc:loop-err-1,lf:1"
            .to_owned(),
        "true,true,false,true,true,false,true,false,true,true,true".to_owned(),
        "true,false,true,true".to_owned(),
    ];

    for profile in [
        SwfPublishProfile::FlashPlayer9Avm2,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        let opts = SwfPublishOptions {
            profile,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let preflight = preflight_swf_publish(&project, &opts);
        assert!(
            preflight.can_proceed(),
            "{profile:?} preflight blocked: {:?}",
            preflight.blocking_items()
        );
        let (swf_bytes, report) = publish_swf_bytes(&project, &opts).expect("publish AVM2 SWF");
        assert!(report.can_proceed());

        // Parse DoABC2 tag (82) and verify verifier-valid exception entries, scope depths, and exact opcodes!
        let tags = collect_uncompressed_swf_tags(&swf_bytes);
        let abc_payload = tags
            .iter()
            .find_map(|(code, p)| (*code == 82).then_some(p.as_slice()))
            .expect("DoABC2 tag 82");
        let name_end = abc_payload[4..]
            .iter()
            .position(|&b| b == 0)
            .expect("DoABC2 name NUL");
        let abc_bytes = &abc_payload[4 + name_end + 1..];
        let bodies = extract_abc_method_bodies(abc_bytes);
        let all_ops: std::collections::HashSet<u8> = bodies
            .iter()
            .flat_map(|b| b.opcodes.iter().copied())
            .collect();
        let total_exceptions: u32 = bodies.iter().map(|b| b.exception_count).sum();
        assert!(
            total_exceptions >= 5,
            "expected METHOD_BODY_INFO exception entries in {profile:?}, got {total_exceptions}"
        );
        for (op, name) in [
            (0x03u8, "throw"),
            (0x5Au8, "newcatch"),
            (0xB1u8, "instanceof"),
            (0xB2u8, "istype"),
            (0xB3u8, "istypelate"),
            (0x86u8, "astype"),
            (0x87u8, "astypelate"),
        ] {
            assert!(
                all_ops.contains(&op),
                "expected AVM2 opcode 0x{op:02X} ({name}) in {profile:?} DoABC2"
            );
        }
        assert!(
            bodies
                .iter()
                .filter(|b| b.opcodes.contains(&0x5A))
                .all(|b| b.max_scope_depth >= b.init_scope_depth + 2),
            "every method emitting newcatch (0x5A) must account for pushed catch scopes in max_scope_depth"
        );

        let play = play_swf_bytes(&swf_bytes, 1).expect("play AVM2 SWF");
        assert_eq!(play.vm_kind, SwfVmKind::Avm2);
        assert_eq!(play.trace_output, expected_traces);

        // Round-trip through neoflash_import::import_project -> validate -> re-publish -> play!
        let reimported = import_swf_bytes(&swf_bytes).expect("re-import AVM2 SWF");
        reimported
            .validate()
            .expect("re-imported AVM2 project must validate");
        let (re_swf_bytes, _) = publish_swf_bytes(&reimported, &opts).expect("re-publish AVM2 SWF");
        let replay = play_swf_bytes(&re_swf_bytes, 1).expect("replay re-exported AVM2 SWF");
        assert_eq!(replay.trace_output, expected_traces);
    }
}

fn read_place_object2_clip_depth(payload: &[u8]) -> (u16, Option<u16>) {
    let flags = payload[0];
    let depth = u16::from_le_bytes([payload[1], payload[2]]);
    if (flags & 0x40) == 0 {
        return (depth, None);
    }
    let mut bit_pos = 24usize;
    let align_byte = |bp: &mut usize| {
        if !(*bp).is_multiple_of(8) {
            *bp = (*bp + 7) & !7;
        }
    };
    let read_ub = |buf: &[u8], bp: &mut usize, n: usize| -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            let b = (buf[*bp / 8] >> (7 - (*bp % 8))) & 1;
            v = (v << 1) | u32::from(b);
            *bp += 1;
        }
        v
    };
    if (flags & 0x02) != 0 {
        bit_pos += 16; // character_id
    }
    if (flags & 0x04) != 0 {
        align_byte(&mut bit_pos);
        if read_ub(payload, &mut bit_pos, 1) == 1 {
            let sb = read_ub(payload, &mut bit_pos, 5) as usize;
            bit_pos += sb * 2;
        }
        if read_ub(payload, &mut bit_pos, 1) == 1 {
            let rb = read_ub(payload, &mut bit_pos, 5) as usize;
            bit_pos += rb * 2;
        }
        let tb = read_ub(payload, &mut bit_pos, 5) as usize;
        bit_pos += tb * 2;
        align_byte(&mut bit_pos);
    }
    if (flags & 0x08) != 0 {
        align_byte(&mut bit_pos);
        let has_add = read_ub(payload, &mut bit_pos, 1) == 1;
        let has_mult = read_ub(payload, &mut bit_pos, 1) == 1;
        let nbits = read_ub(payload, &mut bit_pos, 4) as usize;
        if has_mult {
            bit_pos += nbits * 4;
        }
        if has_add {
            bit_pos += nbits * 4;
        }
        align_byte(&mut bit_pos);
    }
    if (flags & 0x10) != 0 {
        align_byte(&mut bit_pos);
        bit_pos += 16; // ratio
    }
    if (flags & 0x20) != 0 {
        align_byte(&mut bit_pos);
        let mut byte_idx = bit_pos / 8;
        while payload[byte_idx] != 0 {
            byte_idx += 1;
        }
        bit_pos = (byte_idx + 1) * 8;
    }
    align_byte(&mut bit_pos);
    let b = bit_pos / 8;
    let cd = u16::from_le_bytes([payload[b], payload[b + 1]]);
    (depth, Some(cd))
}

fn render_swf_player_rgba(
    swf_player: &SwfBytecodePlayer,
    base_project: &Project,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let f = swf_player.presented_frame();
    let overlay = swf_player.project_with_display_list_overlay(base_project, &base_project.root, f);
    let mut p = CorePlayer::new(&overlay);
    p.seek(&overlay, f);
    p.render_snapshot_rgba(&overlay, width, height)
        .expect("render SwfBytecodePlayer overlay")
}

fn sample_pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let off = ((y * width + x) * 4) as usize;
    [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
}

#[test]
fn t470_mid_timeline_starting_and_ending_mask_layers_swf_export_and_roundtrip() {
    let mut project = Project::blank();
    project.name = "T470 Mid-Timeline Mask Export".to_owned();
    project.width = 120;
    project.height = 80;
    project.background = [20, 24, 32, 255];

    // Nested MovieClip with its own mid-timeline mask active ONLY on frames 1..3 of a 4-frame loop!
    let badge_sym = Symbol {
        id: "sym_badge".to_owned(),
        name: "MaskedBadge".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![
                Layer {
                    id: "badge_masked".to_owned(),
                    name: "BadgeFill".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        mask: Some("badge_mask".to_owned()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "badge_green".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 20.0,
                                fill: [0, 220, 90, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "badge_mask".to_owned(),
                    name: "BadgeMask".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        end_frame: Some(3),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            elements: Vec::new(), // Inactive on frame 0!
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 1,
                            elements: vec![Element {
                                id: "badge_stencil".to_owned(),
                                kind: ElementKind::Rectangle {
                                    width: 20.0,
                                    height: 20.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };
    project.symbols.push(badge_sym);

    let root_id = project.root.clone();
    {
        let root_sym = project.symbol_mut(&root_id).expect("root symbol");
        root_sym.timeline.frame_count = 6;
        root_sym.timeline.layers = vec![
            // Layer 0 (bottom): Masked full-width red bar (x: 0..100, y: 0..40) across all 6 frames,
            // linked to `root_mask` which is ONLY active on frames 2..4!
            Layer {
                id: "root_masked".to_owned(),
                name: "RedMaskedBar".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    mask: Some("root_mask".to_owned()),
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "red_bar".to_owned(),
                        kind: ElementKind::Rectangle {
                            width: 100.0,
                            height: 40.0,
                            fill: [230, 30, 40, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 1: Mid-timeline Mask layer — blank on frames 0..2, left-half stencil (x: 0..50, y: 0..40)
            // on frames 2..4, ended at frame 4 (`end_frame: Some(4)` -> inactive on frames 4..6)!
            Layer {
                id: "root_mask".to_owned(),
                name: "MidSpanMask".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    end_frame: Some(4),
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: Vec::new(),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 2,
                        elements: vec![Element {
                            id: "left_stencil".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 50.0,
                                height: 40.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 2 (top, unmasked): Nested MaskedBadge instance at (0, 50)
            Layer {
                id: "root_badge".to_owned(),
                name: "BadgeLayer".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "badge_inst".to_owned(),
                        name: "badge".to_owned(),
                        transform: Transform {
                            x: 0.0,
                            y: 50.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: "sym_badge".to_owned(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
        ];
    }

    for profile in [
        SwfPublishProfile::FlashPlayer8Avm1,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        let opts = SwfPublishOptions {
            profile,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let (swf_bytes, report) =
            publish_swf_bytes(&project, &opts).expect("publish mid-timeline mask SWF");
        assert!(report.can_proceed());

        // Inspect root timeline tags frame-by-frame to verify clip_depth is ONLY active on frames 2..4!
        let tags = collect_uncompressed_swf_tags(&swf_bytes);
        let mut active_clip_depths = std::collections::BTreeMap::<u16, u16>::new();
        let mut per_frame_active_clips = Vec::new();
        for (code, payload) in &tags {
            match *code {
                26 => {
                    let (depth, cd) = read_place_object2_clip_depth(payload);
                    if let Some(c) = cd {
                        active_clip_depths.insert(depth, c);
                    }
                }
                28 => {
                    let depth = u16::from_le_bytes([payload[0], payload[1]]);
                    active_clip_depths.remove(&depth);
                }
                1 => {
                    per_frame_active_clips.push(!active_clip_depths.is_empty());
                }
                _ => {}
            }
        }
        assert_eq!(
            per_frame_active_clips,
            vec![false, false, true, true, false, false],
            "root clip_depth must be placed ONLY on active mask frames 2..4 and removed on frame 4"
        );

        // Verify pixel-for-pixel parity across all 6 frames between CorePlayer, SwfBytecodePlayer,
        // backward seek_to_frame, restart, and re-imported Project!
        let mut core_player = CorePlayer::new(&project);
        let mut swf_player =
            SwfBytecodePlayer::from_bytes(&swf_bytes).expect("SwfBytecodePlayer::from_bytes");
        let reimported = import_swf_bytes(&swf_bytes).expect("re-import mid-timeline mask SWF");
        reimported
            .validate()
            .expect("re-imported mid-timeline mask Project must validate");
        let mut reimported_player = CorePlayer::new(&reimported);

        let mut frame_snapshots = Vec::with_capacity(6);
        for f in 0..6u32 {
            if f > 0 {
                core_player.tick(&project);
                reimported_player.tick(&reimported);
            }
            swf_player.step_frame();

            let core_rgba = core_player
                .render_snapshot_rgba(&project, 120, 80)
                .expect("core render");
            let swf_rgba = render_swf_player_rgba(&swf_player, &reimported, 120, 80);
            let re_rgba = reimported_player
                .render_snapshot_rgba(&reimported, 120, 80)
                .expect("reimported render");

            // Check Masked Red Bar: inside stencil (25, 20) vs outside stencil (75, 20)
            // (`render_snapshot_rgba` renders unmasked transparent background `[0, 0, 0, 0]`).
            let inside_bar = sample_pixel(&swf_rgba, 120, 25, 20);
            let outside_bar = sample_pixel(&swf_rgba, 120, 75, 20);
            if (2..4).contains(&f) {
                assert_eq!(inside_bar, [230, 30, 40, 255], "frame {f} inside mask");
                assert_eq!(outside_bar, [0, 0, 0, 0], "frame {f} clipped outside mask");
            } else {
                assert_eq!(
                    inside_bar,
                    [0, 0, 0, 0],
                    "frame {f} inactive mask must hide masked bar"
                );
                assert_eq!(outside_bar, [0, 0, 0, 0]);
            }
            assert_eq!(
                swf_rgba, core_rgba,
                "full-frame SwfBytecodePlayer vs CorePlayer RGBA mismatch on frame {f}"
            );
            assert_eq!(
                swf_rgba, re_rgba,
                "full-frame Re-imported CorePlayer RGBA mismatch on frame {f}"
            );

            // Check nested MaskedBadge (4-frame loop, active on child frames 1 and 2 -> root frames 1, 2, 5)
            let inside_badge = sample_pixel(&swf_rgba, 120, 10, 60);
            let outside_badge = sample_pixel(&swf_rgba, 120, 30, 60);
            if matches!(f % 4, 1 | 2) {
                assert_eq!(
                    inside_badge,
                    [0, 220, 90, 255],
                    "nested badge active on frame {f}"
                );
                assert_eq!(
                    outside_badge,
                    [0, 0, 0, 0],
                    "nested badge clipped on frame {f}"
                );
            } else {
                assert_eq!(
                    inside_badge,
                    [0, 0, 0, 0],
                    "nested badge inactive on frame {f}"
                );
            }
            frame_snapshots.push(swf_rgba);
        }

        // Deterministic backward seek_to_frame and restart on SwfBytecodePlayer!
        for target_f in [3u32, 0, 2, 5, 1] {
            swf_player.seek_to_frame(target_f);
            let seek_rgba = render_swf_player_rgba(&swf_player, &reimported, 120, 80);
            assert_eq!(
                seek_rgba, frame_snapshots[target_f as usize],
                "deterministic backward/forward seek_to_frame({target_f}) mismatch"
            );
        }
        swf_player.restart();
        let restart_rgba = render_swf_player_rgba(&swf_player, &reimported, 120, 80);
        assert_eq!(restart_rgba, frame_snapshots[0], "restart() mismatch");
    }
}
