//! Deterministic, bounded, content-addressed sidecar bundle archival (`OversizedFlaBundle`)
//! for oversized (`>64 MiB` resource / `>128 MiB` JSON) binary Flash 3–CS4 `.fla` documents.
//!
//! Keeps inline `Project` validation and resource caps (`128 MiB` JSON, `16 MiB` per resource,
//! `64 MiB` total inline resources, `4096 px` per axis) unchanged while storing:
//! 1. Unscaled, un-reencoded full-resolution original bitmaps, sounds, and videos that were
//!    downscaled, JPEG-reencoded, or shed to fit inline `Project` limits; and
//! 2. Schema 3 `NFD1` delta chunks for `>64 MiB` `.fla` containers whose inline `source_archive`
//!    fell back to `"external"`, achieving 100% byte-exact SHA-256 `.fla` reconstruction.

use super::{
    BinaryRawStream, BinaryStreamRole, BinaryTimelineDocument, MAX_CONTAINER, container,
    contents::ContentsDescriptorClass,
    decode_nfd1_delta_gzip, encode_blobs_delta_gzip, fnv1a64_bytes,
    media::{
        BinaryVideoPayload, decode_binary_bitmap, decode_binary_sound_with_limit,
        decode_binary_video,
    },
    reconstruct_legacy_fla_bytes,
    timeline::TimelinePayload,
};
use crate::{ImportError, ImportFormat, ImportReport, Severity, Statistics};
use neoflash_core::{MAX_PROJECT_JSON_BYTES, Project, extensions::sha256_hex};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Maximum decoded byte length of a single chunk in an [`OversizedFlaBundle`] (`256 MiB`).
pub const MAX_OVERSIZED_BUNDLE_CHUNK_BYTES: usize = 256 * 1024 * 1024;
/// Maximum combined byte length across all sidecar chunks in an [`OversizedFlaBundle`] (`1 GiB`).
pub const MAX_OVERSIZED_BUNDLE_TOTAL_BYTES: usize = 1024 * 1024 * 1024;
/// Maximum reconstructed `.fla` container byte length supported by [`OversizedFlaBundle`] (`512 MiB`).
pub const MAX_OVERSIZED_BUNDLE_SOURCE_CONTAINER_BYTES: u64 = MAX_CONTAINER;
/// Deterministic slice size used when splitting oversized Schema 3 `NFD1` delta streams (`64 MiB`).
const BUNDLE_DELTA_PART_BYTES: usize = 64 * 1024 * 1024;
const BUNDLE_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_REPORT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OversizedChunkRole {
    /// Unscaled / lossless original media for an inline resource that was downscaled or re-encoded.
    FullResolutionMedia,
    /// Full original media for an asset omitted from inline `Project::resources` due to inline caps.
    OmittedMedia,
    /// Schema 3 `NFD1` gzipped delta chunk for reconstructing an oversized `.fla` container.
    SourceDeltaGzip,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OversizedMediaChunk {
    pub sha256: String,
    pub byte_len: usize,
    pub media_type: String,
    pub role: OversizedChunkRole,
    #[serde(skip)]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OversizedResourceEntry {
    pub resource_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<u16>,
    pub media_type: String,
    pub chunk_sha256: String,
    pub byte_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_height: Option<u32>,
    pub inline_downscaled: bool,
    pub inline_reencoded: bool,
    pub inline_omitted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OversizedDeltaReference {
    InlineResource { id: String },
    BundleChunk { sha256: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum OversizedSourceArchive {
    InlineProject {
        source_bytes: u64,
        source_fnv1a64: String,
        source_sha256: String,
    },
    BundleDeltaGzip {
        source_bytes: u64,
        source_fnv1a64: String,
        source_sha256: String,
        delta_chunk_sha256s: Vec<String>,
        references: Vec<OversizedDeltaReference>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OversizedBundleManifest {
    schema_version: u32,
    chunks: Vec<OversizedChunkManifestEntry>,
    full_resolution_resources: BTreeMap<String, OversizedResourceEntry>,
    source_archive: OversizedSourceArchive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OversizedChunkManifestEntry {
    sha256: String,
    byte_len: usize,
    media_type: String,
    role: OversizedChunkRole,
}

#[derive(Clone, Debug)]
pub struct OversizedFlaBundle {
    pub project: Project,
    pub report: ImportReport,
    pub chunks: Vec<OversizedMediaChunk>,
    pub full_resolution_resources: BTreeMap<String, OversizedResourceEntry>,
    pub source_archive: OversizedSourceArchive,
    inline_media_cache: BTreeMap<String, Vec<u8>>,
}

fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn build_inline_media_cache(project: &Project) -> BTreeMap<String, Vec<u8>> {
    let mut cache = BTreeMap::new();
    for res in &project.resources {
        if res.id == "fla-provenance"
            || res.path == "legacy/provenance.json"
            || res.path.starts_with("legacy/source.fla")
        {
            continue;
        }
        if let Ok(bytes) = res.decoded_bytes() {
            cache.insert(res.id.clone(), bytes);
        }
    }
    cache
}

struct ChunkBuilder {
    chunks: Vec<OversizedMediaChunk>,
    by_sha: BTreeMap<String, usize>,
    total_bytes: usize,
}

impl ChunkBuilder {
    fn new() -> Self {
        Self {
            chunks: Vec::new(),
            by_sha: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    fn insert(
        &mut self,
        data: Vec<u8>,
        media_type: impl Into<String>,
        role: OversizedChunkRole,
    ) -> Result<String, ImportError> {
        if data.len() > MAX_OVERSIZED_BUNDLE_CHUNK_BYTES {
            return Err(container(format!(
                "oversized bundle chunk ({} bytes) exceeds 256 MiB per-chunk limit",
                data.len()
            )));
        }
        let sha = sha256_hex(&data);
        if let Some(&existing_idx) = self.by_sha.get(&sha) {
            if self.chunks[existing_idx].data != data {
                return Err(container(
                    "SHA-256 collision in oversized bundle chunk store",
                ));
            }
            return Ok(sha);
        }
        let next_total = self
            .total_bytes
            .checked_add(data.len())
            .filter(|&n| n <= MAX_OVERSIZED_BUNDLE_TOTAL_BYTES)
            .ok_or_else(|| {
                container("oversized FLA bundle exceeds 1 GiB total sidecar chunk budget")
            })?;
        let idx = self.chunks.len();
        self.total_bytes = next_total;
        self.by_sha.insert(sha.clone(), idx);
        self.chunks.push(OversizedMediaChunk {
            sha256: sha.clone(),
            byte_len: data.len(),
            media_type: media_type.into(),
            role,
            data,
        });
        Ok(sha)
    }

    fn remaining_budget(&self) -> usize {
        MAX_OVERSIZED_BUNDLE_TOTAL_BYTES.saturating_sub(self.total_bytes)
    }
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

fn collect_oversized_media(
    document: &BinaryTimelineDocument,
    project: &Project,
    chunks: &mut ChunkBuilder,
) -> Result<BTreeMap<String, OversizedResourceEntry>, ImportError> {
    let mut streams: BTreeMap<usize, &BinaryRawStream> = BTreeMap::new();
    for raw in &document.raw_streams {
        streams.insert(raw.index, raw);
    }
    let mut inline_by_id: BTreeMap<&str, &neoflash_core::ResourceUnit> = BTreeMap::new();
    for res in &project.resources {
        inline_by_id.insert(res.id.as_str(), res);
    }

    let mut entries = BTreeMap::new();

    for descriptor in &document.contents.descriptors {
        let Some(id) = descriptor.media_id else {
            continue;
        };
        let Some(stream) = descriptor.stream_index.and_then(|idx| streams.get(&idx)) else {
            continue;
        };
        if stream.role != BinaryStreamRole::Media
            || !stream
                .name
                .strip_prefix('/')
                .unwrap_or(&stream.name)
                .eq_ignore_ascii_case(&descriptor.stream_name)
        {
            continue;
        }

        match descriptor.class {
            ContentsDescriptorClass::Bitmap => {
                let Some(settings) = descriptor.bitmap.as_ref() else {
                    continue;
                };
                let res_id = format!("legacy-bitmap-{id}");
                let is_raw_raster = settings.encoded_byte_count <= 0
                    && !stream.bytes.starts_with(&[0xff, 0xd8, 0xff, 0xe0])
                    && stream.bytes.len() >= 8
                    && stream.bytes[0] <= 3;
                if is_raw_raster
                    && let Some(inline_res) = inline_by_id.get(res_id.as_str())
                    && inline_res.media_type == "image/png"
                {
                    let rw = u16::from_le_bytes([stream.bytes[4], stream.bytes[5]]) as u32;
                    let rh = u16::from_le_bytes([stream.bytes[6], stream.bytes[7]]) as u32;
                    let inline_dims = inline_res
                        .decoded_bytes()
                        .ok()
                        .and_then(|b| image_dimensions(&b));
                    if inline_dims == Some((rw, rh)) {
                        continue;
                    }
                }
                let Ok(decoded) = decode_binary_bitmap(&stream.bytes, settings.encoded_byte_count)
                else {
                    continue;
                };
                if decoded.original_bytes != stream.bytes {
                    continue;
                }
                let (orig_w, orig_h) = (decoded.width, decoded.height);
                let source_jpeg = decoded.source_jpeg().map(|b| b.into_owned());

                let (need_sidecar, inline_downscaled, inline_reencoded, inline_omitted) =
                    if let Some(inline_res) = inline_by_id.get(res_id.as_str()) {
                        let inline_dims = inline_res
                            .decoded_bytes()
                            .ok()
                            .and_then(|b| image_dimensions(&b));
                        let downscaled = inline_dims != Some((orig_w, orig_h));
                        let reencoded =
                            source_jpeg.is_none() && inline_res.media_type == "image/jpeg";
                        (downscaled || reencoded, downscaled, reencoded, false)
                    } else {
                        (true, false, false, true)
                    };

                if need_sidecar {
                    let (full_bytes, media_type) = if let Some(jpeg) = source_jpeg {
                        (jpeg, "image/jpeg")
                    } else {
                        let png = decoded
                            .png_bytes_fast()
                            .map_err(|e| container(format!("bitmap PNG encode failed: {e}")))?;
                        (png, "image/png")
                    };
                    let byte_len = full_bytes.len();
                    let role = if inline_omitted {
                        OversizedChunkRole::OmittedMedia
                    } else {
                        OversizedChunkRole::FullResolutionMedia
                    };
                    let sha = chunks.insert(full_bytes, media_type, role)?;
                    entries.insert(
                        res_id.clone(),
                        OversizedResourceEntry {
                            resource_id: res_id,
                            media_id: Some(id),
                            media_type: media_type.to_owned(),
                            chunk_sha256: sha,
                            byte_len,
                            original_width: Some(orig_w),
                            original_height: Some(orig_h),
                            inline_downscaled,
                            inline_reencoded,
                            inline_omitted,
                        },
                    );
                }
            }
            ContentsDescriptorClass::Sound => {
                let sound_prefix = format!("legacy/sounds/{id}/");
                if project
                    .resources
                    .iter()
                    .any(|r| r.path.starts_with(&sound_prefix))
                {
                    continue;
                }
                let Some(settings) = descriptor.sound.as_ref() else {
                    continue;
                };
                let Ok(decoded) = decode_binary_sound_with_limit(
                    &stream.bytes,
                    descriptor.subtype_revision.unwrap_or(0),
                    settings,
                    MAX_OVERSIZED_BUNDLE_CHUNK_BYTES,
                ) else {
                    continue;
                };
                let res_id = format!("legacy-sound-{id}");
                let byte_len = decoded.bytes.len();
                let media_type = decoded.media_type.to_owned();
                let sha = chunks.insert(
                    decoded.bytes,
                    media_type.clone(),
                    OversizedChunkRole::OmittedMedia,
                )?;
                entries.insert(
                    res_id.clone(),
                    OversizedResourceEntry {
                        resource_id: res_id,
                        media_id: Some(id),
                        media_type,
                        chunk_sha256: sha,
                        byte_len,
                        original_width: None,
                        original_height: None,
                        inline_downscaled: false,
                        inline_reencoded: false,
                        inline_omitted: true,
                    },
                );
            }
            ContentsDescriptorClass::Video => {
                let video_prefix = format!("legacy/videos/{id}/");
                if project
                    .resources
                    .iter()
                    .any(|r| r.path.starts_with(&video_prefix))
                {
                    continue;
                }
                let Ok(decoded) = decode_binary_video(&stream.bytes) else {
                    continue;
                };
                let BinaryVideoPayload::Container {
                    media_type, bytes, ..
                } = decoded.payload
                else {
                    continue;
                };
                let res_id = format!("legacy-video-{id}");
                let byte_len = bytes.len();
                let media_type_str = media_type.to_owned();
                let sha = chunks.insert(
                    bytes,
                    media_type_str.clone(),
                    OversizedChunkRole::OmittedMedia,
                )?;
                entries.insert(
                    res_id.clone(),
                    OversizedResourceEntry {
                        resource_id: res_id,
                        media_id: Some(id),
                        media_type: media_type_str,
                        chunk_sha256: sha,
                        byte_len,
                        original_width: None,
                        original_height: None,
                        inline_downscaled: false,
                        inline_reencoded: false,
                        inline_omitted: true,
                    },
                );
            }
            _ => {}
        }
    }

    // Also capture CPicBitmap rev 0 inline rasters that exceeded 4096 px or inline resource limits.
    for tl_stream in &document.streams {
        for obj in &tl_stream.objects {
            let Some(TimelinePayload::Bitmap(b)) = &obj.payload else {
                continue;
            };
            if b.media_id.unwrap_or(0) != 0 {
                continue;
            }
            let Some(span) = b.inline_raster else {
                continue;
            };
            let Some(raw) = tl_stream.original_bytes.get(span.start..span.end) else {
                continue;
            };
            let Ok(decoded) = decode_binary_bitmap(raw, 0) else {
                continue;
            };
            let res_id = format!("fla-inline-bitmap-{}-{}", tl_stream.index, obj.object_id);
            let (orig_w, orig_h) = (decoded.width, decoded.height);
            let inline_dims = inline_by_id
                .get(res_id.as_str())
                .and_then(|r| r.decoded_bytes().ok())
                .and_then(|bytes| image_dimensions(&bytes));
            let omitted = !inline_by_id.contains_key(res_id.as_str());
            let downscaled = !omitted && inline_dims != Some((orig_w, orig_h));
            if omitted || downscaled {
                let Ok(png) = decoded.png_bytes() else {
                    continue;
                };
                let byte_len = png.len();
                let role = if omitted {
                    OversizedChunkRole::OmittedMedia
                } else {
                    OversizedChunkRole::FullResolutionMedia
                };
                let sha = chunks.insert(png, "image/png", role)?;
                entries.insert(
                    res_id.clone(),
                    OversizedResourceEntry {
                        resource_id: res_id,
                        media_id: None,
                        media_type: "image/png".to_owned(),
                        chunk_sha256: sha,
                        byte_len,
                        original_width: Some(orig_w),
                        original_height: Some(orig_h),
                        inline_downscaled: downscaled,
                        inline_reencoded: false,
                        inline_omitted: omitted,
                    },
                );
            }
        }
    }

    Ok(entries)
}

/// Import a legacy binary `.fla` (`Flash 3–CS4`) into an [`OversizedFlaBundle`] that keeps the
/// inline [`Project`] within NeoFlash's portable `128 MiB` JSON / `64 MiB` resource caps while
/// preserving unscaled full-resolution media and Schema 3 `NFD1` delta chunks for byte-exact
/// SHA-256 `.fla` reconstruction up to `512 MiB` containers / `1 GiB` bundle storage.
pub fn import_legacy_fla_oversized_bundle(path: &Path) -> Result<OversizedFlaBundle, ImportError> {
    let document = super::read_for_import(path)?
        .ok_or_else(|| container("source is not a recognized legacy binary FLA container"))?;
    let mut report = ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::LegacyBinaryFla,
        version: document.inspection.authoring_version.clone(),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };
    let project_opt = super::convert::convert(&document, &mut report);
    report.can_import = project_opt.is_some()
        && !report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
    let Some(project) = project_opt.filter(|_| report.can_import) else {
        return Err(ImportError::Blocked(
            report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| format!("{} [{}]: {}", d.location, d.code, d.message))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    };
    project
        .validate()
        .map_err(|e| container(format!("converted project failed validation: {e}")))?;

    let mut chunk_builder = ChunkBuilder::new();
    let full_resolution_resources =
        collect_oversized_media(&document, &project, &mut chunk_builder)?;

    let source_bytes_u64 = document.original_bytes.len() as u64;
    let source_fnv1a64 = format!("{:016x}", fnv1a64_bytes(&document.original_bytes));
    let source_sha256 = sha256_hex(&document.original_bytes);

    let inline_media_cache = build_inline_media_cache(&project);

    let source_archive = if reconstruct_legacy_fla_bytes(&project)?.is_some() {
        OversizedSourceArchive::InlineProject {
            source_bytes: source_bytes_u64,
            source_fnv1a64,
            source_sha256,
        }
    } else {
        let mut candidate_keys: Vec<(String, &[u8])> = Vec::new();
        for (id, bytes) in &inline_media_cache {
            candidate_keys.push((format!("res:{id}"), bytes.as_slice()));
        }
        for chunk in &chunk_builder.chunks {
            candidate_keys.push((format!("chunk:{}", chunk.sha256), chunk.data.as_slice()));
        }
        let slices: Vec<(&str, &[u8])> = candidate_keys
            .iter()
            .map(|(k, b)| (k.as_str(), *b))
            .collect();
        let rem_budget = chunk_builder.remaining_budget();
        let (delta_gz, used_keys) =
            encode_blobs_delta_gzip(&document.original_bytes, &slices, rem_budget, false)
                .map_err(|e| container(format!("failed to encode NFD1 delta archive: {e}")))?
                .ok_or_else(|| {
                    container(
                        "oversized FLA NFD1 delta archive exceeds 1 GiB sidecar bundle budget",
                    )
                })?;

        let mut references = Vec::with_capacity(used_keys.len());
        for key in used_keys {
            if let Some(id) = key.strip_prefix("res:") {
                references.push(OversizedDeltaReference::InlineResource { id: id.to_owned() });
            } else if let Some(sha) = key.strip_prefix("chunk:") {
                references.push(OversizedDeltaReference::BundleChunk {
                    sha256: sha.to_owned(),
                });
            } else {
                return Err(container("internal NFD1 delta reference tag mismatch"));
            }
        }

        let mut delta_chunk_sha256s = Vec::new();
        for part in delta_gz.chunks(BUNDLE_DELTA_PART_BYTES) {
            let sha = chunk_builder.insert(
                part.to_vec(),
                "application/x-neoflash-nfd1+gzip",
                OversizedChunkRole::SourceDeltaGzip,
            )?;
            delta_chunk_sha256s.push(sha);
        }

        OversizedSourceArchive::BundleDeltaGzip {
            source_bytes: source_bytes_u64,
            source_fnv1a64,
            source_sha256,
            delta_chunk_sha256s,
            references,
        }
    };

    Ok(OversizedFlaBundle {
        project,
        report,
        chunks: chunk_builder.chunks,
        full_resolution_resources,
        source_archive,
        inline_media_cache,
    })
}

fn ensure_not_symlink(path: &Path) -> Result<fs::Metadata, ImportError> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(container(format!(
            "refusing symlink inside oversized FLA bundle: {}",
            path.display()
        )));
    }
    Ok(meta)
}

fn read_regular_file_bounded(
    base_dir: &Path,
    segments: &[&str],
    max_bytes: usize,
) -> Result<Vec<u8>, ImportError> {
    let mut cur = base_dir.to_path_buf();
    for (idx, seg) in segments.iter().enumerate() {
        if seg.is_empty() || *seg == "." || *seg == ".." || seg.contains(['/', '\\', ':', '\0']) {
            return Err(container(format!(
                "rejected unsafe path segment in oversized bundle: {seg:?}"
            )));
        }
        cur.push(seg);
        let meta = ensure_not_symlink(&cur)?;
        let is_last = idx + 1 == segments.len();
        if is_last {
            if !meta.is_file() {
                return Err(container(format!(
                    "expected regular file in oversized bundle: {}",
                    cur.display()
                )));
            }
            if meta.len() > max_bytes as u64 {
                return Err(container(format!(
                    "oversized bundle file {} ({} bytes) exceeds {} byte limit",
                    cur.display(),
                    meta.len(),
                    max_bytes
                )));
            }
        } else if !meta.is_dir() {
            return Err(container(format!(
                "expected directory in oversized bundle: {}",
                cur.display()
            )));
        }
    }
    let mut out = Vec::new();
    File::open(&cur)?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut out)?;
    if out.len() > max_bytes {
        return Err(container(format!(
            "oversized bundle file {} grew beyond {} byte limit",
            cur.display(),
            max_bytes
        )));
    }
    Ok(out)
}

fn write_atomic_regular_file(path: &Path, bytes: &[u8]) -> Result<(), ImportError> {
    let mut f = OpenOptions::new().create_new(true).write(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

impl OversizedFlaBundle {
    /// Return the unscaled, un-reencoded full-resolution media bytes for `resource_id`, either
    /// from the sidecar chunk store (when the inline asset was downscaled, JPEG-reencoded, or
    /// omitted) or from the inline `Project` resource when no inline approximation occurred.
    pub fn full_resolution_resource(&self, resource_id: &str) -> Option<&[u8]> {
        if let Some(entry) = self.full_resolution_resources.get(resource_id) {
            return self
                .chunks
                .iter()
                .find(|c| c.sha256 == entry.chunk_sha256)
                .map(|c| c.data.as_slice());
        }
        self.inline_media_cache.get(resource_id).map(Vec::as_slice)
    }

    /// Reconstruct the byte-exact original binary `.fla` archive from either the inline `Project`
    /// provenance or the bundle's Schema 3 `NFD1` delta chunks, verifying exact byte count,
    /// 64-bit FNV-1a, and SHA-256 digests.
    pub fn reconstruct_fla_bytes(&self) -> Result<Vec<u8>, ImportError> {
        let (expected_u64, expected_fnv, expected_sha, restored) = match &self.source_archive {
            OversizedSourceArchive::InlineProject {
                source_bytes,
                source_fnv1a64,
                source_sha256,
            } => {
                let bytes = reconstruct_legacy_fla_bytes(&self.project)?.ok_or_else(|| {
                    container("bundle declares inline FLA archive, but project has none")
                })?;
                (
                    *source_bytes,
                    source_fnv1a64.as_str(),
                    source_sha256.as_str(),
                    bytes,
                )
            }
            OversizedSourceArchive::BundleDeltaGzip {
                source_bytes,
                source_fnv1a64,
                source_sha256,
                delta_chunk_sha256s,
                references,
            } => {
                if *source_bytes > MAX_CONTAINER {
                    return Err(container("declared FLA source_bytes exceeds 512 MiB limit"));
                }
                if delta_chunk_sha256s.is_empty() {
                    return Err(container("bundle delta archive has no delta chunks"));
                }
                let chunk_map: BTreeMap<&str, &[u8]> = self
                    .chunks
                    .iter()
                    .map(|c| (c.sha256.as_str(), c.data.as_slice()))
                    .collect();

                let mut compressed = Vec::new();
                for sha in delta_chunk_sha256s {
                    let part = chunk_map.get(sha.as_str()).ok_or_else(|| {
                        container(format!("missing NFD1 delta chunk {sha} in bundle"))
                    })?;
                    let next_len = compressed
                        .len()
                        .checked_add(part.len())
                        .filter(|&n| n <= MAX_OVERSIZED_BUNDLE_TOTAL_BYTES)
                        .ok_or_else(|| {
                            container("combined NFD1 delta chunks exceed 1 GiB bundle limit")
                        })?;
                    compressed.reserve(part.len());
                    compressed.extend_from_slice(part);
                    debug_assert_eq!(compressed.len(), next_len);
                }

                let mut owned_inline: Vec<(usize, Vec<u8>)> = Vec::new();
                for (idx, r) in references.iter().enumerate() {
                    if let OversizedDeltaReference::InlineResource { id } = r
                        && !self.inline_media_cache.contains_key(id)
                    {
                        let unit = self
                            .project
                            .resources
                            .iter()
                            .find(|u| &u.id == id)
                            .ok_or_else(|| {
                                container(format!("missing inline delta reference resource {id}"))
                            })?;
                        let decoded = unit.decoded_bytes().map_err(|e| {
                            container(format!("corrupt inline delta reference {id}: {e}"))
                        })?;
                        owned_inline.push((idx, decoded));
                    }
                }
                let owned_inline_map: BTreeMap<usize, &[u8]> = owned_inline
                    .iter()
                    .map(|(idx, v)| (*idx, v.as_slice()))
                    .collect();

                let mut ref_slices: Vec<&[u8]> = Vec::with_capacity(references.len());
                for (idx, r) in references.iter().enumerate() {
                    match r {
                        OversizedDeltaReference::InlineResource { id } => {
                            if let Some(cached) = self.inline_media_cache.get(id) {
                                ref_slices.push(cached.as_slice());
                            } else if let Some(&decoded) = owned_inline_map.get(&idx) {
                                ref_slices.push(decoded);
                            } else {
                                return Err(container(format!(
                                    "missing inline delta reference resource {id}"
                                )));
                            }
                        }
                        OversizedDeltaReference::BundleChunk { sha256 } => {
                            let slice = chunk_map.get(sha256.as_str()).ok_or_else(|| {
                                container(format!("missing sidecar delta reference chunk {sha256}"))
                            })?;
                            ref_slices.push(slice);
                        }
                    }
                }

                let out = decode_nfd1_delta_gzip(&compressed, *source_bytes, &ref_slices)?;
                (
                    *source_bytes,
                    source_fnv1a64.as_str(),
                    source_sha256.as_str(),
                    out,
                )
            }
        };

        if restored.len() as u64 != expected_u64 {
            return Err(container(format!(
                "reconstructed FLA length {} does not match expected {}",
                restored.len(),
                expected_u64
            )));
        }
        let actual_fnv = format!("{:016x}", fnv1a64_bytes(&restored));
        if actual_fnv != expected_fnv {
            return Err(container(
                "reconstructed FLA FNV-1a digest does not match bundle manifest",
            ));
        }
        let actual_sha = sha256_hex(&restored);
        if !actual_sha.eq_ignore_ascii_case(expected_sha) {
            return Err(container(
                "reconstructed FLA SHA-256 digest does not match bundle manifest",
            ));
        }
        Ok(restored)
    }

    fn validate_invariants(&self) -> Result<String, ImportError> {
        let (declared_source_bytes, declared_source_fnv, declared_source_sha) =
            match &self.source_archive {
                OversizedSourceArchive::InlineProject {
                    source_bytes,
                    source_fnv1a64,
                    source_sha256,
                }
                | OversizedSourceArchive::BundleDeltaGzip {
                    source_bytes,
                    source_fnv1a64,
                    source_sha256,
                    ..
                } => (
                    *source_bytes,
                    source_fnv1a64.as_str(),
                    source_sha256.as_str(),
                ),
            };
        if declared_source_bytes > MAX_OVERSIZED_BUNDLE_SOURCE_CONTAINER_BYTES {
            return Err(container("bundle source_bytes exceeds 512 MiB limit"));
        }
        if declared_source_fnv.len() != 16
            || !declared_source_fnv.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(container("invalid source_fnv1a64 in bundle source_archive"));
        }
        if !is_valid_sha256_hex(declared_source_sha) {
            return Err(container("invalid source_sha256 in bundle source_archive"));
        }

        self.project
            .validate()
            .map_err(|e| container(format!("invalid bundle project: {e}")))?;
        let project_json = self
            .project
            .to_json()
            .map_err(|e| container(format!("bundle project JSON serialization failed: {e}")))?;
        if project_json.len() > MAX_PROJECT_JSON_BYTES {
            return Err(container(
                "bundle project JSON exceeds 128 MiB inline project limit",
            ));
        }

        let mut seen = BTreeSet::new();
        let mut total_bytes = 0usize;
        for chunk in &self.chunks {
            if !is_valid_sha256_hex(&chunk.sha256) {
                return Err(container(format!(
                    "invalid chunk SHA-256 hex identifier: {:?}",
                    chunk.sha256
                )));
            }
            if !seen.insert(chunk.sha256.as_str()) {
                return Err(container(format!(
                    "duplicate chunk SHA-256 in bundle: {}",
                    chunk.sha256
                )));
            }
            if chunk.data.len() != chunk.byte_len {
                return Err(container(format!(
                    "chunk {} byte_len {} does not match payload length {}",
                    chunk.sha256,
                    chunk.byte_len,
                    chunk.data.len()
                )));
            }
            if chunk.byte_len > MAX_OVERSIZED_BUNDLE_CHUNK_BYTES {
                return Err(container(format!(
                    "chunk {} ({} bytes) exceeds 256 MiB per-chunk limit",
                    chunk.sha256, chunk.byte_len
                )));
            }
            total_bytes = total_bytes
                .checked_add(chunk.byte_len)
                .filter(|&n| n <= MAX_OVERSIZED_BUNDLE_TOTAL_BYTES)
                .ok_or_else(|| {
                    container("bundle chunks exceed 1 GiB total sidecar storage limit")
                })?;
            if sha256_hex(&chunk.data) != chunk.sha256 {
                return Err(container(format!(
                    "chunk {} SHA-256 digest mismatch",
                    chunk.sha256
                )));
            }
        }

        for (key, entry) in &self.full_resolution_resources {
            if key != &entry.resource_id {
                return Err(container(
                    "full_resolution_resources map key disagrees with resource_id",
                ));
            }
            let Some(chunk) = self.chunks.iter().find(|c| c.sha256 == entry.chunk_sha256) else {
                return Err(container(format!(
                    "resource {} references absent chunk {}",
                    entry.resource_id, entry.chunk_sha256
                )));
            };
            if chunk.byte_len != entry.byte_len {
                return Err(container(format!(
                    "resource {} byte_len disagrees with chunk {}",
                    entry.resource_id, entry.chunk_sha256
                )));
            }
        }

        if let OversizedSourceArchive::BundleDeltaGzip {
            source_bytes,
            source_sha256,
            delta_chunk_sha256s,
            references,
            ..
        } = &self.source_archive
        {
            if *source_bytes > MAX_CONTAINER {
                return Err(container("bundle source_bytes exceeds 512 MiB limit"));
            }
            if !is_valid_sha256_hex(source_sha256) {
                return Err(container("invalid source_sha256 in bundle source_archive"));
            }
            if delta_chunk_sha256s.is_empty() {
                return Err(container("bundle delta archive lists zero chunks"));
            }
            for sha in delta_chunk_sha256s {
                if !seen.contains(sha.as_str()) {
                    return Err(container(format!(
                        "bundle source_archive references missing delta chunk {sha}"
                    )));
                }
            }
            for r in references {
                match r {
                    OversizedDeltaReference::InlineResource { id } => {
                        if !self.project.resources.iter().any(|u| &u.id == id) {
                            return Err(container(format!(
                                "bundle source_archive references missing inline resource {id}"
                            )));
                        }
                    }
                    OversizedDeltaReference::BundleChunk { sha256 } => {
                        if !seen.contains(sha256.as_str()) {
                            return Err(container(format!(
                                "bundle source_archive references missing chunk {sha256}"
                            )));
                        }
                    }
                }
            }
        }

        Ok(project_json)
    }

    /// Persist this bundle atomically to `dir` via a sibling temporary staging directory and
    /// atomic `rename`, rejecting symlink targets.
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), ImportError> {
        let project_json = self.validate_invariants()?;
        let parent = dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let parent_meta = ensure_not_symlink(parent)?;
        if !parent_meta.is_dir() {
            return Err(container(format!(
                "bundle parent path is not a directory: {}",
                parent.display()
            )));
        }
        if let Ok(existing_meta) = fs::symlink_metadata(dir) {
            if existing_meta.file_type().is_symlink() {
                return Err(container(format!(
                    "refusing to overwrite symlink target: {}",
                    dir.display()
                )));
            }
            if !existing_meta.is_dir() {
                return Err(container(format!(
                    "refusing to overwrite non-directory path: {}",
                    dir.display()
                )));
            }
        }

        static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
        let nonce = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let staging: PathBuf = parent.join(format!(".neoflash-bundle-tmp-{pid}-{nonce}"));
        let backup: PathBuf = parent.join(format!(".neoflash-bundle-bak-{pid}-{nonce}"));

        fs::create_dir(&staging)?;
        let write_result = (|| -> Result<(), ImportError> {
            let chunks_dir = staging.join("chunks");
            fs::create_dir(&chunks_dir)?;
            for chunk in &self.chunks {
                let chunk_path = chunks_dir.join(format!("{}.bin", chunk.sha256));
                write_atomic_regular_file(&chunk_path, &chunk.data)?;
            }

            let report_bytes = serde_json::to_vec_pretty(&self.report)
                .map_err(|e| container(format!("report serialization failed: {e}")))?;
            let manifest = OversizedBundleManifest {
                schema_version: BUNDLE_SCHEMA_VERSION,
                chunks: self
                    .chunks
                    .iter()
                    .map(|c| OversizedChunkManifestEntry {
                        sha256: c.sha256.clone(),
                        byte_len: c.byte_len,
                        media_type: c.media_type.clone(),
                        role: c.role,
                    })
                    .collect(),
                full_resolution_resources: self.full_resolution_resources.clone(),
                source_archive: self.source_archive.clone(),
            };
            let manifest_bytes = serde_json::to_vec_pretty(&manifest)
                .map_err(|e| container(format!("manifest serialization failed: {e}")))?;

            write_atomic_regular_file(&staging.join("project.nflash"), project_json.as_bytes())?;
            write_atomic_regular_file(&staging.join("report.json"), &report_bytes)?;
            write_atomic_regular_file(&staging.join("bundle-manifest.json"), &manifest_bytes)?;
            Ok(())
        })();

        if let Err(err) = write_result {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }

        if dir.exists() {
            if let Err(err) = fs::rename(dir, &backup) {
                let _ = fs::remove_dir_all(&staging);
                return Err(err.into());
            }
            if let Err(err) = fs::rename(&staging, dir) {
                let _ = fs::rename(&backup, dir);
                let _ = fs::remove_dir_all(&staging);
                return Err(err.into());
            }
            let _ = fs::remove_dir_all(&backup);
        } else if let Err(err) = fs::rename(&staging, dir) {
            let _ = fs::remove_dir_all(&staging);
            return Err(err.into());
        }

        Ok(())
    }

    /// Load and cryptographically verify an [`OversizedFlaBundle`] from `dir`, rejecting symlinks,
    /// path traversal, corrupt chunk digests, and oversized manifests or chunks.
    pub fn load_from_dir(dir: &Path) -> Result<Self, ImportError> {
        let root_meta = ensure_not_symlink(dir)?;
        if !root_meta.is_dir() {
            return Err(container(format!(
                "oversized FLA bundle path is not a directory: {}",
                dir.display()
            )));
        }

        for top_entry in fs::read_dir(dir)? {
            let top_entry = top_entry?;
            ensure_not_symlink(&top_entry.path())?;
        }

        let manifest_bytes =
            read_regular_file_bounded(dir, &["bundle-manifest.json"], MAX_MANIFEST_BYTES)?;
        let manifest: OversizedBundleManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| container(format!("invalid bundle-manifest.json: {e}")))?;
        if manifest.schema_version != BUNDLE_SCHEMA_VERSION {
            return Err(container(format!(
                "unsupported oversized bundle schema_version {}",
                manifest.schema_version
            )));
        }

        let project_bytes =
            read_regular_file_bounded(dir, &["project.nflash"], MAX_PROJECT_JSON_BYTES)?;
        let project_str = std::str::from_utf8(&project_bytes)
            .map_err(|e| container(format!("project.nflash is not UTF-8: {e}")))?;
        let project = Project::from_json(project_str)
            .map_err(|e| container(format!("invalid project.nflash: {e}")))?;

        let report_bytes = read_regular_file_bounded(dir, &["report.json"], MAX_REPORT_BYTES)?;
        let report: ImportReport = serde_json::from_slice(&report_bytes)
            .map_err(|e| container(format!("invalid report.json: {e}")))?;

        let chunks_dir = dir.join("chunks");
        if chunks_dir.exists() {
            let chunks_meta = ensure_not_symlink(&chunks_dir)?;
            if !chunks_meta.is_dir() {
                return Err(container("bundle chunks path is not a directory"));
            }
            for entry in fs::read_dir(&chunks_dir)? {
                let entry = entry?;
                ensure_not_symlink(&entry.path())?;
            }
        }

        let mut chunks = Vec::with_capacity(manifest.chunks.len());
        let mut seen = BTreeSet::new();
        let mut total_bytes = 0usize;
        for entry in manifest.chunks {
            if entry.sha256.contains(['/', '\\', ':', '\0']) || entry.sha256.contains("..") {
                return Err(container(format!(
                    "rejected path traversal in bundle chunk identifier: {:?}",
                    entry.sha256
                )));
            }
            if !is_valid_sha256_hex(&entry.sha256) {
                return Err(container(format!(
                    "invalid chunk SHA-256 identifier in manifest: {:?}",
                    entry.sha256
                )));
            }
            if !seen.insert(entry.sha256.clone()) {
                return Err(container(format!(
                    "duplicate chunk SHA-256 in manifest: {}",
                    entry.sha256
                )));
            }
            if entry.byte_len > MAX_OVERSIZED_BUNDLE_CHUNK_BYTES {
                return Err(container(format!(
                    "manifest chunk {} ({} bytes) exceeds 256 MiB per-chunk limit",
                    entry.sha256, entry.byte_len
                )));
            }
            total_bytes = total_bytes
                .checked_add(entry.byte_len)
                .filter(|&n| n <= MAX_OVERSIZED_BUNDLE_TOTAL_BYTES)
                .ok_or_else(|| {
                    container("manifest chunks exceed 1 GiB total sidecar storage limit")
                })?;

            let filename = format!("{}.bin", entry.sha256);
            let data = read_regular_file_bounded(
                dir,
                &["chunks", &filename],
                MAX_OVERSIZED_BUNDLE_CHUNK_BYTES,
            )?;
            if data.len() != entry.byte_len {
                return Err(container(format!(
                    "chunk {} length {} does not match manifest byte_len {}",
                    entry.sha256,
                    data.len(),
                    entry.byte_len
                )));
            }
            let actual_sha = sha256_hex(&data);
            if actual_sha != entry.sha256 {
                return Err(container(format!(
                    "chunk {} SHA-256 verification failed (got {actual_sha})",
                    entry.sha256
                )));
            }
            chunks.push(OversizedMediaChunk {
                sha256: entry.sha256,
                byte_len: entry.byte_len,
                media_type: entry.media_type,
                role: entry.role,
                data,
            });
        }

        let inline_media_cache = build_inline_media_cache(&project);
        let bundle = Self {
            project,
            report,
            chunks,
            full_resolution_resources: manifest.full_resolution_resources,
            source_archive: manifest.source_archive,
            inline_media_cache,
        };
        bundle.validate_invariants()?;
        Ok(bundle)
    }
}
