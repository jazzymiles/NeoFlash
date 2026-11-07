//! Numbered image sequence detection and frame/keyframe/symbol import workflows (`T-115`).
//!
//! Detects numeric filename patterns (such as `walk_0001.png`, `walk_0002.png`, `frame-1.webp`),
//! reports sequence gaps or unreadable frames as location-aware [`crate::Diagnostic`] entries in
//! [`crate::ImportReport`], and populates a [`neoflash_core::Project`] with bitmap resources and
//! keyframes on a single layer, separate layers, or inside a reusable Graphic symbol.

use crate::{ImportError, ImportFormat, ImportReport, Statistics};
use image::GenericImageView;
use neoflash_core::{
    Element, ElementKind, InstancePlayback, Keyframe, Layer, LayerProperties, Project,
    ResourceUnit, Symbol, SymbolKind, Timeline, Transform, new_id,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

/// How imported sequence frames are arranged in the destination [`Project`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSequencePlacement {
    /// Place each sequence image into its own sequential keyframe on a single timeline layer.
    #[default]
    KeyframesOnSingleLayer,
    /// Place each sequence image onto its own separate timeline layer at frame 0.
    SeparateLayers,
    /// Create a `SymbolKind::Graphic` symbol containing the frame-by-frame sequence and place a
    /// synchronized `InstancePlayback::Loop` instance on the target timeline.
    ConvertToGraphicSymbol,
}

/// Options controlling numbered image sequence import (`T-115`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageSequenceImportOptions {
    pub placement: ImageSequencePlacement,
    /// Number of timeline frames each image spans (`>= 1`, default `1`).
    pub frame_step: u32,
    /// Emit warnings for missing frame numbers in the detected numeric range.
    pub detect_gaps: bool,
    /// When `true` and a gap is detected (e.g. `0001` -> `0003`), hold the previous frame across
    /// the missing frame numbers so timing stays locked to the sequence numbering.
    pub fill_gaps_with_holds: bool,
    /// Center each imported bitmap on the project stage.
    pub center_on_stage: bool,
    /// Enable bilinear smoothing on imported bitmap elements.
    pub smoothing: bool,
    /// Optional explicit name for the created layer or Graphic symbol.
    pub layer_name: Option<String>,
}

impl Default for ImageSequenceImportOptions {
    fn default() -> Self {
        Self {
            placement: ImageSequencePlacement::KeyframesOnSingleLayer,
            frame_step: 1,
            detect_gaps: true,
            fill_gaps_with_holds: true,
            center_on_stage: true,
            smoothing: true,
            layer_name: None,
        }
    }
}

/// One parsed numbered filename inside an image sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberedSequenceItem {
    pub original_name: String,
    pub prefix: String,
    pub number: u32,
    pub padding: usize,
    pub extension: String,
    pub input_index: usize,
}

/// Result of inspecting a list of filenames or paths for a numbered image sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberedSequenceDetection {
    pub prefix: String,
    pub extension: String,
    pub padding: usize,
    pub start_number: u32,
    pub end_number: u32,
    pub items: Vec<NumberedSequenceItem>,
    pub missing_numbers: Vec<u32>,
    pub unmatched_files: Vec<String>,
}

fn is_supported_image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
    )
}

/// Parse a filename into `(prefix, number, digit_padding, extension)` if it ends with a numeric
/// index before a supported image extension (e.g. `walk_0001.png` -> `("walk_", 1, 4, "png")`).
pub fn parse_numbered_image_filename(name: &str, input_index: usize) -> Option<NumberedSequenceItem> {
    let trimmed = name.trim();
    let file_name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let (stem, ext) = file_name.rsplit_once('.')?;
    if stem.is_empty() || !is_supported_image_ext(ext) {
        return None;
    }
    let digit_byte_count = stem
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digit_byte_count == 0 || digit_byte_count > 9 {
        return None;
    }
    let split_at = stem.len() - digit_byte_count;
    let prefix = &stem[..split_at];
    let digits = &stem[split_at..];
    let number: u32 = digits.parse().ok()?;
    Some(NumberedSequenceItem {
        original_name: trimmed.to_owned(),
        prefix: prefix.to_owned(),
        number,
        padding: digit_byte_count,
        extension: ext.to_ascii_lowercase(),
        input_index,
    })
}

