//! Native legacy binary `.fla` (OLE2/CFB) writer for Flash CS4 (`v10`, `0x47`)
//! and Flash 8 (`v8`, `0x3f`) (`T-189`).
//!
//! Produces deterministic sequential Compound File Binary (`.fla`) archives
//! whose `Contents`, `Page <N>`, `Symbol <N>`, and `Media <N>` streams match the
//! exact binary record layout decoded by `neoflash_import::legacy`.

use crate::ExportError;
use crate::fla_preflight::{
    FlaCompatibilityReport, FlaExportOptions, FlaTargetProfile, prepare_project_for_fla_profile,
};
use image::ImageReader;
use neoflash_core::*;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FLA_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Export `project` to a legacy binary `.fla` byte buffer (`LegacyFlashCS4` or `LegacyFlash8`)
/// using default [`FlaExportOptions`].
pub fn export_legacy_binary_fla_bytes(
    project: &Project,
    profile: FlaTargetProfile,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    export_legacy_binary_fla_with_options(project, profile, &FlaExportOptions::default())
}

/// Export `project` to a legacy binary `.fla` byte buffer (`LegacyFlashCS4` or `LegacyFlash8`)
/// with explicit [`FlaExportOptions`].
pub fn export_legacy_binary_fla_with_options(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    if !profile.is_legacy_binary() {
        return Err(ExportError::InvalidProject(format!(
            "Profile '{}' is not a legacy binary FLA profile",
            profile.label()
        )));
    }
    project
        .validate()
        .map_err(|_| ExportError::InvalidProject("Project validation failed".to_owned()))?;
    let (prepared, report) = prepare_project_for_fla_profile(project, profile, options);
    if !report.can_export() {
        let reasons: Vec<String> = report
            .blocking_items()
            .into_iter()
            .map(|item| format!("{}: {}", item.location, item.message))
            .collect();
        return Err(ExportError::UnsupportedScript(format!(
            "Legacy FLA preflight blocked export for {}: {}",
            profile.label(),
            reasons.join("; ")
        )));
    }

    let bytes = write_legacy_cfb_archive(&prepared, profile)?;
    Ok((bytes, report))
}

pub(crate) fn write_legacy_binary_fla_from_prepared(
    prepared: &Project,
    profile: FlaTargetProfile,
) -> Result<Vec<u8>, ExportError> {
    write_legacy_cfb_archive(prepared, profile)
}

/// Re-import a `.fla` archive from in-memory bytes via `neoflash_import::import_project`.
pub fn import_fla_bytes(bytes: &[u8]) -> Result<Project, ExportError> {
    let seq = TEMP_FLA_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = std::env::temp_dir().join(format!(
        "neoflash-roundtrip-{}-{seq}.fla",
        std::process::id()
    ));
    std::fs::write(&tmp_path, bytes)?;
    let res = neoflash_import::import_project(&tmp_path);
    let _ = std::fs::remove_file(&tmp_path);
    res.map_err(|e| ExportError::InvalidProject(format!("Failed to re-import FLA bytes: {e}")))
}

#[derive(Default)]
pub(crate) struct MfcWriter {
    pub(crate) buf: Vec<u8>,
    class_refs: BTreeMap<String, u16>,
    next_ref: u16,
}

impl MfcWriter {
    pub(crate) fn new() -> Self {
        Self {
            buf: Vec::new(),
            class_refs: BTreeMap::new(),
            // Slot 0 is reserved for Null in MFC's combined class/object reference table.
            next_ref: 1,
        }
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, v: f32) {
        let clean = if v.is_finite() { v } else { 0.0 };
        self.buf.extend_from_slice(&clean.to_le_bytes());
    }

    pub(crate) fn bytes(&mut self, slice: &[u8]) {
        self.buf.extend_from_slice(slice);
    }

    pub(crate) fn zeros(&mut self, count: usize) {
        self.buf.resize(self.buf.len() + count, 0);
    }

    pub(crate) fn write_str_len(&mut self, len: usize) {
        if len < 255 {
            self.u8(len as u8);
        } else if len < 0xfffe {
            self.u8(0xff);
            self.u16(len as u16);
        } else {
            self.u8(0xff);
            self.u16(0xffff);
            self.u32(len as u32);
        }
    }

    /// Write an MFC `CString` using ASCII when all codepoints are ASCII, or UTF-16LE with
    /// the `0xff 0xfe 0xff` Unicode marker otherwise.
    pub(crate) fn cstring(&mut self, s: &str) {
        if s.is_ascii() {
            self.write_str_len(s.len());
            self.bytes(s.as_bytes());
        } else {
            let units: Vec<u16> = s.encode_utf16().collect();
            self.u8(0xff);
            self.u16(0xfffe);
            self.write_str_len(units.len());
            for unit in units {
                self.u16(unit);
            }
        }
    }

    /// Write a compact stream name (`Page 1`, `Symbol 1`, `Media 1`).
    pub(crate) fn compact_stream_name(&mut self, name: &str, trailing_nul: bool) {
        let bytes = name.as_bytes();
        let total = bytes.len() + usize::from(trailing_nul);
        self.u8(total as u8);
        self.bytes(bytes);
        if trailing_nul {
            self.u8(0);
        }
    }

    /// Begin a new serialized MFC object of `class_name` (schema 1) and return its
    /// allocated object reference index.
    pub(crate) fn begin_object(&mut self, class_name: &str) -> u16 {
        if let Some(&class_ref) = self.class_refs.get(class_name) {
            self.u16(0x8000 | class_ref);
        } else {
            self.u16(0xffff);
            self.u16(1); // schema = 1
            self.u16(class_name.len() as u16);
            self.bytes(class_name.as_bytes());
            let class_ref = self.next_ref;
            self.next_ref = self.next_ref.saturating_add(1);
            self.class_refs.insert(class_name.to_owned(), class_ref);
        }
        let obj_ref = self.next_ref;
        self.next_ref = self.next_ref.saturating_add(1);
        obj_ref
    }

    pub(crate) fn write_object_ref(&mut self, obj_ref: u16) {
        self.u16(obj_ref);
    }

    pub(crate) fn write_null_ref(&mut self) {
        self.u16(0);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedBitmapMedia {
    pub media_id: u16,
    pub stream_name: String,
    pub library_name: String,
    pub smoothing: bool,
    pub stream_bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSoundMedia {
    pub media_id: u16,
    pub stream_name: String,
    pub library_name: String,
    pub format_flags: u16,
    pub sample_count: u32,
    pub encoded_byte_count: u32,
    pub stream_bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct MediaExportCatalog {
    pub bitmaps: Vec<PreparedBitmapMedia>,
    pub sounds: Vec<PreparedSoundMedia>,
    pub bitmap_by_resource: BTreeMap<String, u16>,
    pub sound_by_resource: BTreeMap<String, u16>,
}

fn find_smoothing_for_resource(project: &Project, resource_id: &str) -> bool {
    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if let ElementKind::Bitmap {
                        resource_id: id,
                        smoothing,
                        ..
                    } = &el.kind
                        && id == resource_id
                    {
                        return *smoothing;
                    }
                }
            }
        }
    }
    true
}

fn encode_native_bitmap_stream(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let w = width.clamp(1, 4096) as u16;
    let h = height.clamp(1, 4096) as u16;
    let stride = w.saturating_mul(4);
    let mut out = Vec::with_capacity(24 + usize::from(stride) * usize::from(h));
    out.push(3); // revision = 3
    out.push(5); // format = 5 (32-bit premultiplied ARGB)
    out.extend_from_slice(&stride.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    for bound in [0_i32, i32::from(w) * 20, 0_i32, i32::from(h) * 20] {
        out.extend_from_slice(&bound.to_le_bytes());
    }
    out.push(1); // alpha = 1
    out.push(0); // compressed = 0
    for px in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(usize::from(w) * usize::from(h))
    {
        let a = px[3];
        let premul =
            |c: u8| -> u8 { ((u32::from(c) * u32::from(a) + 127) / 255).min(u32::from(a)) as u8 };
        out.extend_from_slice(&[a, premul(px[0]), premul(px[1]), premul(px[2])]);
    }
    out
}

fn parse_wav_pcm(bytes: &[u8]) -> Option<(u16, u32, Vec<u8>)> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut channels = 1u16;
    let mut sample_rate = 44100u32;
    let mut bits_per_sample = 16u16;
    let mut pcm_data: Option<Vec<u8>> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        pos += 8;
        let end = pos.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end - pos >= 16 {
            let fmt_tag = u16::from_le_bytes(bytes[pos..pos + 2].try_into().ok()?);
            if fmt_tag != 1 {
                return None;
            }
            channels = u16::from_le_bytes(bytes[pos + 2..pos + 4].try_into().ok()?);
            sample_rate = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?);
            bits_per_sample = u16::from_le_bytes(bytes[pos + 14..pos + 16].try_into().ok()?);
        } else if id == b"data" {
            pcm_data = Some(bytes[pos..end].to_vec());
            break;
        }
        pos = end + (size & 1);
    }
    let pcm = pcm_data?;
    let rate_code: u16 = if sample_rate <= 6000 {
        0
    } else if sample_rate <= 12000 {
        1
    } else if sample_rate <= 24000 {
        2
    } else {
        3
    };
    let size_bit = u16::from(bits_per_sample >= 16);
    let stereo_bit = u16::from(channels >= 2);
    let flags = (rate_code << 2) | (size_bit << 1) | stereo_bit;
    let block_align = usize::from(1 + stereo_bit) * usize::from(1 + size_bit);
    let samples = (pcm.len() / block_align.max(1)) as u32;
    if samples == 0 {
        return None;
    }
    Some((flags, samples, pcm))
}

