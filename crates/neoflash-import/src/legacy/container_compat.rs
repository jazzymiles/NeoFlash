//! A narrow, immutable-view workaround for legacy writers storing timestamps on
//! stream directory entries. MS-CFB 2.6.1 reserves those fields. No directory
//! links, allocation chains, names, lengths or application bytes are repaired.
//! Noncanonical adjacent-red directory coloring is likewise made neutral in the
//! reading view; bounded name/order/cycle validation remains strict.
//! The caller still opens the resulting view in cfb strict mode and compares all
//! logical streams with the original view before returning any content.
use crate::ImportError;
use std::collections::BTreeSet;

const FREE: u32 = 0xffff_ffff;
const END: u32 = 0xffff_fffe;

fn invalid(message: &str) -> ImportError {
    ImportError::Container(format!("invalid CFB metadata view: {message}"))
}

pub(super) fn normalize_timestamps(bytes: &mut [u8]) -> Result<Vec<usize>, ImportError> {
    let word = |offset| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    let sector_size = match word(26) {
        3 => 512,
        4 => 4096,
        _ => return Err(invalid("version")),
    };
    let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let sector_count = bytes.len() / sector_size - 1;
    let offset = |id: u32| -> Result<usize, ImportError> {
        if id as usize >= sector_count {
            return Err(invalid("sector outside file"));
        }
        Ok((id as usize + 1) * sector_size)
    };
    let fat_count = u32_at(44) as usize;
    let difat_count = u32_at(72) as usize;
    if fat_count > 4 * 1024 * 1024 / sector_size || difat_count > 1024 * 1024 / sector_size {
        return Err(invalid("allocation table budget"));
    }
    let mut fat_ids = Vec::new();
    for i in 0..109 {
        let id = u32_at(76 + i * 4);
        if id != FREE {
            fat_ids.push(id);
        }
    }
    let mut protected = BTreeSet::new();
    let mut difat = u32_at(68);
    for _ in 0..difat_count {
        if !protected.insert(difat) {
            return Err(invalid("DIFAT cycle"));
        }
        let start = offset(difat)?;
        for i in 0..sector_size / 4 - 1 {
            let id = u32_at(start + i * 4);
            if id != FREE {
                fat_ids.push(id);
            }
        }
        difat = u32_at(start + sector_size - 4);
    }
    if difat_count != 0 && difat != END {
        return Err(invalid("DIFAT length"));
    }
    if fat_ids.len() != fat_count {
        return Err(invalid("FAT count"));
    }
    let mut fat = Vec::with_capacity(fat_count * sector_size / 4);
    for id in fat_ids {
        if !protected.insert(id) {
            return Err(invalid("overlapping allocation tables"));
        }
        let start = offset(id)?;
        fat.extend((0..sector_size / 4).map(|i| u32_at(start + i * 4)));
    }
    let mut sectors = Vec::new();
    let mut directory = u32_at(48);
    while directory != END {
        if sectors.len() * (sector_size / 128) >= 10_000 {
            return Err(invalid("directory entry budget"));
        }
        if !protected.insert(directory) {
            return Err(invalid("directory cycle or allocation-table overlap"));
        }
        sectors.push(offset(directory)?);
        directory = *fat
            .get(directory as usize)
            .ok_or_else(|| invalid("directory FAT index"))?;
    }
    let entries: Vec<_> = sectors
        .into_iter()
        .flat_map(|sector| (sector..sector + sector_size).step_by(128))
        .collect();
    let adjacent_red = entries.iter().any(|&entry| {
        matches!(bytes[entry + 66], 1 | 2 | 5)
            && bytes[entry + 67] == 0
            && [68, 72].iter().any(|&field| {
                let id =
                    u32::from_le_bytes(bytes[entry + field..entry + field + 4].try_into().unwrap());
                entries.get(id as usize).is_some_and(|&child| {
                    bytes[child + 67] == 0 && matches!(bytes[child + 66], 1 | 2 | 5)
                })
            })
    });
    let mut changed = Vec::new();
    for entry in entries {
        let kind = bytes[entry + 66];
        let fields: &[usize] = match kind {
            2 => &[100, 108],
            5 => &[100],
            _ => &[],
        };
        for &field in fields {
            let at = entry + field;
            if bytes[at..at + 8].iter().any(|&b| b != 0) {
                bytes[at..at + 8].fill(0);
                changed.push(at);
            }
        }
        if adjacent_red && matches!(kind, 1 | 2 | 5) && bytes[entry + 67] == 0 {
            bytes[entry + 67] = 1;
            changed.push(entry + 67);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    fn sample(version: cfb::Version, storage: bool) -> Vec<u8> {
        let mut file =
            cfb::CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
        for (name, value) in [
            ("A", b"first".as_slice()),
            ("B", b"second".as_slice()),
            ("C", b"third".as_slice()),
        ] {
            file.create_stream(name).unwrap().write_all(value).unwrap();
        }
        if storage {
            file.create_storage("Folder").unwrap();
        }
        file.into_inner().into_inner()
    }
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn directory(bytes: &[u8]) -> (usize, usize) {
        let sector = if bytes[26] == 3 { 512 } else { 4096 };
        ((u32_at(bytes, 48) as usize + 1) * sector, sector)
    }

    #[test]
    fn metadata_view_changes_only_reserved_times_and_adjacent_red_colors() {
        for version in [cfb::Version::V3, cfb::Version::V4] {
            let mut bytes = sample(version, version == cfb::Version::V4);
            assert!(normalize_timestamps(&mut bytes).unwrap().is_empty());
            let (directory, sector) = directory(&bytes);
            let mut expected = Vec::new();
            let mut allowed = BTreeSet::new();
            for entry in (directory..directory + sector).step_by(128) {
                let kind = bytes[entry + 66];
                if kind == 2 {
                    for field in [100, 108] {
                        bytes[entry + field..entry + field + 8]
                            .copy_from_slice(&17_u64.to_le_bytes());
                        expected.push(entry + field);
                        allowed.extend(entry + field..entry + field + 8);
                    }
                    bytes[entry + 67] = 0;
                    expected.push(entry + 67);
                    allowed.insert(entry + 67);
                } else if kind == 5 {
                    bytes[entry + 100..entry + 108].copy_from_slice(&23_u64.to_le_bytes());
                    expected.push(entry + 100);
                    allowed.extend(entry + 100..entry + 108);
                    // Root modification time and storage timestamps are valid.
                    bytes[entry + 108..entry + 116].copy_from_slice(&29_u64.to_le_bytes());
                } else if kind == 1 {
                    if bytes[entry + 67] == 0 {
                        expected.push(entry + 67);
                        allowed.insert(entry + 67);
                    }
                    bytes[entry + 100..entry + 108].copy_from_slice(&31_u64.to_le_bytes());
                    bytes[entry + 108..entry + 116].copy_from_slice(&37_u64.to_le_bytes());
                }
            }
            let original = bytes.clone();
            let mut changed = normalize_timestamps(&mut bytes).unwrap();
            changed.sort_unstable();
            expected.sort_unstable();
            assert_eq!(changed, expected);
            assert!(
                original
                    .iter()
                    .zip(&bytes)
                    .enumerate()
                    .all(|(offset, (a, b))| a == b || allowed.contains(&offset))
            );
            assert!(normalize_timestamps(&mut bytes).unwrap().is_empty());
            let mut opened = cfb::OpenOptions::new()
                .strict()
                .open_with(Cursor::new(bytes))
                .unwrap();
            for (name, expected) in [("A", "first"), ("B", "second"), ("C", "third")] {
                let mut value = String::new();
                opened
                    .open_stream(name)
                    .unwrap()
                    .read_to_string(&mut value)
                    .unwrap();
                assert_eq!(value, expected);
            }
        }
    }

    #[test]
    fn metadata_view_rejects_directory_cycles_overlap_and_out_of_file_sectors() {
        let original = sample(cfb::Version::V3, false);
        let (_, sector) = directory(&original);
        let directory_id = u32_at(&original, 48);
        let fat_id = u32_at(&original, 76);
        let mut cycle = original.clone();
        let cell = (fat_id as usize + 1) * sector + directory_id as usize * 4;
        cycle[cell..cell + 4].copy_from_slice(&directory_id.to_le_bytes());
        assert!(normalize_timestamps(&mut cycle).is_err());
        for id in [fat_id, (original.len() / sector) as u32, FREE] {
            let mut invalid = original.clone();
            invalid[48..52].copy_from_slice(&id.to_le_bytes());
            assert!(normalize_timestamps(&mut invalid).is_err());
        }
    }
}
