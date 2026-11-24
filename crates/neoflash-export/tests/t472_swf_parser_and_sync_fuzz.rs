//! `[T-472]` Phase 2 — SWF Flat-Chain Stack-Overflow Regression, Script Converter Fuzzing,
//! & AMF0/AMF3 Sync Payload Fuzzing Suite (`< 5s` debug runtime).
//!
//! Fixed PRNG seed: `0x4E46_3437_3200` (`0` external crates).
//! Covers:
//! 1. `50,000`-term flat expression chains (`1 + 1 + ...`, `a.b.b...`, `a[0][0]...`, `f()()...`,
//!    `true && ...`, `false || ...`, `1 == 1 == ...`, `1 < 1 < ...`, `1 << 1 << ...`, `1 * 1 * ...`)
//!    and combined nested-paren + chain spines (`30 * 200 = 6,000` AST height) in
//!    `preflight_swf_export`, `preflight_swf_publish`, `export_swf_bytes`, and `publish_swf_bytes`
//!    across BOTH AVM1 (`FlashPlayer8Avm1`) and AVM2 (`FlashPlayer10Avm2`), plus a positive
//!    `100`-term `1 + 1 + ...` compilation & `play_swf_bytes` execution check (`trace("100")`).
//! 2. Deterministic mutation & flat-chain / deep-nesting fuzzing of JS, TS, AS1, AS2, and AS3
//!    scripts across `convert_project_scripts`, `compile`, `compile_project`, `check_project`,
//!    `compile_debug`, and `check_warnings`.
//! 3. Prefix truncation, adversarial `U29`/reference-cycle/oversized-length, and seeded PRNG
//!    mutation fuzzing of `neoflash_core::amf_decode` (`AMF0` & `AMF3` wire format used by
//!    `neoflash-sync`).

use neoflash_core::{
    AmfDecoded, AmfGraph, AmfNode, AmfValue, Element, ElementKind, FillRule, FrameScript, Keyframe,
    Layer, LayerProperties, ObjectEvent, ObjectScript, PathCommand, Project, Script,
    ScriptLanguage, SourceUnit, amf_decode,
};
use neoflash_export::{
    SwfCompression, SwfExportOptions, SwfPublishOptions, SwfPublishProfile, export_swf_bytes,
    play_swf_bytes, preflight_swf_export, preflight_swf_publish, publish_swf_bytes,
};
use neoflash_script::{
    ConversionTarget, check_project, check_warnings, compile, compile_debug, compile_project,
    convert_project_scripts,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

const FUZZ_SEED: u64 = 0x4E46_3437_3200;

#[derive(Clone)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next_u64() as usize) % upper
        }
    }

    fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    fn mutate_bytes(&mut self, base: &[u8]) -> Vec<u8> {
        let mut buf = base.to_vec();
        if buf.is_empty() {
            return vec![self.next_u8()];
        }
        let ops = 1 + self.next_usize(5);
        for _ in 0..ops {
            let mode = self.next_usize(7);
            let pos = self.next_usize(buf.len());
            match mode {
                0 => buf[pos] ^= 1u8 << (self.next_usize(8) as u8),
                1 => buf[pos] = self.next_u8(),
                2 if pos + 4 <= buf.len() => {
                    let pat = match self.next_usize(4) {
                        0 => u32::MAX.to_be_bytes(),
                        1 => [0xBF, 0xFF, 0xFF, 0xFF], // max U29 length header!
                        2 => i32::MIN.to_be_bytes(),
                        _ => [0xC0, 0xAF, 0xFF, 0xFE],
                    };
                    buf[pos..pos + 4].copy_from_slice(&pat);
                }
                3 => {
                    let del = 1 + self.next_usize((buf.len() - pos).min(12));
                    buf.drain(pos..pos + del);
                    if buf.is_empty() {
                        buf.push(0);
                    }
                }
                4 if buf.len() < 8192 => {
                    let len = 1 + self.next_usize((buf.len() - pos).min(12));
                    let slice = buf[pos..pos + len].to_vec();
                    let dst = self.next_usize(buf.len() + 1);
                    buf.splice(dst..dst, slice);
                }
                5 => {
                    let tokens = [
                        b"(".as_slice(),
                        b")",
                        b"{",
                        b"}",
                        b"[",
                        b"]",
                        b"<x a={",
                        b"}/>",
                        b"/*",
                        b"\"\\",
                        b">>>=",
                        b"..",
                        b"@",
                    ];
                    let tok = tokens[self.next_usize(tokens.len())];
                    if buf.len() < 8192 {
                        buf.splice(pos..pos, tok.iter().copied());
                    }
                }
                _ => buf[pos] = b';',
            }
        }
        buf
    }

    fn mutate_script(&mut self, base: &str) -> String {
        let raw = self.mutate_bytes(base.as_bytes());
        String::from_utf8_lossy(&raw).into_owned()
    }
}

