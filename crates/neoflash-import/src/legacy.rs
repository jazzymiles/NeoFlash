//! Bounded, read-only CFB and binary FLA structure inspection.
//! No byte-pattern recovery: unknown records stop the current stream.
mod compound;
pub(crate) mod contents;
pub(crate) mod contours;
pub(crate) mod convert;
pub(crate) mod geometry;
pub(crate) mod media;
mod mfc;
pub mod oversized_bundle;
pub(crate) mod timeline;

pub use oversized_bundle::*;

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
                if entry.depth == 1 && validate_ole_property_set_stream(&entry.name, &data) {
                    stream.bytes_consumed = data.len();
                    stream.structure_complete = true;
                } else {
                    stream.stop_code = Some("fla_stream_unsupported".into());
                    stream.stop_message = Some(
                        "Unrecognized stream; no content has been discarded or converted.".into(),
                    );
                }
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
    if name.eq_ignore_ascii_case("Contents") {
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
        if name
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let tail = &name[prefix.len()..];
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
        0x12..=0x14 => "Flash 3",
        0x15..=0x1b => "Flash 4",
        0x1c..=0x20 => "Flash 5",
        0x21..=0x29 => "Flash MX",
        0x2a..=0x38 => "Flash MX 2004",
        0x39..=0x3f => "Flash 8",
        0x40..=0x43 => "Flash CS3",
        0x44..=0x47 => "Flash CS4",
        _ => return None,
    })
}

const FMTID_SUMMARY_INFO: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
const FMTID_DOC_SUMMARY_INFO: [u8; 16] = [
    0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
const FMTID_USER_DEFINED_PROPS: [u8; 16] = [
    0x05, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];

/// Sequentially validate standard root `[MS-OLEPS]` `\x05SummaryInformation` and
/// `\x05DocumentSummaryInformation` property set streams to exact EOF.
fn validate_ole_property_set_stream(name: &str, data: &[u8]) -> bool {
    let is_summary = name == "\u{0005}SummaryInformation";
    let is_doc_summary = name == "\u{0005}DocumentSummaryInformation";
    if (!is_summary && !is_doc_summary) || data.len() < 48 {
        return false;
    }
    let u16_at = |buf: &[u8], off: usize| -> Option<u16> {
        Some(u16::from_le_bytes(buf.get(off..off + 2)?.try_into().ok()?))
    };
    let u32_at = |buf: &[u8], off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
    };
    if u16_at(data, 0) != Some(0xfffe) || u16_at(data, 2).is_none_or(|v| v > 1) {
        return false;
    }
    let Some(num_sets) = u32_at(data, 24).map(|n| n as usize) else {
        return false;
    };
    if (is_summary && num_sets != 1) || (is_doc_summary && !(1..=2).contains(&num_sets)) {
        return false;
    }
    let header_end = 28 + num_sets * 20;
    if data.len() < header_end {
        return false;
    }
    let expected_first_fmtid = if is_summary {
        FMTID_SUMMARY_INFO
    } else {
        FMTID_DOC_SUMMARY_INFO
    };
    if data[28..44] != expected_first_fmtid {
        return false;
    }
    if num_sets == 2 && data[48..64] != FMTID_USER_DEFINED_PROPS {
        return false;
    }

    let mut expected_offset = header_end;
    for s in 0..num_sets {
        let Some(sec_off) = u32_at(data, 28 + s * 20 + 16).map(|n| n as usize) else {
            return false;
        };
        if sec_off != expected_offset {
            return false;
        }
        let Some(cb_section) = u32_at(data, sec_off).map(|n| n as usize) else {
            return false;
        };
        let Some(c_props) = u32_at(data, sec_off + 4).map(|n| n as usize) else {
            return false;
        };
        if cb_section < 8 || c_props > 4096 {
            return false;
        }
        let Some(sec_end) = sec_off.checked_add(cb_section) else {
            return false;
        };
        if sec_end > data.len() {
            return false;
        }
        let sec = &data[sec_off..sec_end];
        let table_end = 8usize.saturating_add(c_props.saturating_mul(8));
        if table_end > sec.len() {
            return false;
        }
        let mut codepage: i16 = 1252;
        for p in 0..c_props {
            let pid = u32_at(sec, 8 + p * 8).unwrap_or(0);
            let poff = u32_at(sec, 8 + p * 8 + 4).unwrap_or(0) as usize;
            if pid == 1
                && poff >= table_end
                && let Some(vt) = u32_at(sec, poff)
                && (vt & 0xffff) == 0x0002
                && let Some(cp) = u16_at(sec, poff + 4)
            {
                codepage = cp as i16;
            }
        }
        for p in 0..c_props {
            let Some(pid) = u32_at(sec, 8 + p * 8) else {
                return false;
            };
            let Some(poff) = u32_at(sec, 8 + p * 8 + 4).map(|n| n as usize) else {
                return false;
            };
            if poff < table_end || poff >= sec.len() {
                return false;
            }
            if !validate_ole_property_entry(pid, &sec[poff..], codepage == 1200) {
                return false;
            }
        }
        expected_offset = sec_end;
    }
    expected_offset <= data.len()
        && data.len() - expected_offset <= 3
        && data[expected_offset..].iter().all(|&b| b == 0)
}

