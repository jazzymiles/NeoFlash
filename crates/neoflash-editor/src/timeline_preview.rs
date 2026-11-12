//! Frame view ▸ Preview / Preview in context (TL-10): keyframe thumbnails in the Timeline spans.
//! Scenes are evaluated once per keyframe and document revision (`History::revision`) and painted
//! with the stage painter; only spans in the visible frame range are evaluated, a bounded number per
//! repaint, so long timelines stay responsive.
use crate::app::Editor;
use crate::stage::world_bounds;
use crate::workspace::FramePreview;
use eframe::egui::{self, Color32, Rect};
use neoflash_core::*;
use neoflash_ui::components::timeline::Geometry;
use neoflash_ui::metrics::{PREVIEW_THUMBNAIL_HEIGHT, PREVIEW_THUMBNAIL_WIDTH};
use std::collections::HashMap;

/// Maximum new thumbnails rasterized/evaluated per repaint (`T-227`); remaining visible spans
/// coalesce into a single follow-up repaint so fast scrolling never stalls the UI thread.
pub const MAX_THUMBNAIL_RENDERS_PER_FRAME: usize = 8;
const EVALUATIONS_PER_PAINT: usize = MAX_THUMBNAIL_RENDERS_PER_FRAME;
/// Off-screen retention margin around `[first_frame..=last_frame]` before stale thumbnails are evicted.
pub const THUMBNAIL_RETENTION_MARGIN_FRAMES: u32 = 64;
/// Upper bound on cached `(layer, frame)` thumbnails for the active symbol/revision.
pub const MAX_CACHED_THUMBNAILS: usize = 192;

struct Thumbnail {
    scene: AuthoredScene,
    /// Content bounds in timeline coordinates; not positive = nothing to show.
    bounds: Rect,
}

/// Thumbnails of the current timeline, keyed by (layer, keyframe) for one (symbol, revision, mode).
#[derive(Default)]
pub(crate) struct ThumbnailCache {
    scope: Option<(Id, u64, FramePreview)>,
    frames: HashMap<u32, AuthoredScene>,
    thumbnails: HashMap<(Id, u32), Thumbnail>,
    budget: usize,
    rendered_this_pass: usize,
    evicted_stale_count: usize,
    coalesced_skip_count: usize,
    active_frame_window: Option<(u32, u32)>,
}

impl ThumbnailCache {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.thumbnails.len()
    }
    #[cfg(test)]
    pub(crate) fn cached_frame_count(&self) -> usize {
        self.frames.len()
    }
    #[cfg(test)]
    pub(crate) fn rendered_in_last_pass(&self) -> usize {
        self.rendered_this_pass
    }
    #[cfg(test)]
    pub(crate) fn evicted_stale_count(&self) -> usize {
        self.evicted_stale_count
    }
    #[cfg(test)]
    pub(crate) fn coalesced_skip_count(&self) -> usize {
        self.coalesced_skip_count
    }
    #[cfg(test)]
    pub(crate) fn bounds(&self, layer: &str, frame: u32) -> Option<Rect> {
        self.thumbnails
            .get(&(layer.to_owned(), frame))
            .map(|thumbnail| thumbnail.bounds)
    }
}

/// Thumbnail box of the span `start..next` (0-based, `next` exclusive) inside `row`: after the
/// keyframe cell and before the span's last cell, at most 62 × 38, centred vertically.
pub(crate) fn thumbnail_rect(geo: &Geometry, row: Rect, start: u32, next: u32) -> Option<Rect> {
    let left = geo.frame_x(start + 1) + geo.frame_width + 1.;
    let right = geo.frame_x(next.max(start + 1)) - 1.;
    let width = (right - left).min(PREVIEW_THUMBNAIL_WIDTH);
    let height = (row.height() - 6.).min(PREVIEW_THUMBNAIL_HEIGHT);
    (width >= 10. && height >= 10.).then(|| {
        Rect::from_min_size(
            egui::pos2(left, row.center().y - height / 2.),
            egui::vec2(width, height),
        )
    })
}

impl Editor {
    /// Start a timeline paint pass: drop thumbnails of another symbol, revision or preview mode.
    pub(crate) fn begin_thumbnail_pass(&mut self) {
        let scope = (
            self.symbol_id().to_owned(),
            self.history.revision(),
            self.workspace.timeline.preview,
        );
        let cache = &mut self.timeline_thumbnails;
        if cache.scope.as_ref() != Some(&scope) {
            cache.frames.clear();
            cache.thumbnails.clear();
            cache.scope = Some(scope);
            cache.evicted_stale_count = 0;
            cache.coalesced_skip_count = 0;
        }
        cache.budget = EVALUATIONS_PER_PAINT;
        cache.rendered_this_pass = 0;
        cache.active_frame_window = None;
    }

