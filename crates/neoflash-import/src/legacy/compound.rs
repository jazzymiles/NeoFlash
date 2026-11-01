//! Bounded, read-only CFB (MS-CFB) stream reader for historical FLA writers.
//!
//! Flash wrote compound documents whose *unused* allocation metadata is stale:
//! DIFAT slots beyond the declared FAT count, MiniFAT entries of deleted streams,
//! nonzero stream timestamps and noncanonical red/black colors. Flash itself only
//! follows the chains that live directory entries reference, and so does this
//! reader. It never repairs or rewrites bytes: every logical stream is exactly the
//! sector chain its directory entry names, truncated to the declared length.
//!
//! What is still rejected: sectors outside the file, cycles inside a chain that
//! is actually read, chains shorter than their stream, directory cycles,
//! overlapping FAT/DIFAT/directory sectors, invalid names and budget overruns.
use crate::ImportError;
use std::collections::BTreeSet;

const FREE: u32 = 0xffff_ffff;
const END: u32 = 0xffff_fffe;
const MAX_REGULAR: u32 = 0xffff_fffa;
const MINI_SECTOR: usize = 64;

fn invalid(message: impl Into<String>) -> ImportError {
    ImportError::Container(format!("invalid CFB structure ({})", message.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryKind {
    Storage,
    Stream,
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    /// Slash-separated path from the root, e.g. `/Contents` or `/Folder/Item`.
    pub path: String,
    pub name: String,
    pub kind: EntryKind,
    pub depth: usize,
    pub len: u64,
    start: u32,
}

/// Tolerated deviations from MS-CFB canonical form, reported to the user.
#[derive(Clone, Debug, Default)]
pub(super) struct Tolerances {
    pub stale_difat_slots: bool,
    pub nonzero_stream_times: bool,
    pub noncanonical_colors: bool,
    pub unterminated_chains: bool,
}

impl Tolerances {
    pub fn describe(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.stale_difat_slots {
            out.push("stale DIFAT/FAT slots beyond the declared allocation count");
        }
        if self.nonzero_stream_times {
            out.push("nonzero reserved stream timestamps");
        }
        if self.noncanonical_colors {
            out.push("noncanonical red/black directory colors");
        }
        if self.unterminated_chains {
            out.push("sector chains continuing past their stream length");
        }
        out
    }
}

pub(super) struct Compound<'a> {
    bytes: &'a [u8],
    sector: usize,
    sectors: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    mini_stream: Vec<u8>,
    pub entries: Vec<Entry>,
    pub tolerances: Tolerances,
}

pub(super) struct Limits {
    pub entries: usize,
    pub depth: usize,
    pub stream: u64,
    pub fat_bytes: usize,
    pub difat_bytes: usize,
}

impl<'a> Compound<'a> {
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Self, ImportError> {
        if bytes.len() < 512 || bytes[..8] != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
            return Err(invalid("header signature"));
        }
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let sector = match (u16_at(26), u16_at(28), u16_at(30)) {
            (3, 0xfffe, 9) => 512,
            (4, 0xfffe, 12) => 4096,
            _ => return Err(invalid("version, byte order or sector size")),
        };
        if u16_at(32) != 6 {
            return Err(invalid("mini sector size"));
        }
        if bytes.len() < sector || !bytes.len().is_multiple_of(sector) {
            return Err(invalid("file is not sector-aligned"));
        }
        let sectors = bytes.len() / sector - 1;
        let fat_count = u32_at(44) as usize;
        let difat_count = u32_at(72) as usize;
        if fat_count > sectors
            || fat_count * sector > limits.fat_bytes
            || difat_count > sectors
            || difat_count * sector > limits.difat_bytes
        {
            return Err(invalid("FAT/DIFAT allocation budget"));
        }
        let mut tolerances = Tolerances::default();
        let mut reserved = BTreeSet::new();
        let check = |id: u32, what: &str| -> Result<usize, ImportError> {
            if (id as usize) < sectors {
                Ok(id as usize)
            } else {
                Err(invalid(format!("{what} sector outside file")))
            }
        };
        // The FAT count is authoritative. Writers did not always clear the slots
        // after it, so only the first `fat_count` listed sectors are read.
        let mut fat_ids = Vec::with_capacity(fat_count);
        for i in 0..109 {
            let id = u32_at(76 + i * 4);
            if fat_ids.len() < fat_count {
                if id == FREE {
                    return Err(invalid("missing FAT sector in header DIFAT"));
                }
                fat_ids.push(id);
            } else if id != FREE {
                tolerances.stale_difat_slots = true;
            }
        }
        let mut difat = u32_at(68);
        let mut difat_read = 0;
        while fat_ids.len() < fat_count {
            // Each DIFAT sector adds entries and `reserved` rejects cycles.
            let id = check(difat, "DIFAT")?;
            if !reserved.insert(id) {
                return Err(invalid("DIFAT cycle"));
            }
            difat_read += 1;
            let start = (id + 1) * sector;
            for i in 0..sector / 4 - 1 {
                let fat_id = u32_at(start + i * 4);
                if fat_ids.len() < fat_count {
                    fat_ids.push(fat_id);
                } else if fat_id != FREE {
                    tolerances.stale_difat_slots = true;
                }
            }
            difat = u32_at(start + sector - 4);
        }
        if difat_read < difat_count {
            tolerances.stale_difat_slots = true;
        }
        let mut fat = Vec::with_capacity(fat_count * sector / 4);
        for id in fat_ids {
            let id = check(id, "FAT")?;
            if !reserved.insert(id) {
                return Err(invalid("overlapping allocation tables"));
            }
            let start = (id + 1) * sector;
            fat.extend((0..sector / 4).map(|i| u32_at(start + i * 4)));
        }
        let mut result = Self {
            bytes,
            sector,
            sectors,
            mini_cutoff: u64::from(u32_at(56)),
            fat,
            minifat: Vec::new(),
            mini_stream: Vec::new(),
            entries: Vec::new(),
            tolerances,
        };
        // Directory: an unbounded chain, terminated by END (or FREE from some writers).
        let max_dir_sectors = limits.entries.div_ceil(sector / 128) + 1;
        let mut directory = Vec::new();
        let mut next = u32_at(48);
        while next <= MAX_REGULAR {
            if directory.len() / sector >= max_dir_sectors {
                return Err(invalid("directory entry budget"));
            }
            let id = check(next, "directory")?;
            if !reserved.insert(id) {
                return Err(invalid("directory cycle or allocation-table overlap"));
            }
            directory.extend_from_slice(result.sector_bytes(id));
            next = *result
                .fat
                .get(id)
                .ok_or_else(|| invalid("directory FAT index"))?;
        }
        if directory.is_empty() {
            return Err(invalid("empty directory"));
        }
        let root = raw_entry(&directory, 0)?;
        if root.kind != 5 {
            return Err(invalid("root entry type"));
        }
        // Mini stream container and MiniFAT are only read when present.
        let root_len = if u16_at(26) == 3 {
            root.size & 0xffff_ffff
        } else {
            root.size
        };
        if root_len > 0 {
            result.mini_stream = result.regular(root.start, root_len, limits.stream * 8)?;
        }
        let minifat_sectors = u32_at(64) as usize;
        if minifat_sectors > 0 {
            if minifat_sectors * sector > limits.fat_bytes {
                return Err(invalid("MiniFAT budget"));
            }
            let raw = result.regular(u32_at(60), (minifat_sectors * sector) as u64, u64::MAX)?;
            result.minifat = raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
        }
        result.walk(&directory, root.child, limits, u16_at(26) == 3)?;
        Ok(result)
    }

    fn sector_bytes(&self, id: usize) -> &'a [u8] {
        let start = (id + 1) * self.sector;
        &self.bytes[start..start + self.sector]
    }

    /// Pre-order walk matching the reference `cfb` reader: siblings in in-order
    /// tree order, a storage's contents directly after the storage. Inventory
    /// indices (and therefore imported symbol IDs) depend on this order.
    fn walk(
        &mut self,
        directory: &[u8],
        root_child: u32,
        limits: &Limits,
        v3: bool,
    ) -> Result<(), ImportError> {
        let count = directory.len() / 128;
        let mut seen = BTreeSet::from([0u32]);
        let mut names = BTreeSet::new();
        // (parent path, depth, entry, visit right siblings)
        let mut stack: Vec<(String, usize, u32)> = Vec::new();
        let mut pushes = 0usize;
        let mut spine = |stack: &mut Vec<(String, usize, u32)>,
                         parent: &str,
                         depth: usize,
                         mut id: u32|
         -> Result<(), ImportError> {
            while id != FREE {
                pushes += 1;
                if id as usize >= count || pushes > count {
                    return Err(invalid("loop or link outside directory tree"));
                }
                stack.push((parent.to_owned(), depth, id));
                id = raw_entry(directory, id as usize)?.left;
            }
            Ok(())
        };
        spine(&mut stack, "", 1, root_child)?;
        while let Some((parent, depth, id)) = stack.pop() {
            if !seen.insert(id) {
                return Err(invalid("loop in directory tree"));
            }
            if self.entries.len() >= limits.entries {
                return Err(ImportError::Container(format!(
                    "CFB exceeds {} directory entries",
                    limits.entries
                )));
            }
            let raw = raw_entry(directory, id as usize)?;
            spine(&mut stack, &parent, depth, raw.right)?;
            let kind = match raw.kind {
                1 => EntryKind::Storage,
                2 => EntryKind::Stream,
                // Unallocated entries reachable from stale links carry no content.
                0 => continue,
                _ => return Err(invalid("directory entry type")),
            };
            if raw.color > 1 {
                return Err(invalid("directory entry color"));
            }
            if raw.color == 0 {
                for link in [raw.left, raw.right] {
                    if (link as usize) < count && directory[link as usize * 128 + 67] == 0 {
                        self.tolerances.noncanonical_colors = true;
                    }
                }
            }
            if kind == EntryKind::Stream && directory[id as usize * 128 + 100..][..16] != [0; 16] {
                self.tolerances.nonzero_stream_times = true;
            }
            let path = format!("{parent}/{}", raw.name);
            if !names.insert(path.to_uppercase()) {
                return Err(ImportError::Container(
                    "duplicate CFB directory path".into(),
                ));
            }
            if depth > limits.depth {
                return Err(ImportError::Container(format!(
                    "CFB storage nesting exceeds {}",
                    limits.depth
                )));
            }
            if kind == EntryKind::Storage {
                spine(&mut stack, &path, depth + 1, raw.child)?;
            }
            // Version 3 readers must ignore the size's high word (MS-CFB 2.6.3).
            let len = if v3 { raw.size & 0xffff_ffff } else { raw.size };
            if kind == EntryKind::Stream && len > limits.stream {
                return Err(ImportError::Container(format!(
                    "CFB stream exceeds {} MiB",
                    limits.stream / (1024 * 1024)
                )));
            }
            self.entries.push(Entry {
                path,
                name: raw.name,
                kind,
                depth,
                len: if kind == EntryKind::Stream { len } else { 0 },
                start: raw.start,
            });
        }
        Ok(())
    }

    fn chain(
        &mut self,
        mut next: u32,
        needed: usize,
        mini: bool,
    ) -> Result<Vec<usize>, ImportError> {
        let (table, limit) = if mini {
            (&self.minifat, self.mini_stream.len() / MINI_SECTOR)
        } else {
            (&self.fat, self.sectors)
        };
        let mut ids = Vec::with_capacity(needed);
        let mut seen = BTreeSet::new();
        while ids.len() < needed {
            if next > MAX_REGULAR || next as usize >= limit {
                return Err(invalid("stream chain is truncated or leaves the file"));
            }
            if !seen.insert(next) {
                return Err(invalid("cycle in stream chain"));
            }
            ids.push(next as usize);
            next = *table
                .get(next as usize)
                .ok_or_else(|| invalid("chain entry outside allocation table"))?;
        }
        if next != END && next != FREE {
            self.tolerances.unterminated_chains = true;
        }
        Ok(ids)
    }

    fn regular(&mut self, start: u32, len: u64, cap: u64) -> Result<Vec<u8>, ImportError> {
        if len > cap {
            return Err(invalid("stream budget"));
        }
        let needed = usize::try_from(len.div_ceil(self.sector as u64))
            .map_err(|_| invalid("stream length"))?;
        let ids = self.chain(start, needed, false)?;
        let mut out = Vec::with_capacity(len as usize);
        for id in ids {
            out.extend_from_slice(self.sector_bytes(id));
        }
        out.truncate(len as usize);
        Ok(out)
    }

    /// Read one stream's exact logical bytes.
    pub fn read(&mut self, entry: &Entry) -> Result<Vec<u8>, ImportError> {
        if entry.kind != EntryKind::Stream {
            return Err(invalid("not a stream"));
        }
        if entry.len == 0 {
            return Ok(Vec::new());
        }
        if entry.len < self.mini_cutoff {
            let needed = (entry.len as usize).div_ceil(MINI_SECTOR);
            let ids = self.chain(entry.start, needed, true)?;
            let mut out = Vec::with_capacity(entry.len as usize);
            for id in ids {
                out.extend_from_slice(&self.mini_stream[id * MINI_SECTOR..][..MINI_SECTOR]);
            }
            out.truncate(entry.len as usize);
            Ok(out)
        } else {
            self.regular(entry.start, entry.len, entry.len)
        }
    }
}