fn validate_ole_typed_value(buf: &[u8], depth: usize) -> Option<usize> {
    if depth > 4 || buf.len() < 4 {
        return None;
    }
    let vt_word = u32::from_le_bytes(buf[..4].try_into().ok()?);
    if (vt_word >> 16) != 0 {
        return None;
    }
    let vt = (vt_word & 0xffff) as u16;
    let body = &buf[4..];
    let align4 = |n: usize| -> Option<usize> { n.checked_add(3).map(|v| v & !3) };
    let used_body = match vt {
        0x0000 | 0x0001 => 0,
        0x0002 | 0x0012 | 0x000b => {
            if body.len() < 2 {
                return None;
            }
            2
        }
        0x0003 | 0x0013 | 0x0004 | 0x000a | 0x0016 | 0x0017 => {
            if body.len() < 4 {
                return None;
            }
            4
        }
        0x0005 | 0x0006 | 0x0007 | 0x0014 | 0x0015 | 0x0040 => {
            if body.len() < 8 {
                return None;
            }
            8
        }
        0x0048 => {
            if body.len() < 16 {
                return None;
            }
            16
        }
        0x001e | 0x0041..=0x0047 => {
            if body.len() < 4 {
                return None;
            }
            let cb = u32::from_le_bytes(body[..4].try_into().ok()?) as usize;
            let total = 4usize.checked_add(cb)?;
            if total > body.len() {
                return None;
            }
            total
        }
        0x001f => {
            if body.len() < 4 {
                return None;
            }
            let cch = u32::from_le_bytes(body[..4].try_into().ok()?) as usize;
            let cb = cch.checked_mul(2)?;
            let total = 4usize.checked_add(cb)?;
            if total > body.len() {
                return None;
            }
            total
        }
        0x101e => {
            if body.len() < 4 {
                return None;
            }
            let count = u32::from_le_bytes(body[..4].try_into().ok()?) as usize;
            if count > 4096 {
                return None;
            }
            let mut pos = 4usize;
            for i in 0..count {
                if pos + 4 > body.len() {
                    return None;
                }
                let cb = u32::from_le_bytes(body[pos..pos + 4].try_into().ok()?) as usize;
                let next = pos.checked_add(4)?.checked_add(cb)?;
                if next > body.len() {
                    return None;
                }
                pos = if i + 1 < count {
                    align4(next)?.min(body.len())
                } else {
                    next
                };
            }
            pos
        }
        0x100c => {
            if body.len() < 4 {
                return None;
            }
            let count = u32::from_le_bytes(body[..4].try_into().ok()?) as usize;
            if count > 4096 {
                return None;
            }
            let mut pos = 4usize;
            for i in 0..count {
                let consumed = validate_ole_typed_value(body.get(pos..)?, depth + 1)?;
                let next = pos.checked_add(consumed)?;
                if next > body.len() {
                    return None;
                }
                pos = if i + 1 < count {
                    align4(next)?.min(body.len())
                } else {
                    next
                };
            }
            pos
        }
        _ => return None,
    };
    4usize.checked_add(used_body)
}