pub(crate) fn build_media_catalog(project: &Project) -> MediaExportCatalog {
    let mut catalog = MediaExportCatalog::default();
    let mut next_media_id = 1u16;
    for res in &project.resources {
        if matches!(
            res.media_type.as_str(),
            "application/x-neoflash-fla-source"
                | "application/x-neoflash-fla-provenance"
                | "application/x-neoflash-swf-source"
                | "application/x-neoflash-swf-provenance"
                | "application/x-neoflash-swc-source"
                | "application/x-neoflash-swc-provenance"
        ) {
            continue;
        }
        let Ok(raw) = res.decoded_bytes() else {
            continue;
        };
        if res.media_type.starts_with("image/") {
            if let Ok(reader) = ImageReader::new(Cursor::new(&raw)).with_guessed_format()
                && let Ok(decoded) = reader.decode()
            {
                let rgba = decoded.into_rgba8();
                if rgba.width() >= 1
                    && rgba.height() >= 1
                    && rgba.width() <= 4096
                    && rgba.height() <= 4096
                {
                    let media_id = next_media_id;
                    next_media_id = next_media_id.saturating_add(1);
                    let stream_name = format!("Media {media_id}");
                    let library_name = res.path.rsplit('/').next().unwrap_or(&res.id).to_owned();
                    let smoothing = find_smoothing_for_resource(project, &res.id);
                    let stream_bytes =
                        encode_native_bitmap_stream(rgba.width(), rgba.height(), rgba.as_raw());
                    catalog.bitmap_by_resource.insert(res.id.clone(), media_id);
                    catalog.bitmaps.push(PreparedBitmapMedia {
                        media_id,
                        stream_name,
                        library_name,
                        smoothing,
                        stream_bytes,
                    });
                }
            }
        } else if res.media_type.starts_with("audio/") {
            let media_id = next_media_id;
            let stream_name = format!("Media {media_id}");
            let library_name = res.path.rsplit('/').next().unwrap_or(&res.id).to_owned();
            if let Some((format_flags, sample_count, pcm_bytes)) = parse_wav_pcm(&raw) {
                next_media_id = next_media_id.saturating_add(1);
                catalog.sound_by_resource.insert(res.id.clone(), media_id);
                catalog.sounds.push(PreparedSoundMedia {
                    media_id,
                    stream_name,
                    library_name,
                    format_flags,
                    sample_count,
                    encoded_byte_count: pcm_bytes.len() as u32,
                    stream_bytes: pcm_bytes,
                });
            } else if res.media_type == "audio/mpeg" && !raw.is_empty() {
                next_media_id = next_media_id.saturating_add(1);
                catalog.sound_by_resource.insert(res.id.clone(), media_id);
                // Codec 2 = MP3, 44.1 kHz stereo (flags = (2 << 4) | (3 << 2) | 2 | 1 = 0x2f).
                let format_flags = 0x002f;
                let sample_count = ((raw.len() as u32) * 8).max(576);
                let encoded_byte_count = raw.len() as u32;
                catalog.sounds.push(PreparedSoundMedia {
                    media_id,
                    stream_name,
                    library_name,
                    format_flags,
                    sample_count,
                    encoded_byte_count,
                    stream_bytes: raw,
                });
            }
        }
    }
    catalog
}

#[derive(Clone, Debug)]
struct LegacyFolderEntry {
    id: u32,
    parent_id: Option<u32>,
    leaf_name: String,
    #[allow(dead_code)]
    full_path: String,
}

fn collect_legacy_folders(project: &Project) -> (Vec<LegacyFolderEntry>, BTreeMap<String, u32>) {
    let mut all_paths = std::collections::BTreeSet::new();
    for folder in &project.library_folders {
        let trimmed = folder.trim_matches('/');
        if !trimmed.is_empty() {
            let mut prefix = String::new();
            for seg in trimmed.split('/') {
                if seg.is_empty() {
                    continue;
                }
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(seg);
                all_paths.insert(prefix.clone());
            }
        }
    }
    for sym in &project.symbols {
        let trimmed = sym.folder.trim_matches('/');
        if !trimmed.is_empty() {
            let mut prefix = String::new();
            for seg in trimmed.split('/') {
                if seg.is_empty() {
                    continue;
                }
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(seg);
                all_paths.insert(prefix.clone());
            }
        }
    }

    let mut entries = Vec::new();
    let mut path_to_id = BTreeMap::new();
    for (idx, path) in all_paths.into_iter().enumerate() {
        let id = (idx + 1) as u32;
        path_to_id.insert(path.clone(), id);
        let (parent_id, leaf_name) = match path.rsplit_once('/') {
            Some((parent, leaf)) => (path_to_id.get(parent).copied(), leaf.to_owned()),
            None => (None, path.clone()),
        };
        entries.push(LegacyFolderEntry {
            id,
            parent_id,
            leaf_name,
            full_path: path,
        });
    }
    (entries, path_to_id)
}

fn write_item_id(w: &mut MfcWriter, id: Option<[u32; 2]>) {
    if let Some([a, b]) = id {
        w.u32(1);
        w.u32(a);
        w.u32(b);
    } else {
        w.u32(0);
    }
}

fn write_linkage(w: &mut MfcWriter, linkage_id: Option<&str>, class_name: Option<&str>) {
    w.u8(5); // linkage revision = 5
    let has_id = linkage_id.is_some_and(|s| !s.is_empty());
    let has_class = class_name.is_some_and(|s| !s.is_empty());
    let flags = if has_id || has_class { 5 } else { 0 };
    w.u32(flags);
    w.cstring(linkage_id.unwrap_or(""));
    w.cstring(""); // url
    w.cstring(class_name.unwrap_or(""));
    w.u8(0); // export_flags
    w.u32(0); // aux revision = 0
    w.cstring("");
    w.cstring("");
    w.u32(0); // take(4)
    write_item_id(w, None);
}

fn write_page_library_metadata(
    w: &mut MfcWriter,
    parent: Option<[u32; 2]>,
    item: Option<[u32; 2]>,
    linkage_id: Option<&str>,
    class_name: Option<&str>,
) {
    w.u32(5); // library_metadata revision = 5
    write_item_id(w, parent);
    write_item_id(w, item);
    w.u32(0); // attributes
    write_linkage(w, linkage_id, class_name);
}

fn detect_project_as_version(project: &Project, profile: FlaTargetProfile) -> &'static str {
    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            match fs.script.language {
                ScriptLanguage::ActionScript3 if profile.supports_as3() => return "3",
                ScriptLanguage::ActionScript1 => return "1",
                ScriptLanguage::ActionScript2 => return "2",
                _ => {}
            }
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        match os.script.language {
                            ScriptLanguage::ActionScript3 if profile.supports_as3() => return "3",
                            ScriptLanguage::ActionScript1 => return "1",
                            ScriptLanguage::ActionScript2 => return "2",
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if project.document_class.is_some() && profile.supports_as3() {
        return "3";
    }
    "2"
}

