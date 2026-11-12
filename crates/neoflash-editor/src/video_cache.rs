//! Streaming native video decoder and bounded GPU texture cache for the Stage (`T-201`).
//!
//! Driven by Rust's `TimelineVideoPreview` (`Player` / `VideoCommand` scheduler and static scrub
//! evaluation), this module decodes `Embedded` and `External` video streams once per distinct
//! deployment source via `neoflash_export::resolve_and_decode_project_video` and caches per-frame
//! `egui::TextureHandle`s. GPU textures are bounded here; decoded CPU frames remain for the stream
//! lifetime within an aggregate byte/frame budget so random seeks stay deterministic.

use eframe::egui::{self, TextureHandle, TextureId, TextureOptions};
use neoflash_core::{
    ColorTransform, Project, ResolvedVideoPlacement,
    timeline_video::{VideoDeploymentMode, timeline_video_sources},
};
use neoflash_export::{DecodedVideoFrame, DecodedVideoStream, resolve_and_decode_project_video};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

const MAX_VIDEO_TEXTURE_BYTES: usize = 128 * 1024 * 1024;
const MAX_VIDEO_DECODED_STREAM_CACHE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_VIDEO_DECODED_STREAM_CACHE_FRAMES: usize = 144_000;
const EXTERNAL_VIDEO_RECHECK_INTERVAL: Duration = Duration::from_secs(1);
type TextureKey = (String, usize, [u64; 8]);

