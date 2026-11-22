//! Pure-Rust Adobe XFL 2.2 ZIP-FLA (`.fla`) archive exporter.
//!
//! Serializes a [`neoflash_core::Project`] into a standard ZIP-compressed XFL 2.2
//! `.fla` container (`mimetype`, `<stem>.xfl`, `PublishSettings.xml`,
//! `DOMDocument.xml`, `LIBRARY/*.xml`, `LIBRARY/media/*`, and `.as` sources)
//! compatible with Adobe Flash CS5.5/CS6/Animate and round-trip importable by
//! `crate::xfl` whenever the project stays within NeoFlash's XFL importer subset.

use crate::{
    ImportError, source,
    xfl::media::{FlashSoundCache, build_flash_sound_cache, inspect_audio_bytes},
};
use neoflash_core::{
    AuthoredBitmapFill, AuthoredTextFieldOptions, Color, ColorTransform, Easing, Element,
    ElementKind, FilterSpec, FrameLabelType, FrameSound, GradientKind, GradientPaint, GradientStop,
    Id, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, ObjectEvent, ObjectScript,
    PathCommand, Project, ResourceUnit, Script, ScriptLanguage, SoundEffect, SoundSync, Symbol,
    SymbolKind, TextAlign, TextFieldType, TextFormat, TextFormatRun, Timeline, Transform,
    TweenKind, sample_layer_in_at_fps, transform_matrix, valid_library_folder,
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
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

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

    let as_sources = collect_actionscript_sources(project, &ctx.doc_stem)?;

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
            if let (Some(dat_href), Some(cache)) = (&snd.cache_href, &snd.cache) {
                writer.add_file(
                    &format!("bin/{dat_href}"),
                    &cache.cache_bytes,
                    CompressionMethod::Deflated,
                )?;
            }
        }
    }
    for (dat_href, vid_bytes) in &ctx.video_blobs {
        writer.add_file(
            &format!("bin/{dat_href}"),
            vid_bytes,
            CompressionMethod::Deflated,
        )?;
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
    cache_href: Option<String>,
    cache: Option<FlashSoundCache>,
    linkage_id: Option<String>,
}

type VideoLookupKey = (Option<Id>, (u64, u64));

struct VideoExport {
    item_name: String,
    width: f64,
    height: f64,
    video_data_href: Option<String>,
}

struct ExportContext {
    doc_stem: String,
    as_version: u8,
    fps: f64,
    include_hidden_layers: bool,
    folders: Vec<String>,
    symbol_order: Vec<Id>,
    symbols: HashMap<Id, SymbolExport>,
    bitmap_order: Vec<Id>,
    bitmaps: HashMap<Id, BitmapExport>,
    sound_order: Vec<Id>,
    sounds: HashMap<Id, SoundExport>,
    embedded_video_ids: HashSet<Id>,
    video_blobs: Vec<(String, Vec<u8>)>,
    video_order: Vec<VideoLookupKey>,
    videos: HashMap<VideoLookupKey, VideoExport>,
}

