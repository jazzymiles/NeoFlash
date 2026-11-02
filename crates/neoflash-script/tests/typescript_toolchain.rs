use neoflash_core::{Project, Script, ScriptLanguage, SourceUnit};
use neoflash_script::{
    DebuggerSession, SourceMap, apply_quick_fix, check_project_warnings, check_warnings, compile,
    compile_debug, compile_project, compile_with_program, compile_with_source_map, decode_vlq,
    encode_vlq, levenshtein_distance, suggest_identifier,
};
use std::process::Command;

fn script(language: ScriptLanguage, source: &str) -> Script {
    Script {
        language,
        source: source.to_string(),
    }
}

fn source_unit(path: &str, language: ScriptLanguage, source: &str) -> SourceUnit {
    SourceUnit {
        id: path.to_string(),
        path: path.to_string(),
        language,
        source: source.to_string(),
    }
}

fn eval_js(code: &str) -> String {
    let output = Command::new("node")
        .arg("-e")
        .arg(code)
        .output()
        .expect("node binary must be available to execute compiled JS");
    assert!(
        output.status.success(),
        "node execution failed:\nstdout: {}\nstderr: {}\ncode:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        code
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn typescript_static_type_checker_accepts_valid_annotations_and_strips_types() {
    let valid_ts = r#"
interface Point {
    x: number;
    y: number;
    label?: string;
    readonly id: number;
}
type Metric = number | string;
class Entity {
    name: string;
    constructor(name: string) {
        this.name = name;
    }
}
class Player extends Entity {
    score: number = 0;
    addScore(delta: number, bonus: number = 1): number {
        this.score = this.score + delta + bonus;
        return this.score;
    }
}
function formatPoint(pt: Point, tag: Metric): string {
    return pt.x + "," + pt.y + ":" + tag;
}
const origin: Point = { x: 10, y: 20, id: 1 };
const coords: number[] = [1, 2, 3];
const tuple: [number, string] = [42, "ok"];
const hero: Player = new Player("Neo");
const total: number = hero.addScore(coords[0]);
const summary: string = formatPoint(origin, tuple[1]);
this.result = summary + "=" + total;
"#;
    let compiled = compile(&script(ScriptLanguage::TypeScript, valid_ts))
        .expect("valid TypeScript program should pass static type checking and compile");
    assert!(!compiled.contains("interface Point"));
    assert!(!compiled.contains("type Metric"));

    let out = eval_js(&format!(
        "const ctx = {{}}; (function() {{ {compiled} }}).call(ctx); console.log(ctx.result);"
    ));
    assert_eq!(out, "10,20:ok=2");
}

#[test]
fn typescript_static_type_checker_reports_located_diagnostics_for_type_violations() {
    // 1. Variable initialization mismatch
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "const x: number = \"not a number\";",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 1);
    assert!(
        err[0]
            .message
            .contains("is not assignable to type 'number'"),
        "unexpected error: {:?}",
        err
    );

    // 2. Variable reassignment mismatch & const reassignment
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "let flag: boolean = true;\nflag = 123;",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(
        err[0]
            .message
            .contains("is not assignable to type 'boolean'"),
        "unexpected error: {:?}",
        err
    );

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "const fixed: number = 10;\nfixed = 20;",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(err[0].message.contains("Cannot assign to 'fixed'"));

    // 3. Function return type violation & missing return
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "function compute(a: number): number {\n    return \"wrong\";\n}",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(
        err[0]
            .message
            .contains("is not assignable to return type 'number'"),
        "unexpected error: {:?}",
        err
    );

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "function mustReturn(a: number): string {\n    const b: number = a + 1;\n}",
    ))
    .unwrap_err();
    assert!(
        err[0].message.contains("must return a value"),
        "unexpected error: {:?}",
        err
    );

    // 4. Function call argument count & parameter type mismatch
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "function add(a: number, b: number): number { return a + b; }\nadd(1);",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(err[0].message.contains("Expected 2 argument(s), but got 1"));

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "function greet(name: string): string { return name; }\ngreet(999);",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(
        err[0]
            .message
            .contains("not assignable to parameter of type 'string'")
    );

    // 5. Array element type mismatch, union mismatch, interface property mismatch & readonly check
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "const nums: number[] = [1, \"bad\", 3];",
    ))
    .unwrap_err();
    assert!(err[0].message.contains("array element type 'number'"));

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "let id: number | string = true;",
    ))
    .unwrap_err();
    assert!(err[0].message.contains("number | string"));

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "interface Box { width: number; readonly tag: string; }\nconst b: Box = { width: 10 };",
    ))
    .unwrap_err();
    assert!(err[0].message.contains("Property 'tag' is missing"));

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "interface Box { width: number; readonly tag: string; }\nconst b: Box = { width: 10, tag: \"a\" };\nb.tag = \"b\";",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 3);
    assert!(err[0].message.contains("read-only property"));

    // 6. Arithmetic operator type error & misspelled identifier suggestion
    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "const bad: number = \"hello\" - 3;",
    ))
    .unwrap_err();
    assert!(
        err[0]
            .message
            .contains("Operator '-' requires 'number' operands")
    );

    let err = compile(&script(
        ScriptLanguage::TypeScript,
        "const velocity: number = 5;\nthis.x = velocty;",
    ))
    .unwrap_err();
    assert_eq!(err[0].line, 2);
    assert!(
        err[0].message.contains("Did you mean 'velocity'?"),
        "expected suggestion in {:?}",
        err
    );
}

