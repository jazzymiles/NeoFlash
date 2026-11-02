//! Pure-Rust Adobe XFL 2.2 ZIP-FLA (`.fla`) archive exporter.
//!
//! Serializes a [`neoflash_core::Project`] into a standard ZIP-compressed XFL 2.2
//! `.fla` container (`mimetype`, `<stem>.xfl`, `PublishSettings.xml`,
//! `DOMDocument.xml`, `LIBRARY/*.xml`, `LIBRARY/media/*`, and `.as` sources)
//! compatible with Adobe Flash CS5.5/CS6/Animate and round-trip importable by
//! `crate::xfl` whenever the project stays within NeoFlash's XFL importer subset.

use crate::{ImportError, source};
use neoflash_core::{
    Color, ColorTransform, Easing, Element, ElementKind, FilterSpec, FrameSound, GradientKind,
    GradientPaint, GradientStop, Id, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole,
    ObjectEvent, ObjectScript, PathCommand, Project, ResourceUnit, Script, ScriptLanguage,
    SoundEffect, SoundSync, Symbol, SymbolKind, TextAlign, Timeline, Transform, TweenKind,
    sample_layer_in, transform_matrix, valid_library_folder,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{Cursor, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const XFL_NS: &str = "http://ns.adobe.com/xfl/2008/";
const FLA_MIMETYPE: &[u8] = b"application/vnd.adobe.fla";
const XFL_PROXY: &[u8] = b"PROXY-CS5";

const MAX_ARCHIVE_FILES: usize = 10_000;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// Maximum bytes in a single `<Edge edges="...">` attribute (`xfl/shape.rs` caps at 65,535).
const MAX_EDGE_ATTR_BYTES: usize = 48_000;
/// Maximum segments in a single `<DOMShape>` (`xfl/shape.rs` caps at 16,384).
const MAX_SHAPE_SEGMENTS: usize = 15_000;
/// Adaptive cubic-to-quadratic subdivision flatness tolerance in pixels (~0.5 twip).
const CUBIC_SUBDIV_TOL_PX: f64 = 0.025;
/// Maximum recursive depth when approximating a cubic Bézier with quadratics (at most 8 quads).
const CUBIC_SUBDIV_MAX_DEPTH: u8 = 3;

type Point24_8 = [i64; 2];

/// Serialize `project` as a ZIP-compressed Adobe XFL 2.2 `.fla` archive in memory.
pub fn export_fla_bytes(project: &Project) -> Result<Vec<u8>, ImportError> {
    let ctx = ExportContext::build(project)?;

    let publish_xml = render_publish_settings(project, &ctx);
    let dom_document_xml = render_dom_document(project, &ctx);

    let mut symbol_xmls: Vec<(String, String)> = Vec::with_capacity(ctx.symbol_order.len());
    for sym_id in &ctx.symbol_order {
        if let Some(symbol) = project.symbol(sym_id)
            && let Some(info) = ctx.symbols.get(sym_id)
        {
            let xml = render_symbol_item(symbol, info, &ctx);
            symbol_xmls.push((format!("LIBRARY/{}", info.xml_href), xml));
        }
    }

    let as_sources = collect_actionscript_sources(project, &ctx.doc_stem);

    let mut writer = ArchiveBuilder::new();
    writer.add_file("mimetype", FLA_MIMETYPE, CompressionMethod::Stored)?;
    writer.add_file(
        &format!("{}.xfl", ctx.doc_stem),
        XFL_PROXY,
        CompressionMethod::Stored,
    )?;
    writer.add_file(
        "PublishSettings.xml",
        publish_xml.as_bytes(),
        CompressionMethod::Deflated,
    )?;
    writer.add_file(
        "DOMDocument.xml",
        dom_document_xml.as_bytes(),
        CompressionMethod::Deflated,
    )?;

    for (entry_path, xml) in &symbol_xmls {
        writer.add_file(entry_path, xml.as_bytes(), CompressionMethod::Deflated)?;
    }
    for bmp_id in &ctx.bitmap_order {
        if let Some(bmp) = ctx.bitmaps.get(bmp_id) {
            writer.add_file(&bmp.zip_path, &bmp.bytes, CompressionMethod::Deflated)?;
        }
    }
    for snd_id in &ctx.sound_order {
        if let Some(snd) = ctx.sounds.get(snd_id) {
            writer.add_file(&snd.zip_path, &snd.bytes, CompressionMethod::Deflated)?;
        }
    }
    for (entry_path, source_text) in &as_sources {
        writer.add_file(
            entry_path,
            source_text.as_bytes(),
            CompressionMethod::Deflated,
        )?;
    }

    writer.finish()
}

/// Write `project` as a ZIP-compressed Adobe XFL 2.2 `.fla` archive to `path` atomically.
pub fn export_fla(project: &Project, path: &Path) -> Result<(), ImportError> {
    if path.as_os_str().is_empty() {
        return Err(ImportError::Container("target FLA path is empty".into()));
    }
    let bytes = export_fla_bytes(project)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.fla");

    let (mut tmp_file, tmp_path) = create_unique_temp_file(parent, file_stem)?;
    let write_res = (|| -> std::io::Result<()> {
        tmp_file.write_all(&bytes)?;
        tmp_file.sync_all()?;
        drop(tmp_file);
        fs::rename(&tmp_path, path)
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(ImportError::Io(err));
    }
    Ok(())
}

fn create_unique_temp_file(dir: &Path, stem: &str) -> std::io::Result<(fs::File, PathBuf)> {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let pid = std::process::id();
    let clean_stem: String = stem
        .chars()
        .take(48)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    for _ in 0..256 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(".{clean_stem}.tmp-{pid}-{n}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, candidate)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "unable to allocate temporary file for atomic FLA export",
    ))
}

struct ArchiveBuilder {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    seen_lower: HashSet<String>,
    total_bytes: u64,
    entry_count: usize,
}

impl ArchiveBuilder {
    fn new() -> Self {
        Self {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
            seen_lower: HashSet::new(),
            total_bytes: 0,
            entry_count: 0,
        }
    }

    fn add_file(
        &mut self,
        name: &str,
        bytes: &[u8],
        method: CompressionMethod,
    ) -> Result<(), ImportError> {
        source::safe_name(name)?;
        if !self.seen_lower.insert(name.to_ascii_lowercase()) {
            return Err(ImportError::Container(format!(
                "duplicate archive path during FLA export: {name}"
            )));
        }
        self.entry_count += 1;
        if self.entry_count > MAX_ARCHIVE_FILES {
            return Err(ImportError::Container(
                "exported FLA archive exceeds 10000 entries".into(),
            ));
        }
        let len = bytes.len() as u64;
        if len > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(ImportError::Container(format!(
                "exported FLA entry {name} exceeds 8 MiB limit"
            )));
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(len)
            .ok_or_else(|| ImportError::Container("exported FLA archive size overflow".into()))?;
        if self.total_bytes > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(ImportError::Container(
                "exported FLA archive exceeds 64 MiB uncompressed limit".into(),
            ));
        }
        let options = SimpleFileOptions::default().compression_method(method);
        self.zip
            .start_file(name, options)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        self.zip.write_all(bytes)?;
        Ok(())
    }

    fn finish(self) -> Result<Vec<u8>, ImportError> {
        let cursor = self
            .zip
            .finish()
            .map_err(|e| ImportError::Container(e.to_string()))?;
        Ok(cursor.into_inner())
    }
}

struct SymbolExport {
    item_name: String,
    folder: String,
    xml_href: String,
    kind: SymbolKind,
}

struct BitmapExport {
    item_name: String,
    href: String,
    zip_path: String,
    bytes: Vec<u8>,
    allow_smoothing: bool,
}

struct SoundExport {
    item_name: String,
    href: String,
    zip_path: String,
    bytes: Vec<u8>,
    duration_44: u64,
    emit_mp3_sample_count: bool,
}

struct ExportContext {
    doc_stem: String,
    as_version: u8,
    include_hidden_layers: bool,
    folders: Vec<String>,
    symbol_order: Vec<Id>,
    symbols: HashMap<Id, SymbolExport>,
    bitmap_order: Vec<Id>,
    bitmaps: HashMap<Id, BitmapExport>,
    sound_order: Vec<Id>,
    sounds: HashMap<Id, SoundExport>,
}

impl ExportContext {
    fn build(project: &Project) -> Result<Self, ImportError> {
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for elem in &key.elements {
                        if !matches!(elem.kind, ElementKind::Instance { .. })
                            && elem
                                .scripts
                                .iter()
                                .any(|s| !s.script.source.trim().is_empty())
                        {
                            return Err(ImportError::Container(format!(
                                "object scripts on non-symbol element {:?} in symbol {:?} cannot be represented in XFL 2.2 .fla; convert the element to a symbol first",
                                elem.name, sym.name
                            )));
                        }
                    }
                }
            }
        }
        let doc_stem = sanitize_doc_stem(&project.name);
        let as_version = detect_as_version(project);
        let include_hidden_layers = !can_exclude_hidden_layers(project);

        let folders: Vec<String> = project
            .library_folder_paths()
            .into_iter()
            .filter(|f| !f.is_empty() && valid_library_folder(f))
            .collect();

        let (symbol_order, symbols) = collect_symbols(project);
        let (bitmap_order, bitmaps, sound_order, sounds) = collect_media(project)?;

        Ok(Self {
            doc_stem,
            as_version,
            include_hidden_layers,
            folders,
            symbol_order,
            symbols,
            bitmap_order,
            bitmaps,
            sound_order,
            sounds,
        })
    }
}

fn is_actionscript(lang: ScriptLanguage) -> bool {
    matches!(
        lang,
        ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3
    )
}

fn encode_script_with_language_tag(script: &Script) -> String {
    match script.language {
        ScriptLanguage::JavaScript => {
            format!("/* neoflash:language=javascript */\n{}", script.source)
        }
        ScriptLanguage::TypeScript => {
            format!("/* neoflash:language=typescript */\n{}", script.source)
        }
        ScriptLanguage::ActionScript1 => {
            format!("/* neoflash:language=actionscript1 */\n{}", script.source)
        }
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => script.source.clone(),
    }
}

fn detect_as_version(project: &Project) -> u8 {
    if project
        .document_class
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return 3;
    }
    if project
        .sources
        .iter()
        .any(|s| s.language == ScriptLanguage::ActionScript3)
    {
        return 3;
    }
    for sym in &project.symbols {
        if sym
            .timeline
            .scripts
            .iter()
            .any(|s| s.script.language == ScriptLanguage::ActionScript3)
        {
            return 3;
        }
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                for elem in &key.elements {
                    if elem
                        .scripts
                        .iter()
                        .any(|s| s.script.language == ScriptLanguage::ActionScript3)
                    {
                        return 3;
                    }
                }
            }
        }
    }
    2
}

/// Checks whether `<InvisibleLayer>0</InvisibleLayer>` can be emitted so hidden NeoFlash
/// layers round-trip with `layer.visible == false` without hitting `excluded_layer_label`
/// or `excluded_layer_duration` in `xfl.rs`.
fn can_exclude_hidden_layers(project: &Project) -> bool {
    let mut saw_hidden = false;
    for sym in &project.symbols {
        let timeline = &sym.timeline;
        let timeline_end = compute_timeline_frames(timeline);
        let mut published_end = 0u32;
        let mut excluded_end = 0u32;
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            let layer_end = layer_effective_end(layer, timeline_end);
            if layer.visible {
                published_end = published_end.max(layer_end);
            } else {
                saw_hidden = true;
                excluded_end = excluded_end.max(layer_end);
                if layer.keyframes.iter().any(|k| !k.label.is_empty()) {
                    return false;
                }
            }
        }
        if excluded_end > published_end {
            return false;
        }
    }
    // When no hidden layers exist at all, default Flash setting `1` produces zero warnings.
    saw_hidden
}

fn sanitize_doc_stem(name: &str) -> String {
    let base = sanitize_file_token(name, "Document");
    let lower = base.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "mimetype" | "domdocument" | "publishsettings" | "library" | "bin"
    ) {
        format!("{base}_doc")
    } else {
        base
    }
}

