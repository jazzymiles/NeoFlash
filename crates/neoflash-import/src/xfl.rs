use self::media::{looks_like_mp3 as looks_like_xfl_mp3, wav_duration_44 as parse_wav_duration_44};
use crate::{ImportReport, Statistics, source};
use neoflash_core::*;
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) mod media;
mod publish;
mod shape;

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
        resource_bytes_total: 0,
        language: publish.language,
        has_publish_settings,
        include_hidden_layers: publish.include_hidden_layers,
        text_warning: false,
        direct_filter_strength,
        legacy_filter_strength_warning: false,
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
        parser.parse_media(media, binary_files);
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
    if let Some(raw_doc_class) = publish.document_class {
        let trimmed = raw_doc_class.trim();
        if !trimmed.is_empty() {
            if is_valid_qualified_class_name(trimmed) {
                p.document_class = Some(trimmed.to_owned());
            } else {
                parser.report.warning(
                    "xfl_invalid_document_class",
                    "PublishSettings.xml",
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
                            && !is_tlf_namespace_element(node.tag_name().name(), namespace))
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
    width: u32,
    height: u32,
    smoothing: bool,
}

#[derive(Clone, Debug)]
struct XflSoundAsset {
    resource_id: Id,
    duration_44: u64,
}

struct Parser<'a> {
    report: &'a mut ImportReport,
    symbols: HashMap<String, (Id, SymbolKind)>,
    bitmaps: HashMap<String, XflBitmapAsset>,
    sounds: HashMap<String, XflSoundAsset>,
    videos: HashMap<String, (f64, f64)>,
    font_items: HashMap<String, String>,
    extra_symbols: Vec<Symbol>,
    resources: Vec<ResourceUnit>,
    resource_bytes_total: usize,
    language: Option<ScriptLanguage>,
    has_publish_settings: bool,
    include_hidden_layers: bool,
    text_warning: bool,
    direct_filter_strength: bool,
    legacy_filter_strength_warning: bool,
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
        if node
            .tag_name()
            .namespace()
            .is_some_and(|ns| ns != "http://ns.adobe.com/xfl/2008/")
        {
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
            if n.tag_name()
                .namespace()
                .is_some_and(|ns| ns != "http://ns.adobe.com/xfl/2008/")
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

    fn audit_filters(&mut self, filters_node: Node<'_, '_>, file: &str, location: &str) {
        self.audit(
            filters_node,
            file,
            &[],
            &[
                "DropShadowFilter",
                "GlowFilter",
                "BevelFilter",
                "BlurFilter",
                "GradientGlowFilter",
                "GradientBevelFilter",
                "AdjustColorFilter",
            ],
        );
        let mut saw_filter = false;
        for f in elements(filters_node) {
            saw_filter = true;
            match f.tag_name().name() {
                "DropShadowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "distance",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "hideObject",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "GlowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "BevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "highlightColor",
                            "highlightAlpha",
                            "shadowColor",
                            "shadowAlpha",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "BlurFilter" => {
                    self.audit(f, file, &["blurX", "blurY", "quality", "isEnabled"], &[]);
                }
                "GradientGlowFilter" | "GradientBevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &["GradientEntry"],
                    );
                    for entry in elements(f).filter(|n| n.has_tag_name("GradientEntry")) {
                        self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                    }
                }
                "AdjustColorFilter" => {
                    self.audit(
                        f,
                        file,
                        &["brightness", "contrast", "saturation", "hue", "isEnabled"],
                        &[],
                    );
                }
                _ => {}
            }
        }
        if saw_filter {
            self.report.warning(
                "xfl_filters_approximated",
                location,
                "XFL display filters (DropShadow/Glow/Bevel/Blur/AdjustColor) are not rendered on authored elements yet.",
            );
        }
    }

    fn parse_instance_filters(
        &mut self,
        filters_node: Node<'_, '_>,
        file: &str,
        location: &str,
    ) -> Vec<FilterSpec> {
        self.audit(
            filters_node,
            file,
            &[],
            &[
                "DropShadowFilter",
                "GlowFilter",
                "BevelFilter",
                "BlurFilter",
                "GradientGlowFilter",
                "GradientBevelFilter",
                "AdjustColorFilter",
            ],
        );
        let mut parsed = Vec::new();
        let mut omitted = false;
        for f in elements(filters_node) {
            let f_loc = self.location(f, file);
            let spec = match f.tag_name().name() {
                "DropShadowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "distance",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "hideObject",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let alpha = self.number(f, "alpha", 1.0, file).clamp(0.0, 1.0);
                    let rgba = self.color(f.attribute("color").unwrap_or("#000000"), alpha, &f_loc);
                    let color =
                        (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let inner = self.boolean(f, "inner", false, file);
                    let knockout = self.boolean(f, "knockout", false, file);
                    let hide_object = self.boolean(f, "hideObject", false, file);
                    enabled.then_some(FilterSpec::DropShadow {
                        distance,
                        angle,
                        color,
                        alpha,
                        blur_x,
                        blur_y,
                        strength,
                        quality,
                        inner,
                        knockout,
                        hide_object,
                    })
                }
                "GlowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let alpha = self.number(f, "alpha", 1.0, file).clamp(0.0, 1.0);
                    let rgba = self.color(f.attribute("color").unwrap_or("#FF0000"), alpha, &f_loc);
                    let color =
                        (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let inner = self.boolean(f, "inner", false, file);
                    let knockout = self.boolean(f, "knockout", false, file);
                    enabled.then_some(FilterSpec::Glow {
                        color,
                        alpha,
                        blur_x,
                        blur_y,
                        strength,
                        quality,
                        inner,
                        knockout,
                    })
                }
                "BevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "highlightColor",
                            "highlightAlpha",
                            "shadowColor",
                            "shadowAlpha",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let highlight_alpha =
                        self.number(f, "highlightAlpha", 1.0, file).clamp(0.0, 1.0);
                    let h_rgba = self.color(
                        f.attribute("highlightColor").unwrap_or("#FFFFFF"),
                        highlight_alpha,
                        &f_loc,
                    );
                    let highlight_color = (u32::from(h_rgba[0]) << 16)
                        | (u32::from(h_rgba[1]) << 8)
                        | u32::from(h_rgba[2]);
                    let shadow_alpha = self.number(f, "shadowAlpha", 1.0, file).clamp(0.0, 1.0);
                    let s_rgba = self.color(
                        f.attribute("shadowColor").unwrap_or("#000000"),
                        shadow_alpha,
                        &f_loc,
                    );
                    let shadow_color = (u32::from(s_rgba[0]) << 16)
                        | (u32::from(s_rgba[1]) << 8)
                        | u32::from(s_rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let placement = match f.attribute("type").unwrap_or("inner") {
                        "outer" => "outer".to_owned(),
                        "full" => "full".to_owned(),
                        _ => "inner".to_owned(),
                    };
                    let knockout = self.boolean(f, "knockout", false, file);
                    enabled.then_some(FilterSpec::Bevel {
                        distance,
                        angle,
                        highlight_color,
                        highlight_alpha,
                        shadow_color,
                        shadow_alpha,
                        blur_x,
                        blur_y,
                        strength,
                        quality,
                        placement,
                        knockout,
                    })
                }
                "BlurFilter" => {
                    self.audit(f, file, &["blurX", "blurY", "quality", "isEnabled"], &[]);
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    enabled.then_some(FilterSpec::Blur {
                        blur_x,
                        blur_y,
                        quality,
                    })
                }
                "GradientGlowFilter" | "GradientBevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &["GradientEntry"],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let placement = match f.attribute("type").unwrap_or("inner") {
                        "outer" => "outer".to_owned(),
                        "full" => "full".to_owned(),
                        _ => "inner".to_owned(),
                    };
                    let knockout = self.boolean(f, "knockout", false, file);
                    let mut stops: Vec<(u32, f64, u8)> = Vec::new();
                    for entry in elements(f).filter(|n| n.has_tag_name("GradientEntry")) {
                        self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                        let e_loc = self.location(entry, file);
                        let alpha = self.number(entry, "alpha", 1.0, file).clamp(0.0, 1.0);
                        let rgba = self.color(
                            entry.attribute("color").unwrap_or("#000000"),
                            alpha,
                            &e_loc,
                        );
                        let color = (u32::from(rgba[0]) << 16)
                            | (u32::from(rgba[1]) << 8)
                            | u32::from(rgba[2]);
                        let raw_ratio = self.number(entry, "ratio", 0.0, file);
                        let ratio = if raw_ratio <= 1.0 {
                            (raw_ratio.clamp(0.0, 1.0) * 255.0).round() as u8
                        } else {
                            raw_ratio.clamp(0.0, 255.0).round() as u8
                        };
                        stops.push((color, alpha, ratio));
                    }
                    stops.sort_by_key(|s| s.2);
                    stops.truncate(16);
                    if stops.is_empty() {
                        stops.push((0x000000, 0.0, 0));
                        stops.push((0xFFFFFF, 1.0, 255));
                    }
                    let colors: Vec<u32> = stops.iter().map(|s| s.0).collect();
                    let alphas: Vec<f64> = stops.iter().map(|s| s.1).collect();
                    let ratios: Vec<u8> = stops.iter().map(|s| s.2).collect();
                    enabled.then(|| {
                        if f.tag_name().name() == "GradientGlowFilter" {
                            FilterSpec::GradientGlow {
                                distance,
                                angle,
                                colors,
                                alphas,
                                ratios,
                                blur_x,
                                blur_y,
                                strength,
                                quality,
                                placement,
                                knockout,
                            }
                        } else {
                            FilterSpec::GradientBevel {
                                distance,
                                angle,
                                colors,
                                alphas,
                                ratios,
                                blur_x,
                                blur_y,
                                strength,
                                quality,
                                placement,
                                knockout,
                            }
                        }
                    })
                }
                "AdjustColorFilter" => {
                    self.audit(
                        f,
                        file,
                        &["brightness", "contrast", "saturation", "hue", "isEnabled"],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let brightness = self.number(f, "brightness", 0.0, file).clamp(-100.0, 100.0);
                    let contrast = self.number(f, "contrast", 0.0, file).clamp(-100.0, 100.0);
                    let saturation = self.number(f, "saturation", 0.0, file).clamp(-100.0, 100.0);
                    let hue = self.number(f, "hue", 0.0, file).clamp(-180.0, 180.0);
                    enabled.then(|| FilterSpec::ColorMatrix {
                        matrix: adjust_color_matrix(brightness, contrast, saturation, hue),
                    })
                }
                _ => None,
            };
            if let Some(filter) = spec {
                if parsed.len() < MAX_OBJECT_FILTERS {
                    parsed.push(filter);
                } else {
                    omitted = true;
                }
            } else {
                omitted = true;
            }
        }
        if omitted {
            self.report.warning(
                "xfl_filters_approximated",
                location,
                "Disabled or excess (>8) XFL display filters were omitted.",
            );
        }
        parsed
    }

    fn symbol(&mut self, node: Node<'_, '_>, id: Id, kind: SymbolKind, file: &str) -> Symbol {
        self.audit(
            node,
            file,
            &[
                "name",
                "currentFrame",
                "guides",
                "autoNamed",
                "layerDepthEnabled",
            ],
            &["layers"],
        );
        if self.boolean(node, "layerDepthEnabled", false, file) {
            self.report.warning(
                "xfl_layer_depth_ignored",
                self.location(node, file),
                "Animate CC layerDepthEnabled Z-depth is approximated as flat 2D painter order.",
            );
        }
        let mut timeline = Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
        };
        let Some(layers) = child(node, "layers") else {
            self.report.error(
                "missing_layers",
                self.location(node, file),
                "Timeline lacks layers.",
            );
            return Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id,
                name: node.attribute("name").unwrap_or("Scene 1").into(),
                kind,
                timeline,
            };
        };
        self.audit(layers, file, &[], &["DOMLayer"]);
        let layer_nodes: Vec<_> = elements(layers)
            .filter(|n| n.has_tag_name("DOMLayer"))
            .collect();
        let mut parent_indices: Vec<Option<(usize, String)>> = vec![None; layer_nodes.len()];
        let mut spans = vec![];
        let mut instance_starts: BTreeMap<u32, Vec<(Id, String)>> = BTreeMap::new();
        let mut instance_ends: BTreeMap<u32, BTreeSet<Id>> = BTreeMap::new();
        let mut published_end = 0;
        let mut excluded_end = 0;
        for (li, layer_node) in layer_nodes.iter().copied().enumerate() {
            self.audit(
                layer_node,
                file,
                &[
                    "name",
                    "color",
                    "visible",
                    "locked",
                    "current",
                    "isSelected",
                    "autoNamed",
                    "layerType",
                    "parentLayerIndex",
                    "animationType",
                    "open",
                    "heightLiteral",
                    "heightMultiplier",
                    "outline",
                    "useOutlineView",
                    "highlighted",
                    "poseLocations",
                ],
                &["frames"],
            );
            for attr in ["heightLiteral", "heightMultiplier"] {
                if layer_node.attribute(attr).is_some() {
                    let h = self.integer(layer_node, attr, 1, file);
                    if h == 0 {
                        self.report.error(
                            "integer_required",
                            self.location(layer_node, file),
                            format!("{attr} must be a positive integer."),
                        );
                    }
                }
            }
            if let Some(anim) = layer_node.attribute("animationType")
                && !anim.is_empty()
                && anim != "none"
            {
                self.report.warning(
                    "motion_object_not_supported",
                    self.location(layer_node, file),
                    format!(
                        "XFL layer animationType={anim:?} is approximated using its authored keyframe poses."
                    ),
                );
            }
            let role = match layer_node.attribute("layerType").unwrap_or("normal") {
                "normal" => LayerRole::Normal,
                "guide" => LayerRole::Guide,
                "mask" => LayerRole::Mask,
                "folder" => LayerRole::Folder,
                "camera" => {
                    self.report.warning(
                        "xfl_camera_layer_approximated",
                        self.location(layer_node, file),
                        "Animate CC camera layer is imported as an ordinary layer; virtual camera pan/zoom is not evaluated.",
                    );
                    LayerRole::Normal
                }
                other => {
                    self.report.error(
                        "layer_type",
                        self.location(layer_node, file),
                        format!("Unsupported XFL layerType {other:?}."),
                    );
                    LayerRole::Normal
                }
            };
            let open = self.boolean(layer_node, "open", true, file);
            if layer_node.attribute("parentLayerIndex").is_some() {
                let p_idx = self.integer(layer_node, "parentLayerIndex", u32::MAX, file) as usize;
                if p_idx >= layer_nodes.len() || p_idx == li {
                    self.report.error(
                        "invalid_layer_parent",
                        self.location(layer_node, file),
                        "parentLayerIndex must reference a distinct layer in the same timeline.",
                    );
                } else {
                    parent_indices[li] = Some((p_idx, self.location(layer_node, file)));
                }
            }
            let authoring_visible = self.boolean(layer_node, "visible", true, file);
            let excluded = !authoring_visible && !self.include_hidden_layers;
            if !authoring_visible {
                self.report.warning(
                    "hidden_layer_publish_policy",
                    self.location(layer_node, file),
                    if excluded {
                        "InvisibleLayer disables publishing hidden layers. Artwork is retained on a hidden NeoFlash layer and excluded from playback; hidden-layer actions/labels require unsupported layer-owned script/label semantics."
                    } else {
                        "Flash includes hidden layers in publishing (the default). This layer is made visible in NeoFlash to preserve published artwork/playback; the original authoring eye-toggle state is not retained."
                    },
                );
            }
            let mut layer = Layer {
                properties: LayerProperties {
                    role,
                    collapsed: role == LayerRole::Folder && !open,
                    ..Default::default()
                },
                id: new_id("layer"),
                name: layer_node.attribute("name").unwrap_or("Layer").into(),
                visible: !excluded,
                locked: self.boolean(layer_node, "locked", false, file),
                keyframes: vec![],
            };
            type RawKeyframeEntry = (u32, u32, String, Vec<(Id, String)>, Keyframe);
            let mut ends = vec![];
            let mut raw_keys: Vec<RawKeyframeEntry> = vec![];
            if let Some(frames) = child(layer_node, "frames") {
                self.audit(frames, file, &[], &["DOMFrame"]);
                for frame_node in elements(frames).filter(|n| n.has_tag_name("DOMFrame")) {
                    let has_motion_xml = child(frame_node, "MotionObjectXML").is_some()
                        || child(frame_node, "motionObjectXML").is_some();
                    self.audit(
                        frame_node,
                        file,
                        &[
                            "index",
                            "duration",
                            "name",
                            "labelType",
                            "keyMode",
                            "tweenType",
                            "acceleration",
                            "easeMethodName",
                            "isMotionObject",
                            "visibleAnimationKeyframes",
                            "bookmark",
                            "tweenInstanceName",
                            "isIKPose",
                            "motionTweenOrientToPath",
                            "motionTweenSnap",
                            "motionTweenRotate",
                            "motionTweenRotateTimes",
                            "motionTweenScale",
                            "motionTweenSync",
                            "shapeTweenBlend",
                            "hasCustomEase",
                            "useSingleEaseCurve",
                            "soundName",
                            "soundSync",
                            "soundEffect",
                            "soundLoopMode",
                            "soundLoop",
                            "inPoint44",
                            "outPoint44",
                            "soundZoomLevel",
                        ],
                        &[
                            "elements",
                            "Actionscript",
                            "tweens",
                            "SoundEnvelope",
                            "MotionObjectXML",
                            "motionObjectXML",
                            "MorphShape",
                            "betweenFrameMatrixList",
                            "IKTree",
                        ],
                    );
                    let _ = self.boolean(frame_node, "isMotionObject", false, file);
                    let _ = self.boolean(frame_node, "bookmark", false, file);
                    let _ = self.boolean(frame_node, "isIKPose", false, file);
                    for motion_tag in ["MotionObjectXML", "motionObjectXML"] {
                        if let Some(sub) = child(frame_node, motion_tag) {
                            self.audit_unmodeled_xfl_subtree(sub, file);
                            self.report.warning(
                                "motion_object_not_supported",
                                self.location(frame_node, file),
                                "XFL motion-object property-track XML is approximated by holding the authored keyframe pose.",
                            );
                        }
                    }
                    for morph_tag in ["MorphShape", "betweenFrameMatrixList"] {
                        if let Some(sub) = child(frame_node, morph_tag) {
                            self.audit_unmodeled_xfl_subtree(sub, file);
                            self.report.warning(
                                "xfl_morph_hints_ignored",
                                self.location(frame_node, file),
                                "XFL shape-tween MorphShape hints/segments are ignored; NeoFlash resamples contours automatically.",
                            );
                        }
                    }
                    if let Some(sub) = child(frame_node, "IKTree") {
                        self.audit_unmodeled_xfl_subtree(sub, file);
                        self.report.warning(
                            "xfl_ik_approximated",
                            self.location(frame_node, file),
                            "XFL inverse-kinematics IKTree pose constraints are approximated using authored element poses.",
                        );
                    }
                    let frame_loc = self.location(frame_node, file);
                    let (tween, tween_kind, orient_to_path) =
                        self.frame_tween(frame_node, has_motion_xml, file);
                    let sound = self.frame_sound(frame_node, file);
                    let mut frame_label: String = frame_node.attribute("name").unwrap_or("").into();
                    match frame_node.attribute("labelType").unwrap_or("name") {
                        "name" => {}
                        "comment" => {
                            frame_label.clear();
                            self.report.warning(
                                "xfl_frame_comment",
                                &frame_loc,
                                "Authoring frame comment (labelType=\"comment\") is not exported as a runtime jump label.",
                            );
                        }
                        "anchor" => {
                            self.report.warning(
                                "xfl_frame_anchor",
                                &frame_loc,
                                "Browser anchor frame label (labelType=\"anchor\") is imported as an ordinary frame label.",
                            );
                        }
                        other => {
                            self.report.error(
                                "frame_label_type",
                                &frame_loc,
                                format!("Unsupported frame labelType {other:?}."),
                            );
                        }
                    }
                    let frame = self.integer(frame_node, "index", 0, file);
                    let duration = self.integer(frame_node, "duration", 1, file);
                    if duration == 0 {
                        self.report.error(
                            "frame_duration",
                            &frame_loc,
                            "Frame duration must be positive.",
                        );
                    }
                    let end = frame.saturating_add(duration.max(1));
                    if role != LayerRole::Folder {
                        if excluded {
                            excluded_end = excluded_end.max(end);
                            if !frame_label.is_empty() {
                                self.report.error("excluded_layer_label", &frame_loc, "A label on an excluded hidden layer cannot be retained with its publication ownership in the current model. Move the label to a published layer before importing.");
                            }
                        } else {
                            published_end = published_end.max(end);
                        }
                        timeline.frame_count = timeline.frame_count.max(end);
                        ends.push((frame, end));
                    } else if frame_node
                        .attribute("name")
                        .is_some_and(|name| !name.is_empty())
                        || tween.is_some()
                        || sound.is_some()
                    {
                        self.report.error(
                            "folder_layer_elements",
                            &frame_loc,
                            "Folder layers cannot own labels, tweens or sounds.",
                        );
                    }
                    let mut key = Keyframe {
                        frame,
                        elements: vec![],
                        tween,
                        tween_kind,
                        orient_to_path,
                        sound,
                        label: frame_label,
                        ..Default::default()
                    };
                    let mut elem_locs = vec![];
                    if let Some(container) = child(frame_node, "elements") {
                        self.audit(
                            container,
                            file,
                            &[],
                            &[
                                "DOMSymbolInstance",
                                "DOMBitmapInstance",
                                "DOMStaticText",
                                "DOMDynamicText",
                                "DOMInputText",
                                "DOMTLFText",
                                "DOMGroup",
                                "DOMVideoInstance",
                                "DOMCompiledClipInstance",
                                "DOMComponentInstance",
                                "DOMRectangleObject",
                                "DOMOvalObject",
                                "DOMShape",
                            ],
                        );
                        for (ei, element_node) in elements(container).enumerate() {
                            if role == LayerRole::Folder {
                                self.report.error(
                                    "folder_layer_elements",
                                    self.location(element_node, file),
                                    "Folder layers must not contain display elements.",
                                );
                                continue;
                            }
                            let identity = format!("xfl-{id}-{li}-{ei}");
                            for element in self.element_with_depth(
                                element_node,
                                identity,
                                frame,
                                0,
                                kind,
                                file,
                            ) {
                                elem_locs
                                    .push((element.id.clone(), self.location(element_node, file)));
                                key.elements.push(element);
                            }
                        }
                    }
                    if let Some(actions) = child(frame_node, "Actionscript") {
                        self.audit(actions, file, &["breakpoints"], &["script"]);
                        if let Some(script) = child(actions, "script") {
                            self.audit(script, file, &[], &[]);
                            let source = immediate_text(script);
                            if !source.trim().is_empty() {
                                if role == LayerRole::Folder {
                                    self.report.error(
                                        "folder_layer_elements",
                                        self.location(script, file),
                                        "Folder layers cannot own frame scripts.",
                                    );
                                }
                                if excluded {
                                    self.report.error("excluded_layer_script", self.location(script, file), "A frame script on an excluded hidden layer cannot retain its publication ownership in the current model. Import is blocked rather than executing or discarding that source.");
                                }
                                let script_loc = self.location(script, file);
                                if let Some((language, clean_source)) =
                                    strip_neoflash_language_marker(&source)
                                {
                                    timeline.scripts.push(FrameScript {
                                        frame,
                                        script: Script {
                                            language,
                                            source: clean_source,
                                        },
                                    });
                                } else if let Some(language) =
                                    self.resolve_script_language(&source, &script_loc)
                                {
                                    timeline.scripts.push(FrameScript {
                                        frame,
                                        script: Script { language, source },
                                    });
                                    self.report.warning("script_preserved",script_loc,"ActionScript source retained unchanged. Execution compatibility is checked separately by the compiler/exporter.");
                                }
                            }
                        }
                    }
                    if role != LayerRole::Folder {
                        raw_keys.push((frame, end, frame_loc, elem_locs, key));
                    }
                }
            }
            raw_keys.sort_by_key(|entry| entry.0);
            ends.sort_unstable();
            for pair in ends.windows(2) {
                if pair[0].1 > pair[1].0 {
                    self.report.error(
                        "overlapping_frames",
                        self.location(layer_node, file),
                        "XFL frame spans overlap.",
                    );
                }
            }
            // Resolve classic and shape tween continuity across consecutive keyframes.
            for idx in 0..raw_keys.len() {
                let (prev_tween, prev_kind, prev_end, prev_loc) = {
                    let prev = &raw_keys[idx];
                    (prev.4.tween, prev.4.tween_kind, prev.1, prev.2.clone())
                };
                if prev_tween.is_none() {
                    continue;
                }
                if idx + 1 >= raw_keys.len() || raw_keys[idx + 1].0 != prev_end {
                    raw_keys[idx].4.tween = None;
                    raw_keys[idx].4.tween_kind = TweenKind::Classic;
                    raw_keys[idx].4.orient_to_path = false;
                    self.report.warning(
                        "xfl_tween_dropped",
                        prev_loc,
                        "Unterminated or gap-separated tween has no adjacent end keyframe; its pose is held.",
                    );
                    continue;
                }
                match prev_kind {
                    TweenKind::Classic => {
                        let compatible = raw_keys[idx].4.elements.len() == 1
                            && raw_keys[idx + 1].4.elements.len() == 1
                            && classic_tween_matches(
                                &raw_keys[idx].4.elements[0],
                                &raw_keys[idx + 1].4.elements[0],
                            );
                        if compatible {
                            let continued_id = raw_keys[idx].4.elements[0].id.clone();
                            raw_keys[idx + 1].4.elements[0].id = continued_id.clone();
                            if let Some(slot) = raw_keys[idx + 1].3.first_mut() {
                                slot.0 = continued_id;
                            }
                        } else if raw_keys[idx + 1].4.elements.is_empty() {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Classic motion tween ends at a blank keyframe; its start pose is held.",
                            );
                        } else {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Classic motion tween does not connect a single compatible element across keyframes; poses are held.",
                            );
                        }
                    }
                    TweenKind::Shape => {
                        let is_shape = |e: &Element| {
                            matches!(
                                e.kind,
                                ElementKind::Rectangle { .. }
                                    | ElementKind::Ellipse { .. }
                                    | ElementKind::Path { .. }
                                    | ElementKind::PaintedPath { .. }
                            )
                        };
                        let valid_from = !raw_keys[idx].4.elements.is_empty()
                            && raw_keys[idx].4.elements.iter().all(is_shape);
                        let valid_to = !raw_keys[idx + 1].4.elements.is_empty()
                            && raw_keys[idx + 1].4.elements.iter().all(is_shape);
                        if !valid_from || !valid_to {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.tween_kind = TweenKind::Classic;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Shape tween requires vector shape elements on both start and end keyframes; tween dropped and poses held.",
                            );
                        }
                    }
                    TweenKind::Motion => {}
                }
            }
            // Record true instance placement lifetimes (continuous classic-tween chains
            // share one element ID and do not end/restart at intermediate tween keys).
            let mut active_instances: HashMap<Id, (Id, u32)> = HashMap::new();
            for (frame, end, _, elem_locs, key) in &raw_keys {
                let current_ids: BTreeSet<&str> =
                    key.elements.iter().map(|e| e.id.as_str()).collect();
                active_instances.retain(|elem_id, (sym_id, prev_end)| {
                    if *prev_end == *frame && current_ids.contains(elem_id.as_str()) {
                        true
                    } else {
                        instance_ends
                            .entry(*prev_end)
                            .or_default()
                            .insert(sym_id.clone());
                        false
                    }
                });
                for element in &key.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                        if let Some(entry) = active_instances.get_mut(&element.id) {
                            entry.1 = *end;
                        } else {
                            let loc = elem_locs
                                .iter()
                                .find(|(eid, _)| eid == &element.id)
                                .map(|(_, l)| l.clone())
                                .unwrap_or_default();
                            instance_starts
                                .entry(*frame)
                                .or_default()
                                .push((symbol_id.clone(), loc));
                            active_instances.insert(element.id.clone(), (symbol_id.clone(), *end));
                        }
                    }
                }
            }
            for (_, (sym_id, last_end)) in active_instances {
                instance_ends.entry(last_end).or_default().insert(sym_id);
            }
            layer.keyframes = raw_keys.into_iter().map(|entry| entry.4).collect();
            spans.push(ends);
            timeline.layers.push(layer);
        }
        let roles_and_ids: Vec<(LayerRole, Id)> = timeline
            .layers
            .iter()
            .map(|l| (l.properties.role, l.id.clone()))
            .collect();
        for (li, parent_entry) in parent_indices.into_iter().enumerate() {
            let Some((p_idx, loc)) = parent_entry else {
                continue;
            };
            let (parent_role, ref parent_id) = roles_and_ids[p_idx];
            let child_role = roles_and_ids[li].0;
            match parent_role {
                LayerRole::Folder => {
                    timeline.layers[li].properties.parent = Some(parent_id.clone());
                }
                LayerRole::Mask if child_role == LayerRole::Normal => {
                    timeline.layers[li].properties.mask = Some(parent_id.clone());
                }
                LayerRole::Guide if child_role == LayerRole::Normal => {
                    timeline.layers[li].properties.motion_guide = Some(parent_id.clone());
                }
                _ => {
                    self.report.error(
                        "invalid_layer_parent",
                        loc,
                        "parentLayerIndex must target a folder layer, or a mask/guide layer from a normal child layer.",
                    );
                }
            }
        }
        for (frame, starts) in instance_starts {
            if let Some(ends) = instance_ends.get(&frame) {
                for (symbol, location) in starts {
                    if ends.contains(&symbol) {
                        self.report.warning(
                            "ambiguous_instance_continuity",
                            location,
                            "An adjacent key ends and places the same library symbol without a classic tween; the importer treats the second keyframe as a new placement span.",
                        );
                    }
                }
            }
        }
        if excluded_end > published_end {
            self.report.error("excluded_layer_duration", self.location(node, file), "An excluded hidden layer defines a longer timeline than the published layers. Published duration cannot yet be established without dropping authored frame structure; align the visible timeline extent before importing.");
        }
        for (layer, ends) in timeline.layers.iter_mut().zip(spans) {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            for (_, end) in ends {
                if end < timeline.frame_count && !layer.keyframes.iter().any(|k| k.frame == end) {
                    layer.keyframes.push(Keyframe {
                        frame: end,
                        elements: vec![],
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    });
                }
            }
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        // XFL lists topmost layers first; NeoFlash stores painter order.
        timeline.layers.reverse();
        Symbol {
            folder: String::new(),
            class_name: None,
            linkage_id: None,
            id,
            name: node.attribute("name").unwrap_or("Scene 1").into(),
            kind,
            timeline,
        }
    }

    fn element_with_depth(
        &mut self,
        node: Node<'_, '_>,
        base_id: Id,
        frame: u32,
        depth: usize,
        enclosing_kind: SymbolKind,
        file: &str,
    ) -> Vec<Element> {
        let tag = node.tag_name().name();
        if tag == "DOMShape" {
            return self.shape(node, base_id, file).unwrap_or_default();
        }
        let location = self.location(node, file);
        if tag == "DOMGroup" {
            self.audit(
                node,
                file,
                &["name", "selected", "locked"],
                &["matrix", "transformationPoint", "members"],
            );
            let _ = self.transform(node, file);
            if depth >= 32 {
                self.report.error(
                    "xfl_group_depth",
                    &location,
                    "Nested DOMGroup depth exceeds 32 levels.",
                );
                return vec![];
            }
            let mut grouped = Vec::new();
            if let Some(members) = child(node, "members") {
                self.audit(
                    members,
                    file,
                    &[],
                    &[
                        "DOMSymbolInstance",
                        "DOMBitmapInstance",
                        "DOMStaticText",
                        "DOMDynamicText",
                        "DOMInputText",
                        "DOMTLFText",
                        "DOMGroup",
                        "DOMVideoInstance",
                        "DOMCompiledClipInstance",
                        "DOMComponentInstance",
                        "DOMRectangleObject",
                        "DOMOvalObject",
                        "DOMShape",
                    ],
                );
                for (mi, member) in elements(members).enumerate() {
                    let member_id = format!("{base_id}-g{mi}");
                    grouped.extend(self.element_with_depth(
                        member,
                        member_id,
                        frame,
                        depth + 1,
                        enclosing_kind,
                        file,
                    ));
                }
            }
            return grouped;
        }
        let mut id = base_id;
        // A later authored instance span never inherits an earlier
        // XML slot's state unless explicitly connected by a classic tween.
        if matches!(
            tag,
            "DOMSymbolInstance" | "DOMCompiledClipInstance" | "DOMComponentInstance"
        ) && frame != 0
        {
            id.push_str(&format!("-placement-{frame}"));
        }
        let mut transform = self.transform(node, file);
        let mut name: String = node.attribute("name").unwrap_or(tag).into();
        let mut scripts = Vec::new();
        let mut visible = true;
        let mut cache_as_bitmap = false;
        let mut blend_mode = BlendMode::Normal;
        let mut accessibility = None;
        let mut filters = Vec::new();
        let kind = match tag {
            "DOMSymbolInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "libraryItemName",
                        "symbolType",
                        "firstFrame",
                        "loop",
                        "trackAsMenu",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "blendMode",
                        "cacheAsBitmap",
                        "exportAsBitmap",
                        "isVisible",
                        "bits32",
                        "matrix3D",
                        "rotationX",
                        "rotationY",
                        "rotationZ",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "Actionscript",
                        "MatteColor",
                        "persistentData",
                    ],
                );
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                    self.report.warning("transform_pivot", &location, "Placement matrix retained; Flash's 2D authoring centerPoint3DX/Y pivot is not retained. Actual 3D transforms remain unsupported.");
                }
                cache_as_bitmap = self.boolean(node, "cacheAsBitmap", false, file);
                let _ = self.boolean(node, "exportAsBitmap", false, file);
                visible = self.boolean(node, "isVisible", true, file);
                let has_accessible_data = self.boolean(node, "hasAccessibleData", false, file);
                let silent = self.boolean(node, "silent", false, file);
                let _ = self.boolean(node, "forceSimple", false, file);
                let tab_index = self.integer(node, "tabIndex", 0, file).clamp(0, 10_000);
                if let Some(bm) = node.attribute("blendMode")
                    && !bm.is_empty()
                {
                    if let Some(parsed_bm) = BlendMode::from_name(bm) {
                        blend_mode = parsed_bm;
                    } else {
                        self.report.warning(
                            "xfl_blend_mode_approximated",
                            &location,
                            format!(
                                "XFL instance blendMode={bm:?} is rendered with normal alpha compositing."
                            ),
                        );
                    }
                }
                if has_accessible_data
                    || node
                        .attribute("accName")
                        .is_some_and(|s| !s.trim().is_empty())
                {
                    let raw_acc_name = node
                        .attribute("accName")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            node.attribute("name")
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                        })
                        .or_else(|| {
                            node.attribute("libraryItemName")
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                        })
                        .unwrap_or("Instance");
                    let acc_name: String = raw_acc_name.chars().take(256).collect();
                    let acc_desc: String = node
                        .attribute("description")
                        .unwrap_or("")
                        .chars()
                        .take(4096)
                        .collect();
                    accessibility = Some(Accessibility {
                        name: acc_name,
                        description: acc_desc,
                        role: AccessibleRole::Button,
                        tab_order: tab_index,
                        disabled: silent,
                    });
                }
                if node.attribute("matrix3D").is_some()
                    || node.attribute("rotationX").is_some()
                    || node.attribute("rotationY").is_some()
                    || node.attribute("rotationZ").is_some()
                {
                    if let Some(m3d_str) = node.attribute("matrix3D") {
                        let vals: Vec<f64> = m3d_str
                            .split(|c: char| c.is_ascii_whitespace() || c == ',')
                            .filter(|s| !s.is_empty())
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if vals.len() == 16 && vals.iter().all(|v| v.is_finite()) {
                            let persp = (1.0 / (1.0 + vals[14] / 1000.0)).clamp(0.1, 10.0);
                            let mat2d = [
                                vals[0] * persp,
                                vals[1] * persp,
                                vals[4] * persp,
                                vals[5] * persp,
                                vals[12],
                                vals[13],
                            ];
                            if let Some(mut projected) = Transform::from_matrix(mat2d) {
                                projected.alpha = transform.alpha;
                                projected.color_transform = transform.color_transform;
                                transform = projected;
                            }
                        }
                    }
                    if node.attribute("rotationZ").is_some() {
                        let rz = self.number(node, "rotationZ", 0.0, file);
                        if rz.is_finite() {
                            transform.rotation = (transform.rotation + rz).clamp(-1e6, 1e6);
                        }
                    }
                    if node.attribute("rotationX").is_some() {
                        let rx = self.number(node, "rotationX", 0.0, file);
                        if rx.is_finite() {
                            let cos_x = rx.to_radians().cos();
                            if cos_x.abs() >= 0.05 {
                                transform.scale_y *= cos_x;
                            }
                        }
                    }
                    if node.attribute("rotationY").is_some() {
                        let ry = self.number(node, "rotationY", 0.0, file);
                        if ry.is_finite() {
                            let cos_y = ry.to_radians().cos();
                            if cos_y.abs() >= 0.05 {
                                transform.scale_x *= cos_y;
                            }
                        }
                    }
                    self.report.warning(
                        "xfl_3d_approximated",
                        &location,
                        "XFL 3D instance transform attributes (matrix3D/rotationX/Y/Z) are projected to 2D placement.",
                    );
                }
                if let Some(filters_node) = child(node, "filters") {
                    if enclosing_kind == SymbolKind::Button {
                        self.audit_filters(filters_node, file, &location);
                    } else {
                        filters = self.parse_instance_filters(filters_node, file, &location);
                    }
                }
                if let Some(matte) = child(node, "MatteColor") {
                    self.audit(matte, file, &["color", "alpha"], &[]);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                if let Some(actions) = child(node, "Actionscript") {
                    self.audit(actions, file, &["breakpoints"], &["script"]);
                    if let Some(script_node) = child(actions, "script") {
                        self.audit(script_node, file, &[], &[]);
                        let source = immediate_text(script_node);
                        if !source.trim().is_empty() {
                            let script_loc = self.location(script_node, file);
                            let unwrapped_neoflash = parse_neoflash_object_scripts(&source);
                            if !unwrapped_neoflash.is_empty() {
                                scripts.extend(unwrapped_neoflash);
                            } else if let Some(language) =
                                self.resolve_script_language(&source, &script_loc)
                            {
                                scripts.push(ObjectScript {
                                    event: ObjectEvent::Load,
                                    script: Script { language, source },
                                });
                                self.report.warning(
                                    "script_preserved",
                                    script_loc,
                                    "Object ActionScript handler source retained unchanged.",
                                );
                            }
                        }
                    }
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let Some((symbol_id, target_kind)) = self.symbols.get(target).cloned() else {
                    if matches!(target, "__Camera__" | "___camera___instance") {
                        self.report.warning(
                            "xfl_camera_layer_ignored",
                            &location,
                            "Animate CC virtual camera symbol instance is omitted.",
                        );
                        return vec![];
                    }
                    self.report.error(
                        "missing_symbol",
                        &location,
                        format!("Unknown symbol {target}"),
                    );
                    return vec![];
                };
                let first = self.integer(node, "firstFrame", 0, file);
                let track_as_menu = self.boolean(node, "trackAsMenu", false, file);
                let default_type = match target_kind {
                    SymbolKind::Button => "button",
                    _ => "movie clip",
                };
                let playback = match node.attribute("symbolType").unwrap_or(default_type) {
                    "button" => {
                        if target_kind != SymbolKind::Button {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "Button instance behavior on a non-button library symbol is approximated using the target symbol's timeline.",
                            );
                        }
                        if track_as_menu {
                            self.report.warning(
                                "xfl_button_track_as_menu",
                                &location,
                                "Button trackAsMenu behavior is approximated with standard button pointer tracking.",
                            );
                        }
                        if first != 0 || node.attribute("loop").is_some() {
                            self.report.warning(
                                "clip_start_frame",
                                &location,
                                "Button instances ignore graphic loop and firstFrame attributes.",
                            );
                        }
                        InstancePlayback::Independent
                    }
                    "movie clip" => {
                        if target_kind == SymbolKind::Button {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "MovieClip instance behavior on a button library symbol is approximated.",
                            );
                        }
                        if track_as_menu {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "trackAsMenu on a MovieClip instance is ignored.",
                            );
                        }
                        if first != 0 {
                            self.report.warning(
                                "clip_start_frame",
                                &location,
                                "MovieClip firstFrame authoring attribute is ignored; MovieClips always start at frame 0.",
                            );
                        }
                        InstancePlayback::Independent
                    }
                    "graphic" => {
                        if target_kind == SymbolKind::Button {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "Graphic instance behavior on a button library symbol is approximated.",
                            );
                        }
                        if track_as_menu {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "trackAsMenu on a graphic instance is ignored.",
                            );
                        }
                        match node.attribute("loop").unwrap_or("loop") {
                            "single frame" => InstancePlayback::SingleFrame(first),
                            "loop" => {
                                if first == 0 {
                                    InstancePlayback::Loop
                                } else {
                                    InstancePlayback::LoopFrom(first)
                                }
                            }
                            "play once" => {
                                if first == 0 {
                                    InstancePlayback::PlayOnce
                                } else {
                                    InstancePlayback::PlayOnceFrom(first)
                                }
                            }
                            other => {
                                self.report.error(
                                    "graphic_loop",
                                    &location,
                                    format!("Unsupported playback mode {other}"),
                                );
                                InstancePlayback::Loop
                            }
                        }
                    }
                    other => {
                        self.report.error(
                            "instance_type",
                            &location,
                            format!("Unsupported instance type {other}"),
                        );
                        InstancePlayback::Independent
                    }
                };
                ElementKind::Instance {
                    symbol_id,
                    playback,
                }
            }
            "DOMCompiledClipInstance" | "DOMComponentInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "uniqueID",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "dataBindingXML",
                        "parametersAsXML",
                        "persistentData",
                    ],
                );
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                }
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                for meta_tag in ["dataBindingXML", "parametersAsXML"] {
                    if let Some(sub) = child(node, meta_tag) {
                        self.audit_unmodeled_xfl_subtree(sub, file);
                    }
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let symbol_id = if let Some((sid, _)) = self.symbols.get(target).cloned() {
                    sid
                } else if !target.is_empty() {
                    let placeholder_idx = self.extra_symbols.len() + 1;
                    let sid = format!("xfl-component-placeholder-{placeholder_idx}");
                    let lid = format!("xfl-component-placeholder-layer-{placeholder_idx}");
                    let sym_name = target.rsplit_once('/').map_or(target, |(_, b)| b);
                    self.extra_symbols.push(Symbol {
                        folder: String::new(),
                        class_name: None,
                        linkage_id: None,
                        id: sid.clone(),
                        name: sym_name.to_owned(),
                        kind: SymbolKind::MovieClip,
                        timeline: Timeline {
                            guides: vec![],
                            frame_count: 1,
                            layers: vec![Layer {
                                properties: LayerProperties::default(),
                                id: lid,
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
                    });
                    self.symbols
                        .insert(target.to_owned(), (sid.clone(), SymbolKind::MovieClip));
                    sid
                } else {
                    self.report.error(
                        "missing_symbol",
                        &location,
                        "Component instance is missing libraryItemName.",
                    );
                    return vec![];
                };
                self.report.warning(
                    "xfl_component_approximated",
                    &location,
                    format!("{tag} is approximated as a MovieClip instance."),
                );
                ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::Independent,
                }
            }
            "DOMVideoInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "frameLeft",
                        "frameRight",
                        "frameTop",
                        "frameBottom",
                        "centerPoint3DX",
                        "centerPoint3DY",
                    ],
                    &["matrix", "transformationPoint", "color", "filters"],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let (default_w, default_h) =
                    self.videos.get(target).copied().unwrap_or((320.0, 240.0));
                let fl = self.number(node, "frameLeft", 0.0, file);
                let fr = self.number(node, "frameRight", 0.0, file);
                let ft = self.number(node, "frameTop", 0.0, file);
                let fb = self.number(node, "frameBottom", 0.0, file);
                let width = if fr > fl {
                    ((fr - fl) / 20.0).clamp(1.0, 8192.0)
                } else {
                    default_w.clamp(1.0, 8192.0)
                };
                let height = if fb > ft {
                    ((fb - ft) / 20.0).clamp(1.0, 8192.0)
                } else {
                    default_h.clamp(1.0, 8192.0)
                };
                self.report.warning(
                    "xfl_video_approximated",
                    &location,
                    "DOMVideoInstance is imported as a Video viewport; embedded FLV/video stream data is not bundled.",
                );
                ElementKind::Video { width, height }
            }
            "DOMBitmapInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "frameLeft",
                        "frameRight",
                        "frameTop",
                        "frameBottom",
                    ],
                    &["matrix", "transformationPoint", "color", "filters"],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                    self.report.warning(
                        "transform_pivot",
                        &location,
                        "Placement matrix retained; Flash's 2D authoring centerPoint3DX/Y pivot is not retained. Actual 3D transforms remain unsupported.",
                    );
                }
                if node.attribute("name").is_some_and(|n| !n.is_empty()) {
                    self.report.warning(
                        "xfl_bitmap_instance_name",
                        &location,
                        "DOMBitmapInstance name is not addressable as a scriptable display object; wrap in a MovieClip symbol if script access is needed.",
                    );
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let Some(asset) = self.bitmaps.get(target).cloned() else {
                    self.report.error(
                        "missing_bitmap",
                        &location,
                        format!("Unknown bitmap library item {target:?}."),
                    );
                    return vec![];
                };
                name = String::new();
                ElementKind::Bitmap {
                    resource_id: asset.resource_id,
                    width: asset.width,
                    height: asset.height,
                    smoothing: asset.smoothing,
                }
            }
            "DOMStaticText" | "DOMDynamicText" | "DOMInputText" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "width",
                        "height",
                        "left",
                        "top",
                        "selected",
                        "locked",
                        "isSelectable",
                        "fontRenderingMode",
                        "antiAliasSharpness",
                        "antiAliasThickness",
                        "referenceID",
                        "uniqueIdentifier",
                        "autoExpand",
                        "renderAsHTML",
                        "lineType",
                        "includeOutlines",
                        "orientation",
                        "border",
                        "variableName",
                        "scrollable",
                        "maxCharacters",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "textRuns",
                        "persistentData",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                let _ = self.boolean(node, "isSelectable", true, file);
                let _ = self.boolean(node, "autoExpand", false, file);
                let render_as_html = self.boolean(node, "renderAsHTML", false, file);
                let _ = self.boolean(node, "includeOutlines", false, file);
                let border = self.boolean(node, "border", false, file);
                let _ = self.boolean(node, "scrollable", false, file);
                let _ = self.u32_attr(node, "maxCharacters", 0, file);
                let orientation = node.attribute("orientation").unwrap_or("horizontal");
                let line_type = node.attribute("lineType").unwrap_or("single line");
                let Some(runs_wrapper) = child(node, "textRuns") else {
                    if tag == "DOMStaticText" {
                        self.report.error(
                            "missing_text_runs",
                            &location,
                            "Static text has no textRuns.",
                        );
                        return vec![];
                    }
                    let left = self.number(node, "left", 0., file);
                    let top = self.number(node, "top", 0., file);
                    let matrix = transform_matrix(transform);
                    transform.x += matrix[0] * left + matrix[2] * top;
                    transform.y += matrix[1] * left + matrix[3] * top;
                    let width = self
                        .number(node, "width", 100., file)
                        .clamp(0.0, 1_000_000.0);
                    let height = self
                        .number(node, "height", 20., file)
                        .clamp(0.0, 1_000_000.0);
                    let multiline = matches!(line_type, "multiline" | "multiline no wrap");
                    let word_wrap = line_type == "multiline";
                    return vec![Element {
                        id,
                        accessibility: None,
                        name,
                        transform,
                        kind: ElementKind::TextBox {
                            text: String::new(),
                            width,
                            height,
                            format: TextFormat::default(),
                            word_wrap,
                            multiline,
                        },
                        scripts,
                        ..Element::default()
                    }];
                };
                self.audit(runs_wrapper, file, &[], &["DOMTextRun"]);
                let run_nodes: Vec<_> = elements(runs_wrapper)
                    .filter(|n| n.has_tag_name("DOMTextRun"))
                    .collect();
                let mut combined_text = String::new();
                let mut first_format: Option<TextFormat> = None;
                let mut first_color_rgba: Color = [0, 0, 0, 255];
                let mut first_raw_face: String = "Arial".into();
                let mut first_raw_size: f64 = 12.0;
                let mut any_nondefault_layout = false;
                let mut any_nonleft_align = false;
                for (ri, run_node) in run_nodes.iter().copied().enumerate() {
                    self.audit(run_node, file, &[], &["characters", "textAttrs"]);
                    let Some(characters) = child(run_node, "characters") else {
                        self.report.error(
                            "missing_text",
                            &location,
                            "Text run has no characters node.",
                        );
                        return vec![];
                    };
                    self.audit(characters, file, &[], &[]);
                    let run_text = immediate_text(characters);
                    combined_text.push_str(&run_text);
                    let Some(attrs_wrapper) = child(run_node, "textAttrs") else {
                        self.report.error(
                            "missing_text_style",
                            &location,
                            "Text run has no textAttrs.",
                        );
                        return vec![];
                    };
                    self.audit(attrs_wrapper, file, &[], &["DOMTextAttrs"]);
                    let Some(attrs) = child(attrs_wrapper, "DOMTextAttrs") else {
                        self.report
                            .error("missing_text_style", &location, "No DOMTextAttrs.");
                        return vec![];
                    };
                    self.audit(
                        attrs,
                        file,
                        &[
                            "face",
                            "size",
                            "fillColor",
                            "alpha",
                            "bitmapSize",
                            "aliasText",
                            "autoKern",
                            "alignment",
                            "letterSpacing",
                            "lineSpacing",
                            "lineHeight",
                            "leftMargin",
                            "rightMargin",
                            "indent",
                            "rotation",
                            "characterPosition",
                            "useScreenSpacing",
                            "bold",
                            "italic",
                            "underline",
                            "url",
                            "target",
                        ],
                        &[],
                    );
                    let _ = self.boolean(attrs, "aliasText", false, file);
                    let auto_kern = self.boolean(attrs, "autoKern", false, file);
                    let _ = self.boolean(attrs, "rotation", false, file);
                    let _ = self.boolean(attrs, "useScreenSpacing", false, file);
                    let bold = self.boolean(attrs, "bold", false, file);
                    let italic = self.boolean(attrs, "italic", false, file);
                    let underline = self.boolean(attrs, "underline", false, file);
                    let letter_spacing = self.number(attrs, "letterSpacing", 0., file);
                    let left_margin = self.number(attrs, "leftMargin", 0., file);
                    let right_margin = self.number(attrs, "rightMargin", 0., file);
                    let indent = self.number(attrs, "indent", 0., file);
                    let line_spacing = self.number(attrs, "lineSpacing", 0., file);
                    let _ = self.number(attrs, "lineHeight", 0., file);
                    if letter_spacing != 0.0
                        || left_margin != 0.0
                        || right_margin != 0.0
                        || indent != 0.0
                        || bold
                        || italic
                        || underline
                        || attrs.attribute("url").is_some_and(|s| !s.is_empty())
                        || attrs.attribute("target").is_some_and(|s| !s.is_empty())
                    {
                        any_nondefault_layout = true;
                    }
                    let align_str = attrs.attribute("alignment").unwrap_or("left");
                    if align_str != "left" {
                        any_nonleft_align = true;
                    }
                    let align = match align_str {
                        "center" => TextAlign::Center,
                        "right" => TextAlign::Right,
                        _ => TextAlign::Left,
                    };
                    let alpha = self.number(attrs, "alpha", 1., file);
                    let color = self.color(
                        attrs.attribute("fillColor").unwrap_or("#000000"),
                        alpha,
                        &location,
                    );
                    let raw_face = attrs.attribute("face").unwrap_or("Arial");
                    let mapped_face = self
                        .font_items
                        .get(raw_face)
                        .map(String::as_str)
                        .unwrap_or(raw_face);
                    let clean_face: String = mapped_face
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(256)
                        .collect();
                    let valid_face = if clean_face.trim().is_empty() {
                        "Arial".to_owned()
                    } else {
                        clean_face
                    };
                    let raw_size = self.number(attrs, "size", 12., file);
                    let valid_size = raw_size.clamp(0.5, 10_000.0);
                    let min_leading = -(valid_size * 1.2) + 0.5;
                    let valid_leading = line_spacing.max(min_leading).clamp(-10_000.0, 10_000.0);
                    let clean_url: String = attrs
                        .attribute("url")
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(1024)
                        .collect();
                    let clean_target: String = attrs
                        .attribute("target")
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(128)
                        .collect();
                    if ri == 0 {
                        first_color_rgba = color;
                        first_raw_face = valid_face.clone();
                        first_raw_size = valid_size;
                        first_format = Some(TextFormat {
                            font: valid_face,
                            size: valid_size,
                            color: (u32::from(color[0]) << 16)
                                | (u32::from(color[1]) << 8)
                                | u32::from(color[2]),
                            bold,
                            italic,
                            leading: valid_leading,
                            align,
                            left_margin: left_margin.clamp(0.0, 10_000.0),
                            right_margin: right_margin.clamp(0.0, 10_000.0),
                            indent: indent.clamp(-10_000.0, 10_000.0),
                            block_indent: 0.0,
                            underline,
                            letter_spacing: letter_spacing.clamp(-1_000.0, 1_000.0),
                            kerning: auto_kern.then_some(true),
                            bullet: false,
                            tab_stops: Vec::new(),
                            url: clean_url,
                            target: clean_target,
                        });
                    }
                }
                let left = self.number(node, "left", 0., file);
                let top = self.number(node, "top", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * left + matrix[2] * top;
                transform.y += matrix[1] * left + matrix[3] * top;
                if !self.text_warning {
                    self.report.warning("text_metrics",&location,"Font names and text are retained; line metrics/antialiasing may differ from Flash. Verify imported typography.");
                    self.text_warning = true;
                }
                let has_newlines = combined_text.contains(['\n', '\r']);
                if tag == "DOMStaticText"
                    && run_nodes.len() == 1
                    && !has_newlines
                    && !any_nondefault_layout
                    && !any_nonleft_align
                    && orientation == "horizontal"
                    && !render_as_html
                {
                    ElementKind::Text {
                        text: combined_text,
                        font_family: first_raw_face,
                        font_size: first_raw_size,
                        color: first_color_rgba,
                    }
                } else {
                    if tag == "DOMStaticText" {
                        if has_newlines {
                            self.report.warning(
                                "multiline_text",
                                &location,
                                "Multiline static text is imported as a bounded TextBox.",
                            );
                        }
                        if run_nodes.len() != 1 {
                            self.report.warning(
                                "rich_text",
                                &location,
                                "Multi-run static text is flattened into a single-format TextBox.",
                            );
                        }
                        if any_nondefault_layout
                            || any_nonleft_align
                            || orientation != "horizontal"
                            || render_as_html
                        {
                            self.report.warning(
                                "xfl_text_approximated",
                                &location,
                                "Static text paragraph layout/orientation/HTML attributes are approximated in a TextBox.",
                            );
                        }
                    } else if render_as_html
                        || border
                        || node
                            .attribute("variableName")
                            .is_some_and(|s| !s.is_empty())
                        || run_nodes.len() > 1
                    {
                        self.report.warning(
                            "xfl_text_approximated",
                            &location,
                            format!(
                                "{tag} authoring options (HTML/border/variableName/multi-run) are approximated in a TextBox."
                            ),
                        );
                    }
                    let normalized: String = normalize_text(&combined_text)
                        .chars()
                        .take(MAX_TEXT_BYTES / 4)
                        .collect();
                    let width = self
                        .number(node, "width", 100., file)
                        .clamp(0.0, 1_000_000.0);
                    let height = self
                        .number(node, "height", 20., file)
                        .clamp(0.0, 1_000_000.0);
                    let multiline =
                        has_newlines || matches!(line_type, "multiline" | "multiline no wrap");
                    let word_wrap = line_type == "multiline"
                        || (tag == "DOMStaticText"
                            && has_newlines
                            && line_type != "multiline no wrap");
                    if first_color_rgba[3] < 255 {
                        transform.alpha = (transform.alpha
                            * (f64::from(first_color_rgba[3]) / 255.0))
                            .clamp(0.0, 1.0);
                    }
                    ElementKind::TextBox {
                        text: normalized,
                        width,
                        height,
                        format: first_format.unwrap_or_default(),
                        word_wrap,
                        multiline,
                    }
                }
            }
            "DOMTLFText" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "selected",
                        "locked",
                        "left",
                        "top",
                        "right",
                        "bottom",
                        "width",
                        "height",
                        "referenceID",
                        "uniqueIdentifier",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "tlfFonts",
                        "markup",
                        "tlfTextObject",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(tlf_fonts) = child(node, "tlfFonts") {
                    self.audit(tlf_fonts, file, &[], &["TLFFont"]);
                    for tf in elements(tlf_fonts).filter(|n| n.has_tag_name("TLFFont")) {
                        self.audit(tf, file, &["platformName", "psName"], &[]);
                    }
                }
                let mut paragraphs = Vec::new();
                let mut format = TextFormat::default();
                if let Some(markup) = child(node, "markup").or_else(|| child(node, "tlfTextObject"))
                {
                    self.audit_tlf_subtree(markup, file);
                    for p_node in markup
                        .descendants()
                        .filter(|n| n.is_element() && n.tag_name().name() == "p")
                    {
                        let mut para = String::new();
                        for desc in p_node.descendants() {
                            if desc.is_text()
                                && let Some(t) = desc.text()
                            {
                                para.push_str(t);
                            } else if desc.is_element() && desc.tag_name().name() == "br" {
                                para.push('\n');
                            }
                        }
                        paragraphs.push(para);
                    }
                    if let Some(span) = markup
                        .descendants()
                        .find(|n| n.is_element() && n.tag_name().name() == "span")
                    {
                        if let Some(ff) = span
                            .attribute("fontFamily")
                            .map(str::trim)
                            .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                        {
                            format.font = ff.chars().take(256).collect();
                        }
                        if let Some(fs) = span
                            .attribute("fontSize")
                            .and_then(|s| s.parse::<f64>().ok())
                            .filter(|v| v.is_finite())
                        {
                            format.size = fs.clamp(0.5, 10_000.0);
                        }
                        if let Some(col_str) = span.attribute("color") {
                            let c = self.color(col_str, 1.0, &location);
                            format.color =
                                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
                        }
                    }
                }
                let left = self.number(node, "left", 0., file);
                let top = self.number(node, "top", 0., file);
                let right = self.number(node, "right", left + 100., file);
                let bottom = self.number(node, "bottom", top + 20., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * left + matrix[2] * top;
                transform.y += matrix[1] * left + matrix[3] * top;
                let width = self
                    .number(node, "width", (right - left).max(100.0), file)
                    .clamp(0.0, 1_000_000.0);
                let height = self
                    .number(node, "height", (bottom - top).max(20.0), file)
                    .clamp(0.0, 1_000_000.0);
                let text: String = normalize_text(&paragraphs.join("\n"))
                    .chars()
                    .take(MAX_TEXT_BYTES / 4)
                    .collect();
                let multiline = text.contains('\n');
                self.report.warning(
                    "xfl_tlf_text_approximated",
                    &location,
                    "DOMTLFText TextFlow markup is imported as a plain TextBox.",
                );
                ElementKind::TextBox {
                    text,
                    width,
                    height,
                    format,
                    word_wrap: true,
                    multiline,
                }
            }
            "DOMRectangleObject" | "DOMOvalObject" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "objectWidth",
                        "objectHeight",
                        "x",
                        "y",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "lockFlag",
                        "topLeftRadius",
                        "topRightRadius",
                        "bottomLeftRadius",
                        "bottomRightRadius",
                        "startAngle",
                        "endAngle",
                        "innerRadius",
                        "closePath",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "fill",
                        "stroke",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                let fill = if let Some(fill_wrapper) = child(node, "fill") {
                    self.audit(
                        fill_wrapper,
                        file,
                        &[],
                        &[
                            "SolidColor",
                            "LinearGradient",
                            "RadialGradient",
                            "BitmapFill",
                        ],
                    );
                    if let Some(solid) = child(fill_wrapper, "SolidColor") {
                        self.audit(solid, file, &["color", "alpha"], &[]);
                        let alpha = self.number(solid, "alpha", 1., file);
                        self.color(
                            solid.attribute("color").unwrap_or("#000000"),
                            alpha,
                            &location,
                        )
                    } else if let Some(grad) = child(fill_wrapper, "LinearGradient")
                        .or_else(|| child(fill_wrapper, "RadialGradient"))
                    {
                        self.audit_unmodeled_xfl_subtree(grad, file);
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "Gradient fill on primitive object is approximated using its first stop color.",
                        );
                        if let Some(entry) =
                            elements(grad).find(|n| n.has_tag_name("GradientEntry"))
                        {
                            let alpha = self.number(entry, "alpha", 1.0, file);
                            self.color(
                                entry.attribute("color").unwrap_or("#000000"),
                                alpha,
                                &location,
                            )
                        } else {
                            [0, 0, 0, 255]
                        }
                    } else if let Some(bf) = child(fill_wrapper, "BitmapFill") {
                        self.audit_unmodeled_xfl_subtree(bf, file);
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "BitmapFill on primitive object is approximated as solid gray.",
                        );
                        [128, 128, 128, 255]
                    } else {
                        [0, 0, 0, 0]
                    }
                } else {
                    [0, 0, 0, 0]
                };
                let (stroke, stroke_width) = if let Some(stroke_wrapper) = child(node, "stroke") {
                    self.audit(
                        stroke_wrapper,
                        file,
                        &[],
                        &[
                            "SolidStroke",
                            "DashedStroke",
                            "DottedStroke",
                            "RaggedStroke",
                            "StippleStroke",
                            "HatchedStroke",
                        ],
                    );
                    if let Some(stroke_node) = elements(stroke_wrapper).next() {
                        self.audit_unmodeled_xfl_subtree(stroke_node, file);
                        if stroke_node.tag_name().name() != "SolidStroke" {
                            self.report.warning(
                                "primitive_stroke",
                                &location,
                                "Patterned stroke on primitive object is approximated as a solid stroke.",
                            );
                        }
                        let w = self
                            .number(stroke_node, "weight", 1.0, file)
                            .clamp(0.0, 1e9);
                        let scolor = if let Some(sfill) = child(stroke_node, "fill")
                            && let Some(solid) = child(sfill, "SolidColor")
                        {
                            let alpha = self.number(solid, "alpha", 1.0, file);
                            self.color(
                                solid.attribute("color").unwrap_or("#000000"),
                                alpha,
                                &location,
                            )
                        } else {
                            [0, 0, 0, 255]
                        };
                        (scolor, w)
                    } else {
                        ([0, 0, 0, 0], 0.0)
                    }
                } else {
                    ([0, 0, 0, 0], 0.0)
                };
                let x = self.number(node, "x", 0., file);
                let y = self.number(node, "y", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * x + matrix[2] * y;
                transform.y += matrix[1] * x + matrix[3] * y;
                let width = self.number(node, "objectWidth", 0., file).clamp(0.0, 1e9);
                let height = self.number(node, "objectHeight", 0., file).clamp(0.0, 1e9);
                if tag == "DOMRectangleObject" {
                    let locked = self.boolean(node, "lockFlag", true, file);
                    let raw_radii = [
                        self.number(node, "topLeftRadius", 0., file),
                        self.number(node, "topRightRadius", 0., file),
                        self.number(node, "bottomLeftRadius", 0., file),
                        self.number(node, "bottomRightRadius", 0., file),
                    ];
                    if raw_radii.iter().any(|&r| r < 0.0) {
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "Negative (inverted) rectangle corner radii are clamped to 0.",
                        );
                    }
                    let corner_radii = raw_radii.map(|r| r.clamp(0.0, 1e9));
                    let opts = RectangleOptions {
                        corner_radii,
                        corners_locked: locked,
                    };
                    if !opts.is_default() {
                        transform.primitive = Some(PrimitiveOptions::Rectangle(opts));
                    }
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                } else {
                    let norm_angle = |a: f64| -> f64 {
                        if (a - 360.0).abs() <= 1e-6 {
                            360.0
                        } else {
                            a.rem_euclid(360.0)
                        }
                    };
                    let start_angle = norm_angle(self.number(node, "startAngle", 0., file));
                    let end_angle = norm_angle(self.number(node, "endAngle", 0., file));
                    let inner_radius = self.number(node, "innerRadius", 0., file).clamp(0.0, 99.0);
                    let close_path = self.boolean(node, "closePath", true, file);
                    let opts = OvalOptions {
                        start_angle,
                        end_angle,
                        inner_radius,
                        close_path,
                    };
                    if !opts.is_default() {
                        transform.primitive = Some(PrimitiveOptions::Oval(opts));
                    }
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                }
            }
            _ => return vec![],
        };
        vec![Element {
            id,
            accessibility,
            name,
            transform,
            kind,
            scripts,
            visible,
            cache_as_bitmap,
            blend_mode,
            filters,
            ..Element::default()
        }]
    }

    fn frame_tween(
        &mut self,
        frame_node: Node<'_, '_>,
        has_motion_xml: bool,
        file: &str,
    ) -> (Option<Easing>, TweenKind, bool) {
        let loc = self.location(frame_node, file);
        let accel = self.number(frame_node, "acceleration", 0., file);
        if !(-100.0..=100.0).contains(&accel) {
            self.report.error(
                "invalid_acceleration",
                &loc,
                "Tween acceleration must be within [-100, 100].",
            );
        }
        let has_custom_ease = self.boolean(frame_node, "hasCustomEase", false, file);
        let single_ease = self.boolean(frame_node, "useSingleEaseCurve", true, file);
        let mut custom_bezier = None;
        let mut named_ease_method: Option<String> =
            frame_node.attribute("easeMethodName").map(str::to_owned);
        if let Some(tweens) = child(frame_node, "tweens") {
            self.audit(tweens, file, &[], &["CustomEase", "Ease"]);
            for ease_preset in elements(tweens).filter(|n| n.has_tag_name("Ease")) {
                self.audit(ease_preset, file, &["target", "method", "intensity"], &[]);
                if named_ease_method.is_none()
                    && let Some(m) = ease_preset.attribute("method")
                {
                    named_ease_method = Some(m.to_owned());
                }
            }
            let eases: Vec<_> = elements(tweens)
                .filter(|n| n.has_tag_name("CustomEase"))
                .collect();
            if has_custom_ease || !eases.is_empty() {
                let chosen = eases
                    .iter()
                    .copied()
                    .find(|n| n.attribute("target") == Some("all"))
                    .or_else(|| eases.first().copied());
                if !single_ease || eases.len() > 1 {
                    self.report.warning(
                        "custom_ease_not_supported",
                        self.location(tweens, file),
                        "Multiple per-property XFL CustomEase curves are approximated with the primary curve.",
                    );
                }
                for ease_node in &eases {
                    self.audit(*ease_node, file, &["target"], &["Point"]);
                    for pt in elements(*ease_node).filter(|n| n.has_tag_name("Point")) {
                        self.audit(pt, file, &["x", "y"], &[]);
                        let _ = self.number(pt, "x", 0., file);
                        let _ = self.number(pt, "y", 0., file);
                    }
                }
                if let Some(ease_node) = chosen {
                    if ease_node.attribute("target") != Some("all") {
                        self.report.warning(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Per-property XFL CustomEase target is applied to the whole tween span.",
                        );
                    }
                    let pts: Vec<_> = elements(ease_node)
                        .filter(|n| n.has_tag_name("Point"))
                        .map(|pt| {
                            [
                                self.number(pt, "x", 0., file),
                                self.number(pt, "y", 0., file),
                            ]
                        })
                        .collect();
                    if pts.len() == 4 && pts[0] == [0., 0.] && pts[3] == [1., 1.] {
                        if !(0.0..=1.0).contains(&pts[1][0])
                            || !(0.0..=1.0).contains(&pts[2][0])
                            || pts[1][1].abs() > 100.0
                            || pts[2][1].abs() > 100.0
                        {
                            self.report.error(
                                "custom_ease_not_supported",
                                self.location(ease_node, file),
                                "CustomEase Bézier controls must have x in [0, 1] and |y| <= 100.",
                            );
                        } else {
                            custom_bezier = Some(Easing::CubicBezier {
                                x1: pts[1][0],
                                y1: pts[1][1],
                                x2: pts[2][0],
                                y2: pts[2][1],
                            });
                        }
                    } else {
                        self.report.warning(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Multi-segment XFL CustomEase curve is approximated with standard tween easing.",
                        );
                    }
                } else {
                    self.report.warning(
                        "custom_ease_not_supported",
                        self.location(tweens, file),
                        "hasCustomEase is true, but no CustomEase curve was supplied.",
                    );
                }
            }
        } else if has_custom_ease {
            self.report.error(
                "custom_ease_not_supported",
                &loc,
                "hasCustomEase is true, but no <tweens> CustomEase curve was supplied.",
            );
        }
        let ease = custom_bezier.unwrap_or_else(|| {
            if let Some(ref method) = named_ease_method {
                let m = method.to_ascii_lowercase();
                if m.contains("bounce") {
                    return Easing::Bounce;
                }
                if m.contains("elastic") {
                    return Easing::Elastic;
                }
                if m.contains("inout") || m.contains("in_out") || m.contains("in-out") {
                    return Easing::EaseInOut;
                }
                if m.contains("in") {
                    return Easing::EaseIn;
                }
                if m.contains("out") {
                    return Easing::EaseOut;
                }
            }
            let clamped = accel.clamp(-100.0, 100.0);
            if clamped == 0.0 {
                Easing::Linear
            } else {
                Easing::Flash {
                    acceleration: clamped,
                }
            }
        });
        let orient = self.boolean(frame_node, "motionTweenOrientToPath", false, file);
        self.boolean(frame_node, "motionTweenSnap", false, file);
        self.boolean(frame_node, "motionTweenScale", true, file);
        self.boolean(frame_node, "motionTweenSync", false, file);
        let rotate_times = self.integer(frame_node, "motionTweenRotateTimes", 0, file);
        let rotate_mode = frame_node.attribute("motionTweenRotate").unwrap_or("auto");
        if !matches!(
            rotate_mode,
            "none" | "auto" | "clockwise" | "counter-clockwise"
        ) {
            self.report.error(
                "invalid_tween_rotate",
                &loc,
                format!("Unsupported motionTweenRotate value {rotate_mode:?}."),
            );
        }
        let shape_blend = frame_node
            .attribute("shapeTweenBlend")
            .unwrap_or("distributive");
        if !matches!(shape_blend, "distributive" | "angular") {
            self.report.error(
                "invalid_shape_tween_blend",
                &loc,
                format!("Unsupported shapeTweenBlend value {shape_blend:?}."),
            );
        }
        match frame_node.attribute("tweenType") {
            None | Some("") | Some("none") => (None, TweenKind::Classic, false),
            Some("motion") => {
                if matches!(rotate_mode, "clockwise" | "counter-clockwise") || rotate_times > 0 {
                    self.report.warning(
                        "tween_rotation",
                        &loc,
                        "Directed classic-tween rotation (clockwise/counter-clockwise turns) is interpolated along the shortest angle.",
                    );
                }
                (Some(ease), TweenKind::Classic, orient)
            }
            Some("shape") => {
                if shape_blend == "angular" {
                    self.report.warning(
                        "shape_tween_blend",
                        &loc,
                        "Angular shape-tween vertex blending is approximated with distributive arc-length resampling.",
                    );
                }
                (Some(ease), TweenKind::Shape, false)
            }
            Some("motion object") => {
                if !has_motion_xml {
                    self.report.warning(
                        "motion_object_not_supported",
                        &loc,
                        "XFL motion-object property-track tween is approximated by holding the authored keyframe pose.",
                    );
                }
                (None, TweenKind::Classic, false)
            }
            Some("IK pose") => {
                self.report.warning(
                    "xfl_ik_approximated",
                    &loc,
                    "XFL inverse-kinematics IK pose tween is approximated using authored keyframe poses.",
                );
                (None, TweenKind::Classic, false)
            }
            Some(other) => {
                self.report.error(
                    "tween_not_supported",
                    &loc,
                    format!("Unsupported XFL tweenType {other:?}."),
                );
                (None, TweenKind::Classic, false)
            }
        }
    }

    fn transform(&mut self, node: Node<'_, '_>, file: &str) -> Transform {
        let mut t = Transform::default();
        if let Some(wrapper) = child(node, "matrix") {
            self.audit(wrapper, file, &[], &["Matrix"]);
            if let Some(matrix) = child(wrapper, "Matrix") {
                self.audit(matrix, file, &["a", "b", "c", "d", "tx", "ty"], &[]);
                let a = self.number(matrix, "a", 1., file);
                let b = self.number(matrix, "b", 0., file);
                let c = self.number(matrix, "c", 0., file);
                let d = self.number(matrix, "d", 1., file);
                let tx = self.number(matrix, "tx", 0., file);
                let ty = self.number(matrix, "ty", 0., file);
                let sx = a.hypot(b);
                let sy = c.hypot(d);
                let det = a * d - b * c;
                if sx < 1e-12 || sy < 1e-12 || det.abs() <= 1e-12 * sx * sy {
                    self.report.error(
                        "matrix_singular",
                        self.location(matrix, file),
                        "Singular or zero-scale placement matrices cannot be represented as an editable 2D transform.",
                    );
                } else if (a * c + b * d).abs() <= 1e-7 * (1. + sx * sy) {
                    t.scale_x = sx;
                    t.scale_y = det / sx;
                    t.rotation = b.atan2(a).to_degrees();
                    t.x = tx;
                    t.y = ty;
                } else if let Some(skewed) = Transform::from_matrix([a, b, c, d, tx, ty]) {
                    t.x = skewed.x;
                    t.y = skewed.y;
                    t.scale_x = skewed.scale_x;
                    t.scale_y = skewed.scale_y;
                    t.rotation = skewed.rotation;
                    t.skew = skewed.skew;
                } else {
                    self.report.error(
                        "matrix_singular",
                        self.location(matrix, file),
                        "Singular or out-of-range placement matrix cannot be represented.",
                    );
                }
            }
        }
        if let Some(wrapper) = child(node, "color") {
            self.audit(wrapper, file, &[], &["Color"]);
            if let Some(color) = child(wrapper, "Color") {
                self.audit(
                    color,
                    file,
                    &[
                        "alphaMultiplier",
                        "redMultiplier",
                        "greenMultiplier",
                        "blueMultiplier",
                        "alphaOffset",
                        "redOffset",
                        "greenOffset",
                        "blueOffset",
                        "tintColor",
                        "tintMultiplier",
                        "brightness",
                    ],
                    &[],
                );
                let has_only_alpha = color.attributes().all(|a| a.name() == "alphaMultiplier");
                if has_only_alpha {
                    t.alpha = self.number(color, "alphaMultiplier", 1., file);
                } else {
                    let loc = self.location(color, file);
                    let mut rm = self.number(color, "redMultiplier", 1., file);
                    let mut gm = self.number(color, "greenMultiplier", 1., file);
                    let mut bm = self.number(color, "blueMultiplier", 1., file);
                    let am = self.number(color, "alphaMultiplier", 1., file);
                    let mut ro = self.number(color, "redOffset", 0., file);
                    let mut go = self.number(color, "greenOffset", 0., file);
                    let mut bo = self.number(color, "blueOffset", 0., file);
                    let ao = self.number(color, "alphaOffset", 0., file);
                    if color.attribute("tintColor").is_some()
                        || color.attribute("tintMultiplier").is_some()
                    {
                        let tm = self
                            .number(color, "tintMultiplier", 0., file)
                            .clamp(0.0, 1.0);
                        let tc = self.color(
                            color.attribute("tintColor").unwrap_or("#000000"),
                            1.0,
                            &loc,
                        );
                        rm = 1.0 - tm;
                        gm = 1.0 - tm;
                        bm = 1.0 - tm;
                        ro = f64::from(tc[0]) * tm;
                        go = f64::from(tc[1]) * tm;
                        bo = f64::from(tc[2]) * tm;
                    } else if color.attribute("brightness").is_some() {
                        let br = self.number(color, "brightness", 0., file).clamp(-1.0, 1.0);
                        if br < 0.0 {
                            let m = 1.0 + br;
                            rm = m;
                            gm = m;
                            bm = m;
                        } else {
                            let m = 1.0 - br;
                            let off = 255.0 * br;
                            rm = m;
                            gm = m;
                            bm = m;
                            ro = off;
                            go = off;
                            bo = off;
                        }
                    }
                    t.alpha = am.clamp(0.0, 1.0);
                    t.color_transform = ColorTransform {
                        multipliers: [
                            rm.clamp(-10_000.0, 10_000.0),
                            gm.clamp(-10_000.0, 10_000.0),
                            bm.clamp(-10_000.0, 10_000.0),
                            1.0,
                        ],
                        offsets: [
                            ro.clamp(-255.0, 255.0),
                            go.clamp(-255.0, 255.0),
                            bo.clamp(-255.0, 255.0),
                            ao.clamp(-255.0, 255.0),
                        ],
                    };
                }
            }
        }
        if let Some(point) = child(node, "transformationPoint") {
            self.audit(point, file, &[], &["Point"]);
            if let Some(point) = child(point, "Point") {
                self.audit(point, file, &["x", "y"], &[]);
                if self.number(point, "x", 0., file) != 0.
                    || self.number(point, "y", 0., file) != 0.
                {
                    self.report.warning(
                        "transform_pivot",
                        self.location(point, file),
                        "Placement matrix retained; authoring transform pivot is not retained.",
                    );
                }
            }
        }
        t
    }

    fn u64_attr(&mut self, node: Node<'_, '_>, attr: &str, default: u64, file: &str) -> u64 {
        let Some(raw) = node.attribute(attr) else {
            return default;
        };
        match raw.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                self.report.error(
                    "invalid_number",
                    self.location(node, file),
                    format!("{attr} must be an unsigned integer."),
                );
                default
            }
        }
    }

    fn u32_attr(&mut self, node: Node<'_, '_>, attr: &str, default: u32, file: &str) -> u32 {
        let Some(raw) = node.attribute(attr) else {
            return default;
        };
        match raw.parse::<u32>() {
            Ok(v) => v,
            Err(_) => {
                self.report.error(
                    "invalid_number",
                    self.location(node, file),
                    format!("{attr} must be a 32-bit unsigned integer."),
                );
                default
            }
        }
    }

    fn parse_media(&mut self, media: Node<'_, '_>, binary_files: &BTreeMap<String, Vec<u8>>) {
        self.audit(
            media,
            "DOMDocument.xml",
            &[],
            &[
                "DOMBitmapItem",
                "DOMSoundItem",
                "DOMVideoItem",
                "DOMCompiledClipItem",
            ],
        );
        for item in elements(media) {
            match item.tag_name().name() {
                "DOMBitmapItem" => self.parse_bitmap_item(item, binary_files),
                "DOMSoundItem" => self.parse_sound_item(item, binary_files),
                "DOMVideoItem" => self.parse_video_item(item),
                "DOMCompiledClipItem" => self.parse_compiled_clip_item(item),
                _ => {
                    self.report.error(
                        "unsupported_assets",
                        self.location(item, "DOMDocument.xml"),
                        "Unsupported media library asset is not imported yet.",
                    );
                }
            }
        }
    }

    fn parse_video_item(&mut self, item: Node<'_, '_>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "href",
                "videoDataHRef",
                "videoType",
                "fps",
                "width",
                "height",
                "length",
                "channels",
                "rate",
                "bits",
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
        let name = item.attribute("name").unwrap_or("");
        let w = self
            .number(item, "width", 320.0, "DOMDocument.xml")
            .clamp(1.0, 8192.0);
        let h = self
            .number(item, "height", 240.0, "DOMDocument.xml")
            .clamp(1.0, 8192.0);
        if !name.is_empty() {
            self.videos.insert(name.to_owned(), (w, h));
        }
        self.report.warning(
            "xfl_video_approximated",
            loc,
            "DOMVideoItem metadata is registered as a Video viewport; embedded FLV/video stream data is not bundled.",
        );
    }

    fn parse_compiled_clip_item(&mut self, item: Node<'_, '_>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "lastModified",
                "tooltip",
                "customIconID",
                "actionscriptClass",
                "swfScmHRef",
                "swfScmSourceFilename",
                "persistLivePreview11",
                "livePreviewScmHRef",
                "livePreviewScmSourceFilename",
                "livePreview11ScmHRef",
                "livePreview11ScmSourceFilename",
                "editFrameIndex",
                "requiredMinimumPlayerVersion",
                "requiredMinimumASVersion",
                "parametersAreLocked",
                "swcPath",
                "rootSymbolLinkageID",
                "playerVersion",
                "actionscriptVersion",
                "displayAsComponent",
                "hashKey",
                "movieRectLeft",
                "movieRectTop",
                "movieRectRight",
                "movieRectBottom",
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
                "classProperties",
                "customIcon",
                "SwcItem",
                "parametersAsXML",
                "dataBindingXML",
                "persistentData",
            ],
        );
        if let Some(pd_node) = child(item, "persistentData") {
            self.audit_persistent_data(pd_node, "DOMDocument.xml");
        }
        for meta_tag in [
            "classProperties",
            "customIcon",
            "SwcItem",
            "parametersAsXML",
            "dataBindingXML",
        ] {
            if let Some(sub) = child(item, meta_tag) {
                self.audit_unmodeled_xfl_subtree(sub, "DOMDocument.xml");
            }
        }
        let name = item.attribute("name").unwrap_or("");
        if !name.is_empty() && !self.symbols.contains_key(name) {
            let idx = self.extra_symbols.len() + 1;
            let sid = format!("xfl-compiled-clip-{idx}");
            let lid = format!("xfl-compiled-clip-layer-{idx}");
            let w = (self.number(item, "movieRectRight", 2000.0, "DOMDocument.xml")
                - self.number(item, "movieRectLeft", 0.0, "DOMDocument.xml"))
                / 20.0;
            let h = (self.number(item, "movieRectBottom", 2000.0, "DOMDocument.xml")
                - self.number(item, "movieRectTop", 0.0, "DOMDocument.xml"))
                / 20.0;
            let rect_el = Element {
                id: format!("xfl-compiled-clip-el-{idx}"),
                name: String::new(),
                accessibility: None,
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: w.clamp(10.0, 4096.0),
                    height: h.clamp(10.0, 4096.0),
                    fill: [180, 180, 180, 64],
                    stroke: [120, 120, 120, 160],
                    stroke_width: 1.0,
                },
                scripts: vec![],
                ..Element::default()
            };
            let display_name = name.rsplit_once('/').map_or(name, |(_, b)| b);
            self.extra_symbols.push(Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id: sid.clone(),
                name: display_name.to_owned(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    guides: vec![],
                    frame_count: 1,
                    layers: vec![Layer {
                        properties: LayerProperties::default(),
                        id: lid,
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![rect_el],
                            ..Default::default()
                        }],
                    }],
                    scripts: vec![],
                },
            });
            self.symbols
                .insert(name.to_owned(), (sid, SymbolKind::MovieClip));
        }
        self.report.warning(
            "xfl_compiled_clip_approximated",
            loc,
            "DOMCompiledClipItem SWC/compiled clip is imported as a placeholder MovieClip symbol.",
        );
    }

    fn register_resource(
        &mut self,
        resource_id: &str,
        path: String,
        item_name: &str,
        media_type: &str,
        bytes: &[u8],
        location: &str,
    ) -> Option<ResourceUnit> {
        if self.resources.len() >= MAX_RESOURCES {
            self.report.error(
                "xfl_media_invalid",
                location,
                "Project exceeds the 1024-resource limit.",
            );
            return None;
        }
        let Some(next_total) = self
            .resource_bytes_total
            .checked_add(bytes.len())
            .filter(|&n| n <= MAX_RESOURCE_TOTAL_BYTES)
        else {
            self.report.error(
                "xfl_media_invalid",
                location,
                "Decoded media resources exceed the 64 MiB project budget.",
            );
            return None;
        };
        let unit = match ResourceUnit::from_bytes(resource_id, path, media_type, bytes) {
            Ok(unit) => unit,
            Err(err) => {
                self.report.error(
                    "xfl_media_invalid",
                    location,
                    format!("Invalid media resource {item_name:?}: {err}"),
                );
                return None;
            }
        };
        self.resource_bytes_total = next_total;
        self.resources.push(unit.clone());
        Some(unit)
    }

    fn parse_bitmap_item(&mut self, item: Node<'_, '_>, binary_files: &BTreeMap<String, Vec<u8>>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "useImportedJPEGData",
                "compressionType",
                "originalCompressionType",
                "quality",
                "href",
                "bitmapDataHRef",
                "frameLeft",
                "frameTop",
                "frameRight",
                "frameBottom",
                "allowSmoothing",
                "useDeblocking",
                "isJPEG",
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
        let smoothing = self.boolean(item, "allowSmoothing", false, "DOMDocument.xml");
        let _ = self.boolean(item, "useImportedJPEGData", true, "DOMDocument.xml");
        let _ = self.boolean(item, "useDeblocking", false, "DOMDocument.xml");
        let _ = self.boolean(item, "isJPEG", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportInFirstFrame", true, "DOMDocument.xml");
        let export_as = self.boolean(item, "linkageExportForAS", false, "DOMDocument.xml");
        let export_rs = self.boolean(item, "linkageExportForRS", false, "DOMDocument.xml");
        let import_rs = self.boolean(item, "linkageImportForRS", false, "DOMDocument.xml");
        if export_as
            || export_rs
            || import_rs
            || item
                .attribute("linkageClassName")
                .is_some_and(|s| !s.is_empty())
            || item
                .attribute("linkageIdentifier")
                .is_some_and(|s| !s.is_empty())
        {
            self.report.warning(
                "xfl_bitmap_linkage_unsupported",
                &loc,
                "ActionScript BitmapData linkage on XFL bitmap items is not bound as a runtime class yet; the bitmap image is imported.",
            );
        }

        let name = item.attribute("name").unwrap_or("");
        if name.is_empty() || self.bitmaps.contains_key(name) {
            self.report.error(
                "xfl_media_invalid",
                &loc,
                "DOMBitmapItem name must be nonempty and unique.",
            );
            return;
        }

        let mut external_bytes: Option<&[u8]> = None;
        if let Some(href) = item.attribute("href").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{href}"))
                .or_else(|| get_binary_ci(binary_files, href));
        }
        if external_bytes.is_none() && source::safe_name(name).is_ok() {
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{name}"))
                .or_else(|| get_binary_ci(binary_files, name));
        }

        let idx = self.bitmaps.len() + 1;
        let resource_id = format!("xfl-bitmap-{idx}");

        let has_dat_href = item
            .attribute("bitmapDataHRef")
            .is_some_and(|s| !s.is_empty());

        if let Some(raw) = external_bytes {
            let owned_png: Option<Vec<u8>> =
                if detect_raster_format(raw).is_none() && raw.starts_with(b"BM") {
                    image::load_from_memory_with_format(raw, image::ImageFormat::Bmp)
                        .ok()
                        .and_then(|img| {
                            let mut buf = std::io::Cursor::new(Vec::new());
                            img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
                            Some(buf.into_inner())
                        })
                } else {
                    None
                };
            let effective_raw = owned_png.as_deref().unwrap_or(raw);
            if let Some((media_type, ext)) = detect_raster_format(effective_raw) {
                let Some(unit) = self.register_resource(
                    &resource_id,
                    build_xfl_resource_path("assets/bitmaps", idx, name, ext),
                    name,
                    media_type,
                    effective_raw,
                    &loc,
                ) else {
                    return;
                };
                match unit.decode_image() {
                    Ok(decoded) => {
                        self.bitmaps.insert(
                            name.to_owned(),
                            XflBitmapAsset {
                                resource_id,
                                width: decoded.width,
                                height: decoded.height,
                                smoothing,
                            },
                        );
                        return;
                    }
                    Err(err) => {
                        self.resources.pop();
                        if !has_dat_href {
                            self.report.error(
                                "xfl_media_invalid",
                                &loc,
                                format!("Cannot decode bitmap image {name:?}: {err}"),
                            );
                            return;
                        }
                    }
                }
            } else if !has_dat_href {
                self.report.error(
                    "xfl_media_invalid",
                    &loc,
                    format!("Bitmap file for {name:?} is neither PNG, JPEG nor BMP."),
                );
                return;
            }
        }

        if let Some(dat_href) = item.attribute("bitmapDataHRef").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(dat_href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            if let Some(raw) = get_binary_ci(binary_files, &format!("bin/{dat_href}"))
                .or_else(|| get_binary_ci(binary_files, dat_href))
            {
                let (bytes_vec, media_type, ext) = if let Some((mt, ext)) =
                    detect_raster_format(raw)
                {
                    (raw.to_vec(), mt, ext)
                } else {
                    match crate::legacy::media::decode_binary_bitmap(raw, 0)
                        .and_then(|decoded| decoded.png_bytes())
                    {
                        Ok(png) => (png, "image/png", "png"),
                        Err(err) => {
                            self.report.warning(
                                    "xfl_media_invalid",
                                    &loc,
                                    format!(
                                        "Cannot decode binary bitmap cache {dat_href:?} ({err}); substituted a 1x1 transparent PNG placeholder."
                                    ),
                                );
                            (SYNTHETIC_1X1_PNG.to_vec(), "image/png", "png")
                        }
                    }
                };
                let Some(unit) = self.register_resource(
                    &resource_id,
                    build_xfl_resource_path("assets/bitmaps", idx, name, ext),
                    name,
                    media_type,
                    &bytes_vec,
                    &loc,
                ) else {
                    return;
                };
                match unit.decode_image() {
                    Ok(decoded) => {
                        self.bitmaps.insert(
                            name.to_owned(),
                            XflBitmapAsset {
                                resource_id,
                                width: decoded.width,
                                height: decoded.height,
                                smoothing,
                            },
                        );
                    }
                    Err(err) => {
                        self.resources.pop();
                        self.report.error(
                            "xfl_media_invalid",
                            &loc,
                            format!("Cannot decode converted bitmap for {name:?}: {err}"),
                        );
                    }
                }
                return;
            }
        }

        self.report.error(
            "xfl_media_invalid",
            &loc,
            format!("Referenced bitmap file for {name:?} is missing."),
        );
    }

    fn parse_sound_item(&mut self, item: Node<'_, '_>, binary_files: &BTreeMap<String, Vec<u8>>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "href",
                "soundDataHRef",
                "format",
                "sampleCount",
                "dataLength",
                "exportFormat",
                "exportBits",
                "sampleRate",
                "bits",
                "channels",
                "cacheFormat",
                "cachedSampleCount",
                "exportNative",
                "use8kSampleRate",
                "deviceSoundHRef",
                "isangan",
                "lastModified",
                "compressionType",
                "quality",
                "bitRate",
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
        let _ = self.boolean(item, "exportNative", false, "DOMDocument.xml");
        let _ = self.boolean(item, "use8kSampleRate", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportInFirstFrame", true, "DOMDocument.xml");
        let export_as = self.boolean(item, "linkageExportForAS", false, "DOMDocument.xml");
        let export_rs = self.boolean(item, "linkageExportForRS", false, "DOMDocument.xml");
        let import_rs = self.boolean(item, "linkageImportForRS", false, "DOMDocument.xml");
        if export_as
            || export_rs
            || import_rs
            || item
                .attribute("linkageClassName")
                .is_some_and(|s| !s.is_empty())
            || item
                .attribute("linkageIdentifier")
                .is_some_and(|s| !s.is_empty())
        {
            self.report.warning(
                "xfl_sound_linkage_unsupported",
                &loc,
                "ActionScript Sound linkage on XFL sound items is not bound as a runtime class yet; the audio asset is imported.",
            );
        }

        let sample_count = self.u32_attr(item, "sampleCount", 0, "DOMDocument.xml");
        let data_length = item
            .attribute("dataLength")
            .map(|_| self.u32_attr(item, "dataLength", 0, "DOMDocument.xml"));

        let name = item.attribute("name").unwrap_or("");
        if name.is_empty() || self.sounds.contains_key(name) {
            self.report.error(
                "xfl_media_invalid",
                &loc,
                "DOMSoundItem name must be nonempty and unique.",
            );
            return;
        }

        let format_str = item.attribute("format").unwrap_or("44kHz 16bit Stereo");
        let (base_flags, fmt_sample_rate) = parse_xfl_sound_format_flags(format_str);

        let mut external_bytes: Option<&[u8]> = None;
        if let Some(href) = item.attribute("href").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{href}"))
                .or_else(|| get_binary_ci(binary_files, href));
        }
        if external_bytes.is_none() && source::safe_name(name).is_ok() {
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{name}"))
                .or_else(|| get_binary_ci(binary_files, name));
        }

        let idx = self.sounds.len() + 1;
        let resource_id = format!("xfl-sound-{idx}");

        if let Some(raw) = external_bytes {
            if raw.len() >= 12 && &raw[0..4] == b"RIFF" && &raw[8..12] == b"WAVE" {
                let Some(duration_44) = parse_wav_duration_44(raw) else {
                    self.report.error(
                        "xfl_media_invalid",
                        &loc,
                        format!("Malformed WAV audio file for {name:?}."),
                    );
                    return;
                };
                if self
                    .register_resource(
                        &resource_id,
                        build_xfl_resource_path("assets/sounds", idx, name, "wav"),
                        name,
                        "audio/wav",
                        raw,
                        &loc,
                    )
                    .is_some()
                {
                    self.sounds.insert(
                        name.to_owned(),
                        XflSoundAsset {
                            resource_id,
                            duration_44,
                        },
                    );
                }
                return;
            }
            if looks_like_xfl_mp3(raw) {
                let duration_44 = if sample_count > 0 {
                    (u64::from(sample_count) * 44_100) / u64::from(fmt_sample_rate)
                } else {
                    0
                };
                if self
                    .register_resource(
                        &resource_id,
                        build_xfl_resource_path("assets/sounds", idx, name, "mp3"),
                        name,
                        "audio/mpeg",
                        raw,
                        &loc,
                    )
                    .is_some()
                {
                    self.sounds.insert(
                        name.to_owned(),
                        XflSoundAsset {
                            resource_id,
                            duration_44,
                        },
                    );
                }
                return;
            }
            self.report.error(
                "xfl_media_invalid",
                &loc,
                format!("Audio file for {name:?} is neither valid WAV nor MP3."),
            );
            return;
        }

        if let Some(dat_href) = item.attribute("soundDataHRef").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(dat_href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            if let Some(raw_slice) = get_binary_ci(binary_files, &format!("bin/{dat_href}"))
                .or_else(|| get_binary_ci(binary_files, dat_href))
            {
                let mut raw = raw_slice;
                let mut flags = base_flags;
                let href_lower = item.attribute("href").unwrap_or(name).to_ascii_lowercase();
                let fmt_lower = format_str.to_ascii_lowercase();
                let is_mp3 = (data_length.is_some_and(|d| d > 0)
                    || href_lower.ends_with(".mp3")
                    || fmt_lower.contains("mp3"))
                    && looks_like_xfl_mp3(raw);
                let mut effective_samples = sample_count;
                let encoded_byte_count = if is_mp3 {
                    flags |= 0x20;
                    let dl = data_length.filter(|&d| d > 0).unwrap_or(raw.len() as u32);
                    raw = &raw[..(dl as usize).min(raw.len())];
                    Some(dl)
                } else if fmt_lower.contains("adpcm") {
                    flags |= 0x10;
                    data_length
                } else {
                    let block_align = usize::from(1 + u16::from((flags & 2) != 0))
                        * usize::from(1 + u16::from((flags & 1) != 0));
                    if effective_samples == 0 && raw.len() >= block_align {
                        effective_samples = (raw.len() / block_align) as u32;
                    } else if effective_samples > 0 {
                        let expected = (effective_samples as usize).saturating_mul(block_align);
                        if expected > 0 && raw.len() > expected && raw.len() % block_align == 0 {
                            raw = &raw[..expected];
                        }
                    }
                    None
                };

                let settings = crate::legacy::contents::ContentsSoundSettings {
                    format_flags: flags,
                    sample_count: effective_samples,
                    encoded_byte_count,
                    secondary_format_flags: None,
                    secondary_sample_count: None,
                    ..Default::default()
                };
                match crate::legacy::media::decode_binary_sound(raw, 8, &settings) {
                    Ok(decoded) => {
                        if decoded.short_read {
                            self.report.warning(
                                "xfl_sound_short_read",
                                &loc,
                                "Binary sound cache ended before declared sample count; available audio frames were imported.",
                            );
                        }
                        if self
                            .register_resource(
                                &resource_id,
                                build_xfl_resource_path(
                                    "assets/sounds",
                                    idx,
                                    name,
                                    decoded.extension,
                                ),
                                name,
                                decoded.media_type,
                                &decoded.bytes,
                                &loc,
                            )
                            .is_some()
                        {
                            self.sounds.insert(
                                name.to_owned(),
                                XflSoundAsset {
                                    resource_id,
                                    duration_44: u64::from(decoded.duration_44),
                                },
                            );
                        }
                    }
                    Err(err) => {
                        self.report.error(
                            "xfl_media_invalid",
                            &loc,
                            format!("Cannot decode binary sound cache {dat_href:?}: {err}"),
                        );
                    }
                }
                return;
            }
        }

        self.report.error(
            "xfl_media_invalid",
            &loc,
            format!("Referenced audio file for {name:?} is missing."),
        );
    }

    fn frame_sound(&mut self, frame_node: Node<'_, '_>, file: &str) -> Option<FrameSound> {
        let loc = self.location(frame_node, file);
        let mut raw_env_points: Vec<(u64, u32, u32)> = Vec::new();
        let has_env_node = if let Some(env_node) = child(frame_node, "SoundEnvelope") {
            self.audit(env_node, file, &[], &["SoundEnvelopePoint"]);
            for pt in elements(env_node).filter(|n| n.has_tag_name("SoundEnvelopePoint")) {
                self.audit(pt, file, &["mark44", "level0", "level1"], &[]);
                let mark44 = self.u64_attr(pt, "mark44", 0, file);
                let level0 = self.u32_attr(pt, "level0", 32_768, file);
                let level1 = self.u32_attr(pt, "level1", 32_768, file);
                raw_env_points.push((mark44, level0, level1));
            }
            if raw_env_points.is_empty() || raw_env_points.windows(2).any(|w| w[1].0 < w[0].0) {
                self.report.error(
                    "invalid_sound_envelope",
                    &loc,
                    "SoundEnvelope must contain at least one SoundEnvelopePoint sorted by mark44.",
                );
            }
            true
        } else {
            false
        };
        let raw_loops = self.u64_attr(frame_node, "soundLoop", 1, file);
        let in_point = self.u64_attr(frame_node, "inPoint44", 0, file);
        let out_point = self.u64_attr(frame_node, "outPoint44", 0, file);
        let _ = self.u64_attr(frame_node, "soundZoomLevel", 0, file);

        let Some(sound_name) = frame_node.attribute("soundName").filter(|s| !s.is_empty()) else {
            if has_env_node {
                self.report.error(
                    "invalid_sound_envelope",
                    &loc,
                    "SoundEnvelope requires a nonempty soundName on DOMFrame.",
                );
            }
            return None;
        };
        let Some(asset) = self.sounds.get(sound_name).cloned() else {
            self.report.error(
                "missing_sound",
                &loc,
                format!("Unknown sound library item {sound_name:?}."),
            );
            return None;
        };

        let sync = match frame_node.attribute("soundSync").unwrap_or("event") {
            "event" => SoundSync::Event,
            "start" => SoundSync::Start,
            "stop" => SoundSync::Stop,
            "stream" => SoundSync::Stream,
            other => {
                self.report.error(
                    "invalid_sound_sync",
                    &loc,
                    format!("Unsupported soundSync value {other:?}."),
                );
                SoundSync::Event
            }
        };

        let (loops, loop_forever) = match frame_node.attribute("soundLoopMode") {
            Some("loop") => (1, true),
            None | Some("repeat") => {
                if raw_loops > u64::from(FrameSound::MAX_LOOPS) {
                    self.report.warning(
                        "xfl_frame_sound",
                        &loc,
                        format!(
                            "Frame-sound repeat count {raw_loops} clamped to {}.",
                            FrameSound::MAX_LOOPS
                        ),
                    );
                    (FrameSound::MAX_LOOPS, false)
                } else {
                    ((raw_loops as u32).max(1), false)
                }
            }
            Some(other) => {
                self.report.error(
                    "invalid_sound_loop_mode",
                    &loc,
                    format!("Unsupported soundLoopMode value {other:?}."),
                );
                (1, false)
            }
        };

        let (effect, envelope) = match frame_node.attribute("soundEffect") {
            Some("none") => (SoundEffect::None, None),
            Some("left channel") => (SoundEffect::LeftChannel, None),
            Some("right channel") => (SoundEffect::RightChannel, None),
            Some("fade left to right") => (SoundEffect::FadeLeftToRight, None),
            Some("fade right to left") => (SoundEffect::FadeRightToLeft, None),
            Some("fade in") => (SoundEffect::FadeIn, None),
            Some("fade out") => (SoundEffect::FadeOut, None),
            Some("custom") => {
                if raw_env_points.is_empty() {
                    (SoundEffect::None, None)
                } else {
                    let points = build_xfl_sound_envelope(
                        raw_env_points,
                        asset.duration_44,
                        &loc,
                        self.report,
                    );
                    (SoundEffect::Custom, Some(points))
                }
            }
            None => {
                let is_unity = raw_env_points.is_empty()
                    || raw_env_points
                        .iter()
                        .all(|&(_, l0, l1)| l0 == 32_768 && l1 == 32_768);
                if is_unity {
                    (SoundEffect::None, None)
                } else {
                    let points = build_xfl_sound_envelope(
                        raw_env_points,
                        asset.duration_44,
                        &loc,
                        self.report,
                    );
                    (SoundEffect::Custom, Some(points))
                }
            }
            Some(other) => {
                self.report.error(
                    "invalid_sound_effect",
                    &loc,
                    format!("Unsupported soundEffect value {other:?}."),
                );
                (SoundEffect::None, None)
            }
        };

        let in_point_opt = (in_point > 0)
            .then(|| (in_point as f64 / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS));
        let out_point_opt = (out_point > 0 && out_point < 0x3fff_ffff)
            .then(|| (out_point as f64 / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS))
            .filter(|&out_s| in_point_opt.is_none_or(|in_s| out_s > in_s));

        Some(FrameSound {
            resource: asset.resource_id,
            sync,
            loops,
            loop_forever,
            effect,
            envelope,
            in_point: in_point_opt,
            out_point: out_point_opt,
        })
    }
}

