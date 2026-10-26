//! Self-contained HTML5 publication. Authored evaluation remains in the Rust WASM runtime.

use base64::{Engine, engine::general_purpose::STANDARD};
use neoflash_core::{ElementKind, Project, Script, ScriptLanguage};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

const HOST: &str = include_str!("../../../web/runtime.js");
const PATH_GEOMETRY: &str = include_str!("../../../web/path.js");
const MARKER: &str = "neoflash-export.json";

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("Invalid project: {0}")]
    InvalidProject(String),
    #[error("Script compilation failed:\n{0}")]
    Scripts(String),
    #[error("HTML5 runtime is missing or incompatible: {0}. Run scripts/build-web.sh first.")]
    MissingRuntime(String),
    #[error(
        "Export destination contains files that do not belong to NeoFlash: {0}. Choose an empty folder or a previous NeoFlash export."
    )]
    Destination(String),
    #[error("Could not write HTML5 export: {0}")]
    Io(#[from] std::io::Error),
    #[error("Could not serialize HTML5 export: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize)]
struct CompiledScript {
    language: ScriptLanguage,
    source: String,
    code: String,
    location: String,
}

#[derive(Serialize)]
struct Bundle<'a> {
    project: &'a Project,
    scripts: Vec<CompiledScript>,
    wasm: String,
    warnings: Vec<String>,
}

/// Reports current publish limitations before the user exports their work.
pub fn export_warnings(project: &Project) -> Vec<String> {
    let mut fonts = BTreeSet::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    if let ElementKind::Text { font_family, .. } = &element.kind {
                        fonts.insert(font_family.clone());
                    }
                }
            }
        }
    }
    fonts.into_iter().map(|font| format!(
        "Font '{font}' is referenced by name, not embedded. Visitors need this font installed; otherwise the browser uses a fallback."
    )).collect()
}

fn compile_scripts(project: &Project) -> Result<Vec<CompiledScript>, ExportError> {
    let mut scripts = Vec::new();
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |script: &Script, location: String| {
        if script.source.trim().is_empty() {
            return;
        }
        let key = serde_json::to_string(script).expect("Script contains only serializable strings");
        if !seen.insert(key) {
            return;
        }
        match neoflash_script::compile(script) {
            Ok(code) => scripts.push(CompiledScript {
                language: script.language,
                source: script.source.clone(),
                code,
                location,
            }),
            Err(errors) => {
                for error in errors {
                    failures.push(format!(
                        "{location}, line {}, column {}: {}",
                        error.line, error.column, error.message
                    ));
                }
            }
        }
    };
    for symbol in &project.symbols {
        for frame in &symbol.timeline.scripts {
            add(
                &frame.script,
                format!("{} / frame {}", symbol.name, frame.frame + 1),
            );
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        add(
                            &handler.script,
                            format!(
                                "{} / {} / frame {} / {} / {:?}",
                                symbol.name,
                                layer.name,
                                key.frame + 1,
                                element.name,
                                handler.event
                            ),
                        );
                    }
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(scripts)
    } else {
        Err(ExportError::Scripts(failures.join("\n")))
    }
}

