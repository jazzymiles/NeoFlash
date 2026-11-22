use self::media::{looks_like_mp3 as looks_like_xfl_mp3, wav_duration_44 as parse_wav_duration_44};
use crate::{ImportReport, Statistics, source};
use neoflash_core::*;
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

mod assets;
mod custom_ease;
mod elements;
mod filters;
mod ik;
pub(crate) mod media;
mod publish;
mod shape;
#[cfg(test)]
mod tests;
mod timeline;

use self::timeline::*;

pub(crate) fn convert(
    files: &BTreeMap<String, String>,
    binary_files: &BTreeMap<String, Vec<u8>>,
    report: &mut ImportReport,
) -> Option<Project> {
    let xml = files.get("DOMDocument.xml")?;
    let main = parse(xml, "DOMDocument.xml", report)?;
    let root = main.root_element();
    if root.tag_name().name() != "DOMDocument" {
        report.error(
            "invalid_root",
            "DOMDocument.xml",
            "Expected DOMDocument root.",
        );
        return None;
    }
    report.version = root.attribute("xflVersion").map(str::to_owned);
    // NeoFlash's first XFL exporter wrote filter strength multiplied by 100 and
    // did not identify its generator. Unmarked files are therefore ambiguous, so
    // preserve their previous behavior instead of changing old exports' appearance.
    // Adobe-authored documents and current NeoFlash exports carry creatorInfo and
    // use the standard direct XFL value.
    let direct_filter_strength = uses_direct_filter_strength(root);
    let has_publish_settings = files.contains_key("PublishSettings.xml");
    let publish = files
        .get("PublishSettings.xml")
        .and_then(|text| parse(text, "PublishSettings.xml", report))
        .map(|doc| publish::read(&doc, report))
        .unwrap_or_default();

    let mut parser = Parser {
        report,
        symbols: HashMap::new(),
        bitmaps: HashMap::new(),
        sounds: HashMap::new(),
        videos: HashMap::new(),
        font_items: HashMap::new(),
        extra_symbols: Vec::new(),
        resources: Vec::new(),
        motion_pose_budget: crate::motion_xml::DOCUMENT_POSE_BUDGET,
        resource_bytes_total: 0,
        language: publish.language,
        has_publish_settings,
        include_hidden_layers: publish.include_hidden_layers,
        text_warning: false,
        direct_filter_strength,
        legacy_filter_strength_warning: false,
        pending_bitmap_fills: Vec::new(),
        pending_video_resource: None,
        stage_size: [550.0, 400.0],
        view_angle_3d: None,
        vanishing_point_3d: None,
    };
    parser.audit(
        root,
        "DOMDocument.xml",
        &[
            "width",
            "height",
            "frameRate",
            "backgroundColor",
            "xflVersion",
            "creatorInfo",
            "platformVersion",
            "platform",
            "versionInfo",
            "majorVersion",
            "minorVersion",
            "buildNumber",
            "currentTimeline",
            "nextSceneIdentifier",
            "playOptionsPlayLoop",
            "playOptionsPlayPages",
            "playOptionsPlayFrameActions",
            "autoSaveHasPrompted",
            "autoSaveEnabled",
            "filetypeGUID",
            "fileGUID",
            "objectsSnapTo",
            "viewAngle3D",
            "gridColor",
            "guidesColor",
            "gridSpacingX",
            "gridSpacingY",
            "snapAlignBorderSpacing",
            "previewMode",
            "gridSnapTo",
            "snapAlignHorizontalSpacing",
            "snapAlignVerticalSpacing",
            "snapAlign",
            "pixelSnap",
            "vanishingPoint3DX",
            "vanishingPoint3DY",
            "timelineLabelWidth",
            "viewOptionsPasteBoardView",
            "guidesVisible",
            "gridVisible",
            "guidesSnapTo",
            "sharedLibraryURL",
            "breakPointCount",
            "tabOrderMode",
            "rulerVisible",
            "gridSnapAccuracy",
            "guidesSnapAccuracy",
            "timelineHeight",
            "useCarbonLineSpacing",
            "viewOptionsAnimationControlVisible",
            "viewOptionsButtonsActive",
            "guidesLocked",
            "snapAlignHorizontalCenter",
            "snapAlignVerticalCenter",
            "rulerUnitType",
            "accName",
            "description",
            "autoLabeling",
            "hasAccessibleData",
            "forceSimple",
            "silent",
            "docType",
            "documentClass",
        ],
        &[
            "timelines",
            "symbols",
            "folders",
            "swatchLists",
            "swatches",
            "extendedSwatchLists",
            "media",
            "fonts",
            "PrinterSettings",
            "publishHistory",
            "scripts",
            "persistentData",
            "documentPersistentData",
            "swcCache",
        ],
    );
    if root.attribute("platform").is_some() {
        parser.report.warning("editor_metadata", "DOMDocument.xml", "Authoring platform metadata is not retained; source artwork and scripts are unaffected.");
    }
    if let Some(printer) = child(root, "PrinterSettings") {
        parser.audit(
            printer,
            "DOMDocument.xml",
            &["platform", "modeData", "namesData"],
            &[],
        );
        parser.report.warning(
            "editor_metadata",
            "DOMDocument.xml/PrinterSettings",
            "Authoring printer-settings metadata is not retained.",
        );
    }
    if let Some(history) = child(root, "publishHistory") {
        parser.audit(history, "DOMDocument.xml", &[], &["PublishItem"]);
        for item in elements(history).filter(|n| n.has_tag_name("PublishItem")) {
            parser.audit(
                item,
                "DOMDocument.xml",
                &["publishSize", "publishTime", "publishDebug"],
                &[],
            );
            let _ = parser.boolean(item, "publishDebug", false, "DOMDocument.xml");
            for attribute in ["publishSize", "publishTime"] {
                if item
                    .attribute(attribute)
                    .is_some_and(|value| value.parse::<u64>().is_err())
                {
                    parser.report.error(
                        "invalid_publish_history",
                        parser.location(item, "DOMDocument.xml"),
                        format!("{attribute} must be an unsigned integer."),
                    );
                }
            }
        }
        parser.report.warning("editor_metadata", "DOMDocument.xml/publishHistory", "Previous Flash publish timestamps and output sizes are not retained; authored content is unchanged.");
    }
    if let Some(scripts_meta) = child(root, "scripts") {
        parser.audit(scripts_meta, "DOMDocument.xml", &[], &["Include"]);
        for inc in elements(scripts_meta).filter(|n| n.has_tag_name("Include")) {
            parser.audit(
                inc,
                "DOMDocument.xml",
                &[
                    "href",
                    "itemID",
                    "lastModified",
                    "loadImmediate",
                    "itemIcon",
                ],
                &[],
            );
        }
        parser.report.warning(
            "editor_metadata",
            "DOMDocument.xml/scripts",
            "Authoring script-navigator entries are not retained separately; bundled .as files are imported directly.",
        );
    }
    for pd_tag in ["persistentData", "documentPersistentData"] {
        if let Some(pd_node) = child(root, pd_tag) {
            parser.audit_persistent_data(pd_node, "DOMDocument.xml");
        }
    }
    if let Some(swc_cache) = child(root, "swcCache") {
        parser.audit(swc_cache, "DOMDocument.xml", &[], &["Swc"]);
        for swc in elements(swc_cache).filter(|n| n.has_tag_name("Swc")) {
            parser.audit(
                swc,
                "DOMDocument.xml",
                &["hashKey", "href"],
                &["classDefinitions"],
            );
            if let Some(defs) = child(swc, "classDefinitions") {
                parser.audit(defs, "DOMDocument.xml", &[], &["ClassDefinition"]);
                for def in elements(defs).filter(|n| n.has_tag_name("ClassDefinition")) {
                    parser.audit(def, "DOMDocument.xml", &["value"], &[]);
                }
            }
        }
        parser.report.warning(
            "editor_metadata",
            "DOMDocument.xml/swcCache",
            "Cached SWC class-definition index is not retained.",
        );
    }
    if let Some(media) = child(root, "media") {
        let video_previews = main
            .root()
            .children()
            .filter_map(|child| child.pi())
            .any(|pi| pi.target == "neoflash-video-frame-previews" && pi.value == Some("v1"));
        parser.parse_media(media, binary_files, video_previews);
        let count = parser
            .bitmaps
            .values()
            .filter(|bitmap| bitmap.source_scale != [1.0, 1.0])
            .count();
        if count > 0 {
            parser.report.warning("xfl_video_frame_previews", "DOMDocument.xml/media", format!("Imported {count} video frame images at preview resolution to fit the decoded-image budget. Stage size and frame timing are preserved. The source package contains the full-resolution images."));
        }
    }
    if let Some(assets) = child(root, "fonts") {
        parser.audit(assets, "DOMDocument.xml", &[], &["DOMFontItem"]);
        for item in elements(assets) {
            if item.has_tag_name("DOMFontItem") {
                parser.parse_font_item(item);
            } else {
                parser.report.error(
                    "unsupported_assets",
                    parser.location(item, "DOMDocument.xml"),
                    "Unsupported font library asset is not imported yet.",
                );
            }
        }
    }
    let mut declared_folders = Vec::new();
    if let Some(folders_node) = child(root, "folders") {
        let first_diagnostic = parser.report.diagnostics.len();
        parser.audit(folders_node, "DOMDocument.xml", &[], &["DOMFolderItem"]);
        let items: Vec<_> = elements(folders_node)
            .filter(|n| n.has_tag_name("DOMFolderItem"))
            .collect();
        if items.is_empty() {
            if !parser.report.diagnostics[first_diagnostic..]
                .iter()
                .any(|d| d.severity == crate::Severity::Error)
            {
                parser.report.warning(
                    "editor_metadata",
                    "DOMDocument.xml/folders",
                    "Empty library folder/swatch marker is not retained; nonempty organization requires a dedicated importer.",
                );
            }
        } else {
            let mut seen_folders = BTreeSet::new();
            for item in items {
                parser.audit(
                    item,
                    "DOMDocument.xml",
                    &["name", "itemID", "isExpanded"],
                    &[],
                );
                parser.boolean(item, "isExpanded", false, "DOMDocument.xml");
                if item.attribute("itemID").is_some() {
                    parser.report.warning(
                        "editor_metadata",
                        parser.location(item, "DOMDocument.xml"),
                        "Flash library folder itemID is not retained.",
                    );
                }
                let raw_folder = item.attribute("name").unwrap_or("");
                let folder_name = normalize_xfl_folder_path(raw_folder);
                if folder_name.is_empty()
                    || !valid_library_folder(&folder_name)
                    || !seen_folders.insert(folder_name.clone())
                {
                    parser.report.error(
                        "invalid_library_folder",
                        parser.location(item, "DOMDocument.xml"),
                        "Library folder paths must be unique, nonempty, normalized NeoFlash folder paths.",
                    );
                } else {
                    declared_folders.push(folder_name);
                }
            }
        }
    }
    for tag in ["swatchLists", "swatches", "extendedSwatchLists"] {
        if let Some(metadata) = child(root, tag) {
            parser.audit_swatch_container(metadata, tag, "DOMDocument.xml");
        }
    }
    let mut libraries = vec![];
    if let Some(symbols) = child(root, "symbols") {
        parser.audit(symbols, "DOMDocument.xml", &[], &["Include"]);
        for include in elements(symbols) {
            parser.audit(
                include,
                "DOMDocument.xml",
                &[
                    "href",
                    "loadImmediate",
                    "itemIcon",
                    "lastModified",
                    "itemID",
                ],
                &[],
            );
            if include.attribute("itemID").is_some() {
                parser.report.warning("editor_metadata", parser.location(include, "DOMDocument.xml"), "Flash library itemID is not retained; symbol references resolve by the original library item name.");
            }
            let href = include.attribute("href").unwrap_or("");
            if let Err(error) = source::safe_name(href) {
                parser
                    .report
                    .error("unsafe_reference", "DOMDocument.xml", error.to_string());
                continue;
            }
            let path = format!("LIBRARY/{href}");
            let Some(xml) = get_file_ci(files, &path) else {
                parser.report.error(
                    "missing_symbol",
                    &path,
                    "Referenced library XML file is missing.",
                );
                continue;
            };
            let Some(doc) = parse(xml, &path, parser.report) else {
                continue;
            };
            let symbol = doc.root_element();
            if !matches!(
                symbol.tag_name().name(),
                "DOMSymbolItem" | "DOMComponentItem"
            ) {
                parser
                    .report
                    .error("invalid_symbol", &path, "Expected DOMSymbolItem.");
                continue;
            }
            let name = symbol.attribute("name").unwrap_or("");
            let has_linkage = symbol.attribute("linkageExportForAS") == Some("true")
                || symbol
                    .attribute("linkageIdentifier")
                    .is_some_and(|s| !s.trim().is_empty())
                || symbol
                    .attribute("linkageClassName")
                    .is_some_and(|s| !s.trim().is_empty());
            let pre_kind = if symbol.has_tag_name("DOMComponentItem") {
                SymbolKind::MovieClip
            } else {
                match symbol.attribute("symbolType").unwrap_or("movie clip") {
                    "graphic" if has_linkage => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => SymbolKind::MovieClip,
                }
            };
            if name.is_empty()
                || parser
                    .symbols
                    .insert(name.into(), (new_id("symbol"), pre_kind))
                    .is_some()
            {
                parser.report.error(
                    "symbol_identity",
                    &path,
                    "Library symbol names must be nonempty and unique.",
                );
            }
            libraries.push((path, doc));
        }
    }
    let mut p = Project::blank();
    p.name = "Imported XFL".into();
    p.width = parser.stage_dimension(root, "width", 550, 65535, "DOMDocument.xml");
    p.height = parser.stage_dimension(root, "height", 400, 65535, "DOMDocument.xml");
    parser.stage_size = [f64::from(p.width), f64::from(p.height)];
    parser.view_angle_3d = root
        .attribute("viewAngle3D")
        .map(|_| parser.number(root, "viewAngle3D", 55.0, "DOMDocument.xml"));
    parser.vanishing_point_3d = match (
        root.attribute("vanishingPoint3DX"),
        root.attribute("vanishingPoint3DY"),
    ) {
        (Some(_), Some(_)) => Some([
            parser.number(
                root,
                "vanishingPoint3DX",
                f64::from(p.width) * 0.5,
                "DOMDocument.xml",
            ),
            parser.number(
                root,
                "vanishingPoint3DY",
                f64::from(p.height) * 0.5,
                "DOMDocument.xml",
            ),
        ]),
        _ => None,
    };
    p.fps = parser.stage_dimension(root, "frameRate", 24, 1000, "DOMDocument.xml");
    p.background = parser.color(
        root.attribute("backgroundColor").unwrap_or("#FFFFFF"),
        1.,
        "DOMDocument.xml",
    );
    p.library_folders = declared_folders;
    let Some(timelines) = child(root, "timelines") else {
        parser.report.error(
            "missing_timeline",
            "DOMDocument.xml",
            "No document timeline.",
        );
        return None;
    };
    parser.audit(timelines, "DOMDocument.xml", &[], &["DOMTimeline"]);
    let scenes: Vec<_> = elements(timelines)
        .filter(|n| n.has_tag_name("DOMTimeline"))
        .collect();
    if scenes.is_empty() {
        parser.report.error(
            "missing_timeline",
            "DOMDocument.xml",
            "No document timeline.",
        );
        return None;
    }
    p.symbols.clear();
    p.symbols.push(parser.symbol(
        scenes[0],
        p.root.clone(),
        SymbolKind::MovieClip,
        "DOMDocument.xml",
    ));
    if scenes.len() > 1 {
        p.scenes.push(p.root.clone());
        for (idx, scene_node) in scenes.iter().enumerate().skip(1) {
            let scene_id = new_id("symbol");
            let mut scene_symbol = parser.symbol(
                *scene_node,
                scene_id.clone(),
                SymbolKind::MovieClip,
                "DOMDocument.xml",
            );
            if scene_node.attribute("name").is_none() {
                scene_symbol.name = format!("Scene {}", idx + 1);
            }
            p.scenes.push(scene_id);
            p.symbols.push(scene_symbol);
        }
    }
    let mut used_export_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut used_exported_classes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for (path, doc) in &libraries {
        let node = doc.root_element();
        let name = node.attribute("name").unwrap_or("");
        let (symbol_folder, display_name) = if let Some(explicit) = node.attribute("folder") {
            let norm_explicit = normalize_xfl_folder_path(explicit);
            if !valid_library_folder(&norm_explicit) {
                parser.report.error(
                    "invalid_library_folder",
                    path,
                    "Symbol folder attribute must be a valid normalized NeoFlash library folder path.",
                );
                (String::new(), name.to_owned())
            } else {
                let base = name.rsplit_once('/').map_or(name, |(_, b)| b);
                (norm_explicit, base.to_owned())
            }
        } else if let Some((prefix, base)) = name.rsplit_once('/') {
            let norm_prefix = normalize_xfl_folder_path(prefix);
            if norm_prefix.is_empty() || base.is_empty() || !valid_library_folder(&norm_prefix) {
                parser.report.error(
                    "invalid_library_folder",
                    path,
                    "Slash-separated symbol name does not form a valid normalized library folder path.",
                );
                (String::new(), name.to_owned())
            } else {
                (norm_prefix, base.to_owned())
            }
        } else {
            (String::new(), name.to_owned())
        };
        if !symbol_folder.is_empty() && !p.library_folders.contains(&symbol_folder) {
            p.library_folders.push(symbol_folder.clone());
        }
        let id = parser
            .symbols
            .get(name)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| new_id("symbol"));
        if node.has_tag_name("DOMComponentItem") {
            let symbol = parser.parse_component_item(
                node,
                id,
                symbol_folder,
                display_name,
                path,
                &mut used_export_ids,
                &mut used_exported_classes,
            );
            p.symbols.push(symbol);
            continue;
        }
        parser.audit(
            node,
            path,
            &[
                "name",
                "folder",
                "itemID",
                "symbolType",
                "lastModified",
                "sourceFlashFilepath",
                "sourceLibraryItemHRef",
                "sourceLastModified",
                "sourceItemID",
                "sourceAutoUpdate",
                "scaleGridLeft",
                "scaleGridRight",
                "scaleGridTop",
                "scaleGridBottom",
                "itemIcon",
                "lastUniqueIdentifier",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
                "isSpriteSubclass",
            ],
            &["timeline", "persistentData"],
        );
        if let Some(pd_node) = child(node, "persistentData") {
            parser.audit_persistent_data(pd_node, path);
        }
        let mut has_scale_grid = false;
        for attr in [
            "scaleGridLeft",
            "scaleGridRight",
            "scaleGridTop",
            "scaleGridBottom",
        ] {
            if node.attribute(attr).is_some() {
                parser.number(node, attr, 0.0, path);
                has_scale_grid = true;
            }
        }
        if has_scale_grid {
            parser.report.warning(
                "editor_metadata",
                path,
                "9-slice scaleGrid guides on DOMSymbolItem are not retained yet.",
            );
        }
        if node.attribute("sourceAutoUpdate") == Some("true") {
            parser.report.error(
                "external_symbol",
                path,
                "External auto-updating symbols are not supported.",
            );
        }
        let kind = match node.attribute("symbolType").unwrap_or("movie clip") {
            "graphic" => SymbolKind::Graphic,
            "movie clip" => SymbolKind::MovieClip,
            "button" => SymbolKind::Button,
            "ik container" => {
                parser.report.warning(
                    "xfl_ik_approximated",
                    path,
                    "IK container symbol imported as a MovieClip; inverse-kinematics bone constraints are not evaluated.",
                );
                SymbolKind::MovieClip
            }
            other => {
                parser.report.error(
                    "symbol_type",
                    path,
                    format!("Symbol kind {other} is not supported yet."),
                );
                SymbolKind::MovieClip
            }
        };
        let timeline = child(node, "timeline").and_then(|wrapper| {
            parser.audit(wrapper, path, &[], &["DOMTimeline"]);
            if elements(wrapper)
                .filter(|n| n.has_tag_name("DOMTimeline"))
                .count()
                != 1
            {
                parser.report.error(
                    "symbol_timeline_count",
                    path,
                    "A symbol must contain exactly one timeline.",
                );
            }
            child(wrapper, "DOMTimeline")
        });
        let Some(timeline) = timeline else {
            parser.report.error(
                "missing_timeline",
                path,
                "Library symbol has no DOMTimeline.",
            );
            continue;
        };
        let mut symbol = parser.symbol(timeline, id, kind, path);
        symbol.folder = symbol_folder;
        symbol.name = display_name;
        parser.apply_symbol_linkage(
            &mut symbol,
            node,
            path,
            &mut used_export_ids,
            &mut used_exported_classes,
        );
        p.symbols.push(symbol);
    }
    p.symbols.extend(parser.extra_symbols);
    let doc_class_candidate = publish
        .document_class
        .map(|s| (s, "PublishSettings.xml"))
        .or_else(|| {
            root.attribute("documentClass")
                .map(|s| (s.to_owned(), "DOMDocument.xml"))
        });
    if let Some((raw_doc_class, doc_class_loc)) = doc_class_candidate {
        let trimmed = raw_doc_class.trim();
        if !trimmed.is_empty() {
            if is_valid_qualified_class_name(trimmed) {
                p.document_class = Some(trimmed.to_owned());
            } else {
                parser.report.warning(
                    "xfl_invalid_document_class",
                    doc_class_loc,
                    format!(
                        "DocumentClass {trimmed:?} is not a valid qualified ActionScript identifier and was omitted."
                    ),
                );
            }
        }
    }
    import_bundled_as_sources(files, &mut p, parser.language, parser.report);
    if p.has_program() {
        p.enable_program();
    }
    normalize_xfl_playback(&mut p.symbols, parser.report);
    normalize_xfl_masks(&mut p, parser.report);
    p.resources = parser.resources;
    if !p.resources.is_empty() {
        p.format_version = p.format_version.max(3);
    }
    if !p.library_folders.is_empty()
        || p.symbols.iter().any(|s| {
            !s.folder.is_empty() || s.timeline.layers.iter().any(|l| !l.properties.is_default())
        })
    {
        p.format_version = p.format_version.max(4);
    }
    if p.symbols.iter().any(|s| {
        s.timeline.layers.iter().any(|l| {
            l.properties.end_frame.is_some()
                || l.keyframes.iter().any(|k| {
                    matches!(k.tween, Some(Easing::Flash { .. }))
                        || k.elements.iter().any(|e| {
                            e.transform.skew != 0.0
                                || !e.transform.color_transform.is_identity()
                                || matches!(e.kind, ElementKind::PaintedPath { .. })
                        })
                })
        })
    }) {
        p.format_version = p.format_version.max(5);
    }
    p.script_language = Some(parser.language.unwrap_or_else(|| p.infer_script_language()));
    p.also_compile_compatible_scripts = true;
    p.upgrade_format_for_features();
    for issue in validate(&p) {
        parser
            .report
            .error("invalid_document", issue.path, issue.message);
    }
    parser.report.statistics = Statistics {
        symbols: p.symbols.len(),
        layers: p.symbols.iter().map(|s| s.timeline.layers.len()).sum(),
        keyframes: p
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .map(|l| l.keyframes.len())
            .sum(),
        elements: p
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .map(|k| k.elements.len())
            .sum(),
        scripts: p
            .symbols
            .iter()
            .map(|s| {
                s.timeline.scripts.len()
                    + s.timeline
                        .layers
                        .iter()
                        .flat_map(|l| &l.keyframes)
                        .flat_map(|k| &k.elements)
                        .map(|e| e.scripts.len())
                        .sum::<usize>()
            })
            .sum::<usize>()
            + p.sources.len(),
    };
    Some(p)
}

