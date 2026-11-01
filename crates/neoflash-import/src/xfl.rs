use crate::{ImportReport, Statistics, source};
use neoflash_core::*;
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod publish;
mod shape;

pub(crate) fn convert(
    files: &BTreeMap<String, String>,
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
    let publish = files
        .get("PublishSettings.xml")
        .and_then(|text| parse(text, "PublishSettings.xml", report))
        .map(|doc| publish::read(&doc, report))
        .unwrap_or_default();

    let mut parser = Parser {
        report,
        symbols: HashMap::new(),
        language: publish.language,
        include_hidden_layers: publish.include_hidden_layers,
        text_warning: false,
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
        ],
        &[
            "timelines",
            "symbols",
            "folders",
            "swatchLists",
            "swatches",
            "media",
            "fonts",
            "PrinterSettings",
            "publishHistory",
        ],
    );
    if root.attribute("platform").is_some() {
        parser.report.warning("editor_metadata", "DOMDocument.xml", "Authoring platform metadata is not retained; source artwork and scripts are unaffected.");
    }
    if let Some(printer) = child(root, "PrinterSettings") {
        // Only an empty marker is proven harmless. Actual printer settings remain unsupported.
        parser.audit(printer, "DOMDocument.xml", &[], &[]);
        parser.report.warning(
            "editor_metadata",
            "DOMDocument.xml/PrinterSettings",
            "Empty printer-settings marker is not retained.",
        );
    }
    if let Some(history) = child(root, "publishHistory") {
        parser.audit(history, "DOMDocument.xml", &[], &["PublishItem"]);
        for item in elements(history).filter(|n| n.has_tag_name("PublishItem")) {
            parser.audit(
                item,
                "DOMDocument.xml",
                &["publishSize", "publishTime"],
                &[],
            );
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
    for tag in ["media", "fonts"] {
        if let Some(assets) = child(root, tag) {
            parser.audit(assets, "DOMDocument.xml", &[], &[]);
            if elements(assets).next().is_some() {
                parser.report.error(
                    "unsupported_assets",
                    format!("DOMDocument.xml/{tag}"),
                    "Media/font library assets are not imported yet.",
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
                let folder_name = item.attribute("name").unwrap_or("");
                if folder_name.is_empty()
                    || !valid_library_folder(folder_name)
                    || !seen_folders.insert(folder_name.to_owned())
                {
                    parser.report.error(
                        "invalid_library_folder",
                        parser.location(item, "DOMDocument.xml"),
                        "Library folder paths must be unique, nonempty, normalized NeoFlash folder paths.",
                    );
                } else {
                    declared_folders.push(folder_name.to_owned());
                }
            }
        }
    }
    for tag in ["swatchLists", "swatches"] {
        if let Some(metadata) = child(root, tag) {
            // Empty authoring markers are harmless. Do not treat an entire
            // unaudited subtree as metadata: it may contain unsupported content.
            let first_diagnostic = parser.report.diagnostics.len();
            parser.audit(metadata, "DOMDocument.xml", &[], &[]);
            if !parser.report.diagnostics[first_diagnostic..]
                .iter()
                .any(|d| d.severity == crate::Severity::Error)
            {
                parser.report.warning(
                    "editor_metadata",
                    format!("DOMDocument.xml/{tag}"),
                    "Empty library folder/swatch marker is not retained; nonempty organization requires a dedicated importer.",
                );
            }
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
            let Some(xml) = files.get(&path) else {
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
            if !symbol.has_tag_name("DOMSymbolItem") {
                parser
                    .report
                    .error("invalid_symbol", &path, "Expected DOMSymbolItem.");
                continue;
            }
            let name = symbol.attribute("name").unwrap_or("");
            if name.is_empty()
                || parser
                    .symbols
                    .insert(name.into(), new_id("symbol"))
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
    p.width = parser.integer(root, "width", 550, "DOMDocument.xml");
    p.height = parser.integer(root, "height", 400, "DOMDocument.xml");
    p.fps = parser.integer(root, "frameRate", 24, "DOMDocument.xml");
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
    for (path, doc) in &libraries {
        let node = doc.root_element();
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
                "sourceAutoUpdate",
            ],
            &["timeline"],
        );
        if node.attribute("sourceAutoUpdate") == Some("true") {
            parser.report.error(
                "external_symbol",
                path,
                "External auto-updating symbols are not supported.",
            );
        }
        let name = node.attribute("name").unwrap_or("");
        let (symbol_folder, display_name) = if let Some(explicit) = node.attribute("folder") {
            if !valid_library_folder(explicit) {
                parser.report.error(
                    "invalid_library_folder",
                    path,
                    "Symbol folder attribute must be a valid normalized NeoFlash library folder path.",
                );
                (String::new(), name.to_owned())
            } else {
                let base = name.rsplit_once('/').map_or(name, |(_, b)| b);
                (explicit.to_owned(), base.to_owned())
            }
        } else if let Some((prefix, base)) = name.rsplit_once('/') {
            if prefix.is_empty() || base.is_empty() || !valid_library_folder(prefix) {
                parser.report.error(
                    "invalid_library_folder",
                    path,
                    "Slash-separated symbol name does not form a valid normalized library folder path.",
                );
                (String::new(), name.to_owned())
            } else {
                (prefix.to_owned(), base.to_owned())
            }
        } else {
            (String::new(), name.to_owned())
        };
        if !symbol_folder.is_empty() && !p.library_folders.contains(&symbol_folder) {
            p.library_folders.push(symbol_folder.clone());
        }
        let kind = match node.attribute("symbolType").unwrap_or("movie clip") {
            "graphic" => SymbolKind::Graphic,
            "movie clip" => SymbolKind::MovieClip,
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
        let id = parser
            .symbols
            .get(name)
            .cloned()
            .unwrap_or_else(|| new_id("symbol"));
        let mut symbol = parser.symbol(timeline, id, kind, path);
        symbol.folder = symbol_folder;
        symbol.name = display_name;
        p.symbols.push(symbol);
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
        scripts: p.symbols.iter().map(|s| s.timeline.scripts.len()).sum(),
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

fn publish_language(properties: Node<'_, '_>, report: &mut ImportReport) -> Option<ScriptLanguage> {
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
            "Publish settings must identify one unambiguous ActionScriptVersion, 2 or 3.",
        );
    }
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
        if language == Some(ScriptLanguage::ActionScript2) && name.starts_with("AS3") {
            report.warning("inactive_script_configuration", "PublishSettings.xml", format!("{name} is inactive because ActionScriptVersion is 2; the AS2 frame source is retained and the original publish settings are unchanged."));
        } else {
            report.error(
                "external_script_configuration",
                "PublishSettings.xml",
                format!("Nonempty {name} cannot be preserved by this importer yet."),
            );
        }
    }
    language
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
                    path == "PublishSettings.xml" || namespace != "http://ns.adobe.com/xfl/2008/"
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

struct Parser<'a> {
    report: &'a mut ImportReport,
    symbols: HashMap<String, Id>,
    language: Option<ScriptLanguage>,
    include_hidden_layers: bool,
    text_warning: bool,
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
                    | "DOMLayer"
                    | "DOMFrame"
                    | "DOMTimeline"
                    | "DOMSymbolInstance"
                    | "DOMStaticText"
                    | "DOMRectangleObject"
                    | "DOMOvalObject"
                    | "DOMShape"
                    | "DOMTextRun"
                    | "Edge"
                    | "FillStyle"
                    | "StrokeStyle"
                    | "PublishItem"
                    | "CustomEase"
                    | "Point"
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

    fn symbol(&mut self, node: Node<'_, '_>, id: Id, kind: SymbolKind, file: &str) -> Symbol {
        self.audit(
            node,
            file,
            &["name", "currentFrame", "guides", "autoNamed"],
            &["layers"],
        );
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
                self.report.error(
                    "motion_object_not_supported",
                    self.location(layer_node, file),
                    format!(
                        "XFL layer animationType={anim:?} is not supported yet; convert to classic/shape tweens or frame keyframes."
                    ),
                );
            }
            let role = match layer_node.attribute("layerType").unwrap_or("normal") {
                "normal" => LayerRole::Normal,
                "guide" => LayerRole::Guide,
                "mask" => LayerRole::Mask,
                "folder" => LayerRole::Folder,
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
                    let has_motion_xml = child(frame_node, "MotionObjectXML").is_some();
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
                            "motionTweenOrientToPath",
                            "motionTweenSnap",
                            "motionTweenRotate",
                            "motionTweenRotateTimes",
                            "motionTweenScale",
                            "motionTweenSync",
                            "shapeTweenBlend",
                            "hasCustomEase",
                            "useSingleEaseCurve",
                        ],
                        if has_motion_xml {
                            &["elements", "Actionscript", "tweens", "MotionObjectXML"]
                        } else {
                            &["elements", "Actionscript", "tweens"]
                        },
                    );
                    if has_motion_xml {
                        self.report.error(
                            "motion_object_not_supported",
                            self.location(frame_node, file),
                            "XFL MotionObjectXML property-track tweens are not imported yet; convert to classic/shape tweens or frame keyframes.",
                        );
                    }
                    let frame_loc = self.location(frame_node, file);
                    let (tween, tween_kind, orient_to_path) =
                        self.frame_tween(frame_node, has_motion_xml, file);
                    if frame_node
                        .attribute("labelType")
                        .is_some_and(|t| t != "name")
                    {
                        self.report.error(
                            "frame_label_type",
                            &frame_loc,
                            "Only ordinary frame labels are supported.",
                        );
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
                            if frame_node
                                .attribute("name")
                                .is_some_and(|name| !name.is_empty())
                            {
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
                    {
                        self.report.error(
                            "folder_layer_elements",
                            &frame_loc,
                            "Folder layers cannot own labels or tweens.",
                        );
                    }
                    let mut key = Keyframe {
                        frame,
                        elements: vec![],
                        tween,
                        tween_kind,
                        orient_to_path,
                        label: frame_node.attribute("name").unwrap_or("").into(),
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
                                "DOMStaticText",
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
                            let mut identity = format!("xfl-{id}-{li}-{ei}");
                            // A later authored instance span never inherits an earlier
                            // XML slot's state unless explicitly connected by a classic tween.
                            if element_node.has_tag_name("DOMSymbolInstance") && frame != 0 {
                                identity.push_str(&format!("-placement-{frame}"));
                            }
                            if let Some(element) = self.element(element_node, identity, file) {
                                elem_locs
                                    .push((element.id.clone(), self.location(element_node, file)));
                                key.elements.push(element);
                            }
                        }
                    }
                    if let Some(actions) = child(frame_node, "Actionscript") {
                        self.audit(actions, file, &[], &["script"]);
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
                                if let Some(language) = self.language {
                                    timeline.scripts.push(FrameScript {
                                        frame,
                                        script: Script { language, source },
                                    });
                                    self.report.warning("script_preserved",self.location(script,file),"ActionScript source retained unchanged. Execution compatibility is checked separately by the compiler/exporter.");
                                } else {
                                    self.report.error("script_language_unknown",self.location(script,file),"Cannot determine AS2/AS3. Supply PublishSettings.xml with an unambiguous ActionScriptVersion.");
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
                            self.report.error(
                                "invalid_shape_tween",
                                prev_loc,
                                "Shape tweens require vector shape elements on both the start and end keyframes.",
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
                        self.report.error("ambiguous_instance_continuity", location, "An adjacent key ends and places the same library symbol. XML order, name and referenceID do not establish whether the Flash instance continues, is reordered or is replaced. Use a held placement span or an explicit blank-frame gap; this importer does not guess instance lifetimes.");
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

    fn element(&mut self, node: Node<'_, '_>, id: Id, file: &str) -> Option<Element> {
        let tag = node.tag_name().name();
        if tag == "DOMShape" {
            return self.shape(node, id, file);
        }
        let location = self.location(node, file);
        let mut transform = self.transform(node, file);
        let name = node.attribute("name").unwrap_or(tag).into();
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
                        "selected",
                        "referenceID",
                        "centerPoint3DX",
                        "centerPoint3DY",
                    ],
                    &["matrix", "transformationPoint", "color"],
                );
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                    self.report.warning("transform_pivot", &location, "Placement matrix retained; Flash's 2D authoring centerPoint3DX/Y pivot is not retained. Actual 3D transforms remain unsupported.");
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let Some(symbol_id) = self.symbols.get(target).cloned() else {
                    self.report.error(
                        "missing_symbol",
                        &location,
                        format!("Unknown symbol {target}"),
                    );
                    return None;
                };
                let first = self.integer(node, "firstFrame", 0, file);
                let playback = match node.attribute("symbolType").unwrap_or("movie clip") {
                    "movie clip" => {
                        if first != 0 {
                            self.report.error(
                                "clip_start_frame",
                                &location,
                                "MovieClip start-frame override is unsupported.",
                            );
                        }
                        InstancePlayback::Independent
                    }
                    "graphic" => match node.attribute("loop").unwrap_or("loop") {
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
                    },
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
            "DOMStaticText" => {
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
                        "isSelectable",
                        "fontRenderingMode",
                        "antiAliasSharpness",
                        "antiAliasThickness",
                        "referenceID",
                    ],
                    &["matrix", "transformationPoint", "textRuns"],
                );
                let Some(runs) = child(node, "textRuns") else {
                    self.report.error(
                        "missing_text_runs",
                        &location,
                        "Static text has no textRuns.",
                    );
                    return None;
                };
                self.audit(runs, file, &[], &["DOMTextRun"]);
                let runs: Vec<_> = elements(runs).collect();
                if runs.len() != 1 {
                    self.report.error(
                        "rich_text",
                        &location,
                        "Only a single plain static text run is supported.",
                    );
                    return None;
                }
                self.audit(runs[0], file, &[], &["characters", "textAttrs"]);
                let Some(characters) = child(runs[0], "characters") else {
                    self.report.error(
                        "missing_text",
                        &location,
                        "Text run has no characters node.",
                    );
                    return None;
                };
                self.audit(characters, file, &[], &[]);
                let text = immediate_text(characters);
                if text.contains(['\n', '\r']) {
                    self.report.error(
                        "multiline_text",
                        &location,
                        "Multiline text layout is not imported yet.",
                    );
                }
                let Some(attrs_wrapper) = child(runs[0], "textAttrs") else {
                    self.report.error(
                        "missing_text_style",
                        &location,
                        "Text run has no textAttrs.",
                    );
                    return None;
                };
                self.audit(attrs_wrapper, file, &[], &["DOMTextAttrs"]);
                let Some(attrs) = child(attrs_wrapper, "DOMTextAttrs") else {
                    self.report
                        .error("missing_text_style", &location, "No DOMTextAttrs.");
                    return None;
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
                        "leftMargin",
                        "rightMargin",
                        "indent",
                    ],
                    &[],
                );
                for field in ["letterSpacing", "leftMargin", "rightMargin", "indent"] {
                    if self.number(attrs, field, 0., file) != 0. {
                        self.report.error(
                            "text_layout",
                            &location,
                            format!("Nonzero {field} is unsupported."),
                        );
                    }
                }
                if attrs.attribute("alignment").is_some_and(|a| a != "left") {
                    self.report.error(
                        "text_alignment",
                        &location,
                        "Only left-aligned text is supported.",
                    );
                }
                let left = self.number(node, "left", 0., file);
                let top = self.number(node, "top", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * left + matrix[2] * top;
                transform.y += matrix[1] * left + matrix[3] * top;
                let alpha = self.number(attrs, "alpha", 1., file);
                let color = self.color(
                    attrs.attribute("fillColor").unwrap_or("#000000"),
                    alpha,
                    &location,
                );
                if !self.text_warning {
                    self.report.warning("text_metrics",&location,"Font names and text are retained; line metrics/antialiasing may differ from Flash. Verify imported typography.");
                    self.text_warning = true;
                }
                ElementKind::Text {
                    text,
                    font_family: attrs.attribute("face").unwrap_or("Arial").into(),
                    font_size: self.number(attrs, "size", 12., file),
                    color,
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
                        "referenceID",
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
                    &["matrix", "transformationPoint", "fill", "stroke"],
                );
                for field in [
                    "topLeftRadius",
                    "topRightRadius",
                    "bottomLeftRadius",
                    "bottomRightRadius",
                    "startAngle",
                    "endAngle",
                    "innerRadius",
                ] {
                    if self.number(node, field, 0., file) != 0. {
                        self.report.error(
                            "primitive_geometry",
                            &location,
                            format!("Nonzero {field} is unsupported."),
                        );
                    }
                }
                if child(node, "stroke").is_some() {
                    self.report.error(
                        "primitive_stroke",
                        &location,
                        "Imported primitive strokes are not supported yet.",
                    );
                }
                let fill = if let Some(fill) = child(node, "fill") {
                    self.audit(fill, file, &[], &["SolidColor"]);
                    if let Some(solid) = child(fill, "SolidColor") {
                        self.audit(solid, file, &["color", "alpha"], &[]);
                        let alpha = self.number(solid, "alpha", 1., file);
                        self.color(
                            solid.attribute("color").unwrap_or("#000000"),
                            alpha,
                            &location,
                        )
                    } else {
                        [0, 0, 0, 0]
                    }
                } else {
                    [0, 0, 0, 0]
                };
                let x = self.number(node, "x", 0., file);
                let y = self.number(node, "y", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * x + matrix[2] * y;
                transform.y += matrix[1] * x + matrix[3] * y;
                let width = self.number(node, "objectWidth", 0., file);
                let height = self.number(node, "objectHeight", 0., file);
                if tag == "DOMRectangleObject" {
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.,
                    }
                } else {
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.,
                    }
                }
            }
            _ => return None,
        };
        Some(Element {
            id,
            accessibility: None,
            name,
            transform,
            kind,
            scripts: vec![],
        })
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
        if let Some(tweens) = child(frame_node, "tweens") {
            self.audit(tweens, file, &[], &["CustomEase"]);
            let eases: Vec<_> = elements(tweens)
                .filter(|n| n.has_tag_name("CustomEase"))
                .collect();
            if has_custom_ease || !eases.is_empty() {
                if !single_ease || eases.len() != 1 {
                    self.report.error(
                        "custom_ease_not_supported",
                        self.location(tweens, file),
                        "Only a single global CustomEase target=\"all\" cubic Bézier curve is supported.",
                    );
                } else {
                    let ease_node = eases[0];
                    self.audit(ease_node, file, &["target"], &["Point"]);
                    if ease_node.attribute("target") != Some("all") {
                        self.report.error(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Per-property XFL CustomEase targets are not supported; use target=\"all\".",
                        );
                    }
                    let pts: Vec<_> = elements(ease_node)
                        .filter(|n| n.has_tag_name("Point"))
                        .map(|pt| {
                            self.audit(pt, file, &["x", "y"], &[]);
                            [
                                self.number(pt, "x", 0., file),
                                self.number(pt, "y", 0., file),
                            ]
                        })
                        .collect();
                    if pts.len() != 4 || pts[0] != [0., 0.] || pts[3] != [1., 1.] {
                        self.report.error(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Multi-segment XFL CustomEase curves are not supported yet.",
                        );
                    } else if !(0.0..=1.0).contains(&pts[1][0])
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
                    self.report.error(
                        "motion_object_not_supported",
                        &loc,
                        "XFL motion-object property-track tweens are not imported yet; convert to classic/shape tweens or frame keyframes.",
                    );
                }
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
                self.audit(color, file, &["alphaMultiplier"], &[]);
                t.alpha = self.number(color, "alphaMultiplier", 1., file);
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
}

fn classic_tween_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
            | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. })
            | (ElementKind::Path { .. }, ElementKind::Path { .. })
            | (ElementKind::Text { .. }, ElementKind::Text { .. }) => true,
            _ => false,
        }
}