fn assert_no_panic<F: FnOnce()>(context: &str, f: F) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_owned()
        };
        panic!("PANIC caught during fuzzing [{context}]: {msg}");
    }
}

fn build_repeated_chain(first: &str, sep_and_rhs: &str, count: usize) -> String {
    let mut out = String::with_capacity(first.len() + sep_and_rhs.len() * count + 32);
    out.push_str(first);
    for _ in 0..count {
        out.push_str(sep_and_rhs);
    }
    out
}

#[test]
fn t472_swf_parser_50000_flat_chains_and_combined_spines_never_stack_overflow() {
    // Run inside a dedicated 2 MiB stack thread to guarantee even tight thread stacks never overflow!
    let handle = std::thread::Builder::new()
        .name("t472-tight-stack-swf".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            // 1. Positive check: 200-term `1 + 1 + ... + 1` (near MAX_SWF_CHAIN_LEN = 256)
            //    compiles AND executes cleanly in both AVM1 and AVM2 on a 2 MiB stack!
            let valid_200 = build_repeated_chain("1", "+1", 199);
            for (lang, profile) in [
                (ScriptLanguage::ActionScript2, SwfPublishProfile::FlashPlayer8Avm1),
                (ScriptLanguage::ActionScript3, SwfPublishProfile::FlashPlayer10Avm2),
            ] {
                let mut proj = Project::blank();
                let root_id = proj.root.clone();
                proj.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
                    layer: None,
                    frame: 0,
                    script: Script {
                        language: lang,
                        source: format!("var total = {valid_200}; trace(total); stop();"),
                    },
                }];
                let opts = SwfExportOptions {
                    publish: SwfPublishOptions {
                        profile,
                        compression: SwfCompression::UncompressedFws,
                        ..SwfPublishOptions::default()
                    },
                    strict: true,
                };
                let pre = preflight_swf_export(&proj, &opts);
                assert!(
                    pre.can_proceed(),
                    "200-term chain must pass SWF preflight ({profile:?}): {:?}",
                    pre.items
                );
                let (swf_bytes, _) =
                    export_swf_bytes(&proj, &opts).expect("200-term chain must export");
                let play = play_swf_bytes(&swf_bytes, 1).expect("play 200-term SWF");
                assert_eq!(play.trace_output, vec!["200".to_owned()]);
            }

            // 2. Adversarial 50,000-term flat chains (< 250 KiB each, well under 1 MiB limit!)
            //    + combined nested-paren + chain spine (`30 * 200 = 6,000` AST height where
            //    paren depth = 30 < 40 and local chain = 200 < 256) + 500-deep E4X tag chain!
            let flat_add_50k = build_repeated_chain("1", "+1", 50_000);
            let flat_mul_50k = build_repeated_chain("1", "*1", 50_000);
            let flat_member_50k = build_repeated_chain("a", ".b", 50_000);
            let flat_index_50k = build_repeated_chain("a", "[0]", 50_000);
            let flat_call_50k = build_repeated_chain("f", "()", 50_000);
            let flat_and_50k = build_repeated_chain("1", "&&1", 50_000);
            let flat_or_50k = build_repeated_chain("0", "||0", 50_000);
            let flat_eq_50k = build_repeated_chain("1", "==1", 50_000);
            let flat_rel_50k = build_repeated_chain("1", "<2", 50_000);
            let flat_shl_50k = build_repeated_chain("1", "<<1", 50_000);
            let flat_bitor_50k = build_repeated_chain("1", "|1", 50_000);

            let sub_chain_200 = build_repeated_chain("1", "+1", 200);
            let mut combined_paren_chain = "1".to_owned();
            for _ in 0..30 {
                combined_paren_chain = format!("({combined_paren_chain})+{sub_chain_200}");
            }
            let deep_xml_tags_400 =
                format!("{}x{}", "<a>".repeat(400), "</a>".repeat(400));

            for (name, expr_src, avm2_only) in [
                ("flat_add_50k", &flat_add_50k, false),
                ("flat_mul_50k", &flat_mul_50k, false),
                ("flat_member_50k", &flat_member_50k, false),
                ("flat_index_50k", &flat_index_50k, false),
                ("flat_call_50k", &flat_call_50k, false),
                ("flat_and_50k", &flat_and_50k, false),
                ("flat_or_50k", &flat_or_50k, false),
                ("flat_eq_50k", &flat_eq_50k, false),
                ("flat_rel_50k", &flat_rel_50k, false),
                ("flat_shl_50k", &flat_shl_50k, false),
                ("flat_bitor_50k", &flat_bitor_50k, false),
                ("combined_paren_chain_6000", &combined_paren_chain, false),
                ("deep_xml_tags_400", &deep_xml_tags_400, true),
            ] {
                assert!(
                    expr_src.len() < 1_048_576,
                    "adversarial script '{name}' must be under 1 MiB ({} bytes)",
                    expr_src.len()
                );

                // Check AVM1 frame script & external source
                if !avm2_only {
                    let mut p_avm1 = Project::blank();
                    let root_id = p_avm1.root.clone();
                    p_avm1.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
                        layer: None,
                        frame: 0,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: format!("var x = {expr_src};"),
                        },
                    }];
                    let opts_avm1 = SwfExportOptions {
                        publish: SwfPublishOptions {
                            profile: SwfPublishProfile::FlashPlayer8Avm1,
                            compression: SwfCompression::UncompressedFws,
                            ..SwfPublishOptions::default()
                        },
                        strict: true,
                    };
                    let rep1 = preflight_swf_export(&p_avm1, &opts_avm1);
                    assert!(
                        rep1.has_blocking(),
                        "AVM1 preflight must block '{name}' cleanly, got: {:?}",
                        rep1.items
                    );
                    assert!(
                        export_swf_bytes(&p_avm1, &opts_avm1).is_err(),
                        "AVM1 export_swf_bytes must return clean Err for '{name}'"
                    );
                }

                // Check AVM2 document class + timeline frame script
                let mut p_avm2 = Project::blank();
                p_avm2.document_class = Some("ChainProbe".into());
                p_avm2.sources = vec![SourceUnit {
                    id: "s_probe".into(),
                    path: "ChainProbe.as".into(),
                    language: ScriptLanguage::ActionScript3,
                    source: format!(
                        "package {{ import flash.display.MovieClip; public class ChainProbe extends MovieClip {{ public function ChainProbe() {{ super(); var x:* = {expr_src}; }} }} }}"
                    ),
                }];
                let opts_avm2 = SwfExportOptions {
                    publish: SwfPublishOptions {
                        profile: SwfPublishProfile::FlashPlayer10Avm2,
                        compression: SwfCompression::UncompressedFws,
                        ..SwfPublishOptions::default()
                    },
                    strict: true,
                };
                let rep2 = preflight_swf_publish(&p_avm2, &opts_avm2.publish);
                assert!(
                    rep2.has_blocking(),
                    "AVM2 preflight must block '{name}' cleanly, got: {:?}",
                    rep2.items
                );
                assert!(
                    export_swf_bytes(&p_avm2, &opts_avm2).is_err(),
                    "AVM2 export_swf_bytes must return clean Err for '{name}'"
                );
                assert!(
                    publish_swf_bytes(&p_avm2, &opts_avm2.publish).is_err(),
                    "AVM2 publish_swf_bytes must return clean Err for '{name}'"
                );
            }
        })
        .expect("spawn tight-stack thread");

    handle
        .join()
        .expect("tight-stack SWF chain test must never panic or overflow");
}

