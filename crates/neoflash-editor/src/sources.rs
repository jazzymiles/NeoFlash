//! Project code editing shares the existing Actions workspace and Undo history.
//!
//! The NF2 script workspace (`NF2/Code Editor` + compiler messages + script bar) is shared by
//! frame/object actions and project source files; see docs/DESIGN-SYSTEM.md › Panel files.
use crate::app::{Editor, ScriptTarget};
use crate::code_tools::CodeCommand;
use crate::script_api::{HintContext, ThisKind};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::CodeEditorPrefs;
use eframe::egui;
use neoflash_core::{Script, ScriptLanguage, SourceUnit, SymbolKind, new_id};
use neoflash_script::{Diagnostic, DiagnosticSeverity};
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::tokens::{radius, space};
use std::hash::{Hash, Hasher};

mod actions_workspace;
mod code_checks;
mod code_editor_host;
mod debugger_authoring;
mod debugger_eval;
#[path = "sources_diagnostics.rs"]
mod diagnostics_ui;
#[cfg(test)]
mod integration_tests;
mod project_sources;
mod script_assist_folding;
mod syntax_colors;

#[cfg(test)]
pub(crate) use code_editor_host::tests_jump;
use code_editor_host::{CODE_MIN_HEIGHT, CodeEditorHostState, code_editor};
#[allow(unused_imports)]
pub(crate) use code_editor_host::{code_font_id, code_layout_with_prefs, jump_to};
#[allow(unused_imports)]
pub use debugger_eval::{BreakpointSpec, DebuggerScope, DebuggerVariable, WatchExpression};
use debugger_eval::{
    DEBUGGER_AUTHORING_STATES, DebuggerAuthoringState, ExprEvaluator, find_variable_in_slice,
    find_variable_in_slice_mut, split_var_path,
};
#[allow(unused_imports)]
pub(crate) use script_assist_folding::append_script_assist_to_source;
#[allow(unused_imports)]
pub use script_assist_folding::{
    FoldRegion, ScriptAssistTemplate, compute_fold_regions, format_script_assist_snippet,
};
#[allow(unused_imports)]
pub use syntax_colors::{SyntaxColorOverrides, SyntaxColorSlot};

pub(crate) struct CodeFeedback {
    pub(super) target: egui::Id,
    pub(super) fingerprint: u64,
    pub(super) text: String,
    pub(super) success: bool,
    pub(super) diagnostics: Vec<Diagnostic>,
}

fn diagnostic_severity(severity: DiagnosticSeverity) -> Severity {
    match severity {
        DiagnosticSeverity::Error => Severity::Error,
        DiagnosticSeverity::Warning => Severity::Warning,
        DiagnosticSeverity::Info => Severity::Info,
    }
}