#[test]
fn js_and_ts_modules_bundle_in_topological_order_and_execute_with_handlers() {
    let mut project = Project::blank();
    project.sources = vec![
        source_unit(
            "src/index.ts",
            ScriptLanguage::TypeScript,
            r#"
import { scaleVec, type Vec2 } from "./math/vec";
import Greeter, * as greetNs from "./utils/greeter.js";

export class StageController {
    label: string;
    constructor(prefix: string) {
        const g = new Greeter(prefix);
        this.label = g.greet(greetNs.DEFAULT_TARGET);
    }
    compute(x: number, y: number, factor: number): string {
        const v: Vec2 = scaleVec({ x, y }, factor);
        return this.label + ":" + v.x + "," + v.y;
    }
}
"#,
        ),
        source_unit(
            "src/math/vec.ts",
            ScriptLanguage::TypeScript,
            r#"
export interface Vec2 {
    x: number;
    y: number;
}
export function scaleVec(v: Vec2, k: number): Vec2 {
    return { x: v.x * k, y: v.y * k };
}
"#,
        ),
        source_unit(
            "src/utils/greeter.js",
            ScriptLanguage::JavaScript,
            r#"
export const DEFAULT_TARGET = "NeoFlash";
export default class Greeter {
    constructor(prefix) {
        this.prefix = prefix;
    }
    greet(target) {
        return this.prefix + " " + target;
    }
}
"#,
        ),
    ];

    let program = compile_project(&project).expect("multi-module JS/TS project should compile");
    // Dependencies (`src/math/vec.ts` and `src/utils/greeter.js`) must precede `src/index.ts`
    let idx_vec = program
        .module_names
        .iter()
        .position(|m| m == "src/math/vec.ts")
        .unwrap();
    let idx_greet = program
        .module_names
        .iter()
        .position(|m| m == "src/utils/greeter.js")
        .unwrap();
    let idx_main = program
        .module_names
        .iter()
        .position(|m| m == "src/index.ts")
        .unwrap();
    assert!(idx_vec < idx_main && idx_greet < idx_main);
    assert!(program.class_names.contains(&"StageController".to_string()));

    // Compile a TypeScript frame handler that imports from the project modules
    let handler = script(
        ScriptLanguage::TypeScript,
        r#"
import { StageController } from "./src/index";
const ctrl: StageController = new StageController("Hello");
this.output = ctrl.compute(3, 4, 5);
"#,
    );
    let handler_js =
        compile_with_program(&handler, &program).expect("TS handler with imports should compile");

    // Execute the bundled module code + handler in Node
    let full_js = format!(
        r#"
const __nfBuiltins = {{ __nfTrace: (...a) => console.log(...a) }};
const __nfProgram = (function() {{
{}
}})();
const clip = {{}};
(function(__nfProgram) {{
{}
}}).call(clip, __nfProgram);
console.log(clip.output);
"#,
        program.code, handler_js
    );
    let stdout = eval_js(&full_js);
    assert_eq!(stdout, "Hello NeoFlash:15,20");

    // Cross-module type error in handler is caught statically!
    let bad_handler = script(
        ScriptLanguage::TypeScript,
        r#"
import { scaleVec } from "./src/math/vec";
scaleVec({ x: 1, y: 2 }, "not-a-number");
"#,
    );
    let err = compile_with_program(&bad_handler, &program).unwrap_err();
    assert!(
        err[0]
            .message
            .contains("not assignable to parameter of type 'number'"),
        "expected cross-module type error, got: {:?}",
        err
    );
}

