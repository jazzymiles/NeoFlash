//! Native macOS (`NSAccessibility` via `eframe`/`accesskit`) semantic child trees and action
//! routing for Stage, Timeline, and Library (`T-417`).
//!
//! When AccessKit is active on an [`egui::Context`], this module enriches existing container/row
//! widgets and attaches pure semantic child [`accesskit::Node`]s without altering visual layout,
//! painting, widget hit-testing rects, or the NF2 Stage event order.

use crate::app::{Editor, ScriptTarget};
use crate::library::LibrarySource;
use crate::panels::timeline::TimelineVirtualWindow;
use crate::selection::SelectionTarget;
use crate::stage_mapping::StageMapping;
use eframe::egui::{self, Pos2, Rect, Vec2, accesskit};
use neoflash_core::*;
use neoflash_ui::components::timeline::{self as tl, Geometry, SpanType};
use std::collections::{HashMap, HashSet};

/// Hard per-pass cap on emitted Timeline keyframe cell nodes so huge (e.g. 1,000 × 1,000)
/// timelines stay strictly `O(visible)` well under 1 ms.
pub(crate) const MAX_VISIBLE_TIMELINE_A11Y_CELLS: usize = 512;

/// Hard per-pass cap on emitted Stage semantic element nodes across nested timelines.
const MAX_STAGE_A11Y_NODES: usize = 512;

/// Hard per-pass cap on emitted Library resource `TreeItem` nodes.
const MAX_LIBRARY_RESOURCE_A11Y_NODES: usize = 128;

/// Maximum symbol-instance nesting depth traversed for Stage accessibility nodes.
const MAX_STAGE_A11Y_DEPTH: usize = 5;

#[derive(Clone, Debug)]
struct StageActionTarget {
    symbol: Id,
    layer: Id,
    selectable_element: Id,
    instance_path: Vec<Id>,
}

#[derive(Clone, Debug)]
enum TimelineActionTarget {
    Playhead {
        symbol: Id,
    },
    LayerRow {
        symbol: Id,
        layer_id: Id,
        layer_idx: usize,
    },
    KeyframeCell {
        symbol: Id,
        layer_id: Id,
        layer_idx: usize,
        frame: u32,
    },
}

#[derive(Clone, Debug)]
enum LibraryActionTarget {
    Folder { path: String },
    FolderDisclosure { path: String },
    Symbol { id: String },
    Resource { id: String },
}

/// Ephemeral per-viewport AccessKit action lookup tables populated during UI passes.
#[derive(Default)]
pub(crate) struct AccessibilityBridge {
    stage_targets: HashMap<(egui::ViewportId, accesskit::NodeId), StageActionTarget>,
    timeline_targets: HashMap<(egui::ViewportId, accesskit::NodeId), TimelineActionTarget>,
    library_targets: HashMap<(egui::ViewportId, accesskit::NodeId), LibraryActionTarget>,
}

/// Recorded geometry and state of a rendered Timeline layer row for semantic enrichment.
pub(crate) struct TimelineLayerRowA11y {
    pub response_id: egui::Id,
    pub layer_idx: usize,
    pub rect: Rect,
    pub kind: tl::LayerKind,
    pub depth: usize,
    pub row_selected: bool,
}

/// Recorded geometry of the Timeline frame grid viewport for visible keyframe cell emission.
#[derive(Clone, Copy)]
pub(crate) struct TimelineGridA11y {
    pub response_id: egui::Id,
    pub grid_rect: Rect,
    pub grid_clip: Rect,
    pub cell_width: f32,
    pub row_height: f32,
}

/// Recorded Library row metadata for semantic `Role::TreeItem` enrichment.
pub(crate) enum LibraryRowA11y {
    Root {
        response_id: egui::Id,
        rect: Rect,
        selected: bool,
    },
    Folder {
        response_id: egui::Id,
        disclosure_id: egui::Id,
        path: String,
        depth: usize,
        expanded: bool,
        selected: bool,
        rect: Rect,
    },
    Symbol {
        response_id: egui::Id,
        id: String,
        name: String,
        folder: String,
        kind: SymbolKind,
        linkage: String,
        class_name: String,
        frames: u32,
        depth: usize,
        selected: bool,
        rect: Rect,
    },
}

/// Returns `true` when AccessKit tree generation is active for the current pass on `ctx`.
///
/// Queries the pre-existing `accesskit_root_id` entry so no extra node is inserted.
#[inline]
pub(crate) fn is_accesskit_active(ctx: &egui::Context) -> bool {
    ctx.accesskit_node_builder(egui::accesskit_root_id(), |_| ())
        .is_some()
}

#[inline]
pub(crate) fn node_id_of(id: egui::Id) -> accesskit::NodeId {
    accesskit::NodeId(id.value())
}

fn sanitize_rect(rect: Rect) -> Rect {
    if !rect.min.x.is_finite()
        || !rect.min.y.is_finite()
        || !rect.max.x.is_finite()
        || !rect.max.y.is_finite()
    {
        return Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0));
    }
    let x0 = rect.min.x;
    let y0 = rect.min.y;
    let x1 = rect.max.x.max(x0 + 1.0);
    let y1 = rect.max.y.max(y0 + 1.0);
    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

fn clamp_rect_into_viewport(raw: Rect, viewport: Rect) -> accesskit::Rect {
    let vp = sanitize_rect(viewport);
    if !raw.min.x.is_finite()
        || !raw.min.y.is_finite()
        || !raw.max.x.is_finite()
        || !raw.max.y.is_finite()
        || raw.is_negative()
    {
        return to_ak_rect(Rect::from_min_size(
            vp.min,
            Vec2::new(1.0_f32.min(vp.width()), 1.0_f32.min(vp.height())),
        ));
    }
    if raw.intersects(vp) {
        let inter = raw.intersect(vp);
        let min_w = 1.0_f32.min(vp.width());
        let min_h = 1.0_f32.min(vp.height());
        let x0 = inter
            .min
            .x
            .clamp(vp.min.x, (vp.max.x - min_w).max(vp.min.x));
        let y0 = inter
            .min
            .y
            .clamp(vp.min.y, (vp.max.y - min_h).max(vp.min.y));
        let x1 = inter.max.x.max(x0 + min_w).min(vp.max.x);
        let y1 = inter.max.y.max(y0 + min_h).min(vp.max.y);
        return accesskit::Rect {
            x0: f64::from(x0),
            y0: f64::from(y0),
            x1: f64::from(x1),
            y1: f64::from(y1),
        };
    }
    let min_w = 1.0_f32.min(vp.width());
    let min_h = 1.0_f32.min(vp.height());
    let x0 = raw.min.x.clamp(vp.min.x, (vp.max.x - min_w).max(vp.min.x));
    let y0 = raw.min.y.clamp(vp.min.y, (vp.max.y - min_h).max(vp.min.y));
    accesskit::Rect {
        x0: f64::from(x0),
        y0: f64::from(y0),
        x1: f64::from((x0 + min_w).min(vp.max.x)),
        y1: f64::from((y0 + min_h).min(vp.max.y)),
    }
}

fn to_ak_rect(rect: Rect) -> accesskit::Rect {
    let r = sanitize_rect(rect);
    accesskit::Rect {
        x0: f64::from(r.min.x),
        y0: f64::from(r.min.y),
        x1: f64::from(r.max.x),
        y1: f64::from(r.max.y),
    }
}

/// Detach custom virtual `NodeId`s that `egui` defaulted onto `accesskit_root_id()` so each node
/// has a single parent in `TreeUpdate`.
fn detach_from_accesskit_root(ctx: &egui::Context, detached: &HashSet<accesskit::NodeId>) {
    if detached.is_empty() {
        return;
    }
    ctx.accesskit_node_builder(egui::accesskit_root_id(), |root| {
        let filtered: Vec<accesskit::NodeId> = root
            .children()
            .iter()
            .copied()
            .filter(|cid| !detached.contains(cid))
            .collect();
        root.set_children(filtered);
    });
}

fn append_unique_children(node: &mut accesskit::Node, extra: &[accesskit::NodeId]) {
    if extra.is_empty() {
        return;
    }
    let mut children: Vec<accesskit::NodeId> = node.children().to_vec();
    for &cid in extra {
        if !children.contains(&cid) {
            children.push(cid);
        }
    }
    node.set_children(children);
}

fn map_content_role(role: ContentAccessibleRole) -> Option<accesskit::Role> {
    use ContentAccessibleRole as C;
    use accesskit::Role as R;
    match role {
        C::Auto => None,
        C::Button => Some(R::Button),
        C::CheckBox => Some(R::CheckBox),
        C::RadioButton => Some(R::RadioButton),
        C::ComboBox => Some(R::ComboBox),
        C::List => Some(R::List),
        C::ListItem => Some(R::ListItem),
        C::Menu => Some(R::Menu),
        C::MenuItem => Some(R::MenuItem),
        C::Slider => Some(R::Slider),
        C::ProgressBar => Some(R::ProgressIndicator),
        C::Tab => Some(R::Tab),
        C::TabPanel => Some(R::TabPanel),
        C::Dialog => Some(R::Dialog),
        C::Heading => Some(R::Heading),
        C::Link => Some(R::Link),
        C::Image => Some(R::Image),
        C::Group => Some(R::Group),
        C::Region => Some(R::Region),
        C::Application => Some(R::Application),
        C::Presentation => Some(R::GenericContainer),
    }
}

fn element_raw_text(el: &Element) -> Option<&str> {
    match &el.kind {
        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => Some(text.as_str()),
        _ => None,
    }
}

fn estimate_element_local_rect(project: &Project, el: &Element) -> Rect {
    match &el.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::TextBox { width, height, .. }
        | ElementKind::Video { width, height } => Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new((*width).max(1.0) as f32, (*height).max(1.0) as f32),
        ),
        ElementKind::Bitmap { width, height, .. } => Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new((*width).max(1) as f32, (*height).max(1) as f32),
        ),
        ElementKind::Text {
            text, font_size, ..
        } => {
            let w = ((text.chars().count().max(1) as f64) * font_size.max(1.0) * 0.6).max(4.0);
            let h = (font_size.max(1.0) * 1.2).max(4.0);
            Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32, h as f32))
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            if let Ok(Some(b)) = path_bounds(commands) {
                Rect::from_min_max(
                    Pos2::new(b.x_min as f32, b.y_min as f32),
                    Pos2::new(
                        (b.x_max.max(b.x_min + 1.0)) as f32,
                        (b.y_max.max(b.y_min + 1.0)) as f32,
                    ),
                )
            } else {
                Rect::from_min_size(Pos2::ZERO, Vec2::splat(24.0))
            }
        }
        ElementKind::Instance { symbol_id, .. } => {
            if let Some(sym) = project.symbol(symbol_id) {
                // For hotspot buttons (empty Up frame 0, Hit geometry on frame 3), inspect frame 3
                // or frame 0 to estimate local bounds.
                let frames_to_try: &[u32] =
                    if sym.kind == SymbolKind::Button && sym.timeline.frame_count > 3 {
                        &[0, 3]
                    } else {
                        &[0]
                    };
                for &f in frames_to_try {
                    let items = evaluate(project, symbol_id, f);
                    if !items.is_empty() {
                        let mut union = Rect::NOTHING;
                        for item in &items {
                            let local = match &item.kind {
                                ElementKind::Rectangle { width, height, .. }
                                | ElementKind::Ellipse { width, height, .. }
                                | ElementKind::TextBox { width, height, .. }
                                | ElementKind::Video { width, height } => Rect::from_min_size(
                                    Pos2::ZERO,
                                    Vec2::new(*width as f32, *height as f32),
                                ),
                                ElementKind::Bitmap { width, height, .. } => Rect::from_min_size(
                                    Pos2::ZERO,
                                    Vec2::new(*width as f32, *height as f32),
                                ),
                                ElementKind::Path { commands, .. }
                                | ElementKind::PaintedPath { commands, .. } => {
                                    path_bounds(commands).ok().flatten().map_or(
                                        Rect::NOTHING,
                                        |b| {
                                            Rect::from_min_max(
                                                Pos2::new(b.x_min as f32, b.y_min as f32),
                                                Pos2::new(b.x_max as f32, b.y_max as f32),
                                            )
                                        },
                                    )
                                }
                                ElementKind::Text {
                                    text, font_size, ..
                                } => Rect::from_min_size(
                                    Pos2::ZERO,
                                    Vec2::new(
                                        (text.chars().count().max(1) as f64 * font_size * 0.6)
                                            as f32,
                                        (font_size * 1.2) as f32,
                                    ),
                                ),
                                ElementKind::Instance { .. } => Rect::NOTHING,
                            };
                            if !local.is_negative() {
                                for pt in [
                                    local.left_top(),
                                    local.right_top(),
                                    local.right_bottom(),
                                    local.left_bottom(),
                                ] {
                                    union.extend_with(crate::stage::affine(item.transform, pt));
                                }
                            }
                        }
                        if !union.is_negative() && union.is_finite() {
                            return sanitize_rect(union);
                        }
                    }
                }
            }
            Rect::from_center_size(Pos2::ZERO, Vec2::splat(24.0))
        }
    }
}