struct RawEntry {
    name: String,
    kind: u8,
    color: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

fn raw_entry(directory: &[u8], index: usize) -> Result<RawEntry, ImportError> {
    let e = directory
        .get(index * 128..index * 128 + 128)
        .ok_or_else(|| invalid("directory entry outside directory"))?;
    let u32_at = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
    let name_len = usize::from(u16::from_le_bytes([e[64], e[65]]));
    let kind = e[66];
    let name = if kind == 0 {
        String::new()
    } else {
        if !(2..=64).contains(&name_len) || name_len % 2 != 0 {
            return Err(invalid("directory name length"));
        }
        let units: Vec<u16> = e[..name_len - 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let name = String::from_utf16(&units).map_err(|_| invalid("directory name encoding"))?;
        if name.is_empty() || name.contains(['/', '\\', ':', '!']) {
            return Err(invalid("directory name"));
        }
        name
    };
    Ok(RawEntry {
        name,
        kind,
        color: e[67],
        left: u32_at(68),
        right: u32_at(72),
        child: u32_at(76),
        start: u32_at(116),
        size: u64::from_le_bytes(e[120..128].try_into().unwrap()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const LIMITS: Limits = Limits {
        entries: 10_000,
        depth: 16,
        stream: 8 << 20,
        fat_bytes: 4 << 20,
        difat_bytes: 1 << 20,
    };

    fn sample(version: cfb::Version) -> Vec<u8> {
        let mut file =
            cfb::CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
        file.create_stream("Small")
            .unwrap()
            .write_all(b"mini")
            .unwrap();
        file.create_stream("Large")
            .unwrap()
            .write_all(&(0..9000u32).map(|v| v as u8).collect::<Vec<_>>())
            .unwrap();
        file.create_storage("Folder").unwrap();
        file.create_stream("Folder/Inner")
            .unwrap()
            .write_all(b"x")
            .unwrap();
        file.into_inner().into_inner()
    }

    fn streams(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, ImportError> {
        let mut c = Compound::open(bytes, &LIMITS)?;
        let entries = c.entries.clone();
        entries
            .iter()
            .filter(|e| e.kind == EntryKind::Stream)
            .map(|e| Ok((e.path.clone(), c.read(e)?)))
            .collect()
    }

    #[test]
    fn regular_and_mini_streams_match_reference_writer() {
        for version in [cfb::Version::V3, cfb::Version::V4] {
            let s = streams(&sample(version)).unwrap();
            let names: Vec<_> = s.iter().map(|(p, _)| p.as_str()).collect();
            // CFB name order: shorter names first, then case-insensitive.
            assert_eq!(names, ["/Large", "/Small", "/Folder/Inner"]);
            assert_eq!(s[1].1, b"mini");
            assert_eq!(s[0].1.len(), 9000);
            assert_eq!(s[0].1[8999], (8999u32) as u8);
        }
    }

    #[test]
    fn stale_metadata_outside_live_chains_is_tolerated() {
        let mut bytes = sample(cfb::Version::V3);
        // Stale header DIFAT slot after the declared FAT count.
        bytes[80..84].copy_from_slice(&7u32.to_le_bytes());
        // Nonzero stream timestamps and an all-red directory.
        let dir = (u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize + 1) * 512;
        for e in 0..4 {
            bytes[dir + e * 128 + 67] = 0;
            if bytes[dir + e * 128 + 66] == 2 {
                bytes[dir + e * 128 + 100..dir + e * 128 + 108].fill(9);
            }
        }
        let c = Compound::open(&bytes, &LIMITS).unwrap();
        assert!(c.tolerances.stale_difat_slots);
        assert!(c.tolerances.nonzero_stream_times);
        assert_eq!(
            streams(&bytes).unwrap(),
            streams(&sample(cfb::Version::V3)).unwrap()
        );
    }

    #[test]
    fn cycles_out_of_file_sectors_and_directory_loops_are_rejected() {
        let original = sample(cfb::Version::V3);
        let fat = (u32::from_le_bytes(original[76..80].try_into().unwrap()) as usize + 1) * 512;
        let dir_id = u32::from_le_bytes(original[48..52].try_into().unwrap());
        let mut cycle = original.clone();
        cycle[fat + dir_id as usize * 4..][..4].copy_from_slice(&dir_id.to_le_bytes());
        assert!(Compound::open(&cycle, &LIMITS).is_err());
        let mut outside = original.clone();
        outside[48..52].copy_from_slice(&((original.len() / 512) as u32).to_le_bytes());
        assert!(Compound::open(&outside, &LIMITS).is_err());
        let mut tree = original.clone();
        let dir = (dir_id as usize + 1) * 512;
        let root_child = u32::from_le_bytes(tree[dir + 76..dir + 80].try_into().unwrap());
        let at = dir + root_child as usize * 128 + 68;
        tree[at..at + 4].copy_from_slice(&root_child.to_le_bytes());
        assert!(Compound::open(&tree, &LIMITS).is_err());
        // A live stream whose chain is cut short is an error, not a short read.
        let entry = Compound::open(&original, &LIMITS)
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.name == "Large")
            .unwrap();
        let mut short = original.clone();
        short[fat + entry.start as usize * 4..][..4].copy_from_slice(&END.to_le_bytes());
        let mut c = Compound::open(&short, &LIMITS).unwrap();
        assert!(c.read(&entry).is_err());
    }
}
