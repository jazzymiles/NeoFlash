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

/// New thumbnails evaluated per repaint; the rest follow on the next repaints.
const EVALUATIONS_PER_PAINT: usize = 12;

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
}

impl ThumbnailCache {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.thumbnails.len()
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
        }
        cache.budget = EVALUATIONS_PER_PAINT;
    }

    fn evaluate_thumbnail(
        &mut self,
        layer: &Layer,
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
            let ids: Vec<&Id> = layer
                .key_at(frame)
                .map(|key| key.elements.iter().map(|e| &e.id).collect())
                .unwrap_or_default();
            scene.draws.retain(|draw| {
                draw.item
                    .instance_path
                    .first()
                    .is_some_and(|id| ids.contains(&id))
            });
        }
        let bounds = if context && self.editing.len() == 1 {
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
    pub(crate) fn paint_span_thumbnail(
        &mut self,
        painter: &egui::Painter,
        layer: &Layer,
        frame: u32,
        rect: Rect,
    ) {
        let id = (layer.id.clone(), frame);
        let thumbnail = match self.timeline_thumbnails.thumbnails.remove(&id) {
            Some(thumbnail) => thumbnail,
            None if self.timeline_thumbnails.budget > 0 => {
                self.timeline_thumbnails.budget -= 1;
                self.evaluate_thumbnail(layer, frame, painter)
            }
            None => {
                painter.ctx().request_repaint();
                return;
            }
        };
        let bounds = thumbnail.bounds;
        if bounds.is_finite() && bounds.is_positive() {
            let zoom = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
            let origin = rect.center() - bounds.center().to_vec2() * zoom;
            let clipped = painter.with_clip_rect(painter.clip_rect().intersect(rect));
            if self.workspace.timeline.preview == FramePreview::InContext && self.editing.len() == 1
            {
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
