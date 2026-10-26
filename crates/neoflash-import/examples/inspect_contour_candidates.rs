//! Aggregate-only candidate audit. Never emits names, coordinates, scripts or
//! artwork, and never renders or enables document conversion.
use neoflash_import::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
fn gather(
    root: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if depth > 16 {
        return Err("directory depth limit".into());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            gather(&entry.path(), depth + 1, out)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
        {
            if out.len() == 10_000 {
                return Err("file count limit".into());
            }
            out.push(entry.path());
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("provide local corpus directory")?;
    let output = std::env::args_os()
        .nth(2)
        .ok_or("provide new aggregate JSON output path")?;
    let mut paths = vec![];
    gather(Path::new(&root), 0, &mut paths)?;
    let mut failures = 0;
    let mut f3 = 0;
    let mut other = 0;
    let mut empty = 0;
    let mut attempted = 0;
    let mut accepted = 0;
    let mut accepted_complete_owner = 0;
    let mut contours = 0;
    let mut edges = 0;
    let mut rejected = BTreeMap::<String, usize>::new();
    for path in &paths {
        let document = match read_binary_timeline(path) {
            Ok(v) => v,
            Err(_) => {
                failures += 1;
                continue;
            }
        };
        if document.contents.revision != Some(0x14) {
            other += 1;
            continue;
        }
        f3 += 1;
        for stream in &document.streams {
            for (index, shape) in stream.shapes.iter().enumerate() {
                if shape.record.edges.is_empty() {
                    empty += 1;
                    continue;
                }
                attempted += 1;
                match adapt_binary_solid_shape(
                    &shape.record,
                    BinaryContourContext {
                        contents_revision: 0x14,
                    },
                ) {
                    Ok(value) => {
                        accepted += 1;
                        contours += value.contours.len();
                        edges += value.contours.iter().map(|c| c.edges.len()).sum::<usize>();
                        accepted_complete_owner += usize::from(
                            stream
                                .objects
                                .iter()
                                .any(|o| o.shape_index == Some(index) && o.complete),
                        );
                    }
                    Err(error) => *rejected.entry(error.code).or_default() += 1,
                }
            }
        }
    }
    let report = json!({
        "files":paths.len(),"container_failures":failures,"flash3_containers":f3,"other_targets_skipped":other,
        "empty_shape_blocks_not_attempted":empty,"nonempty_shape_candidates":attempted,
        "accepted_shape_candidates":accepted,"accepted_with_complete_enclosing_record":accepted_complete_owner,
        "accepted_contours":contours,"accepted_boundary_edges":edges,"rejected_by_code":rejected,
        "editable_conversion_enabled":false,
        "scope":"Flash3 shape2/data4 one opaque plain solid fill; candidate shapes, not complete documents"
    });
    let json = serde_json::to_string_pretty(&report)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(json.as_bytes())?;
    println!("{json}");
    Ok(())
}
