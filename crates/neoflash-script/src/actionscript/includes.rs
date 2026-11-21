//! AS1/AS2/AS3 textual includes resolve immutable project resources, never raw disk/HTTP inside `neoflash-script`.
//! Disk and open-tab `.as` hydration happens at the authoring/CLI boundary (`T-412`).
use crate::Diagnostic;
use neoflash_core::{AuthoredPlayback, ResourceUnit, Script};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct Expanded {
    pub script: Script,
    pub(super) base_path: String,
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
            let is_included = !path.is_empty() && path != &self.base_path;
            if is_included {
                error.path = Some(path.clone());
            }
            if let Some(source) = self.sources.get(path) {
                error.span = (
                    crate::line_col_to_byte_offset(source, error.line, error.column),
                    crate::line_col_to_byte_offset(source, error.end_line, error.end_column),
                );
                error.fixes = crate::warnings::quick_fixes_for_diagnostic(&error, source);
            }
            if is_included {
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
            base_path: base_path.to_owned(),
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