fn publish_value(node: Node<'_, '_>, report: &mut ImportReport) -> Option<String> {
    let location = format!(
        "PublishSettings.xml:{} <{}>",
        node.document().text_pos_at(node.range().start).row,
        node.tag_name().name()
    );
    if node.tag_name().namespace().is_some()
        || node.attributes().len() != 0
        || elements(node).next().is_some()
    {
        report.error("invalid_script_configuration", location, "Language and script configuration fields must be unnamespaced plain text without attributes; nested content is not ignored.");
        return None;
    }
    // Comments can split a value into several text nodes. Never inspect just the
    // first fragment and mistake an active/ambiguous setting for an AS2 default.
    Some(
        node.children()
            .filter(Node::is_text)
            .filter_map(|n| n.text())
            .collect::<String>()
            .trim()
            .to_owned(),
    )
}

fn publish_language(
    properties: Node<'_, '_>,
    report: &mut ImportReport,
) -> (Option<ScriptLanguage>, Option<String>) {
    report.warning("publish_settings", "PublishSettings.xml", "ActionScriptVersion and hidden-layer publishing are read from the current profile; other Flash publishing options are not carried into HTML5 export.");
    let mut versions = std::collections::BTreeSet::new();
    let mut valid = true;
    for node in properties
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "ActionScriptVersion")
    {
        if node.parent() != Some(properties) {
            report.error("invalid_script_configuration", "PublishSettings.xml", "ActionScriptVersion must be a direct child of the selected PublishFlashProperties container.");
            valid = false;
        }
        match publish_value(node, report) {
            Some(value) => {
                versions.insert(value);
            }
            None => valid = false,
        }
    }
    let language = if valid && versions.len() == 1 {
        match versions.iter().next().map(String::as_str) {
            Some("1") => Some(ScriptLanguage::ActionScript1),
            Some("2") => Some(ScriptLanguage::ActionScript2),
            Some("3") => Some(ScriptLanguage::ActionScript3),
            _ => None,
        }
    } else {
        None
    };
    if language.is_none() {
        report.error(
            "script_language",
            "PublishSettings.xml",
            "Publish settings must identify one unambiguous ActionScriptVersion (1, 2 or 3).",
        );
    }
    let mut document_class = None;
    for node in properties.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        if !matches!(
            name,
            "DocumentClass"
                | "AS3DocumentClass"
                | "AS3PackagePaths"
                | "PackagePaths"
                | "AS3LibraryPaths"
                | "AS3ExternalLibraryPaths"
                | "AS3ConfigConst"
        ) {
            continue;
        }
        if node.parent() != Some(properties) {
            report.error("invalid_script_configuration", "PublishSettings.xml", format!("{name} must be a direct child of the selected PublishFlashProperties container."));
        }
        let Some(value) = publish_value(node, report) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        match language {
            None => {
                report.error(
                    "external_script_configuration",
                    "PublishSettings.xml",
                    format!("Nonempty {name} cannot be interpreted when ActionScriptVersion is ambiguous or missing."),
                );
            }
            Some(lang) if lang != ScriptLanguage::ActionScript3 && name.starts_with("AS3") => {
                report.warning(
                    "inactive_script_configuration",
                    "PublishSettings.xml",
                    format!(
                        "{name} is inactive because ActionScriptVersion is not 3; the frame source is retained and the original publish settings are unchanged."
                    ),
                );
            }
            Some(ScriptLanguage::ActionScript3)
                if matches!(name, "DocumentClass" | "AS3DocumentClass") =>
            {
                document_class = Some(value.clone());
                report.warning(
                    "external_script_configuration",
                    "PublishSettings.xml",
                    format!(
                        "Document class {value:?} from {name} is retained; bundled .as sources in the XFL folder/archive are imported, while external filesystem classpaths are not read."
                    ),
                );
            }
            Some(_) => {
                report.warning(
                    "external_script_configuration",
                    "PublishSettings.xml",
                    format!(
                        "Nonempty {name} is approximated: bundled .as files inside the XFL folder/archive are imported, while external filesystem classpaths and SWC libraries are not resolved."
                    ),
                );
            }
        }
    }
    (language, document_class)
}