    /// Start a virtualized timeline paint pass (`T-227`): resets the per-frame rasterization budget
    /// (`MAX_THUMBNAIL_RENDERS_PER_FRAME`) and evicts stale off-screen frame/thumbnail entries outside
    /// `[first_frame - margin ..= last_frame + margin]`.
    pub(crate) fn begin_thumbnail_pass_for_window(
        &mut self,
        first_frame: u32,
        last_frame: u32,
        visible_layer_ids: &[Id],
    ) {
        self.begin_thumbnail_pass();
        let keep_first = first_frame.saturating_sub(THUMBNAIL_RETENTION_MARGIN_FRAMES);
        let keep_last = last_frame.saturating_add(THUMBNAIL_RETENTION_MARGIN_FRAMES);
        let cache = &mut self.timeline_thumbnails;
        cache.active_frame_window = Some((first_frame, last_frame));

        let before_frames = cache.frames.len();
        cache
            .frames
            .retain(|&frame, _| frame >= keep_first && frame <= keep_last);
        let before_thumbs = cache.thumbnails.len();
        cache
            .thumbnails
            .retain(|(_, frame), _| *frame >= keep_first && *frame <= keep_last);
        if cache.thumbnails.len() > MAX_CACHED_THUMBNAILS && !visible_layer_ids.is_empty() {
            cache.thumbnails.retain(|(layer_id, frame), _| {
                (*frame >= first_frame && *frame <= last_frame)
                    || visible_layer_ids.iter().any(|vis| vis == layer_id)
            });
        }
        cache.evicted_stale_count +=
            (before_frames - cache.frames.len()) + (before_thumbs - cache.thumbnails.len());
    }

    fn evaluate_thumbnail_for_elements(
        &mut self,
        key_element_ids: &[Id],
        frame: u32,
        painter: &egui::Painter,
    ) -> Thumbnail {
        let context = self.workspace.timeline.preview == FramePreview::InContext;
        let symbol = self.symbol_id().to_owned();
        let project = &self.project;
        let full = self
            .timeline_thumbnails
            .frames
            .entry(frame)
            .or_insert_with(|| evaluate_scene(project, &symbol, frame, !context, true));
        let mut scene = full.clone();
        if !context {
            // The layer alone: draws whose top-level object sits on this layer.
            scene.draws.retain(|draw| {
                draw.item
                    .instance_path
                    .first()
                    .is_some_and(|id| key_element_ids.contains(id))
            });
        }
        let bounds = if context && self.editing_scene() {
            Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.project.width as f32, self.project.height as f32),
            )
        } else {
            scene.draws.iter().fold(Rect::NOTHING, |bounds, draw| {
                bounds.union(world_bounds(&draw.item, painter))
            })
        };
        Thumbnail { scene, bounds }
    }

    /// Paint the thumbnail of `layer`'s keyframe at `frame` into `rect` (cached; evaluated within
    /// the per-repaint budget).
    #[allow(dead_code)]
    pub(crate) fn paint_span_thumbnail(
        &mut self,
        painter: &egui::Painter,
        layer: &Layer,
        frame: u32,
        rect: Rect,
    ) {
        let key_element_ids: Vec<Id> = layer
            .key_at(frame)
            .map(|key| key.elements.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
        self.paint_span_thumbnail_by_id(painter, &layer.id, &key_element_ids, frame, rect);
    }

    /// Paint the thumbnail of `(layer_id, frame)` into `rect` without requiring a cloned `Layer`.
    /// Off-screen thumbnail boxes and requests exceeding `MAX_THUMBNAIL_RENDERS_PER_FRAME` are
    /// coalesced/skipped so scrolling large timelines stays bounded.
    pub(crate) fn paint_span_thumbnail_by_id(
        &mut self,
        painter: &egui::Painter,
        layer_id: &str,
        key_element_ids: &[Id],
        frame: u32,
        rect: Rect,
    ) {
        let clipped_rect = painter.clip_rect().intersect(rect);
        if !clipped_rect.is_positive() {
            self.timeline_thumbnails.coalesced_skip_count += 1;
            return;
        }
        let id = (layer_id.to_owned(), frame);
        let thumbnail = match self.timeline_thumbnails.thumbnails.remove(&id) {
            Some(thumbnail) => thumbnail,
            None if self.timeline_thumbnails.budget > 0 => {
                self.timeline_thumbnails.budget -= 1;
                self.timeline_thumbnails.rendered_this_pass += 1;
                self.evaluate_thumbnail_for_elements(key_element_ids, frame, painter)
            }
            None => {
                self.timeline_thumbnails.coalesced_skip_count += 1;
                painter.ctx().request_repaint();
                return;
            }
        };
        let bounds = thumbnail.bounds;
        if bounds.is_finite() && bounds.is_positive() {
            let zoom = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
            let origin = rect.center() - bounds.center().to_vec2() * zoom;
            let clipped = painter.with_clip_rect(clipped_rect);
            if self.workspace.timeline.preview == FramePreview::InContext && self.editing_scene() {
                let [r, g, b, a] = self.project.background;
                clipped.rect_filled(
                    Rect::from_min_size(origin, bounds.size() * zoom),
                    0.,
                    Color32::from_rgba_unmultiplied(r, g, b, a),
                );
            }
            self.paint_scene_at(&clipped, &thumbnail.scene, origin, zoom);
        }
        self.timeline_thumbnails.thumbnails.insert(id, thumbnail);
    }
}
