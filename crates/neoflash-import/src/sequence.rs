//! Numbered image sequence detection and frame/keyframe/symbol import workflows (`T-115`).
//!
//! Detects numeric filename patterns (such as `walk_0001.png`, `walk_0002.png`, `frame-1.webp`),
//! reports sequence gaps or unreadable frames as location-aware [`crate::Diagnostic`] entries in
//! [`crate::ImportReport`], and populates a [`neoflash_core::Project`] with bitmap resources and
//! keyframes on a single layer, separate layers, or inside a reusable Graphic symbol.

use crate::{ImportError, ImportFormat, ImportReport, Statistics};
use image::{GenericImageView, ImageReader, Limits};
use neoflash_core::{
    Element, ElementKind, InstancePlayback, Keyframe, Layer, LayerProperties, MAX_RESOURCE_BYTES,
    MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, Project, ResourceUnit, Symbol, SymbolKind, Timeline,
    Transform, new_id,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

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
pub fn parse_numbered_image_filename(
    name: &str,
    input_index: usize,
) -> Option<NumberedSequenceItem> {
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

/// One fully bounded numbered-sequence import prepared from filesystem paths.
///
/// `paths` is the resolved, stable frame order. A single numbered seed may expand to matching
/// siblings when `expand_single_seed` is requested; a non-numbered single image is deliberately
/// left to the ordinary image-import path.
#[derive(Debug)]
pub struct ImportedNumberedImageSequence {
    pub project: Project,
    pub report: ImportReport,
    pub detection: NumberedSequenceDetection,
    pub paths: Vec<PathBuf>,
}

fn is_sequence_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
            )
        })
}

fn push_sequence_candidate(
    candidates: &mut Vec<(u32, PathBuf)>,
    number: u32,
    path: PathBuf,
) -> Result<(), ImportError> {
    if candidates.len() >= MAX_RESOURCES {
        return Err(ImportError::Container(format!(
            "Image sequence exceeds the {MAX_RESOURCES}-resource limit"
        )));
    }
    candidates.push((number, path));
    Ok(())
}

