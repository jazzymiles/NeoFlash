//! SWC component and library archive (`catalog.xml` + embedded `library.swf`)
//! inspection, conversion, class/symbol linkage, and multi-library dependency
//! & version conflict resolution.

use crate::{ImportError, ImportFormat, ImportReport, Severity, Statistics, source, swf};
use neoflash_core::{
    Element, ElementKind, Keyframe, Layer, LayerProperties, MAX_RESOURCE_TOTAL_BYTES,
    MAX_RESOURCES, Project, ResourceUnit, ScriptLanguage, SourceUnit, Symbol, SymbolKind, Timeline,
    Transform,
};
use roxmltree::{Document, Node, ParsingOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwcDependencyKind {
    Inheritance,
    Signature,
    Expression,
    Namespace,
}

impl SwcDependencyKind {
    pub fn from_catalog_type(raw: &str) -> Self {
        match raw.trim() {
            "i" | "inheritance" => Self::Inheritance,
            "s" | "signature" => Self::Signature,
            "n" | "namespace" => Self::Namespace,
            _ => Self::Expression,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcDependency {
    pub id: String,
    pub kind: SwcDependencyKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcScriptDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_checksum: Option<u64>,
    pub defs: Vec<String>,
    pub deps: Vec<SwcDependency>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcComponent {
    pub class_name: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcVersions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swc_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_build: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_supported_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_build: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcFeatures {
    pub script_deps: bool,
    pub components: bool,
    pub files: bool,
    pub method_deps: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcLibraryEntry {
    pub path: String,
    pub scripts: Vec<SwcScriptDef>,
    pub keep_as3_metadata: Vec<String>,
    pub digests: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcAssetFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_time: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xmlns: Option<String>,
    pub versions: SwcVersions,
    pub features: SwcFeatures,
    pub components: Vec<SwcComponent>,
    pub libraries: Vec<SwcLibraryEntry>,
    pub files: Vec<SwcAssetFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcInspection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swc_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_name: Option<String>,
    pub library_paths: Vec<String>,
    pub script_count: usize,
    pub definition_count: usize,
    pub dependency_count: usize,
    pub component_count: usize,
    pub asset_file_count: usize,
    pub definitions: Vec<String>,
    pub unresolved_external_dependencies: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwcLibrary {
    pub name: String,
    pub catalog: SwcCatalog,
    pub inspection: SwcInspection,
    pub project: Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwcConflictPolicy {
    PreferNewer,
    PreferExisting,
    ErrorOnConflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcVersionConflict {
    pub definition: String,
    pub existing_library: String,
    pub incoming_library: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_mod_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming_mod_time: Option<u64>,
    pub winner: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwcLinkReport {
    pub linked_definitions: Vec<String>,
    pub linked_symbols: usize,
    pub linked_sources: usize,
    pub linked_resources: usize,
    pub resolved_dependencies: Vec<String>,
    pub unresolved_dependencies: Vec<String>,
    pub conflicts: Vec<SwcVersionConflict>,
    pub library_load_order: Vec<String>,
}

pub fn normalize_catalog_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.replace([':', '/'], ".")
}

pub fn is_flash_builtin_definition(def_id: &str) -> bool {
    let norm = normalize_catalog_id(def_id);
    if norm.starts_with("flash.")
        || norm.starts_with("avmplus.")
        || norm.starts_with("adobe.utils.")
        || norm.starts_with("__AS3__.")
    {
        return true;
    }
    matches!(
        norm.as_str(),
        "Object"
            | "Boolean"
            | "Number"
            | "int"
            | "uint"
            | "String"
            | "Array"
            | "Vector"
            | "Function"
            | "Class"
            | "Math"
            | "Date"
            | "RegExp"
            | "Error"
            | "ArgumentError"
            | "DefinitionError"
            | "EvalError"
            | "RangeError"
            | "ReferenceError"
            | "SecurityError"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "VerifyError"
            | "XML"
            | "XMLList"
            | "QName"
            | "Namespace"
            | "JSON"
            | "trace"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "escape"
            | "unescape"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "AS3"
    )
}

pub fn parse_swc_catalog(
    xml: &str,
    location: &str,
    report: &mut ImportReport,
) -> Option<SwcCatalog> {
    let doc = match Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    ) {
        Ok(d) => d,
        Err(err) => {
            report.error("swc_catalog_xml", location, err.to_string());
            return None;
        }
    };

    let root = doc.root_element();
    if root.tag_name().name() != "swc" {
        report.error(
            "swc_catalog_invalid",
            location,
            format!(
                "Expected <swc> root element in catalog.xml, found <{}>.",
                root.tag_name().name()
            ),
        );
        return None;
    }

    let xmlns = root.tag_name().namespace().map(str::to_owned);
    if let Some(ns) = xmlns.as_deref()
        && !ns.starts_with("http://www.adobe.com/flash/swccatalog/")
    {
        report.warning(
            "swc_catalog_namespace",
            location,
            format!(
                "Non-standard SWC catalog XML namespace {ns:?} parsed using local element names."
            ),
        );
    }

    let mut catalog = SwcCatalog {
        xmlns,
        ..Default::default()
    };

    for section in root.children().filter(Node::is_element) {
        match section.tag_name().name() {
            "versions" => {
                for v in section.children().filter(Node::is_element) {
                    match v.tag_name().name() {
                        "swc" => {
                            catalog.versions.swc_version =
                                v.attribute("version").map(str::to_owned);
                        }
                        "flex" => {
                            catalog.versions.flex_version =
                                v.attribute("version").map(str::to_owned);
                            catalog.versions.flex_build = v.attribute("build").map(str::to_owned);
                            catalog.versions.minimum_supported_version =
                                v.attribute("minimumSupportedVersion").map(str::to_owned);
                        }
                        "compiler" => {
                            catalog.versions.compiler_name = v.attribute("name").map(str::to_owned);
                            catalog.versions.compiler_version =
                                v.attribute("version").map(str::to_owned);
                            catalog.versions.compiler_build =
                                v.attribute("build").map(str::to_owned);
                        }
                        _ => {}
                    }
                }
            }
            "features" => {
                for f in section.children().filter(Node::is_element) {
                    match f.tag_name().name() {
                        "feature-script-deps" => catalog.features.script_deps = true,
                        "feature-components" => catalog.features.components = true,
                        "feature-files" => catalog.features.files = true,
                        "feature-method-deps" => catalog.features.method_deps = true,
                        _ => {}
                    }
                }
            }
            "components" => {
                for comp in section
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "component")
                {
                    let class_name = comp
                        .attribute("className")
                        .map(normalize_catalog_id)
                        .unwrap_or_default();
                    let name = comp
                        .attribute("name")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            class_name
                                .rsplit_once('.')
                                .map_or_else(|| class_name.clone(), |(_, short)| short.to_owned())
                        });
                    if !class_name.is_empty() {
                        catalog.components.push(SwcComponent {
                            class_name,
                            name,
                            uri: comp.attribute("uri").map(str::to_owned),
                            icon: comp.attribute("icon").map(str::to_owned),
                        });
                    }
                }
            }
            "libraries" => {
                for lib in section
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "library")
                {
                    let path = lib
                        .attribute("path")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("library.swf")
                        .to_owned();
                    if let Err(err) = source::safe_name(&path) {
                        report.error("swc_unsafe_path", location, err.to_string());
                        return None;
                    }
                    let mut entry = SwcLibraryEntry {
                        path,
                        ..Default::default()
                    };
                    for child in lib.children().filter(Node::is_element) {
                        match child.tag_name().name() {
                            "script" => {
                                let name = child
                                    .attribute("name")
                                    .map(str::trim)
                                    .unwrap_or("")
                                    .to_owned();
                                let mod_time = child
                                    .attribute("mod")
                                    .and_then(|s| s.trim().parse::<u64>().ok());
                                let signature_checksum = child
                                    .attribute("signatureChecksum")
                                    .and_then(|s| s.trim().parse::<u64>().ok());
                                let mut defs = Vec::new();
                                let mut deps = Vec::new();
                                for item in child.children().filter(Node::is_element) {
                                    match item.tag_name().name() {
                                        "def" => {
                                            if let Some(id) = item
                                                .attribute("id")
                                                .map(normalize_catalog_id)
                                                .filter(|s| !s.is_empty())
                                            {
                                                defs.push(id);
                                            }
                                        }
                                        "dep" => {
                                            if let Some(id) = item
                                                .attribute("id")
                                                .map(normalize_catalog_id)
                                                .filter(|s| !s.is_empty())
                                            {
                                                let kind = SwcDependencyKind::from_catalog_type(
                                                    item.attribute("type").unwrap_or("e"),
                                                );
                                                deps.push(SwcDependency { id, kind });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                entry.scripts.push(SwcScriptDef {
                                    name,
                                    mod_time,
                                    signature_checksum,
                                    defs,
                                    deps,
                                });
                            }
                            "keep-as3-metadata" => {
                                for m in child
                                    .children()
                                    .filter(|n| n.is_element() && n.tag_name().name() == "metadata")
                                {
                                    if let Some(name) =
                                        m.attribute("name").map(str::trim).filter(|s| !s.is_empty())
                                    {
                                        entry.keep_as3_metadata.push(name.to_owned());
                                    }
                                }
                            }
                            "digests" => {
                                for d in child
                                    .children()
                                    .filter(|n| n.is_element() && n.tag_name().name() == "digest")
                                {
                                    let dtype = d.attribute("type").unwrap_or("SHA-256").to_owned();
                                    let val = d.attribute("value").unwrap_or("").to_owned();
                                    if !val.is_empty() {
                                        entry.digests.push((dtype, val));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    catalog.libraries.push(entry);
                }
            }
            "files" => {
                for f in section
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "file")
                {
                    if let Some(path) = f.attribute("path").map(str::trim).filter(|s| !s.is_empty())
                    {
                        if let Err(err) = source::safe_name(path) {
                            report.error("swc_unsafe_path", location, err.to_string());
                            return None;
                        }
                        let mod_time = f
                            .attribute("mod")
                            .and_then(|s| s.trim().parse::<u64>().ok());
                        catalog.files.push(SwcAssetFile {
                            path: path.to_owned(),
                            mod_time,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Some(catalog)
}

pub fn build_swc_inspection(catalog: &SwcCatalog) -> SwcInspection {
    let mut definitions_set = BTreeSet::new();
    let mut deps_set = BTreeSet::new();
    let mut script_count = 0usize;
    let mut dependency_count = 0usize;
    let mut library_paths = Vec::with_capacity(catalog.libraries.len());

    for lib in &catalog.libraries {
        library_paths.push(lib.path.clone());
        script_count += lib.scripts.len();
        for script in &lib.scripts {
            for def in &script.defs {
                definitions_set.insert(def.clone());
            }
            for dep in &script.deps {
                dependency_count += 1;
                deps_set.insert(dep.id.clone());
            }
        }
    }

    let definitions: Vec<String> = definitions_set.iter().cloned().collect();
    let unresolved_external_dependencies: Vec<String> = deps_set
        .into_iter()
        .filter(|dep| !definitions_set.contains(dep) && !is_flash_builtin_definition(dep))
        .collect();

    SwcInspection {
        swc_version: catalog.versions.swc_version.clone(),
        flex_version: catalog.versions.flex_version.clone(),
        compiler_name: catalog.versions.compiler_name.clone(),
        library_paths,
        script_count,
        definition_count: definitions.len(),
        dependency_count,
        component_count: catalog.components.len(),
        asset_file_count: catalog.files.len(),
        definitions,
        unresolved_external_dependencies,
    }
}

pub(crate) fn convert(
    files: &BTreeMap<String, String>,
    binary_files: &BTreeMap<String, Vec<u8>>,
    location: &str,
    report: &mut ImportReport,
) -> Option<Project> {
    let Some(catalog_xml) = files.get("catalog.xml") else {
        report.error(
            "swc_missing_catalog",
            location,
            "SWC archive does not contain catalog.xml.",
        );
        return None;
    };
    let catalog = parse_swc_catalog(catalog_xml, &format!("{location}:catalog.xml"), report)?;
    let inspection = build_swc_inspection(&catalog);
    if report.version.is_none() {
        report.version = inspection
            .swc_version
            .clone()
            .or_else(|| inspection.flex_version.clone());
    }
    report.swc = Some(inspection.clone());

    if !inspection.unresolved_external_dependencies.is_empty() {
        report.warning(
            "swc_external_dependencies",
            location,
            format!(
                "SWC references {} external class definition(s) not bundled in this archive: {}.",
                inspection.unresolved_external_dependencies.len(),
                inspection
                    .unresolved_external_dependencies
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    let lib_paths: Vec<String> = if catalog.libraries.is_empty() {
        vec!["library.swf".to_owned()]
    } else {
        catalog.libraries.iter().map(|l| l.path.clone()).collect()
    };

    let mut base_project: Option<Project> = None;
    for lib_path in &lib_paths {
        let Some(swf_bytes) = binary_files.get(lib_path).or_else(|| {
            binary_files
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(lib_path))
                .map(|(_, v)| v)
        }) else {
            report.error(
                "swc_missing_library_swf",
                location,
                format!("SWC catalog references {lib_path:?}, which is missing from the archive."),
            );
            return None;
        };

        let container = match swf::read_container_from_bytes(swf_bytes.clone()) {
            Ok(c) => c,
            Err(err) => {
                report.error("swc_invalid_library_swf", location, err.to_string());
                return None;
            }
        };

        let mut swf_report = ImportReport {
            source: format!("{location}:{lib_path}"),
            format: ImportFormat::Swf,
            version: None,
            can_import: false,
            statistics: Statistics::default(),
            diagnostics: Vec::new(),
            binary_fla: None,
            swf: None,
            swc: None,
        };
        let swf_project = swf::convert(
            &container,
            &format!("{location}:{lib_path}"),
            &mut swf_report,
        );
        if report.swf.is_none() {
            report.swf = swf_report.swf;
        }
        for diag in swf_report.diagnostics {
            report.diagnostics.push(diag);
        }
        let proj = swf_project?;
        if let Some(existing) = &mut base_project {
            merge_swf_project_into(existing, proj);
        } else {
            base_project = Some(proj);
        }
    }

    let mut project = base_project?;
    project.name = Path::new(location)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("SWC Library")
        .to_owned();

    // Wire catalog script definitions, inheritance/dependencies, and component symbols.
    wire_catalog_into_project(&mut project, &catalog, binary_files, location, report);

    if let Err(err) = project.validate() {
        report.error(
            "swc_invalid_project",
            location,
            format!("Converted SWC project failed validation: {err}"),
        );
        return None;
    }

    report.statistics = compute_project_statistics(&project);
    Some(project)
}

fn merge_swf_project_into(dst: &mut Project, src: Project) {
    let existing_sym_ids: HashSet<String> = dst.symbols.iter().map(|s| s.id.clone()).collect();
    for sym in src.symbols {
        if sym.id == src.root || existing_sym_ids.contains(&sym.id) {
            continue;
        }
        dst.symbols.push(sym);
    }
    let existing_src_paths: HashSet<String> = dst.sources.iter().map(|s| s.path.clone()).collect();
    for unit in src.sources {
        if !existing_src_paths.contains(&unit.path) && dst.sources.len() < 1024 {
            dst.sources.push(unit);
        }
    }
    let existing_res_ids: HashSet<String> = dst.resources.iter().map(|r| r.id.clone()).collect();
    for res in src.resources {
        if !existing_res_ids.contains(&res.id) && dst.resources.len() < MAX_RESOURCES {
            dst.resources.push(res);
        }
    }
}

fn wire_catalog_into_project(
    project: &mut Project,
    catalog: &SwcCatalog,
    binary_files: &BTreeMap<String, Vec<u8>>,
    location: &str,
    report: &mut ImportReport,
) {
    let mut existing_classes: HashSet<String> = project
        .symbols
        .iter()
        .filter_map(|s| s.class_name.clone())
        .collect();
    let mut existing_linkages: HashSet<String> = project
        .symbols
        .iter()
        .filter_map(|s| s.linkage_id.clone())
        .collect();

    // Map existing decompiled source paths (e.g. "scripts/com/example/Foo.as") to class names.
    let mut covered_defs = HashSet::new();
    for unit in &project.sources {
        let stripped = unit
            .path
            .strip_prefix("scripts/")
            .unwrap_or(&unit.path)
            .strip_suffix(".as")
            .unwrap_or(&unit.path);
        let dotted = stripped.replace('/', ".");
        covered_defs.insert(dotted);
    }

    // Ensure every catalog <script><def id="..."/></script> has a corresponding AS3 SourceUnit
    // and, if it extends DisplayObject/Sprite/MovieClip/SimpleButton or is a component, a library Symbol.
    let component_classes: HashMap<String, &SwcComponent> = catalog
        .components
        .iter()
        .map(|c| (c.class_name.clone(), c))
        .collect();

    for lib in &catalog.libraries {
        for script in &lib.scripts {
            for def_id in &script.defs {
                if !is_valid_qualified_class_name(def_id) {
                    continue;
                }
                if !covered_defs.contains(def_id) && project.sources.len() < 1024 {
                    let source_code = synthesize_swc_script_stub(def_id, script);
                    let rel_path = format!("scripts/{}.as", def_id.replace('.', "/"));
                    let src_id = format!("swc-source-{}", project.sources.len() + 1);
                    project.sources.push(SourceUnit {
                        id: src_id,
                        path: rel_path,
                        language: ScriptLanguage::ActionScript3,
                        source: source_code,
                    });
                    covered_defs.insert(def_id.clone());
                }

                let is_display_super = script.deps.iter().any(|d| {
                    d.kind == SwcDependencyKind::Inheritance
                        && (d.id.starts_with("flash.display.")
                            || component_classes.contains_key(&d.id))
                });
                let comp_meta = component_classes.get(def_id).copied();

                if (is_display_super || comp_meta.is_some()) && !existing_classes.contains(def_id) {
                    // Check if there is an unlinked non-root symbol with matching short name.
                    let short_name = comp_meta.map(|c| c.name.as_str()).unwrap_or_else(|| {
                        def_id.rsplit_once('.').map_or(def_id.as_str(), |(_, s)| s)
                    });
                    if let Some(sym) = project.symbols.iter_mut().find(|s| {
                        s.id != project.root
                            && s.class_name.is_none()
                            && (s.name == short_name || s.name == *def_id)
                    }) {
                        sym.class_name = Some(def_id.clone());
                        if sym.linkage_id.is_none() && existing_linkages.insert(def_id.clone()) {
                            sym.linkage_id = Some(def_id.clone());
                        }
                        existing_classes.insert(def_id.clone());
                    } else {
                        let idx = project.symbols.len() + 1;
                        let sym_id = format!("swc-symbol-{idx}");
                        let layer_id = format!("swc-layer-{idx}");
                        let linkage = existing_linkages
                            .insert(def_id.clone())
                            .then(|| def_id.clone());
                        existing_classes.insert(def_id.clone());
                        project.symbols.push(Symbol {
                            folder: if comp_meta.is_some() {
                                "SWC Components".to_owned()
                            } else {
                                "SWC Definitions".to_owned()
                            },
                            class_name: Some(def_id.clone()),
                            linkage_id: linkage,
                            id: sym_id,
                            name: short_name.to_owned(),
                            kind: SymbolKind::MovieClip,
                            timeline: Timeline {
                                guides: Vec::new(),
                                frame_count: 1,
                                layers: vec![Layer {
                                    properties: LayerProperties::default(),
                                    id: layer_id,
                                    name: "Layer 1".to_owned(),
                                    visible: true,
                                    locked: false,
                                    keyframes: vec![Keyframe {
                                        frame: 0,
                                        elements: vec![Element {
                                            id: format!("swc-el-{idx}"),
                                            name: String::new(),
                                            accessibility: None,
                                            transform: Transform::default(),
                                            kind: ElementKind::Rectangle {
                                                width: 100.0,
                                                height: 100.0,
                                                fill: [120, 160, 220, 64],
                                                stroke: [80, 120, 180, 180],
                                                stroke_width: 1.0,
                                            },
                                            scripts: Vec::new(),
                                            ..Element::default()
                                        }],
                                        ..Keyframe::default()
                                    }],
                                }],
                                scripts: Vec::new(),
                            },
                        });
                    }
                }
            }
        }
    }

    // Import any bundled binary asset files listed under <files> (PNG, JPEG, WAV, MP3).
    let mut resource_bytes: usize = project.resources.iter().map(|r| r.byte_len()).sum();
    for asset in &catalog.files {
        if project.resources.len() >= MAX_RESOURCES {
            break;
        }
        let Some(raw) = binary_files.get(&asset.path).or_else(|| {
            binary_files
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&asset.path))
                .map(|(_, v)| v)
        }) else {
            report.warning(
                "swc_missing_asset_file",
                location,
                format!(
                    "SWC catalog <file path={:?}/> is not present in the archive.",
                    asset.path
                ),
            );
            continue;
        };
        let Some(media_type) = detect_swc_asset_media_type(&asset.path, raw) else {
            continue;
        };
        let Some(next_total) = resource_bytes
            .checked_add(raw.len())
            .filter(|&n| n <= MAX_RESOURCE_TOTAL_BYTES)
        else {
            break;
        };
        let res_id = format!("swc-resource-{}", project.resources.len() + 1);
        let res_path = format!("swc/{}", asset.path.trim_start_matches('/'));
        if let Ok(unit) = ResourceUnit::from_bytes(&res_id, res_path, media_type, raw) {
            resource_bytes = next_total;
            project.resources.push(unit);
        }
    }
    upgrade_format_version_for_swc(project);
}

fn upgrade_format_version_for_swc(project: &mut Project) {
    if !project.sources.is_empty()
        || project
            .symbols
            .iter()
            .any(|s| s.class_name.is_some() || s.linkage_id.is_some())
    {
        project.format_version = project.format_version.max(2);
    }
    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    if project.symbols.iter().any(|s| !s.folder.is_empty()) {
        project.format_version = project.format_version.max(4);
    }
}

fn detect_swc_asset_media_type(path: &str, bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || path.to_ascii_lowercase().ends_with(".png") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff])
        || path.to_ascii_lowercase().ends_with(".jpg")
        || path.to_ascii_lowercase().ends_with(".jpeg")
    {
        Some("image/jpeg")
    } else if (bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE")
        || path.to_ascii_lowercase().ends_with(".wav")
    {
        Some("audio/wav")
    } else if path.to_ascii_lowercase().ends_with(".mp3") {
        Some("audio/mpeg")
    } else {
        None
    }
}

fn synthesize_swc_script_stub(def_id: &str, script: &SwcScriptDef) -> String {
    let (pkg, cls) = match def_id.rsplit_once('.') {
        Some((p, c)) => (p, c),
        None => ("", def_id),
    };
    let super_id = script
        .deps
        .iter()
        .find(|d| d.kind == SwcDependencyKind::Inheritance && d.id != "Object" && d.id != def_id)
        .map(|d| d.id.as_str());
    let mut imports = BTreeSet::new();
    for dep in &script.deps {
        if dep.id != def_id && dep.id.contains('.') && is_valid_qualified_class_name(&dep.id) {
            imports.insert(dep.id.clone());
        }
    }
    let mut out = String::new();
    if pkg.is_empty() {
        out.push_str("package {\n");
    } else {
        out.push_str(&format!("package {pkg} {{\n"));
    }
    for imp in &imports {
        out.push_str(&format!("    import {imp};\n"));
    }
    if let Some(sup) = super_id {
        let sup_short = sup.rsplit_once('.').map_or(sup, |(_, s)| s);
        out.push_str(&format!("    public class {cls} extends {sup_short} {{\n"));
        out.push_str(&format!(
            "        public function {cls}() {{\n            super();\n        }}\n"
        ));
    } else {
        out.push_str(&format!("    public class {cls} {{\n"));
        out.push_str(&format!("        public function {cls}() {{\n        }}\n"));
    }
    out.push_str("    }\n}\n");
    out
}

fn compute_project_statistics(project: &Project) -> Statistics {
    let mut stats = Statistics {
        symbols: project.symbols.len(),
        scripts: project.sources.len(),
        ..Default::default()
    };
    for sym in &project.symbols {
        stats.layers += sym.timeline.layers.len();
        stats.scripts += sym.timeline.scripts.len();
        for layer in &sym.timeline.layers {
            stats.keyframes += layer.keyframes.len();
            for key in &layer.keyframes {
                stats.elements += key.elements.len();
                for el in &key.elements {
                    stats.scripts += el.scripts.len();
                }
            }
        }
    }
    stats
}

fn is_valid_qualified_class_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 512
        && trimmed.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '$'))
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        })
}

pub fn import_swc(path: &Path) -> Result<SwcLibrary, ImportError> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(ImportError::Container(
            "SWC archive exceeds 64 MiB limit".into(),
        ));
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("library")
        .to_owned();
    import_swc_bytes(&name, &bytes)
}

pub fn import_swc_bytes(name: &str, bytes: &[u8]) -> Result<SwcLibrary, ImportError> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(ImportError::Container(
            "SWC archive exceeds 64 MiB limit".into(),
        ));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ImportError::Container(e.to_string()))?;
    if zip.len() > 10_000 {
        return Err(ImportError::Container(
            "SWC archive exceeds 10000 entries".into(),
        ));
    }
    let mut files = BTreeMap::new();
    let mut binary_files = BTreeMap::new();
    let mut total = 0u64;
    let mut names = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        let entry_name = entry.name().to_owned();
        source::safe_name(&entry_name)?;
        if !names.insert(entry_name.clone()) {
            return Err(ImportError::Container(format!(
                "duplicate SWC ZIP entry: {entry_name}"
            )));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| ImportError::Container("SWC ZIP size overflow".into()))?;
        if entry.size() > 8 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            return Err(ImportError::Container(
                "SWC ZIP exceeds 8 MiB per entry or 64 MiB total uncompressed limit".into(),
            ));
        }
        if !entry.is_dir() {
            if entry_name.ends_with(".xml") {
                let mut content = String::new();
                entry
                    .by_ref()
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_string(&mut content)?;
                files.insert(entry_name, content);
            } else {
                let mut raw = Vec::new();
                entry
                    .by_ref()
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut raw)?;
                binary_files.insert(entry_name, raw);
            }
        }
    }

    let mut report = ImportReport {
        source: name.to_owned(),
        format: ImportFormat::Swc,
        version: None,
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };
    let Some(catalog_xml) = files.get("catalog.xml") else {
        return Err(ImportError::Blocked(
            "SWC archive does not contain catalog.xml".into(),
        ));
    };
    let Some(catalog) = parse_swc_catalog(catalog_xml, &format!("{name}:catalog.xml"), &mut report)
    else {
        return Err(ImportError::Blocked(
            report
                .diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    };
    let Some(project) = convert(&files, &binary_files, name, &mut report) else {
        return Err(ImportError::Blocked(
            report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| format!("{} [{}]: {}", d.location, d.code, d.message))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    };
    let inspection = report.swc.unwrap_or_else(|| build_swc_inspection(&catalog));
    Ok(SwcLibrary {
        name: name.to_owned(),
        catalog,
        inspection,
        project,
    })
}

pub fn link_swc_into_project(
    project: &mut Project,
    library: &SwcLibrary,
    policy: SwcConflictPolicy,
) -> Result<SwcLinkReport, ImportError> {
    resolve_swc_dependencies(project, std::slice::from_ref(library), policy)
}

pub fn resolve_swc_dependencies(
    project: &mut Project,
    libraries: &[SwcLibrary],
    policy: SwcConflictPolicy,
) -> Result<SwcLinkReport, ImportError> {
    if libraries.is_empty() {
        return Ok(SwcLinkReport::default());
    }

    // 1. Collect definitions per library and build dependency graph for topological sort.
    let mut def_providers: BTreeMap<String, Vec<(usize, Option<u64>)>> = BTreeMap::new();
    let mut lib_defs: Vec<BTreeSet<String>> = vec![BTreeSet::new(); libraries.len()];
    let mut lib_deps: Vec<BTreeSet<String>> = vec![BTreeSet::new(); libraries.len()];

    for (idx, lib) in libraries.iter().enumerate() {
        for entry in &lib.catalog.libraries {
            for script in &entry.scripts {
                for def in &script.defs {
                    lib_defs[idx].insert(def.clone());
                    def_providers
                        .entry(def.clone())
                        .or_default()
                        .push((idx, script.mod_time));
                }
                for dep in &script.deps {
                    lib_deps[idx].insert(dep.id.clone());
                }
            }
        }
    }

    // 2. Topological sort: if library A depends on a definition provided by library B, B comes before A.
    let n = libraries.len();
    let mut adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];
    for a in 0..n {
        for dep in &lib_deps[a] {
            if lib_defs[a].contains(dep) {
                continue;
            }
            if let Some(providers) = def_providers.get(dep) {
                for &(b, _) in providers {
                    if b != a && adj[b].insert(a) {
                        in_degree[a] += 1;
                    }
                }
            }
        }
    }

    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        // Pick unvisited node with minimum in_degree, tie-breaking by library name then index.
        let next = (0..n)
            .filter(|&i| !visited[i])
            .min_by(|&a, &b| {
                in_degree[a]
                    .cmp(&in_degree[b])
                    .then_with(|| libraries[a].name.cmp(&libraries[b].name))
                    .then_with(|| a.cmp(&b))
            })
            .unwrap();
        visited[next] = true;
        order.push(next);
        for &target in &adj[next] {
            in_degree[target] = in_degree[target].saturating_sub(1);
        }
    }

    // 3. Resolve conflicts across libraries in load order.
    let mut winning_provider: BTreeMap<String, (usize, Option<u64>)> = BTreeMap::new();
    let mut conflicts = Vec::new();

    for &lib_idx in &order {
        let lib = &libraries[lib_idx];
        let mut mod_by_def = HashMap::new();
        for entry in &lib.catalog.libraries {
            for script in &entry.scripts {
                for def in &script.defs {
                    mod_by_def.insert(def.clone(), script.mod_time);
                }
            }
        }
        for def in &lib_defs[lib_idx] {
            let incoming_mod = mod_by_def.get(def).copied().flatten();
            if let Some(&(existing_idx, existing_mod)) = winning_provider.get(def) {
                if existing_idx == lib_idx {
                    continue;
                }
                let existing_name = libraries[existing_idx].name.clone();
                let incoming_name = lib.name.clone();
                let replace = match policy {
                    SwcConflictPolicy::ErrorOnConflict => {
                        return Err(ImportError::Blocked(format!(
                            "SWC definition conflict on {def:?} between {existing_name:?} and {incoming_name:?}"
                        )));
                    }
                    SwcConflictPolicy::PreferExisting => false,
                    SwcConflictPolicy::PreferNewer => {
                        incoming_mod.unwrap_or(0) > existing_mod.unwrap_or(0)
                    }
                };
                let winner = if replace {
                    winning_provider.insert(def.clone(), (lib_idx, incoming_mod));
                    incoming_name.clone()
                } else {
                    existing_name.clone()
                };
                conflicts.push(SwcVersionConflict {
                    definition: def.clone(),
                    existing_library: existing_name,
                    incoming_library: incoming_name,
                    existing_mod_time: existing_mod,
                    incoming_mod_time: incoming_mod,
                    winner,
                });
            } else {
                winning_provider.insert(def.clone(), (lib_idx, incoming_mod));
            }
        }
    }

    // 4. Link winning symbols, sources, and resources into `project`.
    let mut linked_symbols = 0usize;
    let mut linked_sources = 0usize;
    let mut linked_resources = 0usize;
    let mut linked_definitions_set = BTreeSet::new();

    // Collect existing definitions in `project`
    let mut project_defs: BTreeSet<String> = project
        .symbols
        .iter()
        .filter_map(|s| s.class_name.clone())
        .collect();
    for src in &project.sources {
        let stripped = src
            .path
            .strip_prefix("scripts/")
            .unwrap_or(&src.path)
            .strip_suffix(".as")
            .unwrap_or(&src.path);
        project_defs.insert(stripped.replace('/', "."));
    }

    for &lib_idx in &order {
        let lib = &libraries[lib_idx];
        let prefix = sanitize_swc_prefix(&lib.name, lib_idx);

        // Resources first (remapping IDs and paths if needed)
        let mut resource_id_map: HashMap<String, String> = HashMap::new();
        let existing_res_ids: HashSet<String> =
            project.resources.iter().map(|r| r.id.clone()).collect();
        let mut existing_res_paths: HashSet<String> = project
            .resources
            .iter()
            .map(|r| r.path.to_ascii_lowercase())
            .collect();
        for res in &lib.project.resources {
            if project.resources.len() >= MAX_RESOURCES {
                break;
            }
            let new_id = if existing_res_ids.contains(&res.id) {
                format!("{prefix}-{}", res.id)
            } else {
                res.id.clone()
            };
            resource_id_map.insert(res.id.clone(), new_id.clone());
            let mut cloned = res.clone();
            cloned.id = new_id;
            if existing_res_paths.contains(&cloned.path.to_ascii_lowercase()) {
                cloned.path = format!("{prefix}/{}", cloned.path.trim_start_matches('/'));
            }
            existing_res_paths.insert(cloned.path.to_ascii_lowercase());
            project.resources.push(cloned);
            linked_resources += 1;
        }

        // Non-root symbols
        let mut symbol_id_map: HashMap<String, String> = HashMap::new();
        let existing_sym_ids: HashSet<String> =
            project.symbols.iter().map(|s| s.id.clone()).collect();
        for sym in &lib.project.symbols {
            if sym.id == lib.project.root {
                continue;
            }
            if let Some(cls) = sym.class_name.as_deref()
                && winning_provider
                    .get(cls)
                    .is_some_and(|&(winner_idx, _)| winner_idx != lib_idx)
            {
                continue;
            }
            let new_sym_id = if existing_sym_ids.contains(&sym.id) {
                format!("{prefix}-{}", sym.id)
            } else {
                sym.id.clone()
            };
            symbol_id_map.insert(sym.id.clone(), new_sym_id);
        }

        for sym in &lib.project.symbols {
            if sym.id == lib.project.root {
                continue;
            }
            let Some(new_sym_id) = symbol_id_map.get(&sym.id).cloned() else {
                continue;
            };
            if let Some(cls) = sym.class_name.as_deref() {
                // Remove superseded symbol in `project` if winning_provider selected this library
                project
                    .symbols
                    .retain(|s| s.id == project.root || s.class_name.as_deref() != Some(cls));
                linked_definitions_set.insert(cls.to_owned());
                project_defs.insert(cls.to_owned());
            }
            let mut cloned = sym.clone();
            cloned.id = new_sym_id;
            for (li, layer) in cloned.timeline.layers.iter_mut().enumerate() {
                layer.id = format!("{}-layer-{li}", cloned.id);
                for (ki, key) in layer.keyframes.iter_mut().enumerate() {
                    if let Some(snd) = &mut key.sound
                        && let Some(mapped_res) = resource_id_map.get(&snd.resource)
                    {
                        snd.resource = mapped_res.clone();
                    }
                    for (ei, el) in key.elements.iter_mut().enumerate() {
                        el.id = format!("{}-el-{li}-{ki}-{ei}", cloned.id);
                        match &mut el.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                if let Some(mapped_sym) = symbol_id_map.get(symbol_id) {
                                    *symbol_id = mapped_sym.clone();
                                }
                            }
                            ElementKind::Bitmap { resource_id, .. } => {
                                if let Some(mapped_res) = resource_id_map.get(resource_id) {
                                    *resource_id = mapped_res.clone();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            project.symbols.push(cloned);
            linked_symbols += 1;
        }

        // Sources
        for src in &lib.project.sources {
            if project.sources.len() >= 1024 {
                break;
            }
            let stripped = src
                .path
                .strip_prefix("scripts/")
                .unwrap_or(&src.path)
                .strip_suffix(".as")
                .unwrap_or(&src.path);
            let def_name = stripped.replace('/', ".");
            if winning_provider
                .get(&def_name)
                .is_some_and(|&(winner_idx, _)| winner_idx != lib_idx)
            {
                continue;
            }
            project.sources.retain(|s| s.path != src.path);
            let mut cloned = src.clone();
            cloned.id = format!("{prefix}-src-{}", project.sources.len() + 1);
            project.sources.push(cloned);
            linked_definitions_set.insert(def_name.clone());
            project_defs.insert(def_name);
            linked_sources += 1;
        }
    }

    let mut all_deps = BTreeSet::new();
    for deps in &lib_deps {
        for dep in deps {
            all_deps.insert(dep.clone());
        }
    }
    let mut resolved_dependencies = Vec::new();
    let mut unresolved_dependencies = Vec::new();
    for dep in all_deps {
        if project_defs.contains(&dep) || is_flash_builtin_definition(&dep) {
            resolved_dependencies.push(dep);
        } else {
            unresolved_dependencies.push(dep);
        }
    }

    upgrade_format_version_for_swc(project);

    Ok(SwcLinkReport {
        linked_definitions: linked_definitions_set.into_iter().collect(),
        linked_symbols,
        linked_sources,
        linked_resources,
        resolved_dependencies,
        unresolved_dependencies,
        conflicts,
        library_load_order: order
            .into_iter()
            .map(|i| libraries[i].name.clone())
            .collect(),
    })
}

fn sanitize_swc_prefix(name: &str, idx: usize) -> String {
    let mut clean = String::new();
    for ch in name.chars().take(32) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            clean.push(ch);
        } else {
            clean.push('-');
        }
    }
    let trimmed = clean.trim_matches('-');
    if trimmed.is_empty() {
        format!("swc-{idx}")
    } else {
        format!("swc-{idx}-{trimmed}")
    }
}