fn is_tlf_namespace_element(tag: &str, namespace: &str) -> bool {
    namespace == "http://ns.adobe.com/textLayout/2008"
        && matches!(
            tag,
            "TextFlow"
                | "p"
                | "span"
                | "br"
                | "div"
                | "a"
                | "tab"
                | "tcy"
                | "g"
                | "img"
                | "format"
                | "ListElement"
                | "ListItemElement"
        )
}

fn parse<'a>(xml: &'a str, path: &str, report: &mut ImportReport) -> Option<Document<'a>> {
    match Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    ) {
        Ok(doc) => {
            // Local-name lookup deliberately supports Adobe's default namespace
            // and the original unnamespaced fixtures. Validate every node first,
            // including empty asset/metadata containers that need no conversion.
            for node in doc.descendants().filter(Node::is_element) {
                if node.tag_name().namespace().is_some_and(|namespace| {
                    path == "PublishSettings.xml"
                        || (namespace != "http://ns.adobe.com/xfl/2008/"
                            && !is_tlf_namespace_element(node.tag_name().name(), namespace)
                            && !is_fl_motion_namespace_element(node, namespace))
                }) {
                    report.error(
                        "unknown_namespace",
                        format!("{path}:{} <{}>", doc.text_pos_at(node.range().start).row, node.tag_name().name()),
                        "Unknown XML element namespace; matching an XFL local name does not establish supported semantics.",
                    );
                }
            }
            Some(doc)
        }
        Err(error) => {
            report.error("xml_parse", path, error.to_string());
            None
        }
    }
}