impl ExportContext {
    fn build(project: &Project) -> Result<Self, ImportError> {
        let doc_stem = sanitize_doc_stem(&project.name);
        let as_version = detect_as_version(project);
        let include_hidden_layers = !can_exclude_hidden_layers(project);

        let folders: Vec<String> = project
            .library_folder_paths()
            .into_iter()
            .filter(|f| !f.is_empty() && valid_library_folder(f))
            .collect();

        let (symbol_order, symbols) = collect_symbols(project);
        let (bitmap_order, bitmaps, sound_order, sounds, embedded_video_order, embedded_videos) =
            collect_media(project)?;
        let embedded_video_ids: HashSet<Id> = embedded_videos.keys().cloned().collect();

        let mut video_order = Vec::new();
        let mut videos = HashMap::new();
        let mut used_video_names = HashSet::new();
        let mut placed_video_resources = HashSet::new();
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                let mut active_video_res: Option<Id> = None;
                for key in &layer.keyframes {
                    if let Some(span) = &key.video {
                        active_video_res = embedded_video_ids
                            .contains(&span.resource)
                            .then(|| span.resource.clone());
                    } else if !key
                        .elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Video { .. }))
                    {
                        active_video_res = None;
                    }
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
                        if let ElementKind::Video { width, height } = &elem.kind {
                            if !width.is_finite()
                                || !height.is_finite()
                                || *width <= 0.0
                                || *height <= 0.0
                            {
                                return Err(ImportError::Container(format!(
                                    "video element {:?} in symbol {:?} has invalid dimensions {}x{}",
                                    elem.name, sym.name, width, height
                                )));
                            }
                            let eff_res = key
                                .video
                                .as_ref()
                                .filter(|v| embedded_video_ids.contains(&v.resource))
                                .map(|v| v.resource.clone())
                                .or_else(|| active_video_res.clone());
                            if let Some(rid) = &eff_res {
                                placed_video_resources.insert(rid.clone());
                            }
                            let key_tuple = (eff_res.clone(), video_size_key(*width, *height));
                            videos.entry(key_tuple.clone()).or_insert_with(|| {
                                let clamped_w = width.clamp(1.0, 8192.0);
                                let clamped_h = height.clamp(1.0, 8192.0);
                                let (desired, dat_href) = if let Some(rid) = &eff_res
                                    && let Some((stem, dat)) = embedded_videos.get(rid)
                                {
                                    (stem.clone(), Some(dat.clone()))
                                } else {
                                    (
                                        format!(
                                            "Video_{}x{}",
                                            clamped_w.round() as u32,
                                            clamped_h.round() as u32
                                        ),
                                        None,
                                    )
                                };
                                let item_name = dedup_exact_name(&mut used_video_names, &desired);
                                video_order.push(key_tuple);
                                VideoExport {
                                    item_name,
                                    width: clamped_w,
                                    height: clamped_h,
                                    video_data_href: dat_href,
                                }
                            });
                        }
                    }
                }
            }
        }
        let mut video_blobs = Vec::with_capacity(embedded_video_order.len());
        for (rid, bytes) in embedded_video_order {
            if let Some((stem, dat_href)) = embedded_videos.get(&rid) {
                video_blobs.push((dat_href.clone(), bytes));
                if !placed_video_resources.contains(&rid) {
                    let key_tuple = (Some(rid), video_size_key(320.0, 240.0));
                    videos.entry(key_tuple.clone()).or_insert_with(|| {
                        let item_name = dedup_exact_name(&mut used_video_names, stem);
                        video_order.push(key_tuple);
                        VideoExport {
                            item_name,
                            width: 320.0,
                            height: 240.0,
                            video_data_href: Some(dat_href.clone()),
                        }
                    });
                }
            }
        }

        Ok(Self {
            doc_stem,
            as_version,
            fps: f64::from(project.fps.max(1)),
            include_hidden_layers,
            folders,
            symbol_order,
            symbols,
            bitmap_order,
            bitmaps,
            sound_order,
            sounds,
            embedded_video_ids,
            video_blobs,
            video_order,
            videos,
        })
    }
}

fn video_size_key(width: f64, height: f64) -> (u64, u64) {
    let w_twips = (width.clamp(1.0, 8192.0) * 20.0).round() as u64;
    let h_twips = (height.clamp(1.0, 8192.0) * 20.0).round() as u64;
    (w_twips, h_twips)
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
        ScriptLanguage::ActionScript2 => {
            format!("/* neoflash:language=actionscript2 */\n{}", script.source)
        }
        ScriptLanguage::ActionScript3 => {
            format!("/* neoflash:language=actionscript3 */\n{}", script.source)
        }
    }
}

