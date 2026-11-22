use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::ExportError;
use ab_glyph::{Font, FontArc, FontVec, PxScale, ScaleFont};
use neoflash_core::{
    AuthoredBitmapFill, AuthoredDraw, AuthoredPlayback, AuthoredScene, BlendMode, Color,
    ColorTransform, DecodedImage, DrawItem, ElementKind, FillRule, FilterSpec, GradientKind,
    GradientPaint, Id, MaskCoverage, PathCommand, Project, ResolvedVideoPlacement,
    TimelineVideoTracker, filter_padding, filter_rgba, oval_path_commands, rectangle_path_commands,
    resolve_scene_video_placements,
};
use tiny_skia::{
    Color as SkiaColor, FillRule as SkiaFillRule, FilterQuality, GradientStop as SkiaGradientStop,
    IntRect, IntSize, LineCap, LineJoin, LinearGradient, Mask as SkiaMask, Paint as SkiaPaint,
    Path as SkiaPath, PathBuilder, Pixmap, PixmapPaint, Point as SkiaPoint, RadialGradient,
    SpreadMode, Stroke as SkiaStroke, Transform as SkiaTransform,
};

use super::progressive::{
    insert_decoded_video_stream, placement_video_cache_key, preflight_and_decode_timeline_videos,
};
use super::*;

/// Render a single frame (`0`-based) of a `Project` (or specific symbol) into an RGBA8 buffer of
/// size `width * height * 4`. Used both by the `Dialog / Export video` live preview and by batch export.
pub fn render_project_frame_rgba(
    project: &Project,
    symbol: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
) -> Result<Vec<u8>, ExportError> {
    render_project_frame_rgba_with_base_dir(
        project,
        symbol,
        frame,
        width,
        height,
        transparent_bg,
        runtime_capture,
        None,
    )
}

/// Render a single frame (`0`-based) of a `Project` (or specific symbol) into an RGBA8 buffer,
/// resolving `VideoDeploymentMode::External` relative paths against `base_dir` (`T-201`).
#[allow(clippy::too_many_arguments)]
pub fn render_project_frame_rgba_with_base_dir(
    project: &Project,
    symbol: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
    base_dir: Option<&Path>,
) -> Result<Vec<u8>, ExportError> {
    let opts = VideoOptions {
        symbol: symbol.map(str::to_owned),
        width,
        height,
        transparent_bg,
        runtime_capture,
        base_dir: base_dir.map(Path::to_path_buf),
        ..VideoOptions::default()
    };
    let prepared = prepare_timeline(project, &opts)?;
    let mut video_cache =
        preflight_and_decode_timeline_videos(&prepared.eval_project, &prepared.root_id, base_dir)?;
    let target_frame = if runtime_capture {
        frame
    } else {
        frame % prepared.authored_span.max(1)
    };
    let fps = opts.resolved_fps(&prepared.eval_project);
    let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);

    let (scene, filter_nodes, placements) = if runtime_capture {
        let mut playback =
            AuthoredPlayback::with_live_scripts(&prepared.eval_project, &prepared.root_id, 0, true);
        let mut tracker = TimelineVideoTracker::new();
        tracker.step_with_commands(playback.drain_video_commands(), 0);
        for _ in 0..target_frame {
            playback.tick(&prepared.eval_project);
            tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
        }
        let scene = playback.scene(&prepared.eval_project, false, true);
        let clip_frames = playback.clip_frames();
        let filter_nodes = AuthoredScene::resolve_node_filters(
            &prepared.eval_project,
            playback.symbol(),
            playback.frame(),
            false,
            Some(&clip_frames),
            playback.is_live_scripts(),
            false,
            true,
        );
        let mut map = HashMap::new();
        for draw in &scene.draws {
            if matches!(draw.item.kind, ElementKind::Video { .. })
                && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
            {
                map.insert(draw.item.instance_path.clone(), p.clone());
            }
        }
        (scene, filter_nodes, map)
    } else {
        let (scene, filter_nodes) = AuthoredScene::evaluate_editable_stage_with_filters(
            &prepared.eval_project,
            &prepared.root_id,
            target_frame,
            false,
            true,
        );
        let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
        let map = resolve_scene_video_placements(
            &prepared.eval_project,
            &prepared.root_id,
            target_frame,
            &HashMap::new(),
            &items,
        );
        (scene, filter_nodes, map)
    };

    for placement in placements.values() {
        let cache_key = placement_video_cache_key(placement);
        if !video_cache.contains_key(&cache_key) {
            let stream = resolve_and_decode_project_video(
                &prepared.eval_project,
                &placement.resource,
                placement.source.as_ref(),
                base_dir,
            )
            .map_err(ExportError::Options)?;
            insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                .map_err(ExportError::Options)?;
        }
    }

    let mut bitmap_cache = HashMap::new();
    Ok(render_authored_scene_with_effects_rgba(
        &prepared.eval_project,
        &scene,
        &filter_nodes,
        width.max(1),
        height.max(1),
        transparent_bg,
        &mut bitmap_cache,
        &mut video_cache,
        &placements,
    ))
}

/// Stateful sequential frame renderer for image-sequence exports (`T-259`), avoiding `O(N^2)`
/// video decode and `AuthoredPlayback` re-stepping when `has_timeline_video` is true.
pub(crate) struct SequentialVideoFrameSession {
    prepared: PreparedTimeline,
    video_cache: HashMap<String, Option<Arc<DecodedVideoStream>>>,
    bitmap_cache: HashMap<String, Option<Arc<DecodedImage>>>,
    base_dir: Option<PathBuf>,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
    dt_ms: u32,
    live_state: Option<(AuthoredPlayback, TimelineVideoTracker, u32)>,
}

impl SequentialVideoFrameSession {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        project: &Project,
        symbol: Option<&str>,
        start_frame: u32,
        width: u32,
        height: u32,
        transparent_bg: bool,
        runtime_capture: bool,
        base_dir: Option<&Path>,
    ) -> Result<Self, ExportError> {
        let opts = VideoOptions {
            symbol: symbol.map(str::to_owned),
            width,
            height,
            transparent_bg,
            runtime_capture,
            base_dir: base_dir.map(Path::to_path_buf),
            ..VideoOptions::default()
        };
        let prepared = prepare_timeline(project, &opts)?;
        let video_cache = preflight_and_decode_timeline_videos(
            &prepared.eval_project,
            &prepared.root_id,
            base_dir,
        )?;
        let fps = opts.resolved_fps(&prepared.eval_project);
        let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
        let live_state = runtime_capture.then(|| {
            let mut playback = AuthoredPlayback::with_live_scripts(
                &prepared.eval_project,
                &prepared.root_id,
                0,
                true,
            );
            let mut tracker = TimelineVideoTracker::new();
            tracker.step_with_commands(playback.drain_video_commands(), 0);
            for _ in 0..start_frame {
                playback.tick(&prepared.eval_project);
                tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
            }
            (playback, tracker, start_frame)
        });
        Ok(Self {
            prepared,
            video_cache,
            bitmap_cache: HashMap::new(),
            base_dir: base_dir.map(Path::to_path_buf),
            width: width.max(1),
            height: height.max(1),
            transparent_bg,
            runtime_capture,
            dt_ms,
            live_state,
        })
    }

    pub(crate) fn render_frame(&mut self, frame: u32) -> Result<Vec<u8>, ExportError> {
        let target_frame = if self.runtime_capture {
            frame
        } else {
            frame % self.prepared.authored_span.max(1)
        };

        let (scene, filter_nodes, placements) =
            if let Some((playback, tracker, cur_frame)) = &mut self.live_state {
                if target_frame < *cur_frame {
                    *playback = AuthoredPlayback::with_live_scripts(
                        &self.prepared.eval_project,
                        &self.prepared.root_id,
                        0,
                        true,
                    );
                    *tracker = TimelineVideoTracker::new();
                    tracker.step_with_commands(playback.drain_video_commands(), 0);
                    *cur_frame = 0;
                }
                while *cur_frame < target_frame {
                    playback.tick(&self.prepared.eval_project);
                    tracker.step_with_commands(playback.drain_video_commands(), self.dt_ms);
                    *cur_frame += 1;
                }
                let scene = playback.scene(&self.prepared.eval_project, false, true);
                let clip_frames = playback.clip_frames();
                let filter_nodes = AuthoredScene::resolve_node_filters(
                    &self.prepared.eval_project,
                    playback.symbol(),
                    playback.frame(),
                    false,
                    Some(&clip_frames),
                    playback.is_live_scripts(),
                    false,
                    true,
                );
                let mut map = HashMap::new();
                for draw in &scene.draws {
                    if matches!(draw.item.kind, ElementKind::Video { .. })
                        && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
                    {
                        map.insert(draw.item.instance_path.clone(), p.clone());
                    }
                }
                (scene, filter_nodes, map)
            } else {
                let (scene, filter_nodes) = AuthoredScene::evaluate_editable_stage_with_filters(
                    &self.prepared.eval_project,
                    &self.prepared.root_id,
                    target_frame,
                    false,
                    true,
                );
                let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
                let map = resolve_scene_video_placements(
                    &self.prepared.eval_project,
                    &self.prepared.root_id,
                    target_frame,
                    &HashMap::new(),
                    &items,
                );
                (scene, filter_nodes, map)
            };

        for placement in placements.values() {
            let cache_key = placement_video_cache_key(placement);
            if !self.video_cache.contains_key(&cache_key) {
                let stream = resolve_and_decode_project_video(
                    &self.prepared.eval_project,
                    &placement.resource,
                    placement.source.as_ref(),
                    self.base_dir.as_deref(),
                )
                .map_err(ExportError::Options)?;
                insert_decoded_video_stream(&mut self.video_cache, cache_key, stream)
                    .map_err(ExportError::Options)?;
            }
        }

        Ok(render_authored_scene_with_effects_rgba(
            &self.prepared.eval_project,
            &scene,
            &filter_nodes,
            self.width,
            self.height,
            self.transparent_bg,
            &mut self.bitmap_cache,
            &mut self.video_cache,
            &placements,
        ))
    }
}