fn elements<'a, 'b>(node: Node<'a, 'b>) -> impl Iterator<Item = Node<'a, 'b>> {
    node.children().filter(Node::is_element)
}

fn normalize_filter_strength(raw: f64, direct: bool) -> f64 {
    (if direct { raw } else { raw / 100.0 }).clamp(0.0, 255.0)
}

fn uses_direct_filter_strength(root: Node<'_, '_>) -> bool {
    root.attribute("creatorInfo").is_some()
}

fn child<'a, 'b>(node: Node<'a, 'b>, tag: &str) -> Option<Node<'a, 'b>> {
    elements(node).find(|n| n.tag_name().name() == tag)
}

fn immediate_text(node: Node<'_, '_>) -> String {
    // XML comments and processing instructions can divide a logical text value.
    // Include all immediate text/CDATA fragments; nested elements are rejected
    // independently by audit and must not be flattened into authored content.
    node.children()
        .filter(Node::is_text)
        .filter_map(|n| n.text())
        .collect()
}

#[derive(Clone, Debug)]
struct XflBitmapAsset {
    resource_id: Id,
    average_color: Color,
    width: u32,
    height: u32,
    /// Full-resolution XFL pixels per imported bitmap pixel on each axis.
    source_scale: [f64; 2],
    smoothing: bool,
}