#[test]
fn modules_report_missing_imports_missing_exports_and_circular_dependencies() {
    // 1. Missing module diagnostic
    let mut missing_proj = Project::blank();
    missing_proj.sources = vec![source_unit(
        "main.ts",
        ScriptLanguage::TypeScript,
        "import { foo } from \"./nonexistent\";\n",
    )];
    let errs = compile_project(&missing_proj).unwrap_err();
    assert_eq!(errs[0].line, 1);
    assert!(
        errs[0]
            .message
            .contains("Cannot find module './nonexistent'")
    );

    // 2. Circular dependency diagnostic (`a.ts -> b.ts -> a.ts`)
    let mut cycle_proj = Project::blank();
    cycle_proj.sources = vec![
        source_unit(
            "a.ts",
            ScriptLanguage::TypeScript,
            "import { bVal } from \"./b\";\nexport const aVal: number = bVal + 1;\n",
        ),
        source_unit(
            "b.ts",
            ScriptLanguage::TypeScript,
            "import { aVal } from \"./a\";\nexport const bVal: number = aVal + 1;\n",
        ),
    ];
    let errs = compile_project(&cycle_proj).unwrap_err();
    assert!(
        errs.iter().any(
            |e| e.message.contains("Circular module dependency detected")
                && e.message.contains("a.ts -> b.ts -> a.ts")
        ),
        "expected circular dependency diagnostic, got: {:?}",
        errs
    );

    // 3. Missing named export with Levenshtein suggestion
    let mut bad_export_proj = Project::blank();
    bad_export_proj.sources = vec![
        source_unit(
            "math.ts",
            ScriptLanguage::TypeScript,
            "export function calculateDistance(x: number, y: number): number { return x + y; }\n",
        ),
        source_unit(
            "app.ts",
            ScriptLanguage::TypeScript,
            "import { calculateDistnce } from \"./math\";\n",
        ),
    ];
    let errs = compile_project(&bad_export_proj).unwrap_err();
    assert!(
        errs[0]
            .message
            .contains("has no exported member 'calculateDistnce'")
            && errs[0]
                .message
                .contains("Did you mean 'calculateDistance'?"),
        "unexpected error: {:?}",
        errs
    );
}

#[test]
fn jsx_and_tsx_compile_dom_elements_and_stage_display_nodes() {
    let tsx_source = r#"
function Badge(props: { label: string }): any {
    return <span className="badge">{props.label}</span>;
}
const card = <div className="card" id="main-card">Hello <Badge label="TSX" /></div>;
const clip = <MovieClip x={10} y={25} name="heroClip"><Sprite x={5} y={8} /></MovieClip>;
this.cardHtml = card.outerHTML;
this.cardClass = card.className;
this.clipSummary = `${clip.type}:${clip.name}@${clip.x},${clip.y}->${clip.children[0].type}@${clip.children[0].x},${clip.children[0].y}`;
"#;
    let compiled = compile(&script(ScriptLanguage::TypeScript, tsx_source))
        .expect("TSX handler with DOM and stage elements should compile");
    assert!(compiled.contains("__nfJsx"));

    let out = eval_js(&format!(
        "const ctx = {{}}; (function() {{ {compiled} }}).call(ctx); console.log(JSON.stringify(ctx));"
    ));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["cardClass"], "card");
    assert_eq!(
        parsed["cardHtml"],
        "<div class=\"card\" id=\"main-card\">Hello <span class=\"badge\">TSX</span></div>"
    );
    assert_eq!(
        parsed["clipSummary"],
        "MovieClip:heroClip@10,25->Sprite@5,8"
    );
}