#[derive(Default)]
struct RasterFonts(HashMap<(String, bool, bool), FontArc>);

impl RasterFonts {
    fn get(&mut self, project: &Project, family: &str, bold: bool, italic: bool) -> FontArc {
        self.0
            .entry((family.to_owned(), bold, italic))
            .or_insert_with(|| {
                use crate::fonts::{
                    fallback_font, matching_system_faces, resolve_project_font_bytes,
                    system_font_db,
                };
                let embedded =
                    resolve_project_font_bytes(project, family, bold, italic).and_then(|raw| {
                        let bytes = neoflash_core::sfnt_view_bytes(&raw)
                            .map(std::borrow::Cow::into_owned)
                            .unwrap_or(raw);
                        let index = neoflash_core::select_best_sfnt_face_index(
                            &bytes, family, bold, italic,
                        );
                        FontVec::try_from_vec_and_index(bytes, index).ok()
                    });
                let face = embedded.or_else(|| {
                    matching_system_faces(family, bold, italic)
                        .into_iter()
                        .find_map(|face| {
                            system_font_db()
                                .with_face_data(face.id, |bytes, index| {
                                    FontVec::try_from_vec_and_index(bytes.to_vec(), index).ok()
                                })
                                .flatten()
                        })
                });
                face.map(FontArc::new).unwrap_or_else(|| {
                    FontArc::try_from_slice(fallback_font(family, bold))
                        .expect("valid bundled fallback font")
                })
            })
            .clone()
    }
}

/// Render an `AuthoredScene` into a straight-RGBA8 pixel buffer (`width * height * 4`).
#[allow(clippy::too_many_arguments)]
pub fn render_authored_scene_rgba(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
) -> Result<Vec<u8>, ExportError> {
    let mut video_cache: HashMap<String, Option<Arc<DecodedVideoStream>>> = HashMap::new();
    let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
    let placements =
        resolve_scene_video_placements(project, symbol_id, frame, &HashMap::new(), &items);
    for placement in placements.values() {
        let cache_key = placement_video_cache_key(placement);
        if !video_cache.contains_key(&cache_key) {
            let stream = resolve_and_decode_project_video(
                project,
                &placement.resource,
                placement.source.as_ref(),
                None,
            )
            .map_err(ExportError::Options)?;
            insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                .map_err(ExportError::Options)?;
        }
    }
    let filter_nodes = scene.resolve_node_filters_for_scene(project, symbol_id, Some(frame));
    Ok(render_authored_scene_with_effects_rgba(
        project,
        scene,
        &filter_nodes,
        width,
        height,
        transparent_bg,
        bitmap_cache,
        &mut video_cache,
        &placements,
    ))
}

/// Render an `AuthoredScene` with pre-resolved `ResolvedVideoPlacement`s and decoded video streams (`T-201`).
#[allow(clippy::too_many_arguments)]
pub fn render_authored_scene_with_video_rgba(
    project: &Project,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) -> Vec<u8> {
    let filter_nodes = scene.resolve_node_filters_for_scene(project, &project.root, None);
    render_authored_scene_with_effects_rgba(
        project,
        scene,
        &filter_nodes,
        width,
        height,
        transparent_bg,
        bitmap_cache,
        video_cache,
        video_placements,
    )
}

pub(crate) type FilterNodeEntry = (Vec<Id>, Vec<FilterSpec>, Vec<String>, BlendMode);

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_authored_scene_with_effects_rgba(
    project: &Project,
    scene: &AuthoredScene,
    filter_nodes: &[FilterNodeEntry],
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) -> Vec<u8> {
    let w = width.max(1);
    let h = height.max(1);
    let Some(mut pixmap) = Pixmap::new(w, h) else {
        return vec![0; (w * h * 4) as usize];
    };
    if !transparent_bg {
        let bg = project.background;
        pixmap.fill(SkiaColor::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
    }

    let sx = w as f32 / project.width.max(1) as f32;
    let sy = h as f32 / project.height.max(1) as f32;
    let stage_scale = SkiaTransform::from_scale(sx, sy);

    let viewport = IntRect::from_xywh(0, 0, w, h).expect("allocated pixmap dimensions");
    let clips: HashMap<_, _> = scene
        .clips
        .iter()
        .map(|clip| (clip.id.as_str(), clip))
        .collect();
    let clip_bounds: HashMap<_, _> = clips
        .iter()
        .map(|(&id, clip)| (id, clip_pixel_bounds(clip, stage_scale, viewport)))
        .collect();
    let mut bitmap_brush = BitmapBrushCache::default();
    let mut fonts = RasterFonts::default();

    let has_effects = !filter_nodes.is_empty()
        || !scene.mask_groups.is_empty()
        || scene.draws.iter().any(|d| !d.blend_mode.is_normal());

    if !has_effects {
        // Adjacent placements share a mask. Bound its allocation to the intersection of the
        // artboards in this run, and release it before drawing the next run in stacking order.
        for draws in scene.draws.chunk_by(|a, b| a.clip_ids == b.clip_ids) {
            let ids = &draws[0].clip_ids;
            let Some(rect) = ids.iter().try_fold(viewport, |area, id| {
                area.intersect(&clip_bounds.get(id.as_str()).copied().flatten()?)
            }) else {
                continue;
            };
            let transform = stage_scale.post_translate(-(rect.x() as f32), -(rect.y() as f32));
            let mask = if ids.is_empty() {
                None
            } else {
                let Some(mask) = build_clip_mask(
                    project,
                    rect,
                    [
                        f64::from(project.width) / f64::from(w),
                        f64::from(project.height) / f64::from(h),
                    ],
                    transform,
                    ids,
                    &clips,
                ) else {
                    continue;
                };
                Some(mask)
            };
            let mut crop = (rect != viewport)
                .then(|| pixmap.clone_rect(rect).expect("bounded pixmap rectangle"));
            let surface = crop.as_mut().unwrap_or(&mut pixmap);
            for draw in draws {
                draw_authored_item(
                    surface,
                    project,
                    draw,
                    transform,
                    mask.as_ref(),
                    bitmap_cache,
                    &mut bitmap_brush,
                    &mut fonts,
                    video_cache,
                    video_placements,
                );
            }
            if let Some(crop) = crop {
                for (src, dst) in crop.pixels().chunks_exact(rect.width() as usize).zip(
                    pixmap
                        .pixels_mut()
                        .chunks_exact_mut(w as usize)
                        .skip(rect.y() as usize),
                ) {
                    dst[rect.x() as usize..rect.right() as usize].copy_from_slice(src);
                }
            }
        }
    } else {
        let mut ctx = EffectRenderCtx {
            project,
            scene,
            w,
            h,
            sx: f64::from(sx),
            sy: f64::from(sy),
            stage_scale,
            viewport,
            step: [
                f64::from(project.width) / f64::from(w),
                f64::from(project.height) / f64::from(h),
            ],
            clips: &clips,
            clip_bounds: &clip_bounds,
            bitmap_cache,
            bitmap_brush: &mut bitmap_brush,
            fonts: &mut fonts,
            video_cache,
            video_placements,
        };
        render_scene_with_effect_groups(&mut pixmap, &mut ctx, filter_nodes);
    }

    // Convert premultiplied RGBA from tiny-skia to straight RGBA8 for encoders and egui textures.
    let mut out = pixmap.take();
    unpremultiply_rgba_in_place(&mut out);
    out
}

fn unpremultiply_rgba_in_place(bytes: &mut [u8]) {
    for px in bytes.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = f32::from(a) / 255.0;
            px[0] = ((f32::from(px[0]) / af).round() as u32).min(255) as u8;
            px[1] = ((f32::from(px[1]) / af).round() as u32).min(255) as u8;
            px[2] = ((f32::from(px[2]) / af).round() as u32).min(255) as u8;
        }
    }
}

fn premultiply_rgba_in_place(bytes: &mut [u8]) {
    for px in bytes.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a == 0 {
            *px = [0; 4];
        } else if a < 255 {
            px[0] = ((u32::from(px[0]) * a + 127) / 255) as u8;
            px[1] = ((u32::from(px[1]) * a + 127) / 255) as u8;
            px[2] = ((u32::from(px[2]) * a + 127) / 255) as u8;
        }
    }
}