#[derive(Clone, Debug)]
struct XflSoundAsset {
    resource_id: Id,
    duration_44: u64,
}

#[derive(Clone, Debug)]
struct XflVideoAsset {
    width: f64,
    height: f64,
    resource_id: Option<Id>,
    sync_video: bool,
    import_audio: bool,
}

struct Parser<'a> {
    report: &'a mut ImportReport,
    symbols: HashMap<String, (Id, SymbolKind)>,
    bitmaps: HashMap<String, XflBitmapAsset>,
    sounds: HashMap<String, XflSoundAsset>,
    videos: HashMap<String, XflVideoAsset>,
    font_items: HashMap<String, String>,
    extra_symbols: Vec<Symbol>,
    resources: Vec<ResourceUnit>,
    resource_bytes_total: usize,
    motion_pose_budget: usize,
    language: Option<ScriptLanguage>,
    has_publish_settings: bool,
    include_hidden_layers: bool,
    text_warning: bool,
    direct_filter_strength: bool,
    legacy_filter_strength_warning: bool,
    pending_bitmap_fills: Vec<AuthoredBitmapFill>,
    pending_video_resource: Option<(Id, bool, bool)>,
    stage_size: [f64; 2],
    view_angle_3d: Option<f64>,
    vanishing_point_3d: Option<[f64; 2]>,
}

