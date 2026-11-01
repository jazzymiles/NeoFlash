//! Bounded, read-only CFB and binary FLA structure inspection.
//! No byte-pattern recovery: unknown records stop the current stream.
mod compound;
pub(crate) mod contents;
pub(crate) mod contours;
pub(crate) mod convert;
pub(crate) mod geometry;
pub(crate) mod media;
mod mfc;
pub(crate) mod timeline;

use crate::ImportError;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

const MAX_CONTAINER: u64 = 512 * 1024 * 1024;
const MAX_STREAM: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const LIMITS: compound::Limits = compound::Limits {
    entries: MAX_ENTRIES,
    depth: 16,
    stream: MAX_STREAM,
    fat_bytes: 16 * 1024 * 1024,
    difat_bytes: 1024 * 1024,
};

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
    #[serde(default)]
    pub geometry: geometry::GeometryCounts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<contents::ContentsCounts>,
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
    /// Tolerated non-canonical CFB metadata outside the live stream chains
    /// (stale allocation slots, reserved timestamps, directory colors). Stream
    /// bytes are read unchanged from the chains their directory entries name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub container_tolerances: Vec<String>,
}

/// Explicit opt-in preservation API. Contains private artwork; unlike inspect(),
/// callers must not publish this data without the source owner's authorization.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryGeometryDocument {
    pub inspection: BinaryFlaInspection,
    pub streams: Vec<BinaryStreamGeometry>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryStreamGeometry {
    pub index: usize,
    /// Only complete, validated shape blocks before the stream's first stop.
    /// The enclosing object can still be incomplete; see inspection.streams.
    pub shapes: Vec<geometry::LocatedBinaryShape>,
}

/// Read complete shape blocks as preservation IR without making a Project.
/// Unknown records stop their stream. This does not assemble contour topology or
/// resolve scene/layer/library ownership. Contents is separately available through
/// read_binary_contents; this API does not retain its private payload.
pub fn read_binary_geometry(path: &Path) -> Result<BinaryGeometryDocument, ImportError> {
    let document = read(path, true, false, false)?
        .ok_or_else(|| container("compound document is not recognized as binary FLA"))?;
    Ok(BinaryGeometryDocument {
        inspection: document.inspection,
        streams: document.geometry,
    })
}

fn container(message: impl Into<String>) -> ImportError {
    ImportError::Container(message.into())
}

/// Explicit opt-in Contents IR. Contains private names/metadata and exact unread bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryContentsDocument {
    pub inspection: BinaryFlaInspection,
    pub contents: contents::BinaryContentsPrefix,
}

pub fn read_binary_contents(path: &Path) -> Result<BinaryContentsDocument, ImportError> {
    let document = read(path, false, true, false)?
        .ok_or_else(|| container("compound document is not recognized as binary FLA"))?;
    Ok(BinaryContentsDocument {
        inspection: document.inspection,
        contents: document
            .contents
            .ok_or_else(|| container("Contents stream is absent"))?,
    })
}

/// Contents and timeline preservation from one immutable container snapshot.
/// Contains private source names, scripts, raw bytes and artwork. No conversion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryTimelineDocument {
    pub inspection: BinaryFlaInspection,
    pub contents: contents::BinaryContentsPrefix,
    pub streams: Vec<timeline::BinaryTimelineStream>,
    #[serde(default)]
    pub raw_streams: Vec<BinaryRawStream>,
    #[serde(default)]
    pub original_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryRawStream {
    pub index: usize,
    pub name: String,
    pub role: BinaryStreamRole,
    pub bytes: Vec<u8>,
}

pub fn read_binary_timeline(path: &Path) -> Result<BinaryTimelineDocument, ImportError> {
    read_for_import(path)?
        .ok_or_else(|| container("compound document is not recognized as binary FLA"))
}

pub(crate) fn read_for_import(path: &Path) -> Result<Option<BinaryTimelineDocument>, ImportError> {
    let Some(document) = read(path, false, true, true)? else {
        return Ok(None);
    };
    Ok(Some(BinaryTimelineDocument {
        inspection: document.inspection,
        contents: document
            .contents
            .ok_or_else(|| container("Contents stream is absent"))?,
        streams: document.timeline,
        raw_streams: document.raw_streams,
        original_bytes: document.original_bytes,
    }))
}

struct ReadDocument {
    inspection: BinaryFlaInspection,
    geometry: Vec<BinaryStreamGeometry>,
    timeline: Vec<timeline::BinaryTimelineStream>,
    contents: Option<contents::BinaryContentsPrefix>,
    raw_streams: Vec<BinaryRawStream>,
    original_bytes: Vec<u8>,
}

