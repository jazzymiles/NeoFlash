//! Strict, read-only legacy inspection and deliberately bounded XFL conversion.
mod legacy;
mod source;
mod swf;
mod xfl;

pub use legacy::contents::*;
pub use legacy::contours::*;
pub use legacy::geometry::*;
pub use legacy::timeline::*;
pub use legacy::{
    BinaryContentsDocument, BinaryFlaInspection, BinaryGeometryDocument, BinaryStreamGeometry,
    BinaryStreamInspection, BinaryStreamRole, BinaryTimelineDocument, RecordCounts,
    read_binary_contents, read_binary_geometry, read_binary_timeline,
};
pub use swf::{SwfCompression, SwfInspection, SwfTagCounts};

use neoflash_core::Project;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    XflDirectory,
    XflArchive,
    LegacyBinaryFla,
    Swf,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub location: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Statistics {
    pub symbols: usize,
    pub layers: usize,
    pub keyframes: usize,
    pub elements: usize,
    pub scripts: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportReport {
    pub source: String,
    pub format: ImportFormat,
    pub version: Option<String>,
    pub can_import: bool,
    pub statistics: Statistics,
    pub diagnostics: Vec<Diagnostic>,
    /// Structural inspection, not an editable conversion or a completeness claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_fla: Option<BinaryFlaInspection>,
    /// Bounded read-only SWF header and tag inspection, without bytecode execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swf: Option<SwfInspection>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("Cannot read import source: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid or unsafe source container: {0}")]
    Container(String),
    #[error("Import blocked: {0}")]
    Blocked(String),
}

impl ImportReport {
    pub(crate) fn error(
        &mut self,
        code: &str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: code.into(),
            location: location.into(),
            message: message.into(),
        });
    }
    pub(crate) fn warning(
        &mut self,
        code: &str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: code.into(),
            location: location.into(),
            message: message.into(),
        });
    }
}

pub fn inspect(path: &Path) -> Result<ImportReport, ImportError> {
    Ok(analyze(path)?.0)
}

pub fn import_project(path: &Path) -> Result<Project, ImportError> {
    let (report, project) = analyze(path)?;
    if report.can_import {
        return project.ok_or_else(|| ImportError::Blocked("no converted document".into()));
    }
    Err(ImportError::Blocked(
        report
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| format!("{} [{}]: {}", d.location, d.code, d.message))
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

fn analyze(path: &Path) -> Result<(ImportReport, Option<Project>), ImportError> {
    let source = source::read(path)?;
    let mut report = ImportReport {
        source: path.display().to_string(),
        format: source.format.clone(),
        version: source.version.clone(),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
    };
    let project = match source.format {
        ImportFormat::XflDirectory | ImportFormat::XflArchive => {
            xfl::convert(&source.files, &mut report)
        }
        ImportFormat::LegacyBinaryFla => source
            .binary
            .as_ref()
            .and_then(|binary| legacy::convert::convert(binary, &mut report)),
        ImportFormat::Swf => {
            let location = path.display().to_string();
            if let Some(container) = source.swf.as_ref() {
                swf::inspect(container, &location, &mut report);
            } else {
                report.error(
                    "swf_not_implemented",
                    location,
                    "SWF container inspected read-only. Playback, bytecode execution/decompilation and editable source recovery are not implemented.",
                );
            }
            None
        }
        ImportFormat::Unknown => {
            report.error(
                "unknown_format",
                path.display().to_string(),
                "Source is not a recognized XFL document, XFL ZIP, compound binary FLA or SWF.",
            );
            None
        }
    };
    report.can_import = project.is_some()
        && !report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
    Ok((report, project))
}