struct OffscreenSurface {
    left: i32,
    top: i32,
    pixmap: Pixmap,
}

struct EffectGroup {
    parent: Option<usize>,
    sort_key: (usize, usize, usize),
    clip_ids: Vec<String>,
    blend_mode: BlendMode,
    filters: Vec<FilterSpec>,
    members: Vec<bool>,
    exact_leaf_path: Option<Vec<Id>>,
}

struct EffectRenderCtx<'a> {
    project: &'a Project,
    scene: &'a AuthoredScene,
    w: u32,
    h: u32,
    sx: f64,
    sy: f64,
    stage_scale: SkiaTransform,
    viewport: IntRect,
    step: [f64; 2],
    clips: &'a HashMap<&'a str, &'a neoflash_core::RuntimeClip>,
    clip_bounds: &'a HashMap<&'a str, Option<IntRect>>,
    bitmap_cache: &'a mut HashMap<String, Option<Arc<DecodedImage>>>,
    bitmap_brush: &'a mut BitmapBrushCache,
    fonts: &'a mut RasterFonts,
    video_cache: &'a mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &'a HashMap<Vec<String>, ResolvedVideoPlacement>,
}

fn parse_group_sort_key(
    id: &str,
    mask_groups: &[neoflash_core::AuthoredMaskGroup],
    order: usize,
) -> (usize, usize, usize) {
    let parse_len = |rest: &str| {
        serde_json::from_str::<Vec<String>>(rest)
            .map(|v| v.len())
            .unwrap_or(1)
    };
    if let Some(rest) = id.strip_prefix("blend-folder:") {
        (parse_len(rest), 50, order)
    } else if let Some(rest) = id.strip_prefix("blend-layer:") {
        (parse_len(rest), 100, order)
    } else if let Some(rest) = id.strip_prefix("layer-group:") {
        (parse_len(rest), 150, order)
    } else if let Some(rest) = id.strip_prefix("blend-instance:") {
        (parse_len(rest), 200, order)
    } else {
        let mut depth = 1usize;
        let mut cur = mask_groups
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.parent_id.as_deref());
        for _ in 0..=mask_groups.len() {
            let Some(pid) = cur else {
                break;
            };
            depth += 1;
            cur = mask_groups
                .iter()
                .find(|g| g.id == pid)
                .and_then(|g| g.parent_id.as_deref());
        }
        (depth, 150, order)
    }
}

fn draw_in_mask_group(
    draw: &AuthoredDraw,
    target_id: &str,
    mask_groups: &[neoflash_core::AuthoredMaskGroup],
) -> bool {
    let mut cur = draw.mask_group.as_deref();
    for _ in 0..=mask_groups.len() {
        let Some(id) = cur else {
            return false;
        };
        if id == target_id {
            return true;
        }
        cur = mask_groups
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.parent_id.as_deref());
    }
    false
}

fn scale_filter_spec(spec: &FilterSpec, sx: f64, sy: f64) -> FilterSpec {
    if (sx - 1.0).abs() < 1e-6 && (sy - 1.0).abs() < 1e-6 {
        return spec.clone();
    }
    let scale_polar = |distance: f64, angle: f64| {
        let rad = angle.rem_euclid(360.0).to_radians();
        let dx = distance * rad.cos() * sx;
        let dy = distance * rad.sin() * sy;
        let new_dist = dx.hypot(dy);
        let new_angle = if new_dist > 1e-9 {
            dy.atan2(dx).to_degrees().rem_euclid(360.0)
        } else {
            angle
        };
        (new_dist, new_angle)
    };
    let mut out = spec.clone();
    let enabled = out.is_enabled();
    match &mut out {
        FilterSpec::Blur { blur_x, blur_y, .. } | FilterSpec::Glow { blur_x, blur_y, .. } => {
            *blur_x *= sx;
            *blur_y *= sy;
        }
        FilterSpec::DropShadow {
            distance,
            angle,
            blur_x,
            blur_y,
            ..
        }
        | FilterSpec::Bevel {
            distance,
            angle,
            blur_x,
            blur_y,
            ..
        }
        | FilterSpec::GradientGlow {
            distance,
            angle,
            blur_x,
            blur_y,
            ..
        }
        | FilterSpec::GradientBevel {
            distance,
            angle,
            blur_x,
            blur_y,
            ..
        } => {
            let (nd, na) = scale_polar(*distance, *angle);
            *distance = nd;
            *angle = na;
            *blur_x *= sx;
            *blur_y *= sy;
        }
        FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {}
    }
    out.set_enabled(enabled);
    out
}

fn transform_local_box_with_stroke(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    xf: SkiaTransform,
    stroke_w: f64,
) -> Option<tiny_skia::Rect> {
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (cx, cy) in corners {
        let mut pt = SkiaPoint::from_xy(cx, cy);
        xf.map_point(&mut pt);
        if pt.x.is_finite() && pt.y.is_finite() {
            min_x = min_x.min(pt.x);
            min_y = min_y.min(pt.y);
            max_x = max_x.max(pt.x);
            max_y = max_y.max(pt.y);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        return None;
    }
    let pad = if stroke_w > 0.0 {
        let max_s = xf.sx.hypot(xf.ky).max(xf.kx.hypot(xf.sy)).max(0.01);
        (stroke_w as f32) * max_s * 0.6 + 2.0
    } else {
        1.0
    };
    tiny_skia::Rect::from_ltrb(
        min_x - pad,
        min_y - pad,
        (max_x + pad).max(min_x - pad + 1.0),
        (max_y + pad).max(min_y - pad + 1.0),
    )
}

fn draw_stage_px_bounds(
    draw: &AuthoredDraw,
    stage_scale: SkiaTransform,
    viewport: IntRect,
) -> Option<tiny_skia::Rect> {
    let item = &draw.item;
    let xf = stage_scale.pre_concat(affine_to_skia(item.transform));
    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            width,
            height,
            stroke_width,
            ..
        } => {
            if *width <= 0.0 || *height <= 0.0 {
                return None;
            }
            transform_local_box_with_stroke(
                0.0,
                0.0,
                *width as f32,
                *height as f32,
                xf,
                *stroke_width,
            )
        }
        ElementKind::Path {
            commands,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            stroke_width,
            ..
        } => {
            let local = build_skia_path(commands)?.bounds();
            transform_local_box_with_stroke(
                local.left(),
                local.top(),
                local.right(),
                local.bottom(),
                xf,
                *stroke_width,
            )
        }
        ElementKind::Bitmap { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return None;
            }
            transform_local_box_with_stroke(0.0, 0.0, *width as f32, *height as f32, xf, 0.0)
        }
        ElementKind::TextBox { width, height, .. } => transform_local_box_with_stroke(
            0.0,
            0.0,
            width.ceil().max(1.0) as f32,
            height.ceil().max(1.0) as f32,
            xf,
            0.0,
        ),
        ElementKind::Video { width, height } => {
            if *width <= 0.0 || *height <= 0.0 {
                return None;
            }
            transform_local_box_with_stroke(0.0, 0.0, *width as f32, *height as f32, xf, 0.0)
        }
        ElementKind::Text { .. } => Some(viewport.to_rect()),
        ElementKind::Instance { .. } => None,
    }
}

fn render_scene_with_effect_groups(
    root_pixmap: &mut Pixmap,
    ctx: &mut EffectRenderCtx<'_>,
    filter_nodes: &[FilterNodeEntry],
) {
    let n_draws = ctx.scene.draws.len();
    if n_draws == 0 {
        return;
    }

    let mut groups: Vec<EffectGroup> = Vec::new();
    let mut blend_instance_idx: HashMap<String, usize> = HashMap::new();
    for (order, mg) in ctx.scene.mask_groups.iter().enumerate() {
        let members: Vec<bool> = ctx
            .scene
            .draws
            .iter()
            .map(|d| draw_in_mask_group(d, &mg.id, &ctx.scene.mask_groups))
            .collect();
        if !members.iter().any(|&m| m) {
            continue;
        }
        let idx = groups.len();
        if mg.id.starts_with("blend-instance:") {
            blend_instance_idx.insert(mg.id.clone(), idx);
        }
        groups.push(EffectGroup {
            parent: None,
            sort_key: parse_group_sort_key(&mg.id, &ctx.scene.mask_groups, order),
            clip_ids: mg.clip_ids.clone(),
            blend_mode: mg.blend_mode,
            filters: Vec::new(),
            members,
            exact_leaf_path: None,
        });
    }

    let base_order = ctx.scene.mask_groups.len();
    for (offset, (path, filters, outer_clips, blend_mode)) in filter_nodes.iter().enumerate() {
        let scaled: Vec<FilterSpec> = filters
            .iter()
            .map(|f| scale_filter_spec(f, ctx.sx, ctx.sy))
            .collect();
        let inst_id = format!(
            "blend-instance:{}",
            serde_json::to_string(path).unwrap_or_default()
        );
        if let Some(&idx) = blend_instance_idx.get(&inst_id) {
            groups[idx].filters = scaled;
            for cid in outer_clips {
                if !groups[idx].clip_ids.contains(cid) {
                    groups[idx].clip_ids.push(cid.clone());
                }
            }
            continue;
        }
        let members: Vec<bool> = ctx
            .scene
            .draws
            .iter()
            .map(|d| d.item.instance_path.starts_with(path))
            .collect();
        if !members.iter().any(|&m| m) {
            continue;
        }
        groups.push(EffectGroup {
            parent: None,
            sort_key: (path.len(), 200, base_order + offset),
            clip_ids: outer_clips.clone(),
            blend_mode: *blend_mode,
            filters: scaled,
            members,
            exact_leaf_path: Some(path.clone()),
        });
    }

    groups.sort_by_key(|g| g.sort_key);
    for idx in 0..groups.len() {
        groups[idx].parent = (0..idx)
            .rfind(|&p| (0..n_draws).all(|i| !groups[idx].members[i] || groups[p].members[i]));
    }

    let draw_group: Vec<Option<usize>> = (0..n_draws)
        .map(|i| (0..groups.len()).rfind(|&g| groups[g].members[i]))
        .collect();
    let draw_bounds: Vec<Option<tiny_skia::Rect>> = ctx
        .scene
        .draws
        .iter()
        .map(|d| draw_stage_px_bounds(d, ctx.stage_scale, ctx.viewport))
        .collect();

    let mut active = HashSet::new();
    let mut root_surface = OffscreenSurface {
        left: 0,
        top: 0,
        pixmap: Pixmap::new(1, 1).expect("1x1 placeholder"),
    };
    std::mem::swap(&mut root_surface.pixmap, root_pixmap);
    render_group_into(
        None,
        &mut root_surface,
        &[],
        &groups,
        &draw_group,
        &draw_bounds,
        ctx,
        &mut active,
    );
    std::mem::swap(&mut root_surface.pixmap, root_pixmap);
}