#[test]
fn compiler_warnings_and_quick_fixes_cover_unused_missing_type_deprecated_unreachable_and_typos() {
    // 1. TypeScript: unused variable, missing type annotation, unreachable code, misspelled identifier
    let ts_src = r#"let unusedCounter: number = 0;
let inferredTitle = "Welcome";
function run(): number {
    trce("starting");
    return 42;
    const dead = 99;
}
run();
"#;
    let warnings = check_warnings(&script(ScriptLanguage::TypeScript, ts_src));
    let codes: Vec<&str> = warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(codes.contains(&"unused-variable"), "warnings: {warnings:?}");
    assert!(
        codes.contains(&"missing-type-annotation"),
        "warnings: {warnings:?}"
    );
    assert!(
        codes.contains(&"unreachable-code"),
        "warnings: {warnings:?}"
    );
    assert!(
        codes.contains(&"misspelled-identifier"),
        "warnings: {warnings:?}"
    );

    // Apply quick fix for missing type annotation
    let missing_ty_w = warnings
        .iter()
        .find(|w| w.code == "missing-type-annotation")
        .unwrap();
    let fixed_ty = apply_quick_fix(ts_src, &missing_ty_w.quick_fixes[0]);
    assert!(fixed_ty.contains("let inferredTitle: string = \"Welcome\";"));

    // Apply quick fix for misspelled `trce` -> `trace`
    let typo_w = warnings
        .iter()
        .find(|w| w.code == "misspelled-identifier")
        .unwrap();
    let fixed_typo = apply_quick_fix(ts_src, &typo_w.quick_fixes[0]);
    assert!(fixed_typo.contains("trace(\"starting\");"));

    // Apply quick fix for unreachable code
    let unreach_w = warnings
        .iter()
        .find(|w| w.code == "unreachable-code")
        .unwrap();
    let fixed_unreach = apply_quick_fix(ts_src, &unreach_w.quick_fixes[0]);
    assert!(!fixed_unreach.contains("const dead = 99;"));

    // 2. ActionScript 2.0: deprecated constructs (`tellTarget`, `eq`, `and`), unused var, missing type
    let as2_src = r#"var unusedTemp = 10;
tellTarget("hero") {
    if (name eq "neo" and ready) {
        play();
    }
}
"#;
    let as2_warnings = check_warnings(&script(ScriptLanguage::ActionScript2, as2_src));
    let as2_codes: Vec<&str> = as2_warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(as2_codes.contains(&"unused-variable"));
    assert!(as2_codes.contains(&"missing-type-annotation"));
    assert!(as2_codes.contains(&"deprecated-construct"));

    let tell_target_w = as2_warnings
        .iter()
        .find(|w| w.message.contains("'tellTarget'"))
        .unwrap();
    let modernized = apply_quick_fix(as2_src, &tell_target_w.quick_fixes[0]);
    assert!(modernized.contains("with(\"hero\")"));

    // Project-wide warnings check
    let mut proj = Project::blank();
    proj.sources.push(source_unit(
        "legacy.as",
        ScriptLanguage::ActionScript2,
        as2_src,
    ));
    assert!(!check_project_warnings(&proj).is_empty());

    // Levenshtein distance sanity check
    assert_eq!(levenshtein_distance("gotoAndPlay", "gotoAndPly"), 1);
    assert_eq!(
        suggest_identifier("gotoAndPly", ["gotoAndPlay", "gotoAndStop", "play"]),
        Some(("gotoAndPlay", 1))
    );
}