fn transform_rect_to_screen(
    local: Rect,
    world_edit_matrix: [f64; 6],
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
) -> Rect {
    let stage_matrix = mapping.map_matrix(world_edit_matrix);
    let z = if zoom.is_finite() && zoom > 1e-6 {
        zoom
    } else {
        1.0
    };
    let mut out = Rect::NOTHING;
    for corner in [
        local.left_top(),
        local.right_top(),
        local.right_bottom(),
        local.left_bottom(),
    ] {
        let stage_pt = crate::stage::affine(stage_matrix, corner);
        out.extend_with(origin + stage_pt.to_vec2() * z);
    }
    sanitize_rect(out)
}

fn symbol_level_extended_accessibility(sym: &Symbol) -> Option<&ExtendedAccessibility> {
    sym.timeline
        .layers
        .first()
        .and_then(|l| l.key_at(0))
        .and_then(|k| {
            k.extended_accessibility
                .iter()
                .find(|x| x.element_id == sym.id || x.element_id == "@symbol")
        })
}

#[derive(Clone)]
struct StageChildSortKey {
    reading_order: Option<u32>,
    tab_index: Option<u32>,
    spatial: [f32; 2],
    painter_order: usize,
    tie_id: String,
}

fn sort_stage_siblings(items: &mut [(StageChildSortKey, accesskit::NodeId)], top_down: bool) {
    items.sort_by(|(a, _), (b, _)| {
        match (a.reading_order, b.reading_order) {
            (Some(ra), Some(rb)) if ra != rb => return ra.cmp(&rb),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        match (a.tab_index, b.tab_index) {
            (Some(ta), Some(tb)) if ta != tb => return ta.cmp(&tb),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        let spatial_cmp = if top_down {
            a.spatial[1]
                .total_cmp(&b.spatial[1])
                .then_with(|| a.spatial[0].total_cmp(&b.spatial[0]))
        } else {
            a.spatial[0]
                .total_cmp(&b.spatial[0])
                .then_with(|| a.spatial[1].total_cmp(&b.spatial[1]))
        };
        spatial_cmp
            .then_with(|| a.painter_order.cmp(&b.painter_order))
            .then_with(|| a.tie_id.cmp(&b.tie_id))
    });
}

struct StageEmitContext<'a> {
    ctx: &'a egui::Context,
    viewport_id: egui::ViewportId,
    stage_rect: Rect,
    origin: Pos2,
    zoom: f32,
    mapping: StageMapping,
    draw_bounds: Vec<(&'a [Id], Rect)>,
    top_down: bool,
    auto_label: bool,
    emitted_count: usize,
    detached: HashSet<accesskit::NodeId>,
}

impl StageEmitContext<'_> {
    fn lookup_draw_screen_rect(&self, path: &[Id]) -> Option<Rect> {
        let mut acc = Rect::NOTHING;
        for (draw_path, r) in &self.draw_bounds {
            if draw_path.starts_with(path) {
                acc = acc.union(*r);
            }
        }
        (!acc.is_negative() && acc.is_finite()).then(|| sanitize_rect(acc))
    }
}

fn timeline_layer_role_badge(layer: &Layer, kind: tl::LayerKind) -> &'static str {
    match layer.properties.role {
        LayerRole::Folder => "Folder",
        LayerRole::Mask => "Mask",
        LayerRole::Guide => "Guide",
        LayerRole::Normal => {
            if kind == tl::LayerKind::Sound
                || (layer.keyframes.iter().any(|k| k.sound.is_some())
                    && layer.keyframes.iter().all(|k| k.elements.is_empty()))
            {
                "Audio"
            } else {
                "Normal"
            }
        }
    }
}

fn classify_resource_kind_label(unit: &ResourceUnit) -> &'static str {
    let lower_mt = unit.media_type.to_ascii_lowercase();
    let lower_path = unit.path.to_ascii_lowercase();
    if unit.is_image() {
        "Bitmap"
    } else if unit.is_audio() {
        "Audio"
    } else if lower_mt.starts_with("video/")
        || lower_path.ends_with(".mp4")
        || lower_path.ends_with(".flv")
        || lower_path.ends_with(".webm")
        || lower_path.ends_with(".mov")
        || lower_path.ends_with(".avi")
    {
        "Video"
    } else if is_font_resource_unit(unit) {
        "Font"
    } else {
        "Resource"
    }
}

impl Editor {
    // =========================================================================
    // 1. STAGE ACCESSIBILITY TREE & ACTIONS
    // =========================================================================