#[allow(clippy::too_many_arguments)]
fn compute_group_bounds(
    gid: usize,
    groups: &[EffectGroup],
    draw_bounds: &[Option<tiny_skia::Rect>],
    w: u32,
    h: u32,
) -> Option<(i32, i32, u32, u32)> {
    let mut union: Option<tiny_skia::Rect> = None;
    for (i, &member) in groups[gid].members.iter().enumerate() {
        if member && let Some(b) = draw_bounds[i] {
            union = Some(match union {
                Some(acc) => acc.join(&b).unwrap_or(acc),
                None => b,
            });
        }
    }
    let rect = union?;
    let mut pad_l = 2i32;
    let mut pad_t = 2i32;
    let mut pad_r = 2i32;
    let mut pad_b = 2i32;
    for (idx, g) in groups.iter().enumerate() {
        let inside_subtree = idx == gid || {
            let mut cur = g.parent;
            let mut found = false;
            for _ in 0..=groups.len() {
                if cur == Some(gid) {
                    found = true;
                    break;
                }
                let Some(p) = cur else {
                    break;
                };
                cur = groups[p].parent;
            }
            found
        };
        if inside_subtree
            && !g.filters.is_empty()
            && let Ok(pad) = filter_padding(&g.filters)
        {
            pad_l = pad_l.saturating_add(pad.left as i32 + 2);
            pad_t = pad_t.saturating_add(pad.top as i32 + 2);
            pad_r = pad_r.saturating_add(pad.right as i32 + 2);
            pad_b = pad_b.saturating_add(pad.bottom as i32 + 2);
        }
    }
    let min_x = (rect.left().floor() as i32 - pad_l).max(-pad_r.max(64));
    let min_y = (rect.top().floor() as i32 - pad_t).max(-pad_b.max(64));
    let max_x = (rect.right().ceil() as i32 + pad_r).min(w as i32 + pad_l.max(64));
    let max_y = (rect.bottom().ceil() as i32 + pad_b).min(h as i32 + pad_t.max(64));
    if max_x <= 0
        || max_y <= 0
        || min_x >= w as i32
        || min_y >= h as i32
        || max_x <= min_x
        || max_y <= min_y
    {
        return None;
    }
    let gw = (max_x - min_x) as u32;
    let gh = (max_y - min_y) as u32;
    if gw > 4096 || gh > 4096 {
        let cl_x0 = min_x.max(0);
        let cl_y0 = min_y.max(0);
        let cl_x1 = max_x.min(w as i32);
        let cl_y1 = max_y.min(h as i32);
        if cl_x1 <= cl_x0 || cl_y1 <= cl_y0 {
            return None;
        }
        return Some((cl_x0, cl_y0, (cl_x1 - cl_x0) as u32, (cl_y1 - cl_y0) as u32));
    }
    Some((min_x, min_y, gw, gh))
}

#[allow(clippy::too_many_arguments)]
fn render_group_into(
    gid: Option<usize>,
    surface: &mut OffscreenSurface,
    deferred_clips: &[String],
    groups: &[EffectGroup],
    draw_group: &[Option<usize>],
    draw_bounds: &[Option<tiny_skia::Rect>],
    ctx: &mut EffectRenderCtx<'_>,
    active: &mut HashSet<usize>,
) {
    let mut emitted_children = HashSet::new();
    let stage_bg = {
        let bg = ctx.project.background;
        [bg[0], bg[1], bg[2], 255]
    };
    let use_root_backdrop = gid.is_none();

    for (idx, draw) in ctx.scene.draws.iter().enumerate() {
        let belongs = match gid {
            None => true,
            Some(g) => groups[g].members[idx],
        };
        if !belongs {
            continue;
        }
        if draw_group[idx] == gid {
            let extra_clips: Vec<String> = draw
                .clip_ids
                .iter()
                .filter(|c| !deferred_clips.contains(c))
                .cloned()
                .collect();
            let effective_blend = if gid.is_some_and(|g| {
                groups[g]
                    .exact_leaf_path
                    .as_ref()
                    .is_some_and(|p| p == &draw.item.instance_path)
            }) {
                BlendMode::Normal
            } else {
                draw.blend_mode
            };

            if effective_blend.is_normal() {
                let xf = ctx
                    .stage_scale
                    .post_translate(-(surface.left as f32), -(surface.top as f32));
                let mask_opt = if extra_clips.is_empty() {
                    None
                } else {
                    if let Some(sr) = IntRect::from_xywh(
                        surface.left,
                        surface.top,
                        surface.pixmap.width(),
                        surface.pixmap.height(),
                    ) {
                        let clip_area = extra_clips.iter().try_fold(sr, |area, id| {
                            area.intersect(&ctx.clip_bounds.get(id.as_str()).copied().flatten()?)
                        });
                        if clip_area.is_none() {
                            continue;
                        }
                    }
                    let Some(m) = build_offset_clip_mask(
                        ctx.project,
                        surface.left,
                        surface.top,
                        surface.pixmap.width(),
                        surface.pixmap.height(),
                        ctx.step,
                        xf,
                        &extra_clips,
                        ctx.clips,
                    ) else {
                        continue;
                    };
                    Some(m)
                };
                draw_authored_item(
                    &mut surface.pixmap,
                    ctx.project,
                    draw,
                    xf,
                    mask_opt.as_ref(),
                    ctx.bitmap_cache,
                    ctx.bitmap_brush,
                    ctx.fonts,
                    ctx.video_cache,
                    ctx.video_placements,
                );
            } else {
                let Some(b) = draw_bounds[idx] else {
                    continue;
                };
                let x0 = (b.left().floor() as i32 - 2).max(0);
                let y0 = (b.top().floor() as i32 - 2).max(0);
                let x1 = (b.right().ceil() as i32 + 2).min(ctx.w as i32);
                let y1 = (b.bottom().ceil() as i32 + 2).min(ctx.h as i32);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let pw = (x1 - x0) as u32;
                let ph = (y1 - y0) as u32;
                let Some(mut part_px) = Pixmap::new(pw, ph) else {
                    continue;
                };
                let xf = ctx.stage_scale.post_translate(-(x0 as f32), -(y0 as f32));
                let mask_opt = if extra_clips.is_empty() {
                    None
                } else {
                    let Some(m) = build_offset_clip_mask(
                        ctx.project,
                        x0,
                        y0,
                        pw,
                        ph,
                        ctx.step,
                        xf,
                        &extra_clips,
                        ctx.clips,
                    ) else {
                        continue;
                    };
                    Some(m)
                };
                draw_authored_item(
                    &mut part_px,
                    ctx.project,
                    draw,
                    xf,
                    mask_opt.as_ref(),
                    ctx.bitmap_cache,
                    ctx.bitmap_brush,
                    ctx.fonts,
                    ctx.video_cache,
                    ctx.video_placements,
                );
                let part = OffscreenSurface {
                    left: x0,
                    top: y0,
                    pixmap: part_px,
                };
                composite_surface_blended(
                    surface,
                    &part,
                    effective_blend,
                    use_root_backdrop.then_some(stage_bg),
                );
            }
        } else if let Some(mut child) = draw_group[idx] {
            for _ in 0..=groups.len() {
                if groups[child].parent == gid {
                    break;
                }
                let Some(p) = groups[child].parent else {
                    break;
                };
                child = p;
            }
            if groups[child].parent != gid || !emitted_children.insert(child) {
                continue;
            }
            if !active.insert(child) {
                continue;
            }
            if let Some((gx, gy, gw, gh)) =
                compute_group_bounds(child, groups, draw_bounds, ctx.w, ctx.h)
                && let Some(child_px) = Pixmap::new(gw, gh)
            {
                let mut child_surface = OffscreenSurface {
                    left: gx,
                    top: gy,
                    pixmap: child_px,
                };
                let mut child_deferred = deferred_clips.to_vec();
                for cid in &groups[child].clip_ids {
                    if !child_deferred.contains(cid) {
                        child_deferred.push(cid.clone());
                    }
                }
                render_group_into(
                    Some(child),
                    &mut child_surface,
                    &child_deferred,
                    groups,
                    draw_group,
                    draw_bounds,
                    ctx,
                    active,
                );
                if !groups[child].filters.is_empty() {
                    apply_filters_to_surface(&mut child_surface, &groups[child].filters);
                }
                let own_clips: Vec<String> = groups[child]
                    .clip_ids
                    .iter()
                    .filter(|c| !deferred_clips.contains(c))
                    .cloned()
                    .collect();
                if !own_clips.is_empty() {
                    let xf = ctx
                        .stage_scale
                        .post_translate(-(child_surface.left as f32), -(child_surface.top as f32));
                    if let Some(mask) = build_offset_clip_mask(
                        ctx.project,
                        child_surface.left,
                        child_surface.top,
                        child_surface.pixmap.width(),
                        child_surface.pixmap.height(),
                        ctx.step,
                        xf,
                        &own_clips,
                        ctx.clips,
                    ) {
                        apply_skia_mask_in_place(&mut child_surface.pixmap, &mask);
                    } else {
                        child_surface.pixmap.data_mut().fill(0);
                    }
                }
                composite_surface_blended(
                    surface,
                    &child_surface,
                    groups[child].blend_mode,
                    use_root_backdrop.then_some(stage_bg),
                );
            }
            active.remove(&child);
        }
    }
}