#[test]
fn t472_fuzz_script_converter_and_swf_compiler_mutations() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x10);

    // 1. Test 50,000-term flat chains & deep parens in `convert_project_scripts`, `compile`, and `check_warnings`
    //    inside a tight 2 MiB stack thread to catch any Oxc / TS-checker / AS-converter recursion!
    let handle = std::thread::Builder::new()
        .name("t472-tight-stack-convert".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let flat_add_50k = build_repeated_chain("1", "+1", 50_000);
            let flat_member_50k = build_repeated_chain("a", ".b", 50_000);
            let deep_parens_600 = format!("{}1{}", "(".repeat(600), ")".repeat(600));
            let deep_unary_600 = format!("{}1", "!".repeat(600));
            let deep_ternary_400 = build_repeated_chain("1", "?1:1", 400);
            let newline_div_bypass = format!(
                "let a = 1\n/ 2; // `\n{}0{};",
                "(".repeat(5000),
                ")".repeat(5000)
            );
            let postfix_div_bypass = format!(
                "let a = 1; a++ / 2; // `\n{}0{};",
                "(".repeat(5000),
                ")".repeat(5000)
            );
            let deep_unary_plus_5000 = format!("var v = {}0;", "+ ".repeat(5000));
            let deep_jsx_5000 = format!(
                "var v = {}x{};",
                "<a>".repeat(5000),
                "</a>".repeat(5000)
            );
            let deep_braceless_if_5000 = format!("{};", "if(1)".repeat(5000));
            let deep_labels_5000 = format!("{};", "L:".repeat(5000));

            // Positive [A-04] ECMAScript cases on 2 MiB stack:
            // 1) Regex literal after block `}` + newline and after `break\n/re/` containing escaped `\(` and `//\``
            // 2) 200-term `a < b && c < d` comparison chain must NOT trip JSX depth!
            let cmp_chain_200 = build_repeated_chain("1 < 2", " && 1 < 2", 199);
            for web_lang in [ScriptLanguage::JavaScript, ScriptLanguage::TypeScript] {
                let valid_regex_and_cmp = Script {
                    language: web_lang,
                    source: format!(
                        "let s = \"foo(//`\";\nif (true) {{}}\n/foo\\(\\/\\/`/.test(s);\nlet ok1 = /foo\\(\\/\\/`/.test(s);\nfor (let i = 0; i < 1; i++) {{\n  break\n  /foo\\(\\/\\/`/.test(s);\n}}\nlet ok2 = {cmp_chain_200};\nreturn ok1 && ok2;"
                    ),
                };
                assert!(
                    compile(&valid_regex_and_cmp).is_ok(),
                    "valid block-regex, break-ASI-regex, and 200-term `<` chain must compile in {web_lang:?}"
                );
            }

            for (label, body) in [
                ("flat_add_50k", format!("var v = {flat_add_50k};")),
                ("flat_member_50k", format!("var v = {flat_member_50k};")),
                ("deep_parens_600", format!("var v = {deep_parens_600};")),
                ("deep_unary_600", format!("var v = {deep_unary_600};")),
                ("deep_ternary_400", format!("var v = {deep_ternary_400};")),
                ("newline_div_bypass_5000", newline_div_bypass),
                ("postfix_div_bypass_5000", postfix_div_bypass),
                ("deep_unary_plus_5000", deep_unary_plus_5000),
                ("deep_jsx_5000", deep_jsx_5000),
                ("deep_braceless_if_5000", deep_braceless_if_5000),
                ("deep_labels_5000", deep_labels_5000),
            ] {
                for lang in [
                    ScriptLanguage::ActionScript2,
                    ScriptLanguage::ActionScript3,
                    ScriptLanguage::JavaScript,
                    ScriptLanguage::TypeScript,
                ] {
                    let s = Script {
                        language: lang,
                        source: body.clone(),
                    };
                    assert_no_panic(&format!("compile_{lang:?}_{label}"), || {
                        let _ = compile(&s);
                        let _ = check_warnings(&s);
                        let _ = compile_debug(&s, "fuzz.script");
                    });

                    let mut proj = Project::blank();
                    proj.script_language = Some(lang);
                    let root_id = proj.root.clone();
                    proj.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
                        layer: None,
                        frame: 0,
                        script: s,
                    }];
                    for target in [
                        ConversionTarget::ActionScript3,
                        ConversionTarget::ActionScript2,
                        ConversionTarget::JavaScript,
                    ] {
                        assert_no_panic(&format!("convert_{lang:?}_to_{target:?}_{label}"), || {
                            if let Ok((converted, _)) = convert_project_scripts(&proj, target) {
                                converted
                                    .validate()
                                    .expect("converted project must validate");
                            }
                        });
                    }
                }
            }
        })
        .expect("spawn converter thread");
    handle
        .join()
        .expect("converter flat-chain / deep-paren thread must never overflow");

    // 2. Seeded PRNG mutation fuzzing across AS2, AS3, JS, and TS in `convert_project_scripts` & SWF preflight
    let as2_seed = r#"