fn sanitize_file_token(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if out.len() >= 72 {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches(|c: char| c == ' ' || c == '_' || c == '.');
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn sanitize_library_display_name(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if !is_valid_xml_char(ch) || matches!(ch, '\r' | '\n' | '\t') {
            continue;
        }
        if ch == '/' || ch == '\\' {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn dedup_case_insensitive(used_lower: &mut HashSet<String>, stem: &str, suffix: &str) -> String {
    let first = format!("{stem}{suffix}");
    if used_lower.insert(first.to_ascii_lowercase()) {
        return first;
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{stem}_{idx}{suffix}");
        if used_lower.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        idx += 1;
    }
}

fn collect_symbols(project: &Project) -> (Vec<Id>, HashMap<Id, SymbolExport>) {
    let mut order = Vec::new();
    let mut map = HashMap::new();
    let mut used_names = HashSet::new();
    let mut used_hrefs_lower = HashSet::new();

    for sym in &project.symbols {
        if project.is_scene(&sym.id) {
            continue;
        }
        let folder = if !sym.folder.is_empty() && valid_library_folder(&sym.folder) {
            sym.folder.clone()
        } else {
            String::new()
        };
        let base_display = sanitize_library_display_name(&sym.name, "Symbol");
        let mut display = base_display.clone();
        let mut full_name = if folder.is_empty() {
            display.clone()
        } else {
            format!("{folder}/{display}")
        };
        let mut suffix_n = 2usize;
        while !used_names.insert(full_name.clone()) {
            display = format!("{base_display} {suffix_n}");
            full_name = if folder.is_empty() {
                display.clone()
            } else {
                format!("{folder}/{display}")
            };
            suffix_n += 1;
        }

        let file_stem = sanitize_file_token(&display, "Symbol");
        let safe_folder_prefix = if folder.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = folder
                .split('/')
                .map(|seg| sanitize_file_token(seg, "Folder"))
                .collect();
            format!("{}/", parts.join("/"))
        };
        let xml_href = dedup_case_insensitive(
            &mut used_hrefs_lower,
            &format!("{safe_folder_prefix}{file_stem}"),
            ".xml",
        );

        order.push(sym.id.clone());
        map.insert(
            sym.id.clone(),
            SymbolExport {
                item_name: full_name,
                folder,
                xml_href,
                kind: sym.kind,
            },
        );
    }

    (order, map)
}

type MediaCollections = (
    Vec<Id>,
    HashMap<Id, BitmapExport>,
    Vec<Id>,
    HashMap<Id, SoundExport>,
);

fn collect_media(project: &Project) -> Result<MediaCollections, ImportError> {
    let mut smoothed_bitmaps: HashSet<&str> = HashSet::new();
    let mut referenced_bitmaps: HashSet<&str> = HashSet::new();
    let mut referenced_sounds: HashSet<&str> = HashSet::new();
    let mut custom_env_sounds: HashSet<&str> = HashSet::new();

    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                if let Some(snd) = &key.sound {
                    referenced_sounds.insert(snd.resource.as_str());
                    if snd.effect == SoundEffect::Custom
                        || snd.envelope.as_ref().is_some_and(|pts| !pts.is_empty())
                    {
                        custom_env_sounds.insert(snd.resource.as_str());
                    }
                }
                for elem in &key.elements {
                    if let ElementKind::Bitmap {
                        resource_id,
                        smoothing,
                        ..
                    } = &elem.kind
                    {
                        referenced_bitmaps.insert(resource_id.as_str());
                        if *smoothing {
                            smoothed_bitmaps.insert(resource_id.as_str());
                        }
                    }
                }
            }
        }
    }

    let mut bitmap_order = Vec::new();
    let mut bitmaps = HashMap::new();
    let mut sound_order = Vec::new();
    let mut sounds = HashMap::new();

    let mut used_bitmap_names = HashSet::new();
    let mut used_sound_names = HashSet::new();
    let mut used_media_files_lower = HashSet::new();

    for res in &project.resources {
        if is_internal_provenance_resource(res) {
            continue;
        }
        let is_img_type = res
            .media_type
            .get(..6)
            .is_some_and(|p| p.eq_ignore_ascii_case("image/"));
        let is_aud_type = res.is_audio();
        let want_img = is_img_type || referenced_bitmaps.contains(res.id.as_str());
        let want_aud = is_aud_type || referenced_sounds.contains(res.id.as_str());
        if !want_img && !want_aud {
            return Err(ImportError::Container(format!(
                "resource {:?} ({}) cannot be represented in XFL 2.2 .fla media library",
                res.path, res.media_type
            )));
        }
        let raw = res.decoded_bytes().map_err(|err| {
            ImportError::Container(format!("invalid resource {:?}: {err}", res.path))
        })?;

        if want_img && let Some((bytes, ext)) = normalize_bitmap_bytes(&raw) {
            if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(ImportError::Container(format!(
                    "bitmap resource {:?} exceeds 8 MiB XFL entry limit",
                    res.path
                )));
            }
            let stem = clean_resource_stem(&res.path, "bitmap");
            let item_name = dedup_exact_name(&mut used_bitmap_names, &format!("{stem}.{ext}"));
            let file_name =
                dedup_case_insensitive(&mut used_media_files_lower, &stem, &format!(".{ext}"));
            bitmap_order.push(res.id.clone());
            bitmaps.insert(
                res.id.clone(),
                BitmapExport {
                    item_name,
                    href: format!("media/{file_name}"),
                    zip_path: format!("LIBRARY/media/{file_name}"),
                    bytes,
                    allow_smoothing: smoothed_bitmaps.contains(res.id.as_str()),
                },
            );
            continue;
        }

        if want_aud && let Some((ext, duration_44, is_mp3)) = inspect_audio_bytes(&raw) {
            if raw.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(ImportError::Container(format!(
                    "audio resource {:?} exceeds 8 MiB XFL entry limit",
                    res.path
                )));
            }
            let stem = clean_resource_stem(&res.path, "sound");
            let item_name = dedup_exact_name(&mut used_sound_names, &format!("{stem}.{ext}"));
            let file_name =
                dedup_case_insensitive(&mut used_media_files_lower, &stem, &format!(".{ext}"));
            let emit_mp3_sample_count = is_mp3 && custom_env_sounds.contains(res.id.as_str());
            sound_order.push(res.id.clone());
            sounds.insert(
                res.id.clone(),
                SoundExport {
                    item_name,
                    href: format!("media/{file_name}"),
                    zip_path: format!("LIBRARY/media/{file_name}"),
                    bytes: raw,
                    duration_44,
                    emit_mp3_sample_count,
                },
            );
        }
    }

    Ok((bitmap_order, bitmaps, sound_order, sounds))
}

fn is_internal_provenance_resource(res: &ResourceUnit) -> bool {
    res.id == "fla-provenance"
        || matches!(
            res.path.as_str(),
            "legacy/provenance.json"
                | "legacy/import-provenance.json"
                | "legacy/source.fla"
                | "legacy/source.swf"
        )
        || res
            .media_type
            .eq_ignore_ascii_case("application/x-neoflash-fla-source")
        || res.media_type.eq_ignore_ascii_case("application/json")
}

fn clean_resource_stem(resource_path: &str, fallback: &str) -> String {
    let file_part = resource_path
        .rsplit_once('/')
        .map_or(resource_path, |(_, name)| name);
    let raw_stem = match file_part.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => file_part,
    };
    // Strip importer index prefix like "0-hero" on "assets/bitmaps/0-hero.png" when followed by a non-empty stem.
    let stripped = if (resource_path.starts_with("assets/bitmaps/")
        || resource_path.starts_with("assets/sounds/"))
        && let Some((digits, rest)) = raw_stem.split_once('-')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !rest.is_empty()
    {
        rest
    } else {
        raw_stem
    };
    sanitize_file_token(stripped, fallback)
}

