use super::*;
use crate::legacy::contents::{BinaryContentsPrefix, ContentsDescriptorClass};
use crate::legacy::media::{
    BinaryVideoPayload, decode_binary_bitmap, decode_binary_sound, decode_binary_video,
    resize_rgba_proportional,
};
use image::{
    ExtendedColorType, ImageEncoder,
    codecs::jpeg::JpegEncoder,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use neoflash_core::{MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ResourceUnit, valid_library_folder};
use std::collections::{BTreeMap, BTreeSet};

/// Budget kept free for the provenance record within the project's resources.
const PROVENANCE_RESERVE: usize = 512 * 1024;

#[derive(Clone, Debug)]
pub(super) struct BitmapAsset {
    pub resource_id: String,
    pub width: f64,
    pub height: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub origin_x: f64,
    pub origin_y: f64,
    pub bounds_scale_x: f64,
    pub bounds_scale_y: f64,
    pub smoothing: bool,
    pub average_color: Color,
}

#[derive(Clone, Debug)]
pub(super) struct SoundAsset {
    pub resource_id: String,
    pub duration_44: u32,
}

#[derive(Clone, Debug)]
pub(super) struct VideoAsset {
    pub resource_id: Option<String>,
    pub width: f64,
    pub height: f64,
    pub sync_video: bool,
    pub import_audio: bool,
}

#[derive(Clone, Debug)]
pub(super) struct CompiledClipAsset {
    pub reconstructed: Option<super::compiled_clip::ReconstructedSwfClip>,
    pub icon_bitmap: Option<BitmapAsset>,
    pub width: f64,
    pub height: f64,
}

pub(super) struct ExtractedAssets {
    pub resources: Vec<ResourceUnit>,
    pub bitmaps: BTreeMap<u16, BitmapAsset>,
    pub sounds: BTreeMap<u16, SoundAsset>,
    pub videos: BTreeMap<u16, VideoAsset>,
    pub compiled_clips: BTreeMap<u16, CompiledClipAsset>,
    pub skipped_bitmaps: BTreeSet<u16>,
}

fn remap_compiled_clip_resources(
    clip: &mut super::compiled_clip::ReconstructedSwfClip,
    remap: &BTreeMap<String, String>,
) {
    for sym in std::iter::once(&mut clip.main_symbol).chain(clip.helper_symbols.iter_mut()) {
        for kf in sym
            .timeline
            .layers
            .iter_mut()
            .flat_map(|l| &mut l.keyframes)
        {
            if let Some(snd) = &mut kf.sound
                && let Some(target) = remap.get(&snd.resource)
            {
                snd.resource = target.clone();
            }
            if let Some(vid) = &mut kf.video
                && let Some(target) = remap.get(&vid.resource)
            {
                vid.resource = target.clone();
            }
            for bf in &mut kf.bitmap_fills {
                if let Some(target) = remap.get(&bf.resource_id) {
                    bf.resource_id = target.clone();
                }
            }
            for el in &mut kf.elements {
                if let ElementKind::Bitmap { resource_id, .. } = &mut el.kind
                    && let Some(target) = remap.get(resource_id.as_str())
                {
                    *resource_id = target.clone();
                }
            }
        }
    }
}

pub(super) fn compact_duplicate_media_under_pressure(
    resources: &mut Vec<ResourceUnit>,
    bitmaps: &mut BTreeMap<u16, BitmapAsset>,
    sounds: &mut BTreeMap<u16, SoundAsset>,
    videos: &mut BTreeMap<u16, VideoAsset>,
    compiled_clips: &mut BTreeMap<u16, CompiledClipAsset>,
    used_paths: &mut BTreeSet<String>,
    total: &mut usize,
) {
    // Never merge a linked sound (`!id.starts_with("legacy-")`) into another sound.
    let mut canonical: Vec<(usize, String, String, std::sync::Arc<str>)> = Vec::new();
    let mut remap: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::with_capacity(resources.len());
    for res in resources.drain(..) {
        let is_linked_sound = res.is_audio() && !res.id.starts_with("legacy-");
        if let Some((canon_idx, canon_id, _, _)) = canonical.iter_mut().find(|(_, _, mt, b64)| {
            mt == &res.media_type && b64.as_ref() == res.data_base64.as_ref()
        }) {
            let canon_is_linked_sound = res.is_audio() && !canon_id.starts_with("legacy-");
            if is_linked_sound && canon_is_linked_sound {
                // Distinct linked sounds must keep both linkage identifiers intact.
                let idx = out.len();
                canonical.push((
                    idx,
                    res.id.clone(),
                    res.media_type.clone(),
                    res.data_base64.clone(),
                ));
                out.push(res);
                continue;
            }
            if is_linked_sound && !canon_is_linked_sound {
                // Upgrade the earlier unlinked sound resource to carry this sound's linkage ID.
                let old_unlinked_id = canon_id.clone();
                *canon_id = res.id.clone();
                out[*canon_idx].id = res.id.clone();
                remap.insert(old_unlinked_id, res.id.clone());
                *total = total.saturating_sub(res.byte_len());
                used_paths.remove(&res.path.to_ascii_lowercase());
                continue;
            }
            *total = total.saturating_sub(res.byte_len());
            used_paths.remove(&res.path.to_ascii_lowercase());
            remap.insert(res.id.clone(), canon_id.clone());
        } else {
            let idx = out.len();
            canonical.push((
                idx,
                res.id.clone(),
                res.media_type.clone(),
                res.data_base64.clone(),
            ));
            out.push(res);
        }
    }
    *resources = out;
    if !remap.is_empty() {
        for asset in bitmaps.values_mut() {
            if let Some(target) = remap.get(&asset.resource_id) {
                asset.resource_id = target.clone();
            }
        }
        for asset in sounds.values_mut() {
            if let Some(target) = remap.get(&asset.resource_id) {
                asset.resource_id = target.clone();
            }
        }
        for asset in videos.values_mut() {
            if let Some(res_id) = asset.resource_id.as_mut()
                && let Some(target) = remap.get(res_id.as_str())
            {
                *res_id = target.clone();
            }
        }
        for clip in compiled_clips.values_mut() {
            if let Some(rec) = clip.reconstructed.as_mut() {
                remap_compiled_clip_resources(rec, &remap);
            }
            if let Some(icon) = clip.icon_bitmap.as_mut()
                && let Some(target) = remap.get(&icon.resource_id)
            {
                icon.resource_id = target.clone();
            }
        }
    }
}

fn allocate_unique_jpg_path(old_path: &str, used_paths: &BTreeSet<String>) -> String {
    let stem = old_path
        .strip_suffix(".png")
        .or_else(|| old_path.strip_suffix(".PNG"))
        .or_else(|| old_path.strip_suffix(".jpg"))
        .or_else(|| old_path.strip_suffix(".JPG"))
        .unwrap_or(old_path);
    let first = format!("{stem}.jpg");
    if !used_paths.contains(&first.to_ascii_lowercase()) {
        return first;
    }
    let mut seq = 2usize;
    loop {
        let cand = format!("{stem}-{seq}.jpg");
        if !used_paths.contains(&cand.to_ascii_lowercase()) {
            return cand;
        }
        seq += 1;
    }
}

/// Import-time resources keep their IDs while the remaining descriptors are read.
/// Record which encodings have been tried so each new arrival does not recompress
/// the same already-optimal PNGs or retest every translucent bitmap.
#[derive(Default)]
pub(super) struct BitmapCompaction {
    lossless: BTreeSet<String>,
    jpeg: BTreeSet<String>,
}
/// Only invoked when total decoded media exceeds the 64 MiB resource budget:
/// 1. First tries lossless `CompressionType::Best` PNG across all PNG bitmaps.
/// 2. Only if budget is still exceeded, re-encodes opaque photographic PNGs into quality-90 JPEG,
///    deduplicating `.jpg` paths and emitting an explicit `fla_bitmap_reencoded` warning.
pub(super) fn compact_bitmaps_under_pressure(
    attempted: &mut BitmapCompaction,
    resources: &mut [ResourceUnit],
    used_paths: &mut BTreeSet<String>,
    total: &mut usize,
    needed_bytes: usize,
    report: &mut ImportReport,
) {
    let budget = MAX_RESOURCE_TOTAL_BYTES - PROVENANCE_RESERVE;
    // Pass 1: Lossless PNG re-compression (`CompressionType::Best`).
    for res in resources.iter_mut() {
        if total.checked_add(needed_bytes).is_some_and(|n| n <= budget) {
            return;
        }
        let old_len = res.byte_len();
        if res.media_type != "image/png"
            || old_len <= 64 * 1024
            || !attempted.lossless.insert(res.id.clone())
        {
            continue;
        }
        let Ok(decoded) = res.decode_image() else {
            continue;
        };
        let mut best_png = Vec::new();
        if PngEncoder::new_with_quality(&mut best_png, CompressionType::Best, FilterType::Adaptive)
            .write_image(
                &decoded.rgba,
                decoded.width,
                decoded.height,
                ExtendedColorType::Rgba8,
            )
            .is_ok()
            && best_png.len() + 8 * 1024 < old_len
            && let Ok(updated) =
                ResourceUnit::from_bytes(res.id.clone(), res.path.clone(), "image/png", &best_png)
        {
            *total = total
                .saturating_sub(old_len)
                .saturating_add(updated.byte_len());
            *res = updated;
        }
    }

    // Pass 2: Lossy JPEG fallback on opaque bitmaps only when lossless PNG still exceeds budget.
    for res in resources.iter_mut() {
        if total.checked_add(needed_bytes).is_some_and(|n| n <= budget) {
            break;
        }
        let old_len = res.byte_len();
        if res.media_type != "image/png"
            || old_len <= 128 * 1024
            || !attempted.jpeg.insert(res.id.clone())
        {
            continue;
        }
        let Ok(decoded) = res.decode_image() else {
            continue;
        };
        let is_opaque = decoded
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255);
        if !is_opaque {
            continue;
        }
        let mut rgb = Vec::with_capacity((decoded.width as usize) * (decoded.height as usize) * 3);
        for px in decoded.rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut jpg = Vec::new();
        if JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode(&rgb, decoded.width, decoded.height, ExtendedColorType::Rgb8)
            .is_ok()
            && jpg.len() + 32 * 1024 < old_len
        {
            let old_lower = res.path.to_ascii_lowercase();
            used_paths.remove(&old_lower);
            let new_path = allocate_unique_jpg_path(&res.path, used_paths);
            if let Ok(updated) =
                ResourceUnit::from_bytes(res.id.clone(), new_path.clone(), "image/jpeg", &jpg)
            {
                let loc = res
                    .id
                    .strip_prefix("legacy-bitmap-")
                    .map(|mid| format!("media {mid}"))
                    .unwrap_or_else(|| res.id.clone());
                report.warning(
                    "fla_bitmap_reencoded",
                    loc,
                    "Opaque bitmap was re-encoded from lossless PNG to high-quality JPEG (quality 90) to fit the 64 MiB project resource budget; lossless source bytes remain in the original FLA.",
                );
                used_paths.insert(new_path.to_ascii_lowercase());
                *total = total
                    .saturating_sub(old_len)
                    .saturating_add(updated.byte_len());
                *res = updated;
            } else {
                used_paths.insert(old_lower);
            }
        }
    }
}

