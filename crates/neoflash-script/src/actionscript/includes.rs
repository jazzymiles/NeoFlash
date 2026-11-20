//! AS1/AS2/AS3 textual includes resolve immutable project resources, never raw disk/HTTP inside `neoflash-script`.
//! Disk and open-tab `.as` hydration happens at the authoring/CLI boundary (`T-412`).
use crate::Diagnostic;
use neoflash_core::{AuthoredPlayback, ResourceUnit, Script};
use std::collections::BTreeMap;

pub(super) struct Expanded {
    pub script: Script,
    pub(super) origins: Vec<(String, usize)>,
    pub(super) sources: BTreeMap<String, String>,
}

impl Expanded {
    pub fn diagnostic(&self, mut error: Diagnostic) -> Diagnostic {
        if let Some((path, line)) = self.origins.get(error.line.saturating_sub(1)) {
            let end = self.origins.get(error.end_line.saturating_sub(1));
            error.line = *line;
            error.end_line = end
                .filter(|(end_path, _)| end_path == path)
                .map_or(*line, |(_, line)| *line);
            error.path = (!path.is_empty()).then(|| path.clone());
            if let Some(source) = self.sources.get(path) {
                error.span = (
                    crate::line_col_to_byte_offset(source, error.line, error.column),
                    crate::line_col_to_byte_offset(source, error.end_line, error.end_column),
                );
                error.fixes = crate::warnings::quick_fixes_for_diagnostic(&error, source);
            }
            if !path.is_empty() {
                error.message = format!("#include {path}: {}", error.message);
            }
        }
        error
    }
}

pub(super) fn expand(
    script: &Script,
    resources: &BTreeMap<String, ResourceUnit>,
) -> Result<Expanded, Diagnostic> {
    expand_at(script, "", resources)
}

pub(super) fn expand_at(
    script: &Script,
    base_path: &str,
    resources: &BTreeMap<String, ResourceUnit>,
) -> Result<Expanded, Diagnostic> {
    match AuthoredPlayback::expand_script_includes(script, base_path, resources) {
        Ok((expanded_script, origins, sources)) => Ok(Expanded {
            script: expanded_script,
            origins,
            sources,
        }),
        Err((failing_script, offset, failing_path, prefix_path, message)) => {
            let mut diag = Diagnostic::at(&failing_script, offset, message);
            if !failing_path.is_empty() {
                diag.path = Some(failing_path.clone());
                if prefix_path {
                    diag.message = format!("#include {failing_path}: {}", diag.message);
                }
            }
            Err(diag)
        }
    }
}
