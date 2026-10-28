use crate::{ImportError, ImportFormat};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

const MAX_FILES: usize = 10_000;
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

pub(crate) struct Source {
    pub format: ImportFormat,
    pub version: Option<String>,
    pub files: BTreeMap<String, String>,
    pub binary: Option<crate::legacy::BinaryTimelineDocument>,
}

pub(crate) fn read(path: &Path) -> Result<Source, ImportError> {
    let mut files = BTreeMap::new();
    if path.is_dir() {
        read_directory(path, &mut files)?;
        return Ok(Source {
            format: if files.contains_key("DOMDocument.xml") {
                ImportFormat::XflDirectory
            } else {
                ImportFormat::Unknown
            },
            version: None,
            files,
            binary: None,
        });
    }
    let mut file = File::open(path)?;
    let mut header = [0u8; 16];
    let read = file.read(&mut header)?;
    if path.file_name().is_some_and(|n| n == "DOMDocument.xml")
        || header[..read].starts_with(b"PROXY-CS5")
    {
        return read_directory_source(path.parent().unwrap_or_else(|| Path::new(".")));
    }
    if read >= 8 && matches!(&header[..3], b"FWS" | b"CWS" | b"ZWS") {
        let declared = u32::from_le_bytes(header[4..8].try_into().unwrap());
        if declared < 8 {
            return Err(ImportError::Container("invalid SWF declared length".into()));
        }
        if &header[..3] == b"FWS" && u64::from(declared) != file.metadata()?.len() {
            return Err(ImportError::Container(
                "uncompressed SWF length does not match its header".into(),
            ));
        }
        return Ok(Source {
            format: ImportFormat::Swf,
            version: Some(format!(
                "{} ({})",
                header[3],
                String::from_utf8_lossy(&header[..3])
            )),
            files,
            binary: None,
        });
    }
    if header[..read].starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        let binary = crate::legacy::read_for_import(path)?;
        return Ok(Source {
            format: if binary.is_some() {
                ImportFormat::LegacyBinaryFla
            } else {
                ImportFormat::Unknown
            },
            version: binary
                .as_ref()
                .and_then(|b| b.inspection.authoring_version.clone()),
            files,
            binary,
        });
    }
    if header[..read].starts_with(b"PK\x03\x04") {
        if file.metadata()?.len() > MAX_TOTAL {
            return Err(ImportError::Container("archive exceeds 64 MiB".into()));
        }
        let mut zip = zip::ZipArchive::new(File::open(path)?)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        if zip.len() > MAX_FILES {
            return Err(ImportError::Container(
                "archive exceeds 10000 entries".into(),
            ));
        }
        let mut total = 0u64;
        let mut names = std::collections::HashSet::new();
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| ImportError::Container(e.to_string()))?;
            let name = entry.name().to_owned();
            safe_name(&name)?;
            if !names.insert(name.clone()) {
                return Err(ImportError::Container(format!(
                    "duplicate ZIP entry: {name}"
                )));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(ImportError::Container(format!("symlink ZIP entry: {name}")));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| ImportError::Container("ZIP size overflow".into()))?;
            if entry.size() > MAX_FILE || total > MAX_TOTAL {
                return Err(ImportError::Container(
                    "ZIP exceeds 8 MiB per entry or 64 MiB total uncompressed limit".into(),
                ));
            }
            if name.ends_with(".xml") && !entry.is_dir() {
                let mut content = String::new();
                entry
                    .by_ref()
                    .take(MAX_FILE + 1)
                    .read_to_string(&mut content)?;
                if content.len() as u64 > MAX_FILE {
                    return Err(ImportError::Container(
                        "ZIP entry exceeded its size budget".into(),
                    ));
                }
                files.insert(name, content);
            }
        }
        let format = if files.contains_key("DOMDocument.xml") {
            ImportFormat::XflArchive
        } else {
            ImportFormat::Unknown
        };
        return Ok(Source {
            format,
            version: None,
            files,
            binary: None,
        });
    }
    Ok(Source {
        format: ImportFormat::Unknown,
        version: None,
        files,
        binary: None,
    })
}

fn read_directory_source(path: &Path) -> Result<Source, ImportError> {
    read(path)
}

pub(crate) fn safe_name(name: &str) -> Result<(), ImportError> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0')
        || name.contains("//")
        || name.split('/').any(|part| part == "." || part == "..")
        || Path::new(name).is_absolute()
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(ImportError::Container(format!(
            "unsafe relative path: {name}"
        )));
    }
    Ok(())
}

fn read_directory(root: &Path, files: &mut BTreeMap<String, String>) -> Result<(), ImportError> {
    let root = root.canonicalize()?;
    let mut total = 0;
    let mut count = 0;
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        total: &mut u64,
        count: &mut usize,
        files: &mut BTreeMap<String, String>,
    ) -> Result<(), ImportError> {
        if depth > 16 {
            return Err(ImportError::Container(
                "directory nesting exceeds 16".into(),
            ));
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            *count += 1;
            if *count > MAX_FILES {
                return Err(ImportError::Container(
                    "directory exceeds 10000 entries".into(),
                ));
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(ImportError::Container(format!(
                    "symlink entry is not allowed: {}",
                    entry.path().display()
                )));
            }
            if kind.is_dir() {
                walk(root, &entry.path(), depth + 1, total, count, files)?;
                continue;
            }
            if !kind.is_file() {
                return Err(ImportError::Container("nonregular source entry".into()));
            }
            let size = entry.metadata()?.len();
            *total = total.saturating_add(size);
            if size > MAX_FILE || *total > MAX_TOTAL {
                return Err(ImportError::Container(
                    "directory exceeds 8 MiB per file or 64 MiB total limit".into(),
                ));
            }
            if entry.path().extension().is_some_and(|ext| ext == "xml") {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                safe_name(&name)?;
                let mut content = String::new();
                File::open(entry.path())?
                    .take(MAX_FILE + 1)
                    .read_to_string(&mut content)?;
                if content.len() as u64 > MAX_FILE {
                    return Err(ImportError::Container(
                        "source file grew beyond limit".into(),
                    ));
                }
                files.insert(name, content);
            }
        }
        Ok(())
    }
    walk(&root, &root, 0, &mut total, &mut count, files)
}