impl Parser<'_> {
    fn location(&self, node: Node<'_, '_>, file: &str) -> String {
        format!(
            "{file}:{} <{}>",
            node.document().text_pos_at(node.range().start).row,
            node.tag_name().name()
        )
    }

    fn audit(&mut self, node: Node<'_, '_>, file: &str, attrs: &[&str], children: &[&str]) {
        let location = self.location(node, file);
        if node.tag_name().namespace().is_some_and(|ns| {
            ns != "http://ns.adobe.com/xfl/2008/" && !is_fl_motion_namespace_element(node, ns)
        }) {
            self.report
                .error("unknown_namespace", &location, "Unknown XML namespace.");
        }
        for attr in node.attributes() {
            if let Some(namespace) = attr.namespace() {
                self.report.error(
                    "unsupported_attribute_namespace",
                    &location,
                    format!(
                        "Namespaced attribute {{{namespace}}}{} is unsupported; XFL attributes must have no namespace.",
                        attr.name()
                    ),
                );
                continue;
            }
            if !attrs.contains(&attr.name()) {
                self.report.error(
                    "unsupported_attribute",
                    &location,
                    format!(
                        "Attribute {}={:?} is not represented in the current document model.",
                        attr.name(),
                        attr.value()
                    ),
                );
            }
        }
        if !matches!(node.tag_name().name(), "script" | "characters")
            && node
                .children()
                .any(|n| n.is_text() && n.text().is_some_and(|text| !text.trim().is_empty()))
        {
            self.report.error(
                "unexpected_text",
                &location,
                "Unexpected text content in a structural XML node.",
            );
        }
        for item in elements(node) {
            if !children.contains(&item.tag_name().name()) {
                self.report.error(
                    "unsupported_element",
                    self.location(item, file),
                    "This XFL content is not supported; import is blocked to avoid dropping it.",
                );
            }
        }
        for tag in children {
            if !matches!(
                *tag,
                "Include"
                    | "DOMFolderItem"
                    | "DOMBitmapItem"
                    | "DOMSoundItem"
                    | "DOMVideoItem"
                    | "DOMCompiledClipItem"
                    | "DOMFontItem"
                    | "DOMLayer"
                    | "DOMFrame"
                    | "DOMTimeline"
                    | "DOMSymbolInstance"
                    | "DOMBitmapInstance"
                    | "DOMStaticText"
                    | "DOMDynamicText"
                    | "DOMInputText"
                    | "DOMTLFText"
                    | "DOMGroup"
                    | "DOMVideoInstance"
                    | "DOMCompiledClipInstance"
                    | "DOMComponentInstance"
                    | "DOMRectangleObject"
                    | "DOMOvalObject"
                    | "DOMShape"
                    | "DOMTextRun"
                    | "Edge"
                    | "FillStyle"
                    | "StrokeStyle"
                    | "GradientEntry"
                    | "SoundEnvelopePoint"
                    | "PublishItem"
                    | "CustomEase"
                    | "Ease"
                    | "Point"
                    | "swatchList"
                    | "SolidSwatchItem"
                    | "LinearGradientSwatchItem"
                    | "RadialGradientSwatchItem"
                    | "Swc"
                    | "ClassDefinition"
                    | "PD"
                    | "pd"
                    | "DropShadowFilter"
                    | "GlowFilter"
                    | "BevelFilter"
                    | "BlurFilter"
                    | "GradientGlowFilter"
                    | "GradientBevelFilter"
                    | "AdjustColorFilter"
                    | "TLFFont"
                    | "ikNode"
                    | "IKNode"
                    | "DOMBone"
                    | "Bone"
            ) && elements(node)
                .filter(|n| n.tag_name().name() == *tag)
                .count()
                > 1
            {
                self.report.error(
                    "duplicate_container",
                    &location,
                    format!("Repeated {tag} container is not valid for this importer."),
                );
            }
        }
    }

    fn number(&mut self, node: Node<'_, '_>, attr: &str, default: f64, file: &str) -> f64 {
        let Some(value) = node.attribute(attr) else {
            return default;
        };
        match value.parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => {
                self.report.error(
                    "invalid_number",
                    self.location(node, file),
                    format!("{attr} must be finite."),
                );
                default
            }
        }
    }

    fn filter_strength(&mut self, node: Node<'_, '_>, file: &str) -> f64 {
        let direct = self.direct_filter_strength;
        let raw = self.number(node, "strength", if direct { 1.0 } else { 100.0 }, file);
        if !direct && node.attribute("strength").is_some() && !self.legacy_filter_strength_warning {
            self.legacy_filter_strength_warning = true;
            let location = self.location(node, file);
            self.report.warning(
                "legacy_neoflash_filter_strength",
                location,
                "Unmarked XFL filter strength is interpreted with the legacy NeoFlash ×100 compatibility scale; resave with current NeoFlash or authoring metadata to use standard direct XFL values.",
            );
        }
        normalize_filter_strength(raw, direct)
    }

    fn boolean(&mut self, node: Node<'_, '_>, attr: &str, default: bool, file: &str) -> bool {
        match node.attribute(attr) {
            None => default,
            Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => {
                self.report.error(
                    "invalid_boolean",
                    self.location(node, file),
                    format!("{attr} must be true/false or 1/0."),
                );
                default
            }
        }
    }

    fn integer(&mut self, node: Node<'_, '_>, attr: &str, default: u32, file: &str) -> u32 {
        let value = self.number(node, attr, default as f64, file);
        if value < 0. || value > 1_000_000. || value.fract() != 0. {
            self.report.error("integer_required",self.location(node,file),format!("{attr} must be a nonnegative integer <=1000000; fractional FPS is not supported yet."));
            default
        } else {
            value as u32
        }
    }

    fn color(&mut self, value: &str, alpha: f64, location: &str) -> Color {
        if !(0.0..=1.0).contains(&alpha) {
            self.report
                .error("invalid_alpha", location, "Color alpha must be in [0,1].");
        }
        let color = value
            .strip_prefix('#')
            .filter(|s| s.len() == 6)
            .and_then(|s| u32::from_str_radix(s, 16).ok());
        if let Some(rgb) = color {
            [
                (rgb >> 16) as u8,
                (rgb >> 8) as u8,
                rgb as u8,
                (alpha.clamp(0., 1.) * 255.).round() as u8,
            ]
        } else {
            self.report.error(
                "invalid_color",
                location,
                format!("Unsupported color {value}"),
            );
            [0, 0, 0, 255]
        }
    }

    fn stage_dimension(
        &mut self,
        node: Node<'_, '_>,
        attr: &str,
        default: u32,
        max_val: u32,
        file: &str,
    ) -> u32 {
        let value = self.number(node, attr, f64::from(default), file);
        if value <= 0.0 || value > f64::from(max_val) {
            self.report.error(
                "integer_required",
                self.location(node, file),
                format!("{attr} must be a positive number <= {max_val}."),
            );
            default
        } else if value.fract() != 0.0 {
            let rounded = (value.round() as u32).clamp(1, max_val);
            self.report.warning(
                "xfl_stage_rounded",
                self.location(node, file),
                format!("Fractional {attr}={value} rounded to {rounded}."),
            );
            rounded
        } else {
            value as u32
        }
    }

    fn audit_persistent_data(&mut self, pd_node: Node<'_, '_>, file: &str) {
        self.audit(pd_node, file, &[], &["PD", "pd"]);
        for pd in elements(pd_node).filter(|n| matches!(n.tag_name().name(), "PD" | "pd")) {
            self.audit(pd, file, &["n", "t", "v"], &[]);
        }
        self.report.warning(
            "editor_metadata",
            format!("{file}/{}", pd_node.tag_name().name()),
            "Authoring persistentData metadata is not retained.",
        );
    }

    fn audit_swatch_container(&mut self, metadata: Node<'_, '_>, tag: &str, file: &str) {
        let first_diagnostic = self.report.diagnostics.len();
        if tag == "swatches" {
            self.audit_swatches_node(metadata, file);
        } else {
            self.audit(metadata, file, &[], &["swatchList"]);
            for sl in elements(metadata).filter(|n| n.has_tag_name("swatchList")) {
                self.audit(sl, file, &[], &["swatches", "swatchListInfo"]);
                if let Some(info) = child(sl, "swatchListInfo") {
                    self.audit(info, file, &["name"], &[]);
                }
                if let Some(sw) = child(sl, "swatches") {
                    self.audit_swatches_node(sw, file);
                }
            }
        }
        if !self.report.diagnostics[first_diagnostic..]
            .iter()
            .any(|d| d.severity == crate::Severity::Error)
        {
            self.report.warning(
                "editor_metadata",
                format!("{file}/{tag}"),
                "Authoring color-swatch palettes are not retained.",
            );
        }
    }

    fn audit_swatches_node(&mut self, sw: Node<'_, '_>, file: &str) {
        self.audit(
            sw,
            file,
            &[],
            &[
                "swatchListInfo",
                "SolidSwatchItem",
                "LinearGradientSwatchItem",
                "RadialGradientSwatchItem",
            ],
        );
        if let Some(info) = child(sw, "swatchListInfo") {
            self.audit(info, file, &["name"], &[]);
        }
        for item in elements(sw) {
            match item.tag_name().name() {
                "SolidSwatchItem" => {
                    self.audit(
                        item,
                        file,
                        &["color", "alpha", "hue", "saturation", "brightness"],
                        &[],
                    );
                }
                "LinearGradientSwatchItem" | "RadialGradientSwatchItem" => {
                    self.audit(
                        item,
                        file,
                        &["spreadMethod", "interpolationMethod", "focalPointRatio"],
                        &["GradientEntry"],
                    );
                    for entry in elements(item).filter(|n| n.has_tag_name("GradientEntry")) {
                        self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_font_item(&mut self, item: Node<'_, '_>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "font",
                "id",
                "size",
                "style",
                "sourceLastImported",
                "embedRanges",
                "embeddedCharacters",
                "bitmapSize",
                "isDefineFont4",
                "lastModified",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[],
        );
        let _ = self.boolean(item, "isDefineFont4", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportForAS", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportInFirstFrame", true, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportForRS", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageImportForRS", false, "DOMDocument.xml");
        if let (Some(name), Some(font)) = (
            item.attribute("name")
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            item.attribute("font")
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)),
        ) {
            self.font_items.insert(name.to_owned(), font.to_owned());
            self.font_items.insert(format!("{name}*"), font.to_owned());
        }
        self.report.warning(
            "xfl_font_item_approximated",
            loc,
            "DOMFontItem embedded glyph outlines are not bundled; text uses the mapped system/CSS font family.",
        );
    }

    fn apply_symbol_linkage(
        &mut self,
        symbol: &mut Symbol,
        node: Node<'_, '_>,
        path: &str,
        used_export_ids: &mut std::collections::HashSet<String>,
        used_exported_classes: &mut std::collections::HashSet<String>,
    ) {
        let export_as = self.boolean(node, "linkageExportForAS", false, path);
        let _ = self.boolean(node, "linkageExportInFirstFrame", true, path);
        let export_rs = self.boolean(node, "linkageExportForRS", false, path);
        let import_rs = self.boolean(node, "linkageImportForRS", false, path);
        let _ = self.boolean(node, "isSpriteSubclass", false, path);
        if export_rs
            || import_rs
            || node
                .attribute("linkageURL")
                .is_some_and(|s| !s.trim().is_empty())
        {
            self.report.warning(
                "xfl_linkage_approximated",
                path,
                "Runtime shared-library linkage (linkageExportForRS/linkageImportForRS/linkageURL) is not fetched externally; local symbol content is retained.",
            );
        }
        let raw_id = node
            .attribute("linkageIdentifier")
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let raw_class = node
            .attribute("linkageClassName")
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if !export_as && raw_id.is_none() && raw_class.is_none() {
            return;
        }
        if symbol.kind == SymbolKind::Graphic {
            symbol.kind = SymbolKind::MovieClip;
        }
        if let Some(cls) = raw_class {
            if is_valid_qualified_class_name(cls) && used_exported_classes.insert(cls.to_owned()) {
                symbol.class_name = Some(cls.to_owned());
            } else {
                self.report.warning(
                    "xfl_linkage_approximated",
                    path,
                    format!(
                        "Symbol linkageClassName {cls:?} is invalid or duplicated across library symbols and was omitted."
                    ),
                );
            }
        }
        let candidate_id = raw_id.or(symbol.class_name.as_deref());
        if let Some(lid) = candidate_id {
            if !lid.is_empty()
                && lid.len() <= 512
                && !lid.chars().any(char::is_control)
                && used_export_ids.insert(lid.to_owned())
            {
                symbol.linkage_id = Some(lid.to_owned());
            } else if raw_id.is_some() {
                self.report.warning(
                    "xfl_linkage_approximated",
                    path,
                    format!(
                        "Symbol linkageIdentifier {lid:?} is invalid or duplicated across library symbols and was omitted."
                    ),
                );
            }
        }
        if let Some(base_cls) = node
            .attribute("linkageBaseClass")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !matches!(
                base_cls,
                "flash.display.MovieClip" | "flash.display.Sprite" | "flash.display.SimpleButton"
            )
        {
            self.report.warning(
                "xfl_linkage_approximated",
                path,
                format!(
                    "Custom linkageBaseClass {base_cls:?} relies on bundled .as sources if present."
                ),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_component_item(
        &mut self,
        node: Node<'_, '_>,
        id: Id,
        symbol_folder: String,
        display_name: String,
        path: &str,
        used_export_ids: &mut std::collections::HashSet<String>,
        used_exported_classes: &mut std::collections::HashSet<String>,
    ) -> Symbol {
        self.audit(
            node,
            path,
            &[
                "name",
                "folder",
                "itemID",
                "symbolType",
                "lastModified",
                "lastUniqueIdentifier",
                "sourceFlashFilepath",
                "sourceLibraryItemHRef",
                "sourceLastModified",
                "sourceItemID",
                "sourceAutoUpdate",
                "tooltip",
                "customIconID",
                "actionscriptClass",
                "livePreviewScmHRef",
                "livePreviewScmSourceFilename",
                "persistLivePreview11",
                "livePreview11ScmHRef",
                "livePreview11ScmSourceFilename",
                "editFrameIndex",
                "requiredMinimumPlayerVersion",
                "requiredMinimumASVersion",
                "parametersAreLocked",
                "displayAsComponent",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[
                "timeline",
                "parametersAsXML",
                "classProperties",
                "customIcon",
                "dataBindingXML",
                "persistentData",
            ],
        );
        if let Some(pd_node) = child(node, "persistentData") {
            self.audit_persistent_data(pd_node, path);
        }
        for meta_tag in [
            "parametersAsXML",
            "classProperties",
            "customIcon",
            "dataBindingXML",
        ] {
            if let Some(sub) = child(node, meta_tag) {
                self.audit_unmodeled_xfl_subtree(sub, path);
            }
        }
        let mut symbol = if let Some(timeline_node) =
            child(node, "timeline").and_then(|w| child(w, "DOMTimeline"))
        {
            self.symbol(timeline_node, id, SymbolKind::MovieClip, path)
        } else {
            let layer_id = format!("xfl-component-layer-{}", self.extra_symbols.len() + 1);
            Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id,
                name: display_name.clone(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    guides: vec![],
                    frame_count: 1,
                    layers: vec![Layer {
                        properties: LayerProperties::default(),
                        id: layer_id,
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![],
                            ..Default::default()
                        }],
                    }],
                    scripts: vec![],
                },
            }
        };
        symbol.folder = symbol_folder;
        symbol.name = display_name;
        self.apply_symbol_linkage(
            &mut symbol,
            node,
            path,
            used_export_ids,
            used_exported_classes,
        );
        if symbol.class_name.is_none()
            && let Some(as_cls) = node
                .attribute("actionscriptClass")
                .map(str::trim)
                .filter(|s| !s.is_empty())
            && is_valid_qualified_class_name(as_cls)
            && used_exported_classes.insert(as_cls.to_owned())
        {
            symbol.class_name = Some(as_cls.to_owned());
        }
        self.report.warning(
            "xfl_component_approximated",
            path,
            "DOMComponentItem imported as a MovieClip; authoring component parameters and live-preview SWFs are not evaluated.",
        );
        symbol
    }

    fn resolve_script_language(&mut self, source: &str, location: &str) -> Option<ScriptLanguage> {
        if let Some(lang) = self.language {
            return Some(lang);
        }
        if self.has_publish_settings {
            self.report.error(
                "script_language_unknown",
                location,
                "Cannot determine AS2/AS3. Supply PublishSettings.xml with an unambiguous ActionScriptVersion.",
            );
            return None;
        }
        let inferred = if source.contains("package ")
            || source.contains("import flash.")
            || source.contains("addEventListener(")
        {
            ScriptLanguage::ActionScript3
        } else {
            ScriptLanguage::ActionScript2
        };
        self.report.warning(
            "script_language_defaulted",
            location,
            format!("PublishSettings.xml is absent; defaulted script language to {inferred:?}."),
        );
        Some(inferred)
    }

    fn audit_unmodeled_xfl_subtree(&mut self, root_sub: Node<'_, '_>, file: &str) {
        for n in root_sub.descendants().filter(Node::is_element) {
            let loc = self.location(n, file);
            if n.tag_name().namespace().is_some_and(|ns| {
                ns != "http://ns.adobe.com/xfl/2008/" && !is_fl_motion_namespace_element(n, ns)
            }) {
                self.report
                    .error("unknown_namespace", &loc, "Unknown XML namespace.");
            }
            for attr in n.attributes() {
                if let Some(namespace) = attr.namespace() {
                    self.report.error(
                        "unsupported_attribute_namespace",
                        &loc,
                        format!(
                            "Namespaced attribute {{{namespace}}}{} is unsupported; XFL attributes must have no namespace.",
                            attr.name()
                        ),
                    );
                }
            }
        }
    }

    fn audit_tlf_subtree(&mut self, root_sub: Node<'_, '_>, file: &str) {
        for n in root_sub.descendants().filter(Node::is_element) {
            let loc = self.location(n, file);
            if let Some(ns) = n.tag_name().namespace()
                && ns != "http://ns.adobe.com/xfl/2008/"
                && !is_tlf_namespace_element(n.tag_name().name(), ns)
            {
                self.report
                    .error("unknown_namespace", &loc, "Unknown XML namespace.");
            }
            for attr in n.attributes() {
                if let Some(namespace) = attr.namespace() {
                    self.report.error(
                        "unsupported_attribute_namespace",
                        &loc,
                        format!(
                            "Namespaced attribute {{{namespace}}}{} is unsupported.",
                            attr.name()
                        ),
                    );
                }
            }
        }
    }
}

fn is_fl_motion_namespace_element(node: Node<'_, '_>, ns: &str) -> bool {
    ns == "fl.motion.*"
        && node
            .ancestors()
            .any(|a| a.is_element() && a.tag_name().name() == "Motion")
}

fn normalize_xfl_folder_path(raw: &str) -> String {
    if raw.is_empty() || raw.starts_with('/') || raw.ends_with('/') {
        return raw.to_owned();
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.iter().any(|s| s.trim().is_empty()) {
        return raw.to_owned();
    }
    parts
        .into_iter()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn get_file_ci<'a>(files: &'a BTreeMap<String, String>, target: &str) -> Option<&'a String> {
    files.get(target).or_else(|| {
        files
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(target))
            .map(|(_, v)| v)
    })
}

fn get_binary_ci<'a>(files: &'a BTreeMap<String, Vec<u8>>, target: &str) -> Option<&'a [u8]> {
    files.get(target).map(Vec::as_slice).or_else(|| {
        files
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(target))
            .map(|(_, v)| v.as_slice())
    })
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

