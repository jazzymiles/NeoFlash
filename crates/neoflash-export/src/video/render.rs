use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use crate::ExportError;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use neoflash_core::{
    AuthoredBitmapFill, AuthoredDraw, AuthoredPlayback, AuthoredScene, Color, ColorTransform,
    DecodedImage, DrawItem, ElementKind, FillRule, GradientKind, GradientPaint, MaskCoverage,
    PathCommand, Project, ResolvedVideoPlacement, TimelineVideoTracker, evaluate_scene,
    oval_path_commands, rectangle_path_commands, resolve_scene_video_placements,
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

    let (scene, placements) = if runtime_capture {
        let mut playback =
            AuthoredPlayback::with_live_scripts(&prepared.eval_project, &prepared.root_id, 0, true);
        let mut tracker = TimelineVideoTracker::new();
        tracker.step_with_commands(playback.drain_video_commands(), 0);
        for _ in 0..target_frame {
            playback.tick(&prepared.eval_project);
            tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
        }
        let scene = playback.scene(&prepared.eval_project, false, true);
        let mut map = HashMap::new();
        for draw in &scene.draws {
            if matches!(draw.item.kind, ElementKind::Video { .. })
                && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
            {
                map.insert(draw.item.instance_path.clone(), p.clone());
            }
        }
        (scene, map)
    } else {
        let scene = evaluate_scene(
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
        (scene, map)
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
    Ok(render_authored_scene_with_video_rgba(
        &prepared.eval_project,
        &scene,
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

        let (scene, placements) = if let Some((playback, tracker, cur_frame)) = &mut self.live_state
        {
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
            let mut map = HashMap::new();
            for draw in &scene.draws {
                if matches!(draw.item.kind, ElementKind::Video { .. })
                    && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
                {
                    map.insert(draw.item.instance_path.clone(), p.clone());
                }
            }
            (scene, map)
        } else {
            let scene = evaluate_scene(
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
            (scene, map)
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

        Ok(render_authored_scene_with_video_rgba(
            &self.prepared.eval_project,
            &scene,
            self.width,
            self.height,
            self.transparent_bg,
            &mut self.bitmap_cache,
            &mut self.video_cache,
            &placements,
        ))
    }
}

struct BundledFonts {
    regular: FontRef<'static>,
    bold: FontRef<'static>,
    mono: FontRef<'static>,
}

fn bundled_fonts() -> &'static BundledFonts {
    static FONTS: OnceLock<BundledFonts> = OnceLock::new();
    FONTS.get_or_init(|| BundledFonts {
        regular: FontRef::try_from_slice(FONT_INTER_REGULAR).expect("valid Inter-Regular.ttf"),
        bold: FontRef::try_from_slice(FONT_INTER_BOLD).expect("valid Inter-SemiBold.ttf"),
        mono: FontRef::try_from_slice(FONT_MONO).expect("valid RobotoMono-Regular.ttf"),
    })
}

fn select_font(family: &str, bold: bool) -> &'static FontRef<'static> {
    let fonts = bundled_fonts();
    let lower = family.to_ascii_lowercase();
    if lower.contains("mono") || lower.contains("courier") || lower.contains("code") {
        &fonts.mono
    } else if bold {
        &fonts.bold
    } else {
        &fonts.regular
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
    Ok(render_authored_scene_with_video_rgba(
        project,
        scene,
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
        let mut crop =
            (rect != viewport).then(|| pixmap.clone_rect(rect).expect("bounded pixmap rectangle"));
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

    // Convert premultiplied RGBA from tiny-skia to straight RGBA8 for encoders and egui textures.
    let mut out = pixmap.take();
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = f32::from(a) / 255.0;
            px[0] = ((f32::from(px[0]) / af).round() as u32).min(255) as u8;
            px[1] = ((f32::from(px[1]) / af).round() as u32).min(255) as u8;
            px[2] = ((f32::from(px[2]) / af).round() as u32).min(255) as u8;
        }
    }
    out
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

fn apply_color(base: Color, ct: ColorTransform, alpha: f64) -> Color {
    let mut c = ct.apply(base);
    let safe_alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        1.0
    };
    c[3] = (f64::from(c[3]) * safe_alpha).round().clamp(0.0, 255.0) as u8;
    c
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
    alpha: f64,
) -> SkiaPaint<'a> {
    let mut paint = SkiaPaint {
        anti_alias: true,
        ..SkiaPaint::default()
    };
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        if grad.stops.len() == 1 {
            let c = apply_color(grad.stops[0].color, ct, alpha);
            paint.set_color_rgba8(c[0], c[1], c[2], c[3]);
            return paint;
        }
        let mut stops = Vec::with_capacity(grad.stops.len());
        let mut push_stop = |ratio: u8, color: Color| {
            let c = apply_color(color, ct, alpha);
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
    let c = apply_color(solid, ct, alpha);
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
            let pixels = bitmap_pixmap(&image, ct, 1.0)?;
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
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) {
    let item: &DrawItem = &draw.item;
    let alpha = item.alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
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
                alpha,
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
                alpha,
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
                alpha,
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
                alpha,
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
                draw_bitmap(
                    pixmap, &img, *width, *height, *smoothing, ct, alpha, xf, mask,
                );
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let c = apply_color(*color, ct, alpha);
            draw_plain_text(
                pixmap,
                text,
                select_font(font_family, false),
                *font_size as f32,
                0.,
                c,
                xf,
                mask,
            );
        }
        ElementKind::TextBox { .. } => {
            if let Some(field) = neoflash_core::TextFieldState::from_kind(&item.kind) {
                draw_text_box(pixmap, project, &field, ct, alpha, xf, mask);
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
                draw_bitmap(
                    pixmap,
                    &frame.image,
                    target_w,
                    target_h,
                    true,
                    ct,
                    alpha,
                    xf,
                    mask,
                );
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
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    // The evaluated image color transform already includes ancestor and placement opacity.
    let alpha = if bitmap_paint.is_some() { 1.0 } else { alpha };
    let Some(path) = build_skia_path(commands) else {
        return;
    };
    let fill_visible = fill.1.is_some() || apply_color(fill.0, ct, alpha)[3] > 0;
    if let Some(paint) = bitmap_paint {
        pixmap.fill_path(&path, paint, to_skia_fill_rule(fill_rule), xf, mask);
    } else if fill_visible {
        let paint = configure_paint_shader(fill.0, fill.1, ct, alpha);
        pixmap.fill_path(&path, &paint, to_skia_fill_rule(fill_rule), xf, mask);
    }
    let (stroke_color, stroke_grad, stroke_width) = stroke;
    let stroke_visible = stroke_width > 0.0
        && (stroke_grad.is_some() || apply_color(stroke_color, ct, alpha)[3] > 0);
    if stroke_visible {
        let paint = configure_paint_shader(stroke_color, stroke_grad, ct, alpha);
        let skia_stroke = SkiaStroke {
            width: stroke_width as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..SkiaStroke::default()
        };
        pixmap.stroke_path(&path, &paint, &skia_stroke, xf, mask);
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
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(src_pixmap) = bitmap_pixmap(img, ct, alpha) else {
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

fn bitmap_pixmap(img: &DecodedImage, ct: ColorTransform, alpha: f64) -> Option<Pixmap> {
    let mut src_pixmap = Pixmap::new(img.width, img.height)?;
    let unchanged = ct.is_identity() && alpha == 1.0;
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
        let c = apply_color(*src, ct, alpha);
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
    ct: ColorTransform,
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(box_size) = field.box_size else {
        return;
    };
    let fmt = &field.format;
    let embedded =
        crate::fonts::resolve_project_font_bytes(project, &fmt.font, fmt.bold, fmt.italic);
    let fallback = crate::fonts::fallback_font(&fmt.font, fmt.bold);
    let bytes = embedded.as_deref().unwrap_or(fallback);
    let Ok(runs) = crate::fonts::positioned_text_runs(field, bytes)
        .or_else(|_| crate::fonts::positioned_text_runs(field, fallback))
    else {
        return;
    };
    let Some(mut local) = Pixmap::new(
        (box_size.width.ceil() as u32).clamp(1, 4096),
        (box_size.height.ceil() as u32).clamp(1, 4096),
    ) else {
        return;
    };
    let sfnt = neoflash_core::sfnt_view_bytes(bytes);
    let font = FontRef::try_from_slice(sfnt.as_deref().unwrap_or(bytes)).ok();
    for run in runs {
        let fmt = run.format.as_ref().unwrap_or(&field.format);
        let rgb = fmt.color.to_be_bytes();
        let color = apply_color([rgb[1], rgb[2], rgb[3], 255], ct, alpha);
        draw_plain_text(
            &mut local,
            &run.text,
            if fmt.font == field.format.font && fmt.bold == field.format.bold {
                font.as_ref()
                    .unwrap_or_else(|| select_font(&fmt.font, fmt.bold))
            } else {
                select_font(&fmt.font, fmt.bold)
            },
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
    font: &FontRef<'_>,
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
