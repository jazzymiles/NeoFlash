//! Script compilation for the editor and HTML5 publisher.
//!
//! JavaScript is parsed as a function body. TypeScript is transformed by Oxc
//! (syntax transformation, not TypeScript type checking). ActionScript uses a
//! deliberately small, explicit frontend; see docs/COMPATIBILITY.md.

mod actionscript;
pub use actionscript::project::{ProjectProgram, compile_project, compile_with_program};

use neoflash_core::{Script, ScriptLanguage};
use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

/// Line and column are one-based Unicode character positions in the user's source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub language: ScriptLanguage,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} {}:{}: {}",
            self.language, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

impl Diagnostic {
    pub(crate) fn at(script: &Script, offset: usize, message: impl Into<String>) -> Self {
        let mut offset = offset.min(script.source.len());
        while !script.source.is_char_boundary(offset) {
            offset -= 1;
        }
        let prefix = &script.source[..offset];
        Self {
            language: script.language,
            line: prefix.chars().filter(|c| *c == '\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
            message: message.into(),
        }
    }
}

/// Compile one frame/object handler into a JavaScript function body.
///
/// The browser calls this body with `this` bound to the current display object.
/// A failed script is never replaced by an empty handler: publication must stop.
pub fn compile(script: &Script) -> Result<String, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    match script.language {
        ScriptLanguage::ActionScript1 => {
            // Keep original source/identity while reusing the bounded AVM1 syntax
            // frontend. Case behavior is captured in the generated unit, including
            // callbacks, and never changes AS2/AS3 execution in the same project.
            let legacy = Script {
                language: ScriptLanguage::ActionScript2,
                source: script.source.clone(),
            };
            actionscript::compile_legacy(&legacy)
                .map(|body| format!("const __nfLegacyCase = true;\n{body}"))
                .map_err(|mut error| {
                    error.language = ScriptLanguage::ActionScript1;
                    vec![error]
                })
        }
        ScriptLanguage::JavaScript => compile_web(script, false),
        ScriptLanguage::TypeScript => compile_web(script, true),
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
            actionscript::compile(script).map_err(|e| vec![e])
        }
    }
}

fn web_errors(script: &Script, prefix_len: usize, errors: Vec<OxcDiagnostic>) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|error| {
            let offset = error
                .labels
                .as_ref()
                .and_then(|labels| labels.first())
                .map_or(prefix_len, |label| label.offset());
            Diagnostic::at(script, offset.saturating_sub(prefix_len), error.to_string())
        })
        .collect()
}

fn compile_web(script: &Script, typescript: bool) -> Result<String, Vec<Diagnostic>> {
    // Match the runtime's actual function-body context: `return` is legal,
    // module imports/exports and top-level await are not.
    const PREFIX: &str = "function __neoflash_handler__(){\"use strict\";\n";
    let wrapped = format!("{PREFIX}{}\n}}", script.source);
    let allocator = Allocator::default();
    let source_type = if typescript {
        SourceType::ts()
    } else {
        SourceType::cjs()
    };
    let parsed = Parser::new(&allocator, &wrapped, source_type).parse();
    if !parsed.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), parsed.errors));
    }
    if parsed.program.body.len() != 1 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "The script must be a single function body; an unmatched closing brace escapes the handler.",
        )]);
    }
    let mut program = parsed.program;
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&program);
    if !semantic.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), semantic.errors));
    }
    if !typescript {
        return Ok(script.source.clone());
    }
    let options = TransformOptions::default();
    let transformed = Transformer::new(&allocator, Path::new("handler.ts"), &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transformed.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), transformed.errors));
    }
    // Inline handlers have no module loader. Do not emit a dependency on a
    // transformer helper which the exported runtime does not ship.
    #[allow(deprecated)]
    if !transformed.helpers_used.is_empty() {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "This TypeScript construct requires runtime helpers not yet bundled by NeoFlash. Use equivalent plain JavaScript.",
        )]);
    }
    let code = Codegen::new().build(&program).code;
    Ok(format!("{code}\nreturn __neoflash_handler__.call(this);"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(language: ScriptLanguage, source: &str) -> Script {
        Script {
            language,
            source: source.into(),
        }
    }

    #[test]
    fn javascript_preserves_source_and_allows_function_body_return() {
        let source = "if (this.x > 20) return;\nthis.x += 2;";
        assert_eq!(
            compile(&script(ScriptLanguage::JavaScript, source)).unwrap(),
            source
        );
    }

    #[test]
    fn syntax_error_has_original_source_position() {
        let diagnostics =
            compile(&script(ScriptLanguage::JavaScript, "this.play();\nlet = ;")).unwrap_err();
        assert_eq!(diagnostics[0].line, 2);
        assert!(diagnostics[0].column > 1);
    }

    #[test]
    fn javascript_rejects_module_syntax_in_handlers() {
        assert!(compile(&script(ScriptLanguage::JavaScript, "import x from 'x';")).is_err());
        assert!(compile(&script(ScriptLanguage::JavaScript, "break;")).is_err());
        assert!(
            compile(&script(
                ScriptLanguage::JavaScript,
                "with (this) { x = 1; }"
            ))
            .is_err()
        );
        assert!(compile(&script(ScriptLanguage::JavaScript, "let x = 010;")).is_err());
        assert!(
            compile(&script(
                ScriptLanguage::JavaScript,
                "} const escaped = 1; function x(){"
            ))
            .is_err()
        );
    }

    #[test]
    fn typescript_is_transformed_with_real_parser() {
        let output = compile(&script(
            ScriptLanguage::TypeScript,
            "interface Point { x: number; }\nconst p: Point = { x: 4 }; this.x = p.x;",
        ))
        .unwrap();
        assert!(!output.contains("interface Point"));
        assert!(!output.contains(": Point"));
        assert!(output.contains("this.x = p.x"));
        assert!(output.contains(".call(this)"));
    }

    #[test]
    fn diagnostic_counts_unicode_characters() {
        let input = script(ScriptLanguage::ActionScript3, "// привет\n  α + @");
        let error = Diagnostic::at(&input, input.source.find('@').unwrap(), "unsupported");
        assert_eq!((error.line, error.column), (2, 7));
    }
}
