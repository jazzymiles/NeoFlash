//! Read existing export metadata without changing publishing contracts.
use crate::debugger_protocol::DebugSource;
use neoflash_core::ScriptLanguage;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub(crate) struct ScriptOrigin {
    pub(crate) language: ScriptLanguage,
    pub(crate) source: String,
    pub(crate) location: String,
}

#[derive(Deserialize)]
struct BundleUnit {
    code: String,
    source_url: String,
    source_locations: Vec<neoflash_script::RuntimeSourceLocation>,
}

#[derive(Deserialize)]
struct DebugBundle {
    scripts: Vec<BundleUnit>,
    program: Option<BundleUnit>,
}

pub(crate) fn read_export_sources(
    index: &Path,
) -> Result<(Vec<DebugSource>, Vec<ScriptOrigin>), String> {
    let file = index.with_file_name("bundle.js");
    if std::fs::metadata(&file).map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
        return Err("Debug bundle exceeds the 128 MiB metadata limit.".into());
    }
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let json = text
        .strip_prefix("globalThis.NeoFlashBundle = ")
        .and_then(|s| s.strip_suffix(";\n"))
        .ok_or("Unsupported Test Movie bundle format")?;
    let bundle: DebugBundle = serde_json::from_str(json).map_err(|e| e.to_string())?;
    #[derive(Deserialize)]
    struct Origins {
        scripts: Vec<ScriptOrigin>,
    }
    let origins: Origins = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let sources = bundle
        .scripts
        .into_iter()
        .chain(bundle.program)
        .map(|unit| DebugSource {
            source_url: unit.source_url,
            code: unit.code,
            locations: unit.source_locations,
        })
        .collect();
    Ok((sources, origins.scripts))
}
