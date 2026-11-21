//! Filesystem-safe output names shared by GUI export routes.
//!
//! GUI sequence exporters open their native save dialog inside one child directory named after
//! the project. Explicitly configured final paths and headless `--out` paths bypass these helpers
//! and therefore keep their exact-path contract.

use neoflash_core::Project;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// Leave enough room for numbered-frame suffixes and the sibling temporary names used by atomic
// writers. Linux/macOS commonly cap a component at 255 UTF-8 bytes while Windows counts UTF-16
// code units, so enforce both budgets instead of counting Unicode scalar values.
const MAX_FOLDER_UTF8_BYTES: usize = 180;
const MAX_FOLDER_UTF16_UNITS: usize = 180;
const MAX_FILE_STEM_UTF8_BYTES: usize = 160;
const MAX_FILE_STEM_UTF16_UNITS: usize = 160;

fn windows_device_name(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn safe_component(
    raw: &str,
    fallback: &str,
    max_utf8_bytes: usize,
    max_utf16_units: usize,
) -> String {
    let mut out = String::new();
    let mut previous_replacement = false;
    let mut utf16_units = 0usize;
    for ch in raw.trim().chars() {
        let forbidden =
            ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        let candidate = if forbidden {
            if previous_replacement {
                continue;
            }
            '_'
        } else {
            ch
        };
        let next_utf8 = out.len().saturating_add(candidate.len_utf8());
        let next_utf16 = utf16_units.saturating_add(candidate.len_utf16());
        if next_utf8 > max_utf8_bytes || next_utf16 > max_utf16_units {
            break;
        }
        out.push(candidate);
        utf16_units = next_utf16;
        previous_replacement = forbidden;
    }
    let trimmed = out.trim_matches(|ch| ch == ' ' || ch == '.');
    let mut safe = if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    };
    if windows_device_name(&safe) {
        while safe.len().saturating_add(1) > max_utf8_bytes
            || safe.encode_utf16().count().saturating_add(1) > max_utf16_units
        {
            safe.pop();
        }
        safe.insert(0, '_');
    }
    safe
}

/// Keep the human project name (including Unicode and spaces) while producing a component valid
/// on macOS, Windows and ordinary Linux filesystems. The byte/unit budgets leave room for sibling
/// staging names and target filenames.
pub(crate) fn safe_project_folder_name(raw: &str) -> String {
    safe_component(
        raw,
        "Untitled",
        MAX_FOLDER_UTF8_BYTES,
        MAX_FOLDER_UTF16_UNITS,
    )
}

/// Produce a portable file stem while retaining Unicode and the user's chosen basename.
pub(crate) fn safe_output_file_stem(raw: &str, fallback: &str) -> String {
    safe_component(
        raw,
        fallback,
        MAX_FILE_STEM_UTF8_BYTES,
        MAX_FILE_STEM_UTF16_UNITS,
    )
}

pub(crate) fn safe_output_file_name(
    raw_name: &str,
    fallback_stem: &str,
    extension: &str,
) -> String {
    let raw_stem = Path::new(raw_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(raw_name);
    let stem = safe_output_file_stem(raw_stem, fallback_stem);
    format!("{stem}.{extension}")
}

pub(crate) fn criteo_archive_name(project: &Project) -> String {
    format!("{}.zip", safe_output_file_stem(&project.name, "Untitled"))
}

pub(crate) fn project_folder_name(project: &Project) -> String {
    safe_project_folder_name(&project.name)
}

fn already_project_folder(parent: &Path, name: &str) -> bool {
    let exact_name = parent
        .file_name()
        .and_then(|component| component.to_str())
        .is_some_and(|component| component == name);
    exact_name
        || parent.file_name().is_some_and(|_| {
            same_file::is_same_file(parent, parent.with_file_name(name)).unwrap_or(false)
        })
}

/// Resolve a parent-folder chooser result to the project's final output directory without adding
/// the same component twice when the user selected that directory directly.
pub(crate) fn project_output_dir(parent: &Path, project: &Project) -> PathBuf {
    let name = project_folder_name(project);
    if already_project_folder(parent, &name) {
        parent.to_path_buf()
    } else {
        parent.join(name)
    }
}

/// Ensure the save dialog opens inside the actual project-named directory. Existing directories
/// are never cleaned or claimed: their contents remain visible to the native save dialog so its
/// ordinary overwrite confirmation applies to the basename the user selects.
pub(crate) fn prepare_project_output_dir(
    parent: &Path,
    project: &Project,
) -> io::Result<(PathBuf, bool)> {
    let output_dir = project_output_dir(parent, project);
    if output_dir.is_dir() {
        return Ok((output_dir, false));
    }
    if output_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "project output path '{}' exists and is not a directory",
                output_dir.display()
            ),
        ));
    }
    fs::create_dir_all(&output_dir)?;
    Ok((output_dir, true))
}