fn apply_filters_to_surface(surface: &mut OffscreenSurface, filters: &[FilterSpec]) {
    let w = surface.pixmap.width();
    let h = surface.pixmap.height();
    let pixels = (w as usize).saturating_mul(h as usize);
    if pixels == 0 {
        return;
    }
    if pixels <= neoflash_core::MAX_FILTER_PIXELS {
        let mut straight = surface.pixmap.data().to_vec();
        unpremultiply_rgba_in_place(&mut straight);
        if let Ok(mut filtered) = filter_rgba(w, h, &straight, filters) {
            premultiply_rgba_in_place(&mut filtered);
            surface.pixmap.data_mut().copy_from_slice(&filtered);
        }
        return;
    }
    let ratio = ((neoflash_core::MAX_FILTER_PIXELS as f64) / (pixels as f64)).sqrt();
    let sw = ((w as f64 * ratio).floor() as u32).max(1);
    let sh = ((h as f64 * ratio).floor() as u32).max(1);
    let Some(mut scaled_px) = Pixmap::new(sw, sh) else {
        return;
    };
    scaled_px.draw_pixmap(
        0,
        0,
        surface.pixmap.as_ref(),
        &PixmapPaint {
            opacity: 1.0,
            blend_mode: tiny_skia::BlendMode::SourceOver,
            quality: FilterQuality::Bilinear,
        },
        SkiaTransform::from_scale(sw as f32 / w as f32, sh as f32 / h as f32),
        None,
    );
    let scaled_filters: Vec<FilterSpec> = filters
        .iter()
        .map(|f| {
            scale_filter_spec(
                f,
                f64::from(sw) / f64::from(w),
                f64::from(sh) / f64::from(h),
            )
        })
        .collect();
    let mut straight = scaled_px.data().to_vec();
    unpremultiply_rgba_in_place(&mut straight);
    if let Ok(mut filtered) = filter_rgba(sw, sh, &straight, &scaled_filters) {
        premultiply_rgba_in_place(&mut filtered);
        scaled_px.data_mut().copy_from_slice(&filtered);
        surface.pixmap.data_mut().fill(0);
        surface.pixmap.draw_pixmap(
            0,
            0,
            scaled_px.as_ref(),
            &PixmapPaint {
                opacity: 1.0,
                blend_mode: tiny_skia::BlendMode::SourceOver,
                quality: FilterQuality::Bilinear,
            },
            SkiaTransform::from_scale(w as f32 / sw as f32, h as f32 / sh as f32),
            None,
        );
    }
}