fn dedup_exact_name(used: &mut HashSet<String>, desired: &str) -> String {
    if used.insert(desired.to_owned()) {
        return desired.to_owned();
    }
    let (stem, ext) = match desired.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, Some(e)),
        _ => (desired, None),
    };
    let mut n = 2usize;
    loop {
        let candidate = match ext {
            Some(e) => format!("{stem}_{n}.{e}"),
            None => format!("{stem}_{n}"),
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

fn normalize_bitmap_bytes(raw: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    if raw.starts_with(b"\x89PNG\r\n\x1a\n")
        && image::load_from_memory_with_format(raw, image::ImageFormat::Png).is_ok()
    {
        return Some((raw.to_vec(), "png"));
    }
    if raw.starts_with(&[0xff, 0xd8, 0xff])
        && image::load_from_memory_with_format(raw, image::ImageFormat::Jpeg).is_ok()
    {
        return Some((raw.to_vec(), "jpg"));
    }
    let decoded = image::load_from_memory(raw).ok()?;
    let mut png_bytes = Vec::new();
    decoded
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .ok()?;
    Some((png_bytes, "png"))
}

fn inspect_audio_bytes(raw: &[u8]) -> Option<(&'static str, u64, bool)> {
    if let Some(dur) = parse_wav_duration_44(raw) {
        return Some(("wav", dur, false));
    }
    if looks_like_mp3(raw) {
        return Some(("mp3", 44_100, true));
    }
    None
}

fn parse_wav_duration_44(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt_info: Option<(u32, usize)> = None;
    let mut data_len: Option<usize> = None;
    while pos + 8 <= bytes.len() {
        let tag = &bytes[pos..pos + 4];
        let chunk_len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.checked_add(chunk_len)?;
        if payload_end > bytes.len() {
            return None;
        }
        let payload = &bytes[payload_start..payload_end];
        if tag == b"fmt " {
            if payload.len() < 16 {
                return None;
            }
            let channels = u16::from_le_bytes(payload[2..4].try_into().ok()?);
            let sample_rate = u32::from_le_bytes(payload[4..8].try_into().ok()?);
            let block_align = u16::from_le_bytes(payload[12..14].try_into().ok()?);
            let bits = u16::from_le_bytes(payload[14..16].try_into().ok()?);
            if channels == 0 || sample_rate == 0 {
                return None;
            }
            let derived_align = usize::from(channels) * usize::from(bits.max(8)).div_ceil(8);
            let frame_bytes = usize::from(block_align).max(derived_align).max(1);
            fmt_info = Some((sample_rate, frame_bytes));
        } else if tag == b"data" {
            data_len = Some(payload.len());
        }
        pos = payload_end.saturating_add(chunk_len & 1);
    }
    let (sample_rate, frame_bytes) = fmt_info?;
    let data_bytes = data_len?;
    if data_bytes < frame_bytes {
        return None;
    }
    let sample_frames = (data_bytes / frame_bytes) as u64;
    Some((sample_frames * 44_100) / u64::from(sample_rate))
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    if bytes.len() >= 10
        && &bytes[..3] == b"ID3"
        && (bytes[6] | bytes[7] | bytes[8] | bytes[9]) & 0x80 == 0
    {
        let tag_len = ((bytes[6] as usize) << 21)
            | ((bytes[7] as usize) << 14)
            | ((bytes[8] as usize) << 7)
            | (bytes[9] as usize);
        let footer = if (bytes[5] & 0x10) != 0 { 10 } else { 0 };
        start = (10 + tag_len + footer).min(bytes.len());
    }
    let end = bytes
        .len()
        .saturating_sub(3)
        .min(start.saturating_add(4096));
    (start..end).any(|i| {
        let h = &bytes[i..i + 4];
        h[0] == 0xff
            && (h[1] & 0xe0) == 0xe0
            && ((h[1] >> 3) & 3) != 1
            && ((h[1] >> 1) & 3) != 0
            && ((h[2] >> 4) & 0xf) != 0xf
            && ((h[2] >> 2) & 3) != 3
    })
}

fn collect_actionscript_sources(project: &Project, doc_stem: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut used_lower: HashSet<String> = [
        "mimetype".to_owned(),
        format!("{doc_stem}.xfl").to_ascii_lowercase(),
        "publishsettings.xml".to_owned(),
        "domdocument.xml".to_owned(),
    ]
    .into_iter()
    .collect();

    for src in &project.sources {
        let ext = match src.language {
            ScriptLanguage::JavaScript => ".js",
            ScriptLanguage::TypeScript => ".ts",
            ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3 => ".as",
        };
        let cleaned = sanitize_as_source_path(&src.path);
        let (dir_prefix, file_stem) = match cleaned.rsplit_once('/') {
            Some((dir, base)) => (format!("{dir}/"), base),
            None => (String::new(), cleaned.as_str()),
        };
        let rel_path =
            dedup_case_insensitive(&mut used_lower, &format!("{dir_prefix}{file_stem}"), ext);
        if source::safe_name(&rel_path).is_ok() {
            let body = if src.language == ScriptLanguage::ActionScript1
                && !src.source.starts_with("/* neoflash:language=")
            {
                format!("/* neoflash:language=actionscript1 */\n{}", src.source)
            } else {
                src.source.clone()
            };
            out.push((rel_path, body));
        }
    }
    out
}

fn sanitize_as_source_path(raw_path: &str) -> String {
    let normalized = raw_path.replace('\\', "/");
    let without_ext = if normalized.len() > 3
        && (normalized[normalized.len() - 3..].eq_ignore_ascii_case(".as")
            || normalized[normalized.len() - 3..].eq_ignore_ascii_case(".js")
            || normalized[normalized.len() - 3..].eq_ignore_ascii_case(".ts"))
    {
        &normalized[..normalized.len() - 3]
    } else {
        normalized.as_str()
    };
    let mut segments: Vec<String> = Vec::new();
    for part in without_ext.split('/') {
        let trimmed = part.trim();
        if trimmed.is_empty() || matches!(trimmed, "." | "..") {
            continue;
        }
        segments.push(sanitize_file_token(trimmed, "Script"));
        if segments.len() >= 12 {
            break;
        }
    }
    if segments.is_empty() {
        segments.push("Main".to_owned());
    }
    if segments[0].eq_ignore_ascii_case("LIBRARY") || segments[0].eq_ignore_ascii_case("bin") {
        segments.insert(0, "src".to_owned());
    }
    segments.join("/")
}

fn render_publish_settings(project: &Project, ctx: &ExportContext) -> String {
    let mut xml = String::with_capacity(384);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<flash_profiles>\n");
    xml.push_str("  <flash_profile version=\"1.0\" name=\"Default\" current=\"true\">\n");
    xml.push_str("    <PublishFlashProperties enabled=\"true\">\n");
    writeln!(
        xml,
        "      <ActionScriptVersion>{}</ActionScriptVersion>",
        ctx.as_version
    )
    .unwrap();
    writeln!(
        xml,
        "      <InvisibleLayer>{}</InvisibleLayer>",
        u8::from(ctx.include_hidden_layers)
    )
    .unwrap();
    if let Some(doc_class) = project
        .document_class
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        writeln!(
            xml,
            "      <DocumentClass>{}</DocumentClass>",
            escape_xml_text(doc_class)
        )
        .unwrap();
    }
    xml.push_str("    </PublishFlashProperties>\n");
    xml.push_str("  </flash_profile>\n");
    xml.push_str("</flash_profiles>\n");
    xml
}

fn render_dom_document(project: &Project, ctx: &ExportContext) -> String {
    let mut xml = String::with_capacity(2048);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let bg = fmt_hex_rgb([
        project.background[0],
        project.background[1],
        project.background[2],
    ]);
    writeln!(
        xml,
        "<DOMDocument xmlns=\"{XFL_NS}\" width=\"{}\" height=\"{}\" frameRate=\"{}\" backgroundColor=\"{bg}\" xflVersion=\"2.2\">",
        project.width.max(1),
        project.height.max(1),
        project.fps.max(1),
    )
    .unwrap();

    if !ctx.folders.is_empty() {
        xml.push_str("  <folders>\n");
        for folder in &ctx.folders {
            writeln!(
                xml,
                "    <DOMFolderItem name=\"{}\" isExpanded=\"true\"/>",
                escape_xml_attr(folder)
            )
            .unwrap();
        }
        xml.push_str("  </folders>\n");
    }

    if !ctx.bitmap_order.is_empty() || !ctx.sound_order.is_empty() {
        xml.push_str("  <media>\n");
        for bmp_id in &ctx.bitmap_order {
            if let Some(bmp) = ctx.bitmaps.get(bmp_id) {
                writeln!(
                    xml,
                    "    <DOMBitmapItem name=\"{}\" href=\"{}\" allowSmoothing=\"{}\"/>",
                    escape_xml_attr(&bmp.item_name),
                    escape_xml_attr(&bmp.href),
                    if bmp.allow_smoothing { "true" } else { "false" }
                )
                .unwrap();
            }
        }
        for snd_id in &ctx.sound_order {
            if let Some(snd) = ctx.sounds.get(snd_id) {
                if snd.emit_mp3_sample_count {
                    writeln!(
                        xml,
                        "    <DOMSoundItem name=\"{}\" href=\"{}\" format=\"44kHz 16bit Stereo\" sampleCount=\"44100\"/>",
                        escape_xml_attr(&snd.item_name),
                        escape_xml_attr(&snd.href),
                    )
                    .unwrap();
                } else {
                    writeln!(
                        xml,
                        "    <DOMSoundItem name=\"{}\" href=\"{}\"/>",
                        escape_xml_attr(&snd.item_name),
                        escape_xml_attr(&snd.href),
                    )
                    .unwrap();
                }
            }
        }
        xml.push_str("  </media>\n");
    }

    if !ctx.symbol_order.is_empty() {
        xml.push_str("  <symbols>\n");
        for sym_id in &ctx.symbol_order {
            if let Some(info) = ctx.symbols.get(sym_id) {
                writeln!(
                    xml,
                    "    <Include href=\"{}\" loadImmediate=\"false\"/>",
                    escape_xml_attr(&info.xml_href)
                )
                .unwrap();
            }
        }
        xml.push_str("  </symbols>\n");
    }

    xml.push_str("  <timelines>\n");
    let mut rendered_any_scene = false;
    for (idx, scene_id) in project.scene_ids().iter().enumerate() {
        if let Some(scene_sym) = project.symbol(scene_id) {
            let fallback = format!("Scene {}", idx + 1);
            let scene_name = if scene_sym.name.trim().is_empty() {
                fallback.as_str()
            } else {
                scene_sym.name.as_str()
            };
            render_dom_timeline(&mut xml, "    ", scene_name, &scene_sym.timeline, ctx);
            rendered_any_scene = true;
        }
    }
    if !rendered_any_scene {
        let empty = Timeline {
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
            guides: vec![],
        };
        render_dom_timeline(&mut xml, "    ", "Scene 1", &empty, ctx);
    }
    xml.push_str("  </timelines>\n");
    xml.push_str("</DOMDocument>\n");
    xml
}

fn render_symbol_item(symbol: &Symbol, info: &SymbolExport, ctx: &ExportContext) -> String {
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let sym_type = match symbol.kind {
        SymbolKind::MovieClip => "movie clip",
        SymbolKind::Graphic => "graphic",
        SymbolKind::Button => "button",
    };
    write!(
        xml,
        "<DOMSymbolItem xmlns=\"{XFL_NS}\" name=\"{}\" symbolType=\"{sym_type}\"",
        escape_xml_attr(&info.item_name),
    )
    .unwrap();
    if !info.folder.is_empty() {
        write!(xml, " folder=\"{}\"", escape_xml_attr(&info.folder)).unwrap();
    }
    let class_name = symbol
        .class_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let linkage_id = symbol
        .linkage_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if class_name.is_some() || linkage_id.is_some() {
        xml.push_str(" linkageExportForAS=\"true\"");
        if let Some(cn) = class_name {
            write!(xml, " linkageClassName=\"{}\"", escape_xml_attr(cn)).unwrap();
        }
        if let Some(lid) = linkage_id {
            write!(xml, " linkageIdentifier=\"{}\"", escape_xml_attr(lid)).unwrap();
        }
    }
    xml.push_str(">\n  <timeline>\n");
    let timeline_name = info
        .item_name
        .rsplit_once('/')
        .map_or(info.item_name.as_str(), |(_, b)| b);
    render_dom_timeline(&mut xml, "    ", timeline_name, &symbol.timeline, ctx);
    xml.push_str("  </timeline>\n</DOMSymbolItem>\n");
    xml
}

struct PreparedFrame {
    index: u32,
    duration: u32,
    label: String,
    tween: Option<(Easing, TweenKind, bool)>,
    sound: Option<FrameSound>,
    script: Option<String>,
    elements_xml: Vec<String>,
}

struct PreparedLayer {
    original_id: Option<Id>,
    is_primary_for_id: bool,
    name: String,
    visible: bool,
    locked: bool,
    properties: LayerProperties,
    frames: Vec<PreparedFrame>,
}

fn compute_timeline_frames(timeline: &Timeline) -> u32 {
    let mut frames = timeline.frame_count.max(1);
    for layer in &timeline.layers {
        if layer.properties.role == LayerRole::Folder {
            continue;
        }
        if let Some(end) = layer.properties.end_frame {
            frames = frames.max(end);
        }
        for key in &layer.keyframes {
            frames = frames.max(key.frame.saturating_add(1));
        }
    }
    for fs in &timeline.scripts {
        if !fs.script.source.trim().is_empty() {
            frames = frames.max(fs.frame.saturating_add(1));
        }
    }
    frames
}

fn layer_effective_end(layer: &Layer, timeline_frames: u32) -> u32 {
    layer
        .properties
        .end_frame
        .unwrap_or(timeline_frames)
        .min(timeline_frames)
}

fn render_dom_timeline(
    xml: &mut String,
    indent: &str,
    name: &str,
    timeline: &Timeline,
    ctx: &ExportContext,
) {
    writeln!(
        xml,
        "{indent}<DOMTimeline name=\"{}\">",
        escape_xml_attr(name)
    )
    .unwrap();
    writeln!(xml, "{indent}  <layers>").unwrap();

    let timeline_frames = compute_timeline_frames(timeline);

    // Prepare layers in NeoFlash painter order (bottom-to-top), splitting multi-element
    // continuous symbol-instance layers when needed to preserve 1-element instance continuity.
    let mut painter_layers: Vec<PreparedLayer> = Vec::new();
    if timeline.layers.is_empty() {
        painter_layers.push(PreparedLayer {
            original_id: None,
            is_primary_for_id: false,
            name: "Layer 1".to_owned(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            frames: vec![PreparedFrame {
                index: 0,
                duration: timeline_frames,
                label: String::new(),
                tween: None,
                sound: None,
                script: None,
                elements_xml: Vec::new(),
            }],
        });
    } else {
        for layer in &timeline.layers {
            for sub in prepare_layer(timeline, layer, timeline_frames, ctx) {
                painter_layers.push(sub);
            }
        }
    }

    // XFL stores topmost layers first.
    painter_layers.reverse();

    // Attach frame scripts to existing frame boundaries on published layers,
    // creating a topmost "Actions" layer for any script frame without a matching boundary.
    let mut scripts_by_frame: BTreeMap<u32, String> = BTreeMap::new();
    for fs in &timeline.scripts {
        if fs.script.source.trim().is_empty() {
            continue;
        }
        let encoded = encode_script_with_language_tag(&fs.script);
        scripts_by_frame
            .entry(fs.frame)
            .and_modify(|existing| {
                existing.push('\n');
                existing.push_str(&encoded);
            })
            .or_insert(encoded);
    }

    let mut unplaced_scripts: Vec<(u32, String)> = Vec::new();
    for (frame_idx, source_code) in scripts_by_frame {
        let mut placed = false;
        for layer in &mut painter_layers {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            if !layer.visible && !ctx.include_hidden_layers {
                continue;
            }
            if let Some(slot) = layer
                .frames
                .iter_mut()
                .find(|f| f.index == frame_idx && f.script.is_none())
            {
                slot.script = Some(source_code.clone());
                placed = true;
                break;
            }
        }
        if !placed {
            unplaced_scripts.push((frame_idx, source_code));
        }
    }

    if !unplaced_scripts.is_empty() {
        painter_layers.insert(
            0,
            build_synthetic_actions_layer(&unplaced_scripts, timeline_frames),
        );
    }

    // Map original layer IDs to their XFL layer index for `parentLayerIndex`.
    let mut id_to_xfl_index: HashMap<&str, (usize, LayerRole)> = HashMap::new();
    for (xfl_idx, layer) in painter_layers.iter().enumerate() {
        if layer.is_primary_for_id
            && let Some(orig_id) = layer.original_id.as_deref()
        {
            id_to_xfl_index.insert(orig_id, (xfl_idx, layer.properties.role));
        }
    }

    let layer_indent = format!("{indent}    ");
    for (xfl_idx, layer) in painter_layers.iter().enumerate() {
        let parent_idx = resolve_parent_layer_index(xfl_idx, layer, &id_to_xfl_index);
        render_dom_layer(xml, &layer_indent, layer, parent_idx, ctx);
    }

    writeln!(xml, "{indent}  </layers>").unwrap();
    writeln!(xml, "{indent}</DOMTimeline>").unwrap();
}

fn build_synthetic_actions_layer(scripts: &[(u32, String)], timeline_frames: u32) -> PreparedLayer {
    let mut frames = Vec::new();
    let mut cursor = 0u32;
    for (idx, (script_frame, source_code)) in scripts.iter().enumerate() {
        if *script_frame > cursor {
            frames.push(PreparedFrame {
                index: cursor,
                duration: *script_frame - cursor,
                label: String::new(),
                tween: None,
                sound: None,
                script: None,
                elements_xml: Vec::new(),
            });
        }
        let next_boundary = scripts
            .get(idx + 1)
            .map_or(timeline_frames, |(nf, _)| *nf)
            .max(script_frame.saturating_add(1));
        frames.push(PreparedFrame {
            index: *script_frame,
            duration: next_boundary - *script_frame,
            label: String::new(),
            tween: None,
            sound: None,
            script: Some(source_code.clone()),
            elements_xml: Vec::new(),
        });
        cursor = next_boundary;
    }
    PreparedLayer {
        original_id: None,
        is_primary_for_id: false,
        name: "Actions".to_owned(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        frames,
    }
}

fn resolve_parent_layer_index(
    xfl_idx: usize,
    layer: &PreparedLayer,
    id_to_xfl_index: &HashMap<&str, (usize, LayerRole)>,
) -> Option<usize> {
    let lookup = |target: Option<&Id>, expected_role: LayerRole| -> Option<usize> {
        let target_id = target?;
        let &(idx, role) = id_to_xfl_index.get(target_id.as_str())?;
        (role == expected_role && idx != xfl_idx).then_some(idx)
    };
    if layer.properties.role == LayerRole::Normal {
        lookup(layer.properties.mask.as_ref(), LayerRole::Mask)
            .or_else(|| lookup(layer.properties.motion_guide.as_ref(), LayerRole::Guide))
            .or_else(|| lookup(layer.properties.parent.as_ref(), LayerRole::Folder))
    } else {
        lookup(layer.properties.parent.as_ref(), LayerRole::Folder)
    }
}

fn prepare_layer(
    timeline: &Timeline,
    layer: &Layer,
    timeline_frames: u32,
    ctx: &ExportContext,
) -> Vec<PreparedLayer> {
    if layer.properties.role == LayerRole::Folder {
        return vec![PreparedLayer {
            original_id: Some(layer.id.clone()),
            is_primary_for_id: true,
            name: if layer.name.is_empty() {
                "Folder".to_owned()
            } else {
                layer.name.clone()
            },
            visible: layer.visible,
            locked: layer.locked,
            properties: layer.properties.clone(),
            frames: Vec::new(),
        }];
    }

    let baked_keys = expand_motion_spans(timeline, layer, timeline_frames);
    let split_key_sets = split_multi_element_continuous_instances(&baked_keys, ctx);
    let total_splits = split_key_sets.len();
    let mut out = Vec::with_capacity(total_splits);
    for (sub_idx, sub_keys) in split_key_sets.into_iter().enumerate() {
        let is_primary = sub_idx == 0;
        let frames = build_prepared_frames(layer, &sub_keys, timeline_frames, ctx);
        out.push(PreparedLayer {
            original_id: Some(layer.id.clone()),
            is_primary_for_id: is_primary,
            name: if layer.name.is_empty() {
                "Layer".to_owned()
            } else if total_splits == 1 || is_primary {
                layer.name.clone()
            } else {
                format!("{} ({})", layer.name, sub_idx + 1)
            },
            visible: layer.visible,
            locked: layer.locked,
            properties: layer.properties.clone(),
            frames,
        });
    }
    out
}

/// Evaluates `TweenKind::Motion` property-track spans into standard pose keyframes so
/// XFL 2.2 receives classic tweens without unsupported `MotionObjectXML` payloads.
fn expand_motion_spans(timeline: &Timeline, layer: &Layer, timeline_frames: u32) -> Vec<Keyframe> {
    let mut sorted = layer.keyframes.clone();
    sorted.sort_by_key(|k| k.frame);
    // Deduplicate same-frame keys if any exist by keeping the last entry at each frame.
    let mut deduped: Vec<Keyframe> = Vec::with_capacity(sorted.len());
    for k in sorted {
        if let Some(last) = deduped.last_mut()
            && last.frame == k.frame
        {
            *last = k;
        } else {
            deduped.push(k);
        }
    }

    let layer_end = layer_effective_end(layer, timeline_frames);
    let mut out: Vec<Keyframe> = Vec::with_capacity(deduped.len());
    let mut idx = 0usize;
    while idx < deduped.len() {
        let start = &deduped[idx];
        if start.keyed_properties.is_none()
            && start.tween.is_some()
            && start.tween_kind == TweenKind::Motion
        {
            let mut prop_end_idx = idx + 1;
            while prop_end_idx < deduped.len() && deduped[prop_end_idx].keyed_properties.is_some() {
                prop_end_idx += 1;
            }
            let last_prop_frame = if prop_end_idx > idx + 1 {
                deduped[prop_end_idx - 1].frame
            } else {
                start.frame
            };
            let next_ord_frame = deduped.get(prop_end_idx).map(|k| k.frame);
            let raw_span_end = next_ord_frame
                .unwrap_or(last_prop_frame)
                .max(last_prop_frame)
                .max(start.frame.saturating_add(1));
            let bake_end = raw_span_end.min(layer_end.max(start.frame.saturating_add(1)));

            let mut meta_by_frame: HashMap<u32, (String, Option<FrameSound>)> = HashMap::new();
            for k in &deduped[idx..prop_end_idx] {
                meta_by_frame.insert(k.frame, (k.label.clone(), k.sound.clone()));
            }

            for f in start.frame..bake_end {
                let (label, sound) = meta_by_frame.remove(&f).unwrap_or_default();
                let has_following_frame = f + 1 < bake_end || next_ord_frame == Some(bake_end);
                out.push(Keyframe {
                    frame: f,
                    elements: sample_layer_in(timeline, layer, f),
                    tween: has_following_frame.then_some(Easing::Linear),
                    tween_kind: TweenKind::Classic,
                    label,
                    sound,
                    ..Default::default()
                });
            }
            if next_ord_frame.is_none() && bake_end < layer_end {
                let (label, sound) = meta_by_frame.remove(&bake_end).unwrap_or_default();
                out.push(Keyframe {
                    frame: bake_end,
                    elements: sample_layer_in(timeline, layer, bake_end),
                    tween: None,
                    tween_kind: TweenKind::Classic,
                    label,
                    sound,
                    ..Default::default()
                });
            }
            idx = prop_end_idx;
        } else {
            let mut clean = start.clone();
            clean.keyed_properties = None;
            out.push(clean);
            idx += 1;
        }
    }
    out
}

struct ElementTrack {
    elem_id: Id,
    symbol_and_name: Option<(Id, String)>,
    by_key: Vec<Option<Element>>,
}

/// When a multi-element layer places the same library symbol across adjacent keyframes,
/// `xfl.rs` cannot connect multi-element keyframes via `classic_tween_matches` and would
/// reject the boundary as `ambiguous_instance_continuity`. Splitting into ordered single-track
/// sub-layers preserves exact painter order while giving each continuous symbol instance its
/// own 1-element layer.
fn split_multi_element_continuous_instances(
    keys: &[Keyframe],
    ctx: &ExportContext,
) -> Vec<Vec<Keyframe>> {
    let has_multi = keys.iter().any(|k| k.elements.len() > 1);
    if !has_multi || keys.len() < 2 {
        return vec![keys.to_vec()];
    }
    let has_adjacent_same_symbol = keys.windows(2).any(|pair| {
        if pair[0].elements.len() <= 1 && pair[1].elements.len() <= 1 {
            return false;
        }
        pair[0].elements.iter().any(|ea| {
            let ElementKind::Instance { symbol_id: sa, .. } = &ea.kind else {
                return false;
            };
            ctx.symbols.contains_key(sa)
                && pair[1].elements.iter().any(|eb| {
                    matches!(&eb.kind, ElementKind::Instance { symbol_id: sb, .. } if sb == sa)
                })
        })
    });
    if !has_adjacent_same_symbol {
        return vec![keys.to_vec()];
    }

    let mut tracks: Vec<ElementTrack> = Vec::new();
    for (ki, key) in keys.iter().enumerate() {
        let mut min_track_idx = 0usize;
        for elem in &key.elements {
            let sym_key = match &elem.kind {
                ElementKind::Instance { symbol_id, .. } => {
                    Some((symbol_id.clone(), elem.name.clone()))
                }
                _ => None,
            };
            let matched = tracks[min_track_idx..]
                .iter()
                .position(|tr| {
                    tr.by_key[ki].is_none()
                        && (tr.elem_id == elem.id
                            || (sym_key.is_some() && tr.symbol_and_name == sym_key))
                })
                .map(|rel| min_track_idx + rel);

            if let Some(t_idx) = matched {
                tracks[t_idx].by_key[ki] = Some(elem.clone());
                min_track_idx = t_idx + 1;
            } else {
                let mut by_key = vec![None; keys.len()];
                by_key[ki] = Some(elem.clone());
                tracks.insert(
                    min_track_idx,
                    ElementTrack {
                        elem_id: elem.id.clone(),
                        symbol_and_name: sym_key,
                        by_key,
                    },
                );
                min_track_idx += 1;
            }
        }
    }

    if tracks.is_empty() {
        return vec![keys.to_vec()];
    }

    tracks
        .into_iter()
        .enumerate()
        .map(|(ti, tr)| {
            keys.iter()
                .enumerate()
                .map(|(ki, orig_k)| Keyframe {
                    frame: orig_k.frame,
                    elements: tr.by_key[ki].iter().cloned().collect(),
                    tween: orig_k.tween,
                    tween_kind: orig_k.tween_kind,
                    orient_to_path: orig_k.orient_to_path,
                    label: if ti == 0 {
                        orig_k.label.clone()
                    } else {
                        String::new()
                    },
                    sound: if ti == 0 { orig_k.sound.clone() } else { None },
                    ..Default::default()
                })
                .collect()
        })
        .collect()
}

struct RenderedKeyElements {
    xml_items: Vec<String>,
    all_shapes: bool,
}

fn render_keyframe_elements(elements: &[Element], ctx: &ExportContext) -> RenderedKeyElements {
    let mut xml_items = Vec::with_capacity(elements.len());
    let mut all_shapes = true;
    for elem in elements {
        let (rendered, is_shape) = render_element(elem, ctx);
        if !rendered.is_empty() {
            if !is_shape {
                all_shapes = false;
            }
            xml_items.extend(rendered);
        }
    }
    if xml_items.is_empty() {
        all_shapes = false;
    }
    RenderedKeyElements {
        xml_items,
        all_shapes,
    }
}

fn classic_tween_elements_compatible(
    a: &Element,
    b: &Element,
    a_xml_count: usize,
    b_xml_count: usize,
    ctx: &ExportContext,
) -> bool {
    if a_xml_count != 1 || b_xml_count != 1 || a.name != b.name {
        return false;
    }
    // Check that neither element will decompose into separate fill+stroke Elements on XFL re-import.
    if shape_splits_on_reimport(a) || shape_splits_on_reimport(b) {
        return false;
    }
    match (&a.kind, &b.kind) {
        (
            ElementKind::Instance { symbol_id: sa, .. },
            ElementKind::Instance { symbol_id: sb, .. },
        ) => sa == sb && ctx.symbols.contains_key(sa),
        (
            ElementKind::Bitmap {
                resource_id: ra, ..
            },
            ElementKind::Bitmap {
                resource_id: rb, ..
            },
        ) => ra == rb && ctx.bitmaps.contains_key(ra),
        (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
        | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. }) => {
            let a_plain = !a.has_parametric_options() && !element_has_visible_stroke(a);
            let b_plain = !b.has_parametric_options() && !element_has_visible_stroke(b);
            a_plain == b_plain
        }
        (ElementKind::Path { .. }, ElementKind::Path { .. })
        | (ElementKind::PaintedPath { .. }, ElementKind::PaintedPath { .. })
        | (ElementKind::Text { .. }, ElementKind::Text { .. })
        | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. }) => true,
        _ => false,
    }
}