pub(crate) fn remove_created_output_dir_if_empty(path: &Path, created: bool) {
    if created {
        let _ = fs::remove_dir(path);
    }
}

/// Put a GUI-selected sequence base file inside the project's folder. Non-portable basenames are
/// rejected instead of silently redirecting a native save confirmation to another path. Frame
/// numbering and the format-specific extension remain controlled by the exporter.
pub(crate) fn project_sequence_output_path(
    chosen: &Path,
    prepared_output_dir: &Path,
    fallback_stem: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    let raw_file_name = chosen
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "Sequence name must be a non-empty Unicode filename. Choose another name.".to_owned()
        })?;
    let raw_stem = Path::new(raw_file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(raw_file_name);
    let safe_stem = safe_output_file_stem(raw_stem, fallback_stem);
    if raw_stem != safe_stem {
        return Err(format!(
            "Sequence name '{raw_stem}' is not portable across macOS, Windows and Linux. Choose another name (for example '{safe_stem}')."
        ));
    }
    let raw_extension = Path::new(raw_file_name)
        .extension()
        .and_then(|value| value.to_str());
    if raw_extension != Some(extension) {
        return Err(format!(
            "Sequence filename must end with '.{extension}'. Choose a filename with that extension."
        ));
    }
    let parent = chosen
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let same_directory = parent == prepared_output_dir
        || fs::canonicalize(parent)
            .ok()
            .zip(fs::canonicalize(prepared_output_dir).ok())
            .is_some_and(|(actual, prepared)| actual == prepared);
    if !same_directory {
        return Err(format!(
            "Sequence output must stay inside the selected project folder '{}'. Choose a filename in that folder.",
            prepared_output_dir.display()
        ));
    }
    Ok(prepared_output_dir.join(raw_file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folder_name_preserves_unicode_and_blocks_cross_platform_special_names() {
        assert_eq!(
            safe_project_folder_name(" Ideas in motion "),
            "Ideas in motion"
        );
        assert_eq!(
            safe_project_folder_name("Проект テスト 🎬"),
            "Проект テスト 🎬"
        );
        assert_eq!(safe_project_folder_name("A/:*?\\B"), "A_B");
        assert_eq!(safe_project_folder_name(" .. "), "Untitled");
        assert_eq!(safe_project_folder_name("CON"), "_CON");
        assert_eq!(safe_project_folder_name("conin$"), "_conin$");
        assert_eq!(safe_project_folder_name("CONOUT$.txt"), "_CONOUT$.txt");
        assert_eq!(safe_project_folder_name("COM¹"), "_COM¹");
        assert_eq!(safe_project_folder_name("lpt³.log"), "_lpt³.log");
        assert_eq!(safe_project_folder_name("lpt9.txt"), "_lpt9.txt");
        let emoji = safe_project_folder_name(&"🎬".repeat(120));
        assert!(emoji.len() <= MAX_FOLDER_UTF8_BYTES);
        assert!(emoji.encode_utf16().count() <= MAX_FOLDER_UTF16_UNITS);
        assert!(std::str::from_utf8(emoji.as_bytes()).is_ok());
        assert_eq!(safe_output_file_stem("a?b<>|*", "export"), "a_b_");
        assert_eq!(safe_output_file_stem("nul", "export"), "_nul");
        let emoji_stem = safe_output_file_stem(&"🎬".repeat(120), "export");
        assert!(emoji_stem.len() <= MAX_FILE_STEM_UTF8_BYTES);
        assert!(emoji_stem.encode_utf16().count() <= MAX_FILE_STEM_UTF16_UNITS);
    }

    #[test]
    fn project_output_paths_add_one_named_folder_only() {
        let mut project = Project::blank();
        project.name = "Ideas in motion".into();
        let prepared = Path::new("/tmp/exports/Ideas in motion");
        assert_eq!(
            project_output_dir(Path::new("/tmp/exports"), &project),
            PathBuf::from("/tmp/exports/Ideas in motion")
        );
        assert_eq!(
            project_output_dir(Path::new("/tmp/exports/Ideas in motion"), &project),
            PathBuf::from("/tmp/exports/Ideas in motion")
        );
        project.name = "Demo".into();
        assert_eq!(
            project_output_dir(Path::new("/tmp/exports/demo"), &project),
            PathBuf::from("/tmp/exports/demo/Demo")
        );
        project.name = "Ideas in motion".into();
        assert!(
            project_sequence_output_path(
                Path::new("/tmp/exports/frame.png"),
                prepared,
                "export",
                "png",
            )
            .unwrap_err()
            .contains("must stay inside")
        );
        assert_eq!(
            project_sequence_output_path(
                Path::new("/tmp/exports/Ideas in motion/frame.png"),
                prepared,
                "export",
                "png",
            )
            .unwrap(),
            PathBuf::from("/tmp/exports/Ideas in motion/frame.png")
        );
        assert!(
            project_sequence_output_path(
                Path::new("/tmp/exports/Ideas in motion/A?B.png"),
                prepared,
                "export",
                "png",
            )
            .unwrap_err()
            .contains("not portable")
        );
        assert!(
            project_sequence_output_path(Path::new("/"), prepared, "export", "png")
                .unwrap_err()
                .contains("non-empty Unicode filename")
        );
        assert!(
            project_sequence_output_path(
                Path::new("/tmp/exports/Ideas in motion/frame.jpg"),
                prepared,
                "export",
                "png",
            )
            .unwrap_err()
            .contains("must end with '.png'")
        );
        assert!(
            project_sequence_output_path(
                Path::new("/tmp/exports/Ideas in motion/frame.PNG"),
                prepared,
                "export",
                "png",
            )
            .unwrap_err()
            .contains("must end with '.png'")
        );
    }

    #[test]
    fn project_output_path_obeys_the_actual_filesystem_case_policy() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-output-case-policy-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let selected = root.join("demo");
        fs::create_dir_all(&selected).unwrap();
        let mut project = Project::blank();
        project.name = "Demo".into();

        let resolved = project_output_dir(&selected, &project);
        if root.join("Demo").is_dir() {
            assert_eq!(resolved, selected);
        } else {
            assert_eq!(resolved, selected.join("Demo"));
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparing_output_dir_preserves_existing_unowned_contents_and_cleans_only_new_empty_dirs() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-output-path-policy-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut project = Project::blank();
        project.name = "Output policy".into();

        let (created_dir, created) = prepare_project_output_dir(&root, &project).unwrap();
        assert!(created);
        remove_created_output_dir_if_empty(&created_dir, created);
        assert!(!created_dir.exists());

        fs::create_dir_all(&created_dir).unwrap();
        fs::write(created_dir.join("keep.txt"), b"unowned").unwrap();
        let (existing_dir, created) = prepare_project_output_dir(&root, &project).unwrap();
        assert_eq!(existing_dir, created_dir);
        assert!(!created);
        remove_created_output_dir_if_empty(&existing_dir, created);
        assert_eq!(fs::read(existing_dir.join("keep.txt")).unwrap(), b"unowned");

        fs::remove_dir_all(root).unwrap();
    }
}