    pub(crate) fn handle_stage_accesskit_actions(&mut self, ctx: &egui::Context) {
        let vp = ctx.viewport_id();
        let has_requests = ctx.input(|i| {
            i.events.iter().any(|ev| {
                matches!(
                    ev,
                    egui::Event::AccessKitActionRequest(req)
                        if self.accessibility_bridge.stage_targets.contains_key(&(vp, req.target))
                )
            })
        });
        if !has_requests {
            return;
        }

        let mut matched = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|ev| {
                if let egui::Event::AccessKitActionRequest(req) = ev
                    && let Some(target) = self
                        .accessibility_bridge
                        .stage_targets
                        .get(&(vp, req.target))
                        .cloned()
                    && matches!(
                        req.action,
                        accesskit::Action::Click | accesskit::Action::Focus
                    )
                {
                    matched.push((req.action, target));
                    return false;
                }
                true
            });
        });

        for (action, target) in matched {
            if target.symbol != self.symbol_id() {
                continue;
            }
            match action {
                accesskit::Action::Focus => {
                    if !self.playing {
                        self.select_stage_target_from_accesskit(ctx, &target);
                    }
                }
                accesskit::Action::Click => {
                    if self.playing {
                        if self.authored_preview.is_none() {
                            self.sync_authored_preview();
                        }
                        let click_path = self
                            .authored_preview
                            .as_ref()
                            .and_then(|p| {
                                p.playback
                                    .interactive_ancestor_path(&self.project, &target.instance_path)
                            })
                            .unwrap_or_else(|| target.instance_path.clone());
                        self.click_authored_preview(&click_path);
                        ctx.request_repaint();
                    } else {
                        self.select_stage_target_from_accesskit(ctx, &target);
                    }
                }
                _ => {}
            }
        }
    }

    fn select_stage_target_from_accesskit(
        &mut self,
        ctx: &egui::Context,
        target: &StageActionTarget,
    ) {
        self.finish_edits();
        self.timeline_edit.focused = false;
        if self
            .selected_frame_range()
            .is_some_and(|(start, end, _)| start == end)
        {
            self.clear_timeline_frame_selection();
        }
        self.set_selection(vec![SelectionTarget {
            layer: target.layer.clone(),
            element: target.selectable_element.clone(),
        }]);
        ctx.memory_mut(|m| {
            if let Some(focused) = m.focused() {
                m.surrender_focus(focused);
            }
        });
        ctx.request_repaint();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_stage_accesskit_tree(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        stage_rect: Rect,
        origin: Pos2,
        mapping: StageMapping,
        stage_response_id: egui::Id,
        displayed_draws: &[AuthoredDraw],
    ) {
        if !is_accesskit_active(ctx) {
            return;
        }
        let vp = ctx.viewport_id();
        self.accessibility_bridge
            .stage_targets
            .retain(|(v, _), _| *v != vp);

        let context_kind = if self.editing_scene() {
            "Scene"
        } else if self.edit_context_is_isolated() {
            "Symbol"
        } else {
            "Symbol (Edit in Place)"
        };
        let symbol_name = self.symbol().name.clone();
        let frame_count = self.symbol().timeline.frame_count.max(1);
        let current_frame_1based = self.frame + 1;
        let zoom_pct = (self.zoom * 100.0).round() as i32;
        let stage_desc = format!(
            "{context_kind}: {symbol_name} · Frame {current_frame_1based} of {frame_count} · Zoom {zoom_pct}%"
        );

        let doc_acc = self.project.document_accessibility();
        let mut draw_bounds = Vec::with_capacity(displayed_draws.len());
        let z = if self.zoom.is_finite() && self.zoom > 1e-6 {
            self.zoom
        } else {
            1.0
        };
        for draw in displayed_draws {
            let wb = crate::stage::world_bounds(&draw.item, painter);
            if !wb.is_negative() && wb.is_finite() {
                let sr = Rect::from_min_max(
                    origin + wb.min.to_vec2() * z,
                    origin + wb.max.to_vec2() * z,
                );
                draw_bounds.push((draw.item.instance_path.as_slice(), sanitize_rect(sr)));
            }
        }

        let mut emit_ctx = StageEmitContext {
            ctx,
            viewport_id: vp,
            stage_rect,
            origin,
            zoom: z,
            mapping,
            draw_bounds,
            top_down: doc_acc.reading_order_top_down,
            auto_label: doc_acc.auto_label,
            emitted_count: 0,
            detached: HashSet::new(),
        };

        let active_symbol_id = self.symbol_id().to_owned();
        let stage_children = if doc_acc.accessible
            && let Some(sym) = self.project.symbol(&active_symbol_id)
        {
            let timeline = sym.timeline.clone();
            self.emit_stage_timeline_children(
                &mut emit_ctx,
                &active_symbol_id,
                &timeline,
                self.frame,
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                &[],
                None,
                0,
            )
        } else {
            Vec::new()
        };

        detach_from_accesskit_root(ctx, &emit_ctx.detached);

        ctx.accesskit_node_builder(stage_response_id, |node| {
            node.set_role(accesskit::Role::Region);
            node.set_role_description("Stage");
            node.set_label("Stage");
            node.set_description(stage_desc);
            node.set_bounds(to_ak_rect(stage_rect));
            node.add_action(accesskit::Action::Focus);
            node.add_action(accesskit::Action::Click);
            append_unique_children(node, &stage_children);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_stage_timeline_children(
        &mut self,
        ec: &mut StageEmitContext<'_>,
        root_symbol_id: &str,
        timeline: &Timeline,
        frame: u32,
        parent_matrix: [f64; 6],
        path_prefix: &[Id],
        inherited_top_target: Option<(&str, &str)>,
        depth: usize,
    ) -> Vec<accesskit::NodeId> {
        if depth > MAX_STAGE_A11Y_DEPTH || ec.emitted_count >= MAX_STAGE_A11Y_NODES {
            return Vec::new();
        }

        struct SampledLayerEntry<'a> {
            layer_id: Id,
            element: Element,
            ext: Option<&'a ExtendedAccessibility>,
            child_frame: Option<u32>,
            group_id: Option<Id>,
            painter_idx: usize,
        }

        let fps = f64::from(self.project.fps.max(1));
        let count = timeline.frame_count;
        let mut entries: Vec<SampledLayerEntry<'_>> = Vec::new();
        let mut group_names: HashMap<(Id, Id), String> = HashMap::new();
        let mut text_candidates: Vec<(Id, String, [f64; 2])> = Vec::new();
        let mut element_lookup: HashMap<Id, Element> = HashMap::new();
        let mut painter_counter = 0usize;

        for layer in &timeline.layers {
            if !layer_effectively_visible(timeline, layer)
                || matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
            {
                continue;
            }
            let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
            if frame >= layer_end {
                continue;
            }
            let mut ext_map: HashMap<&str, &ExtendedAccessibility> = HashMap::new();
            if let Some(kf) = layer.key_at(frame) {
                for ext in &kf.extended_accessibility {
                    ext_map.insert(ext.element_id.as_str(), ext);
                }
            }
            let mut member_to_group: HashMap<&str, &AuthoringGroup> = HashMap::new();
            for group in &layer.properties.groups {
                if group.members.len() >= 2 {
                    group_names.insert((layer.id.clone(), group.id.clone()), group.name.clone());
                    for m in &group.members {
                        member_to_group.insert(m.as_str(), group);
                    }
                }
            }

            let sampled = sample_layer_in_at_fps(timeline, layer, frame, fps);
            for el in sampled {
                if !el.visible {
                    continue;
                }
                if layer.properties.role == LayerRole::Mask && el.accessibility.is_none() {
                    continue;
                }
                if let Some(raw_txt) = element_raw_text(&el) {
                    let trimmed = raw_txt.trim();
                    if !trimmed.is_empty() {
                        let local_r = estimate_element_local_rect(&self.project, &el);
                        let world_c =
                            crate::stage::affine(transform_matrix(el.transform), local_r.center());
                        text_candidates.push((
                            el.id.clone(),
                            trimmed.to_owned(),
                            [f64::from(world_c.x), f64::from(world_c.y)],
                        ));
                    }
                }
                element_lookup.insert(el.id.clone(), el.clone());

                let child_frame = if let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &el.kind
                    && let Some(child_sym) = self.project.symbol(symbol_id)
                {
                    let c_count = child_sym.timeline.frame_count.max(1);
                    let age = if layer_effectively_synced(timeline, layer) {
                        frame
                    } else {
                        frame.saturating_sub(placement_start(layer, frame, &el.id))
                    };
                    Some(if child_sym.kind == SymbolKind::Button {
                        0
                    } else {
                        match *playback {
                            InstancePlayback::Independent | InstancePlayback::Loop => age % c_count,
                            InstancePlayback::LoopFrom(start) => {
                                age.saturating_add(start) % c_count
                            }
                            InstancePlayback::PlayOnce => age.min(c_count - 1),
                            InstancePlayback::PlayOnceFrom(start) => {
                                age.saturating_add(start).min(c_count - 1)
                            }
                            InstancePlayback::SingleFrame(f) => f.min(c_count - 1),
                        }
                    })
                } else {
                    None
                };

                let ext = ext_map.get(el.id.as_str()).copied();
                let group_id = member_to_group.get(el.id.as_str()).map(|g| g.id.clone());
                let p_idx = painter_counter;
                painter_counter += 1;
                entries.push(SampledLayerEntry {
                    layer_id: layer.id.clone(),
                    element: el,
                    ext,
                    child_frame,
                    group_id,
                    painter_idx: p_idx,
                });
            }
        }

        let mut direct_siblings: Vec<(StageChildSortKey, accesskit::NodeId)> = Vec::new();
        struct BuiltGroupAccum {
            layer_id: Id,
            first_member_id: Id,
            name: String,
            members: Vec<(StageChildSortKey, accesskit::NodeId)>,
            bounds_union: Rect,
            any_selected: bool,
            min_painter: usize,
        }
        let mut group_buckets: HashMap<(Id, Id), BuiltGroupAccum> = HashMap::new();

        for entry in entries {
            if ec.emitted_count >= MAX_STAGE_A11Y_NODES {
                break;
            }
            let el = &entry.element;
            let ext = entry.ext;
            if ext.is_some_and(|x| x.silent) {
                continue;
            }
            if let ElementKind::Instance { symbol_id, .. } = &el.kind
                && let Some(sym) = self.project.symbol(symbol_id)
                && symbol_level_extended_accessibility(sym).is_some_and(|x| x.silent)
            {
                continue;
            }

            let is_top_selected = depth == 0
                && self.selection.symbol == root_symbol_id
                && self
                    .selection
                    .targets
                    .iter()
                    .any(|t| t.layer == entry.layer_id && t.element == el.id);
            let in_group = entry.group_id.is_some();

            let Some(mut role) =
                self.classify_stage_element_role(el, ext, is_top_selected || in_group)
            else {
                continue;
            };

            let mut full_path = Vec::with_capacity(path_prefix.len() + 1);
            full_path.extend_from_slice(path_prefix);
            full_path.push(el.id.clone());

            let world_matrix = affine_multiply(parent_matrix, transform_matrix(el.transform));
            let raw_screen_rect = ec.lookup_draw_screen_rect(&full_path).unwrap_or_else(|| {
                let local_r = estimate_element_local_rect(&self.project, el);
                transform_rect_to_screen(local_r, world_matrix, ec.mapping, ec.origin, ec.zoom)
            });

            let (top_layer, top_elem) = match inherited_top_target {
                Some((tl, te)) => (tl.to_owned(), te.to_owned()),
                None => (entry.layer_id.clone(), el.id.clone()),
            };

            // Recurse into MovieClip / Graphic instances unless `force_simple` or `!tab_children`.
            let nested_children = if let ElementKind::Instance { symbol_id, .. } = &el.kind
                && let Some(sym) = self.project.symbol(symbol_id).cloned()
                && sym.kind != SymbolKind::Button
            {
                let sym_ext = symbol_level_extended_accessibility(&sym);
                let force_simple =
                    ext.is_some_and(|x| x.force_simple) || sym_ext.is_some_and(|x| x.force_simple);
                let tab_children =
                    ext.is_none_or(|x| x.tab_children) && sym_ext.is_none_or(|x| x.tab_children);
                if !force_simple && tab_children {
                    let c_frame = entry
                        .child_frame
                        .unwrap_or(frame % sym.timeline.frame_count.max(1));
                    self.emit_stage_timeline_children(
                        ec,
                        root_symbol_id,
                        &sym.timeline,
                        c_frame,
                        world_matrix,
                        &full_path,
                        Some((top_layer.as_str(), top_elem.as_str())),
                        depth + 1,
                    )
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            if !nested_children.is_empty() && role == accesskit::Role::GraphicsObject {
                role = accesskit::Role::Group;
            }

            let label = self.resolve_stage_element_label(
                el,
                ext,
                role,
                &element_lookup,
                &text_candidates,
                ec.auto_label,
            );
            let description = self.resolve_stage_element_description(el, ext, &element_lookup);
            let text_value = element_raw_text(el).map(str::to_owned);
            let shortcut = ext
                .map(|x| x.shortcut.trim().to_owned())
                .filter(|s| !s.is_empty());
            let tab_index = el
                .accessibility
                .as_ref()
                .and_then(|a| (a.tab_order > 0).then_some(a.tab_order));
            let reading_order = ext.and_then(|x| x.reading_order).filter(|&ro| ro > 0);
            let is_disabled = el.accessibility.as_ref().is_some_and(|a| a.disabled);
            let live = ext
                .map(|x| x.live)
                .unwrap_or(AccessibilityLivePoliteness::Off);

            let egui_id = egui::Id::new(("neoflash_stage_a11y_node", root_symbol_id, &full_path));
            let ak_id = node_id_of(egui_id);
            ec.emitted_count += 1;
            ec.detached.insert(ak_id);

            let ak_bounds = clamp_rect_into_viewport(raw_screen_rect, ec.stage_rect);
            ec.ctx.accesskit_node_builder(egui_id, |node| {
                node.set_role(role);
                if !label.is_empty() {
                    node.set_label(label);
                }
                if let Some(desc) = description {
                    node.set_description(desc);
                }
                if let Some(val) = text_value {
                    node.set_value(val);
                }
                if let Some(sc) = shortcut {
                    node.set_keyboard_shortcut(sc);
                }
                node.set_selected(is_top_selected);
                if is_disabled {
                    node.set_disabled();
                }
                match live {
                    AccessibilityLivePoliteness::Off => {}
                    AccessibilityLivePoliteness::Polite => {
                        node.set_live(accesskit::Live::Polite);
                    }
                    AccessibilityLivePoliteness::Assertive => {
                        node.set_live(accesskit::Live::Assertive);
                    }
                }
                node.set_bounds(ak_bounds);
                node.add_action(accesskit::Action::Focus);
                node.add_action(accesskit::Action::Click);
                if !nested_children.is_empty() {
                    node.set_children(nested_children);
                }
            });

            self.accessibility_bridge.stage_targets.insert(
                (ec.viewport_id, ak_id),
                StageActionTarget {
                    symbol: root_symbol_id.to_owned(),
                    layer: top_layer.clone(),
                    selectable_element: top_elem.clone(),
                    instance_path: full_path,
                },
            );

            let sort_key = StageChildSortKey {
                reading_order,
                tab_index,
                spatial: [raw_screen_rect.min.x, raw_screen_rect.min.y],
                painter_order: entry.painter_idx,
                tie_id: el.id.clone(),
            };

            if depth == 0
                && let Some(gid) = entry.group_id
            {
                let gkey = (entry.layer_id.clone(), gid.clone());
                let gname = group_names.get(&gkey).cloned().unwrap_or_default();
                let bucket = group_buckets
                    .entry(gkey)
                    .or_insert_with(|| BuiltGroupAccum {
                        layer_id: entry.layer_id.clone(),
                        first_member_id: el.id.clone(),
                        name: gname,
                        members: Vec::new(),
                        bounds_union: Rect::NOTHING,
                        any_selected: false,
                        min_painter: entry.painter_idx,
                    });
                bucket.bounds_union = bucket.bounds_union.union(raw_screen_rect);
                bucket.any_selected |= is_top_selected;
                bucket.min_painter = bucket.min_painter.min(entry.painter_idx);
                bucket.members.push((sort_key, ak_id));
            } else {
                direct_siblings.push((sort_key, ak_id));
            }
        }

        // Materialize multi-member AuthoringGroups as `Role::Group` containers.
        for ((layer_id, group_id), mut bucket) in group_buckets {
            if bucket.members.len() == 1 {
                direct_siblings.push(bucket.members.remove(0));
                continue;
            }
            if ec.emitted_count >= MAX_STAGE_A11Y_NODES {
                direct_siblings.extend(bucket.members);
                continue;
            }
            sort_stage_siblings(&mut bucket.members, ec.top_down);
            let min_ro = bucket
                .members
                .iter()
                .filter_map(|(k, _)| k.reading_order)
                .min();
            let min_tab = bucket.members.iter().filter_map(|(k, _)| k.tab_index).min();
            let member_ids: Vec<accesskit::NodeId> =
                bucket.members.iter().map(|(_, id)| *id).collect();
            let group_egui_id = egui::Id::new((
                "neoflash_stage_a11y_group",
                root_symbol_id,
                &layer_id,
                &group_id,
            ));
            let group_ak_id = node_id_of(group_egui_id);
            ec.emitted_count += 1;
            ec.detached.insert(group_ak_id);

            let group_label = if bucket.name.trim().is_empty() {
                "Group".to_owned()
            } else {
                bucket.name.trim().to_owned()
            };
            let g_rect = sanitize_rect(bucket.bounds_union);
            let ak_bounds = clamp_rect_into_viewport(g_rect, ec.stage_rect);
            ec.ctx.accesskit_node_builder(group_egui_id, |node| {
                node.set_role(accesskit::Role::Group);
                node.set_label(group_label);
                node.set_selected(bucket.any_selected);
                node.set_bounds(ak_bounds);
                node.add_action(accesskit::Action::Focus);
                node.add_action(accesskit::Action::Click);
                node.set_children(member_ids);
            });
            self.accessibility_bridge.stage_targets.insert(
                (ec.viewport_id, group_ak_id),
                StageActionTarget {
                    symbol: root_symbol_id.to_owned(),
                    layer: bucket.layer_id.clone(),
                    selectable_element: bucket.first_member_id.clone(),
                    instance_path: vec![bucket.first_member_id],
                },
            );
            direct_siblings.push((
                StageChildSortKey {
                    reading_order: min_ro,
                    tab_index: min_tab,
                    spatial: [g_rect.min.x, g_rect.min.y],
                    painter_order: bucket.min_painter,
                    tie_id: group_id,
                },
                group_ak_id,
            ));
        }

        sort_stage_siblings(&mut direct_siblings, ec.top_down);
        direct_siblings.into_iter().map(|(_, id)| id).collect()
    }

    fn classify_stage_element_role(
        &self,
        el: &Element,
        ext: Option<&ExtendedAccessibility>,
        force_expose_shape: bool,
    ) -> Option<accesskit::Role> {
        if let Some(ext) = ext
            && ext.role != ContentAccessibleRole::Auto
        {
            if ext.role == ContentAccessibleRole::Presentation && el.accessibility.is_none() {
                return None;
            }
            if let Some(mapped) = map_content_role(ext.role) {
                return Some(mapped);
            }
        }

        match &el.kind {
            ElementKind::Instance { .. } => {
                let effective = el.effective_symbol_kind(&self.project);
                if let Some(acc) = &el.accessibility {
                    return Some(match acc.role {
                        AccessibleRole::Button => accesskit::Role::Button,
                        AccessibleRole::Image => match effective {
                            Some(SymbolKind::MovieClip) => accesskit::Role::Group,
                            _ => accesskit::Role::Image,
                        },
                    });
                }
                Some(match effective {
                    Some(SymbolKind::Button) => accesskit::Role::Button,
                    Some(SymbolKind::MovieClip) => accesskit::Role::Group,
                    Some(SymbolKind::Graphic) | None => accesskit::Role::GraphicsObject,
                })
            }
            ElementKind::TextBox { format, .. } => {
                let opts = format.field_options().unwrap_or_default();
                if opts.field_type == TextFieldType::Input {
                    return Some(accesskit::Role::TextInput);
                }
                if let Some(acc) = &el.accessibility {
                    return Some(match acc.role {
                        AccessibleRole::Button => accesskit::Role::Button,
                        AccessibleRole::Image => accesskit::Role::Image,
                    });
                }
                if opts.selectable {
                    Some(accesskit::Role::Region)
                } else {
                    Some(accesskit::Role::Label)
                }
            }
            ElementKind::Text { .. } => {
                if let Some(acc) = &el.accessibility {
                    return Some(match acc.role {
                        AccessibleRole::Button => accesskit::Role::Button,
                        AccessibleRole::Image => accesskit::Role::Image,
                    });
                }
                Some(accesskit::Role::Label)
            }
            ElementKind::Bitmap { .. } => {
                if let Some(acc) = &el.accessibility
                    && acc.role == AccessibleRole::Button
                {
                    return Some(accesskit::Role::Button);
                }
                Some(accesskit::Role::Image)
            }
            ElementKind::Video { .. } => Some(accesskit::Role::Video),
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let has_explicit = el.accessibility.is_some()
                    || ext.is_some()
                    || !el.name.trim().is_empty()
                    || !el.scripts.is_empty()
                    || force_expose_shape;
                if !has_explicit {
                    return None;
                }
                if let Some(acc) = &el.accessibility {
                    return Some(match acc.role {
                        AccessibleRole::Button => accesskit::Role::Button,
                        AccessibleRole::Image => accesskit::Role::Image,
                    });
                }
                Some(accesskit::Role::GraphicsObject)
            }
        }
    }

    fn resolve_stage_element_label(
        &self,
        el: &Element,
        ext: Option<&ExtendedAccessibility>,
        role: accesskit::Role,
        element_lookup: &HashMap<Id, Element>,
        text_candidates: &[(Id, String, [f64; 2])],
        auto_label_enabled: bool,
    ) -> String {
        if let Some(acc) = &el.accessibility {
            let trimmed = acc.name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
        if let Some(ext) = ext
            && let Some(label_id) = &ext.aria_labelled_by
            && let Some(target_el) = element_lookup.get(label_id)
        {
            if let Some(txt) = element_raw_text(target_el)
                && !txt.trim().is_empty()
            {
                return txt.trim().to_owned();
            }
            if let Some(acc) = &target_el.accessibility
                && !acc.name.trim().is_empty()
            {
                return acc.name.trim().to_owned();
            }
        }
        if !el.name.trim().is_empty() {
            return el.name.trim().to_owned();
        }
        if let Some(txt) = element_raw_text(el) {
            let trimmed = txt.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
        let allow_auto = auto_label_enabled && !ext.is_some_and(|x| x.no_auto_labeling);
        if allow_auto
            && (role == accesskit::Role::Button || matches!(el.kind, ElementKind::Instance { .. }))
        {
            let local_r = estimate_element_local_rect(&self.project, el);
            let world_c = crate::stage::affine(transform_matrix(el.transform), local_r.center());
            let cx = f64::from(world_c.x);
            let cy = f64::from(world_c.y);
            let mut best: Option<(f64, &str)> = None;
            for (tid, txt, tcenter) in text_candidates {
                if tid == &el.id {
                    continue;
                }
                let dist = (tcenter[0] - cx).hypot(tcenter[1] - cy);
                if dist <= 240.0 && best.is_none_or(|(b, _)| dist < b) {
                    best = Some((dist, txt.as_str()));
                }
            }
            if let Some((_, matched)) = best {
                return matched.to_owned();
            }
        }
        match &el.kind {
            ElementKind::Instance { symbol_id, .. } => self
                .project
                .symbol(symbol_id)
                .map(|s| s.name.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Instance".to_owned()),
            ElementKind::Bitmap { resource_id, .. } => self
                .project
                .resource(resource_id)
                .map(|r| r.path.clone())
                .unwrap_or_else(|| "Bitmap".to_owned()),
            ElementKind::Video { .. } => "Video".to_owned(),
            ElementKind::TextBox { .. } if role == accesskit::Role::TextInput => {
                "Text input".to_owned()
            }
            ElementKind::TextBox { .. } | ElementKind::Text { .. } => "Text".to_owned(),
            _ => "Graphic".to_owned(),
        }
    }

    fn resolve_stage_element_description(
        &self,
        el: &Element,
        ext: Option<&ExtendedAccessibility>,
        element_lookup: &HashMap<Id, Element>,
    ) -> Option<String> {
        if let Some(acc) = &el.accessibility {
            let trimmed = acc.description.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
        if let Some(ext) = ext
            && let Some(desc_id) = &ext.aria_described_by
            && let Some(target_el) = element_lookup.get(desc_id)
        {
            if let Some(txt) = element_raw_text(target_el)
                && !txt.trim().is_empty()
            {
                return Some(txt.trim().to_owned());
            }
            if let Some(acc) = &target_el.accessibility
                && !acc.description.trim().is_empty()
            {
                return Some(acc.description.trim().to_owned());
            }
        }
        None
    }

    // =========================================================================
    // 2. TIMELINE ACCESSIBILITY TREE & ACTIONS
    // =========================================================================

    pub(crate) fn handle_timeline_accesskit_actions(&mut self, ctx: &egui::Context) {
        let vp = ctx.viewport_id();
        let has_requests = ctx.input(|i| {
            i.events.iter().any(|ev| {
                matches!(
                    ev,
                    egui::Event::AccessKitActionRequest(req)
                        if self.accessibility_bridge.timeline_targets.contains_key(&(vp, req.target))
                )
            })
        });
        if !has_requests {
            return;
        }

        let mut matched = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|ev| {
                if let egui::Event::AccessKitActionRequest(req) = ev
                    && let Some(target) = self
                        .accessibility_bridge
                        .timeline_targets
                        .get(&(vp, req.target))
                        .cloned()
                {
                    let supported = match target {
                        TimelineActionTarget::Playhead { .. } => matches!(
                            req.action,
                            accesskit::Action::Increment
                                | accesskit::Action::Decrement
                                | accesskit::Action::SetValue
                                | accesskit::Action::Focus
                        ),
                        TimelineActionTarget::LayerRow { .. }
                        | TimelineActionTarget::KeyframeCell { .. } => {
                            matches!(
                                req.action,
                                accesskit::Action::Click | accesskit::Action::Focus
                            )
                        }
                    };
                    if supported {
                        matched.push((req.clone(), target));
                        return false;
                    }
                }
                true
            });
        });

        for (req, target) in matched {
            match target {
                TimelineActionTarget::Playhead { symbol } => {
                    if symbol != self.symbol_id() {
                        continue;
                    }
                    match req.action {
                        accesskit::Action::Increment => {
                            self.seek_playhead_from_accesskit(ctx, self.frame.saturating_add(1));
                        }
                        accesskit::Action::Decrement => {
                            self.seek_playhead_from_accesskit(ctx, self.frame.saturating_sub(1));
                        }
                        accesskit::Action::SetValue => {
                            let parsed = match req.data.as_ref() {
                                Some(accesskit::ActionData::NumericValue(v)) if v.is_finite() => {
                                    Some(*v)
                                }
                                Some(accesskit::ActionData::Value(s)) => {
                                    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
                                }
                                _ => None,
                            };
                            if let Some(val_1based) = parsed {
                                let one_based = val_1based.round().clamp(1.0, 1_000_000.0) as u32;
                                self.seek_playhead_from_accesskit(ctx, one_based - 1);
                            }
                        }
                        accesskit::Action::Focus => {
                            self.timeline_edit.focused = true;
                            ctx.request_repaint();
                        }
                        _ => {}
                    }
                }
                TimelineActionTarget::LayerRow {
                    symbol,
                    layer_id,
                    layer_idx,
                } => {
                    if symbol != self.symbol_id() {
                        continue;
                    }
                    let resolved_idx = self
                        .symbol()
                        .timeline
                        .layers
                        .get(layer_idx)
                        .filter(|l| l.id == layer_id)
                        .map(|_| layer_idx)
                        .or_else(|| {
                            self.symbol()
                                .timeline
                                .layers
                                .iter()
                                .position(|l| l.id == layer_id)
                        });
                    if let Some(idx) = resolved_idx {
                        self.finish_edits();
                        self.timeline_key_drag = None;
                        self.select_layer_row(idx, false, false);
                        self.clear_selection();
                        self.timeline_edit.focused = true;
                        ctx.request_repaint();
                    }
                }
                TimelineActionTarget::KeyframeCell {
                    symbol,
                    layer_id,
                    layer_idx,
                    frame,
                } => {
                    if symbol != self.symbol_id() {
                        continue;
                    }
                    let resolved_idx = self
                        .symbol()
                        .timeline
                        .layers
                        .get(layer_idx)
                        .filter(|l| l.id == layer_id)
                        .map(|_| layer_idx)
                        .or_else(|| {
                            self.symbol()
                                .timeline
                                .layers
                                .iter()
                                .position(|l| l.id == layer_id)
                        });
                    if let Some(idx) = resolved_idx {
                        let max_frame = self.symbol().timeline.frame_count.saturating_sub(1);
                        let target_frame = frame.min(max_frame);
                        self.finish_edits();
                        self.timeline_key_drag = None;
                        self.set_playing(false);
                        self.reset_authored_preview();
                        self.select_layer_row(idx, false, false);
                        self.frame = target_frame;
                        self.select_frame_range(FrameRange {
                            layers: vec![layer_id],
                            start: target_frame,
                            end: target_frame,
                        });
                        self.script_target = ScriptTarget::Frame;
                        self.clear_selection();
                        ctx.request_repaint();
                    }
                }
            }
        }
    }

    fn seek_playhead_from_accesskit(&mut self, ctx: &egui::Context, next_frame_0based: u32) {
        let max_frame = self.symbol().timeline.frame_count.saturating_sub(1);
        let target = next_frame_0based.min(max_frame);
        self.finish_edits();
        self.timeline_key_drag = None;
        self.set_playing(false);
        self.reset_authored_preview();
        self.frame = target;
        if !self.are_layers_deselected()
            && let Some(layer) = self.symbol().timeline.layers.get(self.layer)
        {
            let layer_id = layer.id.clone();
            self.select_frame_range(FrameRange {
                layers: vec![layer_id],
                start: target,
                end: target,
            });
        }
        self.script_target = ScriptTarget::Frame;
        self.clear_selection();
        ctx.request_repaint();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_timeline_accesskit_tree(
        &mut self,
        ctx: &egui::Context,
        timeline_scope_ui_id: egui::Id,
        layer_tree: Option<(egui::Id, Rect)>,
        playhead: Option<(egui::Id, Rect)>,
        layer_rows: &[TimelineLayerRowA11y],
        grid: Option<TimelineGridA11y>,
        layer_indices: &[usize],
        script_frames: &[u32],
    ) {
        if !is_accesskit_active(ctx) {
            return;
        }
        let vp = ctx.viewport_id();
        self.accessibility_bridge
            .timeline_targets
            .retain(|(v, _), _| *v != vp);

        let active_symbol_id = self.symbol_id().to_owned();
        let count = self.symbol().timeline.frame_count.max(1);
        let fps = self.project.fps.max(1);

        ctx.accesskit_node_builder(timeline_scope_ui_id, |node| {
            node.set_role(accesskit::Role::Region);
            node.set_label("Timeline");
        });

        if let Some((layer_tree_id, layer_tree_clip)) = layer_tree {
            let total_rows = layer_indices.len();
            let ak_bounds = to_ak_rect(layer_tree_clip);
            ctx.accesskit_node_builder(layer_tree_id, |node| {
                node.set_role(accesskit::Role::Tree);
                node.set_label("Timeline layers");
                node.set_size_of_set(total_rows);
                node.set_bounds(ak_bounds);
            });
        }

        // 1. Playhead / scrubber (`Role::Slider` with 1-based numeric value & range; no string `value`
        //    so `accesskit_macos` exposes `numeric_value` as `NSNumber` on `NSAccessibilitySliderRole`).
        if let Some((playhead_id, playhead_rect)) = playhead {
            let current_1based = f64::from(self.frame + 1);
            let max_1based = f64::from(count);
            let secs = f64::from(self.frame) / f64::from(fps);
            let ak_id = node_id_of(playhead_id);
            ctx.accesskit_node_builder(playhead_id, |node| {
                node.set_role(accesskit::Role::Slider);
                node.set_label("Playhead");
                node.set_description(format!(
                    "Frame {} of {count} · {secs:.2}s at {fps} fps",
                    self.frame + 1
                ));
                node.set_numeric_value(current_1based);
                node.set_min_numeric_value(1.0);
                node.set_max_numeric_value(max_1based);
                node.set_numeric_value_step(1.0);
                node.set_bounds(to_ak_rect(playhead_rect));
                node.add_action(accesskit::Action::Focus);
                node.add_action(accesskit::Action::Increment);
                node.add_action(accesskit::Action::Decrement);
                node.add_action(accesskit::Action::SetValue);
            });
            self.accessibility_bridge.timeline_targets.insert(
                (vp, ak_id),
                TimelineActionTarget::Playhead {
                    symbol: active_symbol_id.clone(),
                },
            );
        }

        // 2. Visible Timeline layer rows (`Role::TreeItem` with role badge + visibility/lock description).
        let layers_deselected = self.are_layers_deselected();
        for row in layer_rows {
            let Some(layer) = self.symbol().timeline.layers.get(row.layer_idx) else {
                continue;
            };
            let layer_id = layer.id.clone();
            let badge = timeline_layer_role_badge(layer, row.kind);
            let vis_str = if layer.visible { "Visible" } else { "Hidden" };
            let lock_str = if layer.locked { "Locked" } else { "Unlocked" };
            let mut desc = format!("{badge} layer · {vis_str} · {lock_str}");
            if self.is_layer_soloed(row.layer_idx) {
                desc.push_str(" · Solo");
            }
            if layer.properties.outline {
                desc.push_str(" · Outline");
            }
            let is_active_or_sel =
                !layers_deselected && (row.layer_idx == self.layer || row.row_selected);
            let is_folder = layer.properties.role == LayerRole::Folder;
            let folder_expanded = !layer.properties.collapsed;
            let full_label = format!("{} ({})", layer.name, row.kind.label());
            let depth = row.depth;
            let rect = row.rect;
            let ak_id = node_id_of(row.response_id);

            ctx.accesskit_node_builder(row.response_id, |node| {
                node.set_role(accesskit::Role::TreeItem);
                node.set_label(full_label);
                node.set_description(desc);
                node.set_selected(is_active_or_sel);
                node.set_level(depth + 1);
                if is_folder {
                    node.set_expanded(folder_expanded);
                }
                node.set_bounds(to_ak_rect(rect));
                node.add_action(accesskit::Action::Focus);
                node.add_action(accesskit::Action::Click);
            });
            self.accessibility_bridge.timeline_targets.insert(
                (vp, ak_id),
                TimelineActionTarget::LayerRow {
                    symbol: active_symbol_id.clone(),
                    layer_id,
                    layer_idx: row.layer_idx,
                },
            );
        }

        // 3. Visible keyframe spans on visible rows (`Role::Cell` under `Timeline frames` `Role::Grid`).
        let Some(grid) = grid else {
            return;
        };
        if !grid.grid_clip.is_positive() || !grid.grid_clip.is_finite() {
            return;
        }
        let total_rows = layer_indices.len();
        let window = TimelineVirtualWindow::with_default_overscan(
            grid.grid_clip,
            grid.grid_rect.min,
            grid.row_height,
            grid.cell_width,
            total_rows,
            count,
        );
        let first_frame = window.first_frame.min(count);
        let last_frame = window.last_frame.min(count);
        let row_range = window.row_range(total_rows);
        let geo = Geometry {
            origin_x: grid.grid_rect.left(),
            frame_width: grid.cell_width,
            row_height: grid.row_height,
        };
        let end_x = grid.grid_rect.left() + count as f32 * grid.cell_width;
        let frame_sel = self.frame_range();

        let mut cell_ids: Vec<accesskit::NodeId> = Vec::new();
        let mut detached: HashSet<accesskit::NodeId> = HashSet::new();

        'rows: for li in row_range {
            let Some(&layer_idx) = layer_indices.get(li) else {
                continue;
            };
            let y = grid.grid_rect.top() + li as f32 * grid.row_height;
            let row_rect = Rect::from_min_max(
                Pos2::new(grid.grid_rect.left(), y),
                Pos2::new(end_x, y + grid.row_height),
            );
            if !row_rect.intersects(grid.grid_clip) {
                continue;
            }
            let Some(layer) = self.symbol().timeline.layers.get(layer_idx) else {
                continue;
            };
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            let layer_id = layer.id.clone();
            let layer_name = layer.name.clone();
            let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
            let keyframes = &layer.keyframes;
            if keyframes.is_empty() {
                continue;
            }

            let mut start_ki = keyframes
                .partition_point(|k| k.frame <= first_frame)
                .saturating_sub(1);
            while start_ki > 0 && keyframes[start_ki].keyed_properties.is_some() {
                start_ki -= 1;
            }

            let mut emitted_spans: Vec<(usize, u32, u32, Option<SpanType>)> = Vec::new();
            for (offset, key) in keyframes[start_ki..].iter().enumerate() {
                let ki = start_ki + offset;
                if key.frame > last_frame.saturating_add(1) {
                    break;
                }
                if key.keyed_properties.is_some() {
                    if window.span_intersects(key.frame, key.frame) {
                        emitted_spans.push((ki, key.frame, key.frame + 1, None));
                    }
                    continue;
                }
                let motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
                let next_key = keyframes[ki + 1..]
                    .iter()
                    .find(|k| !(motion && k.keyed_properties.is_some()))
                    .map(|k| k.frame);
                let next = next_key.unwrap_or(layer_end).min(count);
                if next > key.frame && window.span_intersects(key.frame, next) {
                    let span_kind = if key.tween.is_some() {
                        Some(match key.tween_kind {
                            TweenKind::Motion => SpanType::MotionTween,
                            _ if !next_key.is_some_and(|f| f < count) => SpanType::BrokenTween,
                            TweenKind::Shape => SpanType::ShapeTween,
                            TweenKind::Classic => SpanType::ClassicTween,
                        })
                    } else if key.sound.is_some() {
                        Some(SpanType::Sound)
                    } else if !key.elements.is_empty() {
                        Some(SpanType::Static)
                    } else {
                        None
                    };
                    emitted_spans.push((ki, key.frame, next, span_kind));
                }
            }

            let mut pending_nodes = Vec::new();
            for (ki, start, next, span_kind) in emitted_spans {
                if cell_ids.len() + pending_nodes.len() >= MAX_VISIBLE_TIMELINE_A11Y_CELLS {
                    break;
                }
                let raw_span_rect = geo.span(row_rect, start + 1, next.max(start + 1));
                if !raw_span_rect.intersects(grid.grid_clip) {
                    continue;
                }
                let clipped_span = raw_span_rect.intersect(grid.grid_clip);
                if clipped_span.width() <= 0.0 || clipped_span.height() <= 0.0 {
                    continue;
                }

                let key = &self.symbol().timeline.layers[layer_idx].keyframes[ki];
                let has_script = script_frames.binary_search(&start).is_ok()
                    && self
                        .symbol()
                        .timeline
                        .key_carries_frame_script(layer_idx, start);

                let mut summary_parts: Vec<String> = Vec::new();
                if key.keyed_properties.is_some() {
                    summary_parts.push("Property keyframe".to_owned());
                } else if key.elements.is_empty() && key.sound.is_none() {
                    summary_parts.push("Blank keyframe".to_owned());
                } else {
                    summary_parts.push("Keyframe".to_owned());
                }
                if next > start + 1 {
                    summary_parts.push(format!("Frames {}–{}", start + 1, next));
                }
                if let Some(sk) = span_kind {
                    match sk {
                        SpanType::ClassicTween => summary_parts.push("Classic tween".to_owned()),
                        SpanType::MotionTween => summary_parts.push("Motion tween".to_owned()),
                        SpanType::ShapeTween => summary_parts.push("Shape tween".to_owned()),
                        SpanType::BrokenTween => summary_parts.push("Incomplete tween".to_owned()),
                        SpanType::Sound => summary_parts.push("Sound".to_owned()),
                        SpanType::Video => summary_parts.push("Video".to_owned()),
                        SpanType::Static | SpanType::Selected => {}
                    }
                }
                if key.sound.is_some() && span_kind != Some(SpanType::Sound) {
                    summary_parts.push("Sound".to_owned());
                }
                if !key.label.trim().is_empty() {
                    summary_parts.push(format!("Label: {}", key.label.trim()));
                }
                if has_script {
                    summary_parts.push("Script".to_owned());
                }

                let cell_label = format!("Layer {layer_name}, Frame {}", start + 1);
                let cell_desc = summary_parts.join(" · ");

                let in_range_sel = frame_sel.as_ref().is_some_and(|r| {
                    r.layers.contains(&layer_id) && r.start <= start && start <= r.end
                });
                let is_current_playhead_cell = !layers_deselected
                    && self.layer == layer_idx
                    && self.frame >= start
                    && self.frame < next.max(start + 1);
                let cell_selected = in_range_sel || is_current_playhead_cell;

                let cell_egui_id = egui::Id::new((
                    "neoflash_tl_a11y_cell",
                    self.symbol_id(),
                    &layer_id,
                    start,
                    key.keyed_properties.is_some(),
                ));
                pending_nodes.push((
                    cell_egui_id,
                    cell_label,
                    cell_desc,
                    cell_selected,
                    clipped_span,
                    li,
                    start,
                ));
            }

            for (cell_egui_id, cell_label, cell_desc, cell_selected, clipped_span, row_i, start) in
                pending_nodes
            {
                let ak_id = node_id_of(cell_egui_id);
                detached.insert(ak_id);
                cell_ids.push(ak_id);
                ctx.accesskit_node_builder(cell_egui_id, |node| {
                    node.set_role(accesskit::Role::Cell);
                    node.set_label(cell_label);
                    node.set_description(cell_desc);
                    node.set_selected(cell_selected);
                    node.set_row_index(row_i);
                    node.set_column_index(start as usize);
                    node.set_bounds(to_ak_rect(clipped_span));
                    node.add_action(accesskit::Action::Focus);
                    node.add_action(accesskit::Action::Click);
                });
                self.accessibility_bridge.timeline_targets.insert(
                    (vp, ak_id),
                    TimelineActionTarget::KeyframeCell {
                        symbol: active_symbol_id.clone(),
                        layer_id: layer_id.clone(),
                        layer_idx,
                        frame: start,
                    },
                );
            }
            if cell_ids.len() >= MAX_VISIBLE_TIMELINE_A11Y_CELLS {
                break 'rows;
            }
        }

        detach_from_accesskit_root(ctx, &detached);
        ctx.accesskit_node_builder(grid.response_id, |node| {
            node.set_role(accesskit::Role::Grid);
            node.set_label("Timeline frames");
            node.set_row_count(total_rows);
            node.set_column_count(count as usize);
            node.set_bounds(to_ak_rect(grid.grid_clip));
            append_unique_children(node, &cell_ids);
        });
    }

    // =========================================================================
    // 3. LIBRARY ACCESSIBILITY TREE & ACTIONS
    // =========================================================================

    pub(crate) fn handle_library_accesskit_actions(&mut self, ctx: &egui::Context) {
        let vp = ctx.viewport_id();
        let has_requests = ctx.input(|i| {
            i.events.iter().any(|ev| {
                matches!(
                    ev,
                    egui::Event::AccessKitActionRequest(req)
                        if self.accessibility_bridge.library_targets.contains_key(&(vp, req.target))
                )
            })
        });
        if !has_requests {
            return;
        }

        let mut matched = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|ev| {
                if let egui::Event::AccessKitActionRequest(req) = ev
                    && let Some(target) = self
                        .accessibility_bridge
                        .library_targets
                        .get(&(vp, req.target))
                        .cloned()
                {
                    let supported = match target {
                        LibraryActionTarget::Folder { .. } => matches!(
                            req.action,
                            accesskit::Action::Click
                                | accesskit::Action::Focus
                                | accesskit::Action::Expand
                                | accesskit::Action::Collapse
                        ),
                        LibraryActionTarget::FolderDisclosure { .. } => {
                            matches!(
                                req.action,
                                accesskit::Action::Click | accesskit::Action::Focus
                            )
                        }
                        LibraryActionTarget::Symbol { .. }
                        | LibraryActionTarget::Resource { .. } => {
                            matches!(
                                req.action,
                                accesskit::Action::Click | accesskit::Action::Focus
                            )
                        }
                    };
                    if supported {
                        matched.push((req.action, target));
                        return false;
                    }
                }
                true
            });
        });

        let source_active = match self.library_panel.source {
            LibrarySource::ActiveDocument => true,
            LibrarySource::Session(idx) => {
                idx == self.active_session
                    || self.sessions.get(idx).and_then(Option::as_ref).is_none()
            }
            LibrarySource::External => self.library_panel.external_library.is_none(),
        };

        for (action, target) in matched {
            match target {
                LibraryActionTarget::Folder { path } => {
                    let folder_exists = path.is_empty()
                        || self
                            .library_source_project()
                            .library_folder_paths()
                            .contains(&path);
                    if !folder_exists {
                        continue;
                    }
                    let sym_selected = if source_active {
                        self.library_selected.is_some()
                    } else {
                        self.library_panel.source_selected.is_some()
                    };
                    let already_selected_folder =
                        !sym_selected && self.library_panel.selected_folder == path;
                    match action {
                        accesskit::Action::Focus => {
                            self.select_library_folder_from_accesskit(&path, source_active);
                        }
                        accesskit::Action::Expand => {
                            if !path.is_empty() {
                                self.library_panel.collapsed.remove(&path);
                            }
                            self.select_library_folder_from_accesskit(&path, source_active);
                        }
                        accesskit::Action::Collapse => {
                            if !path.is_empty() {
                                self.library_panel.collapsed.insert(path.clone());
                            }
                            self.select_library_folder_from_accesskit(&path, source_active);
                        }
                        accesskit::Action::Click => {
                            if already_selected_folder && !path.is_empty() {
                                if !self.library_panel.collapsed.remove(&path) {
                                    self.library_panel.collapsed.insert(path);
                                }
                            } else {
                                self.select_library_folder_from_accesskit(&path, source_active);
                            }
                        }
                        _ => {}
                    }
                    ctx.request_repaint();
                }
                LibraryActionTarget::FolderDisclosure { path } => {
                    let folder_exists = !path.is_empty()
                        && self
                            .library_source_project()
                            .library_folder_paths()
                            .contains(&path);
                    if action == accesskit::Action::Click && folder_exists {
                        if !self.library_panel.collapsed.remove(&path) {
                            self.library_panel.collapsed.insert(path);
                        }
                        ctx.request_repaint();
                    }
                }
                LibraryActionTarget::Symbol { id } => {
                    let live_folder = {
                        let src = self.library_source_project();
                        src.symbol(&id)
                            .filter(|s| !src.is_scene(&s.id))
                            .map(|s| s.folder.clone())
                    };
                    let Some(live_folder) = live_folder else {
                        continue;
                    };
                    if source_active {
                        self.library_selected = Some(id);
                        self.resource_panel.selected = None;
                    } else {
                        self.library_panel.source_selected = Some(id);
                    }
                    self.library_panel.selected_folder = live_folder;
                    ctx.request_repaint();
                }
                LibraryActionTarget::Resource { id } => {
                    let resource_exists = source_active
                        && self.project.resource(&id).is_some_and(|r| {
                            !crate::panels::productivity_tools::is_internal_system_resource_path(
                                &r.path,
                            )
                        });
                    if resource_exists {
                        self.resource_panel.selected = Some(id);
                        self.resource_panel.baseline = None;
                        self.library_selected = None;
                        ctx.request_repaint();
                    }
                }
            }
        }
    }

    fn select_library_folder_from_accesskit(&mut self, folder: &str, source_active: bool) {
        self.library_panel.selected_folder = folder.to_owned();
        if source_active {
            self.library_selected = None;
            self.resource_panel.selected = None;
        } else {
            self.library_panel.source_selected = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_library_accesskit_tree(
        &mut self,
        ctx: &egui::Context,
        tree_ui_id: egui::Id,
        tree_clip: Rect,
        count_detail: &str,
        source_active: bool,
        rows: &[LibraryRowA11y],
        resource_band: Option<Rect>,
    ) {
        if !is_accesskit_active(ctx) {
            return;
        }
        let vp = ctx.viewport_id();
        self.accessibility_bridge
            .library_targets
            .retain(|(v, _), _| *v != vp);

        for row in rows {
            match row {
                LibraryRowA11y::Root {
                    response_id,
                    rect,
                    selected,
                } => {
                    let ak_id = node_id_of(*response_id);
                    let ak_bounds = clamp_rect_into_viewport(*rect, tree_clip);
                    ctx.accesskit_node_builder(*response_id, |node| {
                        node.set_role(accesskit::Role::TreeItem);
                        node.set_label("Library root");
                        node.set_description("Folder");
                        node.set_expanded(true);
                        node.set_selected(*selected);
                        node.set_level(1);
                        node.set_bounds(ak_bounds);
                        node.add_action(accesskit::Action::Focus);
                        node.add_action(accesskit::Action::Click);
                    });
                    self.accessibility_bridge.library_targets.insert(
                        (vp, ak_id),
                        LibraryActionTarget::Folder {
                            path: String::new(),
                        },
                    );
                }
                LibraryRowA11y::Folder {
                    response_id,
                    disclosure_id,
                    path,
                    depth,
                    expanded,
                    selected,
                    rect,
                } => {
                    let short_name = path.rsplit('/').next().unwrap_or(path.as_str());
                    let ak_id = node_id_of(*response_id);
                    let disc_ak_id = node_id_of(*disclosure_id);
                    let ak_bounds = clamp_rect_into_viewport(*rect, tree_clip);
                    let desc = format!("Folder · {path}");
                    ctx.accesskit_node_builder(*response_id, |node| {
                        node.set_role(accesskit::Role::TreeItem);
                        node.set_label(short_name);
                        node.set_description(desc);
                        node.set_expanded(*expanded);
                        node.set_selected(*selected);
                        node.set_level(depth + 2);
                        node.set_bounds(ak_bounds);
                        node.add_action(accesskit::Action::Focus);
                        node.add_action(accesskit::Action::Click);
                        node.add_action(if *expanded {
                            accesskit::Action::Collapse
                        } else {
                            accesskit::Action::Expand
                        });
                    });
                    let disc_label = if *expanded {
                        format!("Collapse {short_name}")
                    } else {
                        format!("Expand {short_name}")
                    };
                    ctx.accesskit_node_builder(*disclosure_id, |node| {
                        node.set_role(accesskit::Role::Button);
                        node.set_label(disc_label);
                        node.add_action(accesskit::Action::Click);
                    });
                    self.accessibility_bridge.library_targets.insert(
                        (vp, ak_id),
                        LibraryActionTarget::Folder { path: path.clone() },
                    );
                    self.accessibility_bridge.library_targets.insert(
                        (vp, disc_ak_id),
                        LibraryActionTarget::FolderDisclosure { path: path.clone() },
                    );
                }
                LibraryRowA11y::Symbol {
                    response_id,
                    id,
                    name,
                    folder,
                    kind,
                    linkage,
                    class_name,
                    frames,
                    depth,
                    selected,
                    rect,
                } => {
                    let kind_str = match kind {
                        SymbolKind::MovieClip => "MovieClip",
                        SymbolKind::Button => "Button",
                        SymbolKind::Graphic => "Graphic",
                    };
                    let mut desc = format!(
                        "{kind_str} · {frames} frame{}",
                        if *frames == 1 { "" } else { "s" }
                    );
                    if !folder.is_empty() {
                        desc.push_str(&format!(" · Folder: {folder}"));
                    }
                    if !linkage.is_empty() {
                        desc.push_str(&format!(" · Linkage: {linkage}"));
                    }
                    if !class_name.is_empty() {
                        desc.push_str(&format!(" · Class: {class_name}"));
                    }
                    let ak_id = node_id_of(*response_id);
                    let ak_bounds = clamp_rect_into_viewport(*rect, tree_clip);
                    ctx.accesskit_node_builder(*response_id, |node| {
                        node.set_role(accesskit::Role::TreeItem);
                        node.set_label(name.clone());
                        node.set_description(desc);
                        node.set_selected(*selected);
                        node.set_level(depth + 2);
                        node.set_bounds(ak_bounds);
                        node.add_action(accesskit::Action::Focus);
                        node.add_action(accesskit::Action::Click);
                    });
                    self.accessibility_bridge
                        .library_targets
                        .insert((vp, ak_id), LibraryActionTarget::Symbol { id: id.clone() });
                }
            }
        }

        // Emit semantic `Role::TreeItem` nodes for active-document Library resources
        // (`Bitmap` / `Audio` / `Video` / `Font`) without modifying `resources.rs`.
        let mut resource_node_ids = Vec::new();
        let mut detached = HashSet::new();
        if source_active {
            let fallback_band = resource_band.unwrap_or(tree_clip);
            let ak_res_bounds = clamp_rect_into_viewport(fallback_band, tree_clip);
            for unit in self
                .project
                .resources
                .iter()
                .filter(|r| {
                    !crate::panels::productivity_tools::is_internal_system_resource_path(&r.path)
                })
                .take(MAX_LIBRARY_RESOURCE_A11Y_NODES)
            {
                let kind_str = classify_resource_kind_label(unit);
                let desc = format!("{kind_str} · {}", unit.media_type);
                let is_sel = self.resource_panel.selected.as_ref() == Some(&unit.id);
                let res_egui_id = egui::Id::new(("neoflash_lib_a11y_res", &unit.id));
                let ak_id = node_id_of(res_egui_id);
                detached.insert(ak_id);
                resource_node_ids.push(ak_id);
                let res_name = unit.path.clone();
                ctx.accesskit_node_builder(res_egui_id, |node| {
                    node.set_role(accesskit::Role::TreeItem);
                    node.set_label(res_name);
                    node.set_description(desc);
                    node.set_selected(is_sel);
                    node.set_level(2);
                    node.set_bounds(ak_res_bounds);
                    node.add_action(accesskit::Action::Focus);
                    node.add_action(accesskit::Action::Click);
                });
                self.accessibility_bridge.library_targets.insert(
                    (vp, ak_id),
                    LibraryActionTarget::Resource {
                        id: unit.id.clone(),
                    },
                );
            }
        }

        detach_from_accesskit_root(ctx, &detached);
        let desc = count_detail.to_owned();
        ctx.accesskit_node_builder(tree_ui_id, |node| {
            node.set_role(accesskit::Role::Tree);
            node.set_label("Library");
            node.set_description(desc);
            node.set_bounds(to_ak_rect(tree_clip));
            append_unique_children(node, &resource_node_ids);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_map(update: &accesskit::TreeUpdate) -> HashMap<accesskit::NodeId, &accesskit::Node> {
        update.nodes.iter().map(|(id, n)| (*id, n)).collect()
    }

    fn assert_valid_single_parent_tree(update: &accesskit::TreeUpdate) {
        let map = node_map(update);
        let root_id = update.tree.as_ref().expect("tree root present").root;
        assert!(map.contains_key(&root_id), "root must exist in nodes");
        let mut parent_of: HashMap<accesskit::NodeId, accesskit::NodeId> = HashMap::new();
        for (pid, pnode) in &update.nodes {
            for cid in pnode.children() {
                assert!(
                    map.contains_key(cid),
                    "child {cid:?} of {pid:?} ({:?}) missing from TreeUpdate.nodes",
                    pnode.label()
                );
                let prev = parent_of.insert(*cid, *pid);
                assert!(
                    prev.is_none(),
                    "node {cid:?} ({:?}) has duplicate parents: {prev:?} and {pid:?}",
                    map.get(cid).and_then(|n| n.label())
                );
            }
        }
    }

    fn assert_finite_positive_bounds(node: &accesskit::Node, context: &str) -> accesskit::Rect {
        let b = node
            .bounds()
            .unwrap_or_else(|| panic!("{context}: expected finite bounds on {:?}", node.label()));
        assert!(
            b.x0.is_finite()
                && b.y0.is_finite()
                && b.x1.is_finite()
                && b.y1.is_finite()
                && b.x1 > b.x0
                && b.y1 > b.y0,
            "{context}: invalid bounds {b:?} on {:?}",
            node.label()
        );
        b
    }

    #[test]
    fn stage_semantic_tree_exposes_roles_bounds_tab_index_silent_and_force_simple() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let btn_sym = Symbol {
            id: "sym-btn".into(),
            name: "PlayBtnSym".into(),
            folder: String::new(),
            kind: SymbolKind::Button,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "btn-layer".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "btn-rect".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 120.0,
                                height: 44.0,
                                fill: [40, 120, 220, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };

        let hud_mc = Symbol {
            id: "sym-hud".into(),
            name: "HudClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "hud-layer".into(),
                    name: "Controls".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "nested-mute-btn".into(),
                            name: "MuteAudioBtn".into(),
                            transform: Transform {
                                x: 10.0,
                                y: 8.0,
                                ..Default::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-btn".into(),
                                playback: InstancePlayback::SingleFrame(0),
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };

        let mut input_fmt = TextFormat::default();
        let mut input_opts = input_fmt.field_options().unwrap_or_default();
        input_opts.field_type = TextFieldType::Input;
        input_opts.selectable = true;
        input_fmt.set_field_options(Some(input_opts));

        let mut dyn_fmt = TextFormat::default();
        let mut dyn_opts = dyn_fmt.field_options().unwrap_or_default();
        dyn_opts.field_type = TextFieldType::Dynamic;
        dyn_opts.selectable = true;
        dyn_fmt.set_field_options(Some(dyn_opts));

        let mut static_fmt = TextFormat::default();
        let mut static_opts = static_fmt.field_options().unwrap_or_default();
        static_opts.field_type = TextFieldType::Dynamic;
        static_opts.selectable = false;
        static_fmt.set_field_options(Some(static_opts));

        let btn_el = Element {
            id: "el-start-btn".into(),
            name: "startBtn".into(),
            transform: Transform {
                x: 60.0,
                y: 80.0,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-btn".into(),
                playback: InstancePlayback::SingleFrame(0),
            },
            accessibility: Some(Accessibility {
                name: "Start Game".into(),
                description: "Launches level one".into(),
                role: AccessibleRole::Button,
                tab_order: 2,
                disabled: false,
            }),
            ..Default::default()
        };

        let input_el = Element {
            id: "el-player-input".into(),
            name: "playerNameInput".into(),
            transform: Transform {
                x: 240.0,
                y: 260.0,
                ..Default::default()
            },
            kind: ElementKind::TextBox {
                text: "Pilot42".into(),
                width: 160.0,
                height: 32.0,
                format: input_fmt,
                word_wrap: false,
                multiline: false,
            },
            accessibility: Some(Accessibility {
                name: "Player Handle".into(),
                description: "Enter pilot callsign".into(),
                role: AccessibleRole::Button,
                tab_order: 1,
                disabled: false,
            }),
            ..Default::default()
        };

        let dyn_el = Element {
            id: "el-score-dyn".into(),
            name: "scoreField".into(),
            transform: Transform {
                x: 60.0,
                y: 20.0,
                ..Default::default()
            },
            kind: ElementKind::TextBox {
                text: "Score: 9000".into(),
                width: 140.0,
                height: 28.0,
                format: dyn_fmt,
                word_wrap: false,
                multiline: false,
            },
            ..Default::default()
        };

        let static_el = Element {
            id: "el-banner-static".into(),
            name: String::new(),
            transform: Transform {
                x: 260.0,
                y: 20.0,
                ..Default::default()
            },
            kind: ElementKind::TextBox {
                text: "NEO ARCADE".into(),
                width: 150.0,
                height: 28.0,
                format: static_fmt,
                word_wrap: false,
                multiline: false,
            },
            ..Default::default()
        };

        let hud_el = Element {
            id: "el-hud-mc".into(),
            name: "hudContainer".into(),
            transform: Transform {
                x: 60.0,
                y: 340.0,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-hud".into(),
                playback: InstancePlayback::Independent,
            },
            ..Default::default()
        };

        let silent_el = Element {
            id: "el-secret-silent".into(),
            name: "HiddenSecretButton".into(),
            transform: Transform {
                x: 400.0,
                y: 80.0,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-btn".into(),
                playback: InstancePlayback::SingleFrame(0),
            },
            accessibility: Some(Accessibility {
                name: "ShouldNeverAppear".into(),
                description: String::new(),
                role: AccessibleRole::Button,
                tab_order: 3,
                disabled: false,
            }),
            ..Default::default()
        };

        let mut kf = Keyframe {
            frame: 0,
            elements: vec![btn_el, input_el, dyn_el, static_el, hud_el, silent_el],
            ..Default::default()
        };
        kf.set_extended_accessibility(ExtendedAccessibility {
            element_id: "el-start-btn".into(),
            shortcut: "Alt+S".into(),
            ..Default::default()
        });
        kf.set_extended_accessibility(ExtendedAccessibility {
            element_id: "el-secret-silent".into(),
            silent: true,
            ..Default::default()
        });

        let mut project = Project::blank();
        project.format_version = 9;
        project.symbols[0].timeline.layers[0].keyframes = vec![kf];
        project.symbols.push(btn_sym);
        project.symbols.push(hud_mc);
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.select_single_id("el-start-btn".into());

        let render_stage = |ed: &mut Editor| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                    ..Default::default()
                },
                |ctx| ed.stage(ctx),
            )
        };

        let out = render_stage(&mut editor);
        let ak = out
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit update emitted");
        assert_valid_single_parent_tree(ak);

        let map = node_map(ak);
        let (&stage_nid, &stage_node) = map
            .iter()
            .find(|(_, n)| n.label() == Some("Stage") && n.role() == accesskit::Role::Region)
            .expect("Stage region node present");
        let stage_bounds = assert_finite_positive_bounds(stage_node, "Stage container");
        assert!(
            stage_node
                .description()
                .is_some_and(|d| d.contains("Frame 1 of") && d.contains("Zoom")),
            "Stage description={:?}",
            stage_node.description()
        );

        // Silent element MUST NOT appear anywhere in `accesskit_update`.
        assert!(
            !map.values().any(|n| {
                n.label() == Some("ShouldNeverAppear") || n.label() == Some("HiddenSecretButton")
            }),
            "silent: true element must be excluded from AccessKit tree"
        );

        let child_labels_and_roles: Vec<(Option<&str>, accesskit::Role)> = stage_node
            .children()
            .iter()
            .filter_map(|cid| map.get(cid).copied())
            .map(|n| (n.label(), n.role()))
            .collect();

        // Explicit positive `tab_index` ordering:
        // 1st: `Player Handle` (`tab_order: 1`, even though lower on Stage at y=260!)
        // 2nd: `Start Game` (`tab_order: 2`, at y=80)
        // Remaining unindexed items in spatial `(top, left)` order: `scoreField`, `NEO ARCADE`, `hudContainer`.
        let pos_input = child_labels_and_roles
            .iter()
            .position(|(l, _)| *l == Some("Player Handle"))
            .expect("Player Handle child of Stage");
        let pos_btn = child_labels_and_roles
            .iter()
            .position(|(l, _)| *l == Some("Start Game"))
            .expect("Start Game child of Stage");
        let pos_score = child_labels_and_roles
            .iter()
            .position(|(l, _)| *l == Some("scoreField"))
            .expect("scoreField child of Stage");
        let pos_banner = child_labels_and_roles
            .iter()
            .position(|(l, _)| *l == Some("NEO ARCADE"))
            .expect("NEO ARCADE child of Stage");
        let pos_hud = child_labels_and_roles
            .iter()
            .position(|(l, _)| *l == Some("hudContainer"))
            .expect("hudContainer child of Stage");

        assert!(
            pos_input < pos_btn
                && pos_btn < pos_score
                && pos_score < pos_banner
                && pos_banner < pos_hud,
            "expected tab_index then spatial (top, left) child order, got: {child_labels_and_roles:?}"
        );

        let find_stage_child = |lbl: &str| -> &accesskit::Node {
            stage_node
                .children()
                .iter()
                .filter_map(|cid| map.get(cid).copied())
                .find(|n| n.label() == Some(lbl))
                .unwrap_or_else(|| panic!("missing Stage child {lbl}"))
        };

        let start_node = find_stage_child("Start Game");
        assert_eq!(start_node.role(), accesskit::Role::Button);
        assert_eq!(start_node.description(), Some("Launches level one"));
        assert_eq!(start_node.keyboard_shortcut(), Some("Alt+S"));
        assert_eq!(start_node.is_selected(), Some(true));
        let start_b = assert_finite_positive_bounds(start_node, "Start Game");
        assert!(start_b.x0 >= stage_bounds.x0 && start_b.x1 <= stage_bounds.x1);

        let input_node = find_stage_child("Player Handle");
        assert_eq!(input_node.role(), accesskit::Role::TextInput);
        assert_eq!(input_node.value(), Some("Pilot42"));
        assert_eq!(input_node.description(), Some("Enter pilot callsign"));
        assert_eq!(input_node.is_selected(), Some(false));
        assert_finite_positive_bounds(input_node, "Player Handle");

        let score_node = find_stage_child("scoreField");
        assert_eq!(score_node.role(), accesskit::Role::Region);
        assert_eq!(score_node.value(), Some("Score: 9000"));

        let banner_node = find_stage_child("NEO ARCADE");
        assert_eq!(banner_node.role(), accesskit::Role::Label);
        assert_eq!(banner_node.value(), Some("NEO ARCADE"));

        // `hudContainer` (`force_simple == false`) exposes nested `MuteAudioBtn` child!
        let hud_node = find_stage_child("hudContainer");
        assert_eq!(hud_node.role(), accesskit::Role::Group);
        assert_eq!(hud_node.children().len(), 1);
        let nested_mute = map
            .get(&hud_node.children()[0])
            .expect("nested child in TreeUpdate");
        assert_eq!(nested_mute.label(), Some("MuteAudioBtn"));
        assert_eq!(nested_mute.role(), accesskit::Role::Button);
        assert_finite_positive_bounds(nested_mute, "MuteAudioBtn");
        let _ = stage_nid;

        // Now set `force_simple: true` on `el-hud-mc` and verify nested child is suppressed!
        editor.symbol_mut().timeline.layers[0].keyframes[0].set_extended_accessibility(
            ExtendedAccessibility {
                element_id: "el-hud-mc".into(),
                force_simple: true,
                ..Default::default()
            },
        );
        let out_simple = render_stage(&mut editor);
        let ak_simple = out_simple
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        assert_valid_single_parent_tree(ak_simple);
        let map_simple = node_map(ak_simple);
        assert!(
            !map_simple
                .values()
                .any(|n| n.label() == Some("MuteAudioBtn")),
            "force_simple: true must suppress MovieClip child nodes"
        );
        let hud_simple = map_simple
            .values()
            .find(|n| n.label() == Some("hudContainer"))
            .expect("hudContainer itself remains exposed");
        assert!(hud_simple.children().is_empty());
    }

    #[test]
    fn stage_accesskit_actions_select_in_authoring_and_activate_in_live_preview() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let btn_sym = Symbol {
            id: "sym-action-btn".into(),
            name: "ActionBtn".into(),
            folder: String::new(),
            kind: SymbolKind::Button,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "l".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "r".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 80.0,
                                height: 36.0,
                                fill: [200, 80, 40, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };

        let mut project = Project::blank();
        project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
            frame: 0,
            elements: vec![
                Element {
                    id: "el-btn-a".into(),
                    name: "btnAlpha".into(),
                    transform: Transform {
                        x: 40.0,
                        y: 40.0,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-action-btn".into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::Click,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "trace(\"a11y_alpha_clicked\");".into(),
                        },
                    }],
                    ..Default::default()
                },
                Element {
                    id: "el-btn-b".into(),
                    name: "btnBeta".into(),
                    transform: Transform {
                        x: 200.0,
                        y: 40.0,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-action-btn".into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    ..Default::default()
                },
            ],
            ..Default::default()
        }];
        project.symbols.push(btn_sym);
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.history.clear();

        let run_stage = |ed: &mut Editor, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 768.0))),
                    events,
                    ..Default::default()
                },
                |ctx| ed.stage(ctx),
            )
        };

        let out0 = run_stage(&mut editor, vec![]);
        let ak0 = out0.platform_output.accesskit_update.as_ref().unwrap();
        let find_nid = |ak: &accesskit::TreeUpdate, label: &str| -> accesskit::NodeId {
            ak.nodes
                .iter()
                .find_map(|(id, n)| (n.label() == Some(label)).then_some(*id))
                .unwrap_or_else(|| panic!("missing node {label}"))
        };
        let alpha_nid = find_nid(ak0, "btnAlpha");
        let beta_nid = find_nid(ak0, "btnBeta");
        assert!(editor.selection.is_empty());

        // 1. Authoring `Action::Focus` selects `el-btn-b` with 0 Undo steps.
        let out_focus = run_stage(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Focus,
                    target: beta_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(
            editor.selection.single().map(|t| t.element.as_str()),
            Some("el-btn-b")
        );
        assert!(!editor.history.can_undo());
        let ak_focus = out_focus.platform_output.accesskit_update.as_ref().unwrap();
        assert_eq!(
            node_map(ak_focus).get(&beta_nid).unwrap().is_selected(),
            Some(true)
        );

        // 2. Authoring `Action::Click` switches selection to `el-btn-a` with 0 Undo steps.
        let out_click = run_stage(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: alpha_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(
            editor.selection.single().map(|t| t.element.as_str()),
            Some("el-btn-a")
        );
        assert!(!editor.history.can_undo());
        let ak_click = out_click.platform_output.accesskit_update.as_ref().unwrap();
        assert_eq!(
            node_map(ak_click).get(&alpha_nid).unwrap().is_selected(),
            Some(true)
        );

        // 3. Live `playing` mode `Action::Click` dispatches the button's Click script!
        editor.set_playing(true);
        editor.sync_authored_preview();
        let _ = run_stage(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: alpha_nid,
                    data: None,
                },
            )],
        );
        assert!(
            editor
                .output
                .iter()
                .any(|line| line.contains("a11y_alpha_clicked")),
            "expected live preview trace from AccessKit Click, got: {:?}",
            editor.output.iter().collect::<Vec<_>>()
        );
        assert!(!editor.history.can_undo());
    }

    #[test]
    fn timeline_accesskit_exposes_playhead_layers_and_keyframes_and_executes_actions() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let mut project = Project::blank();
        project.format_version = 6;
        let wav_bytes = b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00";
        let audio_res = ResourceUnit::from_bytes("res-sfx", "sfx/beep.wav", "audio/wav", wav_bytes)
            .expect("valid wav resource");
        project.resources.push(audio_res);
        project.symbols[0].timeline.frame_count = 20;
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: Some("layer-art".into()),
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "stop();".into(),
            },
        });
        project.symbols[0].timeline.layers = vec![
            Layer {
                id: "layer-audio".into(),
                name: "SFX".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound::new("res-sfx")),
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            },
            Layer {
                id: "layer-guide".into(),
                name: "PathGuide".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties {
                    role: LayerRole::Guide,
                    ..LayerProperties::default()
                },
            },
            Layer {
                id: "layer-mask".into(),
                name: "Vignette".into(),
                visible: false,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    ..LayerProperties::default()
                },
            },
            Layer {
                id: "layer-folder".into(),
                name: "Actors".into(),
                visible: true,
                locked: false,
                keyframes: Vec::new(),
                properties: LayerProperties {
                    role: LayerRole::Folder,
                    collapsed: false,
                    ..LayerProperties::default()
                },
            },
            Layer {
                id: "layer-art".into(),
                name: "HeroArt".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "intro".into(),
                        tween: Some(Easing::Linear),
                        tween_kind: TweenKind::Classic,
                        elements: vec![Element {
                            id: "hero-box".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 30.0,
                                fill: [60, 140, 220, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 9,
                        elements: vec![Element {
                            id: "hero-box".into(),
                            name: String::new(),
                            transform: Transform {
                                x: 120.0,
                                y: 0.0,
                                ..Default::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 30.0,
                                fill: [60, 140, 220, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                ],
                properties: LayerProperties::default(),
            },
        ];
        project.upgrade_format_for_features();
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.layer = 4;
        editor.history.clear();

        let run_tl = |ed: &mut Editor, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 360.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| ed.timeline_contents(ui));
                },
            )
        };

        let out = run_tl(&mut editor, vec![]);
        let ak = out.platform_output.accesskit_update.as_ref().unwrap();
        assert_valid_single_parent_tree(ak);
        let map = node_map(ak);

        // 1. Playhead slider node (`value()` must remain `None` so `accesskit_macos` returns
        //    `NSNumber(numeric_value)` on `NSAccessibilitySliderRole`).
        let (&playhead_nid, &playhead_node) = map
            .iter()
            .find(|(_, n)| n.label() == Some("Playhead") && n.role() == accesskit::Role::Slider)
            .expect("Playhead Slider node present");
        assert_eq!(playhead_node.value(), None);
        assert_eq!(playhead_node.numeric_value(), Some(1.0));
        assert_eq!(playhead_node.min_numeric_value(), Some(1.0));
        assert_eq!(playhead_node.max_numeric_value(), Some(20.0));
        assert!(
            playhead_node
                .description()
                .is_some_and(|d| d.contains("Frame 1 of 20") && d.contains("fps")),
            "Playhead desc={:?}",
            playhead_node.description()
        );
        assert_finite_positive_bounds(playhead_node, "Playhead");

        // 2. Layer rows (`Role::TreeItem` parented under `"Timeline layers"` `Role::Tree`)
        let (_, &layers_tree_node) = map
            .iter()
            .find(|(_, n)| {
                n.label() == Some("Timeline layers") && n.role() == accesskit::Role::Tree
            })
            .expect("Timeline layers Role::Tree container present");
        assert_finite_positive_bounds(layers_tree_node, "Timeline layers tree");
        let find_layer_row = |prefix: &str| -> (accesskit::NodeId, &accesskit::Node) {
            map.iter()
                .find(|(_, n)| {
                    n.role() == accesskit::Role::TreeItem
                        && n.label().is_some_and(|l| l.starts_with(prefix))
                })
                .map(|(id, n)| (*id, *n))
                .unwrap_or_else(|| panic!("missing layer TreeItem {prefix}"))
        };
        let (art_nid, art_row) = find_layer_row("HeroArt (");
        assert_eq!(art_row.is_selected(), Some(true));
        assert!(
            art_row.description().is_some_and(|d| d.contains("Normal")
                && d.contains("Visible")
                && d.contains("Unlocked")),
            "art_row desc={:?}",
            art_row.description()
        );

        let (guide_nid, guide_row) = find_layer_row("PathGuide (");
        assert_eq!(guide_row.is_selected(), Some(false));
        assert!(
            guide_row
                .description()
                .is_some_and(|d| d.contains("Guide") && d.contains("Locked")),
            "guide_row desc={:?}",
            guide_row.description()
        );

        let (mask_nid, mask_row) = find_layer_row("Vignette (");
        assert!(
            mask_row
                .description()
                .is_some_and(|d| d.contains("Mask") && d.contains("Hidden"))
        );

        let (folder_nid, folder_row) = find_layer_row("Actors (");
        assert!(
            folder_row
                .description()
                .is_some_and(|d| d.contains("Folder"))
        );
        assert_eq!(folder_row.is_expanded(), Some(true));

        let (audio_nid, audio_row) = find_layer_row("SFX (");
        assert!(audio_row.description().is_some_and(|d| d.contains("Audio")));

        for layer_nid in [art_nid, folder_nid, mask_nid, guide_nid, audio_nid] {
            assert!(
                layers_tree_node.children().contains(&layer_nid),
                "Timeline layer TreeItem {layer_nid:?} must be parented under `Timeline layers` Role::Tree"
            );
        }

        // 3. Keyframe span cells under `Timeline frames` `Role::Grid`
        let (_, &grid_node) = map
            .iter()
            .find(|(_, n)| {
                n.label() == Some("Timeline frames") && n.role() == accesskit::Role::Grid
            })
            .expect("Timeline frames Grid node present");
        let find_cell = |lbl: &str| -> (accesskit::NodeId, &accesskit::Node) {
            grid_node
                .children()
                .iter()
                .filter_map(|cid| map.get(cid).map(|n| (*cid, *n)))
                .find(|(_, n)| n.label() == Some(lbl))
                .unwrap_or_else(|| panic!("missing keyframe cell {lbl}"))
        };
        let (_, hero_f1) = find_cell("Layer HeroArt, Frame 1");
        assert_eq!(hero_f1.role(), accesskit::Role::Cell);
        assert_finite_positive_bounds(hero_f1, "HeroArt Frame 1");
        let f1_desc = hero_f1.description().unwrap_or("");
        assert!(
            f1_desc.contains("Classic tween")
                && f1_desc.contains("Label: intro")
                && f1_desc.contains("Script"),
            "expected tween + label + script summary, got {f1_desc:?}"
        );
        let (hero_f10_nid, hero_f10) = find_cell("Layer HeroArt, Frame 10");
        assert_finite_positive_bounds(hero_f10, "HeroArt Frame 10");

        let (_, sfx_f1) = find_cell("Layer SFX, Frame 1");
        assert!(
            sfx_f1.description().unwrap_or("").contains("Sound"),
            "expected Sound summary on SFX Frame 1, got {:?}",
            sfx_f1.description()
        );

        // 4. Execute Playhead Increment / Decrement / SetValue without mutating Undo history!
        let _ = run_tl(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Increment,
                    target: playhead_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.frame, 1);
        assert!(!editor.history.can_undo());

        let _ = run_tl(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Decrement,
                    target: playhead_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.frame, 0);
        assert!(!editor.history.can_undo());

        let out_set = run_tl(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::SetValue,
                    target: playhead_nid,
                    data: Some(accesskit::ActionData::NumericValue(7.0)),
                },
            )],
        );
        assert_eq!(
            editor.frame, 6,
            "1-based SetValue(7.0) seeks 0-based frame 6"
        );
        assert!(!editor.history.can_undo());
        let set_map = node_map(out_set.platform_output.accesskit_update.as_ref().unwrap());
        let set_playhead = set_map.get(&playhead_nid).unwrap();
        assert_eq!(set_playhead.value(), None);
        assert_eq!(set_playhead.numeric_value(), Some(7.0));

        // 5. Execute LayerRow `Action::Click` -> switches `editor.layer` to `PathGuide` (index 1).
        let _ = run_tl(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: guide_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.layer, 1);
        assert!(!editor.history.can_undo());

        // 6. Execute KeyframeCell `Action::Click` on `Layer HeroArt, Frame 10` -> selects layer 4, frame 9!
        let _ = run_tl(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: hero_f10_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.layer, 4);
        assert_eq!(editor.frame, 9);
        assert!(!editor.history.can_undo());

        // 7. Switching to a new scene invalidates stale scene-1 Timeline action targets cleanly!
        editor.new_scene();
        editor.history.clear();
        assert_eq!(editor.frame, 0);
        assert_eq!(editor.layer, 0);
        let _ = run_tl(
            &mut editor,
            vec![
                egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: hero_f10_nid,
                    data: None,
                }),
                egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
                    action: accesskit::Action::SetValue,
                    target: playhead_nid,
                    data: Some(accesskit::ActionData::NumericValue(15.0)),
                }),
            ],
        );
        assert_eq!(editor.frame, 0);
        assert_eq!(editor.layer, 0);
        assert!(!editor.history.can_undo());
    }

    #[test]
    fn thousand_layer_timeline_accesskit_bounds_emitted_nodes_to_visible_viewport() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let mut project = Project::blank();
        project.symbols[0].timeline.frame_count = 1_000;
        project.symbols[0].timeline.layers = (0..1_000)
            .map(|i| Layer {
                id: format!("l_{i}"),
                name: format!("Layer {i}"),
                visible: true,
                locked: false,
                keyframes: (0..20)
                    .map(|k| Keyframe {
                        frame: k * 50,
                        ..Default::default()
                    })
                    .collect(),
                properties: LayerProperties::default(),
            })
            .collect();

        let mut editor = Editor::from_project(&ctx, project);
        let run_tl = |ed: &mut Editor| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 320.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| ed.timeline_contents(ui));
                },
            )
        };

        // Warmup pass + timed pass
        let _ = run_tl(&mut editor);
        let t0 = std::time::Instant::now();
        let out = run_tl(&mut editor);
        let elapsed = t0.elapsed();

        let ak = out.platform_output.accesskit_update.as_ref().unwrap();
        assert_valid_single_parent_tree(ak);

        let layer_tree_items = ak
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == accesskit::Role::TreeItem)
            .count();
        let keyframe_cells = ak
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == accesskit::Role::Cell)
            .count();

        assert!(
            layer_tree_items > 0 && layer_tree_items <= 32,
            "1,000-layer timeline must only emit visible layer TreeItems, got {layer_tree_items}"
        );
        assert!(
            keyframe_cells > 0 && keyframe_cells <= MAX_VISIBLE_TIMELINE_A11Y_CELLS,
            "1,000x1,000 timeline must cap keyframe cells to <= {MAX_VISIBLE_TIMELINE_A11Y_CELLS}, got {keyframe_cells}"
        );
        assert!(
            ak.nodes.len() < 750,
            "total AccessKit nodes on 1,000x1,000 timeline must stay O(visible), got {}",
            ak.nodes.len()
        );
        assert!(
            elapsed.as_millis() < 100,
            "1,000x1,000 timeline pass with AccessKit stayed fast ({elapsed:?})"
        );
    }

    #[test]
    fn library_accesskit_exposes_tree_folders_symbols_resources_and_executes_actions() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let mut project = Project::blank();
        project.add_library_folder("UI").unwrap();
        project.symbols.push(Symbol {
            id: "sym-hero-mc".into(),
            name: "HeroRunner".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 8,
                layers: vec![Layer {
                    id: "l1".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: Some("game.HeroRunner".into()),
            linkage_id: Some("hero_runner".into()),
        });
        project.symbols.push(Symbol {
            id: "sym-ui-btn".into(),
            name: "PauseButton".into(),
            folder: "UI".into(),
            kind: SymbolKind::Button,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "l2".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        let png_1x1: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        project.resources.push(
            ResourceUnit::from_bytes("res-atlas", "sprites/atlas.png", "image/png", png_1x1)
                .unwrap(),
        );
        project.format_version = 6;
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.history.clear();

        let run_lib = |ed: &mut Editor, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(340.0, 640.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| ed.library_body(ui));
                },
            )
        };

        let out = run_lib(&mut editor, vec![]);
        let ak = out.platform_output.accesskit_update.as_ref().unwrap();
        assert_valid_single_parent_tree(ak);
        let map = node_map(ak);

        let (_, &tree_node) = map
            .iter()
            .find(|(_, n)| n.label() == Some("Library") && n.role() == accesskit::Role::Tree)
            .expect("Library Role::Tree container present");
        assert_finite_positive_bounds(tree_node, "Library tree");

        fn find_tree_item<'a>(
            ak_map: &HashMap<accesskit::NodeId, &'a accesskit::Node>,
            lbl: &str,
        ) -> (accesskit::NodeId, &'a accesskit::Node) {
            ak_map
                .iter()
                .find(|(_, n)| n.role() == accesskit::Role::TreeItem && n.label() == Some(lbl))
                .map(|(id, n)| (*id, *n))
                .unwrap_or_else(|| panic!("missing Library TreeItem {lbl}"))
        }

        let (root_item_nid, _) = find_tree_item(&map, "Library root");
        let (ui_folder_nid, ui_folder) = find_tree_item(&map, "UI");
        assert_eq!(ui_folder.is_expanded(), Some(true));
        assert!(ui_folder.description().unwrap_or("").contains("Folder"));
        assert_finite_positive_bounds(ui_folder, "UI folder");

        let (hero_nid, hero_item) = find_tree_item(&map, "HeroRunner");
        let hero_desc = hero_item.description().unwrap_or("");
        assert!(
            hero_desc.contains("MovieClip")
                && hero_desc.contains("Linkage: hero_runner")
                && hero_desc.contains("Class: game.HeroRunner"),
            "hero_desc={hero_desc:?}"
        );
        assert_finite_positive_bounds(hero_item, "HeroRunner");

        let (pause_item_nid, pause_item) = find_tree_item(&map, "PauseButton");
        assert!(pause_item.description().unwrap_or("").contains("Button"));

        let (atlas_nid, atlas_item) = find_tree_item(&map, "sprites/atlas.png");
        assert!(atlas_item.description().unwrap_or("").contains("Bitmap"));
        for child_nid in [
            root_item_nid,
            ui_folder_nid,
            hero_nid,
            pause_item_nid,
            atlas_nid,
        ] {
            assert!(
                tree_node.children().contains(&child_nid),
                "Library TreeItem {child_nid:?} must be parented under `Library` Role::Tree"
            );
        }

        // 1. `Action::Click` on `HeroRunner` selects the symbol!
        let out_sel_sym = run_lib(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: hero_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.library_selected.as_deref(), Some("sym-hero-mc"));
        let map_sel = node_map(
            out_sel_sym
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap(),
        );
        assert_eq!(
            find_tree_item(&map_sel, "HeroRunner").1.is_selected(),
            Some(true)
        );

        // 2. `Action::Click` on `sprites/atlas.png` selects the resource!
        let _ = run_lib(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: atlas_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.resource_panel.selected.as_deref(), Some("res-atlas"));
        assert!(editor.library_selected.is_none());

        // 3. `Action::Click` on `UI` folder selects it, and a 2nd `Action::Click` collapses it (`PauseButton` hidden)!
        let _ = run_lib(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: ui_folder_nid,
                    data: None,
                },
            )],
        );
        assert_eq!(editor.library_panel.selected_folder, "UI");

        let out_collapsed = run_lib(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: ui_folder_nid,
                    data: None,
                },
            )],
        );
        let ak_col = out_collapsed
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        assert_valid_single_parent_tree(ak_col);
        let map_col = node_map(ak_col);
        assert_eq!(find_tree_item(&map_col, "UI").1.is_expanded(), Some(false));
        assert!(
            !map_col.values().any(|n| n.label() == Some("PauseButton")),
            "collapsed folder children must not appear in AccessKit tree"
        );

        // 4. `Action::Expand` re-expands `UI`!
        let out_expanded = run_lib(
            &mut editor,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Expand,
                    target: ui_folder_nid,
                    data: None,
                },
            )],
        );
        let map_exp = node_map(
            out_expanded
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap(),
        );
        assert_eq!(find_tree_item(&map_exp, "UI").1.is_expanded(), Some(true));
        assert!(!editor.history.can_undo());

        // 5. Stale action requests targeting a deleted symbol or resource are safely ignored!
        editor.project.symbols.retain(|s| s.id != "sym-hero-mc");
        editor.project.resources.retain(|r| r.id != "res-atlas");
        editor.library_selected = None;
        editor.resource_panel.selected = None;
        let _ = run_lib(
            &mut editor,
            vec![
                egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: hero_nid,
                    data: None,
                }),
                egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target: atlas_nid,
                    data: None,
                }),
            ],
        );
        assert!(editor.library_selected.is_none());
        assert!(editor.resource_panel.selected.is_none());
    }

    #[test]
    fn full_editor_accesskit_tree_has_single_parent_per_node_and_valid_project() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.show_panel(crate::workspace::PanelId::Library);
        editor.show_panel(crate::workspace::PanelId::Timeline);

        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 960.0))),
                ..Default::default()
            },
            |ctx| {
                editor.dock_grid(ctx);
                editor.document_bar(ctx);
                editor.stage(ctx);
            },
        );
        let ak = out.platform_output.accesskit_update.as_ref().unwrap();
        assert_valid_single_parent_tree(ak);
        editor.project.validate().unwrap();
    }
}