fn read(
    path: &Path,
    retain_geometry: bool,
    retain_contents: bool,
    retain_timeline: bool,
) -> Result<Option<ReadDocument>, ImportError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_CONTAINER {
        return Err(container("CFB exceeds 512 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONTAINER + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTAINER {
        return Err(container("CFB grew beyond 512 MiB"));
    }
    let major = preflight(&bytes)?;
    // The snapshot bounds allocation and makes concurrent source changes harmless.
    let mut compound = compound::Compound::open(&bytes, &LIMITS)?;
    let mut entries = Vec::new();
    // The root entry counts as a storage, as in the reference reader.
    let mut storage_count = 1;
    let mut logical_bytes = 0u64;
    for entry in compound.entries.clone() {
        if entry.kind == compound::EntryKind::Storage {
            storage_count += 1;
            continue;
        }
        logical_bytes = logical_bytes
            .checked_add(entry.len)
            .ok_or_else(|| container("CFB logical size overflow"))?;
        if logical_bytes > MAX_CONTAINER {
            return Err(container("CFB logical streams exceed 512 MiB"));
        }
        let role = if entry.depth == 1 {
            role(&entry.name)
        } else {
            BinaryStreamRole::Other
        };
        entries.push((entry, role));
    }
    let has_contents = entries.iter().any(|e| e.1 == BinaryStreamRole::Contents);
    let has_timeline = entries
        .iter()
        .any(|e| matches!(e.1, BinaryStreamRole::Scene | BinaryStreamRole::Symbol));
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
        container_tolerances: compound
            .tolerances
            .describe()
            .into_iter()
            .map(str::to_owned)
            .collect(),
    };
    let inventory: contents::Inventory = entries
        .iter()
        .enumerate()
        .filter(|(_, (entry, _))| entry.depth == 1)
        .map(|(index, (entry, role))| {
            (
                entry.name.to_ascii_uppercase(),
                contents::InventoryEntry { index, role: *role },
            )
        })
        .collect();
    let mut contents_prefix = None;
    let mut geometry_streams = Vec::new();
    let mut retained_items_remaining = mfc::DOCUMENT_GEOMETRY_ITEMS;
    let mut timeline_streams = Vec::new();
    let mut timeline_items_remaining = mfc::DOCUMENT_TIMELINE_ITEMS;
    let mut raw_streams = Vec::new();
    for (index, (entry, role)) in entries.into_iter().enumerate() {
        let data = compound.read(&entry)?;
        let byte_length = entry.len;
        if data.len() as u64 != byte_length {
            return Err(container("CFB stream length mismatch"));
        }
        if retain_timeline {
            raw_streams.push(BinaryRawStream {
                index,
                name: entry.path.clone(),
                role,
                bytes: data.clone(),
            });
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
            geometry: geometry::GeometryCounts::default(),
            contents: None,
            stop_code: None,
            stop_message: None,
        };
        match role {
            BinaryStreamRole::Contents => {
                result.contents_revision = data.first().copied();
                result.authoring_version = result
                    .contents_revision
                    .and_then(version)
                    .map(str::to_owned);
                let prefix = contents::inspect(&data, &mut stream, &inventory, retain_contents);
                if retain_contents {
                    contents_prefix = Some(prefix);
                }
            }
            BinaryStreamRole::Scene | BinaryStreamRole::Symbol => {
                let decoded = mfc::inspect(
                    &data,
                    &mut stream,
                    retain_geometry,
                    retain_timeline,
                    &mut retained_items_remaining,
                    &mut timeline_items_remaining,
                );
                if !decoded.shapes.is_empty() {
                    geometry_streams.push(BinaryStreamGeometry {
                        index,
                        shapes: decoded.shapes,
                    });
                }
                if let Some(timeline) = decoded.timeline {
                    timeline_streams.push(timeline);
                }
            }
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
    Ok(Some(ReadDocument {
        inspection: result,
        geometry: geometry_streams,
        timeline: timeline_streams,
        contents: contents_prefix,
        raw_streams,
        original_bytes: if retain_timeline { bytes } else { Vec::new() },
    }))
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
        || dword(44) * sector > LIMITS.fat_bytes
        || dword(72) > sectors
        || dword(72) * sector > LIMITS.difat_bytes
    {
        return Err(container(
            "CFB FAT/DIFAT allocation exceeds 16 MiB/1 MiB table budgets",
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