fn build_contents_stream(
    project: &Project,
    profile: FlaTargetProfile,
    scenes: &[&Symbol],
    library_symbols: &[(u32, &Symbol)],
    media: &MediaExportCatalog,
) -> Vec<u8> {
    let version: u8 = match profile {
        FlaTargetProfile::LegacyFlash8 => 0x3f,
        _ => 0x47,
    };
    let (folders, folder_map) = collect_legacy_folders(project);
    let mut w = MfcWriter::new();

    w.u8(version);
    w.u32(1); // compatibility_marker
    w.u32(0); // ole_count
    w.u8(0);
    w.u8(0);
    for threshold in [0x21_u8, 0x2b, 0x3b, 0x40, 0x44] {
        if version >= threshold {
            w.u32(0);
        }
    }

    // 1. Scene list
    let mut first_scene_obj_ref = 0u16;
    for (idx, scene) in scenes.iter().enumerate() {
        let obj_ref = w.begin_object("CDocumentPage");
        if idx == 0 {
            first_scene_obj_ref = obj_ref;
        }
        w.u8(5); // descriptor revision = 5
        let stream_name = format!("Page {}", idx + 1);
        w.compact_stream_name(&stream_name, true);
        w.cstring(&scene.name);
        let scene_lib_id = (idx + 1) as u32;
        w.u32(scene_lib_id);
        w.u8(2); // MovieClip
        write_page_library_metadata(&mut w, None, Some([0, scene_lib_id]), None, None);
    }
    w.write_null_ref();

    w.u16((scenes.len() + 1) as u16); // next_scene_id
    w.u16(1); // next_item_tag
    if first_scene_obj_ref != 0 {
        w.write_object_ref(first_scene_obj_ref);
    } else {
        w.write_null_ref();
    }

    // 2. Symbol list
    for (idx, (lib_id, sym)) in library_symbols.iter().enumerate() {
        w.begin_object("CDocumentPage");
        w.u8(5); // descriptor revision = 5
        let stream_name = format!("Symbol {}", idx + 1);
        w.compact_stream_name(&stream_name, true);
        w.cstring(&sym.name);
        w.u32(*lib_id);
        let sym_type = match sym.kind {
            SymbolKind::Graphic => 0,
            SymbolKind::Button => 1,
            SymbolKind::MovieClip => 2,
        };
        w.u8(sym_type);
        let parent_folder = folder_map
            .get(sym.folder.trim_matches('/'))
            .copied()
            .map(|fid| [1, fid]);
        write_page_library_metadata(
            &mut w,
            parent_folder,
            Some([2, *lib_id]),
            sym.linkage_id.as_deref(),
            sym.class_name.as_deref(),
        );
    }
    w.write_null_ref();
    w.u16((library_symbols.len() + 1) as u16); // library_counter

    // 3. Media list
    for bmp in &media.bitmaps {
        w.begin_object("CMediaBits");
        w.u8(4); // media descriptor revision = 4
        w.compact_stream_name(&bmp.stream_name, false);
        w.cstring(&bmp.library_name);
        w.u16(bmp.media_id);
        w.cstring(""); // revision >= 2
        w.u32(0); // revision >= 3
        // library_metadata revision 3
        w.u32(3);
        w.u32(0); // parent = 0
        w.u32(u32::from(bmp.media_id)); // item
        w.u32(0); // attributes
        // CMediaBits subtype revision 3
        w.u8(3);
        w.u8(1); // lossless compression
        w.u8(80); // quality
        w.u8(u8::from(bmp.smoothing));
        w.i32(0); // encoded_byte_count = 0 (native raster stream)
    }
    for snd in &media.sounds {
        w.begin_object("CMediaSound");
        w.u8(4); // media descriptor revision = 4
        w.compact_stream_name(&snd.stream_name, false);
        w.cstring(&snd.library_name);
        w.u16(snd.media_id);
        w.cstring(""); // revision >= 2
        w.u32(0); // revision >= 3
        // library_metadata revision 3
        w.u32(3);
        w.u32(0); // parent = 0
        w.u32(u32::from(snd.media_id)); // item
        w.u32(0); // attributes
        // CMediaSound subtype revision 6
        w.u8(6);
        w.u16(snd.format_flags);
        w.u32(snd.sample_count);
        w.i16(0); // export_format
        w.i16(0); // export_bits
        w.u32(snd.encoded_byte_count);
        w.u8(0); // imported_mp3_quality
    }
    w.write_null_ref();
    let total_media = media.bitmaps.len() + media.sounds.len();
    w.u16((total_media + 1) as u16); // media_counter

    // 4. Document settings
    w.u16(0); // ruler_unit
    w.u8(0); // grid_display_mode
    let width_twips = (project.width.max(1) as i32).saturating_mul(20);
    let height_twips = (project.height.max(1) as i32).saturating_mul(20);
    for bound in [0_i32, width_twips, 0_i32, height_twips] {
        w.i32(bound);
    }
    w.u32(360 << 16); // grid_spacing_fixed16_twips (18 px)
    w.zeros(3); // preview_and_ruler_flags
    w.zeros(2); // view_play_and_printer_flags
    for margin in [0_i32; 4] {
        w.i32(margin);
    }
    w.zeros(4); // print_flags
    w.i32(65536); // print_scale_fixed16 = 1.0
    w.u16(0); // printer_mode length
    w.u16(0); // printer_names length
    w.u32(u32::from_le_bytes(project.background)); // background_rgba_word
    w.u32(0x00c0_c0c0); // grid_rgba_word
    w.u32(project.fps.max(1).saturating_mul(65536)); // frame_rate_fixed16
    w.zeros(3); // storyboard_flags
    w.i32(0); // storyboard_margin_twips

    // 5. Extensions
    // 5a. publish(version) (version >= 0x2d)
    w.u16(0); // document_properties count = 0
    w.u32(1); // 1 publish profile
    w.u16(2); // 2 properties in profile 0
    w.cstring("Vector::ActionScriptVersion");
    w.cstring(detect_project_as_version(project, profile));
    w.cstring("Vector::Invisible Layer");
    w.cstring("1");
    w.u32(0); // first_word
    w.cstring(""); // first_string
    w.cstring(""); // second_string
    w.u32(0); // second_word

    // 5b. extension_properties (version >= 0x15)
    w.u16(0);
    // 5c. two color_list(0) (version >= 0x16)
    w.u16(0);
    w.u8(0);
    w.u16(0);
    w.u8(0);
    // 5d. library_view (version >= 0x18)
    w.u32(1); // revision = 1
    w.zeros(28);
    // 5e. folders (version >= 0x19)
    w.u32(1); // folder collection header
    w.u32(folders.len() as u32);
    for folder in &folders {
        w.u32(4); // folder revision = 4
        w.cstring(&folder.leaf_name);
        // library_metadata revision 4
        w.u32(4);
        write_item_id(&mut w, folder.parent_id.map(|pid| [1, pid]));
        write_item_id(&mut w, Some([1, folder.id]));
        w.u32(0); // attributes
    }
    // 5f. object_properties (version >= 0x1a & >= 0x2d)
    w.u16(0); // document object_properties
    w.u16(0); // profile 0 object_properties
    // 5g. version >= 0x1b .. 0x3f
    w.u16(1); // next symbol identifier (0x1b)
    w.zeros(7); // guides color and 3 flags (0x1c)
    w.u32(0); // font library header (0x1d)
    w.u32(0); // embedded font count = 0 (0x1d)
    w.cstring(""); // shared_library_url (0x1e)
    w.zeros(14); // grid flags, accuracy, Y spacing (0x1f)
    w.cstring(""); // template description (0x22)
    w.cstring(""); // template filename (0x22)
    w.u32(0); // preview frame (0x23)
    w.zeros(8); // pixel grid and snap (0x24)
    w.u32(0); // document_accessibility revision = 0 (0x26)
    w.u32((total_media + 1) as u32); // next media identifier (0x27)
    w.zeros(16); // (0x2e)
    w.u8(0); // (0x30)
    w.u8(0); // persistent_data presence = 0 (0x32)
    w.u32(0); // (0x33)
    w.u32(0); // document_type (0x34)
    w.u32(0); // language selector presence = 0 (0x35)
    w.u32(0); // stage-language presence = 0 (0x36)
    w.u32(0); // (0x38)
    w.cstring(""); // metadata_xml (0x3a)
    w.cstring(""); // (0x3c)
    w.u32(0); // (0x3d)
    w.u32(0); // postprocessor presence = 0 (0x3e)
    w.zeros(8); // timeline height and label width (0x3f)

    if version >= 0x42 {
        // shared_libraries: revision 0, count 0
        w.u8(0);
        w.u32(0);
    }
    if version >= 0x43 {
        // writer_identity: 4 words + length 0
        for word in [10_u16, 0, 0, 0] {
            w.u16(word);
        }
        w.u16(0);
    }
    if version >= 0x45 {
        w.zeros(8);
    }
    if version >= 0x46 {
        w.zeros(8);
    }
    if version >= 0x47 {
        w.zeros(4);
    }

    w.buf
}

