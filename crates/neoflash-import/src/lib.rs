//! Strict, read-only legacy inspection and deliberately bounded XFL conversion.
pub mod figma;
mod legacy;
pub mod legacy_interchange;
pub mod psd;
pub mod sequence;
mod source;
pub mod swc;
mod swf;
pub mod vector_fidelity;
pub mod workflows;
mod xfl;
mod xfl_export;

pub use figma::*;
pub use legacy::contents::*;
pub use legacy::contours::*;
pub use legacy::geometry::*;
pub use legacy::timeline::*;
pub use legacy::{
    BinaryContentsDocument, BinaryFlaInspection, BinaryGeometryDocument, BinaryStreamGeometry,
    BinaryStreamInspection, BinaryStreamRole, BinaryTimelineDocument, RecordCounts,
    read_binary_contents, read_binary_geometry, read_binary_timeline,
};
pub use legacy_interchange::*;
pub use psd::*;
pub use sequence::*;
pub use swc::{
    SwcAssetFile, SwcCatalog, SwcComponent, SwcConflictPolicy, SwcDependency, SwcDependencyKind,
    SwcFeatures, SwcInspection, SwcLibrary, SwcLibraryEntry, SwcLinkReport, SwcScriptDef,
    SwcVersionConflict, SwcVersions, build_swc_inspection, import_swc, import_swc_bytes,
    is_flash_builtin_definition, link_swc_into_project, normalize_catalog_id, parse_swc_catalog,
    resolve_swc_dependencies,
};
pub use swf::{
    SwfAudioEvent, SwfBytecodePlayer, SwfCompression, SwfDisplayEntry, SwfFrameState,
    SwfInspection, SwfPlaybackResult, SwfTagCounts, SwfVmKind, play_swf_bytes, play_swf_path,
};
pub use vector_fidelity::*;
pub use xfl_export::{export_fla, export_fla_bytes};

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
    Swc,
    Figma,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub location: String,
    pub message: String,
}

/// Alias for [`Diagnostic`] emitted by import workflows.
pub type ImportDiagnostic = Diagnostic;

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
    /// SWC catalog and embedded library inspection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swc: Option<SwcInspection>,
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
        swc: None,
    };
    let project = match source.format {
        ImportFormat::XflDirectory | ImportFormat::XflArchive => {
            xfl::convert(&source.files, &source.binary_files, &mut report)
        }
        ImportFormat::LegacyBinaryFla => source
            .binary
            .as_ref()
            .and_then(|binary| legacy::convert::convert(binary, &mut report)),
        ImportFormat::Swf => {
            let location = path.display().to_string();
            if let Some(container) = source.swf.as_ref() {
                swf::convert(container, &location, &mut report)
            } else {
                report.error(
                    "swf_invalid_container",
                    location,
                    "SWF container could not be loaded for conversion.",
                );
                None
            }
        }
        ImportFormat::Swc => {
            let location = path.display().to_string();
            swc::convert(&source.files, &source.binary_files, &location, &mut report)
        }
        ImportFormat::Figma => {
            match figma::import_figma_path(path, &figma::FigmaImportOptions::default()) {
                Ok((proj, fig_report)) => {
                    return Ok((fig_report, Some(proj)));
                }
                Err(err) => {
                    report.error(
                        "figma_import_failed",
                        path.display().to_string(),
                        err.to_string(),
                    );
                    None
                }
            }
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