/// Export an entirely local package that also works when index.html is opened directly.
/// `runtime_dir` contains wasm-bindgen's `--target no-modules` output, global `NeoFlashWasm`.
/// Existing unrelated directories are never overwritten.
pub fn export_project(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
) -> Result<PathBuf, ExportError> {
    let issues = neoflash_core::validate(project);
    if !issues.is_empty() {
        return Err(ExportError::InvalidProject(format!("{issues:?}")));
    }
    let scripts = compile_scripts(project)?;
    let bindings_path = runtime_dir.join("neoflash_web.js");
    let wasm_path = runtime_dir.join("neoflash_web_bg.wasm");
    let bindings = fs::read_to_string(&bindings_path)
        .map_err(|_| ExportError::MissingRuntime(bindings_path.display().to_string()))?;
    if !bindings.contains("NeoFlashWasm") {
        return Err(ExportError::MissingRuntime(
            "bindings must use --target no-modules --no-modules-global NeoFlashWasm".into(),
        ));
    }
    let wasm = fs::read(&wasm_path)
        .map_err(|_| ExportError::MissingRuntime(wasm_path.display().to_string()))?;
    if !wasm.starts_with(b"\0asm") {
        return Err(ExportError::MissingRuntime(
            "invalid WebAssembly header".into(),
        ));
    }
    let warnings = export_warnings(project);
    let bundle = Bundle {
        project,
        scripts,
        wasm: STANDARD.encode(wasm),
        warnings: warnings.clone(),
    };
    let bundle_js = format!(
        "globalThis.NeoFlashBundle = {};\n",
        serde_json::to_string(&bundle)?
    );
    let title = html_escape(&project.name);
    let html = html_document(&title);
    let marker =
        serde_json::json!({"generator":"NeoFlash", "format_version":1, "project":project.name});
    let readme = format!(
        "{} — NeoFlash HTML5 export\n\nOpen index.html in a modern browser, or upload this entire folder to static hosting.\nNo plugins, CDN, SWF, build tools or network services are required to play it.\nThe shared Rust evaluator is embedded as WebAssembly in bundle.js.\n\nPublishing notes:\n{}\n\nUse scripts in Actions only from authors you trust: JavaScript executes in the browser.\n",
        project.name,
        if warnings.is_empty() {
            "No font warnings.".into()
        } else {
            warnings.join("\n")
        }
    );

    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        let owned = fs::read_to_string(output_dir.join(MARKER))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|marker| {
                marker["generator"] == "NeoFlash" && marker["format_version"] == 1
            });
        if !owned {
            return Err(ExportError::Destination(output_dir.display().to_string()));
        }
    }
    fs::create_dir_all(output_dir)?;
    // Prepare all data before writing. Replace only generated files, with index written last.
    for (name, contents) in [
        ("neoflash_web.js", bindings.as_str()),
        ("runtime.js", HOST),
        ("path.js", PATH_GEOMETRY),
        ("bundle.js", bundle_js.as_str()),
        ("README.txt", readme.as_str()),
    ] {
        atomic_write(&output_dir.join(name), contents.as_bytes())?;
    }
    atomic_write(
        &output_dir.join(MARKER),
        serde_json::to_string_pretty(&marker)?.as_bytes(),
    )?;
    atomic_write(&output_dir.join("index.html"), html.as_bytes())?;
    Ok(output_dir.join("index.html"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("neoflash-{}.tmp", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html_document(title: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{ color-scheme: dark; font: 13px system-ui, sans-serif; }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; background: #25272b; color: #d2d6dd; min-height: 100vh; display: flex; flex-direction: column; }}
    main {{ flex: 1; display: grid; place-items: center; overflow: hidden; padding: 24px; min-height: 0; }}
    canvas {{ display: block; max-width: 100%; max-height: calc(100vh - 110px); width: auto; height: auto; box-shadow: 0 12px 48px #0005; outline: none; }}
    footer {{ min-height: 46px; display: flex; align-items: center; gap: 12px; padding: 8px 16px; border-top: 1px solid #ffffff12; }}
    button {{ background: #34373e; color: #f3f4f6; border: 1px solid #555a64; border-radius: 4px; font: inherit; padding: 5px 12px; cursor: pointer; }}
    button:hover {{ background: #464c58; }}
    button:focus-visible {{ outline: 2px solid #7ba9ef; outline-offset: 2px; }}
    #name {{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    #status {{ margin-left: auto; font-variant-numeric: tabular-nums; color: #a5acb7; }}
    #error {{ position: fixed; inset: 24px; max-height: 60vh; overflow: auto; margin: auto; z-index: 2; padding: 24px; border: 1px solid #f19393; background: #351d22; color: #ffe0e0; white-space: pre-wrap; display: none; }}
    #loading {{ position: fixed; inset: 0; display: grid; place-items: center; pointer-events: none; }}
    @media (max-width: 600px) {{ main {{ padding: 8px; }} #name {{ display: none; }} }}
  </style>
  <script defer src="neoflash_web.js"></script>
  <script defer src="bundle.js"></script>
  <script defer src="path.js"></script>
  <script defer src="runtime.js"></script>
</head>
<body>
  <main><canvas id="stage" tabindex="0" aria-label="{title}"></canvas></main>
  <footer><button id="toggle" type="button">Pause</button><button id="restart" type="button">Restart</button><span id="name">{title}</span><span id="status" role="status"></span></footer>
  <div id="loading" role="status">Loading project…</div><pre id="error" role="alert"></pre>
</body>
</html>
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn temp() -> PathBuf {
        static ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-export-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn runtime(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
        fs::write(path.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    }
    #[test]
    fn export_escapes_title_and_keeps_sources_external() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let mut project = Project::blank();
        project.name = "</title><script>alert('x')</script>".into();
        let output = dir.join("output");
        export_project(&project, &output, &rt).unwrap();
        let html = fs::read_to_string(output.join("index.html")).unwrap();
        assert!(html.contains("&lt;/title&gt;&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
        assert!(
            fs::read_to_string(output.join("bundle.js"))
                .unwrap()
                .contains("AGFzbQEAAAA=")
        );
        assert!(!html.contains("https://"));
        assert!(html.find("src=\"path.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(
            fs::read_to_string(output.join("path.js"))
                .unwrap()
                .contains("isPointInStroke")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn refuses_unrelated_output_without_modifying_it() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let out = dir.join("output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("index.html"), "existing work").unwrap();
        assert!(matches!(
            export_project(&Project::blank(), &out, &rt),
            Err(ExportError::Destination(_))
        ));
        assert_eq!(
            fs::read_to_string(out.join("index.html")).unwrap(),
            "existing work"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn missing_runtime_does_not_create_output() {
        let dir = temp();
        let out = dir.join("out");
        assert!(matches!(
            export_project(&Project::blank(), &out, &dir.join("missing")),
            Err(ExportError::MissingRuntime(_))
        ));
        assert!(!out.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unsupported_script_blocks_export_before_writing() {
        let dir = temp();
        let out = dir.join("out");
        let mut project = Project::blank();
        project.symbols[0]
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "package example { public class Missing {} }".into(),
                },
            });
        assert!(matches!(
            export_project(&project, &out, &dir.join("missing")),
            Err(ExportError::Scripts(_))
        ));
        assert!(!out.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