struct CachedStream {
    resource_id: String,
    source_b64: Option<Arc<str>>,
    external_stamp: Option<ExternalVideoStamp>,
    last_external_check: Instant,
    result: Result<Arc<DecodedVideoStream>, String>,
    retry_after_cache_generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DecodedStreamCacheUsage {
    bytes: usize,
    frames: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodedStreamCacheBudgetError {
    AccountingOverflow,
    Bytes { required: usize, limit: usize },
    Frames { required: usize, limit: usize },
}

fn decoded_stream_cache_usage<'a>(
    streams: impl Iterator<Item = &'a DecodedVideoStream>,
) -> Option<DecodedStreamCacheUsage> {
    const ARC_ALLOCATION_OVERHEAD: usize = 2 * std::mem::size_of::<usize>();

    let mut seen_streams = HashSet::new();
    let mut seen_images = HashSet::new();
    let mut seen_rgba = HashSet::new();
    let mut seen_decoders = HashSet::new();
    let mut bytes = 0usize;
    let mut frames = 0usize;

    let add_bytes = |total: &mut usize, allocation: usize| {
        *total = total.checked_add(allocation)?;
        Some(())
    };

    for stream in streams {
        if !seen_streams.insert(stream as *const DecodedVideoStream as usize) {
            continue;
        }

        frames = frames.checked_add(stream.logical_frame_count())?;
        let stream_allocation = std::mem::size_of::<DecodedVideoStream>()
            .checked_add(ARC_ALLOCATION_OVERHEAD)?
            .checked_add(
                stream
                    .frames
                    .capacity()
                    .checked_mul(std::mem::size_of::<DecodedVideoFrame>())?,
            )?;
        add_bytes(&mut bytes, stream_allocation)?;

        if let Some(decoder) = &stream.on_demand
            && seen_decoders.insert(Arc::as_ptr(decoder) as usize)
        {
            add_bytes(&mut bytes, decoder.pinned_budget_bytes()?)?;
        }

        for frame in &stream.frames {
            if seen_images.insert(Arc::as_ptr(&frame.image) as usize) {
                add_bytes(
                    &mut bytes,
                    std::mem::size_of::<neoflash_core::DecodedImage>()
                        .checked_add(ARC_ALLOCATION_OVERHEAD)?,
                )?;
            }

            let rgba = &frame.image.rgba;
            let allocation = (rgba.as_ptr() as usize, rgba.len());
            if seen_rgba.insert(allocation) {
                add_bytes(&mut bytes, rgba.len().checked_add(ARC_ALLOCATION_OVERHEAD)?)?;
            }
        }
    }
    Some(DecodedStreamCacheUsage { bytes, frames })
}

fn validate_decoded_stream_cache_budget<'a>(
    streams: impl Iterator<Item = &'a DecodedVideoStream>,
    byte_limit: usize,
    frame_limit: usize,
) -> Result<DecodedStreamCacheUsage, DecodedStreamCacheBudgetError> {
    let usage = decoded_stream_cache_usage(streams)
        .ok_or(DecodedStreamCacheBudgetError::AccountingOverflow)?;
    if usage.frames > frame_limit {
        return Err(DecodedStreamCacheBudgetError::Frames {
            required: usage.frames,
            limit: frame_limit,
        });
    }
    if usage.bytes > byte_limit {
        return Err(DecodedStreamCacheBudgetError::Bytes {
            required: usage.bytes,
            limit: byte_limit,
        });
    }
    Ok(usage)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExternalVideoStamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn source_cache_key(
    resource: &str,
    source: Option<&neoflash_core::VideoSourceSpec>,
    base_dir: Option<&Path>,
) -> String {
    match source {
        Some(source) => {
            let base = base_dir
                .map(|path| path.to_string_lossy())
                .unwrap_or_default();
            format!(
                "{}:{}|{}|{}:{}|{}:{}|{}:{}",
                resource.len(),
                resource,
                source.mode.as_str(),
                source.url_or_path.len(),
                source.url_or_path,
                source.mime_type.len(),
                source.mime_type,
                base.len(),
                base,
            )
        }
        None => resource.to_owned(),
    }
}

fn placement_cache_key(placement: &ResolvedVideoPlacement, base_dir: Option<&Path>) -> String {
    source_cache_key(&placement.resource, placement.source.as_ref(), base_dir)
}

fn external_video_stamp(
    placement: &ResolvedVideoPlacement,
    base_dir: Option<&Path>,
) -> Option<ExternalVideoStamp> {
    let source = placement.source.as_ref()?;
    if source.mode != VideoDeploymentMode::External {
        return None;
    }
    let path = base_dir?.join(&source.url_or_path);
    let metadata = std::fs::metadata(&path).ok()?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Some(ExternalVideoStamp {
        path,
        len: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    })
}

struct CachedFrameTexture {
    handle: TextureHandle,
    bytes: usize,
    image: Arc<egui::ColorImage>,
}

#[derive(Default)]
pub struct VideoTextures {
    streams: HashMap<String, CachedStream>,
    textures: HashMap<TextureKey, CachedFrameTexture>,
    used: HashSet<TextureKey>,
    reported_errors: HashMap<String, String>,
    warnings: Vec<String>,
    last_base_dir: Option<PathBuf>,
    stream_cache_generation: u64,
    authored_source_revision: Option<u64>,
    authored_source_keys: HashSet<String>,
    #[cfg(test)]
    authored_source_scan_count: usize,
}

impl VideoTextures {
    pub fn begin_frame(
        &mut self,
        project: &Project,
        base_dir: Option<&Path>,
        document_revision: u64,
    ) {
        self.used.clear();
        let base_owned = base_dir.map(Path::to_path_buf);
        if self.last_base_dir != base_owned {
            let released_decoded_stream = self.streams.values().any(|cached| cached.result.is_ok());
            self.streams.clear();
            self.textures.clear();
            self.reported_errors.clear();
            self.last_base_dir = base_owned;
            self.authored_source_revision = None;
            self.authored_source_keys.clear();
            if released_decoded_stream {
                self.stream_cache_generation = self.stream_cache_generation.wrapping_add(1);
            }
        }
        if self.streams.is_empty() {
            self.authored_source_revision = None;
            self.authored_source_keys.clear();
            return;
        }
        if self.authored_source_revision != Some(document_revision) {
            self.authored_source_keys = timeline_video_sources(project)
                .into_iter()
                .map(|(resource, source)| source_cache_key(&resource, Some(&source), base_dir))
                .collect();
            self.authored_source_revision = Some(document_revision);
            #[cfg(test)]
            {
                self.authored_source_scan_count += 1;
            }
        }
        let mut released_decoded_stream = false;
        self.streams.retain(|key, cached| {
            let current_resource = project
                .resources
                .iter()
                .find(|resource| resource.id == cached.resource_id);
            let source_matches = match (&cached.source_b64, current_resource) {
                (Some(cached_source), Some(resource)) => {
                    Arc::ptr_eq(cached_source, &resource.data_base64)
                }
                (None, None) => true,
                _ => false,
            };
            let is_still_authored = self.authored_source_keys.contains(key)
                || (key == &cached.resource_id && current_resource.is_some());
            let keep = source_matches && is_still_authored;
            if !keep && cached.result.is_ok() {
                released_decoded_stream = true;
            }
            keep
        });
        if released_decoded_stream {
            self.stream_cache_generation = self.stream_cache_generation.wrapping_add(1);
        }
        self.textures
            .retain(|(id, _, _), _| self.streams.contains_key(id));
        if self.streams.is_empty() {
            self.authored_source_revision = None;
            self.authored_source_keys.clear();
        }
    }

    pub fn stream_for_placement(
        &mut self,
        project: &Project,
        base_dir: Option<&Path>,
        placement: &ResolvedVideoPlacement,
    ) -> Result<Arc<DecodedVideoStream>, String> {
        let res_b64 = project
            .resources
            .iter()
            .find(|r| r.id == placement.resource)
            .map(|r| r.data_base64.clone());
        let cache_key = placement_cache_key(placement, base_dir);
        let check_external = self.streams.get(&cache_key).is_none_or(|cached| {
            cached.last_external_check.elapsed() >= EXTERNAL_VIDEO_RECHECK_INTERVAL
        });
        let external_stamp = if check_external {
            external_video_stamp(placement, base_dir)
        } else {
            self.streams
                .get(&cache_key)
                .and_then(|cached| cached.external_stamp.clone())
        };

        let cache_generation = self.stream_cache_generation;
        let needs_reload = match self.streams.get(&cache_key) {
            Some(cached) => {
                let b64_match = match (&cached.source_b64, &res_b64) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                let budget_can_be_retried = cached
                    .retry_after_cache_generation
                    .is_some_and(|generation| generation != cache_generation);
                !b64_match || cached.external_stamp != external_stamp || budget_can_be_retried
            }
            None => true,
        };

        if needs_reload {
            let previous_had_decoded_stream = self
                .streams
                .get(&cache_key)
                .is_some_and(|cached| cached.result.is_ok());
            self.textures
                .retain(|(stream_key, _, _), _| stream_key != &cache_key);
            let mut result = resolve_and_decode_project_video(
                project,
                &placement.resource,
                placement.source.as_ref(),
                base_dir,
            );
            let mut budget_rejected = false;
            if let Ok(candidate) = &result {
                let cached_streams = self
                    .streams
                    .iter()
                    .filter(|(key, _)| *key != &cache_key)
                    .filter_map(|(_, cached)| cached.result.as_deref().ok());
                if let Err(error) = validate_decoded_stream_cache_budget(
                    cached_streams.chain(std::iter::once(candidate.as_ref())),
                    MAX_VIDEO_DECODED_STREAM_CACHE_BYTES,
                    MAX_VIDEO_DECODED_STREAM_CACHE_FRAMES,
                ) {
                    budget_rejected = true;
                    result = Err(match error {
                        DecodedStreamCacheBudgetError::AccountingOverflow => format!(
                            "Video '{}' cannot be cached because decoded-frame memory accounting overflowed; use smaller or shorter video sources.",
                            placement.resource
                        ),
                        DecodedStreamCacheBudgetError::Bytes { required, limit } => format!(
                            "Video '{}' would retain at least {required} decoded-frame bytes including frame records, exceeding the {} MiB native cache budget; close other documents or use smaller/shorter video sources.",
                            placement.resource,
                            limit / (1024 * 1024)
                        ),
                        DecodedStreamCacheBudgetError::Frames { required, limit } => format!(
                            "Video '{}' would retain {required} decoded frame records, exceeding the aggregate {limit}-frame native cache budget; close other documents or use fewer/shorter video sources.",
                            placement.resource
                        ),
                    });
                }
            }
            if let Err(err) = &result {
                if self.reported_errors.get(&placement.resource) != Some(err) {
                    self.reported_errors
                        .insert(placement.resource.clone(), err.clone());
                    self.warnings.push(err.clone());
                }
            } else {
                self.reported_errors.remove(&placement.resource);
            }
            if previous_had_decoded_stream || result.is_ok() {
                self.stream_cache_generation = self.stream_cache_generation.wrapping_add(1);
            }
            let retry_after_cache_generation =
                budget_rejected.then_some(self.stream_cache_generation);
            self.streams.insert(
                cache_key.clone(),
                CachedStream {
                    resource_id: placement.resource.clone(),
                    source_b64: res_b64,
                    external_stamp,
                    last_external_check: Instant::now(),
                    result,
                    retry_after_cache_generation,
                },
            );
        } else if check_external && let Some(cached) = self.streams.get_mut(&cache_key) {
            cached.last_external_check = Instant::now();
            cached.external_stamp = external_stamp;
        }

        self.streams
            .get(&cache_key)
            .expect("inserted video stream entry")
            .result
            .clone()
    }

    pub fn frame_for_placement(
        &mut self,
        project: &Project,
        base_dir: Option<&Path>,
        placement: &ResolvedVideoPlacement,
    ) -> Option<(usize, DecodedVideoFrame)> {
        let stream = self
            .stream_for_placement(project, base_dir, placement)
            .ok()?;
        stream.frame_at_placement(placement)
    }

    pub fn duration_for_placement(
        &mut self,
        project: &Project,
        base_dir: Option<&Path>,
        placement: &ResolvedVideoPlacement,
    ) -> Result<u32, String> {
        let duration_ms = self
            .stream_for_placement(project, base_dir, placement)?
            .duration_ms;
        (duration_ms > 0)
            .then_some(duration_ms)
            .ok_or_else(|| format!("Video '{}' has no playable duration.", placement.resource))
    }

    pub fn texture_for_placement(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        base_dir: Option<&Path>,
        placement: &ResolvedVideoPlacement,
        color: ColorTransform,
    ) -> Option<TextureId> {
        let (frame_idx, frame) = self.frame_for_placement(project, base_dir, placement)?;
        let channels: [u64; 8] = color
            .multipliers
            .into_iter()
            .chain(color.offsets)
            .map(f64::to_bits)
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        let key: TextureKey = (
            placement_cache_key(placement, base_dir),
            frame_idx,
            channels,
        );
        self.used.insert(key.clone());
        if let Some(tex) = self.textures.get(&key) {
            return Some(tex.handle.id());
        }

        let decoded = &frame.image;
        let bytes = decoded.rgba.len();
        let mut used_bytes: usize = self.textures.values().map(|t| t.bytes).sum();
        if used_bytes + bytes > MAX_VIDEO_TEXTURE_BYTES {
            self.textures.retain(|k, t| {
                if !self.used.contains(k) && used_bytes + bytes > MAX_VIDEO_TEXTURE_BYTES {
                    used_bytes -= t.bytes;
                    false
                } else {
                    true
                }
            });
        }
        if used_bytes + bytes > MAX_VIDEO_TEXTURE_BYTES {
            return None;
        }

        let image = if color.is_identity() {
            Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &decoded.rgba,
            ))
        } else {
            let mut pixels = decoded.rgba.to_vec();
            for px in pixels.as_chunks_mut::<4>().0 {
                let transformed = color.apply([px[0], px[1], px[2], px[3]]);
                px.copy_from_slice(&transformed);
            }
            Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &pixels,
            ))
        };
        let handle = ctx.load_texture(
            format!("video-{}-{frame_idx}", placement.resource),
            image.clone(),
            TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.textures.insert(
            key,
            CachedFrameTexture {
                handle,
                bytes,
                image,
            },
        );
        Some(id)
    }

    pub fn image(&self, id: TextureId) -> Option<(Arc<egui::ColorImage>, bool)> {
        self.textures
            .values()
            .find(|t| t.handle.id() == id)
            .map(|t| (t.image.clone(), true))
    }

    pub fn last_error(&self, resource_id: &str) -> Option<&str> {
        self.reported_errors.get(resource_id).map(String::as_str)
    }

    pub fn take_warning(&mut self) -> Option<String> {
        if self.warnings.is_empty() {
            None
        } else {
            Some(self.warnings.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{DecodedImage, ResourceUnit, VideoSourceSpec, VideoSyncMode};
    use neoflash_export::encode_deterministic_mp4_rgba_frames;

    fn solid_fixture(color: [u8; 4]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(4 * 4 * 4);
        for _ in 0..16 {
            rgba.extend_from_slice(&color);
        }
        encode_deterministic_mp4_rgba_frames(4, 4, 10, &[&rgba])
    }

    fn external_placement(path: &str) -> ResolvedVideoPlacement {
        ResolvedVideoPlacement {
            channel: "video".into(),
            target_path: vec!["video-element".into()],
            layer_id: "video-layer".into(),
            resource: "video-resource".into(),
            source: Some(VideoSourceSpec {
                mode: VideoDeploymentMode::External,
                url_or_path: path.into(),
                mime_type: "video/mp4".into(),
            }),
            sync: VideoSyncMode::Stream,
            time_ms: 0,
            in_point_ms: 0,
            out_point_ms: None,
            playing: false,
            loop_video: false,
            muted: false,
            stopped: false,
        }
    }

    fn expire_external_checks(cache: &mut VideoTextures) {
        for stream in cache.streams.values_mut() {
            stream.last_external_check =
                Instant::now() - EXTERNAL_VIDEO_RECHECK_INTERVAL - Duration::from_millis(1);
        }
    }

    #[test]
    fn decoded_stream_budget_counts_structures_and_deduplicates_aliases_and_shared_images() {
        let shared = Arc::new(DecodedImage {
            width: 4,
            height: 4,
            rgba: vec![1_u8; 64].into(),
        });
        let distinct = Arc::new(DecodedImage {
            width: 4,
            height: 4,
            rgba: vec![2_u8; 64].into(),
        });
        let stream_a = Arc::new(DecodedVideoStream {
            width: 4,
            height: 4,
            fps: 10.0,
            duration_ms: 200,
            frames: vec![
                DecodedVideoFrame {
                    timestamp_ms: 0,
                    duration_ms: 100,
                    image: shared.clone(),
                },
                DecodedVideoFrame {
                    timestamp_ms: 100,
                    duration_ms: 100,
                    image: shared,
                },
            ],
            on_demand: None,
        });
        let stream_b = DecodedVideoStream {
            width: 4,
            height: 4,
            fps: 10.0,
            duration_ms: 100,
            frames: vec![DecodedVideoFrame {
                timestamp_ms: 0,
                duration_ms: 100,
                image: distinct,
            }],
            on_demand: None,
        };

        let stream_a_usage =
            decoded_stream_cache_usage(std::iter::once(stream_a.as_ref())).unwrap();
        assert_eq!(stream_a_usage.frames, 2);
        assert_eq!(
            decoded_stream_cache_usage([stream_a.as_ref(), stream_a.as_ref()].into_iter()),
            Some(stream_a_usage),
            "aliases of the same decoded stream must not consume the budget twice"
        );

        let expected_stream_a_bytes = std::mem::size_of::<DecodedVideoStream>()
            + 2 * std::mem::size_of::<usize>()
            + stream_a.frames.capacity() * std::mem::size_of::<DecodedVideoFrame>()
            + std::mem::size_of::<DecodedImage>()
            + 2 * std::mem::size_of::<usize>()
            + 64
            + 2 * std::mem::size_of::<usize>();
        assert_eq!(stream_a_usage.bytes, expected_stream_a_bytes);

        let usage = decoded_stream_cache_usage([stream_a.as_ref(), &stream_b].into_iter()).unwrap();
        assert_eq!(usage.frames, 3);
        assert_eq!(
            validate_decoded_stream_cache_budget(
                [stream_a.as_ref(), &stream_b].into_iter(),
                usage.bytes,
                usage.frames,
            ),
            Ok(usage)
        );
        assert_eq!(
            validate_decoded_stream_cache_budget(
                [stream_a.as_ref(), &stream_b].into_iter(),
                usage.bytes - 1,
                usage.frames,
            ),
            Err(DecodedStreamCacheBudgetError::Bytes {
                required: usage.bytes,
                limit: usage.bytes - 1,
            })
        );
    }

    #[test]
    fn decoded_stream_budget_rejects_many_tiny_frames_before_payload_limit() {
        let shared = Arc::new(DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![0_u8; 4].into(),
        });
        let tiny_stream = |start_ms: u32| DecodedVideoStream {
            width: 1,
            height: 1,
            fps: 1_000.0,
            duration_ms: 3,
            frames: (0..3)
                .map(|index| DecodedVideoFrame {
                    timestamp_ms: start_ms + index,
                    duration_ms: 1,
                    image: shared.clone(),
                })
                .collect(),
            on_demand: None,
        };
        let stream_a = tiny_stream(0);
        let stream_b = tiny_stream(3);
        let usage = decoded_stream_cache_usage([&stream_a, &stream_b].into_iter()).unwrap();

        assert_eq!(usage.frames, 6);
        assert!(usage.bytes > shared.rgba.len());
        assert_eq!(
            validate_decoded_stream_cache_budget([&stream_a, &stream_b].into_iter(), usize::MAX, 5,),
            Err(DecodedStreamCacheBudgetError::Frames {
                required: 6,
                limit: 5,
            })
        );
    }

    #[test]
    fn source_inventory_scan_is_skipped_for_empty_and_unchanged_caches() {
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "clip.mp4", "video/mp4", &[]).unwrap(),
        );
        let mut cache = VideoTextures::default();

        cache.begin_frame(&project, None, 1);
        assert_eq!(cache.authored_source_scan_count, 0);

        let (resource, source) = timeline_video_sources(&project).pop().unwrap();
        let cache_key = source_cache_key(&resource, Some(&source), None);
        cache.streams.insert(
            cache_key.clone(),
            CachedStream {
                resource_id: resource,
                source_b64: Some(project.resources[0].data_base64.clone()),
                external_stamp: None,
                last_external_check: Instant::now(),
                result: Err("stable codec failure".into()),
                retry_after_cache_generation: None,
            },
        );

        cache.begin_frame(&project, None, 1);
        assert_eq!(cache.authored_source_scan_count, 1);
        assert!(cache.streams.contains_key(&cache_key));
        for _ in 0..100 {
            cache.begin_frame(&project, None, 1);
        }
        assert_eq!(cache.authored_source_scan_count, 1);

        cache.begin_frame(&project, None, 2);
        cache.begin_frame(&project, None, 2);
        assert_eq!(cache.authored_source_scan_count, 2);
    }

    #[test]
    fn begin_frame_evicts_a_removed_external_video_source() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-video-cache-remove-external-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), solid_fixture([255, 0, 0, 255])).unwrap();

        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "clip.mp4", "video/mp4", &[]).unwrap(),
        );
        let placement = external_placement("clip.mp4");
        let mut cache = VideoTextures::default();
        cache.begin_frame(&project, Some(&dir), 1);
        cache
            .stream_for_placement(&project, Some(&dir), &placement)
            .unwrap();
        cache.begin_frame(&project, Some(&dir), 1);
        assert_eq!(cache.streams.len(), 1);

        project.resources.clear();
        cache.begin_frame(&project, Some(&dir), 2);
        assert!(cache.streams.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn budget_rejected_video_recovers_after_an_obsolete_stream_is_evicted() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-video-cache-budget-recovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), solid_fixture([0, 255, 0, 255])).unwrap();

        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "clip.mp4", "video/mp4", &[]).unwrap(),
        );
        let placement = external_placement("clip.mp4");
        let mut cache = VideoTextures::default();
        cache.begin_frame(&project, Some(&dir), 1);

        let shared = Arc::new(DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![0_u8; 4].into(),
        });
        let filler_frame = DecodedVideoFrame {
            timestamp_ms: 0,
            duration_ms: 1,
            image: shared,
        };
        cache.streams.insert(
            "obsolete-filler".into(),
            CachedStream {
                resource_id: "obsolete-filler".into(),
                source_b64: None,
                external_stamp: None,
                last_external_check: Instant::now(),
                result: Ok(Arc::new(DecodedVideoStream {
                    width: 1,
                    height: 1,
                    fps: 1_000.0,
                    duration_ms: MAX_VIDEO_DECODED_STREAM_CACHE_FRAMES as u32,
                    frames: vec![filler_frame; MAX_VIDEO_DECODED_STREAM_CACHE_FRAMES],
                    on_demand: None,
                })),
                retry_after_cache_generation: None,
            },
        );

        let error = cache
            .stream_for_placement(&project, Some(&dir), &placement)
            .unwrap_err();
        assert!(error.contains("frame native cache budget"));
        let placement_key = placement_cache_key(&placement, Some(&dir));
        assert!(
            cache
                .streams
                .get(&placement_key)
                .is_some_and(|cached| cached.retry_after_cache_generation.is_some())
        );

        cache.begin_frame(&project, Some(&dir), 1);
        assert!(!cache.streams.contains_key("obsolete-filler"));
        assert!(cache.streams.contains_key(&placement_key));

        let recovered = cache
            .stream_for_placement(&project, Some(&dir), &placement)
            .unwrap();
        assert_eq!(recovered.frames.len(), 1);
        assert!(cache.last_error("video-resource").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_path_external_video_replacement_refreshes_frames_and_clears_cached_error() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-video-cache-refresh-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        let replacement = dir.join("replacement.mp4");

        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "clip.mp4", "video/mp4", &[]).unwrap(),
        );
        let placement = external_placement("clip.mp4");
        let mut cache = VideoTextures::default();

        std::fs::write(&path, solid_fixture([255, 0, 0, 255])).unwrap();
        let red = cache
            .frame_for_placement(&project, Some(&dir), &placement)
            .unwrap()
            .1;
        assert_eq!(&red.image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(
            cache
                .duration_for_placement(&project, Some(&dir), &placement)
                .unwrap(),
            100
        );

        // Atomic replacement keeps the authored relative path but changes file identity.
        std::fs::write(&replacement, solid_fixture([0, 0, 255, 255])).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        expire_external_checks(&mut cache);
        let blue = cache
            .frame_for_placement(&project, Some(&dir), &placement)
            .unwrap()
            .1;
        assert_eq!(&blue.image.rgba[..4], &[0, 0, 255, 255]);

        std::fs::write(&replacement, b"not a video").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        expire_external_checks(&mut cache);
        assert!(
            cache
                .stream_for_placement(&project, Some(&dir), &placement)
                .is_err()
        );
        assert!(cache.last_error("video-resource").is_some());
        assert!(
            cache
                .duration_for_placement(&project, Some(&dir), &placement)
                .is_err()
        );

        std::fs::write(&replacement, solid_fixture([0, 255, 0, 255])).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        expire_external_checks(&mut cache);
        let green = cache
            .frame_for_placement(&project, Some(&dir), &placement)
            .unwrap()
            .1;
        assert_eq!(&green.image.rgba[..4], &[0, 255, 0, 255]);
        assert!(cache.last_error("video-resource").is_none());
        assert_eq!(
            cache
                .duration_for_placement(&project, Some(&dir), &placement)
                .unwrap(),
            100
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn t229_video_textures_caches_long_form_streams_within_pinned_ring_and_invalidates_revisions() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-video-cache-t229-long-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long_clip.mp4");

        // 240 frames (> MAX_EAGER_VIDEO_FRAMES = 180) so VideoTextures uses OnDemandVideoDecoder.
        let raw_frames: Vec<Vec<u8>> = (0..240usize)
            .map(|idx| [idx as u8, 200u8.wrapping_sub(idx as u8), 90, 255].repeat(16))
            .collect();
        let raw_slices: Vec<&[u8]> = raw_frames.iter().map(Vec::as_slice).collect();
        std::fs::write(
            &path,
            encode_deterministic_mp4_rgba_frames(4, 4, 20, &raw_slices),
        )
        .unwrap();

        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "long_clip.mp4", "video/mp4", &[]).unwrap(),
        );
        let mut cache = VideoTextures::default();
        let mut placement = external_placement("long_clip.mp4");

        let stream = cache
            .stream_for_placement(&project, Some(&dir), &placement)
            .unwrap();
        assert!(stream.frames.is_empty());
        assert!(stream.on_demand.is_some());
        assert_eq!(stream.logical_frame_count(), 240);

        let usage = decoded_stream_cache_usage(std::iter::once(stream.as_ref())).unwrap();
        assert_eq!(usage.frames, 240);
        assert!(usage.bytes < MAX_VIDEO_DECODED_STREAM_CACHE_BYTES);

        for &seek_idx in &[0usize, 120, 239, 45, 200, 3] {
            placement.time_ms = (seek_idx as u32) * 50;
            let (got_idx, frame) = cache
                .frame_for_placement(&project, Some(&dir), &placement)
                .unwrap();
            assert_eq!(got_idx, seek_idx);
            assert_eq!(
                &frame.image.rgba[..4],
                &[seek_idx as u8, 200u8.wrapping_sub(seek_idx as u8), 90, 255]
            );
            assert!(stream.resident_cpu_bytes() <= 64 * 1024 * 1024);
        }

        // Replacing the external long-form source invalidates the old decoder cleanly.
        let replacement = dir.join("replacement_long.mp4");
        let alt_frames: Vec<Vec<u8>> = (0..220usize)
            .map(|_| [9u8, 88, 177, 255].repeat(16))
            .collect();
        let alt_slices: Vec<&[u8]> = alt_frames.iter().map(Vec::as_slice).collect();
        std::fs::write(
            &replacement,
            encode_deterministic_mp4_rgba_frames(4, 4, 20, &alt_slices),
        )
        .unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        expire_external_checks(&mut cache);
        placement.time_ms = 5_000;
        let (_, refreshed) = cache
            .frame_for_placement(&project, Some(&dir), &placement)
            .unwrap();
        assert_eq!(&refreshed.image.rgba[..4], &[9, 88, 177, 255]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