fn detect_as_version(project: &Project) -> u8 {
    if project.script_language == Some(ScriptLanguage::ActionScript3) {
        return 3;
    }
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
        || project.symbols.iter().any(|s| {
            s.class_name
                .as_deref()
                .is_some_and(|c| !c.trim().is_empty())
        })
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
                saw_hidden |= !layer.published_visible();
                continue;
            }
            let layer_end = layer_effective_end(layer, timeline_end);
            if layer.published_visible() {
                published_end = published_end.max(layer_end);
            } else {
                saw_hidden = true;
                excluded_end = excluded_end.max(layer_end);
                if layer.keyframes.iter().any(|k| !k.label.is_empty())
                    || timeline
                        .scripts
                        .iter()
                        .any(|script| script.layer.as_ref() == Some(&layer.id))
                {
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
    Vec<(Id, Vec<u8>)>,
    HashMap<Id, (String, String)>,
);

fn is_auto_generated_sound_id(id: &str) -> bool {
    for prefix in [
        "legacy-sound-",
        "xfl-sound-",
        "swf-sound-",
        "legacy-cc-",
        "fla-",
        "res-",
        "asset-",
        "audio-",
        "baked-",
        "video-audio-",
    ] {
        if id.starts_with(prefix) {
            return true;
        }
    }
    for prefix in ["snd-", "sound-", "s"] {
        if let Some(rest) = id.strip_prefix(prefix)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn explicit_sound_linkage_id(res: &ResourceUnit) -> Option<String> {
    let trimmed = res.id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 512
        || trimmed.chars().any(char::is_control)
        || is_auto_generated_sound_id(trimmed)
    {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn is_valid_as3_linkage_class(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().enumerate().all(|(i, ch)| {
                    ch == '_' || ch == '$' || ch.is_alphabetic() || (i > 0 && ch.is_numeric())
                })
        })
}

fn collect_media(project: &Project) -> Result<MediaCollections, ImportError> {
    let mut smoothed_bitmaps: HashSet<&str> = HashSet::new();
    let mut referenced_bitmaps: HashSet<&str> = HashSet::new();
    let mut referenced_sounds: HashSet<&str> = HashSet::new();
    let mut referenced_videos: HashSet<&str> = HashSet::new();
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
                if let Some(vid) = &key.video {
                    referenced_videos.insert(vid.resource.as_str());
                }
                for bf in &key.bitmap_fills {
                    referenced_bitmaps.insert(bf.resource_id.as_str());
                    if bf.smoothing {
                        smoothed_bitmaps.insert(bf.resource_id.as_str());
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
    let mut embedded_video_order = Vec::new();
    let mut embedded_videos = HashMap::new();

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
        let is_vid_type = res.is_video();
        let referenced_as_bitmap = referenced_bitmaps.contains(res.id.as_str());
        let referenced_as_sound = referenced_sounds.contains(res.id.as_str());
        let referenced_as_video = referenced_videos.contains(res.id.as_str());
        let has_authored_role = referenced_as_bitmap || referenced_as_sound || referenced_as_video;
        // Authored use is authoritative: a resource that decodes as a bitmap remains
        // exportable even when its imported MIME metadata was wrong. MIME classifies
        // only otherwise-unreferenced library media, where no authored role can do so.
        let want_img = referenced_as_bitmap || (!has_authored_role && is_img_type);
        let want_aud = referenced_as_sound || (!has_authored_role && is_aud_type);
        let want_vid = referenced_as_video || (!has_authored_role && is_vid_type);
        if !want_img && !want_aud && !want_vid {
            return Err(ImportError::Container(format!(
                "resource {:?} ({}) cannot be represented in XFL 2.2 .fla media library",
                res.path, res.media_type
            )));
        }
        let raw = res.decoded_bytes().map_err(|err| {
            ImportError::Container(format!("invalid resource {:?}: {err}", res.path))
        })?;

        let bitmap = want_img.then(|| normalize_bitmap_bytes(&raw)).flatten();
        let audio = want_aud.then(|| inspect_audio_bytes(&raw)).flatten();
        let valid_video = want_vid
            && !neoflash_core::inspect_video_resource(&res.path, &res.media_type, &raw, None)
                .iter()
                .any(|d| d.is_error);

        if want_img && bitmap.is_none() {
            return Err(ImportError::Container(format!(
                "resource {:?} ({}) cannot be represented as an XFL bitmap; use decodable PNG or JPEG image data",
                res.path, res.media_type
            )));
        }
        if want_aud && audio.is_none() {
            return Err(ImportError::Container(format!(
                "resource {:?} ({}) cannot be represented as XFL audio; use PCM WAV or MP3 audio data",
                res.path, res.media_type
            )));
        }
        if want_vid && !valid_video {
            return Err(ImportError::Container(format!(
                "resource {:?} ({}) cannot be represented as XFL video; stream header is invalid or unsupported",
                res.path, res.media_type
            )));
        }

        if let Some((bytes, ext)) = bitmap {
            if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(ImportError::Container(format!(
                    "bitmap resource {:?} exceeds 16 MiB XFL entry limit",
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
        }

        if let Some(audio) = audio {
            if raw.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(ImportError::Container(format!(
                    "audio resource {:?} exceeds 16 MiB XFL entry limit",
                    res.path
                )));
            }
            let stem = clean_resource_stem(&res.path, "sound");
            let item_name = dedup_exact_name(
                &mut used_sound_names,
                &format!("{stem}.{}", audio.extension),
            );
            let file_name = dedup_case_insensitive(
                &mut used_media_files_lower,
                &stem,
                &format!(".{}", audio.extension),
            );
            let emit_mp3_sample_count = audio.is_mp3 && custom_env_sounds.contains(res.id.as_str());
            let cache = build_flash_sound_cache(&raw);
            let cache_href = cache
                .as_ref()
                .map(|_| format!("sound{}.dat", sound_order.len() + 1));
            let linkage_id = explicit_sound_linkage_id(res);
            sound_order.push(res.id.clone());
            sounds.insert(
                res.id.clone(),
                SoundExport {
                    item_name,
                    href: format!("media/{file_name}"),
                    zip_path: format!("LIBRARY/media/{file_name}"),
                    bytes: raw.clone(),
                    duration_44: audio.duration_44,
                    emit_mp3_sample_count,
                    cache_href,
                    cache,
                    linkage_id,
                },
            );
        }

        if want_vid && valid_video {
            if raw.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(ImportError::Container(format!(
                    "video resource {:?} exceeds 16 MiB XFL entry limit",
                    res.path
                )));
            }
            let stem = clean_resource_stem(&res.path, "video");
            let dat_href = format!("video{}.dat", embedded_video_order.len() + 1);
            embedded_videos.insert(res.id.clone(), (stem, dat_href));
            embedded_video_order.push((res.id.clone(), raw));
        }
    }

    Ok((
        bitmap_order,
        bitmaps,
        sound_order,
        sounds,
        embedded_video_order,
        embedded_videos,
    ))
}

fn is_internal_provenance_resource(res: &ResourceUnit) -> bool {
    res.id == "fla-provenance"
        || matches!(
            res.path.as_str(),
            "legacy/provenance.json" | "legacy/import-provenance.json"
        )
        || res.path.starts_with("legacy/source.fla")
        || res.path.starts_with("legacy/source.swf")
        || res
            .media_type
            .eq_ignore_ascii_case("application/x-neoflash-fla-source")
        || res
            .media_type
            .eq_ignore_ascii_case("application/x-neoflash-swf-source")
        || res.media_type.eq_ignore_ascii_case("application/json")
        || res.media_type.eq_ignore_ascii_case("text/x-actionscript")
        || res.media_type.eq_ignore_ascii_case("text/plain")
        || res.path.to_ascii_lowercase().ends_with(".as")
        || res.path.to_ascii_lowercase().ends_with(".asi")
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

fn collect_actionscript_sources(
    project: &Project,
    doc_stem: &str,
) -> Result<Vec<(String, String)>, ImportError> {
    let mut out = Vec::new();
    let mut emitted_bytes: HashMap<String, Vec<u8>> = HashMap::new();
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
            emitted_bytes.insert(rel_path.to_ascii_lowercase(), body.as_bytes().to_vec());
            out.push((rel_path, body));
        }
    }

    for res in &project.resources {
        let is_as_res = res.media_type.eq_ignore_ascii_case("text/x-actionscript")
            || res.path.to_ascii_lowercase().ends_with(".as");
        if !is_as_res {
            continue;
        }
        let raw = res.decoded_bytes().map_err(|err| {
            ImportError::Container(format!(
                "invalid ActionScript resource {:?}: {err}",
                res.path
            ))
        })?;
        let text = String::from_utf8(raw).map_err(|_| {
            ImportError::Container(format!(
                "ActionScript resource {:?} is not valid UTF-8",
                res.path
            ))
        })?;
        let cleaned = sanitize_as_source_path(&res.path);
        let (dir_prefix, file_stem) = match cleaned.rsplit_once('/') {
            Some((dir, base)) => (format!("{dir}/"), base),
            None => (String::new(), cleaned.as_str()),
        };
        let rel_path = format!("{dir_prefix}{file_stem}.as");
        let lower = rel_path.to_ascii_lowercase();
        if let Some(existing) = emitted_bytes.get(&lower) {
            if existing.as_slice() == text.as_bytes() {
                continue;
            }
            return Err(ImportError::Container(format!(
                "ActionScript resource {:?} collides with archive path {:?}",
                res.path, rel_path
            )));
        }
        source::safe_name(&rel_path)?;
        if !used_lower.insert(lower.clone()) {
            return Err(ImportError::Container(format!(
                "ActionScript resource {:?} collides with archive path {:?}",
                res.path, rel_path
            )));
        }
        emitted_bytes.insert(lower, text.as_bytes().to_vec());
        out.push((rel_path, text));
    }

    if out.len() > 1024 {
        return Err(ImportError::Container(format!(
            "bundled ActionScript source count ({}) exceeds 1024 XFL import limit",
            out.len()
        )));
    }

    Ok(out)
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
        "<DOMDocument xmlns=\"{XFL_NS}\" width=\"{}\" height=\"{}\" frameRate=\"{}\" backgroundColor=\"{bg}\" xflVersion=\"2.2\" creatorInfo=\"NeoFlash\" versionInfo=\"NeoFlash XFL export 1\">",
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

    if !ctx.bitmap_order.is_empty() || !ctx.sound_order.is_empty() || !ctx.video_order.is_empty() {
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
                let linkage_attrs = match &snd.linkage_id {
                    Some(lid) if ctx.as_version == 3 && is_valid_as3_linkage_class(lid) => format!(
                        " linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\" linkageClassName=\"{}\"",
                        escape_xml_attr(lid)
                    ),
                    Some(lid) => format!(
                        " linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\" linkageIdentifier=\"{}\"",
                        escape_xml_attr(lid)
                    ),
                    None => String::new(),
                };
                if let (Some(dat_href), Some(cache)) = (&snd.cache_href, &snd.cache) {
                    let compressed_attrs = cache
                        .data_length
                        .map(|dl| format!(" dataLength=\"{dl}\" exportNative=\"true\""))
                        .unwrap_or_default();
                    writeln!(
                        xml,
                        "    <DOMSoundItem name=\"{}\" href=\"{}\" soundDataHRef=\"{}\" format=\"{}\" sampleCount=\"{}\"{compressed_attrs}{linkage_attrs}/>",
                        escape_xml_attr(&snd.item_name),
                        escape_xml_attr(&snd.href),
                        escape_xml_attr(dat_href),
                        escape_xml_attr(&cache.format_label),
                        cache.sample_count,
                    )
                    .unwrap();
                } else if snd.emit_mp3_sample_count {
                    writeln!(
                        xml,
                        "    <DOMSoundItem name=\"{}\" href=\"{}\" format=\"44kHz 16bit Stereo\" sampleCount=\"44100\"{linkage_attrs}/>",
                        escape_xml_attr(&snd.item_name),
                        escape_xml_attr(&snd.href),
                    )
                    .unwrap();
                } else {
                    writeln!(
                        xml,
                        "    <DOMSoundItem name=\"{}\" href=\"{}\"{linkage_attrs}/>",
                        escape_xml_attr(&snd.item_name),
                        escape_xml_attr(&snd.href),
                    )
                    .unwrap();
                }
            }
        }
        for vid_key in &ctx.video_order {
            if let Some(vid) = ctx.videos.get(vid_key) {
                if let Some(dat_href) = &vid.video_data_href {
                    writeln!(
                        xml,
                        "    <DOMVideoItem name=\"{}\" videoDataHRef=\"{}\" width=\"{}\" height=\"{}\"/>",
                        escape_xml_attr(&vid.item_name),
                        escape_xml_attr(dat_href),
                        fmt_num(vid.width),
                        fmt_num(vid.height),
                    )
                    .unwrap();
                } else {
                    writeln!(
                        xml,
                        "    <DOMVideoItem name=\"{}\" width=\"{}\" height=\"{}\"/>",
                        escape_xml_attr(&vid.item_name),
                        fmt_num(vid.width),
                        fmt_num(vid.height),
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

mod elements_text;
mod shapes_xml;
#[cfg(test)]
mod tests;
mod timeline_frames;

use elements_text::*;
use shapes_xml::*;
use timeline_frames::*;