fn element_has_visible_stroke(elem: &Element) -> bool {
    let ct = elem
        .transform
        .color_transform
        .with_alpha(elem.transform.alpha);
    match &elem.kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && ct.apply(*stroke)[3] > 0,
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            *stroke_width > 0.0
                && (stroke_gradient
                    .as_ref()
                    .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                    || ct.apply(*stroke)[3] > 0)
        }
        _ => false,
    }
}

/// Returns true when a `<DOMShape>` has both fill and stroke and uses curves/gradients/multiple
/// contours, causing `xfl/shape.rs::general_shape` to emit 2 `PaintedPath` elements on import.
fn shape_splits_on_reimport(elem: &Element) -> bool {
    let ct = elem
        .transform
        .color_transform
        .with_alpha(elem.transform.alpha);
    match &elem.kind {
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let has_fill = fill_gradient
                .as_ref()
                .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                || ct.apply(*fill)[3] > 0;
            let has_stroke = *stroke_width > 0.0
                && (stroke_gradient
                    .as_ref()
                    .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                    || ct.apply(*stroke)[3] > 0);
            if !(has_fill && has_stroke) {
                return false;
            }
            fill_gradient.is_some()
                || stroke_gradient.is_some()
                || commands.iter().any(|c| {
                    matches!(
                        c,
                        PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
                    )
                })
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let has_fill = ct.apply(*fill)[3] > 0;
            let has_stroke = *stroke_width > 0.0 && ct.apply(*stroke)[3] > 0;
            has_fill
                && has_stroke
                && commands.iter().any(|c| {
                    matches!(
                        c,
                        PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
                    )
                })
        }
        ElementKind::Ellipse { fill, .. } => {
            let has_fill = elem.oval_options().close_path && ct.apply(*fill)[3] > 0;
            has_fill && element_has_visible_stroke(elem)
        }
        ElementKind::Rectangle { fill, .. } => {
            ct.apply(*fill)[3] > 0
                && element_has_visible_stroke(elem)
                && elem.has_parametric_options()
        }
        _ => false,
    }
}