fn apply_skia_mask_in_place(pixmap: &mut Pixmap, mask: &SkiaMask) {
    for (px, &m) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(mask.data().iter())
    {
        if m == 0 {
            *px = [0; 4];
        } else if m < 255 {
            let mu = u32::from(m);
            px[0] = ((u32::from(px[0]) * mu + 127) / 255) as u8;
            px[1] = ((u32::from(px[1]) * mu + 127) / 255) as u8;
            px[2] = ((u32::from(px[2]) * mu + 127) / 255) as u8;
            px[3] = ((u32::from(px[3]) * mu + 127) / 255) as u8;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_offset_clip_mask(
    project: &Project,
    left: i32,
    top: i32,
    out_w: u32,
    out_h: u32,
    step: [f64; 2],
    transform: SkiaTransform,
    ids: &[String],
    clips: &HashMap<&str, &neoflash_core::RuntimeClip>,
) -> Option<SkiaMask> {
    if let Some(rect) = IntRect::from_xywh(left, top, out_w, out_h) {
        return build_clip_mask(project, rect, step, transform, ids, clips);
    }
    let simple = ids.iter().all(|id| {
        clips.get(id.as_str()).is_some_and(|c| {
            c.alpha_components.is_none()
                && !c.invert
                && c.components.iter().all(|part| part.clip_ids.is_empty())
        })
    });
    if !simple {
        let mut subset = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = ids.iter().map(String::as_str).collect();
        while let Some(id) = pending.pop() {
            if seen.insert(id) {
                let clip = *clips.get(id)?;
                if let Some(alpha) = &clip.alpha_components {
                    pending.extend(
                        alpha
                            .iter()
                            .flat_map(|c| c.clip_ids.iter().map(String::as_str)),
                    );
                } else {
                    pending.extend(
                        clip.components
                            .iter()
                            .flat_map(|c| c.clip_ids.iter().map(String::as_str)),
                    );
                }
                subset.push(clip.clone());
            }
        }
        if let Ok(coverage) = MaskCoverage::new(project, &subset) {
            let origin = [f64::from(left) * step[0], f64::from(top) * step[1]];
            if let Ok(bytes) = coverage.raster(ids, origin, step, out_w as usize, out_h as usize) {
                return SkiaMask::from_vec(bytes, IntSize::from_wh(out_w, out_h)?);
            }
        }
    }
    let mut combined: Option<SkiaMask> = None;
    for id in ids {
        let clip = clips.get(id.as_str())?;
        let mut mask = SkiaMask::new(out_w, out_h)?;
        for comp in &clip.components {
            if let Some(path) = build_skia_path(&comp.commands) {
                let xf = transform.pre_concat(affine_to_skia(comp.matrix));
                mask.fill_path(&path, to_skia_fill_rule(comp.fill_rule), true, xf);
            }
        }
        if clip.invert {
            mask.invert();
        }
        if let Some(existing) = &mut combined {
            for (dst, src) in existing.data_mut().iter_mut().zip(mask.data().iter()) {
                *dst = ((u16::from(*dst) * u16::from(*src) + 127) / 255) as u8;
            }
        } else {
            combined = Some(mask);
        }
    }
    combined
}

fn composite_surface_blended(
    target: &mut OffscreenSurface,
    source: &OffscreenSurface,
    mode: BlendMode,
    stage_bg: Option<[u8; 4]>,
) {
    let tw = target.pixmap.width() as i32;
    let th = target.pixmap.height() as i32;
    let sw = source.pixmap.width() as i32;
    let sh = source.pixmap.height() as i32;
    match mode {
        BlendMode::Normal | BlendMode::Layer => {
            target.pixmap.draw_pixmap(
                source.left - target.left,
                source.top - target.top,
                source.pixmap.as_ref(),
                &PixmapPaint::default(),
                SkiaTransform::identity(),
                None,
            );
        }
        BlendMode::Alpha => {
            let dst_pixels = target.pixmap.data_mut().as_chunks_mut::<4>().0;
            let src_pixels = source.pixmap.data().as_chunks::<4>().0;
            for dy in 0..th {
                for dx in 0..tw {
                    let sx = (target.left + dx) - source.left;
                    let sy = (target.top + dy) - source.top;
                    let sa = if sx >= 0 && sy >= 0 && sx < sw && sy < sh {
                        u32::from(src_pixels[(sy * sw + sx) as usize][3])
                    } else {
                        0
                    };
                    let tp = &mut dst_pixels[(dy * tw + dx) as usize];
                    tp[0] = ((u32::from(tp[0]) * sa + 127) / 255) as u8;
                    tp[1] = ((u32::from(tp[1]) * sa + 127) / 255) as u8;
                    tp[2] = ((u32::from(tp[2]) * sa + 127) / 255) as u8;
                    tp[3] = ((u32::from(tp[3]) * sa + 127) / 255) as u8;
                }
            }
        }
        BlendMode::Erase => {
            let dst_pixels = target.pixmap.data_mut().as_chunks_mut::<4>().0;
            let src_pixels = source.pixmap.data().as_chunks::<4>().0;
            for sy in 0..sh {
                let dy = (source.top + sy) - target.top;
                if dy < 0 || dy >= th {
                    continue;
                }
                for sx in 0..sw {
                    let sa = u32::from(src_pixels[(sy * sw + sx) as usize][3]);
                    if sa == 0 {
                        continue;
                    }
                    let dx = (source.left + sx) - target.left;
                    if dx >= 0 && dx < tw {
                        let inv = 255 - sa;
                        let tp = &mut dst_pixels[(dy * tw + dx) as usize];
                        tp[0] = ((u32::from(tp[0]) * inv + 127) / 255) as u8;
                        tp[1] = ((u32::from(tp[1]) * inv + 127) / 255) as u8;
                        tp[2] = ((u32::from(tp[2]) * inv + 127) / 255) as u8;
                        tp[3] = ((u32::from(tp[3]) * inv + 127) / 255) as u8;
                    }
                }
            }
        }
        _ => {
            let dst_pixels = target.pixmap.data_mut().as_chunks_mut::<4>().0;
            let src_pixels = source.pixmap.data().as_chunks::<4>().0;
            for sy in 0..sh {
                let gy = source.top + sy;
                let dy = gy - target.top;
                if dy < 0 || dy >= th {
                    continue;
                }
                for sx in 0..sw {
                    let sp = src_pixels[(sy * sw + sx) as usize];
                    if sp[3] == 0 {
                        continue;
                    }
                    let gx = source.left + sx;
                    let dx = gx - target.left;
                    if dx >= 0 && dx < tw {
                        let tp = &mut dst_pixels[(dy * tw + dx) as usize];
                        *tp = blend_premul_pixel(sp, *tp, mode, stage_bg);
                    }
                }
            }
        }
    }
}

fn blend_premul_pixel(
    source: [u8; 4],
    target: [u8; 4],
    mode: BlendMode,
    stage_bg: Option<[u8; 4]>,
) -> [u8; 4] {
    let sa = source[3];
    if sa == 0 {
        return target;
    }
    let sa_f = f64::from(sa) / 255.0;
    let ta_f = f64::from(target[3]) / 255.0;
    let cs: [f64; 3] =
        std::array::from_fn(|i| (f64::from(source[i]) / 255.0 / sa_f).clamp(0.0, 1.0));
    let cb: [f64; 3] = if let Some(bg) = stage_bg {
        let inv = 255 - u32::from(target[3]);
        std::array::from_fn(|i| {
            f64::from((u32::from(target[i]) + (u32::from(bg[i]) * inv + 127) / 255).min(255))
                / 255.0
        })
    } else if ta_f > 0.0 {
        std::array::from_fn(|i| (f64::from(target[i]) / 255.0 / ta_f).clamp(0.0, 1.0))
    } else {
        [0.0; 3]
    };
    let out_a = sa_f + ta_f * (1.0 - sa_f);
    let out_a_u8 = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    let mut rgb = [0u8; 3];
    for c in 0..3 {
        let mixed = match mode {
            BlendMode::Darken => cb[c].min(cs[c]),
            BlendMode::Multiply => cb[c] * cs[c],
            BlendMode::Lighten => cb[c].max(cs[c]),
            BlendMode::Screen => cb[c] + cs[c] - cb[c] * cs[c],
            BlendMode::Add => (cb[c] + cs[c]).min(1.0),
            BlendMode::Subtract => (cb[c] - cs[c]).max(0.0),
            BlendMode::Difference => (cb[c] - cs[c]).abs(),
            BlendMode::Invert => 1.0 - cb[c],
            BlendMode::Overlay => {
                if cb[c] <= 0.5 {
                    2.0 * cb[c] * cs[c]
                } else {
                    1.0 - 2.0 * (1.0 - cb[c]) * (1.0 - cs[c])
                }
            }
            BlendMode::Hardlight => {
                if cs[c] <= 0.5 {
                    2.0 * cb[c] * cs[c]
                } else {
                    1.0 - 2.0 * (1.0 - cb[c]) * (1.0 - cs[c])
                }
            }
            BlendMode::Normal | BlendMode::Layer | BlendMode::Alpha | BlendMode::Erase => cs[c],
        };
        let premul = if stage_bg.is_some() {
            sa_f * mixed + (1.0 - sa_f) * (f64::from(target[c]) / 255.0)
        } else {
            sa_f * (1.0 - ta_f) * cs[c] + ta_f * (1.0 - sa_f) * cb[c] + sa_f * ta_f * mixed
        };
        rgb[c] = (premul * 255.0).round().clamp(0.0, f64::from(out_a_u8)) as u8;
    }
    [rgb[0], rgb[1], rgb[2], out_a_u8]
}

fn clip_pixel_bounds(
    clip: &neoflash_core::RuntimeClip,
    scale: SkiaTransform,
    viewport: IntRect,
) -> Option<IntRect> {
    if clip.invert {
        return Some(viewport);
    }
    let bounds = |commands: &[PathCommand], matrix| {
        build_skia_path(commands)?
            .bounds()
            .transform(scale.pre_concat(affine_to_skia(matrix)))
    };
    let join = |a: tiny_skia::Rect, b| a.join(&b).unwrap_or(viewport.to_rect());
    let union = if let Some(alpha) = &clip.alpha_components {
        alpha
            .iter()
            .filter_map(|c| bounds(&c.commands, c.matrix))
            .reduce(join)
    } else {
        clip.components
            .iter()
            .filter_map(|c| bounds(&c.commands, c.matrix))
            .reduce(join)
    }?;
    union
        .outset(1., 1.)?
        .intersect(&viewport.to_rect())?
        .round_out()?
        .intersect(&viewport)
}

fn build_clip_mask(
    project: &Project,
    rect: IntRect,
    step: [f64; 2],
    transform: SkiaTransform,
    ids: &[String],
    clips: &HashMap<&str, &neoflash_core::RuntimeClip>,
) -> Option<SkiaMask> {
    let out_w = rect.width();
    let out_h = rect.height();
    // Alpha and nested masks use the shared coverage sampler. Only their reachable
    // components count against its work budget, irrespective of other artboards.
    let simple = ids.iter().all(|id| {
        clips.get(id.as_str()).is_some_and(|c| {
            c.alpha_components.is_none()
                && !c.invert
                && c.components.iter().all(|part| part.clip_ids.is_empty())
        })
    });
    if !simple {
        let mut subset = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = ids.iter().map(String::as_str).collect();
        while let Some(id) = pending.pop() {
            if seen.insert(id) {
                let clip = *clips.get(id)?;
                if let Some(alpha) = &clip.alpha_components {
                    pending.extend(
                        alpha
                            .iter()
                            .flat_map(|c| c.clip_ids.iter().map(String::as_str)),
                    );
                } else {
                    pending.extend(
                        clip.components
                            .iter()
                            .flat_map(|c| c.clip_ids.iter().map(String::as_str)),
                    );
                }
                subset.push(clip.clone());
            }
        }
        if let Ok(coverage) = MaskCoverage::new(project, &subset) {
            let origin = [f64::from(rect.x()) * step[0], f64::from(rect.y()) * step[1]];
            if let Ok(bytes) = coverage.raster(ids, origin, step, out_w as usize, out_h as usize) {
                return SkiaMask::from_vec(bytes, IntSize::from_wh(out_w, out_h)?);
            }
        }
    }
    let mut combined: Option<SkiaMask> = None;
    for id in ids {
        let clip = clips.get(id.as_str())?;
        let mut mask = SkiaMask::new(out_w, out_h)?;
        for comp in &clip.components {
            if let Some(path) = build_skia_path(&comp.commands) {
                let xf = transform.pre_concat(affine_to_skia(comp.matrix));
                mask.fill_path(&path, to_skia_fill_rule(comp.fill_rule), true, xf);
            }
        }
        if clip.invert {
            mask.invert();
        }
        if let Some(existing) = &mut combined {
            for (dst, src) in existing.data_mut().iter_mut().zip(mask.data().iter()) {
                *dst = ((u16::from(*dst) * u16::from(*src) + 127) / 255) as u8;
            }
        } else {
            combined = Some(mask);
        }
    }
    combined
}

fn affine_to_skia(m: [f64; 6]) -> SkiaTransform {
    let f = |v: f64, def: f32| if v.is_finite() { v as f32 } else { def };
    SkiaTransform::from_row(
        f(m[0], 1.0),
        f(m[1], 0.0),
        f(m[2], 0.0),
        f(m[3], 1.0),
        f(m[4], 0.0),
        f(m[5], 0.0),
    )
}

fn to_skia_fill_rule(rule: FillRule) -> SkiaFillRule {
    match rule {
        FillRule::NonZero => SkiaFillRule::Winding,
        FillRule::EvenOdd => SkiaFillRule::EvenOdd,
    }
}

fn build_skia_path(commands: &[PathCommand]) -> Option<SkiaPath> {
    let mut pb = PathBuilder::new();
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if x.is_finite() && y.is_finite() {
                    pb.move_to(x as f32, y as f32);
                }
            }
            PathCommand::LineTo { x, y } => {
                if x.is_finite() && y.is_finite() {
                    pb.line_to(x as f32, y as f32);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if cx.is_finite() && cy.is_finite() && x.is_finite() && y.is_finite() {
                    pb.quad_to(cx as f32, cy as f32, x as f32, y as f32);
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
                if cx1.is_finite()
                    && cy1.is_finite()
                    && cx2.is_finite()
                    && cy2.is_finite()
                    && x.is_finite()
                    && y.is_finite()
                {
                    pb.cubic_to(
                        cx1 as f32, cy1 as f32, cx2 as f32, cy2 as f32, x as f32, y as f32,
                    );
                }
            }
            PathCommand::Close => pb.close(),
        }
    }
    pb.finish()
}

fn configure_paint_shader<'a>(
    solid: Color,
    gradient: Option<&GradientPaint>,
    ct: ColorTransform,
) -> SkiaPaint<'a> {
    let mut paint = SkiaPaint {
        anti_alias: true,
        ..SkiaPaint::default()
    };
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        if grad.stops.len() == 1 {
            let c = ct.apply(grad.stops[0].color);
            paint.set_color_rgba8(c[0], c[1], c[2], c[3]);
            return paint;
        }
        let mut stops = Vec::with_capacity(grad.stops.len());
        let mut push_stop = |ratio: u8, color: Color| {
            let c = ct.apply(color);
            stops.push(SkiaGradientStop::new(
                f32::from(ratio) / 255.0,
                SkiaColor::from_rgba8(c[0], c[1], c[2], c[3]),
            ));
        };
        for (i, stop) in grad.stops.iter().enumerate() {
            if grad.linear_rgb && i > 0 {
                // The raster shader interpolates sRGB bytes. Sample the shared linear-light ramp
                // between authored stops, keeping hard stops and ordinary layer compositing intact.
                for ratio in u16::from(grad.stops[i - 1].ratio) + 1..u16::from(stop.ratio) {
                    push_stop(ratio as u8, grad.sample_at_unit(f64::from(ratio) / 255.0));
                }
            }
            push_stop(stop.ratio, stop.color);
        }
        let spread = match grad.spread {
            neoflash_core::GradientSpread::Extend => SpreadMode::Pad,
            neoflash_core::GradientSpread::Reflect => SpreadMode::Reflect,
            neoflash_core::GradientSpread::Repeat => SpreadMode::Repeat,
        };
        let grad_xf = affine_to_skia(grad.matrix);
        let shader = match grad.kind {
            GradientKind::Linear => LinearGradient::new(
                SkiaPoint::from_xy(-819.2, 0.0),
                SkiaPoint::from_xy(819.2, 0.0),
                stops,
                spread,
                grad_xf,
            ),
            GradientKind::Radial => {
                let focal = if grad.focal_point_ratio.is_finite() {
                    grad.focal_point_ratio.clamp(-0.98, 0.98)
                } else {
                    0.0
                };
                let fx = (focal * 819.2) as f32;
                RadialGradient::new(
                    SkiaPoint::from_xy(fx, 0.0),
                    0.0,
                    SkiaPoint::from_xy(0.0, 0.0),
                    819.2,
                    stops,
                    spread,
                    grad_xf,
                )
            }
        };
        if let Some(shader) = shader {
            paint.shader = shader;
            return paint;
        }
    }
    let c = ct.apply(solid);
    paint.set_color_rgba8(c[0], c[1], c[2], c[3]);
    paint
}