fn build_xfl_sound_envelope(
    mut raw_points: Vec<(u64, u32, u32)>,
    asset_duration_44: u64,
    loc: &str,
    report: &mut ImportReport,
) -> Vec<[f64; 3]> {
    raw_points.sort_by_key(|p| p.0);
    if raw_points.len() > FrameSound::MAX_ENVELOPE_POINTS {
        raw_points.truncate(FrameSound::MAX_ENVELOPE_POINTS);
        report.warning(
            "xfl_frame_sound",
            loc,
            format!(
                "Custom sound envelope truncated to {} control points.",
                FrameSound::MAX_ENVELOPE_POINTS
            ),
        );
    }
    if raw_points
        .iter()
        .any(|&(_, l0, l1)| l0 > 32_768 || l1 > 32_768)
    {
        report.warning(
            "xfl_frame_sound",
            loc,
            "Sound envelope gain above unity (>32768) is clamped to 1.0.",
        );
    }
    let max_mark = raw_points.iter().map(|p| p.0).max().unwrap_or(0);
    let denom = asset_duration_44.max(max_mark);
    let mut prev_pos = 0.0_f64;
    let mut points = Vec::with_capacity(raw_points.len());
    for (mark44, level0, level1) in raw_points {
        let pos = if denom == 0 {
            0.0
        } else {
            (mark44 as f64 / denom as f64).clamp(0.0, 1.0)
        }
        .max(prev_pos);
        prev_pos = pos;
        let left = (f64::from(level0) / 32_768.0).clamp(0.0, 1.0);
        let right = (f64::from(level1) / 32_768.0).clamp(0.0, 1.0);
        points.push([pos, left, right]);
    }
    points
}