#[test]
fn source_map_v3_vlq_stack_mapping_and_step_debugger_inspect_locals_and_breakpoints() {
    // 1. VLQ round-trip across negative, zero, positive, and multi-byte values
    for val in [-1024i64, -31, -15, -1, 0, 1, 15, 31, 128, 4096] {
        let mut buf = String::new();
        encode_vlq(val, &mut buf);
        let (decoded, used) = decode_vlq(&buf).expect("VLQ should decode");
        assert_eq!(decoded, val);
        assert_eq!(used, buf.len());
    }

    // 2. Source Map v3 generation and inline base64 comment extraction
    let ts_source =
        "const base: number = 10;\nconst doubled: number = base * 2;\nthis.total = doubled + 5;";
    let compiled_map =
        compile_with_source_map(&script(ScriptLanguage::TypeScript, ts_source), "frame1.ts")
            .expect("compile_with_source_map should succeed");
    assert_eq!(compiled_map.source_map.version, 3);
    assert_eq!(compiled_map.source_map.sources, vec!["frame1.ts"]);
    assert_eq!(compiled_map.source_map.sources_content, vec![ts_source]);
    assert!(!compiled_map.source_map.mappings.is_empty());
    assert!(
        compiled_map
            .code
            .contains("//# sourceMappingURL=data:application/json;base64,")
    );

    let extracted =
        SourceMap::from_inline_comment(&compiled_map.code).expect("inline source map should parse");
    assert_eq!(extracted, compiled_map.source_map);

    // Lookup original position and map runtime stack trace back to `frame1.ts`
    let segments = extracted.decode_segments();
    assert!(!segments.is_empty());
    let last_seg = segments.last().unwrap();
    let orig = extracted
        .lookup_original_position(last_seg.generated_line + 1, last_seg.generated_column + 1)
        .expect("should resolve original position");
    assert_eq!(orig.source, "frame1.ts");
    assert_eq!(orig.line, 3);

    let fake_stack = format!(
        "Error: boom\n    at __neoflash_handler__ (frame1.ts.js:{}:{})",
        last_seg.generated_line + 1,
        last_seg.generated_column + 1
    );
    let mapped_frames = extracted.map_stack_trace(&fake_stack);
    assert_eq!(mapped_frames.len(), 1);
    assert_eq!(mapped_frames[0].source, "frame1.ts");
    assert_eq!(mapped_frames[0].line, 3);

    // 3. Step debugger & breakpoint inspection (`__nfDebug` / `__nfDebugStep` and `DebuggerSession`)
    let debug_compiled = compile_debug(&script(ScriptLanguage::TypeScript, ts_source), "frame1.ts")
        .expect("compile_debug should succeed");
    assert_eq!(debug_compiled.breakable_lines, vec![1, 2, 3]);
    assert!(
        debug_compiled.locals_by_line[&3].contains(&"base".to_string())
            && debug_compiled.locals_by_line[&3].contains(&"doubled".to_string())
    );

    // Rust-side DebuggerSession verification
    let mut session = DebuggerSession::new();
    session.register_source_map("frame1.ts", debug_compiled.source_map.clone());
    session.set_breakpoint("frame1.ts", 2);
    assert!(session.has_breakpoint("frame1.ts", 2));
    let mut locals = std::collections::BTreeMap::new();
    locals.insert("base".to_string(), serde_json::json!(10));
    assert!(!session.on_step("frame1.ts", 1, locals.clone()));
    locals.insert("doubled".to_string(), serde_json::json!(20));
    assert!(session.on_step("frame1.ts", 2, locals));
    assert_eq!(session.locals.get("doubled"), Some(&serde_json::json!(20)));

    // Runtime JS execution of instrumented debug code with breakpoint listener in Node
    let node_debug_out = eval_js(&format!(
        r#"
const ctx = {{}};
const pausedAt = [];
(function() {{
{}
}})();
globalThis.__nfDebug.setBreakpoint("frame1.ts", 3);
globalThis.__nfDebug.onPause((frame) => {{
    pausedAt.push({{ source: frame.source, line: frame.line, locals: frame.locals }});
}});
(function() {{
{}
}}).call(ctx);
console.log(JSON.stringify({{ total: ctx.total, pausedAt, lastLocals: globalThis.__nfDebug.getLocals() }}));
"#,
        neoflash_script::debug_runtime_js(),
        debug_compiled.code
    ));
    let debug_json: serde_json::Value = serde_json::from_str(&node_debug_out).unwrap();
    assert_eq!(debug_json["total"], 25);
    assert_eq!(debug_json["pausedAt"][0]["source"], "frame1.ts");
    assert_eq!(debug_json["pausedAt"][0]["line"], 3);
    assert_eq!(debug_json["lastLocals"]["base"], 10);
    assert_eq!(debug_json["lastLocals"]["doubled"], 20);
}