fn write_legacy_cfb_archive(
    project: &Project,
    profile: FlaTargetProfile,
) -> Result<Vec<u8>, ExportError> {
    let scene_ids: Vec<&str> = if project.scenes.is_empty() {
        vec![project.root.as_str()]
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };

    let mut scenes = Vec::new();
    for scene_id in &scene_ids {
        if let Some(sym) = project.symbol(scene_id) {
            scenes.push(sym);
        }
    }
    if scenes.is_empty() {
        return Err(ExportError::MissingRootSymbol(project.root.clone()));
    }

    let mut library_symbols = Vec::new();
    let mut symbol_lib_ids = BTreeMap::new();
    let mut symbol_kinds = BTreeMap::new();
    let mut next_lib_id = 100u32;
    for sym in &project.symbols {
        symbol_kinds.insert(sym.id.clone(), sym.kind);
        if scene_ids.contains(&sym.id.as_str()) {
            continue;
        }
        let lib_id = next_lib_id;
        next_lib_id = next_lib_id.saturating_add(1);
        symbol_lib_ids.insert(sym.id.clone(), lib_id);
        library_symbols.push((lib_id, sym));
    }

    let media = build_media_catalog(project);
    let contents_bytes = build_contents_stream(project, profile, &scenes, &library_symbols, &media);

    let cursor = Cursor::new(Vec::new());
    let mut comp = cfb::CompoundFile::create_with_version(cfb::Version::V3, cursor)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;

    {
        let mut stream = comp
            .create_stream("/Contents")
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        stream.write_all(&contents_bytes)?;
    }

    for (idx, scene) in scenes.iter().enumerate() {
        let stream_path = format!("/Page {}", idx + 1);
        let bytes = build_timeline_stream(&scene.timeline, &symbol_lib_ids, &symbol_kinds, &media);
        let mut stream = comp
            .create_stream(&stream_path)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        stream.write_all(&bytes)?;
    }

    for (idx, (_, sym)) in library_symbols.iter().enumerate() {
        let stream_path = format!("/Symbol {}", idx + 1);
        let bytes = build_timeline_stream(&sym.timeline, &symbol_lib_ids, &symbol_kinds, &media);
        let mut stream = comp
            .create_stream(&stream_path)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        stream.write_all(&bytes)?;
    }

    for bmp in &media.bitmaps {
        let stream_path = format!("/{}", bmp.stream_name);
        let mut stream = comp
            .create_stream(&stream_path)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        stream.write_all(&bmp.stream_bytes)?;
    }

    for snd in &media.sounds {
        let stream_path = format!("/{}", snd.stream_name);
        let mut stream = comp
            .create_stream(&stream_path)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        stream.write_all(&snd.stream_bytes)?;
    }

    comp.flush()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(comp.into_inner().into_inner())
}

fn matrix_to_words(m: [f64; 6]) -> [i32; 6] {
    let clean = |v: f64, scale: f64| -> i32 {
        if v.is_finite() {
            (v * scale)
                .round()
                .clamp(f64::from(i32::MIN / 2), f64::from(i32::MAX / 2)) as i32
        } else {
            0
        }
    };
    [
        clean(m[0], 65536.0),
        clean(m[1], 65536.0),
        clean(m[2], 65536.0),
        clean(m[3], 65536.0),
        clean(m[4], 20.0),
        clean(m[5], 20.0),
    ]
}

fn color_to_word(c: Color) -> u32 {
    u32::from_le_bytes(c)
}

#[derive(Clone)]
struct EncodedFill {
    color: Color,
    gradient: Option<GradientPaint>,
}

#[derive(Clone)]
struct EncodedStroke {
    color: Color,
    width_twips: u16,
    gradient: Option<GradientPaint>,
}

#[derive(Clone)]
struct EncodedEdge {
    stroke: u16,
    fill_0: u16,
    fill_1: u16,
    start: [i32; 2],
    control: Option<[i32; 2]>,
    end: [i32; 2],
}

fn to_fla_units(pt: [f64; 2]) -> [i32; 2] {
    let conv = |v: f64| -> i32 {
        if v.is_finite() {
            (v * 5120.0)
                .round()
                .clamp(f64::from(i32::MIN / 4), f64::from(i32::MAX / 4)) as i32
        } else {
            0
        }
    };
    [conv(pt[0]), conv(pt[1])]
}

fn transform_point(m: [f64; 6], pt: [f64; 2]) -> [f64; 2] {
    [
        m[0] * pt[0] + m[2] * pt[1] + m[4],
        m[1] * pt[0] + m[3] * pt[1] + m[5],
    ]
}

fn multiply_matrices(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

type FlaEdgeSeg = ([i32; 2], Option<[i32; 2]>, [i32; 2]);
type FlaSubpath = (Vec<FlaEdgeSeg>, bool);

fn append_commands_edges(
    commands: &[PathCommand],
    m: [f64; 6],
    fill_index: u16,
    stroke_index: u16,
    force_close_fill: bool,
    out_edges: &mut Vec<EncodedEdge>,
) {
    // Split into subpaths and convert cubics to quadratics.
    let mut subpaths: Vec<FlaSubpath> = Vec::new();
    let mut current_pt = [0.0_f64, 0.0_f64];
    let mut start_pt = [0.0_f64, 0.0_f64];
    let mut current_segs: Vec<FlaEdgeSeg> = Vec::new();
    let mut has_start = false;

    let push_seg =
        |segs: &mut Vec<FlaEdgeSeg>, p0: [f64; 2], ctrl: Option<[f64; 2]>, p1: [f64; 2]| {
            let s = to_fla_units(transform_point(m, p0));
            let e = to_fla_units(transform_point(m, p1));
            if s == e {
                return;
            }
            let c = ctrl.map(|cp| to_fla_units(transform_point(m, cp)));
            segs.push((s, c, e));
        };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if !current_segs.is_empty() {
                    subpaths.push((std::mem::take(&mut current_segs), false));
                }
                current_pt = [x, y];
                start_pt = [x, y];
                has_start = true;
            }
            PathCommand::LineTo { x, y } => {
                if !has_start {
                    start_pt = current_pt;
                    has_start = true;
                }
                push_seg(&mut current_segs, current_pt, None, [x, y]);
                current_pt = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if !has_start {
                    start_pt = current_pt;
                    has_start = true;
                }
                push_seg(&mut current_segs, current_pt, Some([cx, cy]), [x, y]);
                current_pt = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if !has_start {
                    start_pt = current_pt;
                    has_start = true;
                }
                // Approximate cubic with two quadratics at t = 0.5.
                let p0 = current_pt;
                let p1 = [cx1, cy1];
                let p2 = [cx2, cy2];
                let p3 = [x, y];
                let q0 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
                let q1 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
                let q2 = [0.5 * (p2[0] + p3[0]), 0.5 * (p2[1] + p3[1])];
                let r0 = [0.5 * (q0[0] + q1[0]), 0.5 * (q0[1] + q1[1])];
                let r1 = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                let mid = [0.5 * (r0[0] + r1[0]), 0.5 * (r0[1] + r1[1])];
                push_seg(&mut current_segs, p0, Some(q0), mid);
                push_seg(&mut current_segs, mid, Some(q2), p3);
                current_pt = p3;
            }
            PathCommand::Close => {
                if has_start {
                    push_seg(&mut current_segs, current_pt, None, start_pt);
                    current_pt = start_pt;
                }
                if !current_segs.is_empty() {
                    subpaths.push((std::mem::take(&mut current_segs), true));
                }
            }
        }
    }
    if !current_segs.is_empty() {
        subpaths.push((current_segs, false));
    }

    for (segs, explicit_close) in subpaths {
        if segs.is_empty() {
            continue;
        }
        if fill_index != 0 {
            let mut fill_segs = segs.clone();
            let first_pt = fill_segs[0].0;
            let last_pt = fill_segs.last().unwrap().2;
            if last_pt != first_pt {
                fill_segs.push((last_pt, None, first_pt));
            }
            if fill_segs.len() >= 2 || fill_segs[0].1.is_some() {
                for (s, c, e) in fill_segs {
                    out_edges.push(EncodedEdge {
                        stroke: 0,
                        fill_0: 0,
                        fill_1: fill_index,
                        start: s,
                        control: c,
                        end: e,
                    });
                }
            }
        }
        if stroke_index != 0 {
            let mut stroke_segs = segs;
            if (explicit_close || force_close_fill)
                && let (Some(first), Some(last)) = (stroke_segs.first(), stroke_segs.last())
                && first.0 != last.2
            {
                stroke_segs.push((last.2, None, first.0));
            }
            for (s, c, e) in stroke_segs {
                out_edges.push(EncodedEdge {
                    stroke: stroke_index,
                    fill_0: 0,
                    fill_1: 0,
                    start: s,
                    control: c,
                    end: e,
                });
            }
        }
    }
}