fn validate_ole_property_entry(pid: u32, buf: &[u8], unicode_dict: bool) -> bool {
    if pid == 0 {
        if buf.len() < 4 {
            return false;
        }
        let count = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
        if count > 4096 {
            return false;
        }
        let mut pos = 4usize;
        for _ in 0..count {
            if pos + 8 > buf.len() {
                return false;
            }
            let len = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if len > 65_536 {
                return false;
            }
            let byte_len = if unicode_dict {
                let Some(wbytes) = len.checked_mul(2) else {
                    return false;
                };
                (wbytes + 3) & !3
            } else {
                len
            };
            let Some(next) = pos.checked_add(8).and_then(|v| v.checked_add(byte_len)) else {
                return false;
            };
            if next > buf.len() {
                return false;
            }
            pos = next;
        }
        true
    } else {
        validate_ole_typed_value(buf, 0).is_some()
    }
}

/// Extract `[MS-OLEPS]` `PIDSI_CODEPAGE` (`pid == 1`, `VT_I2`) from a validated root
/// `\x05SummaryInformation` stream when present.
pub(crate) fn ole_summary_codepage(name: &str, data: &[u8]) -> Option<u16> {
    let clean = name.strip_prefix('/').unwrap_or(name);
    if clean != "\u{0005}SummaryInformation" || !validate_ole_property_set_stream(clean, data) {
        return None;
    }
    let u16_at = |buf: &[u8], off: usize| -> Option<u16> {
        Some(u16::from_le_bytes(buf.get(off..off + 2)?.try_into().ok()?))
    };
    let u32_at = |buf: &[u8], off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
    };
    let sec_off = u32_at(data, 44)? as usize;
    let cb_section = u32_at(data, sec_off)? as usize;
    let c_props = u32_at(data, sec_off + 4)? as usize;
    let sec = data.get(sec_off..sec_off.checked_add(cb_section)?)?;
    let table_end = 8usize.checked_add(c_props.checked_mul(8)?)?;
    for p in 0..c_props {
        let pid = u32_at(sec, 8 + p * 8)?;
        let poff = u32_at(sec, 8 + p * 8 + 4)? as usize;
        if pid == 1
            && poff >= table_end
            && let Some(vt) = u32_at(sec, poff)
            && (vt & 0xffff) == 0x0002
        {
            return u16_at(sec, poff + 4);
        }
    }
    None
}

pub(crate) fn fnv1a64_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

const DELTA_MAGIC: &[u8; 4] = b"NFD1";
const DELTA_OP_LITERAL: u8 = 0x00;
const DELTA_OP_RESOURCE_SLICE: u8 = 0x01;
const DELTA_OP_END: u8 = 0xff;
const DELTA_WINDOW: usize = 64;
const DELTA_MIN_MATCH: usize = 128;

const BUZHASH_TABLE: [u64; 256] = {
    let mut table = [0u64; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut z = (i as u64)
            .wrapping_add(1)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        table[i] = z ^ (z >> 31);
        i += 1;
    }
    table
};

#[inline]
fn buzhash_init(window: &[u8]) -> u64 {
    debug_assert_eq!(window.len(), DELTA_WINDOW);
    let mut h = 0u64;
    let mut i = 0usize;
    while i < DELTA_WINDOW {
        h ^= BUZHASH_TABLE[window[i] as usize].rotate_left((DELTA_WINDOW - 1 - i) as u32);
        i += 1;
    }
    h
}

#[derive(Clone, Copy, Default)]
struct DeltaAnchorSlot {
    hash: u64,
    res_idx: u16,
    offset: u32,
    occupied: bool,
}

struct BoundedByteWriter {
    buf: Vec<u8>,
    limit: usize,
}

impl std::io::Write for BoundedByteWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.buf.len().saturating_add(data.len()) > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "exceeded archive budget",
            ));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) type EncodedDeltaArchive = (Vec<u8>, Vec<String>);