/// Resolve a numbered image sequence without reading or decoding any frame bytes.
///
/// Directories and a numbered single seed are explicit sequence requests and therefore return an
/// actionable error when their candidate set is malformed. Multiple ordinary images return
/// `Ok(None)` so editor callers can continue importing them as independent graphics.
pub fn resolve_numbered_image_sequence_paths<P: AsRef<Path>>(
    inputs: &[P],
    expand_single_seed: bool,
) -> Result<Option<(Vec<PathBuf>, NumberedSequenceDetection)>, ImportError> {
    if inputs.is_empty() {
        return Ok(None);
    }
    if inputs.len() > MAX_RESOURCES {
        return Err(ImportError::Container(format!(
            "Image sequence contains {} files, exceeding the {MAX_RESOURCES}-resource limit",
            inputs.len()
        )));
    }

    let first = inputs[0].as_ref();
    let explicit_directory = inputs.len() == 1 && first.is_dir();
    let mut explicit_seed = false;
    let resolved = if explicit_directory {
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(first)? {
            let path = entry?.path();
            if !path.is_file() || !is_sequence_image_path(&path) {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some(item) = parse_numbered_image_filename(file_name, 0) {
                push_sequence_candidate(&mut candidates, item.number, path)?;
            }
        }
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        if candidates.is_empty() {
            return Err(ImportError::Container(format!(
                "{}: directory contains no numbered image sequence frames",
                first.display()
            )));
        }
        candidates.into_iter().map(|(_, path)| path).collect()
    } else if inputs.len() == 1 && expand_single_seed && is_sequence_image_path(first) {
        let Some(file_name) = first.file_name().and_then(|name| name.to_str()) else {
            return Err(ImportError::Container("Invalid image filename".into()));
        };
        let Some(seed) = parse_numbered_image_filename(file_name, 0) else {
            return Ok(None);
        };
        explicit_seed = true;
        let parent = first
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(parent)? {
            let path = entry?.path();
            if !path.is_file() || !is_sequence_image_path(&path) {
                continue;
            }
            let Some(candidate_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some(item) = parse_numbered_image_filename(candidate_name, 0)
                && item.prefix.eq_ignore_ascii_case(&seed.prefix)
                && item.extension.eq_ignore_ascii_case(&seed.extension)
            {
                push_sequence_candidate(&mut candidates, item.number, path)?;
            }
        }
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        if candidates.is_empty() {
            vec![first.to_path_buf()]
        } else {
            candidates.into_iter().map(|(_, path)| path).collect()
        }
    } else {
        if inputs.len() == 1
            || inputs
                .iter()
                .any(|path| !is_sequence_image_path(path.as_ref()))
        {
            return Ok(None);
        }
        inputs
            .iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect()
    };

    let Some(detection) = detect_numbered_image_paths(&resolved) else {
        if explicit_directory || explicit_seed {
            return Err(ImportError::Container(
                "No numbered image sequence filenames detected".into(),
            ));
        }
        return Ok(None);
    };
    if !detection.unmatched_files.is_empty() {
        if explicit_directory || explicit_seed {
            return Err(ImportError::Container(format!(
                "Unmatched file(s) in image sequence: {}",
                detection.unmatched_files.join(", ")
            )));
        }
        return Ok(None);
    }
    if inputs.len() > 1 && detection.items.len() < 2 {
        return Ok(None);
    }
    Ok(Some((resolved, detection)))
}

fn imported_sequence_dimensions(project: &Project) -> Option<(u32, u32)> {
    project
        .symbol(&project.root)?
        .timeline
        .layers
        .iter()
        .flat_map(|layer| &layer.keyframes)
        .flat_map(|keyframe| &keyframe.elements)
        .find_map(|element| match &element.kind {
            ElementKind::Bitmap { width, height, .. } if *width > 0 && *height > 0 => {
                Some((*width, *height))
            }
            _ => None,
        })
}

/// Resolve and import a numbered image sequence using the same bounded reader and decoder as the
/// core sequence workflow. No caller needs to pre-read or pre-decode frames to obtain dimensions.
pub fn import_numbered_image_sequence_paths<P: AsRef<Path>>(
    inputs: &[P],
    expand_single_seed: bool,
) -> Result<Option<ImportedNumberedImageSequence>, ImportError> {
    let Some((paths, detection)) =
        resolve_numbered_image_sequence_paths(inputs, expand_single_seed)?
    else {
        return Ok(None);
    };
    let clean_prefix = detection
        .prefix
        .trim_end_matches(['_', '-', '.', ' '])
        .trim();
    let name = if clean_prefix.is_empty() {
        paths
            .first()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or("Sequence")
            .to_owned()
    } else {
        clean_prefix.to_owned()
    };
    let mut project = Project::blank();
    project.name = name.clone();
    if let Some(root) = project.symbols.first_mut() {
        root.timeline.frame_count = 1;
    }
    let options = ImageSequenceImportOptions {
        layer_name: Some(name),
        center_on_stage: false,
        ..ImageSequenceImportOptions::default()
    };
    let mut report = import_image_sequence(&mut project, &paths, &options)?;
    let unreadable_frames: Vec<String> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "sequence-unreadable-frame")
        .map(|diagnostic| {
            format!(
                "{} [{}]: {}",
                diagnostic.location, diagnostic.code, diagnostic.message
            )
        })
        .collect();
    if !unreadable_frames.is_empty() {
        return Err(ImportError::Container(unreadable_frames.join("; ")));
    }
    if let Some((width, height)) = imported_sequence_dimensions(&project) {
        project.width = width.max(1);
        project.height = height.max(1);
    }
    project.validate().map_err(|error| {
        ImportError::Container(format!(
            "Image sequence import produced an invalid project: {error}"
        ))
    })?;
    report.statistics.symbols = project.symbols.len();
    Ok(Some(ImportedNumberedImageSequence {
        project,
        report,
        detection,
        paths,
    }))
}

fn decode_or_normalize_image_bytes(bytes: &[u8]) -> Result<(Vec<u8>, String, u32, u32), String> {
    if bytes.is_empty() {
        return Err("image file is empty".into());
    }
    if bytes.len() > neoflash_core::MAX_RESOURCE_BYTES {
        return Err(format!(
            "encoded image exceeds the {} MiB resource limit",
            neoflash_core::MAX_RESOURCE_BYTES / (1024 * 1024)
        ));
    }
    let format = image::guess_format(bytes).map_err(|e| format!("cannot detect image: {e}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(neoflash_core::MAX_BITMAP_DIMENSION);
    limits.max_image_height = Some(neoflash_core::MAX_BITMAP_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| format!("cannot decode bounded image: {e}"))?;
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
    let clean = desired.trim_start_matches(['/', '\\']).replace('\\', "/");
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
    if entries.len() > MAX_RESOURCES {
        return Err(ImportError::Container(format!(
            "Image sequence contains {} files, exceeding the {MAX_RESOURCES}-resource limit",
            entries.len()
        )));
    }
    let mut input_bytes = 0usize;
    for (name, bytes) in entries {
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(ImportError::Container(format!(
                "Image sequence entry '{name}' exceeds the {} MiB resource limit",
                MAX_RESOURCE_BYTES / (1024 * 1024)
            )));
        }
        input_bytes = input_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| ImportError::Container("Image sequence byte count overflowed".into()))?;
        if input_bytes > MAX_RESOURCE_TOTAL_BYTES {
            return Err(ImportError::Container(format!(
                "Image sequence input exceeds the {} MiB aggregate resource limit",
                MAX_RESOURCE_TOTAL_BYTES / (1024 * 1024)
            )));
        }
    }
    let mut candidate = project.clone();
    let report = import_image_sequence_bytes_inner(&mut candidate, entries, options)?;
    candidate.validate().map_err(|error| {
        ImportError::Container(format!(
            "Image sequence import produced an invalid project: {error}"
        ))
    })?;
    *project = candidate;
    Ok(report)
}

fn import_image_sequence_bytes_inner(
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
    let seq_base_name = if detection
        .prefix
        .trim_end_matches(['_', '-', '.', ' '])
        .is_empty()
    {
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
    let required_resources = project
        .resources
        .len()
        .checked_add(detection.items.len())
        .ok_or_else(|| ImportError::Container("Image sequence resource count overflowed".into()))?;
    if required_resources > MAX_RESOURCES {
        return Err(ImportError::Container(format!(
            "Image sequence would exceed the {MAX_RESOURCES}-resource project limit"
        )));
    }
    let mut retained_resource_bytes = project
        .resources
        .iter()
        .try_fold(0usize, |total, resource| {
            total.checked_add(resource.byte_len())
        })
        .ok_or_else(|| ImportError::Container("Project resource byte count overflowed".into()))?;
    if retained_resource_bytes > MAX_RESOURCE_TOTAL_BYTES {
        return Err(ImportError::Container(
            "Project resources already exceed the 64 MiB aggregate resource limit".into(),
        ));
    }

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
                retained_resource_bytes = retained_resource_bytes
                    .checked_add(norm_bytes.len())
                    .ok_or_else(|| {
                        ImportError::Container(
                            "Image sequence normalized resource byte count overflowed".into(),
                        )
                    })?;
                if retained_resource_bytes > MAX_RESOURCE_TOTAL_BYTES {
                    return Err(ImportError::Container(format!(
                        "Image sequence normalized resources exceed the {} MiB aggregate resource limit",
                        MAX_RESOURCE_TOTAL_BYTES / (1024 * 1024)
                    )));
                }
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
                let rel_path =
                    unique_resource_path(project, &format!("sequence/{stem}.{desired_ext}"));
                let res_id = new_id("resource");
                let unit =
                    ResourceUnit::from_bytes(res_id.clone(), rel_path, media_type, &norm_bytes)
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
                    (
                        (project.width as f64 - pw) * 0.5,
                        (project.height as f64 - ph) * 0.5,
                    )
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
    if paths.len() > MAX_RESOURCES {
        return Err(ImportError::Container(format!(
            "Image sequence contains {} files, exceeding the {MAX_RESOURCES}-resource limit",
            paths.len()
        )));
    }
    let mut expected_total_bytes = 0u64;
    for p in paths {
        let path = p.as_ref();
        let metadata = std::fs::metadata(path).map_err(|error| {
            ImportError::Container(format!(
                "{}: cannot inspect image sequence entry: {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(ImportError::Container(format!(
                "{}: image sequence entry is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_RESOURCE_BYTES as u64 {
            return Err(ImportError::Container(format!(
                "Image sequence entry '{}' exceeds the {} MiB resource limit",
                path.display(),
                MAX_RESOURCE_BYTES / (1024 * 1024)
            )));
        }
        expected_total_bytes = expected_total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| ImportError::Container("Image sequence byte count overflowed".into()))?;
        if expected_total_bytes > MAX_RESOURCE_TOTAL_BYTES as u64 {
            return Err(ImportError::Container(format!(
                "Image sequence input exceeds the {} MiB aggregate resource limit",
                MAX_RESOURCE_TOTAL_BYTES / (1024 * 1024)
            )));
        }
    }

    let mut entries = Vec::with_capacity(paths.len());
    for p in paths {
        let path = p.as_ref();
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|error| {
                ImportError::Container(format!(
                    "{}: cannot open image sequence entry: {error}",
                    path.display()
                ))
            })?
            .take(MAX_RESOURCE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                ImportError::Container(format!(
                    "{}: cannot read image sequence entry: {error}",
                    path.display()
                ))
            })?;
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(ImportError::Container(format!(
                "Image sequence entry '{}' exceeds the {} MiB resource limit",
                path.display(),
                MAX_RESOURCE_BYTES / (1024 * 1024)
            )));
        }
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

#[cfg(test)]
mod path_import_tests {
    use super::*;

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn png_dimensions_header(width: u32, height: u32) -> Vec<u8> {
        fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc_input = Vec::with_capacity(kind.len() + data.len());
            crc_input.extend_from_slice(kind);
            crc_input.extend_from_slice(data);
            out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        }

        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        push_chunk(&mut out, b"IHDR", &ihdr);
        push_chunk(&mut out, b"IEND", &[]);
        out
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "neoflash-{name}-{}-{}",
            std::process::id(),
            new_id("sequence-test")
        ))
    }

    #[test]
    fn numbered_path_import_rejects_files_counts_and_dimensions_before_unbounded_work() {
        let root = temp_dir("sequence-bounds");
        std::fs::create_dir_all(&root).unwrap();

        let too_large_dir = root.join("file-size");
        std::fs::create_dir_all(&too_large_dir).unwrap();
        let too_large = too_large_dir.join("frame_0001.png");
        std::fs::File::create(&too_large)
            .unwrap()
            .set_len(MAX_RESOURCE_BYTES as u64 + 1)
            .unwrap();
        let err = import_numbered_image_sequence_paths(std::slice::from_ref(&too_large), true)
            .expect_err("oversized sequence file must be rejected");
        assert!(err.to_string().contains("resource limit"), "{err}");

        let huge_dimensions_dir = root.join("dimensions");
        std::fs::create_dir_all(&huge_dimensions_dir).unwrap();
        let huge_dimensions = huge_dimensions_dir.join("frame_0001.png");
        std::fs::write(
            &huge_dimensions,
            png_dimensions_header(neoflash_core::MAX_BITMAP_DIMENSION + 1, 1),
        )
        .unwrap();
        let err =
            import_numbered_image_sequence_paths(std::slice::from_ref(&huge_dimensions), true)
                .expect_err("oversized PNG dimensions must be rejected by bounded decode");
        assert!(
            err.to_string().contains("cannot decode bounded image"),
            "{err}"
        );

        let too_many: Vec<PathBuf> = (0..=MAX_RESOURCES)
            .map(|index| root.join(format!("frame_{index:04}.png")))
            .collect();
        let err = resolve_numbered_image_sequence_paths(&too_many, false)
            .expect_err("too many sequence inputs must be rejected before file IO");
        assert!(err.to_string().contains("resource limit"), "{err}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn non_numbered_single_image_is_not_claimed_as_a_sequence() {
        let ordinary = PathBuf::from("poster.png");
        assert!(
            resolve_numbered_image_sequence_paths(std::slice::from_ref(&ordinary), true)
                .unwrap()
                .is_none()
        );
        assert!(
            import_numbered_image_sequence_paths(std::slice::from_ref(&ordinary), true)
                .unwrap()
                .is_none()
        );
    }
}