fn strip_neoflash_language_marker(source: &str) -> Option<(ScriptLanguage, String)> {
    for (prefix, lang) in [
        (
            "/* neoflash:language=javascript */\n",
            ScriptLanguage::JavaScript,
        ),
        (
            "/* neoflash:language=typescript */\n",
            ScriptLanguage::TypeScript,
        ),
        (
            "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript1,
        ),
        (
            "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript2,
        ),
        (
            "/* neoflash:language=actionscript3 */\n",
            ScriptLanguage::ActionScript3,
        ),
    ] {
        if let Some(rest) = source.strip_prefix(prefix) {
            return Some((lang, rest.to_owned()));
        }
    }
    None
}

fn skip_leading_ws_and_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(after_line) = s.strip_prefix("//") {
            match after_line.find('\n') {
                Some(nl) => s = &after_line[nl + 1..],
                None => return "",
            }
        } else if let Some(after_block) = s.strip_prefix("/*") {
            match after_block.find("*/") {
                Some(end) => s = &after_block[end + 2..],
                None => return "",
            }
        } else {
            return s;
        }
    }
}

fn flash_handler_event(source: &str) -> Option<ObjectEvent> {
    let trimmed = skip_leading_ws_and_comments(source);
    let open = trimmed.find('(')?;
    let handler = skip_leading_ws_and_comments(&trimmed[..open]).trim();
    let close = trimmed[open + 1..].find(')')? + open + 1;
    let event = trimmed[open + 1..close].trim();
    if handler.eq_ignore_ascii_case("on") {
        // NeoFlash currently models the Flash mouse-handler family as Click. `release` is the
        // canonical form emitted by this exporter, while preserving other wrapped `on(...)`
        // sources is still less lossy than silently converting them to Load.
        Some(ObjectEvent::Click)
    } else if handler.eq_ignore_ascii_case("onClipEvent") {
        if event.eq_ignore_ascii_case("enterFrame") {
            Some(ObjectEvent::EnterFrame)
        } else if event.eq_ignore_ascii_case("load") {
            Some(ObjectEvent::Load)
        } else {
            None
        }
    } else {
        None
    }
}