fn collect_element_geometry(
    element: &Element,
    parent_matrix: [f64; 6],
    fills: &mut Vec<EncodedFill>,
    strokes: &mut Vec<EncodedStroke>,
    edges: &mut Vec<EncodedEdge>,
) {
    let m = multiply_matrices(
        parent_matrix,
        neoflash_core::transform_matrix(element.transform),
    );
    let mut add_fill = |color: Color, gradient: Option<GradientPaint>| -> u16 {
        if color[3] == 0 && gradient.is_none() {
            return 0;
        }
        let grad = gradient.map(|mut g| {
            g.matrix = multiply_matrices(m, g.matrix);
            g
        });
        fills.push(EncodedFill {
            color,
            gradient: grad,
        });
        fills.len() as u16
    };
    let mut add_stroke = |color: Color, width: f64, gradient: Option<GradientPaint>| -> u16 {
        if width <= 0.0 || (color[3] == 0 && gradient.is_none()) {
            return 0;
        }
        let grad = gradient.map(|mut g| {
            g.matrix = multiply_matrices(m, g.matrix);
            g
        });
        let width_twips = (width * 20.0).round().clamp(1.0, 65535.0) as u16;
        strokes.push(EncodedStroke {
            color,
            width_twips,
            gradient: grad,
        });
        strokes.len() as u16
    };

    match &element.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let f_idx = add_fill(*fill, None);
            let s_idx = add_stroke(*stroke, *stroke_width, None);
            let cmds = vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: *width, y: 0.0 },
                PathCommand::LineTo {
                    x: *width,
                    y: *height,
                },
                PathCommand::LineTo { x: 0.0, y: *height },
                PathCommand::Close,
            ];
            append_commands_edges(&cmds, m, f_idx, s_idx, true, edges);
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let f_idx = add_fill(*fill, None);
            let s_idx = add_stroke(*stroke, *stroke_width, None);
            let rx = *width * 0.5;
            let ry = *height * 0.5;
            let cx = rx;
            let cy = ry;
            let k = std::f64::consts::FRAC_1_SQRT_2;
            let t = (std::f64::consts::PI / 8.0).tan();
            let cmds = vec![
                PathCommand::MoveTo { x: cx + rx, y: cy },
                PathCommand::QuadraticTo {
                    cx: cx + rx,
                    cy: cy + ry * t,
                    x: cx + rx * k,
                    y: cy + ry * k,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx * t,
                    cy: cy + ry,
                    x: cx,
                    y: cy + ry,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * t,
                    cy: cy + ry,
                    x: cx - rx * k,
                    y: cy + ry * k,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx,
                    cy: cy + ry * t,
                    x: cx - rx,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx,
                    cy: cy - ry * t,
                    x: cx - rx * k,
                    y: cy - ry * k,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * t,
                    cy: cy - ry,
                    x: cx,
                    y: cy - ry,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx * t,
                    cy: cy - ry,
                    x: cx + rx * k,
                    y: cy - ry * k,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx,
                    cy: cy - ry * t,
                    x: cx + rx,
                    y: cy,
                },
                PathCommand::Close,
            ];
            append_commands_edges(&cmds, m, f_idx, s_idx, true, edges);
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, None);
            let s_idx = add_stroke(*stroke, *stroke_width, None);
            append_commands_edges(commands, m, f_idx, s_idx, f_idx != 0, edges);
        }
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, fill_gradient.clone());
            let s_idx = add_stroke(*stroke, *stroke_width, stroke_gradient.clone());
            append_commands_edges(commands, m, f_idx, s_idx, f_idx != 0, edges);
        }
        _ => {}
    }
}

fn write_binary_fill(w: &mut MfcWriter, fill: &EncodedFill) {
    if let Some(grad) = &fill.gradient {
        w.u32(color_to_word(fill.color));
        let flags: u8 = match grad.kind {
            GradientKind::Linear => 0x10,
            GradientKind::Radial => 0x12,
        };
        w.u8(flags);
        w.u8(0); // secondary_flags
        for word in matrix_to_words(grad.matrix) {
            w.i32(word);
        }
        let stops: Vec<&GradientStop> = grad.stops.iter().take(15).collect();
        w.u8(stops.len() as u8);
        // BinaryGradientExtension (shape_revision >= 3)
        let focal = (grad.focal_point_ratio.clamp(-1.0, 1.0) * 255.0).round() as i32;
        w.i32(focal);
        w.u32(0); // pad + RGB
        for stop in stops {
            w.u8(stop.ratio);
            w.u32(color_to_word(stop.color));
        }
    } else {
        w.u32(color_to_word(fill.color));
        w.u8(0); // plain solid fill
        w.u8(0);
    }
}

fn write_frame_shape(w: &mut MfcWriter, elements: &[Element]) {
    let mut fills = Vec::new();
    let mut strokes = Vec::new();
    let mut edges = Vec::new();
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for el in elements {
        if matches!(
            el.kind,
            ElementKind::Rectangle { .. }
                | ElementKind::Ellipse { .. }
                | ElementKind::Path { .. }
                | ElementKind::PaintedPath { .. }
        ) && el.scripts.is_empty()
        {
            collect_element_geometry(el, identity, &mut fills, &mut strokes, &mut edges);
        }
    }

    w.u8(4); // shape_revision = 4
    for word in [65536_i32, 0, 0, 65536, 0, 0] {
        w.i32(word);
    }
    w.u8(4); // data_revision = 4
    w.u32(0); // metadata

    w.u16(fills.len() as u16);
    for fill in &fills {
        write_binary_fill(w, fill);
    }

    w.u16(strokes.len() as u16);
    for stroke in &strokes {
        w.u32(color_to_word(stroke.color));
        w.u16(stroke.width_twips);
        w.u16(0); // solid stroke style
        w.u16(0);
        // BinaryStrokeExtension (shape_revision >= 3)
        w.u8(0); // pixel_hinting
        w.u8(0); // scale_mode = normal
        w.u8(1); // cap_style = round
        w.u8(1); // join_style = round
        w.u16(3 * 256); // miter_limit_word
        write_binary_fill(
            w,
            &EncodedFill {
                color: stroke.color,
                gradient: stroke.gradient.clone(),
            },
        );
    }

    let mut prev = [0_i32, 0_i32];
    let mut cur_styles = (0_u16, 0_u16, 0_u16);
    for edge in &edges {
        let next_styles = (edge.stroke, edge.fill_0, edge.fill_1);
        let style_changed = next_styles != cur_styles;
        let start_delta = [
            edge.start[0].wrapping_sub(prev[0]),
            edge.start[1].wrapping_sub(prev[1]),
        ];
        let start_mode: u8 = if start_delta == [0, 0] { 0 } else { 2 };
        let ctrl_mode: u8 = if edge.control.is_some() { 2 } else { 0 };
        let end_mode: u8 = 2;
        let flags: u8 = (if style_changed { 0x40 } else { 0 })
            | (end_mode << 4)
            | (ctrl_mode << 2)
            | start_mode;
        w.u8(flags);
        if style_changed {
            w.u16(edge.stroke);
            w.u16(edge.fill_0);
            w.u16(edge.fill_1);
            cur_styles = next_styles;
        }
        if start_mode == 2 {
            w.i32(start_delta[0]);
            w.i32(start_delta[1]);
        }
        if let Some(ctrl) = edge.control {
            let ctrl_delta = [
                ctrl[0].wrapping_sub(edge.start[0]),
                ctrl[1].wrapping_sub(edge.start[1]),
            ];
            w.i32(ctrl_delta[0]);
            w.i32(ctrl_delta[1]);
        }
        let end_delta = [
            edge.end[0].wrapping_sub(edge.start[0]),
            edge.end[1].wrapping_sub(edge.start[1]),
        ];
        w.i32(end_delta[0]);
        w.i32(end_delta[1]);
        if edge.control.is_none() {
            // line_flags on straight edges for shape_revision >= 4
            w.u8(0);
        }
        prev = edge.end;
    }
    w.u8(0); // end of edge stream
}