fn encode_rgba_for_budget(
    rgba: &[u8],
    width: u32,
    height: u32,
    prefer_jpeg: bool,
) -> Option<(Vec<u8>, &'static str, &'static str)> {
    let is_opaque = rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
    if prefer_jpeg && is_opaque {
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for px in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut jpg = Vec::new();
        if JpegEncoder::new_with_quality(&mut jpg, 85)
            .encode(&rgb, width, height, ExtendedColorType::Rgb8)
            .is_ok()
        {
            return Some((jpg, "image/jpeg", "jpg"));
        }
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::Adaptive)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .ok()?;
    Some((png, "image/png", "png"))
}

pub(super) fn downscale_bitmaps_under_pressure(
    resources: &mut [ResourceUnit],
    bitmaps: &mut BTreeMap<u16, BitmapAsset>,
    used_paths: &mut BTreeSet<String>,
    total: &mut usize,
    needed_bytes: usize,
    report: &mut ImportReport,
) {
    const MIN_SIDE: u32 = 64;
    let budget = MAX_RESOURCE_TOTAL_BYTES - PROVENANCE_RESERVE;
    for _ in 0..256 {
        if total.checked_add(needed_bytes).is_some_and(|n| n <= budget) {
            break;
        }
        let mut best: Option<(usize, usize)> = None;
        for (idx, res) in resources.iter().enumerate() {
            if !res.id.starts_with("legacy-bitmap-") || res.byte_len() <= 32 * 1024 {
                continue;
            }
            let Some((w, h)) = bitmaps
                .values()
                .find(|a| a.resource_id == res.id)
                .map(|a| (a.width as u32, a.height as u32))
                .or_else(|| {
                    let bytes = res.decoded_bytes().ok()?;
                    image::ImageReader::new(std::io::Cursor::new(&bytes))
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()
                })
            else {
                continue;
            };
            if w.max(h) <= MIN_SIDE {
                continue;
            }
            if best.is_none_or(|(best_len, _)| res.byte_len() > best_len) {
                best = Some((res.byte_len(), idx));
            }
        }
        let Some((old_len, idx)) = best else {
            break;
        };
        let res = &mut resources[idx];
        let Ok(decoded) = res.decode_image() else {
            break;
        };
        let target_max = (decoded.width.max(decoded.height) / 2).max(MIN_SIDE);
        let Some((new_w, new_h, resized)) =
            resize_rgba_proportional(&decoded.rgba, decoded.width, decoded.height, target_max)
        else {
            break;
        };
        let prefer_jpeg = res.media_type == "image/jpeg";
        let Some((bytes, mime, _ext)) = encode_rgba_for_budget(&resized, new_w, new_h, prefer_jpeg)
        else {
            break;
        };
        if bytes.len() >= old_len {
            break;
        }
        let old_path = res.path.clone();
        let new_path = if mime == "image/jpeg" && res.media_type != "image/jpeg" {
            used_paths.remove(&old_path.to_ascii_lowercase());
            let p = allocate_unique_jpg_path(&old_path, used_paths);
            used_paths.insert(p.to_ascii_lowercase());
            p
        } else {
            old_path
        };
        let Ok(updated) = ResourceUnit::from_bytes(res.id.clone(), new_path, mime, &bytes) else {
            break;
        };
        let res_id = res.id.clone();
        let sx = f64::from(decoded.width) / f64::from(new_w);
        let sy = f64::from(decoded.height) / f64::from(new_h);
        *total = total
            .saturating_sub(old_len)
            .saturating_add(updated.byte_len());
        *res = updated;
        for asset in bitmaps.values_mut() {
            if asset.resource_id == res_id {
                asset.width = f64::from(new_w);
                asset.height = f64::from(new_h);
                asset.scale_x *= sx;
                asset.scale_y *= sy;
            }
        }
        let loc = res_id
            .strip_prefix("legacy-bitmap-")
            .map(|mid| format!("media {mid}"))
            .unwrap_or(res_id);
        report.warning(
            "fla_bitmap_downscaled",
            loc,
            format!(
                "Bitmap was downscaled to {new_w}x{new_h} px (preserving Stage dimensions via transform scale) to fit the 64 MiB project resource budget."
            ),
        );
    }
}

