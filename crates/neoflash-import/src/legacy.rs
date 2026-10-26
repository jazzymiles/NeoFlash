//! Bounded, read-only CFB and binary FLA structure inspection.
//! No byte-pattern recovery: unknown records stop the current stream.
mod mfc;

use crate::{ImportError, ImportReport};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

const MAX_CONTAINER: u64 = 64 * 1024 * 1024;
const MAX_STREAM: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryStreamRole {
    Contents,
    Scene,
    Symbol,
    Media,
    Other,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecordCounts {
    pub encountered: usize,
    pub complete: usize,
    pub pages: usize,
    pub layers: usize,
    pub frames: usize,
    pub shapes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryStreamInspection {
    /// Inventory index, deliberately independent of private stream/library names.
    pub index: usize,
    pub role: BinaryStreamRole,
    pub byte_length: u64,
    pub bytes_consumed: usize,
    pub structure_complete: bool,
    pub root_class: Option<String>,
    pub root_schema: Option<u16>,
    pub records: RecordCounts,
    pub stop_code: Option<String>,
    pub stop_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryFlaInspection {
    pub cfb_major_version: u16,
    pub contents_revision: Option<u8>,
    /// Save-target version, not proof of the application that last edited the file.
    pub authoring_version: Option<String>,
    pub storage_count: usize,
    pub logical_bytes: u64,
    pub streams: Vec<BinaryStreamInspection>,
}

fn container(message: impl Into<String>) -> ImportError {
    ImportError::Container(message.into())
}

pub(crate) fn inspect(path: &Path) -> Result<Option<BinaryFlaInspection>, ImportError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_CONTAINER {
        return Err(container("CFB exceeds 64 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONTAINER + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTAINER {
        return Err(container("CFB grew beyond 64 MiB"));
    }
    let major = preflight(&bytes)?;
    // Snapshot bounds allocation and makes concurrent source changes harmless.
    let mut compound = cfb::OpenOptions::new()
        .strict()
        .max_buffer_size(8192)
        .open_with(Cursor::new(bytes))
        .map_err(|e| {
            let text = e.to_string();
            // Never forward library errors containing document stream names.
            let category = [
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
            ]
            .into_iter()
            .find(|s| text.contains(s))
            .unwrap_or("header or directory metadata");
            container(format!("invalid CFB structure ({category})"))
        })?;
    let mut paths = BTreeSet::new();
    let mut entries = Vec::new();
    let mut storage_count = 0;
    let mut logical_bytes = 0u64;
    for (count, entry) in compound.walk().enumerate() {
        if count >= MAX_ENTRIES {
            return Err(container("CFB exceeds 10000 directory entries"));
        }
        if entry.path().components().count() > 17 {
            return Err(container("CFB storage nesting exceeds 16"));
        }
        if !paths.insert(entry.path().to_string_lossy().to_uppercase()) {
            return Err(container("duplicate CFB directory path"));
        }
        if !entry.is_stream() {
            storage_count += 1;
            continue;
        }
        if entry.len() > MAX_STREAM {
            return Err(container("CFB stream exceeds 8 MiB"));
        }
        logical_bytes = logical_bytes
            .checked_add(entry.len())
            .ok_or_else(|| container("CFB logical size overflow"))?;
        if logical_bytes > MAX_CONTAINER {
            return Err(container("CFB logical streams exceed 64 MiB"));
        }
        let role = if entry.path().parent() == Some(Path::new("/")) {
            role(entry.name())
        } else {
            BinaryStreamRole::Other
        };
        entries.push((entry.path().to_path_buf(), entry.len(), role));
    }
    let has_contents = entries.iter().any(|e| e.2 == BinaryStreamRole::Contents);
    let has_timeline = entries
        .iter()
        .any(|e| matches!(e.2, BinaryStreamRole::Scene | BinaryStreamRole::Symbol));
    if !has_contents || !has_timeline {
        return Ok(None);
    }
    let mut result = BinaryFlaInspection {
        cfb_major_version: major,
        contents_revision: None,
        authoring_version: None,
        storage_count,
        logical_bytes,
        streams: vec![],
    };
    for (index, (path, byte_length, role)) in entries.into_iter().enumerate() {
        let mut data = Vec::new();
        compound
            .open_stream(path)
            .map_err(|_| container("cannot open CFB stream"))?
            .take(MAX_STREAM + 1)
            .read_to_end(&mut data)
            .map_err(|_| container("invalid or truncated CFB stream chain"))?;
        if data.len() as u64 != byte_length {
            return Err(container("CFB stream length mismatch"));
        }
        let mut stream = BinaryStreamInspection {
            index,
            role,
            byte_length,
            bytes_consumed: 0,
            structure_complete: false,
            root_class: None,
            root_schema: None,
            records: RecordCounts::default(),
            stop_code: None,
            stop_message: None,
        };
        match role {
            BinaryStreamRole::Contents => {
                if let Some(&revision) = data.first() {
                    result.contents_revision = Some(revision);
                    result.authoring_version = version(revision).map(str::to_owned);
                    stream.bytes_consumed = 1;
                    stream.stop_code = Some(
                        if result.authoring_version.is_some() {
                            "fla_contents_payload_unsupported"
                        } else {
                            "fla_revision_unsupported"
                        }
                        .into(),
                    );
                    stream.stop_message = Some("Contents revision read; scene/library descriptors, document settings and publish/linkage data require further decoding.".into());
                } else {
                    stream.stop_code = Some("fla_truncated_record".into());
                    stream.stop_message = Some("Contents stream has no revision byte.".into());
                }
            }
            BinaryStreamRole::Scene | BinaryStreamRole::Symbol => mfc::inspect(&data, &mut stream),
            BinaryStreamRole::Media => {
                stream.stop_code = Some("fla_media_unsupported".into());
                stream.stop_message = Some(
                    "Media stream validated at container level; payload is not decoded.".into(),
                );
            }
            BinaryStreamRole::Other => {
                stream.stop_code = Some("fla_stream_unsupported".into());
                stream.stop_message =
                    Some("Unrecognized stream; no content has been discarded or converted.".into());
            }
        }
        result.streams.push(stream);
    }
    Ok(Some(result))
}

fn preflight(bytes: &[u8]) -> Result<u16, ImportError> {
    if bytes.len() < 512 {
        return Err(container("truncated CFB header"));
    }
    let word = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let dword = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let major = word(26);
    let sector = match (major, word(30), word(28)) {
        (3, 9, 0xfffe) => 512,
        (4, 12, 0xfffe) => 4096,
        _ => {
            return Err(container(
                "unsupported CFB version, byte order or sector size",
            ));
        }
    };
    if bytes.len() < sector || !bytes.len().is_multiple_of(sector) {
        return Err(container("CFB file is not sector-aligned"));
    }
    let sectors = bytes.len() / sector - 1;
    // Extra, free FAT slots are legal. Bound allocation by a resource budget,
    // not by the minimum number of table sectors implied by the file size.
    if dword(44) > sectors
        || dword(44) > 4 * 1024 * 1024 / sector
        || dword(72) > sectors
        || dword(72) > 1024 * 1024 / sector
    {
        return Err(container(
            "CFB FAT/DIFAT allocation exceeds 4 MiB/1 MiB table budgets",
        ));
    }
    Ok(major)
}

fn role(name: &str) -> BinaryStreamRole {
    if name == "Contents" {
        return BinaryStreamRole::Contents;
    }
    for (prefix, role) in [
        ("Page ", BinaryStreamRole::Scene),
        ("Symbol ", BinaryStreamRole::Symbol),
        ("Media ", BinaryStreamRole::Media),
        ("P ", BinaryStreamRole::Scene),
        ("S ", BinaryStreamRole::Symbol),
        ("M ", BinaryStreamRole::Media),
    ] {
        if let Some(tail) = name.strip_prefix(prefix) {
            // Timestamped CS4 names are hexadecimal identifiers with separators.
            if !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || matches!(b, b' ' | b'-' | b'.'))
            {
                return role;
            }
        }
    }
    BinaryStreamRole::Other
}

fn version(revision: u8) -> Option<&'static str> {
    Some(match revision {
        0x10 => "Flash 1",
        0x11 => "Flash 2",
        0x14 => "Flash 3",
        0x1b => "Flash 4",
        0x20 => "Flash 5",
        0x29 => "Flash MX",
        0x38 => "Flash MX 2004",
        0x3f => "Flash 8",
        0x43 => "Flash CS3",
        0x47 => "Flash CS4",
        _ => return None,
    })
}

pub(crate) fn report(binary: BinaryFlaInspection, report: &mut ImportReport) {
    for stream in &binary.streams {
        if let Some(code) = &stream.stop_code {
            report.error(
                code,
                format!(
                    "CFB stream #{} {:?} @0x{:x}",
                    stream.index, stream.role, stream.bytes_consumed
                ),
                stream
                    .stop_message
                    .as_deref()
                    .unwrap_or("Stream is not fully decoded."),
            );
        }
    }
    report.error("legacy_fla_conversion_unsupported", "CFB document", "Binary FLA structural inspection is implemented; editable conversion is blocked until Contents, all referenced records and their semantics can be preserved. Keep the original file; use XFL export from the authoring application when available.");
    report.binary_fla = Some(binary);
}