fn pack_filter_rgba(rgb: u32, alpha: f64) -> u32 {
    let r = ((rgb >> 16) & 0xff) as u8;
    let g = ((rgb >> 8) & 0xff) as u8;
    let b = (rgb & 0xff) as u8;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    u32::from_le_bytes([r, g, b, a])
}

#[allow(clippy::too_many_arguments)]
fn write_shadow_block(
    w: &mut MfcWriter,
    color_rgba: u32,
    distance: f64,
    blur_x: f64,
    blur_y: f64,
    angle_deg: f64,
    inner: bool,
    knockout: bool,
    quality: u8,
    strength: f64,
    hide_object: bool,
) {
    w.u8(4); // shadow settings revision = 4
    w.u8(1); // filter_state revision = 1
    w.u32(1); // enabled_word = 1
    w.u32(color_rgba);
    w.f32(distance as f32);
    w.f32(blur_x as f32);
    w.f32(blur_y as f32);
    w.f32((angle_deg as f32).to_radians());
    w.u32(u32::from(inner));
    w.u32(u32::from(knockout));
    w.u32(u32::from(quality.max(1)));
    w.u32((strength.clamp(0.0, 255.0) * 100.0).round() as u32);
    w.u32(u32::from(hide_object));
}

fn write_filters(w: &mut MfcWriter, filters: &[FilterSpec]) {
    if filters.is_empty() {
        w.u8(0);
        return;
    }
    w.u8(1);
    w.u32(filters.len() as u32);
    for filter in filters {
        match filter {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                hide_object,
            } => {
                w.u8(0); // DropShadow
                write_shadow_block(
                    w,
                    pack_filter_rgba(*color, *alpha),
                    *distance,
                    *blur_x,
                    *blur_y,
                    *angle,
                    *inner,
                    *knockout,
                    *quality,
                    *strength,
                    *hide_object,
                );
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            } => {
                w.u8(1); // Blur
                w.u8(2); // blur revision = 2
                w.f32(*blur_x as f32);
                w.f32(*blur_y as f32);
                w.u8(1); // filter_state revision = 1
                w.u32(1); // enabled = 1
                w.u32(u32::from((*quality).max(1)));
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
            } => {
                w.u8(2); // Glow
                w.u8(3); // glow revision = 3 (delegates to shadow)
                write_shadow_block(
                    w,
                    pack_filter_rgba(*color, *alpha),
                    0.0,
                    *blur_x,
                    *blur_y,
                    45.0,
                    *inner,
                    *knockout,
                    *quality,
                    *strength,
                    false,
                );
            }
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => {
                w.u8(3); // Bevel
                w.u8(3); // bevel revision = 3
                let is_inner = placement == "inner";
                let is_full = placement == "full";
                write_shadow_block(
                    w,
                    pack_filter_rgba(*shadow_color, *shadow_alpha),
                    *distance,
                    *blur_x,
                    *blur_y,
                    *angle,
                    is_inner,
                    *knockout,
                    *quality,
                    *strength,
                    false,
                );
                w.u32(pack_filter_rgba(*highlight_color, *highlight_alpha));
                w.u32(u32::from(is_full));
            }
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => {
                w.u8(4); // GradientGlow
                w.u8(1); // gradient_revision = 1
                let is_inner = placement == "inner";
                let is_full = placement == "full";
                write_shadow_block(
                    w,
                    0xff00_0000,
                    *distance,
                    *blur_x,
                    *blur_y,
                    *angle,
                    is_inner,
                    *knockout,
                    *quality,
                    *strength,
                    false,
                );
                let count = colors.len().min(alphas.len()).min(ratios.len()).min(15);
                w.u32(count as u32);
                w.u32(0); // gradient_value
                w.u32(u32::from(is_full));
                for i in 0..count {
                    w.u32(u32::from(ratios[i]));
                    w.u32(pack_filter_rgba(colors[i], alphas[i]));
                }
            }
            FilterSpec::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => {
                w.u8(7); // GradientBevel
                w.u8(1); // outer revision = 1
                w.u8(1); // gradient_revision = 1
                let is_inner = placement == "inner";
                let is_full = placement == "full";
                write_shadow_block(
                    w,
                    0xff00_0000,
                    *distance,
                    *blur_x,
                    *blur_y,
                    *angle,
                    is_inner,
                    *knockout,
                    *quality,
                    *strength,
                    false,
                );
                let count = colors.len().min(alphas.len()).min(ratios.len()).min(15);
                w.u32(count as u32);
                w.u32(0); // gradient_value
                w.u32(u32::from(is_full));
                for i in 0..count {
                    w.u32(u32::from(ratios[i]));
                    w.u32(pack_filter_rgba(colors[i], alphas[i]));
                }
            }
            FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {
                w.u8(6); // AdjustColor
                w.u8(1); // revision = 1
                w.u8(1); // filter_state revision = 1
                w.u32(1); // enabled = 1
                w.f32(0.0); // brightness
                w.f32(0.0); // contrast
                w.f32(0.0); // saturation
                w.f32(0.0); // hue
            }
        }
    }
}

fn encode_blend_mode(blend: BlendMode) -> u8 {
    match blend {
        BlendMode::Normal => 1,
        BlendMode::Layer => 2,
        BlendMode::Multiply => 3,
        BlendMode::Screen => 4,
        BlendMode::Lighten => 5,
        BlendMode::Darken => 6,
        BlendMode::Difference => 7,
        BlendMode::Add => 8,
        BlendMode::Subtract => 9,
        BlendMode::Invert => 10,
        BlendMode::Alpha => 11,
        BlendMode::Erase => 12,
        BlendMode::Overlay => 13,
        BlendMode::Hardlight => 14,
    }
}

fn write_actions(w: &mut MfcWriter, script_source: Option<&str>) {
    w.u32(0); // actions revision = 0
    if let Some(src) = script_source.filter(|s| !s.is_empty()) {
        w.u32(1); // mode = 1 (source)
        w.cstring(src);
    } else {
        w.u32(2); // mode = 2 (empty)
    }
}

fn write_text_style(w: &mut MfcWriter, format: &TextFormat, alpha: f64) {
    w.u8(15); // text_style revision = 15
    let size_twips = (format.size * 20.0).round().clamp(20.0, 65535.0) as u16;
    w.u16(size_twips);
    w.cstring(&format.font); // font_family
    w.cstring(&format.font); // font_face (revision >= 14)
    w.u32(0); // font_flags (revision >= 14)
    w.u32(pack_filter_rgba(format.color, alpha));
    w.u8(0); // pitch_and_family
    w.u8(0); // char_set
    w.u8(u8::from(format.bold));
    w.u8(u8::from(format.italic));
    w.u8(0); // use_screen_spacing
    w.u8(u8::from(format.kerning.unwrap_or(false)));
    w.u8(0); // character_position
    let align_code: u8 = match format.align {
        TextAlign::Left => 0,
        TextAlign::Right => 1,
        TextAlign::Center => 2,
    };
    w.u8(align_code);
    let to_i16_twips = |v: f64| -> i16 {
        (v * 20.0)
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
    };
    w.i16(to_i16_twips(format.leading));
    w.i16(to_i16_twips(format.indent));
    w.i16(to_i16_twips(format.left_margin));
    w.i16(to_i16_twips(format.right_margin));
    w.i16(to_i16_twips(format.letter_spacing));
    w.cstring(&format.url); // url (revision >= 5)
    w.u8(0); // vertical (revision >= 6)
    w.u8(0); // right_to_left (revision >= 6)
    w.u8(0); // rotation (revision >= 6)
    w.u8(0); // bitmap_rendering (revision >= 8)
    w.cstring(&format.target); // target (revision >= 7)
    // anti_alias (revision >= 11): antialias_revision = 2, byte = 0, thickness = 0.0, sharpness = 0.0
    w.u8(2);
    w.u8(0);
    w.f32(0.0);
    w.f32(0.0);
    w.cstring(&format.url); // url_override (revision >= 12)
}

fn write_cpic_obj_base_header(w: &mut MfcWriter) {
    w.u8(4); // CPicObj revision = 4
    w.u8(0); // flags = 0
}