fn decoded_bitmap(
    project: &Project,
    cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    resource_id: &str,
) -> Option<Arc<DecodedImage>> {
    cache
        .entry(resource_id.to_owned())
        .or_insert_with(|| {
            project
                .resources
                .iter()
                .find(|resource| resource.id == resource_id)
                .and_then(|resource| resource.decode_image().ok())
        })
        .clone()
}

#[derive(Default)]
struct BitmapBrushCache {
    // Packed texels are shared by repeated image placements in the same frame.
    entries: VecDeque<(String, ColorTransform, Pixmap)>,
    bytes: usize,
}

impl BitmapBrushCache {
    fn paint(
        &mut self,
        project: &Project,
        cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
        fill: &AuthoredBitmapFill,
        ct: ColorTransform,
    ) -> Option<SkiaPaint<'_>> {
        let transform = affine_to_skia(fill.matrix);
        if neoflash_core::invert_affine_matrix(fill.matrix).is_none() {
            let image = decoded_bitmap(project, cache, &fill.resource_id)?;
            let color = ct.apply(*image.rgba.as_chunks::<4>().0.first()?);
            let mut paint = SkiaPaint::default();
            paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
            return Some(paint);
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(id, color, _)| id == &fill.resource_id && *color == ct)
        {
            self.entries.swap(0, index);
        } else {
            let image = decoded_bitmap(project, cache, &fill.resource_id)?;
            let pixels = bitmap_pixmap(&image, ct)?;
            const CACHE_BYTES: usize = 160 * 1024 * 1024;
            while self.bytes + pixels.data().len() > CACHE_BYTES {
                let Some((_, _, old)) = self.entries.pop_back() else {
                    break;
                };
                self.bytes -= old.data().len();
            }
            self.bytes += pixels.data().len();
            self.entries
                .push_front((fill.resource_id.clone(), ct, pixels));
        }
        let (_, _, pixels) = self.entries.front()?;
        Some(SkiaPaint {
            shader: tiny_skia::Pattern::new(
                pixels.as_ref(),
                if fill.repeat {
                    SpreadMode::Repeat
                } else {
                    SpreadMode::Pad
                },
                bitmap_quality(fill.smoothing),
                1.0,
                transform,
            ),
            ..SkiaPaint::default()
        })
    }
}

/// Bake just the tiled or edge-extended brush rectangle for interchange footage.
pub(crate) fn render_local_brush(
    project: &Project,
    fill: &AuthoredBitmapFill,
    bounds: neoflash_core::PathBounds,
) -> Result<(Vec<u8>, u32, u32), ExportError> {
    let error =
        || ExportError::Options(format!("Cannot render bitmap fill '{}'", fill.resource_id));
    let limit = f64::from(neoflash_core::MAX_BITMAP_DIMENSION);
    let scale = (limit / bounds.width().max(bounds.height())).min(1.);
    let width = (bounds.width() * scale).ceil().max(1.) as u32;
    let height = (bounds.height() * scale).ceil().max(1.) as u32;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(error)?;
    let mut cache = BitmapBrushCache::default();
    let mut images = HashMap::new();
    let paint = cache
        .paint(project, &mut images, fill, ColorTransform::default())
        .ok_or_else(error)?;
    let sx = f64::from(width) / bounds.width();
    let sy = f64::from(height) / bounds.height();
    let transform = affine_to_skia([sx, 0., 0., sy, -bounds.x_min * sx, -bounds.y_min * sy]);
    let rect = tiny_skia::Rect::from_xywh(
        bounds.x_min as f32,
        bounds.y_min as f32,
        bounds.width() as f32,
        bounds.height() as f32,
    )
    .ok_or_else(error)?;
    pixmap.fill_rect(rect, &paint, transform, None);
    Ok((
        pixmap
            .encode_png()
            .map_err(|e| ExportError::Options(e.to_string()))?,
        width,
        height,
    ))
}