pub(crate) fn encode_blobs_delta_gzip(
    original_bytes: &[u8],
    candidates: &[(&str, &[u8])],
    budget: usize,
    require_min_match: bool,
) -> Result<Option<EncodedDeltaArchive>, String> {
    use std::io::Write;
    if original_bytes.is_empty() || budget == 0 {
        return Ok(None);
    }
    if require_min_match && candidates.is_empty() {
        return Ok(None);
    }

    let mut candidate_blobs: Vec<(&str, &[u8])> = Vec::new();
    for &(key, bytes) in candidates {
        if candidate_blobs.len() >= usize::from(u16::MAX) {
            break;
        }
        if bytes.len() >= DELTA_MIN_MATCH && u32::try_from(bytes.len()).is_ok() {
            candidate_blobs.push((key, bytes));
        }
    }
    if require_min_match && candidate_blobs.is_empty() {
        return Ok(None);
    }

    enum TempOp<'a> {
        Literal(&'a [u8]),
        Slice {
            cand_idx: u16,
            offset: u32,
            len: u32,
        },
    }

    let n = original_bytes.len();
    let mut ops: Vec<TempOp<'_>> = Vec::new();
    let mut used_map: std::collections::BTreeMap<u16, u16> = std::collections::BTreeMap::new();
    let mut used_ids: Vec<String> = Vec::new();
    let mut matched_total = 0usize;
    let mut lit_start = 0usize;

    if !candidate_blobs.is_empty() && n >= DELTA_WINDOW {
        let total_anchor_candidates: usize = candidate_blobs
            .iter()
            .map(|(_, b)| b.len() / DELTA_WINDOW)
            .sum();
        let table_len = (total_anchor_candidates.saturating_mul(2))
            .next_power_of_two()
            .clamp(1024, 1 << 20);
        let mask = table_len - 1;
        let mut table = vec![DeltaAnchorSlot::default(); table_len];

        for (ridx, (_, bytes)) in candidate_blobs.iter().enumerate() {
            let mut off = 0usize;
            while off + DELTA_WINDOW <= bytes.len() {
                let win = &bytes[off..off + DELTA_WINDOW];
                let first = win[0];
                if !win.iter().all(|&b| b == first) {
                    let h = buzhash_init(win);
                    let base = (h as usize) & mask;
                    for probe in 0..4 {
                        let idx = (base + probe) & mask;
                        if !table[idx].occupied {
                            table[idx] = DeltaAnchorSlot {
                                hash: h,
                                res_idx: ridx as u16,
                                offset: off as u32,
                                occupied: true,
                            };
                            break;
                        }
                    }
                }
                off += DELTA_WINDOW;
            }
        }

        let mut pos = 0usize;
        let mut h = buzhash_init(&original_bytes[..DELTA_WINDOW]);
        while pos + DELTA_WINDOW <= n {
            let mut best: Option<(u16, usize, usize, usize)> = None;
            let base = (h as usize) & mask;
            for probe in 0..4 {
                let slot = table[(base + probe) & mask];
                if !slot.occupied {
                    break;
                }
                if slot.hash != h {
                    continue;
                }
                let rbytes = candidate_blobs[usize::from(slot.res_idx)].1;
                let roff = slot.offset as usize;
                if original_bytes[pos..pos + DELTA_WINDOW] != rbytes[roff..roff + DELTA_WINDOW] {
                    continue;
                }
                let mut s = pos;
                let mut r0 = roff;
                while s > lit_start && r0 > 0 && original_bytes[s - 1] == rbytes[r0 - 1] {
                    s -= 1;
                    r0 -= 1;
                }
                let mut se = pos + DELTA_WINDOW;
                let mut re = roff + DELTA_WINDOW;
                let max_se = n.min(s.saturating_add(u32::MAX as usize));
                while se < max_se && re < rbytes.len() && original_bytes[se] == rbytes[re] {
                    se += 1;
                    re += 1;
                }
                if se - s >= DELTA_MIN_MATCH
                    && best.is_none_or(|(_, _, bs, bse)| (se - s) > (bse - bs))
                {
                    best = Some((slot.res_idx, r0, s, se));
                }
            }

            if let Some((cand_idx, r0, s, se)) = best {
                if s > lit_start {
                    ops.push(TempOp::Literal(&original_bytes[lit_start..s]));
                }
                if let std::collections::btree_map::Entry::Vacant(e) = used_map.entry(cand_idx) {
                    let dense = used_ids.len() as u16;
                    e.insert(dense);
                    used_ids.push(candidate_blobs[usize::from(cand_idx)].0.to_owned());
                }
                let mlen = se - s;
                ops.push(TempOp::Slice {
                    cand_idx,
                    offset: r0 as u32,
                    len: mlen as u32,
                });
                matched_total += mlen;
                pos = se;
                lit_start = se;
                if pos + DELTA_WINDOW <= n {
                    h = buzhash_init(&original_bytes[pos..pos + DELTA_WINDOW]);
                }
                continue;
            }

            if pos + DELTA_WINDOW < n {
                h = h.rotate_left(1)
                    ^ BUZHASH_TABLE[original_bytes[pos] as usize]
                    ^ BUZHASH_TABLE[original_bytes[pos + DELTA_WINDOW] as usize];
            }
            pos += 1;
        }
    }

    if require_min_match && (matched_total < 64 * 1024 || used_ids.is_empty()) {
        return Ok(None);
    }
    if lit_start < n {
        ops.push(TempOp::Literal(&original_bytes[lit_start..n]));
    }

    let compression = if require_min_match {
        flate2::Compression::default()
    } else {
        flate2::Compression::fast()
    };
    let writer = BoundedByteWriter {
        buf: Vec::with_capacity(budget.min(4 * 1024 * 1024)),
        limit: budget,
    };
    let mut gz = flate2::write::GzEncoder::new(writer, compression);
    let write_res = (|| -> std::io::Result<()> {
        gz.write_all(DELTA_MAGIC)?;
        gz.write_all(&(n as u64).to_le_bytes())?;
        gz.write_all(&(used_ids.len() as u16).to_le_bytes())?;
        for op in &ops {
            match *op {
                TempOp::Literal(mut slice) => {
                    while !slice.is_empty() {
                        let chunk_len = slice.len().min(u32::MAX as usize);
                        gz.write_all(&[DELTA_OP_LITERAL])?;
                        gz.write_all(&(chunk_len as u32).to_le_bytes())?;
                        gz.write_all(&slice[..chunk_len])?;
                        slice = &slice[chunk_len..];
                    }
                }
                TempOp::Slice {
                    cand_idx,
                    offset,
                    len,
                } => {
                    let dense = used_map[&cand_idx];
                    gz.write_all(&[DELTA_OP_RESOURCE_SLICE])?;
                    gz.write_all(&dense.to_le_bytes())?;
                    gz.write_all(&offset.to_le_bytes())?;
                    gz.write_all(&len.to_le_bytes())?;
                }
            }
        }
        gz.write_all(&[DELTA_OP_END])?;
        Ok(())
    })();

    if write_res.is_err() {
        return Ok(None);
    }
    match gz.finish() {
        Ok(bounded) if bounded.buf.len() <= budget => Ok(Some((bounded.buf, used_ids))),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::WriteZero => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Losslessly encode `original_bytes` as a gzipped `NFD1` stream referencing exact byte slices
/// of already-extracted `resources` (`legacy-sound-*`, `legacy-bitmap-*`, `legacy-video-*`, etc.)
/// so media-heavy CFB `.fla` containers are not stored twice against the 64 MiB resource budget.
pub(crate) fn encode_resource_delta_gzip(
    original_bytes: &[u8],
    resources: &[neoflash_core::ResourceUnit],
    budget: usize,
) -> Result<Option<EncodedDeltaArchive>, String> {
    if original_bytes.is_empty() || budget == 0 || resources.is_empty() {
        return Ok(None);
    }
    let mut owned: Vec<(&str, Vec<u8>)> = Vec::new();
    for res in resources {
        if res.id == "fla-provenance"
            || res.path.starts_with("legacy/source.fla")
            || owned.len() >= usize::from(u16::MAX)
        {
            continue;
        }
        if let Ok(decoded) = res.decoded_bytes()
            && decoded.len() >= DELTA_MIN_MATCH
            && u32::try_from(decoded.len()).is_ok()
        {
            owned.push((res.id.as_str(), decoded));
        }
    }
    let slices: Vec<(&str, &[u8])> = owned.iter().map(|(id, b)| (*id, b.as_slice())).collect();
    encode_blobs_delta_gzip(original_bytes, &slices, budget, true)
}

pub(crate) fn decode_nfd1_delta_gzip(
    compressed: &[u8],
    expected_u64: u64,
    ref_blobs: &[&[u8]],
) -> Result<Vec<u8>, ImportError> {
    use std::io::Read;

    if expected_u64 > MAX_CONTAINER {
        return Err(container("declared FLA source_bytes exceeds 512 MiB limit"));
    }
    let expected_len = expected_u64 as usize;
    let max_delta_stream = (expected_u64.saturating_add(16 * 1024 * 1024))
        .min(MAX_CONTAINER.saturating_add(16 * 1024 * 1024));
    let mut dec = flate2::read::GzDecoder::new(compressed).take(max_delta_stream);
    let mut header = [0u8; 14];
    dec.read_exact(&mut header)
        .map_err(|e| container(format!("truncated resource-delta-gzip header: {e}")))?;
    if &header[..4] != DELTA_MAGIC {
        return Err(container("invalid resource-delta-gzip magic"));
    }
    let declared_len = u64::from_le_bytes(header[4..12].try_into().unwrap());
    let declared_refs = u16::from_le_bytes([header[12], header[13]]) as usize;
    if declared_len != expected_u64 || declared_refs != ref_blobs.len() {
        return Err(container(
            "resource-delta-gzip header disagrees with provenance metadata",
        ));
    }

    let mut out = Vec::with_capacity(expected_len.min(16 * 1024 * 1024));
    loop {
        let mut op = [0u8; 1];
        dec.read_exact(&mut op)
            .map_err(|e| container(format!("truncated resource-delta-gzip opcode: {e}")))?;
        match op[0] {
            DELTA_OP_END => {
                let mut extra = [0u8; 1];
                if dec
                    .read(&mut extra)
                    .map_err(|e| container(format!("delta trailer read error: {e}")))?
                    != 0
                {
                    return Err(container(
                        "trailing bytes after resource-delta-gzip EOF marker",
                    ));
                }
                break;
            }
            DELTA_OP_LITERAL => {
                let mut lb = [0u8; 4];
                dec.read_exact(&mut lb)
                    .map_err(|e| container(format!("truncated delta literal length: {e}")))?;
                let len = u32::from_le_bytes(lb) as usize;
                if len == 0 || out.len().checked_add(len).is_none_or(|n| n > expected_len) {
                    return Err(container("invalid or out-of-bounds delta literal"));
                }
                let start = out.len();
                out.resize(start + len, 0);
                dec.read_exact(&mut out[start..])
                    .map_err(|e| container(format!("truncated delta literal payload: {e}")))?;
            }
            DELTA_OP_RESOURCE_SLICE => {
                let mut sb = [0u8; 10];
                dec.read_exact(&mut sb).map_err(|e| {
                    container(format!("truncated delta resource slice header: {e}"))
                })?;
                let ridx = u16::from_le_bytes([sb[0], sb[1]]) as usize;
                let roff = u32::from_le_bytes(sb[2..6].try_into().unwrap()) as usize;
                let len = u32::from_le_bytes(sb[6..10].try_into().unwrap()) as usize;
                let Some(src_blob) = ref_blobs.get(ridx) else {
                    return Err(container("delta resource index out of bounds"));
                };
                let Some(slice) = roff
                    .checked_add(len)
                    .and_then(|end| src_blob.get(roff..end))
                else {
                    return Err(container("delta resource slice out of bounds"));
                };
                if len == 0 || out.len().checked_add(len).is_none_or(|n| n > expected_len) {
                    return Err(container("invalid or out-of-bounds delta slice copy"));
                }
                out.extend_from_slice(slice);
            }
            other => {
                return Err(container(format!(
                    "unknown resource-delta-gzip opcode 0x{other:02x}"
                )));
            }
        }
    }
    Ok(out)
}

/// Reconstruct the byte-exact original binary `.fla` archive from an imported `Project`'s
/// `fla-provenance` metadata (supporting Schema 2 raw parts and Schema 3 `"gzip"` or
/// `"resource-delta-gzip"` streams), verifying exact byte count, FNV-1a, and SHA-256 digests.
/// Returns `Ok(None)` when the project has no binary FLA provenance or when its source archive
/// was recorded as `"external"` (`fla_source_not_embedded`).
pub fn reconstruct_legacy_fla_bytes(
    project: &neoflash_core::Project,
) -> Result<Option<Vec<u8>>, ImportError> {
    use std::io::Read;

    let Some(prov_res) = project
        .resources
        .iter()
        .find(|r| r.id == "fla-provenance" || r.path == "legacy/provenance.json")
    else {
        return Ok(None);
    };
    let prov_bytes = prov_res
        .decoded_bytes()
        .map_err(|e| container(format!("invalid fla-provenance resource: {e}")))?;
    let meta: serde_json::Value = serde_json::from_slice(&prov_bytes)
        .map_err(|e| container(format!("invalid fla-provenance JSON: {e}")))?;

    let expected_u64 = meta
        .get("source_bytes")
        .or_else(|| meta.get("source_byte_len"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| container("fla-provenance missing source_bytes"))?;
    if expected_u64 > MAX_CONTAINER {
        return Err(container("declared FLA source_bytes exceeds 512 MiB limit"));
    }
    let expected_len = expected_u64 as usize;
    let expected_fnv = meta
        .get("source_fnv1a64")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| container("fla-provenance missing source_fnv1a64"))?;
    let expected_sha256 = meta
        .get("source_sha256")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            meta.get("source_archive")
                .and_then(|a| a.get("source_sha256"))
                .and_then(serde_json::Value::as_str)
        });

    let raw_ids: Vec<&str> = meta
        .get("source_resources")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();

    let restored = if !raw_ids.is_empty() {
        let mut out = Vec::with_capacity(expected_len.min(16 * 1024 * 1024));
        for rid in raw_ids {
            let unit = project
                .resources
                .iter()
                .find(|r| r.id == rid)
                .ok_or_else(|| container(format!("missing FLA source resource {rid}")))?;
            let chunk = unit
                .decoded_bytes()
                .map_err(|e| container(format!("corrupt FLA source resource {rid}: {e}")))?;
            if out
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > expected_len || n as u64 > MAX_CONTAINER)
            {
                return Err(container(
                    "raw FLA source parts exceed declared source_bytes",
                ));
            }
            out.extend_from_slice(&chunk);
        }
        out
    } else if let Some(archive) = meta.get("source_archive") {
        let encoding = archive
            .get("encoding")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if encoding == "external" {
            return Ok(None);
        }
        let part_ids: Vec<&str> = archive
            .get("resources")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| container("source_archive missing resources array"))?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        if part_ids.is_empty() {
            return Ok(None);
        }
        let mut compressed = Vec::new();
        for rid in part_ids {
            let unit = project
                .resources
                .iter()
                .find(|r| r.id == rid)
                .ok_or_else(|| container(format!("missing FLA archive part {rid}")))?;
            let chunk = unit
                .decoded_bytes()
                .map_err(|e| container(format!("corrupt FLA archive part {rid}: {e}")))?;
            if compressed
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > neoflash_core::MAX_RESOURCE_TOTAL_BYTES)
            {
                return Err(container(
                    "compressed FLA archive exceeds 64 MiB resource limit",
                ));
            }
            compressed.extend_from_slice(&chunk);
        }

        match encoding {
            "gzip" => {
                let mut out = Vec::with_capacity(expected_len.min(16 * 1024 * 1024));
                flate2::read::GzDecoder::new(compressed.as_slice())
                    .take(expected_u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| container(format!("invalid gzip FLA source archive: {e}")))?;
                out
            }
            "resource-delta-gzip" => {
                let ref_ids: Vec<&str> = archive
                    .get("reference_resources")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| container("resource-delta-gzip missing reference_resources"))?
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect();
                if ref_ids.len() > neoflash_core::MAX_RESOURCES {
                    return Err(container("reference_resources exceeds resource limit"));
                }
                let mut ref_blobs: Vec<Vec<u8>> = Vec::with_capacity(ref_ids.len());
                for rid in ref_ids {
                    let unit = project
                        .resources
                        .iter()
                        .find(|r| r.id == rid)
                        .ok_or_else(|| {
                            container(format!("missing delta reference resource {rid}"))
                        })?;
                    let bytes = unit.decoded_bytes().map_err(|e| {
                        container(format!("corrupt delta reference resource {rid}: {e}"))
                    })?;
                    ref_blobs.push(bytes);
                }

                let ref_slices: Vec<&[u8]> = ref_blobs.iter().map(Vec::as_slice).collect();
                decode_nfd1_delta_gzip(&compressed, expected_u64, &ref_slices)?
            }
            other => {
                return Err(container(format!(
                    "unsupported FLA source_archive encoding {other:?}"
                )));
            }
        }
    } else {
        return Ok(None);
    };

    if restored.len() != expected_len {
        return Err(container(format!(
            "reconstructed FLA length {} does not match expected {}",
            restored.len(),
            expected_len
        )));
    }
    let actual_fnv = format!("{:016x}", fnv1a64_bytes(&restored));
    if actual_fnv != expected_fnv {
        return Err(container(
            "reconstructed FLA FNV-1a digest does not match provenance",
        ));
    }
    if let Some(exp_sha) = expected_sha256 {
        let actual_sha = neoflash_core::extensions::sha256_hex(&restored);
        if !actual_sha.eq_ignore_ascii_case(exp_sha) {
            return Err(container(
                "reconstructed FLA SHA-256 digest does not match provenance",
            ));
        }
    }
    Ok(Some(restored))
}