fn build_prepared_frames(
    layer: &Layer,
    keys: &[Keyframe],
    timeline_frames: u32,
    ctx: &ExportContext,
) -> Vec<PreparedFrame> {
    if layer.properties.end_frame == Some(0) {
        return Vec::new();
    }
    let layer_end = layer_effective_end(layer, timeline_frames);
    if keys.is_empty() {
        return vec![PreparedFrame {
            index: 0,
            duration: layer_end.max(1),
            label: String::new(),
            tween: None,
            sound: None,
            script: None,
            elements_xml: Vec::new(),
        }];
    }

    let rendered: Vec<RenderedKeyElements> = keys
        .iter()
        .map(|k| render_keyframe_elements(&k.elements, ctx))
        .collect();

    let mut frames = Vec::with_capacity(keys.len() + 2);
    if keys[0].frame > 0 {
        frames.push(PreparedFrame {
            index: 0,
            duration: keys[0].frame,
            label: String::new(),
            tween: None,
            sound: None,
            script: None,
            elements_xml: Vec::new(),
        });
    }

    let has_motion_guide = layer.properties.motion_guide.is_some();

    for (i, key) in keys.iter().enumerate() {
        let f0 = key.frame;
        let f1 = if let Some(next_k) = keys.get(i + 1) {
            next_k.frame.max(f0.saturating_add(1))
        } else {
            layer_end.max(f0.saturating_add(1))
        };
        let duration = f1 - f0;
        let cur_rendered = &rendered[i];
        let next_pair = keys.get(i + 1).zip(rendered.get(i + 1));

        let authored_tween = key.tween.and_then(|ease| {
            let (next_k, next_r) = next_pair?;
            match key.tween_kind {
                TweenKind::Shape => (cur_rendered.all_shapes && next_r.all_shapes).then_some((
                    ease,
                    TweenKind::Shape,
                    false,
                )),
                TweenKind::Classic | TweenKind::Motion => (key.elements.len() == 1
                    && next_k.elements.len() == 1
                    && classic_tween_elements_compatible(
                        &key.elements[0],
                        &next_k.elements[0],
                        cur_rendered.xml_items.len(),
                        next_r.xml_items.len(),
                        ctx,
                    ))
                .then_some((ease, TweenKind::Classic, key.orient_to_path)),
            }
        });

        // Bridge untweened continuous symbol-instance keyframes on non-guided layers so
        // `xfl.rs` preserves continuous `element.id` without `ambiguous_instance_continuity`.
        let needs_instance_bridge = key.tween.is_none()
            && !has_motion_guide
            && next_pair.is_some_and(|(next_k, next_r)| {
                key.elements.len() == 1
                    && next_k.elements.len() == 1
                    && cur_rendered.xml_items.len() == 1
                    && next_r.xml_items.len() == 1
                    && key.elements[0].name == next_k.elements[0].name
                    && matches!(
                        (&key.elements[0].kind, &next_k.elements[0].kind),
                        (
                            ElementKind::Instance { symbol_id: sa, .. },
                            ElementKind::Instance { symbol_id: sb, .. }
                        ) if sa == sb && ctx.symbols.contains_key(sa)
                    )
            });

        if needs_instance_bridge {
            let linear_classic = Some((Easing::Linear, TweenKind::Classic, false));
            if duration == 1 {
                frames.push(PreparedFrame {
                    index: f0,
                    duration: 1,
                    label: key.label.clone(),
                    tween: linear_classic,
                    sound: key.sound.clone(),
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
            } else {
                frames.push(PreparedFrame {
                    index: f0,
                    duration: duration - 1,
                    label: key.label.clone(),
                    tween: linear_classic,
                    sound: key.sound.clone(),
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
                frames.push(PreparedFrame {
                    index: f1 - 1,
                    duration: 1,
                    label: String::new(),
                    tween: linear_classic,
                    sound: None,
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
            }
        } else {
            frames.push(PreparedFrame {
                index: f0,
                duration,
                label: key.label.clone(),
                tween: authored_tween,
                sound: key.sound.clone(),
                script: None,
                elements_xml: cur_rendered.xml_items.clone(),
            });
        }
    }

    frames
}

fn render_dom_layer(
    xml: &mut String,
    indent: &str,
    layer: &PreparedLayer,
    parent_layer_index: Option<usize>,
    ctx: &ExportContext,
) {
    write!(
        xml,
        "{indent}<DOMLayer name=\"{}\"",
        escape_xml_attr(&layer.name)
    )
    .unwrap();
    if let Some(rgb) = layer.properties.outline_color {
        write!(xml, " color=\"{}\"", fmt_hex_rgb(rgb)).unwrap();
    }
    match layer.properties.role {
        LayerRole::Normal => {}
        LayerRole::Guide => xml.push_str(" layerType=\"guide\""),
        LayerRole::Mask => xml.push_str(" layerType=\"mask\""),
        LayerRole::Folder => xml.push_str(" layerType=\"folder\""),
    }
    if let Some(p_idx) = parent_layer_index {
        write!(xml, " parentLayerIndex=\"{p_idx}\"").unwrap();
    }
    if !layer.visible {
        xml.push_str(" visible=\"false\"");
    }
    if layer.locked {
        xml.push_str(" locked=\"true\"");
    }
    if layer.properties.outline {
        xml.push_str(" outline=\"true\"");
    }
    if layer.properties.role == LayerRole::Folder && layer.properties.collapsed {
        xml.push_str(" open=\"false\"");
    }
    xml.push_str(">\n");

    if layer.frames.is_empty() {
        writeln!(xml, "{indent}  <frames/>").unwrap();
    } else {
        writeln!(xml, "{indent}  <frames>").unwrap();
        let frame_indent = format!("{indent}    ");
        for frame in &layer.frames {
            render_dom_frame(xml, &frame_indent, frame, ctx);
        }
        writeln!(xml, "{indent}  </frames>").unwrap();
    }
    writeln!(xml, "{indent}</DOMLayer>").unwrap();
}

fn render_dom_frame(xml: &mut String, indent: &str, frame: &PreparedFrame, ctx: &ExportContext) {
    write!(xml, "{indent}<DOMFrame index=\"{}\"", frame.index).unwrap();
    if frame.duration > 1 {
        write!(xml, " duration=\"{}\"", frame.duration).unwrap();
    }
    if !frame.label.is_empty() {
        write!(
            xml,
            " name=\"{}\" labelType=\"name\"",
            escape_xml_attr(&frame.label)
        )
        .unwrap();
    }

    let mut custom_ease_bezier: Option<[f64; 4]> = None;
    if let Some((ease, kind, orient_to_path)) = frame.tween {
        let tween_type = match kind {
            TweenKind::Shape => "shape",
            TweenKind::Classic | TweenKind::Motion => "motion",
        };
        write!(xml, " tweenType=\"{tween_type}\"").unwrap();
        match ease {
            Easing::Linear => {}
            Easing::Flash { acceleration } => {
                let clamped = acceleration.clamp(-100.0, 100.0);
                if clamped.abs() > 1e-9 {
                    write!(xml, " acceleration=\"{}\"", fmt_num(clamped)).unwrap();
                }
            }
            Easing::EaseIn => {
                xml.push_str(" acceleration=\"-100\"");
            }
            Easing::EaseOut => {
                xml.push_str(" acceleration=\"100\"");
            }
            Easing::EaseInOut => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([0.42, 0.0, 0.58, 1.0]);
            }
            Easing::Bounce | Easing::Elastic => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([0.34, 1.56, 0.64, 1.0]);
            }
            Easing::CubicBezier { x1, y1, x2, y2 } => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([
                    x1.clamp(0.0, 1.0),
                    y1.clamp(-100.0, 100.0),
                    x2.clamp(0.0, 1.0),
                    y2.clamp(-100.0, 100.0),
                ]);
            }
        }
        if orient_to_path && tween_type == "motion" {
            xml.push_str(" motionTweenOrientToPath=\"true\"");
        }
    }

    let mut sound_envelope_points: Option<Vec<(u64, u32, u32)>> = None;
    if let Some(snd) = &frame.sound
        && let Some(snd_asset) = ctx.sounds.get(&snd.resource)
    {
        write!(
            xml,
            " soundName=\"{}\"",
            escape_xml_attr(&snd_asset.item_name)
        )
        .unwrap();
        let sync_str = match snd.sync {
            SoundSync::Event => "event",
            SoundSync::Start => "start",
            SoundSync::Stop => "stop",
            SoundSync::Stream => "stream",
        };
        write!(xml, " soundSync=\"{sync_str}\"").unwrap();
        if snd.loop_forever {
            xml.push_str(" soundLoopMode=\"loop\"");
        } else if snd.loops > 1 {
            write!(
                xml,
                " soundLoopMode=\"repeat\" soundLoop=\"{}\"",
                snd.loops.min(FrameSound::MAX_LOOPS)
            )
            .unwrap();
        }
        if let Some(in_sec) = snd.in_point
            && in_sec.is_finite()
            && in_sec > 0.0
        {
            let in_44 =
                (in_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0).round() as u64;
            if in_44 > 0 {
                write!(xml, " inPoint44=\"{in_44}\"").unwrap();
            }
        }
        if let Some(out_sec) = snd.out_point
            && out_sec.is_finite()
            && out_sec > 0.0
        {
            let out_44 =
                (out_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0).round() as u64;
            if out_44 > 0 {
                write!(xml, " outPoint44=\"{out_44}\"").unwrap();
            }
        }
        let has_custom_env = snd.effect == SoundEffect::Custom
            || snd.envelope.as_ref().is_some_and(|v| !v.is_empty());
        let effect_str = if has_custom_env {
            "custom"
        } else {
            match snd.effect {
                SoundEffect::None => "none",
                SoundEffect::LeftChannel => "left channel",
                SoundEffect::RightChannel => "right channel",
                SoundEffect::FadeLeftToRight => "fade left to right",
                SoundEffect::FadeRightToLeft => "fade right to left",
                SoundEffect::FadeIn => "fade in",
                SoundEffect::FadeOut => "fade out",
                SoundEffect::Custom => "custom",
            }
        };
        if effect_str != "none" {
            write!(xml, " soundEffect=\"{effect_str}\"").unwrap();
        }
        if has_custom_env {
            let pts = encode_sound_envelope_points(snd, snd_asset.duration_44);
            if !pts.is_empty() {
                sound_envelope_points = Some(pts);
            }
        }
    }

    if custom_ease_bezier.is_none()
        && sound_envelope_points.is_none()
        && frame.script.is_none()
        && frame.elements_xml.is_empty()
    {
        xml.push_str("/>\n");
        return;
    }

    xml.push_str(">\n");
    if let Some([x1, y1, x2, y2]) = custom_ease_bezier {
        writeln!(xml, "{indent}  <tweens>").unwrap();
        writeln!(xml, "{indent}    <CustomEase target=\"all\">").unwrap();
        writeln!(xml, "{indent}      <Point x=\"0\" y=\"0\"/>").unwrap();
        writeln!(
            xml,
            "{indent}      <Point x=\"{}\" y=\"{}\"/>",
            fmt_num(x1),
            fmt_num(y1)
        )
        .unwrap();
        writeln!(
            xml,
            "{indent}      <Point x=\"{}\" y=\"{}\"/>",
            fmt_num(x2),
            fmt_num(y2)
        )
        .unwrap();
        writeln!(xml, "{indent}      <Point x=\"1\" y=\"1\"/>").unwrap();
        writeln!(xml, "{indent}    </CustomEase>").unwrap();
        writeln!(xml, "{indent}  </tweens>").unwrap();
    }

    if let Some(env_pts) = sound_envelope_points {
        writeln!(xml, "{indent}  <SoundEnvelope>").unwrap();
        for (mark44, level0, level1) in env_pts {
            writeln!(
                xml,
                "{indent}    <SoundEnvelopePoint mark44=\"{mark44}\" level0=\"{level0}\" level1=\"{level1}\"/>"
            )
            .unwrap();
        }
        writeln!(xml, "{indent}  </SoundEnvelope>").unwrap();
    }

    if let Some(script_src) = &frame.script {
        writeln!(xml, "{indent}  <Actionscript>").unwrap();
        writeln!(
            xml,
            "{indent}    <script>{}</script>",
            cdata_wrap(script_src)
        )
        .unwrap();
        writeln!(xml, "{indent}  </Actionscript>").unwrap();
    }

    if !frame.elements_xml.is_empty() {
        writeln!(xml, "{indent}  <elements>").unwrap();
        for item_xml in &frame.elements_xml {
            let mut in_cdata = false;
            for line in item_xml.lines() {
                if in_cdata {
                    writeln!(xml, "{line}").unwrap();
                } else {
                    writeln!(xml, "{indent}    {line}").unwrap();
                }
                if let Some(open_pos) = line.rfind("<![CDATA[") {
                    in_cdata = !line[open_pos + 9..].contains("]]>");
                } else if in_cdata && line.contains("]]>") {
                    in_cdata = false;
                }
            }
        }
        writeln!(xml, "{indent}  </elements>").unwrap();
    }
    writeln!(xml, "{indent}</DOMFrame>").unwrap();
}

fn encode_sound_envelope_points(snd: &FrameSound, duration_44: u64) -> Vec<(u64, u32, u32)> {
    let raw = snd.envelope_points();
    let denom = if duration_44 > 0 { duration_44 } else { 44_100 };
    let mut encoded: Vec<(u64, u32, u32)> = raw
        .into_iter()
        .take(FrameSound::MAX_ENVELOPE_POINTS)
        .map(|[pos, left, right]| {
            let p = if pos.is_finite() {
                pos.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let l = if left.is_finite() {
                left.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let r = if right.is_finite() {
                right.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mark44 = (p * denom as f64).round() as u64;
            let level0 = (l * 32_768.0).round() as u32;
            let level1 = (r * 32_768.0).round() as u32;
            (mark44, level0, level1)
        })
        .collect();
    encoded.sort_by_key(|pt| pt.0);
    encoded
}

/// Renders one `Element` to zero, one, or more XFL `<elements>` child blocks and returns
/// `(xml_blocks, is_vector_shape)`.
fn render_element(element: &Element, ctx: &ExportContext) -> (Vec<String>, bool) {
    let ct = element
        .transform
        .color_transform
        .with_alpha(element.transform.alpha);

    match &element.kind {
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let Some(sym_info) = ctx.symbols.get(symbol_id) else {
                return (Vec::new(), false);
            };
            (
                vec![render_symbol_instance(element, sym_info, *playback, ct)],
                false,
            )
        }
        ElementKind::Bitmap { resource_id, .. } => {
            let Some(bmp_info) = ctx.bitmaps.get(resource_id) else {
                return (Vec::new(), false);
            };
            (vec![render_bitmap_instance(element, bmp_info)], false)
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => (
            vec![render_plain_static_text(
                element,
                text,
                font_family,
                *font_size,
                ct.apply(*color),
            )],
            false,
        ),
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            ..
        } => {
            let base_rgba = [
                ((format.color >> 16) & 0xff) as u8,
                ((format.color >> 8) & 0xff) as u8,
                (format.color & 0xff) as u8,
                255,
            ];
            (
                vec![render_textbox_static_text(
                    element,
                    text,
                    *width,
                    *height,
                    format,
                    ct.apply(base_rgba),
                )],
                false,
            )
        }
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
                return (Vec::new(), true);
            }
            let eff_fill = ct.apply(*fill);
            let eff_stroke = ct.apply(*stroke);
            let has_stroke = *stroke_width > 0.0 && eff_stroke[3] > 0;
            if !has_stroke && !element.has_parametric_options() {
                (
                    vec![render_primitive_object(
                        "DOMRectangleObject",
                        element,
                        *width,
                        *height,
                        eff_fill,
                    )],
                    true,
                )
            } else if let Some((commands, _)) = element.to_path_commands() {
                (
                    render_dom_shapes(
                        element,
                        &commands,
                        ShapePaintSpec {
                            fill_color: eff_fill,
                            fill_gradient: None,
                            stroke_color: eff_stroke,
                            stroke_gradient: None,
                            stroke_width: *stroke_width,
                        },
                    ),
                    true,
                )
            } else {
                (Vec::new(), true)
            }
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
                return (Vec::new(), true);
            }
            let close_path = element.oval_options().close_path;
            let eff_fill = if close_path {
                ct.apply(*fill)
            } else {
                [0, 0, 0, 0]
            };
            let eff_stroke = ct.apply(*stroke);
            let has_stroke = *stroke_width > 0.0 && eff_stroke[3] > 0;
            if !has_stroke && !element.has_parametric_options() {
                (
                    vec![render_primitive_object(
                        "DOMOvalObject",
                        element,
                        *width,
                        *height,
                        eff_fill,
                    )],
                    true,
                )
            } else if let Some((commands, _)) = element.to_path_commands() {
                (
                    render_dom_shapes(
                        element,
                        &commands,
                        ShapePaintSpec {
                            fill_color: eff_fill,
                            fill_gradient: None,
                            stroke_color: eff_stroke,
                            stroke_gradient: None,
                            stroke_width: *stroke_width,
                        },
                    ),
                    true,
                )
            } else {
                (Vec::new(), true)
            }
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => (
            render_dom_shapes(
                element,
                commands,
                ShapePaintSpec {
                    fill_color: ct.apply(*fill),
                    fill_gradient: None,
                    stroke_color: ct.apply(*stroke),
                    stroke_gradient: None,
                    stroke_width: *stroke_width,
                },
            ),
            true,
        ),
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => (
            render_dom_shapes(
                element,
                commands,
                ShapePaintSpec {
                    fill_color: ct.apply(*fill),
                    fill_gradient: fill_gradient.as_ref().map(|g| transform_gradient(g, ct)),
                    stroke_color: ct.apply(*stroke),
                    stroke_gradient: stroke_gradient.as_ref().map(|g| transform_gradient(g, ct)),
                    stroke_width: *stroke_width,
                },
            ),
            true,
        ),
        ElementKind::Video { width, height } => {
            if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
                return (Vec::new(), false);
            }
            (
                vec![render_primitive_object(
                    "DOMRectangleObject",
                    element,
                    *width,
                    *height,
                    ct.apply([0, 0, 0, 64]),
                )],
                false,
            )
        }
    }
}

fn transform_gradient(grad: &GradientPaint, ct: ColorTransform) -> GradientPaint {
    GradientPaint {
        kind: grad.kind,
        matrix: grad.matrix,
        stops: grad
            .stops
            .iter()
            .map(|s| GradientStop {
                ratio: s.ratio,
                color: ct.apply(s.color),
            })
            .collect(),
        focal_point_ratio: grad.focal_point_ratio,
    }
}

fn render_symbol_instance(
    element: &Element,
    sym_info: &SymbolExport,
    playback: InstancePlayback,
    ct: ColorTransform,
) -> String {
    let mut xml = String::with_capacity(256);
    write!(
        xml,
        "<DOMSymbolInstance libraryItemName=\"{}\" name=\"{}\"",
        escape_xml_attr(&sym_info.item_name),
        escape_xml_attr(&element.name),
    )
    .unwrap();

    match sym_info.kind {
        SymbolKind::Button => {
            xml.push_str(" symbolType=\"button\"");
        }
        SymbolKind::MovieClip | SymbolKind::Graphic => match playback {
            InstancePlayback::Independent => {
                xml.push_str(" symbolType=\"movie clip\"");
            }
            InstancePlayback::Loop => {
                xml.push_str(" symbolType=\"graphic\" loop=\"loop\"");
            }
            InstancePlayback::LoopFrom(first) => {
                xml.push_str(" symbolType=\"graphic\" loop=\"loop\"");
                if first > 0 {
                    write!(xml, " firstFrame=\"{first}\"").unwrap();
                }
            }
            InstancePlayback::PlayOnce => {
                xml.push_str(" symbolType=\"graphic\" loop=\"play once\"");
            }
            InstancePlayback::PlayOnceFrom(first) => {
                xml.push_str(" symbolType=\"graphic\" loop=\"play once\"");
                if first > 0 {
                    write!(xml, " firstFrame=\"{first}\"").unwrap();
                }
            }
            InstancePlayback::SingleFrame(first) => {
                xml.push_str(" symbolType=\"graphic\" loop=\"single frame\"");
                if first > 0 {
                    write!(xml, " firstFrame=\"{first}\"").unwrap();
                }
            }
        },
    }

    if !element.visible {
        xml.push_str(" isVisible=\"false\"");
    }
    if element.cache_as_bitmap {
        xml.push_str(" cacheAsBitmap=\"true\"");
    }
    if !element.blend_mode.is_normal() {
        write!(xml, " blendMode=\"{}\"", element.blend_mode.as_str()).unwrap();
    }
    if let Some(acc) = &element.accessibility {
        xml.push_str(" hasAccessibleData=\"true\"");
        if !acc.name.is_empty() {
            write!(xml, " accName=\"{}\"", escape_xml_attr(&acc.name)).unwrap();
        }
        if !acc.description.is_empty() {
            write!(
                xml,
                " description=\"{}\"",
                escape_xml_attr(&acc.description)
            )
            .unwrap();
        }
        if acc.tab_order > 0 {
            write!(xml, " tabIndex=\"{}\"", acc.tab_order).unwrap();
        }
        if acc.disabled {
            xml.push_str(" silent=\"true\"");
        }
    }

    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    append_instance_color(&mut children, ct);
    append_instance_filters(&mut children, &element.filters);
    append_object_actionscript(&mut children, &element.scripts);

    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        xml.push_str("</DOMSymbolInstance>");
    }
    xml
}

fn append_instance_filters(out: &mut String, filters: &[FilterSpec]) {
    if filters.is_empty() {
        return;
    }
    out.push_str("  <filters>\n");
    for filter in filters {
        match filter {
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            } => {
                writeln!(
                    out,
                    "    <BlurFilter blurX=\"{}\" blurY=\"{}\" quality=\"{quality}\"/>",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                )
                .unwrap();
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
                write!(
                    out,
                    "    <GlowFilter blurX=\"{}\" blurY=\"{}\" color=\"#{:06X}\" alpha=\"{}\" strength=\"{}\" quality=\"{quality}\"",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    color & 0xff_ffff,
                    fmt_num(*alpha),
                    fmt_num(*strength * 100.0),
                )
                .unwrap();
                if *inner {
                    out.push_str(" inner=\"true\"");
                }
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str("/>\n");
            }
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
                write!(
                    out,
                    "    <DropShadowFilter angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" color=\"#{:06X}\" alpha=\"{}\" strength=\"{}\" quality=\"{quality}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    color & 0xff_ffff,
                    fmt_num(*alpha),
                    fmt_num(*strength * 100.0),
                )
                .unwrap();
                if *inner {
                    out.push_str(" inner=\"true\"");
                }
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                if *hide_object {
                    out.push_str(" hideObject=\"true\"");
                }
                out.push_str("/>\n");
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
                write!(
                    out,
                    "    <BevelFilter angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" highlightColor=\"#{:06X}\" highlightAlpha=\"{}\" shadowColor=\"#{:06X}\" shadowAlpha=\"{}\" strength=\"{}\" quality=\"{quality}\" type=\"{}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    highlight_color & 0xff_ffff,
                    fmt_num(*highlight_alpha),
                    shadow_color & 0xff_ffff,
                    fmt_num(*shadow_alpha),
                    fmt_num(*strength * 100.0),
                    escape_xml_attr(placement),
                )
                .unwrap();
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str("/>\n");
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
            }
            | FilterSpec::GradientBevel {
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
                let tag = if matches!(filter, FilterSpec::GradientGlow { .. }) {
                    "GradientGlowFilter"
                } else {
                    "GradientBevelFilter"
                };
                write!(
                    out,
                    "    <{tag} angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" strength=\"{}\" quality=\"{quality}\" type=\"{}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    fmt_num(*strength * 100.0),
                    escape_xml_attr(placement),
                )
                .unwrap();
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str(">\n");
                let count = colors.len().min(alphas.len()).min(ratios.len());
                for i in 0..count {
                    writeln!(
                        out,
                        "      <GradientEntry color=\"#{:06X}\" alpha=\"{}\" ratio=\"{}\"/>",
                        colors[i] & 0xff_ffff,
                        fmt_num(alphas[i]),
                        fmt_num(f64::from(ratios[i]) / 255.0),
                    )
                    .unwrap();
                }
                writeln!(out, "    </{tag}>").unwrap();
            }
            FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {
                out.push_str(
                    "    <AdjustColorFilter brightness=\"0\" contrast=\"0\" saturation=\"0\" hue=\"0\"/>\n",
                );
            }
        }
    }
    out.push_str("  </filters>\n");
}