#[allow(clippy::too_many_arguments)]
fn draw_authored_item(
    pixmap: &mut Pixmap,
    project: &Project,
    draw: &AuthoredDraw,
    stage_scale: SkiaTransform,
    mask: Option<&SkiaMask>,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    bitmap_brush: &mut BitmapBrushCache,
    fonts: &mut RasterFonts,
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) {
    let item: &DrawItem = &draw.item;
    // The evaluated transform includes both placement and ancestor opacity.
    let ct = item.color_transform;
    let xf = stage_scale.pre_concat(affine_to_skia(item.transform));

    let bitmap_paint = item
        .bitmap_fill
        .as_ref()
        .and_then(|fill| bitmap_brush.paint(project, bitmap_cache, fill, ct));

    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let commands = rectangle_path_commands(*width, *height, Default::default());
            draw_vector_commands(
                pixmap,
                &commands,
                FillRule::NonZero,
                (*fill, None),
                (*stroke, None, *stroke_width),
                bitmap_paint.as_ref(),
                ct,
                xf,
                mask,
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let commands = oval_path_commands(*width, *height, Default::default());
            draw_vector_commands(
                pixmap,
                &commands,
                FillRule::NonZero,
                (*fill, None),
                (*stroke, None, *stroke_width),
                bitmap_paint.as_ref(),
                ct,
                xf,
                mask,
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            draw_vector_commands(
                pixmap,
                commands,
                *fill_rule,
                (*fill, None),
                (*stroke, None, *stroke_width),
                bitmap_paint.as_ref(),
                ct,
                xf,
                mask,
            );
        }
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        } => {
            draw_vector_commands(
                pixmap,
                commands,
                *fill_rule,
                (*fill, fill_gradient.as_ref()),
                (*stroke, stroke_gradient.as_ref(), *stroke_width),
                bitmap_paint.as_ref(),
                ct,
                xf,
                mask,
            );
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            smoothing,
        } => {
            if *width == 0 || *height == 0 {
                return;
            }
            if let Some(img) = decoded_bitmap(project, bitmap_cache, resource_id) {
                draw_bitmap(pixmap, &img, *width, *height, *smoothing, ct, xf, mask);
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let c = ct.apply(*color);
            draw_plain_text(
                pixmap,
                text,
                &fonts.get(project, font_family, false, false),
                *font_size as f32,
                0.,
                c,
                xf,
                mask,
            );
        }
        ElementKind::TextBox { .. } => {
            if let Some(field) = neoflash_core::TextFieldState::from_kind(&item.kind) {
                draw_text_box(pixmap, project, &field, fonts, ct, xf, mask);
            }
        }
        ElementKind::Video { width, height } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let target_w = width.round().max(1.0) as u32;
            let target_h = height.round().max(1.0) as u32;
            if let Some(placement) = video_placements.get(&item.instance_path)
                && let Some(stream) = video_cache
                    .get(&placement_video_cache_key(placement))
                    .or_else(|| video_cache.get(&placement.resource))
                    .and_then(|s| s.as_ref())
                && let Some((_, frame)) = stream.frame_at_placement(placement)
            {
                draw_bitmap(pixmap, &frame.image, target_w, target_h, true, ct, xf, mask);
            }
        }
        ElementKind::Instance { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_vector_commands(
    pixmap: &mut Pixmap,
    commands: &[PathCommand],
    fill_rule: FillRule,
    fill: (Color, Option<&GradientPaint>),
    stroke: (Color, Option<&GradientPaint>, f64),
    bitmap_paint: Option<&SkiaPaint<'_>>,
    ct: ColorTransform,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(path) = build_skia_path(commands) else {
        return;
    };
    let fill_visible = fill.1.is_some() || ct.apply(fill.0)[3] > 0;
    let (stroke_color, stroke_grad, stroke_width) = stroke;
    let stroke_uses_bitmap = !fill_visible && stroke_width > 0.0 && bitmap_paint.is_some();
    if let Some(paint) = bitmap_paint.filter(|_| !stroke_uses_bitmap) {
        pixmap.fill_path(&path, paint, to_skia_fill_rule(fill_rule), xf, mask);
    } else if fill_visible {
        let paint = configure_paint_shader(fill.0, fill.1, ct);
        pixmap.fill_path(&path, &paint, to_skia_fill_rule(fill_rule), xf, mask);
    }
    let stroke_visible = stroke_width > 0.0
        && (stroke_uses_bitmap || stroke_grad.is_some() || ct.apply(stroke_color)[3] > 0);
    if stroke_visible {
        let skia_stroke = SkiaStroke {
            width: stroke_width as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..SkiaStroke::default()
        };
        if let Some(paint) = bitmap_paint.filter(|_| stroke_uses_bitmap) {
            pixmap.stroke_path(&path, paint, &skia_stroke, xf, mask);
        } else {
            let paint = configure_paint_shader(stroke_color, stroke_grad, ct);
            pixmap.stroke_path(&path, &paint, &skia_stroke, xf, mask);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bitmap(
    pixmap: &mut Pixmap,
    img: &DecodedImage,
    target_w: u32,
    target_h: u32,
    smoothing: bool,
    ct: ColorTransform,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(src_pixmap) = bitmap_pixmap(img, ct) else {
        return;
    };
    let scale_x = target_w as f32 / img.width as f32;
    let scale_y = target_h as f32 / img.height as f32;
    let img_xf = xf.pre_concat(SkiaTransform::from_scale(scale_x, scale_y));
    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: bitmap_quality(smoothing),
    };
    pixmap.draw_pixmap(0, 0, src_pixmap.as_ref(), &paint, img_xf, mask);
}

fn bitmap_quality(smoothing: bool) -> FilterQuality {
    if smoothing {
        FilterQuality::Bilinear
    } else {
        FilterQuality::Nearest
    }
}

fn bitmap_pixmap(img: &DecodedImage, ct: ColorTransform) -> Option<Pixmap> {
    let mut src_pixmap = Pixmap::new(img.width, img.height)?;
    let unchanged = ct.is_identity();
    for (dst, src) in src_pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(img.rgba.as_chunks::<4>().0)
    {
        if unchanged && src[3] == 255 {
            *dst = *src;
            continue;
        }
        let c = ct.apply(*src);
        let af = f32::from(c[3]) / 255.0;
        dst[0] = (f32::from(c[0]) * af).round() as u8;
        dst[1] = (f32::from(c[1]) * af).round() as u8;
        dst[2] = (f32::from(c[2]) * af).round() as u8;
        dst[3] = c[3];
    }
    Some(src_pixmap)
}

fn draw_text_box(
    pixmap: &mut Pixmap,
    project: &Project,
    field: &neoflash_core::TextFieldState,
    fonts: &mut RasterFonts,
    ct: ColorTransform,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(box_size) = field.box_size else {
        return;
    };
    let Ok(runs) = crate::fonts::positioned_text_runs_with_fonts(field, |fmt| {
        fonts.get(project, &fmt.font, fmt.bold, fmt.italic)
    }) else {
        return;
    };
    let Some(mut local) = Pixmap::new(
        (box_size.width.ceil() as u32).clamp(1, 4096),
        (box_size.height.ceil() as u32).clamp(1, 4096),
    ) else {
        return;
    };
    for run in runs {
        let fmt = run.format.as_ref().unwrap_or(&field.format);
        let rgb = fmt.color.to_be_bytes();
        let color = ct.apply([rgb[1], rgb[2], rgb[3], 255]);
        draw_plain_text(
            &mut local,
            &run.text,
            &fonts.get(project, &fmt.font, fmt.bold, fmt.italic),
            fmt.size.max(4.0) as f32,
            fmt.letter_spacing as f32,
            color,
            SkiaTransform::from_translate(run.x as f32, run.y as f32),
            None,
        );
    }
    pixmap.draw_pixmap(0, 0, local.as_ref(), &PixmapPaint::default(), xf, mask);
}

#[allow(clippy::too_many_arguments)]
fn draw_plain_text(
    pixmap: &mut Pixmap,
    text: &str,
    font: &FontArc,
    size_px: f32,
    letter_spacing: f32,
    color: Color,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    if text.is_empty() || color[3] == 0 || size_px <= 0.5 {
        return;
    }
    let scale = PxScale::from(
        size_px.clamp(2.0, 512.0) * font.height_unscaled() / font.units_per_em().unwrap_or(1000.),
    );
    let scaled = font.as_scaled(scale);

    // Measure lines for local bitmap buffer.
    let lines: Vec<&str> = text.split('\n').collect();
    let line_height = (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(size_px);
    let mut max_w = 0.0_f32;
    for line in &lines {
        let mut w = 0.0_f32;
        let mut prev = None;
        for ch in line.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(p) = prev {
                w += scaled.kern(p, gid);
            }
            w += scaled.h_advance(gid) + letter_spacing;
            prev = Some(gid);
        }
        max_w = max_w.max(w);
    }

    let buf_w = ((max_w.ceil() as u32) + 8).clamp(4, 4096);
    let buf_h = (((lines.len() as f32 * line_height).ceil() as u32) + 8).clamp(4, 4096);

    let Some(mut text_pixmap) = Pixmap::new(buf_w, buf_h) else {
        return;
    };
    let bw_i = buf_w as i32;
    let bh_i = buf_h as i32;
    let data = text_pixmap.data_mut();
    let base_alpha = f32::from(color[3]) / 255.0;

    for (line_idx, line) in lines.iter().enumerate() {
        let baseline_y = scaled.ascent() + line_idx as f32 * line_height;
        if baseline_y - scaled.ascent() > buf_h as f32 {
            break;
        }
        let mut pen_x = 0.;
        let mut prev = None;
        for ch in line.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(p) = prev {
                pen_x += scaled.kern(p, gid);
            }
            let glyph = gid.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline_y));
            pen_x += scaled.h_advance(gid) + letter_spacing;
            prev = Some(gid);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                let min_x = bounds.min.x as i32;
                let min_y = bounds.min.y as i32;
                outlined.draw(|gx, gy, cov| {
                    let px = min_x + gx as i32;
                    let py = min_y + gy as i32;
                    if px >= 0 && px < bw_i && py >= 0 && py < bh_i {
                        let a_cov = (cov * base_alpha).clamp(0.0, 1.0);
                        if a_cov > 0.0 {
                            let idx = ((py as usize) * (buf_w as usize) + (px as usize)) * 4;
                            let a_byte = (a_cov * 255.0).round() as u8;
                            if a_byte > data[idx + 3] {
                                data[idx] = (f32::from(color[0]) * a_cov).round() as u8;
                                data[idx + 1] = (f32::from(color[1]) * a_cov).round() as u8;
                                data[idx + 2] = (f32::from(color[2]) * a_cov).round() as u8;
                                data[idx + 3] = a_byte;
                            }
                        }
                    }
                });
            }
        }
    }

    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: FilterQuality::Bilinear,
    };
    pixmap.draw_pixmap(0, 0, text_pixmap.as_ref(), &paint, xf, mask);
}
