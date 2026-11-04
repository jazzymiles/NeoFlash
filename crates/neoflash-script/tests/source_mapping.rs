//! Compiler spans, publication wrappers and actual runtime stacks form one mapping contract.
use neoflash_core::{Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};
use neoflash_script::{compile_project, compile_with_program_source_map, compile_with_source_map};
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

fn unit(path: &str, language: ScriptLanguage, source: &str) -> SourceUnit {
    SourceUnit {
        id: path.into(),
        path: path.into(),
        language,
        source: source.into(),
    }
}

fn run(project: &Project, language: ScriptLanguage, source: &str) -> Value {
    let program = compile_project(project).unwrap();
    let compiled = compile_with_program_source_map(
        &Script {
            language,
            source: source.into(),
        },
        &program,
        "Scene / frame 1",
    )
    .unwrap();
    let script = json!({"code": compiled.code, "source_url": "neoflash-script-0.js",
        "source_locations": compiled.source_map.runtime_locations()});
    let program = json!({"code": program.code, "source_url": "neoflash-project.js",
        "source_locations": program.bundle_source_map.map(|map| map.runtime_locations()).unwrap_or_default()});
    let host = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/runtime.js");
    let js = format!(
        r#"
const host = require({host:?});
const script = {script};
const program = {program};
const sources = [script, program];
const offset = host.functionBodyLineOffset();
const as2 = host.createAs2Methods();
const traces = [];
const trace = (...values) => traces.push({{values, frames: host.mapScriptStack(new Error().stack, sources, offset)}});
let result;
try {{
  const api = new Function('__nfBuiltins', program.code + '\n//# sourceURL=neoflash-project.js')({{__nfTrace: trace}});
  host.scriptFunction(script, ['trace', '__nfProgram', '__nfAs2Method', '__nfAs2Value', '__nfLegacy']).call({{}}, trace, api, as2.method, as2.value, {{caseKey: (receiver, name) => Object.keys(receiver).find(key => key.toLowerCase() === name.toLowerCase()) || name}});
  result = {{traces}};
}} catch (error) {{
  error = host.unwrapScriptThrow(error);
  result = {{traces, frames: host.mapScriptStack(error.stack, sources, offset), error: String(error), stack: error.stack}};
}}
console.log(JSON.stringify(result));
"#,
        host = host.to_str().unwrap()
    );
    let mut child = Command::new("node")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(js.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn authored_handlers_map_repeated_calls_nested_functions_unicode_and_includes() {
    for language in [
        ScriptLanguage::ActionScript1,
        ScriptLanguage::ActionScript2,
        ScriptLanguage::ActionScript3,
        ScriptLanguage::JavaScript,
        ScriptLanguage::TypeScript,
    ] {
        let source = "// 🦊 ё repeated identifiers must not choose the first occurrence\nfunction nested() {\n  trace('same');\n  throw new Error('mapping boom');\n}\ntrace('same');\nnested();";
        let output = run(&Project::blank(), language, source);
        assert_eq!(
            output["traces"][0]["frames"][0]["line"], 6,
            "{language:?}: {output}"
        );
        assert_eq!(
            output["traces"][1]["frames"][0]["line"], 3,
            "{language:?}: {output}"
        );
        assert_eq!(output["frames"][0]["line"], 4, "{language:?}: {output}");
        assert_eq!(output["frames"][0]["source"], "Scene / frame 1");
    }
    let mut project = Project::blank();
    project.resources.push(
        ResourceUnit::from_bytes(
            "helper",
            "helper.as",
            "text/plain",
            b"trace('include');\nthrow new Error('included boom');",
        )
        .unwrap(),
    );
    let output = run(
        &project,
        ScriptLanguage::ActionScript3,
        "// ё\nimport flash.geom.Point;\ninclude 'helper.as';\ntrace('after');",
    );
    assert_eq!(
        output["traces"][0]["frames"][0]["source"], "helper.as",
        "{output}"
    );
    assert_eq!(output["frames"][0]["source"], "helper.as", "{output}");
    assert_eq!(output["frames"][0]["line"], 2, "{output}");
    let output = run(
        &Project::blank(),
        ScriptLanguage::ActionScript3,
        "// ё\nimport flash.geom.Point;\ntrace('unicode import');\nthrow new Error('after import');",
    );
    assert_eq!(output["traces"][0]["frames"][0]["line"], 3, "{output}");
    assert_eq!(output["frames"][0]["line"], 4, "{output}");
}

#[test]
fn project_class_and_module_maps_survive_registration_exports_and_handler_imports() {
    let mut project = Project::blank();
    project.sources.push(unit("Hero.as", ScriptLanguage::ActionScript3,
        "package {\n public class Hero {\n public function Hero() {}\n public function boom():void {\n trace('class');\n throw new Error('class boom');\n }\n }\n}"));
    let output = run(
        &project,
        ScriptLanguage::ActionScript3,
        "new Hero().boom();",
    );
    assert_eq!(
        output["traces"][0]["frames"][0]["source"], "Hero.as",
        "{output}"
    );
    assert_eq!(output["traces"][0]["frames"][0]["line"], 5, "{output}");
    assert_eq!(output["frames"][0]["source"], "Hero.as", "{output}");
    assert_eq!(output["frames"][0]["line"], 6, "{output}");

    for language in [ScriptLanguage::JavaScript, ScriptLanguage::TypeScript] {
        let mut project = Project::blank();
        project.sources.push(unit("worker.ts", language,
            "export function fail() {\n const repeated = 'same';\n throw new Error(repeated);\n}\nexport default function later() {\n throw new Error('later');\n}"));
        let output = run(
            &project,
            language,
            "import { fail } from './worker';\nfail();",
        );
        assert_eq!(
            output["frames"][0]["source"], "worker.ts",
            "{language:?}: {output}"
        );
        assert_eq!(output["frames"][0]["line"], 3, "{language:?}: {output}");
        assert!(
            output["frames"]
                .as_array()
                .unwrap()
                .iter()
                .any(|frame| frame["source"] == "Scene / frame 1" && frame["line"] == 2),
            "{output}"
        );
        let output = run(
            &project,
            language,
            "import later from './worker';\nlater();",
        );
        assert_eq!(output["frames"][0]["line"], 6, "{language:?}: {output}");
    }
}

#[test]
fn generated_helpers_and_marker_like_strings_never_invent_authored_locations() {
    let mut project = Project::blank();
    project.sources.push(unit(
        "Hero.as",
        ScriptLanguage::ActionScript3,
        "package { public class Hero {\ninclude 'members.as';\n} }",
    ));
    project.resources.push(ResourceUnit::from_bytes("members", "members.as", "text/plain",
        b"public function Hero() {}\npublic function boom():void {\n trace('/*__nf_origin_0:class1:0*/');\n throw new Error('boom');\n}\n").unwrap());
    let output = run(
        &project,
        ScriptLanguage::ActionScript3,
        "new Hero().boom();",
    );
    assert_eq!(
        output["traces"][0]["values"][0],
        "/*__nf_origin_0:class1:0*/"
    );
    assert_eq!(
        output["traces"][0]["frames"][0]["source"], "members.as",
        "{output}"
    );
    assert_eq!(output["traces"][0]["frames"][0]["line"], 3, "{output}");
    assert_eq!(output["frames"][0]["line"], 4, "{output}");

    let script = Script {
        language: ScriptLanguage::ActionScript3,
        source: "trace('/*__nf_origin_0:123*/');".into(),
    };
    let program = compile_project(&Project::blank()).unwrap();
    let compiled = compile_with_program_source_map(&script, &program, "frame").unwrap();
    assert!(
        compiled.source_map.lookup_original_position(1, 1).is_none(),
        "intrinsic capture is generated code"
    );
    assert!(
        compiled
            .source_map
            .lookup_original_position(compiled.code.lines().count() + 30, 1)
            .is_none(),
        "unmapped lines do not borrow the previous origin"
    );
    // Mapped APIs keep the same byte budget as ordinary compilation and reject before parsing.
    let oversized = Script {
        language: ScriptLanguage::ActionScript3,
        source: "é".repeat(524_289),
    };
    for result in [
        compile_with_source_map(&oversized, "large.as"),
        compile_with_program_source_map(&oversized, &program, "large.as"),
    ] {
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "A single script is limited to 1 MiB.");
        assert_eq!((errors[0].line, errors[0].column), (1, 1));
    }
}