fn build_xfl_resource_path(prefix: &str, idx: usize, item_name: &str, ext: &str) -> String {
    let base = item_name.rsplit_once('/').map_or(item_name, |(_, b)| b);
    let dot_ext = format!(".{ext}");
    let stem = if base.len() > dot_ext.len()
        && base[base.len() - dot_ext.len()..].eq_ignore_ascii_case(&dot_ext)
    {
        &base[..base.len() - dot_ext.len()]
    } else {
        base
    };
    let mut sanitized = String::with_capacity(stem.len().min(64));
    for ch in stem.chars().take(64) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    let trimmed = sanitized.trim_matches('_');
    let final_stem = if trimmed.is_empty() { "asset" } else { trimmed };
    format!("{prefix}/{idx}-{final_stem}.{ext}")
}

fn detect_raster_format(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else {
        None
    }
}

fn parse_xfl_sound_format_flags(format_str: &str) -> (u16, u32) {
    let lower = format_str.to_ascii_lowercase();
    let mut flags = 0u16;
    if lower.contains("stereo") {
        flags |= 0x01;
    }
    if lower.contains("16bit") || lower.contains("16 bit") {
        flags |= 0x02;
    }
    let sample_rate = if lower.contains("44khz") || lower.contains("44.1khz") {
        flags |= 0x0c;
        44_100
    } else if lower.contains("22khz") || lower.contains("22.05khz") {
        flags |= 0x08;
        22_050
    } else if lower.contains("11khz") || lower.contains("11.025khz") {
        flags |= 0x04;
        11_025
    } else if lower.contains("5.5khz") || lower.contains("5khz") || lower.contains("8khz") {
        5_512
    } else {
        flags |= 0x0c;
        44_100
    };
    (flags, sample_rate)
}