fn find_identical_resource_id(
    resources: &[ResourceUnit],
    media_type: &str,
    bytes: &[u8],
) -> Option<String> {
    let probe = ResourceUnit::from_bytes("_probe", "legacy/probe.bin", media_type, bytes).ok()?;
    resources
        .iter()
        .find(|r| r.media_type == probe.media_type && r.data_base64 == probe.data_base64)
        .map(|r| r.id.clone())
}

fn resolve_sound_linkage_id(
    contents_bytes: &[u8],
    descriptor: &ContentsDescriptor,
    language: ScriptLanguage,
    location: &str,
    reserved_ids: &BTreeSet<String>,
    used_ids: &mut BTreeSet<String>,
    report: &mut ImportReport,
) -> Option<String> {
    let lnk = descriptor.linkage.as_ref().or_else(|| {
        descriptor
            .library_metadata
            .as_ref()
            .and_then(|m| m.linkage.as_ref())
    })?;
    let ident = super::read_string(contents_bytes, &lnk.identifier)
        .ok()
        .unwrap_or_default();
    let cls = lnk
        .class_name
        .as_ref()
        .and_then(|c| super::read_string(contents_bytes, c).ok())
        .unwrap_or_default();
    let export_enabled = (lnk.flags & 1) != 0
        || lnk.export_flags.is_some_and(|f| (f & 1) != 0)
        || !ident.trim().is_empty()
        || !cls.trim().is_empty();
    if !export_enabled {
        return None;
    }
    let chosen = if language == ScriptLanguage::ActionScript3 && !cls.trim().is_empty() {
        cls.trim()
    } else if !ident.trim().is_empty() {
        ident.trim()
    } else if !cls.trim().is_empty() {
        cls.trim()
    } else {
        return None;
    };
    let invalid_or_duplicate = chosen.is_empty()
        || chosen.len() > 256
        || chosen.chars().any(char::is_control)
        || chosen == "root"
        || chosen.starts_with("fla-")
        || chosen.starts_with("legacy-")
        || chosen.starts_with("xfl-")
        || chosen.starts_with("swf-")
        || (language == ScriptLanguage::ActionScript3
            && !super::is_valid_qualified_class_name(chosen))
        || reserved_ids.contains(chosen)
        || !used_ids.insert(chosen.to_owned());
    if invalid_or_duplicate {
        report.warning(
            "fla_linkage_approximated",
            location,
            "Sound linkage identifier or ActionScript class name is invalid or collides with another item and was omitted.",
        );
        return None;
    }
    if language == ScriptLanguage::ActionScript3 {
        report.warning(
            "fla_linkage_approximated",
            location,
            "ActionScript 3 sound class linkage is not bound as an instantiable Sound subclass; the audio asset and its XFL linkage metadata are retained.",
        );
    }
    Some(chosen.to_owned())
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
        let name = super::read_string(&contents.original_bytes, &entry.name)?
            .trim()
            .to_owned();
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
    let mut unique = BTreeSet::new();
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
    folders: &BTreeMap<[u32; 2], String>,
    reserved_ids: &BTreeSet<String>,
    language: ScriptLanguage,
    report: &mut ImportReport,
) -> Result<ExtractedAssets, String> {
    let mut resources = Vec::new();
    let mut bitmaps = BTreeMap::new();
    let mut sounds = BTreeMap::new();
    let mut videos = BTreeMap::new();
    let mut compiled_clips = BTreeMap::new();
    let mut skipped_bitmaps = BTreeSet::new();
    let mut used_ids = BTreeSet::new();
    let mut used_paths = BTreeSet::new();
    let mut total = 0usize;
    let mut bitmap_compaction = BitmapCompaction::default();
    let mut streams = BTreeMap::new();
    let mut decoded_streams = BTreeSet::new();
    let mut swf_budget = super::compiled_clip::CompiledClipSwfBudget::default();
    let budget = MAX_RESOURCE_TOTAL_BYTES - PROVENANCE_RESERVE;

    for stream in &document.raw_streams {
        if streams.insert(stream.index, stream).is_some() {
            return Err("Raw archive streams have duplicate identities.".into());
        }
    }

    for descriptor in &document.contents.descriptors {
        let Some(id) = descriptor.media_id else {
            continue;
        };
        let location = format!("media {id}");

        if descriptor.class == ContentsDescriptorClass::Sound {
            let Some(settings) = descriptor.sound.as_ref() else {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Sound descriptor has no typed audio format settings; its bytes remain in the embedded source.",
                );
                if let Some(index) = descriptor.stream_index {
                    decoded_streams.insert(index);
                }
                continue;
            };
            let Some(stream) = descriptor.stream_index.and_then(|idx| streams.get(&idx)) else {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Sound descriptor has no resolved media stream in the archive; it is not imported.",
                );
                continue;
            };
            if stream.role != crate::legacy::BinaryStreamRole::Media
                || !stream
                    .name
                    .strip_prefix('/')
                    .unwrap_or(&stream.name)
                    .eq_ignore_ascii_case(&descriptor.stream_name)
            {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Sound descriptor and media stream do not match; this sound remains in the embedded source.",
                );
                continue;
            };
            decoded_streams.insert(stream.index);
            let decoded = match decode_binary_sound(
                &stream.bytes,
                descriptor.subtype_revision.unwrap_or(0),
                settings,
            ) {
                Ok(decoded) => decoded,
                Err(e) => {
                    report.warning(
                        "fla_media_not_imported",
                        &location,
                        format!(
                            "Sound library item could not be imported ({e}); its bytes remain in the embedded source."
                        ),
                    );
                    continue;
                }
            };
            if decoded.bytes.len() > MAX_RESOURCE_BYTES {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Decoded sound exceeds the 16 MiB single-resource limit; this sound remains in the embedded source.",
                );
                continue;
            }
            let explicit_linkage = resolve_sound_linkage_id(
                &document.contents.original_bytes,
                descriptor,
                language,
                &location,
                reserved_ids,
                &mut used_ids,
                report,
            );
            let over_count = resources.len() >= MAX_RESOURCES - 2;
            let over_bytes = total
                .checked_add(decoded.bytes.len())
                .is_none_or(|n| n > budget);
            if over_count || over_bytes {
                compact_duplicate_media_under_pressure(
                    &mut resources,
                    &mut bitmaps,
                    &mut sounds,
                    &mut videos,
                    &mut compiled_clips,
                    &mut used_paths,
                    &mut total,
                );
                if let Some(existing_id) =
                    find_identical_resource_id(&resources, decoded.media_type, &decoded.bytes)
                {
                    let can_reuse = match &explicit_linkage {
                        None => Some(existing_id),
                        Some(lid) if existing_id.starts_with("legacy-sound-") => {
                            if let Some(existing_res) =
                                resources.iter_mut().find(|r| r.id == existing_id)
                            {
                                existing_res.id = lid.clone();
                                for s_asset in sounds.values_mut() {
                                    if s_asset.resource_id == existing_id {
                                        s_asset.resource_id = lid.clone();
                                    }
                                }
                                Some(lid.clone())
                            } else {
                                None
                            }
                        }
                        Some(_) => None,
                    };
                    if let Some(target_id) = can_reuse {
                        if decoded.short_read {
                            report.warning(
                                "fla_sound_short_read",
                                &location,
                                "The sound stream ended before its declared length; the available complete audio frames were imported.",
                            );
                        }
                        if sounds
                            .insert(
                                id,
                                SoundAsset {
                                    resource_id: target_id,
                                    duration_44: decoded.duration_44,
                                },
                            )
                            .is_some()
                        {
                            return Err(
                                "Sound descriptors have duplicate media identifiers.".into()
                            );
                        }
                        continue;
                    }
                }
                if total
                    .checked_add(decoded.bytes.len())
                    .is_none_or(|n| n > budget)
                {
                    compact_bitmaps_under_pressure(
                        &mut bitmap_compaction,
                        &mut resources,
                        &mut used_paths,
                        &mut total,
                        decoded.bytes.len(),
                        report,
                    );
                }
                if total
                    .checked_add(decoded.bytes.len())
                    .is_none_or(|n| n > budget)
                {
                    downscale_bitmaps_under_pressure(
                        &mut resources,
                        &mut bitmaps,
                        &mut used_paths,
                        &mut total,
                        decoded.bytes.len(),
                        report,
                    );
                }
            }
            if resources.len() >= MAX_RESOURCES - 2 {
                if let Some(lid) = explicit_linkage {
                    used_ids.remove(&lid);
                }
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "The project resource limit is reached; this sound is not imported and remains in the embedded source.",
                );
                continue;
            }
            match total
                .checked_add(decoded.bytes.len())
                .filter(|n| *n <= budget)
            {
                Some(n) => total = n,
                None => {
                    if let Some(lid) = explicit_linkage {
                        used_ids.remove(&lid);
                    }
                    report.warning(
                        "fla_media_not_imported",
                        &location,
                        "Decoded media exceeds the project's resource budget (64 MiB including import provenance); this sound remains in the embedded source.",
                    );
                    continue;
                }
            }
            if decoded.short_read {
                report.warning(
                    "fla_sound_short_read",
                    &location,
                    "The sound stream ended before its declared length; the available complete audio frames were imported.",
                );
            }
            let resource_id = explicit_linkage.unwrap_or_else(|| {
                let fallback_id = format!("legacy-sound-{id}");
                used_ids.insert(fallback_id.clone());
                fallback_id
            });
            let ext = decoded.extension;
            let fallback = format!("legacy/sounds/{id}/sound.{ext}");
            let parent = descriptor
                .library_metadata
                .as_ref()
                .and_then(|m| m.parent)
                .filter(|id| *id != [0, 0]);
            let folder = match parent.map(|id| folders.get(&id)) {
                Some(None) => {
                    report.warning(
                        "fla_library_folder",
                        &location,
                        "The sound's library folder is unresolved; it is placed at the library root.",
                    );
                    None
                }
                other => other.flatten(),
            };
            let name = super::read_string(&document.contents.original_bytes, &descriptor.name).ok();
            let dot_ext = format!(".{ext}");
            let path = name.as_deref().and_then(|name| {
                let name = if let Some(folder) = folder {
                    format!("{folder}/{name}")
                } else {
                    name.to_owned()
                };
                let suffix = if name.to_ascii_lowercase().ends_with(&dot_ext) {
                    ""
                } else {
                    dot_ext.as_str()
                };
                let candidate = format!("legacy/sounds/{id}/{name}{suffix}");
                (!used_paths.contains(&candidate.to_ascii_lowercase())).then_some(candidate)
            });
            let resource = path.and_then(|path| {
                ResourceUnit::from_bytes(&resource_id, path, decoded.media_type, &decoded.bytes)
                    .ok()
            });
            let resource = match resource {
                Some(resource) => resource,
                None => {
                    report.warning(
                        "fla_sound_resource_name",
                        format!("media {id}"),
                        "The original sound name cannot be used as a portable resource path. A stable identity path is used; the original name remains in the embedded Contents provenance.",
                    );
                    ResourceUnit::from_bytes(
                        &resource_id,
                        fallback,
                        decoded.media_type,
                        &decoded.bytes,
                    )
                    .map_err(|e| e.to_string())?
                }
            };
            used_paths.insert(resource.path.to_ascii_lowercase());
            if sounds
                .insert(
                    id,
                    SoundAsset {
                        resource_id,
                        duration_44: decoded.duration_44,
                    },
                )
                .is_some()
            {
                return Err("Sound descriptors have duplicate media identifiers.".into());
            }
            resources.push(resource);
            continue;
        }

        if descriptor.class == ContentsDescriptorClass::LinkedVideo {
            if let Some(index) = descriptor.stream_index {
                decoded_streams.insert(index);
            }
            report.warning(
                "fla_media_not_imported",
                &location,
                "External linked video item has no embedded media stream; a sized Video viewport is retained and original metadata remains in the embedded source.",
            );
            videos.insert(
                id,
                VideoAsset {
                    resource_id: None,
                    width: 160.0,
                    height: 120.0,
                    sync_video: true,
                    import_audio: false,
                },
            );
            continue;
        }

        if descriptor.class == ContentsDescriptorClass::Video {
            let Some(stream) = descriptor.stream_index.and_then(|idx| streams.get(&idx)) else {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Video descriptor has no resolved media stream in the archive; it is not imported.",
                );
                continue;
            };
            if stream.role != crate::legacy::BinaryStreamRole::Media
                || !stream
                    .name
                    .strip_prefix('/')
                    .unwrap_or(&stream.name)
                    .eq_ignore_ascii_case(&descriptor.stream_name)
            {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Video descriptor and media stream do not match; this video remains in the embedded source.",
                );
                continue;
            }
            decoded_streams.insert(stream.index);
            if stream.bytes.is_empty() {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Video media stream is empty; a sized Video viewport is retained and original metadata remains in the embedded source.",
                );
                videos.insert(
                    id,
                    VideoAsset {
                        resource_id: None,
                        width: 160.0,
                        height: 120.0,
                        sync_video: true,
                        import_audio: false,
                    },
                );
                continue;
            }
            let decoded = match decode_binary_video(&stream.bytes) {
                Ok(v) => v,
                Err(e) => {
                    report.warning(
                        "fla_media_not_imported",
                        &location,
                        format!(
                            "Video library item could not be decoded ({e}); its bytes remain in the embedded source."
                        ),
                    );
                    continue;
                }
            };
            match decoded.payload {
                BinaryVideoPayload::EmptyViewport => {
                    if videos
                        .insert(
                            id,
                            VideoAsset {
                                resource_id: None,
                                width: decoded.width,
                                height: decoded.height,
                                sync_video: decoded.sync_video,
                                import_audio: decoded.import_audio,
                            },
                        )
                        .is_some()
                    {
                        return Err("Video descriptors have duplicate media identifiers.".into());
                    }
                }
                BinaryVideoPayload::UnsupportedCodec(reason) => {
                    report.warning(
                        "fla_media_not_imported",
                        &location,
                        format!(
                            "Video library item uses an undecodable embedded codec ({reason}); a sized Video viewport is retained and original bytes remain in the embedded source."
                        ),
                    );
                    videos.insert(
                        id,
                        VideoAsset {
                            resource_id: None,
                            width: decoded.width,
                            height: decoded.height,
                            sync_video: decoded.sync_video,
                            import_audio: decoded.import_audio,
                        },
                    );
                }
                BinaryVideoPayload::Container {
                    media_type,
                    extension,
                    bytes: video_bytes,
                } => {
                    let over_count = resources.len() >= MAX_RESOURCES - 2;
                    let over_bytes = total
                        .checked_add(video_bytes.len())
                        .is_none_or(|n| n > budget);
                    if over_count || over_bytes {
                        compact_duplicate_media_under_pressure(
                            &mut resources,
                            &mut bitmaps,
                            &mut sounds,
                            &mut videos,
                            &mut compiled_clips,
                            &mut used_paths,
                            &mut total,
                        );
                        if let Some(existing_id) =
                            find_identical_resource_id(&resources, media_type, &video_bytes)
                        {
                            if videos
                                .insert(
                                    id,
                                    VideoAsset {
                                        resource_id: Some(existing_id),
                                        width: decoded.width,
                                        height: decoded.height,
                                        sync_video: decoded.sync_video,
                                        import_audio: decoded.import_audio,
                                    },
                                )
                                .is_some()
                            {
                                return Err(
                                    "Video descriptors have duplicate media identifiers.".into()
                                );
                            }
                            continue;
                        }
                        if total
                            .checked_add(video_bytes.len())
                            .is_none_or(|n| n > budget)
                        {
                            compact_bitmaps_under_pressure(
                                &mut bitmap_compaction,
                                &mut resources,
                                &mut used_paths,
                                &mut total,
                                video_bytes.len(),
                                report,
                            );
                        }
                        if total
                            .checked_add(video_bytes.len())
                            .is_none_or(|n| n > budget)
                        {
                            downscale_bitmaps_under_pressure(
                                &mut resources,
                                &mut bitmaps,
                                &mut used_paths,
                                &mut total,
                                video_bytes.len(),
                                report,
                            );
                        }
                    }
                    if resources.len() >= MAX_RESOURCES - 2
                        || total
                            .checked_add(video_bytes.len())
                            .is_none_or(|n| n > budget)
                    {
                        report.warning(
                            "fla_media_not_imported",
                            &location,
                            "Embedded video exceeds the project's resource budget; a sized Video viewport is retained and original bytes remain in the embedded source.",
                        );
                        videos.insert(
                            id,
                            VideoAsset {
                                resource_id: None,
                                width: decoded.width,
                                height: decoded.height,
                                sync_video: decoded.sync_video,
                                import_audio: decoded.import_audio,
                            },
                        );
                        continue;
                    }
                    total += video_bytes.len();
                    let resource_id = format!("legacy-video-{id}");
                    used_ids.insert(resource_id.clone());
                    let fallback = format!("legacy/videos/{id}/video.{extension}");
                    let name =
                        super::read_string(&document.contents.original_bytes, &descriptor.name)
                            .ok();
                    let dot_ext = format!(".{extension}");
                    let path = name.as_deref().and_then(|n| {
                        let suffix = if n.to_ascii_lowercase().ends_with(&dot_ext) {
                            ""
                        } else {
                            dot_ext.as_str()
                        };
                        let cand = format!("legacy/videos/{id}/{n}{suffix}");
                        (!used_paths.contains(&cand.to_ascii_lowercase())).then_some(cand)
                    });
                    let unit = path
                        .and_then(|p| {
                            ResourceUnit::from_bytes(&resource_id, p, media_type, &video_bytes).ok()
                        })
                        .map_or_else(
                            || {
                                ResourceUnit::from_bytes(
                                    &resource_id,
                                    fallback,
                                    media_type,
                                    &video_bytes,
                                )
                                .map_err(|e| e.to_string())
                            },
                            Ok,
                        )?;
                    used_paths.insert(unit.path.to_ascii_lowercase());
                    if videos
                        .insert(
                            id,
                            VideoAsset {
                                resource_id: Some(resource_id),
                                width: decoded.width,
                                height: decoded.height,
                                sync_video: decoded.sync_video,
                                import_audio: decoded.import_audio,
                            },
                        )
                        .is_some()
                    {
                        return Err("Video descriptors have duplicate media identifiers.".into());
                    }
                    resources.push(unit);
                }
            }
            continue;
        }

        if descriptor.class == ContentsDescriptorClass::SwfPage {
            if let Some(index) = descriptor.stream_index {
                decoded_streams.insert(index);
            }
            let Some(cc) = descriptor.compiled_clip.as_ref() else {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Compiled-clip descriptor has no component payload; its bytes remain in the embedded source.",
                );
                continue;
            };
            let (width, height) = if let Some([right, left, bottom, top]) = cc.bounds_words {
                let w = ((f64::from(right) - f64::from(left)) / 20.0).abs();
                let h = ((f64::from(bottom) - f64::from(top)) / 20.0).abs();
                (
                    if w > 0.0 {
                        w.clamp(10.0, 4096.0)
                    } else {
                        100.0
                    },
                    if h > 0.0 {
                        h.clamp(10.0, 4096.0)
                    } else {
                        100.0
                    },
                )
            } else {
                (100.0, 100.0)
            };

            let mut decoded_icon_candidate: Option<(BitmapAsset, Vec<u8>)> = None;
            if let Some(icon_span) = cc.icon_bitmap
                && let Some(icon_raw) = document
                    .contents
                    .original_bytes
                    .get(icon_span.start..icon_span.end)
                && let Ok(decoded_icon) = decode_binary_bitmap(icon_raw, cc.icon_encoded_byte_count)
                && let Ok(png) = decoded_icon.png_bytes()
                && png.len() <= MAX_RESOURCE_BYTES
            {
                let avg = crate::average_bitmap_color(&decoded_icon.rgba);
                decoded_icon_candidate = Some((
                    BitmapAsset {
                        resource_id: format!("legacy-swf-icon-{id}"),
                        width: f64::from(decoded_icon.width),
                        height: f64::from(decoded_icon.height),
                        scale_x: 1.0,
                        scale_y: 1.0,
                        origin_x: 0.0,
                        origin_y: 0.0,
                        bounds_scale_x: 1.0,
                        bounds_scale_y: 1.0,
                        smoothing: false,
                        average_color: avg,
                    },
                    png,
                ));
            }

            let swf_slice = cc
                .primary_swf
                .and_then(|sp| document.contents.original_bytes.get(sp.start..sp.end));
            let preview_asset_opt = decoded_icon_candidate.as_ref().map(|(a, _)| a.clone());

            let (reconstructed, need_icon_resource) = if let Some(swf_bytes) =
                swf_slice.filter(|b| b.len() >= 8 && matches!(&b[..3], b"FWS" | b"CWS" | b"ZWS"))
            {
                match super::compiled_clip::reconstruct_compiled_clip_swf(
                    id,
                    swf_bytes,
                    &document.contents.original_bytes,
                    descriptor,
                    width,
                    height,
                    &preview_asset_opt,
                    language,
                    &location,
                    &mut swf_budget,
                    &mut resources,
                    &mut used_ids,
                    &mut used_paths,
                    &mut total,
                    budget,
                    report,
                ) {
                    Some((rec, used_icon)) => (Some(rec), used_icon),
                    None => (None, true),
                }
            } else {
                report.warning(
                    "fla_media_not_imported",
                    &location,
                    "Compiled-clip library item has no embedded SWF stream; a placeholder symbol keeps its placements.",
                );
                (None, true)
            };

            let mut icon_bitmap = None;
            if need_icon_resource
                && let Some((asset, png)) = decoded_icon_candidate
                && resources.len() < MAX_RESOURCES - 2
                && total.checked_add(png.len()).is_some_and(|n| n <= budget)
            {
                let icon_path = format!("legacy/compiled-clips/{id}/icon.png");
                if let Ok(unit) =
                    ResourceUnit::from_bytes(&asset.resource_id, &icon_path, "image/png", &png)
                {
                    total += png.len();
                    used_ids.insert(asset.resource_id.clone());
                    used_paths.insert(icon_path.to_ascii_lowercase());
                    resources.push(unit);
                    icon_bitmap = Some(asset);
                }
            }

            compiled_clips.insert(
                id,
                CompiledClipAsset {
                    reconstructed,
                    icon_bitmap,
                    width,
                    height,
                },
            );
            continue;
        }

        if descriptor.class != ContentsDescriptorClass::Bitmap {
            report.warning(
                "fla_media_not_imported",
                &location,
                "Auxiliary media library item is not imported; its bytes remain in the embedded source.",
            );
            if let Some(index) = descriptor.stream_index {
                decoded_streams.insert(index);
            }
            continue;
        }
        let Some(settings) = descriptor.bitmap.as_ref() else {
            report.warning(
                "fla_bitmap_skipped",
                &location,
                "Bitmap descriptor has no typed bitmap settings; it becomes an unresolved placeholder.",
            );
            skipped_bitmaps.insert(id);
            if let Some(index) = descriptor.stream_index {
                decoded_streams.insert(index);
            }
            continue;
        };
        let Some(stream) = descriptor.stream_index.and_then(|idx| streams.get(&idx)) else {
            report.warning(
                "fla_bitmap_skipped",
                &location,
                "Bitmap descriptor has no resolved media stream in the archive; it becomes an unresolved placeholder.",
            );
            skipped_bitmaps.insert(id);
            continue;
        };
        if stream.role != crate::legacy::BinaryStreamRole::Media
            || !stream
                .name
                .strip_prefix('/')
                .unwrap_or(&stream.name)
                .eq_ignore_ascii_case(&descriptor.stream_name)
        {
            report.warning(
                "fla_bitmap_skipped",
                &location,
                "Bitmap descriptor and media stream do not match; it becomes an unresolved placeholder.",
            );
            skipped_bitmaps.insert(id);
            continue;
        }
        decoded_streams.insert(stream.index);
        let mut decoded = match decode_binary_bitmap(&stream.bytes, settings.encoded_byte_count) {
            Ok(decoded) if decoded.original_bytes == stream.bytes => decoded,
            Ok(_) => {
                report.warning("fla_bitmap_skipped",&location,"The bitmap payload does not match its media stream; it becomes an unresolved placeholder.");
                skipped_bitmaps.insert(id);
                continue;
            }
            Err(e) => {
                report.warning("fla_bitmap_skipped",&location,format!("The bitmap could not be decoded ({e}); it becomes an unresolved placeholder and its bytes remain in the embedded source."));
                skipped_bitmaps.insert(id);
                continue;
            }
        };
        let (orig_w, orig_h) = (decoded.width, decoded.height);
        let mut downscaled = false;
        if decoded.width > neoflash_core::MAX_BITMAP_DIMENSION
            || decoded.height > neoflash_core::MAX_BITMAP_DIMENSION
        {
            let Some((nw, nh, resized)) = resize_rgba_proportional(
                &decoded.rgba,
                decoded.width,
                decoded.height,
                neoflash_core::MAX_BITMAP_DIMENSION,
            ) else {
                report.warning(
                    "fla_bitmap_skipped",
                    &location,
                    "Bitmap exceeds the 4096 px per-axis limit and could not be proportionally downscaled; it becomes an unresolved placeholder.",
                );
                skipped_bitmaps.insert(id);
                continue;
            };
            decoded.width = nw;
            decoded.height = nh;
            decoded.rgba = resized;
            downscaled = true;
        }
        let [xmin, xmax, ymin, ymax] = decoded.bounds_words;
        let (origin_x, origin_y, bounds_scale_x, bounds_scale_y) = if xmin != i32::MIN
            && i64::from(xmax) > i64::from(xmin)
            && i64::from(ymax) > i64::from(ymin)
        {
            let ox = f64::from(xmin) / 20.0;
            let oy = f64::from(ymin) / 20.0;
            let bw = (f64::from(xmax) - f64::from(xmin)) / 20.0;
            let bh = (f64::from(ymax) - f64::from(ymin)) / 20.0;
            let bsx = (bw / f64::from(orig_w)).clamp(1e-4, 1e4);
            let bsy = (bh / f64::from(orig_h)).clamp(1e-4, 1e4);
            (
                ox,
                oy,
                if (bsx - 1.0).abs() <= 1e-6 { 1.0 } else { bsx },
                if (bsy - 1.0).abs() <= 1e-6 { 1.0 } else { bsy },
            )
        } else {
            if xmin != 0 || ymin != 0 {
                report.warning(
                    "fla_bitmap_origin",
                    &location,
                    "The bitmap's stored bounds are degenerate and start away from 0,0; the image is placed at its instance origin.",
                );
            }
            (0.0, 0.0, 1.0, 1.0)
        };
        let average_color = crate::average_bitmap_color(&decoded.rgba);
        let is_opaque = decoded
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255);
        // A validated source JPEG already contains the authored pixels at its original
        // quality. Expanding photographs to PNG wastes the shared resource budget and
        // can force a second lossy encoding when other library items arrive.
        let (mut cand_bytes, mut cand_mime, mut cand_ext) = if let Some(jpeg) = decoded
            .source_jpeg()
            .filter(|data| !downscaled && data.len() <= MAX_RESOURCE_BYTES)
        {
            (jpeg.into_owned(), "image/jpeg", "jpg")
        } else {
            (
                decoded.png_bytes().map_err(|e| e.to_string())?,
                "image/png",
                "png",
            )
        };
        let mut reencoded_jpeg = false;

        if cand_bytes.len() > MAX_RESOURCE_BYTES {
            let mut best_png = Vec::new();
            let fits_best = PngEncoder::new_with_quality(
                &mut best_png,
                CompressionType::Best,
                FilterType::Adaptive,
            )
            .write_image(
                &decoded.rgba,
                decoded.width,
                decoded.height,
                ExtendedColorType::Rgba8,
            )
            .is_ok()
                && best_png.len() <= MAX_RESOURCE_BYTES;
            if fits_best {
                cand_bytes = best_png;
            } else {
                let mut jpg = Vec::new();
                let fits_jpg = if is_opaque {
                    let mut rgb = Vec::with_capacity(
                        (decoded.width as usize) * (decoded.height as usize) * 3,
                    );
                    for px in decoded.rgba.as_chunks::<4>().0 {
                        rgb.extend_from_slice(&px[..3]);
                    }
                    JpegEncoder::new_with_quality(&mut jpg, 90)
                        .encode(&rgb, decoded.width, decoded.height, ExtendedColorType::Rgb8)
                        .is_ok()
                        && jpg.len() <= MAX_RESOURCE_BYTES
                } else {
                    false
                };
                if fits_jpg {
                    cand_bytes = jpg;
                    cand_mime = "image/jpeg";
                    cand_ext = "jpg";
                    reencoded_jpeg = true;
                } else {
                    while cand_bytes.len() > MAX_RESOURCE_BYTES {
                        let cur_max = decoded.width.max(decoded.height);
                        if cur_max <= 32 {
                            break;
                        }
                        let target = (cur_max / 2).max(32);
                        let Some((nw, nh, resized)) = resize_rgba_proportional(
                            &decoded.rgba,
                            decoded.width,
                            decoded.height,
                            target,
                        ) else {
                            break;
                        };
                        decoded.width = nw;
                        decoded.height = nh;
                        decoded.rgba = resized;
                        downscaled = true;
                        let Some((enc, mime, ext)) =
                            encode_rgba_for_budget(&decoded.rgba, nw, nh, is_opaque)
                        else {
                            break;
                        };
                        if mime == "image/jpeg" {
                            reencoded_jpeg = true;
                        }
                        cand_bytes = enc;
                        cand_mime = mime;
                        cand_ext = ext;
                    }
                }
            }
        }
        if cand_bytes.len() > MAX_RESOURCE_BYTES {
            report.warning(
                "fla_bitmap_skipped",
                &location,
                "Encoded bitmap PNG exceeds the 16 MiB single-resource limit; this bitmap becomes an unresolved placeholder.",
            );
            skipped_bitmaps.insert(id);
            continue;
        }
        let over_count = resources.len() >= MAX_RESOURCES - 2;
        let over_bytes = total
            .checked_add(cand_bytes.len())
            .is_none_or(|n| n > budget);
        if over_count || over_bytes {
            compact_duplicate_media_under_pressure(
                &mut resources,
                &mut bitmaps,
                &mut sounds,
                &mut videos,
                &mut compiled_clips,
                &mut used_paths,
                &mut total,
            );
            if let Some(existing_id) =
                find_identical_resource_id(&resources, cand_mime, &cand_bytes)
            {
                if reencoded_jpeg {
                    report.warning(
                        "fla_bitmap_reencoded",
                        &location,
                        "Opaque bitmap was re-encoded from lossless PNG to high-quality JPEG (quality 90) to fit project resource limits; lossless source bytes remain in the original FLA.",
                    );
                }
                if downscaled {
                    report.warning(
                        "fla_bitmap_downscaled",
                        &location,
                        format!(
                            "Bitmap was downscaled from {orig_w}x{orig_h} px to {}x{} px (preserving Stage dimensions via transform scale) to fit project bitmap limits.",
                            decoded.width, decoded.height
                        ),
                    );
                }
                if bitmaps
                    .insert(
                        id,
                        BitmapAsset {
                            resource_id: existing_id,
                            width: f64::from(decoded.width),
                            height: f64::from(decoded.height),
                            scale_x: f64::from(orig_w) / f64::from(decoded.width),
                            scale_y: f64::from(orig_h) / f64::from(decoded.height),
                            origin_x,
                            origin_y,
                            bounds_scale_x,
                            bounds_scale_y,
                            smoothing: settings.smoothing,
                            average_color,
                        },
                    )
                    .is_some()
                {
                    return Err("Bitmap descriptors have duplicate media identifiers.".into());
                }
                continue;
            }
            if total
                .checked_add(cand_bytes.len())
                .is_none_or(|n| n > budget)
            {
                compact_bitmaps_under_pressure(
                    &mut bitmap_compaction,
                    &mut resources,
                    &mut used_paths,
                    &mut total,
                    cand_bytes.len(),
                    report,
                );
            }
        }
        if resources.len() >= MAX_RESOURCES - 2 {
            report.warning("fla_bitmap_skipped",&location,"The project resource limit is reached; this bitmap becomes an unresolved placeholder.");
            skipped_bitmaps.insert(id);
            continue;
        }
        // Keep room for the embedded original FLA and provenance.
        let (final_bytes, final_mime, final_ext) = if total
            .checked_add(cand_bytes.len())
            .is_some_and(|n| n <= budget)
        {
            total += cand_bytes.len();
            (cand_bytes, cand_mime, cand_ext)
        } else {
            let mut best_png = Vec::new();
            let fits_best_png = cand_mime == "image/png"
                && PngEncoder::new_with_quality(
                    &mut best_png,
                    CompressionType::Best,
                    FilterType::Adaptive,
                )
                .write_image(
                    &decoded.rgba,
                    decoded.width,
                    decoded.height,
                    ExtendedColorType::Rgba8,
                )
                .is_ok()
                && total
                    .checked_add(best_png.len())
                    .is_some_and(|n| n <= budget);
            if fits_best_png {
                total += best_png.len();
                (best_png, "image/png", "png")
            } else {
                let mut jpg = Vec::new();
                let encoded_jpg = if is_opaque && cand_mime == "image/png" {
                    let mut rgb = Vec::with_capacity(
                        (decoded.width as usize) * (decoded.height as usize) * 3,
                    );
                    for px in decoded.rgba.as_chunks::<4>().0 {
                        rgb.extend_from_slice(&px[..3]);
                    }
                    JpegEncoder::new_with_quality(&mut jpg, 90)
                        .encode(&rgb, decoded.width, decoded.height, ExtendedColorType::Rgb8)
                        .is_ok()
                } else {
                    false
                };
                if encoded_jpg && total.checked_add(jpg.len()).is_some_and(|n| n <= budget) {
                    reencoded_jpeg = true;
                    total += jpg.len();
                    (jpg, "image/jpeg", "jpg")
                } else {
                    if encoded_jpg && jpg.len() < cand_bytes.len() {
                        cand_bytes = jpg;
                        cand_mime = "image/jpeg";
                        cand_ext = "jpg";
                        reencoded_jpeg = true;
                    } else if !best_png.is_empty() && best_png.len() < cand_bytes.len() {
                        cand_bytes = best_png;
                    }
                    downscale_bitmaps_under_pressure(
                        &mut resources,
                        &mut bitmaps,
                        &mut used_paths,
                        &mut total,
                        cand_bytes.len(),
                        report,
                    );
                    while total
                        .checked_add(cand_bytes.len())
                        .is_none_or(|n| n > budget)
                    {
                        let cur_max = decoded.width.max(decoded.height);
                        if cur_max <= 64 {
                            break;
                        }
                        let target = (cur_max / 2).max(64);
                        let Some((nw, nh, resized)) = resize_rgba_proportional(
                            &decoded.rgba,
                            decoded.width,
                            decoded.height,
                            target,
                        ) else {
                            break;
                        };
                        decoded.width = nw;
                        decoded.height = nh;
                        decoded.rgba = resized;
                        downscaled = true;
                        let Some((enc, mime, ext)) =
                            encode_rgba_for_budget(&decoded.rgba, nw, nh, is_opaque)
                        else {
                            break;
                        };
                        if mime == "image/jpeg" {
                            reencoded_jpeg = true;
                        }
                        cand_bytes = enc;
                        cand_mime = mime;
                        cand_ext = ext;
                        if total
                            .checked_add(cand_bytes.len())
                            .is_none_or(|n| n > budget)
                        {
                            downscale_bitmaps_under_pressure(
                                &mut resources,
                                &mut bitmaps,
                                &mut used_paths,
                                &mut total,
                                cand_bytes.len(),
                                report,
                            );
                        }
                    }
                    if total
                        .checked_add(cand_bytes.len())
                        .is_some_and(|n| n <= budget)
                    {
                        total += cand_bytes.len();
                        (cand_bytes, cand_mime, cand_ext)
                    } else {
                        report.warning("fla_bitmap_skipped",&location,"Decoded bitmaps exceed the project's resource budget (64 MiB including import provenance); this bitmap becomes an unresolved placeholder.");
                        skipped_bitmaps.insert(id);
                        continue;
                    }
                }
            }
        };
        if reencoded_jpeg {
            report.warning(
                "fla_bitmap_reencoded",
                &location,
                "Opaque bitmap was re-encoded from lossless PNG to high-quality JPEG (quality 90) to fit the 64 MiB project resource budget; lossless source bytes remain in the original FLA.",
            );
        }
        if downscaled {
            report.warning(
                "fla_bitmap_downscaled",
                &location,
                format!(
                    "Bitmap was downscaled from {orig_w}x{orig_h} px to {}x{} px (preserving Stage dimensions via transform scale) to fit project bitmap limits.",
                    decoded.width, decoded.height
                ),
            );
        }
        let resource_id = format!("legacy-bitmap-{id}");
        used_ids.insert(resource_id.clone());
        let fallback = format!("legacy/bitmaps/{id}/bitmap.{final_ext}");
        let parent = descriptor
            .library_metadata
            .as_ref()
            .and_then(|m| m.parent)
            .filter(|id| *id != [0, 0]);
        let folder = match parent.map(|id| folders.get(&id)) {
            Some(None) => {
                report.warning(
                    "fla_library_folder",
                    &location,
                    "The bitmap's library folder is unresolved; it is placed at the library root.",
                );
                None
            }
            other => other.flatten(),
        };
        let name = super::read_string(&document.contents.original_bytes, &descriptor.name).ok();
        let dot_ext = format!(".{final_ext}");
        let path = name.as_deref().and_then(|name| {
            let name = if let Some(folder) = folder {
                format!("{folder}/{name}")
            } else {
                name.to_owned()
            };
            let suffix = if name.to_ascii_lowercase().ends_with(&dot_ext) {
                ""
            } else {
                dot_ext.as_str()
            };
            let cand = format!("legacy/bitmaps/{id}/{name}{suffix}");
            (!used_paths.contains(&cand.to_ascii_lowercase())).then_some(cand)
        });
        let resource = path.and_then(|path| {
            ResourceUnit::from_bytes(&resource_id, path, final_mime, &final_bytes).ok()
        });
        let resource = match resource {
            Some(resource) => resource,
            None => {
                report.warning("fla_bitmap_resource_name", format!("media {id}"), "The original bitmap name cannot be used as a portable resource path. A stable identity path is used; the original name remains in the embedded Contents provenance.");
                ResourceUnit::from_bytes(&resource_id, fallback, final_mime, &final_bytes)
                    .map_err(|e| e.to_string())?
            }
        };
        used_paths.insert(resource.path.to_ascii_lowercase());
        if bitmaps
            .insert(
                id,
                BitmapAsset {
                    resource_id,
                    width: f64::from(decoded.width),
                    height: f64::from(decoded.height),
                    scale_x: f64::from(orig_w) / f64::from(decoded.width),
                    scale_y: f64::from(orig_h) / f64::from(decoded.height),
                    origin_x,
                    origin_y,
                    bounds_scale_x,
                    bounds_scale_y,
                    smoothing: settings.smoothing,
                    average_color,
                },
            )
            .is_some()
        {
            return Err("Bitmap descriptors have duplicate media identifiers.".into());
        }
        resources.push(resource);
    }
    if document.raw_streams.iter().any(|stream| {
        stream.role == crate::legacy::BinaryStreamRole::Media
            && !decoded_streams.contains(&stream.index)
    }) {
        report.warning("fla_media_unreferenced","Media streams","Some media streams have no library descriptor (typically deleted items); they remain only in the embedded source.");
    }
    Ok(ExtractedAssets {
        resources,
        bitmaps,
        sounds,
        videos,
        compiled_clips,
        skipped_bitmaps,
    })
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