fn append_instance_color(out: &mut String, ct: ColorTransform) {
    let m = ct.multipliers.map(|v| if v.is_finite() { v } else { 1.0 });
    let o = ct.offsets.map(|v| if v.is_finite() { v } else { 0.0 });
    let rgb_identity = (m[0] - 1.0).abs() <= 1e-6
        && (m[1] - 1.0).abs() <= 1e-6
        && (m[2] - 1.0).abs() <= 1e-6
        && o.iter().all(|v| v.abs() <= 1e-6);
    let alpha_identity = (m[3] - 1.0).abs() <= 1e-6;
    if rgb_identity && alpha_identity {
        return;
    }
    out.push_str("  <color>\n    <Color");
    if !alpha_identity {
        write!(out, " alphaMultiplier=\"{}\"", fmt_num(m[3])).unwrap();
    }
    if !rgb_identity {
        for (val, attr) in [
            (m[0], "redMultiplier"),
            (m[1], "greenMultiplier"),
            (m[2], "blueMultiplier"),
        ] {
            if (val - 1.0).abs() > 1e-6 {
                write!(out, " {attr}=\"{}\"", fmt_num(val)).unwrap();
            }
        }
        for (val, attr) in [
            (o[3], "alphaOffset"),
            (o[0], "redOffset"),
            (o[1], "greenOffset"),
            (o[2], "blueOffset"),
        ] {
            if val.abs() > 1e-6 {
                write!(out, " {attr}=\"{}\"", fmt_num(val)).unwrap();
            }
        }
    }
    out.push_str("/>\n  </color>\n");
}

fn append_object_actionscript(out: &mut String, scripts: &[ObjectScript]) {
    let mut blocks: Vec<String> = Vec::new();
    for os in scripts {
        let trimmed = os.script.source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body = encode_script_with_language_tag(&os.script);
        if is_actionscript(os.script.language) && starts_with_flash_handler(trimmed) {
            blocks.push(body);
        } else {
            let header = match os.event {
                ObjectEvent::Click => "on (release)",
                ObjectEvent::Load => "onClipEvent (load)",
                ObjectEvent::EnterFrame => "onClipEvent (enterFrame)",
            };
            blocks.push(format!("{header} {{\n{body}\n}}"));
        }
    }
    if !blocks.is_empty() {
        let joined = blocks.join("\n");
        writeln!(
            out,
            "  <Actionscript>\n    <script>{}</script>\n  </Actionscript>",
            cdata_wrap(&joined)
        )
        .unwrap();
    }
}

fn starts_with_flash_handler(trimmed: &str) -> bool {
    for prefix in ["on", "onClipEvent"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && rest.trim_start().starts_with('(')
        {
            return true;
        }
    }
    false
}

fn render_bitmap_instance(element: &Element, bmp_info: &BitmapExport) -> String {
    let mut xml = String::with_capacity(160);
    write!(
        xml,
        "<DOMBitmapInstance libraryItemName=\"{}\"",
        escape_xml_attr(&bmp_info.item_name)
    )
    .unwrap();
    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        xml.push_str("</DOMBitmapInstance>");
    }
    xml
}

fn render_plain_static_text(
    element: &Element,
    text: &str,
    font_family: &str,
    font_size: f64,
    rgba: Color,
) -> String {
    let size = if font_size.is_finite() {
        font_size.clamp(1.0, 10_000.0)
    } else {
        12.0
    };
    let face = if font_family.trim().is_empty() {
        "Arial"
    } else {
        font_family
    };
    let char_count = text.chars().count().max(1) as f64;
    let width = (char_count * size * 0.6).max(4.0);
    let height = (size * 1.2).max(4.0);

    let mut xml = String::with_capacity(320);
    writeln!(
        xml,
        "<DOMStaticText name=\"{}\" width=\"{}\" height=\"{}\" isSelectable=\"false\">",
        escape_xml_attr(&element.name),
        fmt_num(width),
        fmt_num(height),
    )
    .unwrap();
    append_matrix_and_pivot(&mut xml, element.transform);
    xml.push_str("  <textRuns>\n    <DOMTextRun>\n");
    writeln!(
        xml,
        "      <characters>{}</characters>",
        escape_xfl_characters(text)
    )
    .unwrap();
    xml.push_str("      <textAttrs>\n");
    let (hex, alpha_opt) = fmt_color_alpha(rgba);
    let bitmap_size = (size * 20.0).round().clamp(20.0, 200_000.0) as u32;
    write!(
        xml,
        "        <DOMTextAttrs face=\"{}\" size=\"{}\" bitmapSize=\"{bitmap_size}\" fillColor=\"{hex}\"",
        escape_xml_attr(face),
        fmt_num(size),
    )
    .unwrap();
    if let Some(alpha_str) = alpha_opt {
        write!(xml, " alpha=\"{alpha_str}\"").unwrap();
    }
    xml.push_str("/>\n      </textAttrs>\n    </DOMTextRun>\n  </textRuns>\n</DOMStaticText>");
    xml
}

fn render_textbox_static_text(
    element: &Element,
    text: &str,
    width: f64,
    height: f64,
    format: &neoflash_core::TextFormat,
    rgba: Color,
) -> String {
    let size = if format.size.is_finite() {
        format.size.clamp(1.0, 10_000.0)
    } else {
        12.0
    };
    let face = if format.font.trim().is_empty() {
        "Times New Roman"
    } else {
        format.font.as_str()
    };
    let w = if width.is_finite() && width > 0.0 {
        width
    } else {
        100.0
    };
    let h = if height.is_finite() && height > 0.0 {
        height
    } else {
        (size * 1.2).max(4.0)
    };

    let mut xml = String::with_capacity(360);
    writeln!(
        xml,
        "<DOMStaticText name=\"{}\" width=\"{}\" height=\"{}\" isSelectable=\"false\">",
        escape_xml_attr(&element.name),
        fmt_num(w),
        fmt_num(h),
    )
    .unwrap();
    append_matrix_and_pivot(&mut xml, element.transform);
    xml.push_str("  <textRuns>\n    <DOMTextRun>\n");
    writeln!(
        xml,
        "      <characters>{}</characters>",
        escape_xfl_characters(text)
    )
    .unwrap();
    xml.push_str("      <textAttrs>\n");
    let (hex, alpha_opt) = fmt_color_alpha(rgba);
    let bitmap_size = (size * 20.0).round().clamp(20.0, 200_000.0) as u32;
    write!(
        xml,
        "        <DOMTextAttrs face=\"{}\" size=\"{}\" bitmapSize=\"{bitmap_size}\" fillColor=\"{hex}\"",
        escape_xml_attr(face),
        fmt_num(size),
    )
    .unwrap();
    if let Some(alpha_str) = alpha_opt {
        write!(xml, " alpha=\"{alpha_str}\"").unwrap();
    }
    if format.bold {
        xml.push_str(" bold=\"true\"");
    }
    if format.italic {
        xml.push_str(" italic=\"true\"");
    }
    match format.align {
        TextAlign::Left => {}
        TextAlign::Center => xml.push_str(" alignment=\"center\""),
        TextAlign::Right => xml.push_str(" alignment=\"right\""),
    }
    for (val, attr) in [
        (format.leading, "lineSpacing"),
        (format.letter_spacing, "letterSpacing"),
        (format.left_margin, "leftMargin"),
        (format.right_margin, "rightMargin"),
        (format.indent, "indent"),
    ] {
        if val.is_finite() && val.abs() > 1e-6 {
            write!(xml, " {attr}=\"{}\"", fmt_num(val)).unwrap();
        }
    }
    xml.push_str("/>\n      </textAttrs>\n    </DOMTextRun>\n  </textRuns>\n</DOMStaticText>");
    xml
}

fn render_primitive_object(
    tag: &str,
    element: &Element,
    width: f64,
    height: f64,
    fill: Color,
) -> String {
    let mut xml = String::with_capacity(220);
    write!(
        xml,
        "<{tag} name=\"{}\" objectWidth=\"{}\" objectHeight=\"{}\"",
        escape_xml_attr(&element.name),
        fmt_num(width),
        fmt_num(height),
    )
    .unwrap();

    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    if fill[3] > 0 {
        let (hex, alpha_opt) = fmt_color_alpha(fill);
        children.push_str("  <fill>\n");
        write!(children, "    <SolidColor color=\"{hex}\"").unwrap();
        if let Some(a) = alpha_opt {
            write!(children, " alpha=\"{a}\"").unwrap();
        }
        children.push_str("/>\n  </fill>\n");
    }

    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        write!(xml, "</{tag}>").unwrap();
    }
    xml
}

struct ShapePaintSpec {
    fill_color: Color,
    fill_gradient: Option<GradientPaint>,
    stroke_color: Color,
    stroke_gradient: Option<GradientPaint>,
    stroke_width: f64,
}

#[derive(Clone, Copy)]
struct ShapeSeg {
    start: Point24_8,
    ctrl: Option<Point24_8>,
    end: Point24_8,
}

#[derive(Clone, Copy)]
struct StyledSeg {
    seg: ShapeSeg,
    has_fill: bool,
    has_stroke: bool,
}

fn render_dom_shapes(
    element: &Element,
    commands: &[PathCommand],
    paint: ShapePaintSpec,
) -> Vec<String> {
    let valid_fill_grad = paint.fill_gradient.as_ref().and_then(sanitize_gradient);
    let has_fill_paint = valid_fill_grad
        .as_ref()
        .is_some_and(|g| g.stops.iter().any(|s| s.color[3] > 0))
        || paint.fill_color[3] > 0;

    let valid_stroke_grad = paint.stroke_gradient.as_ref().and_then(sanitize_gradient);
    let has_stroke_paint = paint.stroke_width.is_finite()
        && paint.stroke_width > 0.0
        && (valid_stroke_grad
            .as_ref()
            .is_some_and(|g| g.stops.iter().any(|s| s.color[3] > 0))
            || paint.stroke_color[3] > 0);

    if !has_fill_paint && !has_stroke_paint {
        return Vec::new();
    }

    let contour_batches = build_styled_contours(commands, has_fill_paint, has_stroke_paint);
    if contour_batches.is_empty() {
        return Vec::new();
    }

    // Group contours into `<DOMShape>` chunks of at most `MAX_SHAPE_SEGMENTS` segments.
    let mut shape_chunks: Vec<Vec<StyledSeg>> = Vec::new();
    let mut current_chunk: Vec<StyledSeg> = Vec::new();
    for contour in contour_batches {
        if !current_chunk.is_empty() && current_chunk.len() + contour.len() > MAX_SHAPE_SEGMENTS {
            shape_chunks.push(std::mem::take(&mut current_chunk));
        }
        if contour.len() <= MAX_SHAPE_SEGMENTS {
            current_chunk.extend(contour);
        } else if contour.len() <= 16_384 {
            shape_chunks.push(contour);
        } else if !has_fill_paint {
            // Very long stroke-only polyline can be sliced directly at MAX_SHAPE_SEGMENTS.
            for slice in contour.chunks(MAX_SHAPE_SEGMENTS) {
                shape_chunks.push(slice.to_vec());
            }
        }
    }
    if !current_chunk.is_empty() {
        shape_chunks.push(current_chunk);
    }

    let mut out = Vec::with_capacity(shape_chunks.len());
    for segs in shape_chunks {
        let used_fill = segs.iter().any(|s| s.has_fill);
        let used_stroke = segs.iter().any(|s| s.has_stroke);
        if !used_fill && !used_stroke {
            continue;
        }
        let mut xml = String::with_capacity(512);
        writeln!(
            xml,
            "<DOMShape name=\"{}\">",
            escape_xml_attr(&element.name)
        )
        .unwrap();
        append_matrix_and_pivot(&mut xml, element.transform);

        if used_fill {
            xml.push_str("  <fills>\n    <FillStyle index=\"1\">\n");
            render_paint_element(
                &mut xml,
                "      ",
                paint.fill_color,
                valid_fill_grad.as_ref(),
            );
            xml.push_str("    </FillStyle>\n  </fills>\n");
        }
        if used_stroke {
            let weight = paint.stroke_width.clamp(0.05, 100_000.0);
            xml.push_str("  <strokes>\n    <StrokeStyle index=\"1\">\n");
            writeln!(
                xml,
                "      <SolidStroke scaleMode=\"normal\" weight=\"{}\">",
                fmt_num(weight)
            )
            .unwrap();
            xml.push_str("        <fill>\n");
            render_paint_element(
                &mut xml,
                "          ",
                paint.stroke_color,
                valid_stroke_grad.as_ref(),
            );
            xml.push_str(
                "        </fill>\n      </SolidStroke>\n    </StrokeStyle>\n  </strokes>\n",
            );
        }

        xml.push_str("  <edges>\n");
        render_edge_elements(&mut xml, &segs);
        xml.push_str("  </edges>\n</DOMShape>");
        out.push(xml);
    }
    out
}