fn classic_tween_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (
                ElementKind::Bitmap { resource_id: a, .. },
                ElementKind::Bitmap { resource_id: b, .. },
            ) => a == b,
            (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
            | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. })
            | (ElementKind::Path { .. }, ElementKind::Path { .. })
            | (ElementKind::PaintedPath { .. }, ElementKind::PaintedPath { .. })
            | (ElementKind::Text { .. }, ElementKind::Text { .. })
            | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. })
            | (ElementKind::Video { .. }, ElementKind::Video { .. }) => true,
            _ => false,
        }
}

const SYNTHETIC_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x60,
    0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0xA5, 0xF6, 0x45, 0x40, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

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
    ] {
        if let Some(rest) = source.strip_prefix(prefix) {
            return Some((lang, rest.to_owned()));
        }
    }
    None
}

fn parse_neoflash_object_scripts(source: &str) -> Vec<ObjectScript> {
    if !source.contains("/* neoflash:language=") {
        return Vec::new();
    }
    let trimmed = source.trim();
    if let Some((lang, clean)) = strip_neoflash_language_marker(trimmed) {
        return vec![ObjectScript {
            event: ObjectEvent::Load,
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

fn normalize_xfl_playback(symbols: &mut [Symbol], report: &mut ImportReport) {
    let frame_counts: HashMap<String, u32> = symbols
        .iter()
        .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
        .collect();
    for sym in symbols.iter_mut() {
        let sym_name = sym.name.clone();
        for layer in &mut sym.timeline.layers {
            for key in &mut layer.keyframes {
                for el in &mut key.elements {
                    if let ElementKind::Instance {
                        symbol_id,
                        playback,
                    } = &mut el.kind
                        && let Some(&fc) = frame_counts.get(symbol_id)
                    {
                        let max_idx = fc.saturating_sub(1);
                        match playback {
                            InstancePlayback::SingleFrame(f)
                            | InstancePlayback::LoopFrom(f)
                            | InstancePlayback::PlayOnceFrom(f)
                                if *f >= fc =>
                            {
                                *f = max_idx;
                                report.warning(
                                    "xfl_playback_frame_clamped",
                                    format!("{sym_name}:frame {}", key.frame + 1),
                                    format!(
                                        "Graphic instance firstFrame clamped to last frame ({}) of target symbol.",
                                        max_idx + 1
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn is_pure_vector_mask_symbol(
    sym_id: &str,
    symbols: &HashMap<&str, &Symbol>,
    visiting: &mut HashSet<String>,
) -> bool {
    let Some(sym) = symbols.get(sym_id) else {
        return false;
    };
    if !visiting.insert(sym_id.to_owned()) {
        return false;
    }
    let ok = sym
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.role != LayerRole::Guide)
        .all(|l| {
            l.properties.role != LayerRole::Mask
                && l.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                    matches!(
                        el.kind,
                        ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. }
                    ) || matches!(
                        &el.kind,
                        ElementKind::Instance { symbol_id, .. }
                            if is_pure_vector_mask_symbol(symbol_id, symbols, visiting)
                    )
                })
        });
    visiting.remove(sym_id);
    ok
}

fn normalize_xfl_masks(project: &mut Project, report: &mut ImportReport) {
    let sym_map: HashMap<&str, &Symbol> =
        project.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut invalid_masks_per_symbol: Vec<HashSet<String>> =
        Vec::with_capacity(project.symbols.len());
    for sym in &project.symbols {
        let mut bad = HashSet::new();
        for layer in &sym.timeline.layers {
            if layer.properties.role != LayerRole::Mask {
                continue;
            }
            let valid = layer.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                matches!(
                    el.kind,
                    ElementKind::Rectangle { .. }
                        | ElementKind::Ellipse { .. }
                        | ElementKind::Path { .. }
                        | ElementKind::PaintedPath { .. }
                ) || matches!(
                    &el.kind,
                    ElementKind::Instance { symbol_id, .. }
                        if is_pure_vector_mask_symbol(symbol_id, &sym_map, &mut HashSet::new())
                )
            });
            if !valid {
                bad.insert(layer.id.clone());
            }
        }
        invalid_masks_per_symbol.push(bad);
    }
    for (sym, bad_ids) in project.symbols.iter_mut().zip(invalid_masks_per_symbol) {
        if bad_ids.is_empty() {
            continue;
        }
        for layer in &mut sym.timeline.layers {
            if bad_ids.contains(&layer.id) {
                layer.properties.role = LayerRole::Normal;
                report.warning(
                    "xfl_mask_approximated",
                    format!("{}/{}", sym.name, layer.name),
                    "XFL mask layer contains non-vector or nested-mask elements and was converted to a normal layer.",
                );
            }
            if layer
                .properties
                .mask
                .as_ref()
                .is_some_and(|m| bad_ids.contains(m))
            {
                layer.properties.mask = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_xfl_strength_is_direct_and_unmarked_neoflash_export_stays_compatible() {
        let legacy = Document::parse(
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2"/>"#,
        )
        .unwrap();
        let adobe = Document::parse(
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2" creatorInfo="Adobe Flash Professional CS5"/>"#,
        )
        .unwrap();
        let neoflash = Document::parse(
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2" creatorInfo="NeoFlash" versionInfo="NeoFlash XFL export 1"/>"#,
        )
        .unwrap();

        assert!(!uses_direct_filter_strength(legacy.root_element()));
        assert!(uses_direct_filter_strength(adobe.root_element()));
        assert!(uses_direct_filter_strength(neoflash.root_element()));
        assert_eq!(normalize_filter_strength(150.0, false), 1.5);
        assert_eq!(normalize_filter_strength(1.2, true), 1.2);
        assert_eq!(normalize_filter_strength(30_000.0, false), 255.0);
    }
}
