use super::*;
use crate::legacy::contents::{BinaryContentsPrefix, ContentsDescriptorClass};
use crate::legacy::media::decode_binary_bitmap;
use neoflash_core::{MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ResourceUnit, valid_library_folder};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct BitmapAsset {
    pub resource_id: String,
    pub width: f64,
    pub height: f64,
    pub smoothing: bool,
}

/// Rebuild library paths from archive identities, never filesystem paths.
type FolderPaths = (Vec<String>, BTreeMap<[u32; 2], String>);
pub(super) fn folders(contents: &BinaryContentsPrefix) -> Result<FolderPaths, String> {
    let entries = contents
        .extensions
        .as_ref()
        .map(|e| e.folders.as_slice())
        .unwrap_or(&[]);
    if entries.len() > 10_000 {
        return Err("The library has more than 10000 folders.".into());
    }
    let mut identities = BTreeMap::new();
    let mut names = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let name = contents.string_text(&entry.name).ok_or("A library-folder name has an unknown ANSI encoding or invalid UTF-16; its raw bytes are preserved.")?;
        if name.is_empty() || name.contains('/') || !valid_library_folder(&name) {
            return Err(
                "A library-folder name cannot be represented as a normalized NeoFlash folder."
                    .into(),
            );
        }
        names.push(name);
        if let Some(id) = entry.metadata.item.filter(|id| *id != [0, 0])
            && identities.insert(id, index).is_some()
        {
            return Err("Library folders have a duplicate archive identity.".into());
        }
    }
    let mut paths: Vec<Option<String>> = vec![None; entries.len()];
    for index in 0..entries.len() {
        let mut chain = Vec::new();
        let mut current = index;
        let mut base;
        loop {
            if let Some(path) = &paths[current] {
                base = path.clone();
                break;
            }
            if chain.contains(&current) {
                return Err("Library-folder parents contain a cycle.".into());
            }
            if chain.len() >= 256 {
                return Err("Library-folder nesting exceeds 256.".into());
            }
            chain.push(current);
            match entries[current].metadata.parent.filter(|id| *id != [0, 0]) {
                None => {
                    base = String::new();
                    break;
                }
                Some(parent) => {
                    current = *identities
                        .get(&parent)
                        .ok_or("A library folder refers to an absent parent identity.")?
                }
            }
        }
        for item in chain.into_iter().rev() {
            if !base.is_empty() {
                base.push('/');
            }
            base.push_str(&names[item]);
            if !valid_library_folder(&base) {
                return Err(
                    "A library-folder path exceeds NeoFlash's normalized path limits.".into(),
                );
            }
            paths[item] = Some(base.clone());
        }
    }
    let paths: Vec<String> = paths.into_iter().map(Option::unwrap).collect();
    let mut unique = std::collections::BTreeSet::new();
    if paths.iter().any(|path| !unique.insert(path)) {
        return Err("Distinct archive folders have the same NeoFlash path.".into());
    }
    let by_id = identities
        .into_iter()
        .map(|(id, index)| (id, paths[index].clone()))
        .collect();
    Ok((paths, by_id))
}