class Hero extends MovieClip {
    var speed:Number = 4;
    function Hero() { super(); }
    function step(dx:Number):Number {
        if (Key.isDown(Key.LEFT)) { this._x -= dx * this.speed; }
        return this._x;
    }
}
"#;
    let as3_seed = r#"package game {
    import flash.display.MovieClip;
    import flash.events.MouseEvent;
    public class Panel extends MovieClip {
        private var count:int = 0;
        public function Panel(init:int = 1) {
            super();
            this.count = init;
            var xml:XML = <hud score={init}><item id="a">Go</item></hud>;
            trace(xml..@id);
        }
        public function bump(delta:int = 2, ...rest):int {
            try {
                this.count += delta + rest.length;
            } catch (err:Error) {
                this.count = 0;
            } finally {
                stop();
            }
            return this.count;
        }
    }
}"#;
    let ts_seed = r#"
interface Config { speed: number; label?: string; }
const cfg: Config = { speed: 5, label: "ready" };
function advance(x: number, step: number = 1): number {
    return x + step * cfg.speed;
}
this.x = advance(10, 2);
"#;

    let mut cases = 0usize;
    for iter in 0..180 {
        cases += 1;
        match iter % 3 {
            0 => {
                let mut_as2 = rng.mutate_script(as2_seed);
                let mut_handler =
                    rng.mutate_script("on(release) { gotoAndPlay(2); _root.score += 1; }");
                assert_no_panic(&format!("mut_as2_{iter}"), || {
                    let mut p = Project::blank();
                    p.script_language = Some(ScriptLanguage::ActionScript2);
                    p.sources = vec![SourceUnit {
                        id: "u1".into(),
                        path: "Hero.as".into(),
                        language: ScriptLanguage::ActionScript2,
                        source: mut_as2.clone(),
                    }];
                    let root_id = p.root.clone();
                    p.symbol_mut(&root_id).unwrap().timeline.layers = vec![Layer {
                        id: "l1".into(),
                        name: "L1".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "e1".into(),
                                name: "btn".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                        PathCommand::LineTo { x: 20.0, y: 0.0 },
                                        PathCommand::LineTo { x: 20.0, y: 20.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::EvenOdd,
                                    fill: [255, 0, 0, 255],
                                    stroke: [0, 0, 0, 0],
                                    fill_gradient: None,
                                    stroke_gradient: None,
                                    stroke_width: 0.0,
                                },
                                scripts: vec![ObjectScript {
                                    event: ObjectEvent::Click,
                                    script: Script {
                                        language: ScriptLanguage::ActionScript2,
                                        source: mut_handler,
                                    },
                                }],
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    }];
                    for tgt in [
                        ConversionTarget::ActionScript3,
                        ConversionTarget::JavaScript,
                    ] {
                        if let Ok((conv, _)) = convert_project_scripts(&p, tgt) {
                            conv.validate().unwrap();
                        }
                    }
                    let opts = SwfExportOptions {
                        publish: SwfPublishOptions {
                            profile: SwfPublishProfile::FlashPlayer8Avm1,
                            ..SwfPublishOptions::default()
                        },
                        strict: false,
                    };
                    let _ = preflight_swf_export(&p, &opts);
                    let _ = export_swf_bytes(&p, &opts);
                });
            }
            1 => {
                let mut_as3 = rng.mutate_script(as3_seed);
                assert_no_panic(&format!("mut_as3_{iter}"), || {
                    let mut p = Project::blank();
                    p.script_language = Some(ScriptLanguage::ActionScript3);
                    p.document_class = Some("game.Panel".into());
                    p.sources = vec![SourceUnit {
                        id: "u1".into(),
                        path: "game/Panel.as".into(),
                        language: ScriptLanguage::ActionScript3,
                        source: mut_as3,
                    }];
                    for tgt in [
                        ConversionTarget::ActionScript2,
                        ConversionTarget::JavaScript,
                    ] {
                        if let Ok((conv, _)) = convert_project_scripts(&p, tgt) {
                            conv.validate().unwrap();
                        }
                    }
                    let opts = SwfExportOptions {
                        publish: SwfPublishOptions {
                            profile: SwfPublishProfile::FlashPlayer10Avm2,
                            ..SwfPublishOptions::default()
                        },
                        strict: false,
                    };
                    let _ = preflight_swf_export(&p, &opts);
                    let _ = export_swf_bytes(&p, &opts);
                });
            }
            _ => {
                let mut_ts = rng.mutate_script(ts_seed);
                assert_no_panic(&format!("mut_ts_{iter}"), || {
                    let mut p = Project::blank();
                    p.script_language = Some(ScriptLanguage::TypeScript);
                    let root_id = p.root.clone();
                    p.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
                        layer: None,
                        frame: 0,
                        script: Script {
                            language: ScriptLanguage::TypeScript,
                            source: mut_ts,
                        },
                    }];
                    let _ = compile_project(&p);
                    let _ = check_project(&p);
                    if let Ok((conv, _)) = convert_project_scripts(&p, ConversionTarget::JavaScript)
                    {
                        conv.validate().unwrap();
                    }
                });
            }
        }
    }
    assert_eq!(cases, 180);
}

