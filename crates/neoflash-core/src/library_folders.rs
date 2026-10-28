//! Logical library folders are authored document data, never filesystem paths.
use crate::Project;
use std::collections::BTreeSet;

pub fn valid_library_folder(path: &str) -> bool {
    path.len() <= 1024
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && (path.is_empty()
            || path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".." && part.trim() == part))
}

fn ancestors(path: &str) -> impl Iterator<Item = String> + '_ {
    path.match_indices('/')
        .map(|(index, _)| path[..index].to_owned())
        .chain((!path.is_empty()).then(|| path.to_owned()))
}

impl Project {
    pub fn library_folder_paths(&self) -> Vec<String> {
        self.library_folders
            .iter()
            .map(String::as_str)
            .chain(self.symbols.iter().map(|symbol| symbol.folder.as_str()))
            .flat_map(ancestors)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn add_library_folder(&mut self, path: &str) -> Result<(), String> {
        if !valid_library_folder(path) {
            return Err("Use folder names without empty segments, slashes at either end, control characters, or '.' / '..'.".into());
        }
        if path.is_empty() {
            return Ok(());
        }
        let mut folders: BTreeSet<_> = self.library_folder_paths().into_iter().collect();
        folders.extend(ancestors(path));
        self.library_folders = folders.into_iter().collect();
        self.format_version = self.format_version.max(4);
        Ok(())
    }

    pub fn move_library_symbol(&mut self, symbol_id: &str, folder: &str) -> Result<(), String> {
        if symbol_id == self.root {
            return Err("The scene stays outside the symbol library.".into());
        }
        if self.symbol(symbol_id).is_none() {
            return Err("The selected symbol no longer exists.".into());
        }
        self.add_library_folder(folder)?;
        self.symbol_mut(symbol_id).unwrap().folder = folder.to_owned();
        Ok(())
    }

    pub fn relocate_library_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        if from.is_empty() || to.is_empty() || !valid_library_folder(to) {
            return Err("Choose a valid named destination folder.".into());
        }
        if from == to {
            return Ok(());
        }
        if to.starts_with(&format!("{from}/")) {
            return Err("A folder cannot be moved into itself.".into());
        }
        let folders = self.library_folder_paths();
        if !folders.iter().any(|folder| folder == from) {
            return Err("The selected folder no longer exists.".into());
        }
        if folders.iter().any(|folder| folder == to) {
            return Err("A folder with this destination already exists.".into());
        }
        let prefix = format!("{from}/");
        let relocate = |path: &str| {
            if path == from {
                to.to_owned()
            } else if let Some(tail) = path.strip_prefix(&prefix) {
                format!("{to}/{tail}")
            } else {
                path.to_owned()
            }
        };
        self.library_folders = folders.iter().map(|path| relocate(path)).collect();
        for symbol in &mut self.symbols {
            symbol.folder = relocate(&symbol.folder);
        }
        self.add_library_folder(to)
    }
}