/// Detect a numbered image sequence from a slice of filenames or relative paths.
///
/// Groups matching files by `(prefix, extension)`, selects the dominant sequence group, sorts
/// frames by numeric index, and computes any missing numbers in `start_number..=end_number`.
pub fn detect_numbered_image_sequence<S: AsRef<str>>(
    filenames: &[S],
) -> Option<NumberedSequenceDetection> {
    if filenames.is_empty() {
        return None;
    }
    let mut groups: BTreeMap<(String, String), Vec<NumberedSequenceItem>> = BTreeMap::new();
    let mut unmatched = Vec::new();

    for (idx, raw) in filenames.iter().enumerate() {
        let name = raw.as_ref();
        if let Some(item) = parse_numbered_image_filename(name, idx) {
            groups
                .entry((item.prefix.clone(), item.extension.clone()))
                .or_default()
                .push(item);
        } else {
            unmatched.push(name.to_owned());
        }
    }

    let ((prefix, extension), mut best_items) = groups
        .into_iter()
        .max_by_key(|((p, e), items)| (items.len(), p.len(), e.len()))?;

    // Collect any items from other non-dominant groups as unmatched so nothing is silently dropped.
    let chosen_indices: std::collections::HashSet<usize> =
        best_items.iter().map(|i| i.input_index).collect();
    for (idx, raw) in filenames.iter().enumerate() {
        if !chosen_indices.contains(&idx) && !unmatched.iter().any(|u| u == raw.as_ref()) {
            unmatched.push(raw.as_ref().to_owned());
        }
    }

    best_items.sort_by_key(|item| (item.number, item.input_index));
    best_items.dedup_by_key(|item| item.number);

    let start_number = best_items.first()?.number;
    let end_number = best_items.last()?.number;
    let padding = best_items.first().map_or(1, |i| i.padding);

    let present: std::collections::HashSet<u32> = best_items.iter().map(|i| i.number).collect();
    let mut missing_numbers = Vec::new();
    if end_number >= start_number && (end_number - start_number) <= 10_000 {
        for num in start_number..=end_number {
            if !present.contains(&num) {
                missing_numbers.push(num);
            }
        }
    }

    Some(NumberedSequenceDetection {
        prefix,
        extension,
        padding,
        start_number,
        end_number,
        items: best_items,
        missing_numbers,
        unmatched_files: unmatched,
    })
}

/// Detect a numbered image sequence from filesystem paths.
pub fn detect_numbered_image_paths<P: AsRef<Path>>(
    paths: &[P],
) -> Option<NumberedSequenceDetection> {
    let names: Vec<String> = paths
        .iter()
        .map(|p| p.as_ref().display().to_string())
        .collect();
    detect_numbered_image_sequence(&names)
}

fn decode_or_normalize_image_bytes(bytes: &[u8]) -> Result<(Vec<u8>, String, u32, u32), String> {
    if bytes.is_empty() {
        return Err("image file is empty".into());
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("cannot decode image: {e}"))?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return Err("image has zero width or height".into());
    }
    // Keep PNG or JPEG as-is when valid; normalize WebP/BMP/GIF to PNG so ResourceUnit validation succeeds.
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok((bytes.to_vec(), "image/png".into(), w, h));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok((bytes.to_vec(), "image/jpeg".into(), w, h));
    }
    let mut png_bytes = Vec::new();
    img.to_rgba8()
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| format!("failed to encode normalized PNG: {e}"))?;
    Ok((png_bytes, "image/png".into(), w, h))
}