fn write_cpic_obj_base_footer(w: &mut MfcWriter) {
    w.write_null_ref(); // end of child object list
    w.i32(0); // registration X
    w.i32(0); // registration Y
    w.u8(0); // drawing_object_flags
    w.u8(0); // cache_as_bitmap
}

fn write_display_element(
    w: &mut MfcWriter,
    element: &Element,
    symbol_lib_ids: &BTreeMap<String, u32>,
    symbol_kinds: &BTreeMap<String, SymbolKind>,
    media: &MediaExportCatalog,
) {
    match &element.kind {
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let Some(&lib_id) = symbol_lib_ids.get(symbol_id) else {
                return;
            };
            let sym_kind = symbol_kinds
                .get(symbol_id)
                .copied()
                .unwrap_or(SymbolKind::MovieClip);
            let class_name = match sym_kind {
                SymbolKind::MovieClip => "CPicSprite",
                SymbolKind::Button => "CPicButton",
                SymbolKind::Graphic => "CPicSymbol",
            };
            w.begin_object(class_name);
            write_cpic_obj_base_header(w);
            write_cpic_obj_base_footer(w);

            w.u8(17); // CPicSymbol revision = 17
            for word in matrix_to_words(neoflash_core::transform_matrix(element.transform)) {
                w.i32(word);
            }
            let (first_frame, loop_mode): (u16, u16) = match *playback {
                InstancePlayback::Independent | InstancePlayback::Loop => (0, 0),
                InstancePlayback::LoopFrom(f) => (f.min(65535) as u16, 0),
                InstancePlayback::PlayOnce => (0, 1),
                InstancePlayback::PlayOnceFrom(f) => (f.min(65535) as u16, 1),
                InstancePlayback::SingleFrame(f) => (f.min(65535) as u16, 2),
            };
            w.u16(first_frame);
            w.u16(loop_mode);

            // color_transform (revision 1)
            w.u8(1);
            let mut ct = element.transform.color_transform;
            if (element.transform.alpha - 1.0).abs() > 1e-6 && ct.is_identity() {
                ct.multipliers[3] = element.transform.alpha;
            }
            let has_ct = !ct.is_identity();
            let alpha_mul = (ct.multipliers[3] * 256.0).round().clamp(-32768.0, 32767.0) as i16;
            let alpha_off = ct.offsets[3].round().clamp(-32768.0, 32767.0) as i16;
            w.i16(alpha_mul);
            w.i16(alpha_off);
            for ch in 0..3 {
                let mul = (ct.multipliers[ch] * 256.0)
                    .round()
                    .clamp(-32768.0, 32767.0) as i16;
                let off = ct.offsets[ch].round().clamp(-32768.0, 32767.0) as i16;
                w.i16(mul);
                w.i16(off);
            }
            // effect_type = 3 (Advanced) when non-identity so color_transform() reconstructs all channels
            w.u16(if has_ct { 3 } else { 0 });
            w.i16(0); // effect_percent
            w.u32(0); // effect_rgba

            w.cstring(&element.name); // legacy_name (revision >= 6)
            w.u32(lib_id);
            w.u8(0); // timeline_effect (revision >= 11)
            w.u8(0); // persistent_data (revision >= 12)
            w.u8(0); // is_extensible (revision >= 14)
            write_filters(w, &element.filters); // filters (revision >= 15)
            w.u8(encode_blend_mode(element.blend_mode)); // blend_mode (revision >= 16)
            w.u16(0); // selected_effect_index (revision >= 17)

            let script_src = if element.scripts.is_empty() {
                None
            } else {
                let mut blocks = Vec::with_capacity(element.scripts.len());
                for os in &element.scripts {
                    let trimmed = os.script.source.trim();
                    if trimmed.starts_with("on (")
                        || trimmed.starts_with("on(")
                        || trimmed.starts_with("onClipEvent")
                    {
                        blocks.push(trimmed.to_owned());
                    } else {
                        let header = match os.event {
                            ObjectEvent::Click => "on (release) {\n",
                            ObjectEvent::Load => "onClipEvent (load) {\n",
                            ObjectEvent::EnterFrame => "onClipEvent (enterFrame) {\n",
                        };
                        blocks.push(format!("{header}{trimmed}\n}}"));
                    }
                }
                Some(blocks.join("\n"))
            };
            let script_ref = script_src.as_deref();
            match sym_kind {
                SymbolKind::MovieClip => {
                    w.u8(7); // CPicSprite revision = 7
                    write_actions(w, script_ref);
                    w.cstring(&element.name);
                    // component_properties: revision = 1, count = 0
                    w.u8(1);
                    w.u32(0);
                    // accessibility: revision = 0
                    w.u32(0);
                    // property_inspector_state (rev >= 5)
                    w.u32(0);
                }
                SymbolKind::Button => {
                    w.u8(11); // CPicButton revision = 11
                    write_actions(w, script_ref);
                    w.u8(0); // track_as_menu
                    w.cstring(&element.name);
                    w.u32(0); // accessibility revision = 0
                }
                SymbolKind::Graphic => {}
            }
        }
        ElementKind::Bitmap { resource_id, .. } => {
            let Some(&media_id) = media.bitmap_by_resource.get(resource_id) else {
                return;
            };
            w.begin_object("CPicBitmap");
            write_cpic_obj_base_header(w);
            write_cpic_obj_base_footer(w);
            w.u8(2); // CPicBitmap revision = 2
            for word in matrix_to_words(neoflash_core::transform_matrix(element.transform)) {
                w.i32(word);
            }
            w.u16(media_id);
            w.u8(0); // extension_present = 0
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let format = TextFormat {
                font: font_family.clone(),
                size: *font_size,
                color: (u32::from(color[0]) << 16)
                    | (u32::from(color[1]) << 8)
                    | u32::from(color[2]),
                ..Default::default()
            };
            let alpha = element.transform.alpha * (f64::from(color[3]) / 255.0);
            write_cpic_text_object(
                w,
                element,
                text,
                120.0,
                (*font_size * 1.5).max(20.0),
                &format,
                false,
                alpha,
            );
        }
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            ..
        } => {
            write_cpic_text_object(
                w,
                element,
                text,
                *width,
                *height,
                format,
                *word_wrap,
                element.transform.alpha,
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn write_cpic_text_object(
    w: &mut MfcWriter,
    element: &Element,
    text: &str,
    width: f64,
    height: f64,
    format: &TextFormat,
    word_wrap: bool,
    alpha: f64,
) {
    w.begin_object("CPicText");
    write_cpic_obj_base_header(w);
    write_cpic_obj_base_footer(w);

    w.u8(13); // CPicText revision = 13
    for word in matrix_to_words(neoflash_core::transform_matrix(element.transform)) {
        w.i32(word);
    }
    let w_twips = (width.max(1.0) * 20.0).round() as i32;
    let h_twips = (height.max(1.0) * 20.0).round() as i32;
    for bound in [0_i32, w_twips, 0_i32, h_twips] {
        w.i32(bound);
    }
    w.u8(u8::from(!word_wrap)); // auto_expand
    w.u8(0); // reserved
    let units: Vec<u16> = text.encode_utf16().collect();
    let flags: u32 = if units.is_empty() { 0x4000 } else { 0 };
    w.u32(flags);
    w.u16(0); // max_characters
    w.cstring(""); // variable_name
    if units.is_empty() {
        write_text_style(w, format, alpha);
    } else {
        for chunk in units.chunks(500) {
            w.u16(chunk.len() as u16);
            write_text_style(w, format, alpha);
            for &unit in chunk {
                w.u16(unit);
            }
        }
    }
    w.u16(0); // end of text runs
    w.cstring(&element.name); // instance_name (revision >= 6)
    w.u32(0); // accessibility revision = 0 (revision >= 9)
    w.u32(0); // scrollable (revision >= 8)
    w.cstring(""); // localization_id (revision >= 11)
    w.cstring(""); // embed_ranges (revision >= 12)
    write_filters(w, &element.filters); // filters (revision >= 13)
    w.u16(0); // selected_effect_index (revision >= 13)
}

#[allow(clippy::too_many_arguments)]
fn serialize_layer(
    w: &mut MfcWriter,
    timeline: &Timeline,
    layer_idx: usize,
    layer_id_to_idx: &BTreeMap<String, usize>,
    serialized_layers: &mut BTreeMap<usize, u16>,
    symbol_lib_ids: &BTreeMap<String, u32>,
    symbol_kinds: &BTreeMap<String, SymbolKind>,
    media: &MediaExportCatalog,
) -> u16 {
    if let Some(&existing_ref) = serialized_layers.get(&layer_idx) {
        w.write_object_ref(existing_ref);
        return existing_ref;
    }

    let layer = &timeline.layers[layer_idx];
    let layer_obj_ref = w.begin_object("CPicLayer");
    serialized_layers.insert(layer_idx, layer_obj_ref);

    write_cpic_obj_base_header(w);

    let total_frames = timeline.frame_count.max(1);
    if layer.properties.role != LayerRole::Folder {
        let mut effective_keyframes: Vec<Keyframe> = if layer.keyframes.is_empty() {
            vec![Keyframe {
                frame: 0,
                elements: Vec::new(),
                ..Default::default()
            }]
        } else {
            layer.keyframes.clone()
        };
        if layer_idx == 0 && !timeline.scripts.is_empty() {
            for fs in &timeline.scripts {
                if fs.frame < total_frames
                    && !effective_keyframes.iter().any(|k| k.frame == fs.frame)
                {
                    let base_elements = effective_keyframes
                        .iter()
                        .rfind(|k| k.frame < fs.frame)
                        .map(|k| k.elements.clone())
                        .unwrap_or_default();
                    effective_keyframes.push(Keyframe {
                        frame: fs.frame,
                        elements: base_elements,
                        ..Default::default()
                    });
                    effective_keyframes.sort_by_key(|k| k.frame);
                }
            }
        }
        for (k_idx, kf) in effective_keyframes.iter().enumerate() {
            let next_frame = effective_keyframes
                .get(k_idx + 1)
                .map_or(total_frames, |next| next.frame);
            let duration = next_frame.saturating_sub(kf.frame).clamp(1, 65535) as u16;
            let frame_script = timeline
                .scripts
                .iter()
                .find(|fs| fs.frame == kf.frame && layer_idx == 0)
                .map(|fs| fs.script.source.as_str());

            w.begin_object("CPicFrame");
            write_cpic_obj_base_header(w);
            for el in &kf.elements {
                write_display_element(w, el, symbol_lib_ids, symbol_kinds, media);
            }
            write_cpic_obj_base_footer(w);
            write_frame_shape(w, &kf.elements);
            write_cpic_frame_payload(w, duration, Some(kf), frame_script, media);
        }
    }

    write_cpic_obj_base_footer(w);

    // CPicLayer payload (revision = 6)
    w.u8(6);
    w.cstring(&layer.name);
    w.bytes(&[0, u8::from(!layer.visible), u8::from(layer.locked)]); // state_flags (3 bytes)
    w.u32(100); // height_literal
    let outline_rgb = layer.properties.outline_color.unwrap_or([0x4f, 0x80, 0xff]);
    w.u32(u32::from_le_bytes([
        outline_rgb[0],
        outline_rgb[1],
        outline_rgb[2],
        0,
    ]));
    w.u32(u32::from(layer.properties.outline));

    let related_target_id = layer
        .properties
        .mask
        .as_deref()
        .or(layer.properties.motion_guide.as_deref())
        .or(layer.properties.parent.as_deref());
    let related_idx = related_target_id
        .and_then(|id| layer_id_to_idx.get(id).copied())
        .filter(|&idx| idx != layer_idx);

    let layer_type: u8 = match layer.properties.role {
        LayerRole::Folder => 3,
        LayerRole::Guide => 1,
        LayerRole::Mask => 4,
        LayerRole::Normal => {
            if layer.properties.mask.is_some() {
                5
            } else if layer.properties.motion_guide.is_some() {
                2
            } else {
                0
            }
        }
    };
    w.u8(layer_type);

    if let Some(target_idx) = related_idx {
        serialize_layer(
            w,
            timeline,
            target_idx,
            layer_id_to_idx,
            serialized_layers,
            symbol_lib_ids,
            symbol_kinds,
            media,
        );
    } else {
        w.write_null_ref();
    }

    let mask_or_open: u8 =
        if layer.properties.role == LayerRole::Folder && layer.properties.collapsed {
            0
        } else {
            1
        };
    w.u8(mask_or_open);

    layer_obj_ref
}

fn write_cpic_frame_payload(
    w: &mut MfcWriter,
    duration: u16,
    kf: Option<&Keyframe>,
    frame_script: Option<&str>,
    media: &MediaExportCatalog,
) {
    w.u8(24); // CPicFrame revision = 24
    w.u16(duration);

    let (key_mode, accel_word): (u16, u16) = if let Some(k) = kf
        && let Some(easing) = &k.tween
    {
        let accel: i16 = match easing {
            Easing::Linear => 0,
            Easing::EaseIn => -100,
            Easing::EaseOut => 100,
            Easing::EaseInOut => 0,
            Easing::Flash { acceleration } => acceleration.round().clamp(-100.0, 100.0) as i16,
            Easing::CubicBezier { .. } | Easing::Bounce | Easing::Elastic => 0,
        };
        let mode: u16 = match k.tween_kind {
            TweenKind::Shape => 2,
            TweenKind::Classic | TweenKind::Motion => 1 | if k.orient_to_path { 4 } else { 0 },
        };
        (mode, accel as u16)
    } else {
        (0, 0)
    };
    w.u16(key_mode);
    w.u16(accel_word);

    // Frame sound fields
    if let Some(snd) = kf.and_then(|k| k.sound.as_ref())
        && let Some(&sound_media_id) = media.sound_by_resource.get(&snd.resource)
    {
        w.u16(sound_media_id);
        w.u16(0); // sound_envelope_count = 0
        let loop_count: u16 = if snd.loop_forever {
            0x7fff
        } else {
            snd.loops.clamp(1, 32767) as u16
        };
        w.u16(loop_count);
        let sync_code: u8 = match snd.sync {
            SoundSync::Event => 0,
            SoundSync::Start => 1,
            SoundSync::Stop => 2,
            SoundSync::Stream => 3,
        };
        w.u8(sync_code);
        w.u32(0); // sound_in_point
        w.u32(0); // sound_out_point
        w.u16(0); // sound_zoom
    } else {
        w.u16(0); // sound_media_id = 0
        w.u16(0); // sound_envelope_count = 0
        w.u16(1); // sound_loop_count
        w.u8(0); // sound_sync
        w.u32(0); // sound_in_point
        w.u32(0); // sound_out_point
        w.u16(0); // sound_zoom
    }

    let label = kf.map_or("", |k| k.label.as_str());
    w.cstring(label);
    write_actions(w, frame_script);
    w.u32(0); // rotation_mode
    w.u32(0); // rotation_count
    w.u32(u32::from(!label.is_empty())); // label_type
    w.write_null_ref(); // morph_shape = null
    w.u32(0); // shape_tween_blend
    w.write_null_ref(); // sound_settings = null
    w.cstring(""); // extension
    w.u32(0); // reserved
    let sound_effect: u32 = match kf.and_then(|k| k.sound.as_ref()).map(|s| s.effect) {
        Some(SoundEffect::LeftChannel) => 1,
        Some(SoundEffect::RightChannel) => 2,
        Some(SoundEffect::FadeLeftToRight) => 3,
        Some(SoundEffect::FadeRightToLeft) => 4,
        Some(SoundEffect::FadeIn) => 5,
        Some(SoundEffect::FadeOut) => 6,
        _ => 0,
    };
    w.u32(sound_effect);
    w.u32(0); // bookmark
    w.u32(1); // single_ease_curve
    w.u32(0); // custom_ease_present
}

fn build_timeline_stream(
    timeline: &Timeline,
    symbol_lib_ids: &BTreeMap<String, u32>,
    symbol_kinds: &BTreeMap<String, SymbolKind>,
    media: &MediaExportCatalog,
) -> Vec<u8> {
    let mut w = MfcWriter::new();
    w.u8(1); // stream_kind = 1 (CPicPage root)
    w.begin_object("CPicPage");
    write_cpic_obj_base_header(&mut w);

    let layer_id_to_idx: BTreeMap<String, usize> = timeline
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.clone(), i))
        .collect();
    let mut serialized_layers = BTreeMap::new();
    for layer_idx in 0..timeline.layers.len() {
        serialize_layer(
            &mut w,
            timeline,
            layer_idx,
            &layer_id_to_idx,
            &mut serialized_layers,
            symbol_lib_ids,
            symbol_kinds,
            media,
        );
    }

    write_cpic_obj_base_footer(&mut w);

    // CPicPage payload (revision = 3)
    w.u8(3);
    w.u16((timeline.layers.len() + 1) as u16); // next_layer_id
    w.u32(0); // guide_count = 0

    w.buf
}