pub(super) fn resources(
    document: &BinaryTimelineDocument,
    report: &mut ImportReport,
) -> Result<(Vec<ResourceUnit>, BTreeMap<u16, BitmapAsset>), String> {
    let (_, folders) = folders(&document.contents)?;
    let mut resources = Vec::new();
    let mut bitmaps = BTreeMap::new();
    let mut total = 0usize;
    let mut streams = BTreeMap::new();
    let mut decoded_streams = std::collections::BTreeSet::new();
    for stream in &document.raw_streams {
        if streams.insert(stream.index, stream).is_some() {
            return Err("Raw archive streams have duplicate identities.".into());
        }
    }
    for descriptor in &document.contents.descriptors {
        if descriptor.media_id.is_none() {
            continue;
        }
        if descriptor.class != ContentsDescriptorClass::Bitmap {
            return Err("The binary library contains sound, video, or another media class whose native payload conversion is not supported yet; source bytes are preserved.".into());
        }
        if resources.len() >= MAX_RESOURCES {
            return Err("The bitmap library exceeds the project resource limit.".into());
        }
        let id = descriptor.media_id.unwrap();
        let settings = descriptor
            .bitmap
            .as_ref()
            .ok_or("A bitmap descriptor has no typed bitmap settings.")?;
        let stream = streams
            .get(
                &descriptor
                    .stream_index
                    .ok_or("A bitmap descriptor has no resolved stream identity.")?,
            )
            .ok_or("The resolved bitmap stream bytes are absent.")?;
        if stream.role != crate::legacy::BinaryStreamRole::Media
            || !stream
                .name
                .strip_prefix('/')
                .unwrap_or(&stream.name)
                .eq_ignore_ascii_case(&descriptor.stream_name)
        {
            return Err("The bitmap descriptor and immutable media stream disagree.".into());
        }
        let decoded = decode_binary_bitmap(&stream.bytes, settings.encoded_byte_count)
            .map_err(|e| e.to_string())?;
        if decoded.original_bytes != stream.bytes
            || decoded.bounds_words[0] != 0
            || decoded.bounds_words[2] != 0
        {
            return Err("Bitmap source identity or nonzero media bounds origin requires an explicit adapter.".into());
        }
        let png = decoded.png_bytes().map_err(|e| e.to_string())?;
        total = total
            .checked_add(png.len())
            .filter(|n| *n <= MAX_RESOURCE_TOTAL_BYTES)
            .ok_or("Decoded bitmap resources exceed the project's 64 MiB byte limit.")?;
        let resource_id = format!("legacy-bitmap-{id}");
        let fallback = format!("legacy/bitmaps/{id}/bitmap.png");
        let parent = descriptor
            .library_metadata
            .as_ref()
            .and_then(|m| m.parent)
            .filter(|id| *id != [0, 0]);
        let folder = parent
            .map(|id| {
                folders
                    .get(&id)
                    .ok_or("A bitmap refers to an absent library-folder identity.")
            })
            .transpose()?;
        let name = document.contents.string_text(&descriptor.name);
        let path = name.as_deref().map(|name| {
            let name = if let Some(folder) = folder {
                format!("{folder}/{name}")
            } else {
                name.to_owned()
            };
            let suffix = if name.to_ascii_lowercase().ends_with(".png") {
                ""
            } else {
                ".png"
            };
            format!("legacy/bitmaps/{id}/{name}{suffix}")
        });
        let resource = path
            .and_then(|path| ResourceUnit::from_bytes(&resource_id, path, "image/png", &png).ok());
        let resource = match resource {
            Some(resource) => resource,
            None => {
                report.warning("fla_bitmap_resource_name", format!("media {id}"), "The original bitmap name cannot be used as a portable resource path. A stable identity path is used; the original name remains in the embedded Contents provenance.");
                ResourceUnit::from_bytes(&resource_id, fallback, "image/png", &png)
                    .map_err(|e| e.to_string())?
            }
        };
        if bitmaps
            .insert(
                id,
                BitmapAsset {
                    resource_id,
                    width: f64::from(decoded.width),
                    height: f64::from(decoded.height),
                    smoothing: settings.smoothing,
                },
            )
            .is_some()
        {
            return Err("Bitmap descriptors have duplicate media identifiers.".into());
        }
        decoded_streams.insert(stream.index);
        resources.push(resource);
    }
    if document.raw_streams.iter().any(|stream| {
        stream.role == crate::legacy::BinaryStreamRole::Media
            && !decoded_streams.contains(&stream.index)
    }) {
        return Err(
            "An unreferenced media stream has no descriptor; its format cannot be inferred safely."
                .into(),
        );
    }
    Ok((resources, bitmaps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy::contents::{
        ContentsByteSpan, ContentsExtensions, ContentsFolder, ContentsLibraryMetadata,
        ContentsString,
    };

    fn contents() -> BinaryContentsPrefix {
        let mut contents: BinaryContentsPrefix = serde_json::from_value(serde_json::json!({
            "revision": 71, "compatibility_marker": null, "ole_count": 0,
            "next_scene_id": null, "next_item_tag": null, "library_counter": null,
            "media_counter": null, "scenes": [], "library": [], "media": [],
            "selected_scene": null, "references": [], "descriptors": [], "settings": null,
            "bytes_consumed": 0, "stop": null, "original_bytes": []
        }))
        .unwrap();
        let mut entries = Vec::new();
        for (name, item, parent) in [("Child", [1, 2], [1, 1]), ("Parent", [1, 1], [0, 0])] {
            let start = contents.original_bytes.len();
            contents.original_bytes.extend(name.as_bytes());
            let span = ContentsByteSpan {
                start,
                end: contents.original_bytes.len(),
            };
            entries.push(ContentsFolder {
                name: ContentsString {
                    encoded: span,
                    data: span,
                    utf16: false,
                },
                metadata: ContentsLibraryMetadata {
                    item: Some(item),
                    parent: Some(parent),
                    ..Default::default()
                },
                encoded: span,
            });
        }
        contents.extensions = Some(ContentsExtensions {
            folders: entries,
            ..Default::default()
        });
        contents
    }

    #[test]
    fn folder_identity_family_handles_forward_parents_and_rejects_ambiguous_graphs() {
        let original = contents();
        let (paths, identities) = folders(&original).unwrap();
        assert_eq!(paths, ["Parent/Child", "Parent"]);
        assert_eq!(
            identities.get(&[1, 2]).map(String::as_str),
            Some("Parent/Child")
        );
        for (parent, item) in [
            (Some([1, 2]), Some([1, 1])),
            (Some([8, 8]), Some([1, 1])),
            (Some([0, 0]), Some([1, 2])),
        ] {
            let mut invalid = original.clone();
            let entry = &mut invalid.extensions.as_mut().unwrap().folders[1];
            entry.metadata.parent = parent;
            entry.metadata.item = item;
            assert!(folders(&invalid).is_err());
        }
        let mut unsupported = original;
        unsupported.original_bytes[0] = b'/';
        assert!(folders(&unsupported).is_err());
    }
}
