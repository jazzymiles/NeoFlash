//! Filesystem-safe output names shared by GUI export routes.
//!
//! A folder chooser that asks for a *parent* directory must consistently create one child named
//! after the project. Explicitly configured final paths and headless `--out` paths bypass these
//! helpers and therefore keep their exact-path contract.

use neoflash_core::Project;
use std::path::{Path, PathBuf};

const MAX_FOLDER_CHARS: usize = 120;

fn windows_device_name(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

/// Keep the human project name (including Unicode and spaces) while producing a component valid
/// on macOS and Windows. The limit leaves room for sibling staging names and target filenames.
pub(crate) fn safe_project_folder_name(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_replacement = false;
    for ch in raw.trim().chars() {
        let forbidden =
            ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        if forbidden {
            if !previous_replacement {
                out.push('_');
            }
            previous_replacement = true;
        } else {
            out.push(ch);
            previous_replacement = false;
        }
        if out.chars().count() >= MAX_FOLDER_CHARS {
            break;
        }
    }
    let trimmed = out.trim_matches(|ch| ch == ' ' || ch == '.');
    let mut safe = if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        "Untitled".to_owned()
    } else {
        trimmed.to_owned()
    };
    if windows_device_name(&safe) {
        safe.insert(0, '_');
    }
    safe
}

pub(crate) fn project_folder_name(project: &Project) -> String {
    safe_project_folder_name(&project.name)
}

fn already_project_folder(parent: &Path, name: &str) -> bool {
    parent
        .file_name()
        .and_then(|component| component.to_str())
        .is_some_and(|component| component == name || component.eq_ignore_ascii_case(name))
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

/// Put a GUI-selected sequence base file inside the project's folder. The filename remains the
/// one shown in the save dialog, so frame numbering and format-specific naming stay unchanged.
pub(crate) fn project_sequence_output_path(chosen: &Path, project: &Project) -> PathBuf {
    let name = project_folder_name(project);
    let file_name = chosen.file_name().unwrap_or_default();
    let parent = chosen
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if already_project_folder(parent, &name) {
        chosen.to_path_buf()
    } else {
        parent.join(name).join(file_name)
    }
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
        assert_eq!(safe_project_folder_name("lpt9.txt"), "_lpt9.txt");
    }

    #[test]
    fn project_output_paths_add_one_named_folder_only() {
        let mut project = Project::blank();
        project.name = "Ideas in motion".into();
        assert_eq!(
            project_output_dir(Path::new("/tmp/exports"), &project),
            PathBuf::from("/tmp/exports/Ideas in motion")
        );
        assert_eq!(
            project_output_dir(Path::new("/tmp/exports/Ideas in motion"), &project),
            PathBuf::from("/tmp/exports/Ideas in motion")
        );
        assert_eq!(
            project_sequence_output_path(Path::new("/tmp/exports/frame.png"), &project),
            PathBuf::from("/tmp/exports/Ideas in motion/frame.png")
        );
        assert_eq!(
            project_sequence_output_path(
                Path::new("/tmp/exports/Ideas in motion/frame.png"),
                &project,
            ),
            PathBuf::from("/tmp/exports/Ideas in motion/frame.png")
        );
    }
}