fn sanitize_gradient(grad: &GradientPaint) -> Option<GradientPaint> {
    if grad.stops.is_empty() {
        return None;
    }
    let mut stops = grad.stops.clone();
    stops.sort_by_key(|s| s.ratio);
    if stops.len() > 15 {
        let total = stops.len();
        let mut sampled = Vec::with_capacity(15);
        for i in 0..15 {
            let idx = (i * (total - 1)) / 14;
            sampled.push(stops[idx].clone());
        }
        stops = sampled;
    }
    let focal = if grad.focal_point_ratio.is_finite() {
        grad.focal_point_ratio.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let candidate = GradientPaint {
        kind: grad.kind,
        matrix: grad.matrix,
        stops,
        focal_point_ratio: focal,
    };
    candidate.validate().is_ok().then_some(candidate)
}

fn render_paint_element(
    xml: &mut String,
    indent: &str,
    fallback_color: Color,
    gradient: Option<&GradientPaint>,
) {
    if let Some(grad) = gradient {
        let tag = match grad.kind {
            GradientKind::Linear => "LinearGradient",
            GradientKind::Radial => "RadialGradient",
        };
        write!(xml, "{indent}<{tag}").unwrap();
        if grad.kind == GradientKind::Radial && grad.focal_point_ratio.abs() > 1e-6 {
            write!(
                xml,
                " focalPointRatio=\"{}\"",
                fmt_num(grad.focal_point_ratio)
            )
            .unwrap();
        }
        xml.push_str(">\n");
        append_raw_matrix(xml, &format!("{indent}  "), grad.matrix, true);
        for stop in &grad.stops {
            let (hex, alpha_opt) = fmt_color_alpha(stop.color);
            let ratio = f64::from(stop.ratio) / 255.0;
            write!(
                xml,
                "{indent}  <GradientEntry color=\"{hex}\" ratio=\"{}\"",
                fmt_num(ratio)
            )
            .unwrap();
            if let Some(a) = alpha_opt {
                write!(xml, " alpha=\"{a}\"").unwrap();
            }
            xml.push_str("/>\n");
        }
        writeln!(xml, "{indent}</{tag}>").unwrap();
    } else {
        let (hex, alpha_opt) = fmt_color_alpha(fallback_color);
        write!(xml, "{indent}<SolidColor color=\"{hex}\"").unwrap();
        if let Some(a) = alpha_opt {
            write!(xml, " alpha=\"{a}\"").unwrap();
        }
        xml.push_str("/>\n");
    }
}

fn build_styled_contours(
    commands: &[PathCommand],
    has_fill: bool,
    has_stroke: bool,
) -> Vec<Vec<StyledSeg>> {
    let mut contours: Vec<Vec<StyledSeg>> = Vec::new();
    let mut start_px: Option<[f64; 2]> = None;
    let mut start_pt: Option<Point24_8> = None;
    let mut cur_px: Option<[f64; 2]> = None;
    let mut cur_pt: Option<Point24_8> = None;
    let mut explicit_close = false;
    let mut raw_segs: Vec<ShapeSeg> = Vec::new();

    let flush_contour = |raw_segs: &mut Vec<ShapeSeg>,
                         start_pt: Option<Point24_8>,
                         cur_pt: Option<Point24_8>,
                         explicit_close: bool,
                         contours: &mut Vec<Vec<StyledSeg>>| {
        let (Some(s_pt), Some(mut end_pt)) = (start_pt, cur_pt) else {
            raw_segs.clear();
            return;
        };
        if explicit_close && end_pt != s_pt {
            push_quantized_segment(raw_segs, end_pt, None, s_pt);
            end_pt = s_pt;
        }
        if raw_segs.is_empty() {
            return;
        }
        let is_closed = explicit_close || end_pt == s_pt;
        let mut styled: Vec<StyledSeg> = raw_segs
            .drain(..)
            .map(|seg| StyledSeg {
                seg,
                has_fill,
                has_stroke,
            })
            .collect();
        if has_fill && !is_closed {
            // Implicitly close the fill boundary without adding a closing stroke edge.
            styled.push(StyledSeg {
                seg: ShapeSeg {
                    start: end_pt,
                    ctrl: None,
                    end: s_pt,
                },
                has_fill: true,
                has_stroke: false,
            });
        }
        contours.push(styled);
    };

    for &cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                flush_contour(
                    &mut raw_segs,
                    start_pt,
                    cur_pt,
                    explicit_close,
                    &mut contours,
                );
                let q = point_px_to_24_8(x, y);
                start_px = Some([x, y]);
                start_pt = Some(q);
                cur_px = Some([x, y]);
                cur_pt = Some(q);
                explicit_close = false;
            }
            PathCommand::LineTo { x, y } => {
                if let Some(c_pt) = cur_pt {
                    let next_pt = point_px_to_24_8(x, y);
                    push_quantized_segment(&mut raw_segs, c_pt, None, next_pt);
                    cur_px = Some([x, y]);
                    cur_pt = Some(next_pt);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(c_pt) = cur_pt {
                    let ctrl_pt = point_px_to_24_8(cx, cy);
                    let next_pt = point_px_to_24_8(x, y);
                    push_quantized_segment(&mut raw_segs, c_pt, Some(ctrl_pt), next_pt);
                    cur_px = Some([x, y]);
                    cur_pt = Some(next_pt);
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let (Some(c_px), Some(mut c_pt)) = (cur_px, cur_pt) {
                    let mut quads = Vec::with_capacity(4);
                    cubic_to_quadratics(c_px, [cx1, cy1], [cx2, cy2], [x, y], 0, &mut quads);
                    let total_q = quads.len();
                    let final_pt = point_px_to_24_8(x, y);
                    for (qi, (ctrl_px, end_px)) in quads.into_iter().enumerate() {
                        let q_ctrl = point_px_to_24_8(ctrl_px[0], ctrl_px[1]);
                        let q_end = if qi + 1 == total_q {
                            final_pt
                        } else {
                            point_px_to_24_8(end_px[0], end_px[1])
                        };
                        push_quantized_segment(&mut raw_segs, c_pt, Some(q_ctrl), q_end);
                        c_pt = q_end;
                    }
                    cur_px = Some([x, y]);
                    cur_pt = Some(final_pt);
                }
            }
            PathCommand::Close => {
                if start_px.is_some() {
                    explicit_close = true;
                    flush_contour(
                        &mut raw_segs,
                        start_pt,
                        cur_pt,
                        explicit_close,
                        &mut contours,
                    );
                    start_px = None;
                    start_pt = None;
                    cur_px = None;
                    cur_pt = None;
                    explicit_close = false;
                }
            }
        }
    }
    flush_contour(
        &mut raw_segs,
        start_pt,
        cur_pt,
        explicit_close,
        &mut contours,
    );
    contours
}

fn push_quantized_segment(
    out: &mut Vec<ShapeSeg>,
    start: Point24_8,
    ctrl: Option<Point24_8>,
    end: Point24_8,
) {
    let simplified_ctrl = ctrl.filter(|&c| c != start && c != end);
    match simplified_ctrl {
        None => {
            if start != end {
                out.push(ShapeSeg {
                    start,
                    ctrl: None,
                    end,
                });
            }
        }
        Some(c) => {
            if start != end {
                out.push(ShapeSeg {
                    start,
                    ctrl: Some(c),
                    end,
                });
            } else {
                // Split degenerate self-loop quadratic (`start == end`, `c != start`) at t = 0.5
                // into two non-self-loop segments so `order_stroke_chain` never sees `a == b`.
                let mid = [(start[0] + c[0]) / 2, (start[1] + c[1]) / 2];
                if mid != start {
                    out.push(ShapeSeg {
                        start,
                        ctrl: None,
                        end: mid,
                    });
                    out.push(ShapeSeg {
                        start: mid,
                        ctrl: None,
                        end: start,
                    });
                }
            }
        }
    }
}

fn cubic_to_quadratics(
    p0: [f64; 2],
    p1: [f64; 2],
    p2: [f64; 2],
    p3: [f64; 2],
    depth: u8,
    out: &mut Vec<([f64; 2], [f64; 2])>,
) {
    // Degree-reduction error bound between cubic (p0, p1, p2, p3) and midpoint-matched
    // quadratic with control c = (3*(p1 + p2) - (p0 + p3)) / 4 is (sqrt(3)/36) * |p3 - 3*p2 + 3*p1 - p0|.
    let dx = p3[0] - 3.0 * p2[0] + 3.0 * p1[0] - p0[0];
    let dy = p3[1] - 3.0 * p2[1] + 3.0 * p1[1] - p0[1];
    let err = dx.hypot(dy) * 0.048_112_522_432_468_81;

    if depth >= CUBIC_SUBDIV_MAX_DEPTH || !err.is_finite() || err <= CUBIC_SUBDIV_TOL_PX {
        let ctrl = [
            0.75 * (p1[0] + p2[0]) - 0.25 * (p0[0] + p3[0]),
            0.75 * (p1[1] + p2[1]) - 0.25 * (p0[1] + p3[1]),
        ];
        out.push((ctrl, p3));
        return;
    }

    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let mid = midpoint(p012, p123);

    cubic_to_quadratics(p0, p01, p012, mid, depth + 1, out);
    cubic_to_quadratics(mid, p123, p23, p3, depth + 1, out);
}

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])]
}

fn render_edge_elements(xml: &mut String, segs: &[StyledSeg]) {
    let mut idx = 0usize;
    while idx < segs.len() {
        let fill_flag = segs[idx].has_fill;
        let stroke_flag = segs[idx].has_stroke;
        let mut edge_str = String::new();
        let mut cursor: Option<Point24_8> = None;

        while idx < segs.len()
            && segs[idx].has_fill == fill_flag
            && segs[idx].has_stroke == stroke_flag
        {
            let s = segs[idx].seg;
            let mut token = String::with_capacity(48);
            if cursor != Some(s.start) {
                if !edge_str.is_empty() {
                    token.push(' ');
                }
                write!(token, "!{} {}", fmt_24_8(s.start[0]), fmt_24_8(s.start[1])).unwrap();
            }
            match s.ctrl {
                None => {
                    write!(token, "|{} {}", fmt_24_8(s.end[0]), fmt_24_8(s.end[1])).unwrap();
                }
                Some(c) => {
                    write!(
                        token,
                        "[{} {} {} {}",
                        fmt_24_8(c[0]),
                        fmt_24_8(c[1]),
                        fmt_24_8(s.end[0]),
                        fmt_24_8(s.end[1])
                    )
                    .unwrap();
                }
            }
            if !edge_str.is_empty() && edge_str.len() + token.len() > MAX_EDGE_ATTR_BYTES {
                break;
            }
            edge_str.push_str(&token);
            cursor = Some(s.end);
            idx += 1;
        }

        if !edge_str.is_empty() {
            xml.push_str("    <Edge");
            if fill_flag {
                xml.push_str(" fillStyle1=\"1\"");
            }
            if stroke_flag {
                xml.push_str(" strokeStyle=\"1\"");
            }
            writeln!(xml, " edges=\"{edge_str}\"/>").unwrap();
        }
    }
}

fn px_to_24_8(px: f64) -> i64 {
    if !px.is_finite() {
        return 0;
    }
    // 1 pixel = 20 twips; 24.8 fixed-point twips = 20 * 256 = 5120 units per pixel.
    (px * 5120.0)
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
}

fn point_px_to_24_8(x: f64, y: f64) -> Point24_8 {
    [px_to_24_8(x), px_to_24_8(y)]
}

/// Formats a signed 24.8 fixed-point twip value (`val_24_8` within `i32::MIN..=i32::MAX`).
/// Exact integer twips emit decimal integers so `xfl/shape.rs::solid_polygon` fast-path
/// applies; fractional sub-twips emit canonical XFL `#HHHHHH.HH` 24.8 two's-complement hex.
fn fmt_24_8(val_24_8: i64) -> String {
    let clamped = val_24_8.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    if (clamped & 0xff) == 0 {
        return (clamped >> 8).to_string();
    }
    let bits = clamped as u32;
    let raw_whole = (bits >> 8) & 0xff_ffff;
    let frac = (bits & 0xff) as u8;
    if clamped < 0 {
        format!("#{raw_whole:06X}.{frac:02X}")
    } else {
        format!("#{raw_whole:X}.{frac:02X}")
    }
}

fn safe_placement_matrix(mut t: Transform) -> [f64; 6] {
    if !t.x.is_finite() {
        t.x = 0.0;
    }
    if !t.y.is_finite() {
        t.y = 0.0;
    }
    if !t.rotation.is_finite() {
        t.rotation = 0.0;
    }
    if !t.skew.is_finite() {
        t.skew = 0.0;
    }
    if !t.scale_x.is_finite() {
        t.scale_x = 1.0;
    }
    if !t.scale_y.is_finite() {
        t.scale_y = 1.0;
    }
    t.x = t.x.clamp(-1.0e9, 1.0e9);
    t.y = t.y.clamp(-1.0e9, 1.0e9);

    let clamp_scale = |s: f64| -> f64 {
        if s.abs() < 1.0e-4 {
            if s.is_sign_negative() && s != 0.0 {
                -1.0e-4
            } else {
                1.0e-4
            }
        } else {
            s.clamp(-1.0e4, 1.0e4)
        }
    };
    t.scale_x = clamp_scale(t.scale_x);
    t.scale_y = clamp_scale(t.scale_y);

    // Keep X and Y basis vectors non-collinear (`skew` away from ±90° mod 180°).
    let skew_mod = t.skew.rem_euclid(180.0);
    if (skew_mod - 90.0).abs() < 0.05 {
        t.skew += if skew_mod >= 90.0 { 0.05 } else { -0.05 };
    }

    transform_matrix(t)
}

fn append_matrix_and_pivot(out: &mut String, transform: Transform) {
    let m = safe_placement_matrix(transform);
    append_raw_matrix(out, "  ", m, false);
    if let Some([px, py]) = transform.pivot
        && px.is_finite()
        && py.is_finite()
        && (px.abs() > 1.0e-9 || py.abs() > 1.0e-9)
    {
        writeln!(
            out,
            "  <transformationPoint>\n    <Point x=\"{}\" y=\"{}\"/>\n  </transformationPoint>",
            fmt_num(px),
            fmt_num(py),
        )
        .unwrap();
    }
}