#[test]
fn t472_fuzz_amf0_and_amf3_wire_decoder_for_sync() {
    let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x20);

    // Valid AMF0 object + array + ECMA array + reference + date + XML
    let amf0_seed: Vec<u8> = vec![
        0x03, // Object
        0x00, 0x03, b'n', b'u', b'm', 0x00, 0x40, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, // 42.0
        0x00, 0x03, b'a', b'r', b'r', 0x0A, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01, 0x02, 0x00, 0x02,
        b'h', b'i', // StrictArray [true, "hi"]
        0x00, 0x03, b'r', b'e', b'f', 0x07, 0x00, 0x00, // Reference(0) -> self cycle!
        0x00, 0x00, 0x09, // ObjectEnd
    ];

    // Valid AMF3 array + object + trait + vector-int + dictionary + byte-array
    let amf3_seed: Vec<u8> = vec![
        0x09,         // Array
        (3 << 1) | 1, // 3 dense items
        0x01,         // empty associative part
        0x04,
        0x2A, // Integer 42
        0x06,
        (3 << 1) | 1,
        b'a',
        b'b',
        b'c', // String "abc"
        0x0D,
        (2 << 1) | 1,
        0x00,
        0,
        0,
        0,
        1,
        0xFF,
        0xFF,
        0xFF,
        0xFF, // Vector.<int> [1, -1]
    ];

    let mut cases = 0usize;

    // 1. Prefix truncations across AMF0 and AMF3
    for (enc, seed) in [(0u8, &amf0_seed), (3u8, &amf3_seed)] {
        for len in 0..=seed.len() {
            cases += 1;
            assert_no_panic(&format!("amf{enc}_trunc_{len}"), || {
                let _ = amf_decode(enc, &seed[..len]);
            });
        }
    }

    // 2. Adversarial AMF0/AMF3 bombs (huge StrictArray u32::MAX count, huge AMF3 U29 count, deep 300-level nesting)
    let mut deep_amf0 = Vec::with_capacity(600);
    for _ in 0..300 {
        deep_amf0.extend_from_slice(&[0x0A, 0, 0, 0, 1]); // StrictArray(1)
    }
    let mut deep_amf3 = Vec::with_capacity(900);
    for _ in 0..300 {
        deep_amf3.extend_from_slice(&[0x09, 0x03, 0x01]); // Array(dense=1)
    }
    for (enc, adv) in [
        (0u8, vec![0x0A, 0xFF, 0xFF, 0xFF, 0xFF]), // StrictArray len = u32::MAX!
        (0u8, vec![0x0C, 0xFF, 0xFF, 0xFF, 0xFF]), // LongString len = u32::MAX!
        (0u8, vec![0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0x09]), // ECMAArray hint = u32::MAX
        (3u8, vec![0x09, 0xBF, 0xFF, 0xFF, 0xFF, 0x01]), // AMF3 Array U29 max!
        (3u8, vec![0x0C, 0xBF, 0xFF, 0xFF, 0xFF]), // AMF3 ByteArray U29 max!
        (3u8, vec![0x0D, 0xBF, 0xFF, 0xFF, 0xFF, 0x00]), // AMF3 VectorInt U29 max!
        (3u8, vec![0x11, 0xBF, 0xFF, 0xFF, 0xFF, 0x00]), // AMF3 Dictionary U29 max!
        (0u8, deep_amf0),
        (3u8, deep_amf3),
        (99u8, vec![0x01, 0x02]), // unsupported encoding version
    ] {
        cases += 1;
        assert_no_panic(&format!("amf{enc}_adv_{cases}"), || {
            assert!(amf_decode(enc, &adv).is_err());
        });
    }

    // 3. Deterministic PRNG mutations across AMF0, AMF3, and Flash `.sol` Local SharedObject binary streams
    let sol_graph = AmfGraph {
        root: AmfValue::Ref(0),
        nodes: vec![AmfNode::Object {
            class_name: None,
            entries: vec![
                ("level".to_owned(), AmfValue::Number("3".to_owned())),
                ("player".to_owned(), AmfValue::String("Neo".to_owned())),
            ],
        }],
    };
    let sol0_seed = sol_graph
        .encode_sol("saveSlot", 0)
        .expect("encode AMF0 SOL");
    let sol3_seed = sol_graph
        .encode_sol("saveSlot3", 3)
        .expect("encode AMF3 SOL");
    for sol_seed in [&sol0_seed, &sol3_seed] {
        for len in (0..=sol_seed.len()).step_by(2) {
            cases += 1;
            assert_no_panic(&format!("sol_trunc_{len}"), || {
                let _ = AmfDecoded::decode_sol(&sol_seed[..len]);
            });
        }
    }

    for iter in 0..240 {
        let (enc, base) = match iter % 4 {
            0 => (0u8, &amf0_seed),
            1 => (3u8, &amf3_seed),
            2 => (250u8, &sol0_seed),
            _ => (253u8, &sol3_seed),
        };
        let mutated = rng.mutate_bytes(base);
        cases += 1;
        assert_no_panic(&format!("amf_or_sol_{enc}_mut_{iter}"), || {
            if enc >= 250 {
                let _ = AmfDecoded::decode_sol(&mutated);
            } else {
                let _ = amf_decode(enc, &mutated);
            }
        });
    }

    assert!(cases >= 280);
}
