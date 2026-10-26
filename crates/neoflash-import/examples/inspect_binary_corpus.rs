//! Aggregate-only local audit. Never emits paths, stream names, strings or bytes.
use neoflash_import::{ImportFormat, inspect, read_binary_contents, read_binary_geometry};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("provide a local corpus directory")?;
    let output = std::env::args_os()
        .nth(2)
        .ok_or("provide a new aggregate JSON output path")?;
    let mut paths = Vec::new();
    gather(Path::new(&root), 0, &mut paths)?;
    let (mut recognized, mut blocked, mut invalid, mut complete_streams) =
        (0usize, 0usize, 0usize, 0usize);
    let mut versions = BTreeMap::<String, usize>::new();
    let mut roles = BTreeMap::<String, usize>::new();
    let mut codes = BTreeMap::<String, usize>::new();
    let mut classes = BTreeMap::<String, usize>::new();
    let mut failures = BTreeMap::<String, usize>::new();
    let mut complete_records = BTreeMap::<&str, usize>::new();
    let mut geometry_counts = BTreeMap::<&str, usize>::new();
    let mut preserved_shape_blocks = 0;
    let mut contents_counts = BTreeMap::<&str, usize>::new();
    let mut settings_by_target = BTreeMap::<String, usize>::new();
    let mut descriptor_revisions = BTreeMap::<String, usize>::new();
    for path in &paths {
        match inspect(path) {
            Ok(report) => {
                recognized += usize::from(report.format == ImportFormat::LegacyBinaryFla);
                blocked += usize::from(!report.can_import);
                *versions
                    .entry(report.version.unwrap_or_else(|| "unknown".into()))
                    .or_default() += 1;
                if let Some(binary) = report.binary_fla {
                    let expected_shapes = binary
                        .streams
                        .iter()
                        .map(|s| s.geometry.complete_shape_blocks)
                        .sum::<usize>();
                    let preserved = read_binary_geometry(path)?;
                    let retained_shapes = preserved
                        .streams
                        .iter()
                        .map(|s| s.shapes.len())
                        .sum::<usize>();
                    if expected_shapes != retained_shapes {
                        return Err(
                            "geometry retention budget reached or source changed during audit"
                                .into(),
                        );
                    }
                    preserved_shape_blocks += retained_shapes;
                    let contents = read_binary_contents(path)?.contents;
                    let original_length = binary
                        .streams
                        .iter()
                        .find(|stream| stream.role == neoflash_import::BinaryStreamRole::Contents)
                        .ok_or("Contents stream absent")?
                        .byte_length;
                    if contents.original_bytes.len() as u64 != original_length {
                        return Err(
                            "Contents preservation length mismatch or source changed during audit"
                                .into(),
                        );
                    }
                    if contents.settings.is_some() {
                        *settings_by_target
                            .entry(
                                binary
                                    .authoring_version
                                    .clone()
                                    .unwrap_or_else(|| "unknown".into()),
                            )
                            .or_default() += 1;
                    }
                    for descriptor in &contents.descriptors {
                        *descriptor_revisions
                            .entry(format!(
                                "{:?}/base-{}/sub-{:?}",
                                descriptor.class, descriptor.revision, descriptor.subtype_revision
                            ))
                            .or_default() += 1;
                    }
                    let inspected = binary
                        .streams
                        .iter()
                        .find_map(|stream| stream.contents.as_ref())
                        .ok_or("Contents counts absent")?;
                    if inspected.descriptors_complete != contents.descriptors.len()
                        || inspected.settings_reached != contents.settings.is_some()
                    {
                        return Err("Contents inspection/preservation mismatch or source changed during audit".into());
                    }
                    for (kind, count) in [
                        ("descriptors_encountered", inspected.descriptors_encountered),
                        ("descriptors_complete", inspected.descriptors_complete),
                        ("scene_entries", inspected.scene_entries),
                        ("library_entries", inspected.library_entries),
                        ("media_entries", inspected.media_entries),
                        ("resolved_streams", inspected.resolved_streams),
                        ("settings_reached", usize::from(inspected.settings_reached)),
                    ] {
                        *contents_counts.entry(kind).or_default() += count;
                    }
                    for stream in binary.streams {
                        *roles.entry(format!("{:?}", stream.role)).or_default() += 1;
                        complete_streams += usize::from(stream.structure_complete);
                        for (kind, count) in [
                            ("pages", stream.records.pages),
                            ("layers", stream.records.layers),
                            ("frames", stream.records.frames),
                            ("shapes", stream.records.shapes),
                        ] {
                            *complete_records.entry(kind).or_default() += count;
                        }
                        for (kind, count) in [
                            (
                                "complete_shape_blocks",
                                stream.geometry.complete_shape_blocks,
                            ),
                            (
                                "nonempty_shape_blocks",
                                stream.geometry.nonempty_shape_blocks,
                            ),
                            ("fills", stream.geometry.fills),
                            ("strokes", stream.geometry.strokes),
                            ("straight_edges", stream.geometry.straight_edges),
                            ("quadratic_edges", stream.geometry.quadratic_edges),
                            ("auxiliary_curves", stream.geometry.auxiliary_curves),
                            (
                                "unresolved_fill_payloads",
                                stream.geometry.unresolved_fill_payloads,
                            ),
                        ] {
                            *geometry_counts.entry(kind).or_default() += count;
                        }
                        if let Some(class) = stream.root_class {
                            *classes.entry(class).or_default() += 1;
                        }
                    }
                }
                for d in report.diagnostics {
                    *codes.entry(d.code).or_default() += 1;
                }
            }
            Err(error) => {
                invalid += 1;
                let category = match error {
                    neoflash_import::ImportError::Container(message) => {
                        // XFL container errors can include source entry names.
                        // Only our static CFB categories are safe to aggregate.
                        let categories = [
                            "root entry name",
                            "name not null-terminated",
                            "name ordering",
                            "adjacent red nodes",
                            "loop in tree",
                            "root stream len",
                            "FAT has",
                            "not marked",
                            "MiniFAT",
                            "directory",
                            "DIFAT",
                            "FAT",
                            "sector",
                            "header or directory metadata",
                        ];
                        categories
                            .into_iter()
                            .find(|c| message == format!("invalid CFB structure ({c})"))
                            .unwrap_or("container limit or format rejection")
                            .to_owned()
                    }
                    _ => "read failure".into(),
                };
                *failures.entry(category).or_default() += 1;
            }
        }
    }
    let result = json!({"files":paths.len(), "recognized_binary_fla":recognized, "blocked_editable_conversion":blocked, "container_failures":invalid, "container_failure_categories":failures, "save_targets":versions, "stream_roles":roles, "complete_structure_streams":complete_streams, "complete_records":complete_records, "geometry_counts":geometry_counts, "preserved_shape_blocks":preserved_shape_blocks, "contents_counts":contents_counts, "settings_by_save_target":settings_by_target, "descriptor_revisions":descriptor_revisions, "root_classes":classes, "diagnostic_codes":codes});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(&mut file, &result)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn gather(path: &Path, depth: usize, paths: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if depth > 16 || paths.len() > 10_000 {
        return Err(std::io::Error::other("corpus budget exceeded"));
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            gather(&entry.path(), depth + 1, paths)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
        {
            paths.push(entry.path());
        }
    }
    Ok(())
}