fn append_raw_matrix(out: &mut String, indent: &str, m: [f64; 6], always_emit: bool) {
    let [a, b, c, d, tx, ty] = m;
    let is_identity = (a - 1.0).abs() <= 1.0e-9
        && b.abs() <= 1.0e-9
        && c.abs() <= 1.0e-9
        && (d - 1.0).abs() <= 1.0e-9
        && tx.abs() <= 1.0e-9
        && ty.abs() <= 1.0e-9;
    if is_identity && !always_emit {
        return;
    }
    writeln!(out, "{indent}<matrix>").unwrap();
    write!(out, "{indent}  <Matrix").unwrap();
    if (a - 1.0).abs() > 1.0e-9 {
        write!(out, " a=\"{}\"", fmt_num(a)).unwrap();
    }
    if b.abs() > 1.0e-9 {
        write!(out, " b=\"{}\"", fmt_num(b)).unwrap();
    }
    if c.abs() > 1.0e-9 {
        write!(out, " c=\"{}\"", fmt_num(c)).unwrap();
    }
    if (d - 1.0).abs() > 1.0e-9 {
        write!(out, " d=\"{}\"", fmt_num(d)).unwrap();
    }
    if tx.abs() > 1.0e-9 {
        write!(out, " tx=\"{}\"", fmt_num(tx)).unwrap();
    }
    if ty.abs() > 1.0e-9 {
        write!(out, " ty=\"{}\"", fmt_num(ty)).unwrap();
    }
    out.push_str("/>\n");
    writeln!(out, "{indent}</matrix>").unwrap();
}

fn is_valid_xml_char(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

fn escape_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_xfl_characters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push_str("&#xD;");
            continue;
        }
        if ch == '\n' {
            out.push_str("&#xD;");
            continue;
        }
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn cdata_wrap(s: &str) -> String {
    let filtered: String = s.chars().filter(|&c| is_valid_xml_char(c)).collect();
    let escaped = filtered.replace("]]>", "]]]]><![CDATA[>");
    format!("<![CDATA[{escaped}]]>")
}

fn fmt_num(v: f64) -> String {
    if !v.is_finite() || v.abs() < 1.0e-10 {
        return "0".to_owned();
    }
    let rounded = v.round();
    if v.abs() < 1.0e15 && (v - rounded).abs() < 1.0e-10 {
        let int_val = rounded as i64;
        return if int_val == 0 {
            "0".to_owned()
        } else {
            int_val.to_string()
        };
    }
    let formatted = format!("{v:.9}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn fmt_hex_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn fmt_color_alpha(c: Color) -> (String, Option<String>) {
    let hex = fmt_hex_rgb([c[0], c[1], c[2]]);
    let alpha = (c[3] != 255).then(|| fmt_num(f64::from(c[3]) / 255.0));
    (hex, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use zip::ZipArchive;

    fn decode_24_8_ref(token: &str) -> i64 {
        if let Some(hex_body) = token.strip_prefix('#') {
            let (whole_s, frac_s) = hex_body.split_once('.').expect("hex fraction");
            assert!((1..=6).contains(&whole_s.len()));
            assert_eq!(frac_s.len(), 2);
            let raw_whole = u32::from_str_radix(whole_s, 16).unwrap();
            let signed_whole = if raw_whole > 0x7f_ffff {
                i64::from(raw_whole) - 0x100_0000
            } else {
                i64::from(raw_whole)
            };
            let frac = u8::from_str_radix(frac_s, 16).unwrap();
            signed_whole * 256 + i64::from(frac)
        } else {
            token.parse::<i64>().unwrap() * 256
        }
    }

    #[test]
    fn xml_and_cdata_escaping_preserves_content_and_strips_illegal_controls() {
        assert_eq!(
            escape_xml_attr("A&B <\"quote\"> 'single'\n\x00\x07"),
            "A&amp;B &lt;&quot;quote&quot;&gt; &apos;single&apos;&#xA;"
        );
        assert_eq!(
            escape_xml_text("x < y && z > 0\x01"),
            "x &lt; y &amp;&amp; z &gt; 0"
        );
        assert_eq!(
            escape_xfl_characters("Line 1\r\nLine 2\nLine 3 & <more>"),
            "Line 1&#xD;Line 2&#xD;Line 3 &amp; &lt;more&gt;"
        );
        assert_eq!(
            cdata_wrap("trace(\"a]]>b\");\x00"),
            "<![CDATA[trace(\"a]]]]><![CDATA[>b\");]]>"
        );
    }

    #[test]
    fn numeric_formatter_avoids_negative_zero_and_exponents() {
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(-1.0e-12), "0");
        assert_eq!(fmt_num(24.0), "24");
        assert_eq!(fmt_num(-100.0), "-100");
        assert_eq!(fmt_num(0.5), "0.5");
        assert_eq!(fmt_num(1.0e-5), "0.00001");
        assert!(!fmt_num(1.0e-7).contains(['e', 'E']));
        assert_eq!(fmt_num(f64::NAN), "0");
        assert_eq!(fmt_num(f64::INFINITY), "0");
    }

    #[test]
    fn twip_24_8_formatter_round_trips_signed_integers_and_subtwips() {
        assert_eq!(fmt_24_8(0), "0");
        assert_eq!(fmt_24_8(256 * 20), "20");
        assert_eq!(fmt_24_8(-256 * 35), "-35");

        for sample in [
            1_i64,
            5,
            128,
            255,
            257,
            5120 + 64,
            -1,
            -5,
            -128,
            -255,
            -257,
            -5120 - 192,
            i64::from(i32::MAX),
            i64::from(i32::MIN) + 1,
        ] {
            let encoded = fmt_24_8(sample);
            assert_eq!(
                decode_24_8_ref(&encoded),
                sample,
                "failed 24.8 round-trip for {sample} ({encoded})"
            );
        }
    }

    #[test]
    fn cubic_subdivision_converges_to_endpoints() {
        let mut quads = Vec::new();
        cubic_to_quadratics(
            [0.0, 0.0],
            [20.0, 60.0],
            [80.0, -40.0],
            [100.0, 10.0],
            0,
            &mut quads,
        );
        assert!(!quads.is_empty());
        assert!(quads.len() <= 8);
        let last_end = quads.last().unwrap().1;
        assert!((last_end[0] - 100.0).abs() < 1e-9);
        assert!((last_end[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn export_fla_bytes_builds_valid_xfl_archive_for_demo_project() {
        let project = Project::demo();
        let bytes = export_fla_bytes(&project).expect("export demo project");
        assert!(bytes.starts_with(b"PK\x03\x04"));

        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open zip");
        let mut first = archive.by_index(0).expect("first entry");
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        let mut mime = String::new();
        first.read_to_string(&mut mime).unwrap();
        assert_eq!(mime, "application/vnd.adobe.fla");
        drop(first);

        let mut dom_xml = String::new();
        archive
            .by_name("DOMDocument.xml")
            .expect("DOMDocument.xml present")
            .read_to_string(&mut dom_xml)
            .unwrap();
        assert!(dom_xml.contains("xflVersion=\"2.2\""));

        let mut pub_xml = String::new();
        archive
            .by_name("PublishSettings.xml")
            .expect("PublishSettings.xml present")
            .read_to_string(&mut pub_xml)
            .unwrap();
        assert!(pub_xml.contains("<ActionScriptVersion>2</ActionScriptVersion>"));
    }

    #[test]
    fn export_fla_preserves_js_ts_scripts_bold_italic_sound_trim_and_instance_filters() {
        use neoflash_core::{
            Accessibility, AccessibleRole, BlendMode, FrameScript, SourceUnit, TextFormat,
        };

        let mut project = Project::blank();
        project.format_version = 7;
        project.sources.push(SourceUnit {
            id: "src-js".into(),
            path: "scripts/app.js".into(),
            language: ScriptLanguage::JavaScript,
            source: "export const speed = 5;".into(),
        });
        project.sources.push(SourceUnit {
            id: "src-ts".into(),
            path: "scripts/helper.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: "export function double(n: number): number { return n * 2; }".into(),
        });
        project.sources.push(SourceUnit {
            id: "src-as1".into(),
            path: "scripts/legacy.as".into(),
            language: ScriptLanguage::ActionScript1,
            source: "tellTarget (\"/hero\") { play(); }".into(),
        });

        let wav_bytes = [
            b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ',
            16, 0, 0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a',
            b't', b'a', 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        project.resources.push(
            ResourceUnit::from_bytes("snd-1", "audio/beep.wav", "audio/wav", &wav_bytes).unwrap(),
        );

        let child_sym_id = "sym-box".to_owned();
        project.symbols.push(Symbol {
            id: child_sym_id.clone(),
            name: "BoxClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "layer-box".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "el-rect".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 20.0,
                                fill: [255, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                }],
                scripts: vec![],
                guides: vec![],
            },
        });

        let mut sound = FrameSound::new("snd-1");
        sound.in_point = Some(0.25);
        sound.out_point = Some(0.75);

        let fmt = TextFormat {
            font: "Arial".into(),
            size: 18.0,
            bold: true,
            italic: true,
            ..Default::default()
        };

        let root_sym = project.symbol_mut(&project.root.clone()).unwrap();
        root_sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::TypeScript,
                source: "const stageReady: boolean = true;".into(),
            },
        });
        root_sym.timeline.layers[0].keyframes[0].sound = Some(sound);
        root_sym.timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "el-inst".into(),
                name: "box1".into(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: child_sym_id,
                    playback: InstancePlayback::Independent,
                },
                visible: false,
                cache_as_bitmap: true,
                blend_mode: BlendMode::Multiply,
                accessibility: Some(Accessibility {
                    name: "Hero Box".into(),
                    description: "Clickable box".into(),
                    role: AccessibleRole::Button,
                    tab_order: 3,
                    disabled: true,
                }),
                filters: vec![
                    FilterSpec::Blur {
                        blur_x: 6.0,
                        blur_y: 4.0,
                        quality: 2,
                    },
                    FilterSpec::Glow {
                        color: 0x336699,
                        alpha: 0.75,
                        blur_x: 8.0,
                        blur_y: 8.0,
                        strength: 1.5,
                        quality: 2,
                        inner: true,
                        knockout: false,
                    },
                    FilterSpec::DropShadow {
                        distance: 5.0,
                        angle: 45.0,
                        color: 0x112233,
                        alpha: 0.5,
                        blur_x: 4.0,
                        blur_y: 4.0,
                        strength: 1.0,
                        quality: 1,
                        inner: false,
                        knockout: false,
                        hide_object: false,
                    },
                ],
                scripts: vec![ObjectScript {
                    event: ObjectEvent::Click,
                    script: Script {
                        language: ScriptLanguage::JavaScript,
                        source: "this.x += 10;".into(),
                    },
                }],
            },
            Element {
                id: "el-txt".into(),
                name: "label1".into(),
                transform: Transform::default(),
                kind: ElementKind::TextBox {
                    text: "Styled Header".into(),
                    width: 160.0,
                    height: 32.0,
                    format: fmt,
                    word_wrap: false,
                    multiline: false,
                },
                ..Default::default()
            },
        ];
        project.validate().unwrap();

        let temp_path = std::env::temp_dir().join(format!(
            "neoflash-xfl-export-f03-{}.fla",
            std::process::id()
        ));
        export_fla(&project, &temp_path).expect("export fla");

        let mut dom_xml = String::new();
        ZipArchive::new(fs::File::open(&temp_path).unwrap())
            .unwrap()
            .by_name("DOMDocument.xml")
            .unwrap()
            .read_to_string(&mut dom_xml)
            .unwrap();
        assert!(dom_xml.contains("inPoint44=\"11025\""));
        assert!(dom_xml.contains("outPoint44=\"33075\""));
        assert!(dom_xml.contains("isVisible=\"false\""));
        assert!(dom_xml.contains("cacheAsBitmap=\"true\""));
        assert!(dom_xml.contains("blendMode=\"multiply\""));
        assert!(dom_xml.contains("hasAccessibleData=\"true\""));
        assert!(dom_xml.contains("accName=\"Hero Box\""));
        assert!(dom_xml.contains("<BlurFilter blurX=\"6\" blurY=\"4\" quality=\"2\"/>"));
        assert!(dom_xml.contains("bold=\"true\""));
        assert!(dom_xml.contains("italic=\"true\""));

        let imported = crate::import_project(&temp_path).expect("reimport exported fla");
        let _ = fs::remove_file(&temp_path);
        imported.validate().unwrap();

        assert!(
            imported
                .sources
                .iter()
                .any(|s| s.language == ScriptLanguage::JavaScript
                    && s.source.contains("speed = 5"))
        );
        assert!(
            imported
                .sources
                .iter()
                .any(|s| s.language == ScriptLanguage::TypeScript
                    && s.source.contains("double(n: number)"))
        );
        assert!(
            imported
                .sources
                .iter()
                .any(|s| s.language == ScriptLanguage::ActionScript1
                    && s.source == "tellTarget (\"/hero\") { play(); }")
        );
        let imp_root = imported.symbol(&imported.root).unwrap();
        assert_eq!(imp_root.timeline.scripts.len(), 1);
        assert_eq!(
            imp_root.timeline.scripts[0].script.language,
            ScriptLanguage::TypeScript
        );
        assert_eq!(
            imp_root.timeline.scripts[0].script.source,
            "const stageReady: boolean = true;"
        );

        let snd_kf = imp_root
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find_map(|k| k.sound.as_ref())
            .expect("frame sound preserved");
        assert_eq!(snd_kf.in_point, Some(0.25));
        assert_eq!(snd_kf.out_point, Some(0.75));

        let imp_inst = imp_root
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|e| e.name == "box1")
            .expect("instance preserved");
        assert!(!imp_inst.visible);
        assert!(imp_inst.cache_as_bitmap);
        assert_eq!(imp_inst.blend_mode, BlendMode::Multiply);
        let acc = imp_inst
            .accessibility
            .as_ref()
            .expect("accessibility preserved");
        assert_eq!(acc.name, "Hero Box");
        assert_eq!(acc.description, "Clickable box");
        assert_eq!(acc.tab_order, 3);
        assert!(acc.disabled);
        assert_eq!(imp_inst.filters.len(), 3);
        assert_eq!(
            imp_inst.filters[0],
            FilterSpec::Blur {
                blur_x: 6.0,
                blur_y: 4.0,
                quality: 2,
            }
        );
        assert_eq!(imp_inst.scripts.len(), 1);
        assert_eq!(imp_inst.scripts[0].event, ObjectEvent::Click);
        assert_eq!(
            imp_inst.scripts[0].script.language,
            ScriptLanguage::JavaScript
        );
        assert_eq!(imp_inst.scripts[0].script.source, "this.x += 10;");

        let imp_txt = imp_root
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|e| e.name == "label1")
            .expect("textbox preserved");
        let ElementKind::TextBox { format: tf, .. } = &imp_txt.kind else {
            panic!("expected TextBox");
        };
        assert!(tf.bold);
        assert!(tf.italic);
    }
}