fn parse_neoflash_object_scripts(source: &str) -> Vec<ObjectScript> {
    if !source.contains("/* neoflash:language=") {
        return Vec::new();
    }
    let trimmed = source.trim();
    if let Some((lang, clean)) = strip_neoflash_language_marker(trimmed) {
        let event = if matches!(
            lang,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            flash_handler_event(&clean).unwrap_or(ObjectEvent::Load)
        } else {
            ObjectEvent::Load
        };
        return vec![ObjectScript {
            event,
            script: Script {
                language: lang,
                source: clean,
            },
        }];
    }
    let headers = [
        ("on (release) {\n", ObjectEvent::Click),
        ("onClipEvent (load) {\n", ObjectEvent::Load),
        ("onClipEvent (enterFrame) {\n", ObjectEvent::EnterFrame),
    ];
    let boundaries = [
        "\n}\non (release) {\n/* neoflash:language=",
        "\n}\nonClipEvent (load) {\n/* neoflash:language=",
        "\n}\nonClipEvent (enterFrame) {\n/* neoflash:language=",
    ];
    let mut out = Vec::new();
    let mut rest = trimmed;
    while !rest.is_empty() {
        let Some((header, ev)) = headers.iter().find(|(h, _)| rest.starts_with(h)).copied() else {
            return Vec::new();
        };
        let after_header = &rest[header.len()..];
        let next_split = boundaries.iter().filter_map(|b| after_header.find(b)).min();
        let (body, next_rest) = if let Some(pos) = next_split {
            (&after_header[..pos], &after_header[pos + 3..])
        } else if let Some(b) = after_header.strip_suffix("\n}") {
            (b, "")
        } else {
            return Vec::new();
        };
        let Some((lang, clean)) = strip_neoflash_language_marker(body) else {
            return Vec::new();
        };
        out.push(ObjectScript {
            event: ev,
            script: Script {
                language: lang,
                source: clean,
            },
        });
        rest = next_rest;
    }
    out
}

fn import_bundled_as_sources(
    files: &BTreeMap<String, String>,
    project: &mut Project,
    default_lang: Option<ScriptLanguage>,
    report: &mut ImportReport,
) {
    let mut source_idx = 0usize;
    for (path, source) in files {
        let lower = path.to_ascii_lowercase();
        let explicit_lang = if lower.ends_with(".as") {
            None
        } else if lower.ends_with(".js") {
            Some(ScriptLanguage::JavaScript)
        } else if lower.ends_with(".ts") {
            Some(ScriptLanguage::TypeScript)
        } else {
            continue;
        };
        if project.sources.len() >= 1024 {
            report.warning(
                "xfl_sources_truncated",
                path,
                "Bundled ActionScript source file count exceeds 1024; remaining .as files were omitted.",
            );
            break;
        }
        let (lang, clean_source) =
            if let Some((marked_lang, stripped)) = strip_neoflash_language_marker(source) {
                (marked_lang, stripped)
            } else {
                let resolved = explicit_lang.or(default_lang).unwrap_or_else(|| {
                    if source.contains("package ")
                        || source.contains("import flash.")
                        || source.contains("public class ")
                    {
                        ScriptLanguage::ActionScript3
                    } else {
                        ScriptLanguage::ActionScript2
                    }
                });
                (resolved, source.clone())
            };
        source_idx += 1;
        project.sources.push(SourceUnit {
            id: format!("xfl-source-{source_idx}"),
            path: path.clone(),
            language: lang,
            source: clean_source,
        });
    }
}