fn unique_resource_path(project: &Project, desired: &str) -> String {
    let clean = desired
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/");
    let base = if clean.is_empty() {
        "sequence/frame.png".to_owned()
    } else if !clean.contains('/') {
        format!("sequence/{clean}")
    } else {
        clean
    };
    if !project
        .resources
        .iter()
        .any(|r| r.path.eq_ignore_ascii_case(&base))
    {
        return base;
    }
    let (stem, ext) = base.rsplit_once('.').unwrap_or((&base, "png"));
    for idx in 2..10_000 {
        let candidate = format!("{stem}_{idx}.{ext}");
        if !project
            .resources
            .iter()
            .any(|r| r.path.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    format!("{}_{}.png", stem, new_id("seq"))
}

/// Import a numbered image sequence from `(filename, bytes)` entries into an existing [`Project`].
///
/// Emits location-aware [`crate::Diagnostic`] warnings in the returned [`ImportReport`] for:
/// - `sequence-missing-frame`: missing numbers in the sequence range when `options.detect_gaps` is enabled
/// - `sequence-unmatched-file`: files that did not match the numbered sequence pattern
/// - `sequence-unreadable-frame`: corrupted or undecodable frame images (represented by an explicit
///   placeholder rectangle so frame timing and content are never silently dropped)
pub fn import_image_sequence_bytes(
    project: &mut Project,
    entries: &[(String, Vec<u8>)],
    options: &ImageSequenceImportOptions,
) -> Result<ImportReport, ImportError> {
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    let mut report = ImportReport {
        source: names.first().copied().unwrap_or("sequence").to_owned(),
        format: ImportFormat::Unknown,
        version: Some("ImageSequence".to_owned()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    let Some(detection) = detect_numbered_image_sequence(&names) else {
        report.error(
            "sequence-no-numbered-images",
            report.source.clone(),
            "No numbered image files (e.g. name_0001.png) were found in the selection.",
        );
        return Err(ImportError::Blocked(
            "No numbered image sequence found".into(),
        ));
    };

    let step = options.frame_step.max(1);
    let seq_base_name = if detection.prefix.trim_end_matches(['_', '-', '.', ' ']).is_empty() {
        "Image Sequence".to_owned()
    } else {
        detection
            .prefix
            .trim_end_matches(['_', '-', '.', ' '])
            .to_owned()
    };
    let target_name = options
        .layer_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&seq_base_name)
        .to_owned();

    if options.detect_gaps {
        for missing in &detection.missing_numbers {
            let missing_file = format!(
                "{}{:0width$}.{}",
                detection.prefix,
                missing,
                detection.extension,
                width = detection.padding
            );
            report.warning(
                "sequence-missing-frame",
                missing_file,
                format!(
                    "Sequence number {} is missing between {} and {}; {}",
                    missing,
                    detection.start_number,
                    detection.end_number,
                    if options.fill_gaps_with_holds {
                        "holding previous frame exposure."
                    } else {
                        "placing subsequent frames consecutively."
                    }
                ),
            );
        }
    }

    for unmatched in &detection.unmatched_files {
        report.warning(
            "sequence-unmatched-file",
            unmatched.clone(),
            "File does not match the detected numbered sequence pattern and was skipped.",
        );
    }

    let mut keyframes = Vec::with_capacity(detection.items.len());
    let mut separate_layers = Vec::new();
    let mut max_frame_end = 1u32;

    for (seq_idx, item) in detection.items.iter().enumerate() {
        let (_, bytes) = &entries[item.input_index];
        let frame_number = if options.detect_gaps && options.fill_gaps_with_holds {
            (item.number - detection.start_number).saturating_mul(step)
        } else {
            (seq_idx as u32).saturating_mul(step)
        };
        max_frame_end = max_frame_end.max(frame_number.saturating_add(step));

        let element = match decode_or_normalize_image_bytes(bytes) {
            Ok((norm_bytes, media_type, width, height)) => {
                let desired_ext = if media_type == "image/jpeg" {
                    "jpg"
                } else {
                    "png"
                };
                let stem = item
                    .original_name
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&item.original_name)
                    .rsplit_once('.')
                    .map_or(item.original_name.as_str(), |(s, _)| s);
                let rel_path = unique_resource_path(project, &format!("sequence/{stem}.{desired_ext}"));
                let res_id = new_id("resource");
                let unit = ResourceUnit::from_bytes(res_id.clone(), rel_path, media_type, &norm_bytes)
                    .map_err(|e| ImportError::Container(e.to_string()))?;
                project.resources.push(unit);

                let (tx, ty) = if options.center_on_stage {
                    (
                        (project.width as f64 - width as f64) * 0.5,
                        (project.height as f64 - height as f64) * 0.5,
                    )
                } else {
                    (0.0, 0.0)
                };

                Element {
                    id: new_id("element"),
                    name: stem.to_owned(),
                    transform: Transform {
                        x: tx,
                        y: ty,
                        ..Transform::default()
                    },
                    kind: ElementKind::Bitmap {
                        resource_id: res_id,
                        width,
                        height,
                        smoothing: options.smoothing,
                    },
                    ..Element::default()
                }
            }
            Err(reason) => {
                report.warning(
                    "sequence-unreadable-frame",
                    item.original_name.clone(),
                    format!(
                        "Frame image could not be decoded ({reason}); inserted an explicit placeholder shape."
                    ),
                );
                let pw = (project.width as f64 * 0.25).max(32.0);
                let ph = (project.height as f64 * 0.25).max(32.0);
                let (tx, ty) = if options.center_on_stage {
                    ((project.width as f64 - pw) * 0.5, (project.height as f64 - ph) * 0.5)
                } else {
                    (0.0, 0.0)
                };
                Element {
                    id: new_id("element"),
                    name: format!("Placeholder ({})", item.original_name),
                    transform: Transform {
                        x: tx,
                        y: ty,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: pw,
                        height: ph,
                        fill: [220, 68, 68, 180],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 2.0,
                    },
                    ..Element::default()
                }
            }
        };

        report.statistics.elements += 1;

        match options.placement {
            ImageSequencePlacement::KeyframesOnSingleLayer
            | ImageSequencePlacement::ConvertToGraphicSymbol => {
                keyframes.push(Keyframe {
                    frame: frame_number,
                    elements: vec![element],
                    ..Keyframe::default()
                });
                report.statistics.keyframes += 1;
            }
            ImageSequencePlacement::SeparateLayers => {
                let layer_label = item
                    .original_name
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&item.original_name)
                    .to_owned();
                separate_layers.push(Layer {
                    id: new_id("layer"),
                    name: layer_label,
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![element],
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                });
                report.statistics.layers += 1;
                report.statistics.keyframes += 1;
            }
        }
    }

    let root_id = project.root.clone();
    match options.placement {
        ImageSequencePlacement::KeyframesOnSingleLayer => {
            let layer = Layer {
                id: new_id("layer"),
                name: target_name,
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            };
            report.statistics.layers += 1;
            if let Some(root) = project.symbol_mut(&root_id) {
                root.timeline.frame_count = root.timeline.frame_count.max(max_frame_end);
                if root.timeline.layers.len() == 1
                    && root.timeline.layers[0]
                        .keyframes
                        .iter()
                        .all(|k| k.elements.is_empty())
                {
                    root.timeline.layers[0] = layer;
                } else {
                    root.timeline.layers.insert(0, layer);
                }
            }
        }
        ImageSequencePlacement::SeparateLayers => {
            if let Some(root) = project.symbol_mut(&root_id) {
                if root.timeline.layers.len() == 1
                    && root.timeline.layers[0]
                        .keyframes
                        .iter()
                        .all(|k| k.elements.is_empty())
                {
                    root.timeline.layers = separate_layers;
                } else {
                    for (idx, layer) in separate_layers.into_iter().enumerate() {
                        root.timeline.layers.insert(idx, layer);
                    }
                }
            }
        }
        ImageSequencePlacement::ConvertToGraphicSymbol => {
            let sym_id = new_id("symbol");
            let sym = Symbol {
                id: sym_id.clone(),
                name: target_name.clone(),
                folder: String::new(),
                kind: SymbolKind::Graphic,
                timeline: Timeline {
                    frame_count: max_frame_end,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: target_name.clone(),
                        visible: true,
                        locked: false,
                        keyframes,
                        properties: LayerProperties::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            };
            project.symbols.push(sym);
            report.statistics.symbols += 1;
            report.statistics.layers += 1;

            let instance_element = Element {
                id: new_id("element"),
                name: target_name.clone(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: sym_id,
                    playback: InstancePlayback::Loop,
                },
                ..Element::default()
            };
            report.statistics.elements += 1;
            report.statistics.keyframes += 1;
            report.statistics.layers += 1;

            let inst_layer = Layer {
                id: new_id("layer"),
                name: target_name,
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![instance_element],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            };
            if let Some(root) = project.symbol_mut(&root_id) {
                root.timeline.frame_count = root.timeline.frame_count.max(max_frame_end);
                if root.timeline.layers.len() == 1
                    && root.timeline.layers[0]
                        .keyframes
                        .iter()
                        .all(|k| k.elements.is_empty())
                {
                    root.timeline.layers[0] = inst_layer;
                } else {
                    root.timeline.layers.insert(0, inst_layer);
                }
            }
        }
    }

    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    project.upgrade_format_for_features();
    report.can_import = true;
    Ok(report)
}

/// Import a numbered image sequence from filesystem paths into an existing [`Project`].
pub fn import_image_sequence<P: AsRef<Path>>(
    project: &mut Project,
    paths: &[P],
    options: &ImageSequenceImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut entries = Vec::with_capacity(paths.len());
    for p in paths {
        let path = p.as_ref();
        let bytes = fs::read(path)?;
        entries.push((path.display().to_string(), bytes));
    }
    import_image_sequence_bytes(project, &entries, options)
}

/// Create a new [`Project`] populated with a numbered image sequence from `(filename, bytes)` entries.
pub fn import_image_sequence_bytes_as_project(
    name: &str,
    entries: &[(String, Vec<u8>)],
    options: &ImageSequenceImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    if !name.trim().is_empty() {
        project.name = name.trim().to_owned();
    }
    let report = import_image_sequence_bytes(&mut project, entries, options)?;
    Ok((project, report))
}
