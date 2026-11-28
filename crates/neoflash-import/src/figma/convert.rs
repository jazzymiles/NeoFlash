#![forbid(unsafe_code)]
//! Lowering [`FigmaDoc`] into layered [`neoflash_core::Project`] compositions, multi-page scenes,
//! bounded bitmap resources, and reusable Library `MovieClip` symbols (`T-249`, `T-312`).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageFormat, ImageReader, Limits};
use neoflash_core::MAX_BITMAP_DIMENSION;

use super::page_layout::PageLayout;
use super::*;

/// Maximum distinct bitmap resources imported into `Project.resources` (`validation.rs` caps at 256).
const MAX_IMPORTED_BITMAPS: usize = 256;
/// Directly placed bitmaps reserve mutable runtime buffers; vector paints can read their encoded Library image.
const MAX_PLACED_BITMAP_PIXELS: u64 = neoflash_core::MAX_BITMAP_BYTES as u64 / 4;
/// Safe ceiling for total raw encoded image bytes in `Project.resources` (`46 MiB < 64 MiB` and `< 128 MiB` JSON).
const MAX_TOTAL_IMAGE_RAW_BYTES: usize = 46 * 1024 * 1024;
/// Safe ceiling for a single encoded image resource (`14 MiB < 16 MiB`).
const MAX_SINGLE_IMAGE_RAW_BYTES: usize = 14 * 1024 * 1024;
/// Safe ceiling for total symbols in `Project.symbols` (`< 10,000` validation limit).
const MAX_PROJECT_SYMBOLS: usize = 9_200;
/// Safe ceiling for cached overridden component variants.
const MAX_VARIANT_SYMBOLS: usize = 2_400;
/// Safe ceiling for total authored `Element` records (`< 100,000` validation limit).
const MAX_AUTHORED_ELEMENTS: usize = 92_000;
/// Safe ceiling for expanded scene nodes across flattened scenes (`< 100,000` `evaluate::MAX_NODES`).
const MAX_EXPANDED_SCENE_NODES: usize = 90_000;
/// Safe ceiling for symbol nesting depth (`< 64` `evaluate::MAX_DEPTH`).
const MAX_SYMBOL_GRAPH_DEPTH: usize = 52;
/// Hard step ceiling per Auto Layout reflow pass so cyclic or diamond `INSTANCE` DAGs always finish in milliseconds.
const MAX_REFLOW_STEPS: usize = 100_000;

struct ReflowCtx {
    remaining_steps: usize,
    visiting_masters: HashSet<String>,
    hug_memo: HashMap<u64, (f64, f64)>,
}

impl ReflowCtx {
    fn new() -> Self {
        Self {
            remaining_steps: MAX_REFLOW_STEPS,
            visiting_masters: HashSet::new(),
            hug_memo: HashMap::new(),
        }
    }
}

trait MasterNodeRef {
    fn as_figma_node(&self) -> &FigmaNode;
}

impl MasterNodeRef for FigmaNode {
    #[inline]
    fn as_figma_node(&self) -> &FigmaNode {
        self
    }
}

impl MasterNodeRef for &FigmaNode {
    #[inline]
    fn as_figma_node(&self) -> &FigmaNode {
        self
    }
}

#[derive(Clone)]
struct BuiltSymbolInfo {
    symbol_id: String,
    width: f64,
    height: f64,
    expanded_nodes: usize,
    graph_depth: usize,
}

struct FigmaBuildCtx<'a, 'b> {
    options: &'a FigmaImportOptions,
    master_nodes: HashMap<String, &'b FigmaNode>,
    component_symbols: HashMap<String, BuiltSymbolInfo>,
    building_components: HashSet<String>,
    variant_cache: HashMap<u64, BuiltSymbolInfo>,
    reflow_ctx: ReflowCtx,
    image_resources: HashMap<String, (String, u32, u32)>,
    project: &'a mut Project,
    report: &'a mut ImportReport,
    authored_elements: usize,
    page_truncated: bool,
    warned_element_budget: bool,
    warned_expanded_budget: bool,
    warned_symbol_budget: bool,
    warned_override_budget: bool,
}

impl<'a, 'b> FigmaBuildCtx<'a, 'b> {
    fn warn_page_limit(&mut self, page: &FigmaNode, skipped: bool) {
        let outcome = if skipped {
            "Skipped this page"
        } else {
            "Imported only part of this page"
        };
        self.report.warning(
            if skipped { "figma-page-skipped" } else { "figma-page-truncated" },
            format!("{} ({})", page.name, page.id),
            format!("{outcome} because the selection exceeds NeoFlash's project size limits. Import this page separately or select fewer pages."),
        );
    }

    fn retain_page(&mut self, page: &FigmaNode, expanded_nodes: usize) -> bool {
        if !std::mem::take(&mut self.page_truncated) {
            return true;
        }
        self.warn_page_limit(page, expanded_nodes == 0);
        expanded_nodes > 0
    }

    fn lookup_image(&mut self, image_ref: &str, location: &str) -> Option<(String, u32, u32)> {
        if let Some(found) = self.image_resources.get(image_ref) {
            return Some(found.clone());
        }
        let low = image_ref.to_ascii_lowercase();
        if let Some(found) = self.image_resources.get(&low) {
            return Some(found.clone());
        }
        self.report.warning(
            "figma-missing-image-asset",
            location.to_owned(),
            format!("Figma image fill '{image_ref}' is unavailable or skipped; converted to placeholder shape."),
        );
        None
    }

    fn map_blend_mode(&mut self, raw: &str, location: &str) -> BlendMode {
        if !self.options.preserve_blend_modes {
            return BlendMode::Normal;
        }
        match raw {
            "" | "PASS_THROUGH" | "NORMAL" => BlendMode::Normal,
            "MULTIPLY" | "LINEAR_BURN" | "COLOR_BURN" => BlendMode::Multiply,
            "SCREEN" => BlendMode::Screen,
            "LIGHTEN" => BlendMode::Lighten,
            "DARKEN" => BlendMode::Darken,
            "DIFFERENCE" | "EXCLUSION" => BlendMode::Difference,
            "LINEAR_DODGE" | "ADD" | "COLOR_DODGE" | "PLUS_LIGHTER" => BlendMode::Add,
            "OVERLAY" | "SOFT_LIGHT" => BlendMode::Overlay,
            "HARD_LIGHT" => BlendMode::Hardlight,
            other => {
                self.report.warning(
                    "figma-blend-approximated",
                    location.to_owned(),
                    format!("Figma blend mode '{other}' approximated to Normal."),
                );
                BlendMode::Normal
            }
        }
    }

    fn map_effects(&mut self, effects: &[FigmaEffect], location: &str) -> Vec<FilterSpec> {
        let mut out = Vec::new();
        for eff in effects {
            match eff {
                FigmaEffect::DropShadow {
                    color,
                    offset_x,
                    offset_y,
                    radius,
                    inner,
                } => {
                    let distance = offset_x.hypot(*offset_y);
                    let angle = offset_y.atan2(*offset_x).to_degrees();
                    let rgb = (u32::from(color[0]) << 16)
                        | (u32::from(color[1]) << 8)
                        | u32::from(color[2]);
                    let alpha = (f64::from(color[3]) / 255.0).clamp(0.0, 1.0);
                    out.push(FilterSpec::DropShadow {
                        distance: distance.clamp(0.0, 255.0),
                        angle,
                        color: rgb,
                        alpha,
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        strength: 1.0,
                        quality: 2,
                        inner: *inner,
                        knockout: false,
                        hide_object: false,
                    });
                }
                FigmaEffect::LayerBlur { radius } => {
                    if radius.is_finite() && *radius > 0.0 {
                        out.push(FilterSpec::Blur {
                            blur_x: (*radius).clamp(0.0, 255.0),
                            blur_y: (*radius).clamp(0.0, 255.0),
                            quality: 2,
                        });
                    }
                }
                FigmaEffect::BackgroundBlur { radius } => {
                    if radius.is_finite() && *radius > 0.0 {
                        self.report.warning(
                            "figma-background-blur-approximated",
                            location.to_owned(),
                            format!(
                                "Figma BACKGROUND_BLUR ({radius:.1}px) approximated as editable layer Blur filter (backdrop-only compositing is not isolated from underlying layers)."
                            ),
                        );
                        out.push(FilterSpec::Blur {
                            blur_x: (*radius).clamp(0.0, 255.0),
                            blur_y: (*radius).clamp(0.0, 255.0),
                            quality: 2,
                        });
                    }
                }
            }
        }
        out.truncate(neoflash_core::MAX_OBJECT_FILTERS);
        out
    }

    fn node_transform(&self, node: &FigmaNode, offset_x: f64, offset_y: f64) -> Transform {
        let mut tf = Transform {
            x: node.x - offset_x,
            y: node.y - offset_y,
            rotation: node.rotation_deg,
            ..Transform::default()
        };

        if let Some([a, b, c, d, tx, ty]) = node.relative_transform
            && [a, b, c, d, tx, ty].iter().all(|v| v.is_finite())
        {
            tf.x = tx - offset_x;
            tf.y = ty - offset_y;
            let sx = a.hypot(b);
            let sy_abs = c.hypot(d);
            let det = a * d - b * c;
            let sign = if det < 0.0 { -1.0 } else { 1.0 };
            if sx > 1e-6 && sy_abs > 1e-6 {
                let rot = b.atan2(a).to_degrees();
                let y_angle = (-c * sign).atan2(d * sign).to_degrees();
                let mut skew = y_angle - rot;
                while skew <= -180.0 {
                    skew += 360.0;
                }
                while skew > 180.0 {
                    skew -= 360.0;
                }
                tf.rotation = if rot.abs() < 1e-5 { 0.0 } else { rot };
                tf.scale_x = if (sx - 1.0).abs() < 1e-5 { 1.0 } else { sx };
                let sy = sy_abs * sign;
                tf.scale_y = if (sy - 1.0).abs() < 1e-5 { 1.0 } else { sy };
                if skew.abs() > 1e-4 && skew.abs() < 89.9 {
                    tf.skew = skew;
                } else {
                    tf.skew = 0.0;
                }
            }
        }

        let finite_or = |v: f64, def: f64| if v.is_finite() { v } else { def };
        tf.x = finite_or(tf.x, 0.0).clamp(-100_000.0, 100_000.0);
        tf.y = finite_or(tf.y, 0.0).clamp(-100_000.0, 100_000.0);
        tf.scale_x = finite_or(tf.scale_x, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_x.abs() < 1e-4 {
            tf.scale_x = if tf.scale_x.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.scale_y = finite_or(tf.scale_y, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_y.abs() < 1e-4 {
            tf.scale_y = if tf.scale_y.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.rotation = finite_or(tf.rotation, 0.0).rem_euclid(360.0);
        tf.skew = finite_or(tf.skew, 0.0).clamp(-89.9, 89.9);
        tf.alpha = finite_or(node.opacity, 1.0).clamp(0.0, 1.0);
        tf
    }

    fn fallback_shape_commands(
        &self,
        node: &FigmaNode,
    ) -> (Vec<PathCommand>, Option<PrimitiveOptions>) {
        let w = (if node.width.is_finite() {
            node.width
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        let h = (if node.height.is_finite() {
            node.height
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        match node.node_type.as_str() {
            "ELLIPSE" => {
                if let Some(arc) = node.arc_data {
                    let s_deg = if arc.start_angle_rad.is_finite() {
                        arc.start_angle_rad.to_degrees().rem_euclid(360.0)
                    } else {
                        0.0
                    };
                    let span_rad = (arc.end_angle_rad - arc.start_angle_rad).abs();
                    let e_deg = if !arc.end_angle_rad.is_finite()
                        || span_rad >= std::f64::consts::TAU - 1e-4
                    {
                        360.0
                    } else {
                        arc.end_angle_rad.to_degrees().rem_euclid(360.0)
                    };
                    let oval_opts = OvalOptions {
                        start_angle: s_deg,
                        end_angle: e_deg,
                        inner_radius: (if arc.inner_radius_ratio.is_finite() {
                            arc.inner_radius_ratio * 100.0
                        } else {
                            0.0
                        })
                        .clamp(0.0, 99.0),
                        close_path: true,
                    };
                    (
                        oval_path_commands(w, h, oval_opts),
                        (!oval_opts.is_default()).then_some(PrimitiveOptions::Oval(oval_opts)),
                    )
                } else {
                    (oval_path_commands(w, h, OvalOptions::default()), None)
                }
            }
            "STAR" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let inner = node
                    .star_inner_radius
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.382)
                    .clamp(0.05, 0.95);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    true,
                    inner,
                );
                (cmds, None)
            }
            "REGULAR_POLYGON" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    false,
                    0.5,
                );
                (cmds, None)
            }
            "LINE" => (
                vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: w, y: 0.0 },
                ],
                None,
            ),
            _ => {
                let max_r = 0.5 * w.min(h);
                let clamp_r = |v: f64| {
                    if v.is_finite() {
                        v.clamp(0.0, max_r)
                    } else {
                        0.0
                    }
                };
                // Convert Figma `[tl, tr, br, bl]` -> NeoFlash `[tl, tr, bl, br]`.
                let [tl, tr, br, bl] = node.corner_radii.map(clamp_r);
                let nf_radii = [tl, tr, bl, br];
                if nf_radii.iter().any(|&v| v > 0.0) {
                    let rect_opts = RectangleOptions {
                        corner_radii: nf_radii,
                        corners_locked: (nf_radii[0] - nf_radii[1]).abs() < 1e-6
                            && (nf_radii[0] - nf_radii[2]).abs() < 1e-6
                            && (nf_radii[0] - nf_radii[3]).abs() < 1e-6,
                    };
                    (
                        rectangle_path_commands(w, h, rect_opts),
                        (!rect_opts.is_default()).then_some(PrimitiveOptions::Rectangle(rect_opts)),
                    )
                } else {
                    (
                        rectangle_path_commands(w, h, RectangleOptions::default()),
                        None,
                    )
                }
            }
        }
    }

    fn ensure_component_symbol(&mut self, comp_id: &str) -> Option<BuiltSymbolInfo> {
        if let Some(existing) = self.component_symbols.get(comp_id) {
            return Some(existing.clone());
        }
        if self.building_components.contains(comp_id) {
            return None;
        }
        if self.project.symbols.len() >= MAX_PROJECT_SYMBOLS {
            self.page_truncated = true;
            if !self.warned_symbol_budget {
                self.warned_symbol_budget = true;
                self.report.warning(
                    "figma-symbol-budget-exceeded",
                    comp_id.to_owned(),
                    format!("Compacted remaining Figma symbols after reaching {MAX_PROJECT_SYMBOLS} symbols."),
                );
            }
            return None;
        }
        let comp_node = *self.master_nodes.get(comp_id)?;
        self.building_components.insert(comp_id.to_owned());

        let sym_id = new_id("figsym");
        let (layers, expanded_nodes, graph_depth) =
            self.build_symbol_layers_with_optional_bg(comp_node, MAX_EXPANDED_SCENE_NODES / 2, 1);
        self.building_components.remove(comp_id);

        self.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: sanitize_symbol_name(&comp_node.name),
            folder: "Figma Components".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        self.report.statistics.symbols = self.project.symbols.len();

        let info = BuiltSymbolInfo {
            symbol_id: sym_id,
            width: comp_node.width,
            height: comp_node.height,
            expanded_nodes,
            graph_depth,
        };
        self.component_symbols
            .insert(comp_id.to_owned(), info.clone());
        Some(info)
    }

    fn resolve_instance_symbol(
        &mut self,
        comp_id: &str,
        inst_node: &FigmaNode,
        location: &str,
    ) -> Option<BuiltSymbolInfo> {
        let master_ref = *self.master_nodes.get(comp_id)?;
        let resizes_layout_master = inst_node.instance_text_layouts.is_empty()
            && inst_node.auto_layout.has_explicit_size
            && master_uses_layout_or_constraints(master_ref)
            && ((inst_node.width - master_ref.width).abs() > 0.25
                || (inst_node.height - master_ref.height).abs() > 0.25);
        if inst_node.symbol_overrides.is_empty()
            && inst_node.instance_text_layouts.is_empty()
            && !resizes_layout_master
        {
            return self.ensure_component_symbol(comp_id);
        }
        let sig = hash_instance_overrides(comp_id, inst_node, resizes_layout_master);
        if let Some(cached) = self.variant_cache.get(&sig) {
            return Some(cached.clone());
        }
        if self.variant_cache.len() >= MAX_VARIANT_SYMBOLS
            || self.project.symbols.len() >= MAX_PROJECT_SYMBOLS.saturating_sub(400)
            || self.authored_elements >= MAX_AUTHORED_ELEMENTS.saturating_sub(6_000)
            || self.building_components.contains(comp_id)
        {
            if !self.warned_override_budget {
                self.warned_override_budget = true;
                self.report.warning(
                    "figma-instance-override-approximated",
                    location.to_owned(),
                    "Additional component instance overrides reused master symbols to stay within project symbol/element limits.",
                );
            }
            return self.ensure_component_symbol(comp_id);
        }

        let mut specialized = master_ref.clone();
        let had_text_layouts = !inst_node.instance_text_layouts.is_empty();
        let mut any_applied = apply_instance_text_layouts(&mut specialized, inst_node);
        let mut needs_layout_reflow = false;
        for ov in &inst_node.symbol_overrides {
            if apply_override_at_path(&mut specialized, &ov.guid_path, ov, !had_text_layouts) {
                any_applied = true;
                if !had_text_layouts
                    && (ov.characters.is_some()
                        || ov.font_size.is_some()
                        || ov.text_auto_resize.is_some()
                        || ov.visible.is_some()
                        || ov.overridden_symbol_id.is_some())
                {
                    needs_layout_reflow = true;
                }
            }
        }
        if resizes_layout_master {
            self.reflow_ctx.visiting_masters.insert(comp_id.to_owned());
            resize_container_to_dimensions(
                &mut specialized,
                inst_node.width,
                inst_node.height,
                &self.master_nodes,
                &mut self.reflow_ctx,
                0,
            );
            self.reflow_ctx.visiting_masters.remove(comp_id);
            any_applied = true;
            needs_layout_reflow = true;
        }
        if !any_applied {
            return self.ensure_component_symbol(comp_id);
        }
        if needs_layout_reflow {
            self.reflow_ctx.visiting_masters.insert(comp_id.to_owned());
            let _ = reflow_figma_node_inner(
                &mut specialized,
                &self.master_nodes,
                &mut self.reflow_ctx,
                false,
                0,
            );
            self.reflow_ctx.visiting_masters.remove(comp_id);
            apply_reverse_z_order_recursive(&mut specialized, 0);
        }

        self.building_components.insert(comp_id.to_owned());
        let sym_id = new_id("figvar");
        let (layers, expanded_nodes, graph_depth) = self.build_symbol_layers_with_optional_bg(
            &specialized,
            MAX_EXPANDED_SCENE_NODES / 2,
            1,
        );
        self.building_components.remove(comp_id);

        self.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: sanitize_symbol_name(&format!("{} Variant", specialized.name)),
            folder: "Figma Components".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        self.report.statistics.symbols = self.project.symbols.len();

        let info = BuiltSymbolInfo {
            symbol_id: sym_id,
            width: specialized.width,
            height: specialized.height,
            expanded_nodes,
            graph_depth,
        };
        self.variant_cache.insert(sig, info.clone());
        Some(info)
    }

    fn build_symbol_layers_with_optional_bg(
        &mut self,
        container: &FigmaNode,
        expanded_budget: usize,
        depth: usize,
    ) -> (Vec<Layer>, usize, usize) {
        let mut layers = Vec::new();
        let mut expanded_used = 1usize;
        let has_children = !container.children.is_empty();
        let has_border = !container.strokes.is_empty()
            && (container.stroke_weight > 0.0 || container.border_weights.is_some());

        if (!container.fills.is_empty() || (!has_children && has_border))
            && container.width > 0.0
            && container.height > 0.0
            && self.authored_elements < MAX_AUTHORED_ELEMENTS
        {
            let mut bg_node = container.clone();
            if has_children && has_border {
                // Separate foreground Frame/Component borders onto a top "Border" layer above clipped children.
                bg_node.strokes.clear();
                bg_node.stroke_weight = 0.0;
                bg_node.border_weights = None;
                bg_node.stroke_paths.clear();
                bg_node.stroke_paths_are_outlines = false;
            }
            let mut bg_bfs = Vec::new();
            let mut bg_vfs = Vec::new();
            let mut bg_els = self.shape_elements(
                &bg_node,
                Transform::default(),
                BlendMode::Normal,
                &[],
                &format!("{} / Background", container.name),
                expanded_budget.saturating_sub(expanded_used),
                &mut bg_bfs,
                &mut bg_vfs,
            );
            // The placed instance owns visibility, including instances of hidden masters.
            for element in &mut bg_els {
                element.visible = true;
            }
            if !bg_els.is_empty() {
                expanded_used = expanded_used.saturating_add(bg_els.len());
                layers.push(Layer {
                    id: new_id("layer"),
                    name: "Background".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: bg_els,
                        bitmap_fills: bg_bfs,
                        vector_effects: bg_vfs,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                });
                self.report.statistics.layers += 1;
                self.report.statistics.keyframes += 1;
            }
        }

        let rem_budget = expanded_budget.saturating_sub(expanded_used);
        let (child_layers, child_exp, max_dep) = self.build_layers_for_children(
            &container.children,
            0.0,
            0.0,
            if container.clips_content {
                Some((
                    container.width.max(1.0),
                    container.height.max(1.0),
                    container.corner_radii,
                ))
            } else {
                None
            },
            &container.name,
            rem_budget,
            depth + 1,
        );
        expanded_used = expanded_used.saturating_add(child_exp);
        layers.extend(child_layers);

        if has_children
            && has_border
            && container.width > 0.0
            && container.height > 0.0
            && self.authored_elements < MAX_AUTHORED_ELEMENTS
        {
            let mut border_node = container.clone();
            border_node.fills.clear();
            border_node.effects.clear();
            let mut border_bfs = Vec::new();
            let mut border_vfs = Vec::new();
            let mut border_els = self.shape_elements(
                &border_node,
                Transform::default(),
                BlendMode::Normal,
                &[],
                &format!("{} / Border", container.name),
                expanded_budget.saturating_sub(expanded_used),
                &mut border_bfs,
                &mut border_vfs,
            );
            for element in &mut border_els {
                element.visible = true;
            }
            if !border_els.is_empty() {
                expanded_used = expanded_used.saturating_add(border_els.len());
                layers.push(Layer {
                    id: new_id("layer"),
                    name: "Border".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: border_els,
                        bitmap_fills: border_bfs,
                        vector_effects: border_vfs,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                });
                self.report.statistics.layers += 1;
                self.report.statistics.keyframes += 1;
            }
        }

        if layers.is_empty() {
            layers.push(Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            });
        }
        (layers, expanded_used, max_dep)
    }

    fn build_symbol_with_optional_bg(
        &mut self,
        container: &FigmaNode,
        folder: &str,
        expanded_budget: usize,
        depth: usize,
    ) -> Option<BuiltSymbolInfo> {
        if self.project.symbols.len() >= MAX_PROJECT_SYMBOLS {
            self.page_truncated = true;
            if !self.warned_symbol_budget {
                self.warned_symbol_budget = true;
                self.report.warning(
                    "figma-symbol-budget-exceeded",
                    container.name.clone(),
                    format!("Compacted remaining Figma containers after reaching {MAX_PROJECT_SYMBOLS} symbols."),
                );
            }
            return None;
        }
        let sym_id = new_id("figsym");
        let (layers, expanded_nodes, graph_depth) =
            self.build_symbol_layers_with_optional_bg(container, expanded_budget, depth);
        self.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: sanitize_symbol_name(&container.name),
            folder: folder.to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        self.report.statistics.symbols = self.project.symbols.len();
        Some(BuiltSymbolInfo {
            symbol_id: sym_id,
            width: container.width,
            height: container.height,
            expanded_nodes,
            graph_depth,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_leaf_to_elements(
        &mut self,
        node: &FigmaNode,
        offset_x: f64,
        offset_y: f64,
        location: &str,
        expanded_budget: usize,
        depth: usize,
        bitmap_fills: &mut Vec<AuthoredBitmapFill>,
        vector_effects: &mut Vec<ElementVectorEffects>,
    ) -> (Vec<Element>, usize, usize) {
        if depth > MAX_SYMBOL_GRAPH_DEPTH
            || expanded_budget == 0
            || self.authored_elements >= MAX_AUTHORED_ELEMENTS
        {
            self.page_truncated |=
                expanded_budget == 0 || self.authored_elements >= MAX_AUTHORED_ELEMENTS;
            return (Vec::new(), 0, 1);
        }
        let blend_mode = self.map_blend_mode(&node.blend_mode, location);
        let filters = self.map_effects(&node.effects, location);
        let base_tf = self.node_transform(node, offset_x, offset_y);

        // 1. Instance of a registered Figma Component / Symbol (with optional override variant)
        if node.node_type == "INSTANCE"
            && let Some(cid) = &node.component_id
            && let Some(comp_info) = self.resolve_instance_symbol(cid, node, location)
            && depth + comp_info.graph_depth <= MAX_SYMBOL_GRAPH_DEPTH
        {
            if comp_info.expanded_nodes > expanded_budget {
                self.page_truncated = true;
                if !self.warned_expanded_budget {
                    self.warned_expanded_budget = true;
                    self.report.warning(
                        "figma-expanded-node-budget-exceeded",
                        location.to_owned(),
                        format!("Compacted deeply repeated Figma instances after reaching {MAX_EXPANDED_SCENE_NODES} expanded nodes."),
                    );
                }
                return (Vec::new(), 0, 1);
            }
            let mut inst_tf = base_tf;
            if comp_info.width > 1e-3
                && comp_info.height > 1e-3
                && node.width > 1e-3
                && node.height > 1e-3
                && ((node.width - comp_info.width).abs() > 0.25
                    || (node.height - comp_info.height).abs() > 0.25)
            {
                inst_tf.scale_x =
                    (inst_tf.scale_x * (node.width / comp_info.width)).clamp(-1000.0, 1000.0);
                inst_tf.scale_y =
                    (inst_tf.scale_y * (node.height / comp_info.height)).clamp(-1000.0, 1000.0);
            }
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: inst_tf,
                kind: ElementKind::Instance {
                    symbol_id: comp_info.symbol_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.authored_elements += 1;
            self.report.statistics.elements += 1;
            return (
                vec![el],
                comp_info.expanded_nodes,
                1 + comp_info.graph_depth,
            );
        }

        // 1b. Master `COMPONENT` / `SYMBOL` placed directly on a design-system sheet frame
        if matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL")
            && !node.is_state_group
            && self.master_nodes.contains_key(&node.id)
            && let Some(comp_info) = self.ensure_component_symbol(&node.id)
            && depth + comp_info.graph_depth <= MAX_SYMBOL_GRAPH_DEPTH
        {
            if comp_info.expanded_nodes > expanded_budget {
                self.page_truncated = true;
                return (Vec::new(), 0, 1);
            }
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: comp_info.symbol_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.authored_elements += 1;
            self.report.statistics.elements += 1;
            return (
                vec![el],
                comp_info.expanded_nodes,
                1 + comp_info.graph_depth,
            );
        }

        // On frames and other containers, fill geometry describes the background. Boolean
        // operations instead use their computed outline in place of the operand children.
        let container = matches!(
            node.node_type.as_str(),
            "FRAME" | "GROUP" | "SECTION" | "COMPONENT_SET" | "COMPONENT" | "SYMBOL" | "INSTANCE"
        );
        if !node.children.is_empty()
            && (container || node.fill_paths.is_empty())
            && node.text.is_none()
        {
            let Some(sym_info) =
                self.build_symbol_with_optional_bg(node, "Figma Layers", expanded_budget, depth)
            else {
                return (Vec::new(), 0, 1);
            };
            if depth + sym_info.graph_depth > MAX_SYMBOL_GRAPH_DEPTH {
                return (Vec::new(), 0, 1);
            }
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: sym_info.symbol_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.authored_elements += 1;
            self.report.statistics.elements += 1;
            return (vec![el], sym_info.expanded_nodes, 1 + sym_info.graph_depth);
        }

        // 3. Editable Text (`TEXT`)
        if let Some(td) = &node.text {
            let text_color = node
                .fills
                .iter()
                .find_map(|p| match p {
                    FigmaPaint::Solid { color } => Some(*color),
                    FigmaPaint::Gradient { stops, .. } => stops.first().map(|s| s.color),
                    _ => None,
                })
                .unwrap_or([255, 255, 255, 255]);
            let text_rgb = (u32::from(text_color[0]) << 16)
                | (u32::from(text_color[1]) << 8)
                | u32::from(text_color[2]);
            let mut text_tf = base_tf;
            text_tf.alpha = (text_tf.alpha * (f64::from(text_color[3]) / 255.0)).clamp(0.0, 1.0);

            let safe_font = sanitize_font_family(&td.font_family);
            let font_size = td.font_size.clamp(1.0, 1024.0);
            // NeoFlash measures line advances as 1.2 em plus leading; Figma stores the entire advance.
            let min_leading = -(font_size * 0.9);
            let leading = td
                .line_height
                .map(|lh| (lh - font_size * 1.2).clamp(min_leading, 500.0))
                .unwrap_or(2.0);

            let base_fmt = TextFormat {
                font: safe_font,
                size: font_size,
                color: text_rgb,
                bold: td.bold,
                italic: td.italic,
                underline: td.underline,
                align: td.align,
                letter_spacing: td.letter_spacing.clamp(-60.0, 60.0),
                leading,
                ..TextFormat::default()
            };

            let normalized_chars = neoflash_core::normalize_text(&td.characters);
            let mut final_fmt = base_fmt.clone();
            if td.justify {
                final_fmt.set_align_mode(Some("justify".into()));
            }
            let auto_size = matches!(td.auto_resize.as_str(), "HEIGHT" | "WIDTH_AND_HEIGHT");
            {
                // Figma text starts at its frame's edges; the default Flash field has a 2px gutter.
                let mut opts = AuthoredTextFieldOptions {
                    padding_left: 0.0,
                    padding_right: 0.0,
                    padding_top: 0.0,
                    padding_bottom: 0.0,
                    auto_size: if auto_size { "left" } else { "none" }.to_owned(),
                    vertical_align: td.align_v.clone(),
                    strikethrough: td.strikethrough,
                    ..AuthoredTextFieldOptions::default()
                };
                if !td.style_runs.is_empty() {
                    let total_u16 = normalized_chars.encode_utf16().count() as u32;
                    let mut sorted_runs = td.style_runs.clone();
                    sorted_runs.sort_by_key(|sr| (sr.start_utf16, sr.end_utf16));
                    let mut runs = Vec::new();
                    let mut cursor = 0u32;
                    let mut covered_strikethrough = 0u32;
                    for sr in &sorted_runs {
                        let s = sr.start_utf16.min(total_u16).max(cursor);
                        let e = sr.end_utf16.min(total_u16);
                        if e <= s {
                            continue;
                        }
                        if s > cursor {
                            runs.push(TextFormatRun {
                                start: cursor,
                                end: s,
                                format: base_fmt.clone(),
                            });
                        }
                        if sr.style.strikethrough == Some(true) {
                            covered_strikethrough = covered_strikethrough.saturating_add(e - s);
                        }
                        let mut rf = base_fmt.clone();
                        if let Some(fam) = &sr.style.font_family {
                            rf.font = sanitize_font_family(fam);
                        }
                        if let Some(fs) = sr.style.font_size {
                            rf.size = fs.clamp(1.0, 1024.0);
                            let min_l = -(rf.size * 0.9);
                            rf.leading = rf.leading.clamp(min_l, 500.0);
                        }
                        if let Some(b) = sr.style.bold {
                            rf.bold = b;
                        }
                        if let Some(it) = sr.style.italic {
                            rf.italic = it;
                        }
                        if let Some(u) = sr.style.underline {
                            rf.underline = u;
                        }
                        if let Some(c) = sr.style.fill_color {
                            rf.color =
                                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
                        }
                        if let Some(ls) = sr.style.letter_spacing {
                            rf.letter_spacing = ls.clamp(-60.0, 60.0);
                        }
                        if let Some(lh) = sr.style.line_height {
                            let min_l = -(rf.size * 0.9);
                            rf.leading = (lh - rf.size * 1.2).clamp(min_l, 500.0);
                        }
                        runs.push(TextFormatRun {
                            start: s,
                            end: e,
                            format: rf,
                        });
                        cursor = e;
                    }
                    if cursor < total_u16 {
                        runs.push(TextFormatRun {
                            start: cursor,
                            end: total_u16,
                            format: base_fmt,
                        });
                    }
                    if total_u16 > 0 && covered_strikethrough == total_u16 {
                        opts.strikethrough = true;
                    }
                    opts.format_runs = runs;
                }
                final_fmt.set_field_options(Some(opts));
            }

            let word_wrap = td.auto_resize != "WIDTH_AND_HEIGHT";
            let multiline =
                normalized_chars.contains('\n') || (word_wrap && node.height > font_size * 1.6);
            let el = Element {
                id: new_id("el"),
                name: sanitize_element_name(&node.name),
                transform: text_tf,
                kind: ElementKind::TextBox {
                    text: normalized_chars,
                    width: node.width.max(16.0),
                    height: node.height.max(font_size * 1.25).max(16.0),
                    format: final_fmt,
                    word_wrap,
                    multiline,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.authored_elements += 1;
            self.report.statistics.elements += 1;
            return (vec![el], 1, 1);
        }

        let elements = self.shape_elements(
            node,
            base_tf,
            blend_mode,
            &filters,
            location,
            expanded_budget,
            bitmap_fills,
            vector_effects,
        );
        let count = elements.len();
        (elements, count, 1)
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_elements(
        &mut self,
        node: &FigmaNode,
        base_tf: Transform,
        blend_mode: BlendMode,
        filters: &[FilterSpec],
        location: &str,
        expanded_budget: usize,
        bitmap_fills: &mut Vec<AuthoredBitmapFill>,
        vector_effects: &mut Vec<ElementVectorEffects>,
    ) -> Vec<Element> {
        #[derive(Clone)]
        enum ResolvedFigmaStroke {
            Standard {
                solid: Color,
                grad: Option<GradientPaint>,
            },
            Mesh {
                solid: Color,
                kind: FigmaGradientKind,
                stops: Vec<GradientStop>,
                uv_transform: Option<[f64; 6]>,
            },
        }

        let mut out = Vec::new();
        let (fallback_cmds, prim_opt) = self.fallback_shape_commands(node);
        let indep_borders = has_unequal_border_weights(node);
        let mut resolved_strokes: Vec<ResolvedFigmaStroke> = Vec::new();
        for sp in &node.strokes {
            match sp {
                FigmaPaint::Solid { color } => {
                    resolved_strokes.push(ResolvedFigmaStroke::Standard {
                        solid: *color,
                        grad: None,
                    })
                }
                FigmaPaint::Gradient {
                    kind,
                    stops,
                    uv_transform,
                } => {
                    let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                    match kind {
                        FigmaGradientKind::Linear | FigmaGradientKind::Radial => {
                            let gkind = if *kind == FigmaGradientKind::Radial {
                                GradientKind::Radial
                            } else {
                                GradientKind::Linear
                            };
                            let grad = build_figma_gradient_paint(
                                gkind,
                                stops,
                                *uv_transform,
                                node.width,
                                node.height,
                            );
                            resolved_strokes.push(ResolvedFigmaStroke::Standard {
                                solid: fallback_c,
                                grad,
                            });
                        }
                        FigmaGradientKind::Angular | FigmaGradientKind::Diamond => {
                            resolved_strokes.push(ResolvedFigmaStroke::Mesh {
                                solid: fallback_c,
                                kind: *kind,
                                stops: stops.clone(),
                                uv_transform: *uv_transform,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        let stroke_w = if resolved_strokes.is_empty() || indep_borders {
            0.0
        } else {
            node.stroke_weight.clamp(0.0, 256.0)
        };
        let valid_stroke_paths: Vec<(Vec<PathCommand>, FillRule)> = node
            .stroke_paths
            .iter()
            .filter(|(c, _)| validate_path_commands(c).is_ok())
            .cloned()
            .collect();
        let any_mesh_stroke = resolved_strokes
            .iter()
            .any(|s| matches!(s, ResolvedFigmaStroke::Mesh { .. }));
        let use_expanded_stroke_outlines = !indep_borders
            && stroke_w > 0.0
            && node.stroke_paths_are_outlines
            && !valid_stroke_paths.is_empty();
        // Mesh-painted strokes cannot be stored inline on `ElementKind::Path::stroke`; emit them as
        // dedicated expanded outline `Path` elements carrying `ElementVectorEffects.vector_mesh`!
        let defer_all_strokes_to_trailing = !node.fills.is_empty()
            && stroke_w > 0.0
            && (use_expanded_stroke_outlines || any_mesh_stroke);
        let (stroke_solid, stroke_grad) = if defer_all_strokes_to_trailing {
            ([0, 0, 0, 0], None)
        } else {
            match resolved_strokes.first() {
                Some(ResolvedFigmaStroke::Standard { solid, grad }) => (*solid, grad.clone()),
                _ => ([0, 0, 0, 0], None),
            }
        };
        let inline_stroke_w = if defer_all_strokes_to_trailing {
            0.0
        } else {
            stroke_w
        };

        let paths_to_fill: Vec<(Vec<PathCommand>, FillRule)> = if !node.fill_paths.is_empty() {
            node.fill_paths
                .iter()
                .filter(|(c, _)| validate_path_commands(c).is_ok())
                .cloned()
                .collect()
        } else if validate_path_commands(&fallback_cmds).is_ok() {
            vec![(fallback_cmds.clone(), FillRule::NonZero)]
        } else {
            Vec::new()
        };

        let is_rect_node = node.is_rectangular();

        if node.fills.is_empty() && !use_expanded_stroke_outlines && stroke_w > 0.0 {
            let centerline_cmds: Vec<Vec<PathCommand>> = if !valid_stroke_paths.is_empty() {
                valid_stroke_paths.iter().map(|(c, _)| c.clone()).collect()
            } else if validate_path_commands(&fallback_cmds).is_ok() {
                vec![fallback_cmds.clone()]
            } else {
                Vec::new()
            };
            let total_stroke_els = resolved_strokes.len() * centerline_cmds.len();
            let mut emitted_stroke_els = 0usize;
            for rstroke in &resolved_strokes {
                for scmds in &centerline_cmds {
                    if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                        || out.len() >= expanded_budget
                    {
                        self.page_truncated = true;
                        break;
                    }
                    emitted_stroke_els += 1;
                    let is_last = emitted_stroke_els == total_stroke_els;
                    let el_id = new_id("el");
                    let kind = match rstroke {
                        ResolvedFigmaStroke::Standard { solid, grad } => {
                            if grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    fill_gradient: None,
                                    stroke_gradient: grad.clone(),
                                    stroke_width: stroke_w,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    stroke_width: stroke_w,
                                }
                            }
                        }
                        ResolvedFigmaStroke::Mesh {
                            solid,
                            kind,
                            stops,
                            uv_transform,
                        } => {
                            let expanded = expand_variable_width_stroke(
                                scmds,
                                stroke_w,
                                &StrokeWidthProfile::default(),
                            );
                            if !expanded.is_empty() && validate_path_commands(&expanded).is_ok() {
                                if let Some(mesh) = build_figma_mesh_paint(
                                    *kind,
                                    stops,
                                    *uv_transform,
                                    node.width,
                                    node.height,
                                    &expanded,
                                ) {
                                    let rep = mesh.representative_color();
                                    vector_effects.push(ElementVectorEffects {
                                        element_id: el_id.clone(),
                                        vector_mesh: Some(mesh),
                                        ..ElementVectorEffects::default()
                                    });
                                    ElementKind::Path {
                                        commands: expanded,
                                        fill_rule: FillRule::NonZero,
                                        fill: rep,
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: scmds.clone(),
                                        fill_rule: FillRule::NonZero,
                                        fill: [0, 0, 0, 0],
                                        stroke: *solid,
                                        stroke_width: stroke_w,
                                    }
                                }
                            } else {
                                ElementKind::Path {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    stroke_width: stroke_w,
                                }
                            }
                        }
                    };
                    out.push(Element {
                        id: el_id,
                        name: sanitize_element_name(&node.name),
                        transform: base_tf,
                        kind,
                        visible: node.visible,
                        cache_as_bitmap: !filters.is_empty(),
                        blend_mode,
                        filters: if is_last {
                            filters.to_vec()
                        } else {
                            Vec::new()
                        },
                        ..Element::default()
                    });
                    self.authored_elements += 1;
                    self.report.statistics.elements += 1;
                }
            }
        }

        let extra_centerline_strokes = !node.fills.is_empty()
            && !use_expanded_stroke_outlines
            && stroke_w > 0.0
            && (any_mesh_stroke || resolved_strokes.len() > 1);
        let has_trailing_stroke_elements = use_expanded_stroke_outlines || extra_centerline_strokes;

        for (fi, paint) in node.fills.iter().enumerate() {
            if self.authored_elements >= MAX_AUTHORED_ELEMENTS || out.len() >= expanded_budget {
                self.page_truncated = true;
                break;
            }
            let is_last_fill = fi + 1 == node.fills.len();
            let cur_stroke = if is_last_fill {
                stroke_solid
            } else {
                [0, 0, 0, 0]
            };
            let cur_stroke_w = if is_last_fill { inline_stroke_w } else { 0.0 };
            let cur_stroke_grad = if is_last_fill {
                stroke_grad.clone()
            } else {
                None
            };
            let apply_filters_on_fill = is_last_fill && !has_trailing_stroke_elements;

            match paint {
                FigmaPaint::Image {
                    image_ref,
                    scale_mode,
                    alpha,
                    image_transform,
                    scaling_factor,
                } => {
                    if let Some((res_id, iw, ih)) = self.lookup_image(image_ref, location) {
                        // Direct crisp `Bitmap` ONLY for non-rounded rectangular fills matching aspect ratio
                        // without explicit path or custom crop/tile/fit matrix.
                        if node.uses_bitmap_instance(iw, ih, scale_mode, *image_transform) {
                            let mut img_tf = base_tf;
                            if iw > 0 && ih > 0 && node.width > 0.0 && node.height > 0.0 {
                                img_tf.scale_x *= node.width / f64::from(iw);
                                img_tf.scale_y *= node.height / f64::from(ih);
                            }
                            img_tf.alpha = (img_tf.alpha * alpha).clamp(0.0, 1.0);
                            out.push(Element {
                                id: new_id("el"),
                                name: sanitize_element_name(&node.name),
                                transform: img_tf,
                                kind: ElementKind::Bitmap {
                                    resource_id: res_id,
                                    width: iw,
                                    height: ih,
                                    smoothing: true,
                                },
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if apply_filters_on_fill && cur_stroke_w == 0.0 {
                                    filters.to_vec()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.authored_elements += 1;
                            self.report.statistics.elements += 1;
                            if cur_stroke_w > 0.0 && self.authored_elements < MAX_AUTHORED_ELEMENTS
                            {
                                out.push(Element {
                                    id: new_id("el"),
                                    name: format!("{} Stroke", sanitize_element_name(&node.name)),
                                    transform: base_tf,
                                    kind: ElementKind::Rectangle {
                                        width: node.width.max(1.0),
                                        height: node.height.max(1.0),
                                        fill: [0, 0, 0, 0],
                                        stroke: cur_stroke,
                                        stroke_width: cur_stroke_w,
                                    },
                                    visible: node.visible,
                                    cache_as_bitmap: !filters.is_empty(),
                                    blend_mode,
                                    filters: if apply_filters_on_fill {
                                        filters.to_vec()
                                    } else {
                                        Vec::new()
                                    },
                                    ..Element::default()
                                });
                                self.authored_elements += 1;
                                self.report.statistics.elements += 1;
                            }
                        } else if let Some((cmds, rule)) = paths_to_fill.first() {
                            // Cropped / tiled / fitted / non-matching-aspect / rounded / vector shape with bitmap fill -> `Path` + `AuthoredBitmapFill`!
                            let el_id = new_id("el");
                            let mut shp_tf = base_tf;
                            shp_tf.alpha = (shp_tf.alpha * alpha).clamp(0.0, 1.0);
                            let (bf_matrix, bf_repeat) = compute_bitmap_fill_matrix(
                                iw,
                                ih,
                                node.width,
                                node.height,
                                scale_mode,
                                *image_transform,
                                *scaling_factor,
                            );
                            bitmap_fills.push(AuthoredBitmapFill {
                                element_id: el_id.clone(),
                                resource_id: res_id,
                                matrix: bf_matrix,
                                repeat: bf_repeat,
                                smoothing: true,
                            });
                            out.push(Element {
                                id: el_id,
                                name: sanitize_element_name(&node.name),
                                transform: shp_tf,
                                kind: ElementKind::Path {
                                    commands: cmds.clone(),
                                    fill_rule: *rule,
                                    fill: [255, 255, 255, 255],
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                },
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if apply_filters_on_fill {
                                    filters.to_vec()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.authored_elements += 1;
                            self.report.statistics.elements += 1;
                        }
                    } else if let Some((cmds, rule)) = paths_to_fill.first() {
                        out.push(Element {
                            id: new_id("el"),
                            name: sanitize_element_name(&node.name),
                            transform: base_tf,
                            kind: ElementKind::Path {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: [180, 180, 190, 255],
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            ..Element::default()
                        });
                        self.authored_elements += 1;
                        self.report.statistics.elements += 1;
                    }
                }
                FigmaPaint::Solid { color } => {
                    if is_rect_node && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: sanitize_element_name(&node.name),
                            transform: tf,
                            kind: if cur_stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: fallback_cmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: *color,
                                    stroke: cur_stroke,
                                    fill_gradient: None,
                                    stroke_gradient: cur_stroke_grad,
                                    stroke_width: cur_stroke_w,
                                }
                            } else {
                                ElementKind::Rectangle {
                                    width: node.width.max(1.0),
                                    height: node.height.max(1.0),
                                    fill: *color,
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                }
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if apply_filters_on_fill {
                                filters.to_vec()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.authored_elements += 1;
                        self.report.statistics.elements += 1;
                    } else if node.node_type == "ELLIPSE" && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: sanitize_element_name(&node.name),
                            transform: tf,
                            kind: ElementKind::Ellipse {
                                width: node.width.max(1.0),
                                height: node.height.max(1.0),
                                fill: *color,
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if apply_filters_on_fill {
                                filters.to_vec()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.authored_elements += 1;
                        self.report.statistics.elements += 1;
                    } else {
                        for (cmds, rule) in &paths_to_fill {
                            if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                                || out.len() >= expanded_budget
                            {
                                self.page_truncated = true;
                                break;
                            }
                            out.push(Element {
                                id: new_id("el"),
                                name: sanitize_element_name(&node.name),
                                transform: base_tf,
                                kind: if cur_stroke_grad.is_some() {
                                    ElementKind::PaintedPath {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        fill_gradient: None,
                                        stroke_gradient: cur_stroke_grad.clone(),
                                        stroke_width: cur_stroke_w,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        stroke_width: cur_stroke_w,
                                    }
                                },
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if apply_filters_on_fill {
                                    filters.to_vec()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.authored_elements += 1;
                            self.report.statistics.elements += 1;
                        }
                    }
                }
                FigmaPaint::Gradient {
                    kind,
                    stops,
                    uv_transform,
                } => match kind {
                    FigmaGradientKind::Linear | FigmaGradientKind::Radial => {
                        let gkind = if *kind == FigmaGradientKind::Radial {
                            GradientKind::Radial
                        } else {
                            GradientKind::Linear
                        };
                        let grad = build_figma_gradient_paint(
                            gkind,
                            stops,
                            *uv_transform,
                            node.width,
                            node.height,
                        );
                        let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                        for (cmds, rule) in &paths_to_fill {
                            if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                                || out.len() >= expanded_budget
                            {
                                self.page_truncated = true;
                                break;
                            }
                            let kind = if grad.is_some() || cur_stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: cmds.clone(),
                                    fill_rule: *rule,
                                    fill: fallback_c,
                                    stroke: cur_stroke,
                                    fill_gradient: grad.clone(),
                                    stroke_gradient: cur_stroke_grad.clone(),
                                    stroke_width: cur_stroke_w,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: cmds.clone(),
                                    fill_rule: *rule,
                                    fill: fallback_c,
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                }
                            };
                            out.push(Element {
                                id: new_id("el"),
                                name: sanitize_element_name(&node.name),
                                transform: base_tf,
                                kind,
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if apply_filters_on_fill {
                                    filters.to_vec()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.authored_elements += 1;
                            self.report.statistics.elements += 1;
                        }
                    }
                    FigmaGradientKind::Angular | FigmaGradientKind::Diamond => {
                        let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                        for (cmds, rule) in &paths_to_fill {
                            if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                                || out.len() >= expanded_budget
                            {
                                self.page_truncated = true;
                                break;
                            }
                            let el_id = new_id("el");
                            let fill_color = if let Some(mesh) = build_figma_mesh_paint(
                                *kind,
                                stops,
                                *uv_transform,
                                node.width,
                                node.height,
                                cmds,
                            ) {
                                let rep = mesh.representative_color();
                                vector_effects.push(ElementVectorEffects {
                                    element_id: el_id.clone(),
                                    vector_mesh: Some(mesh),
                                    ..ElementVectorEffects::default()
                                });
                                rep
                            } else {
                                fallback_c
                            };
                            let el_kind = if cur_stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: cmds.clone(),
                                    fill_rule: *rule,
                                    fill: fill_color,
                                    stroke: cur_stroke,
                                    fill_gradient: None,
                                    stroke_gradient: cur_stroke_grad.clone(),
                                    stroke_width: cur_stroke_w,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: cmds.clone(),
                                    fill_rule: *rule,
                                    fill: fill_color,
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                }
                            };
                            out.push(Element {
                                id: el_id,
                                name: sanitize_element_name(&node.name),
                                transform: base_tf,
                                kind: el_kind,
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if apply_filters_on_fill {
                                    filters.to_vec()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.authored_elements += 1;
                            self.report.statistics.elements += 1;
                        }
                    }
                },
            }
        }

        if use_expanded_stroke_outlines {
            // `strokeGeometry` in `fig-kiwi` / `.figma.json` is already an expanded filled outline polygon:
            // emit as filled `Path` / `PaintedPath` (`stroke_width = 0.0`) across all visible `node.strokes`!
            let total_outline_els = resolved_strokes.len() * valid_stroke_paths.len();
            let mut emitted_outline_els = 0usize;
            for rstroke in &resolved_strokes {
                for (scmds, srule) in &valid_stroke_paths {
                    if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                        || out.len() >= expanded_budget
                    {
                        self.page_truncated = true;
                        break;
                    }
                    emitted_outline_els += 1;
                    let is_last = emitted_outline_els == total_outline_els;
                    let el_id = new_id("el");
                    let kind = match rstroke {
                        ResolvedFigmaStroke::Standard { solid, grad } => {
                            if let Some(grad) = grad {
                                ElementKind::PaintedPath {
                                    commands: scmds.clone(),
                                    fill_rule: *srule,
                                    fill: *solid,
                                    stroke: [0, 0, 0, 0],
                                    fill_gradient: Some(grad.clone()),
                                    stroke_gradient: None,
                                    stroke_width: 0.0,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: scmds.clone(),
                                    fill_rule: *srule,
                                    fill: *solid,
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                }
                            }
                        }
                        ResolvedFigmaStroke::Mesh {
                            solid,
                            kind,
                            stops,
                            uv_transform,
                        } => {
                            let fill_c = if let Some(mesh) = build_figma_mesh_paint(
                                *kind,
                                stops,
                                *uv_transform,
                                node.width,
                                node.height,
                                scmds,
                            ) {
                                let rep = mesh.representative_color();
                                vector_effects.push(ElementVectorEffects {
                                    element_id: el_id.clone(),
                                    vector_mesh: Some(mesh),
                                    ..ElementVectorEffects::default()
                                });
                                rep
                            } else {
                                *solid
                            };
                            ElementKind::Path {
                                commands: scmds.clone(),
                                fill_rule: *srule,
                                fill: fill_c,
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            }
                        }
                    };
                    out.push(Element {
                        id: el_id,
                        name: if node.fills.is_empty() {
                            sanitize_element_name(&node.name)
                        } else {
                            format!("{} Stroke", sanitize_element_name(&node.name))
                        },
                        transform: base_tf,
                        kind,
                        visible: node.visible,
                        cache_as_bitmap: !filters.is_empty(),
                        blend_mode,
                        filters: if is_last {
                            filters.to_vec()
                        } else {
                            Vec::new()
                        },
                        ..Element::default()
                    });
                    self.authored_elements += 1;
                    self.report.statistics.elements += 1;
                }
            }
        } else if extra_centerline_strokes && stroke_w > 0.0 {
            let extra_cmds: Vec<Vec<PathCommand>> = if !valid_stroke_paths.is_empty() {
                valid_stroke_paths.iter().map(|(c, _)| c.clone()).collect()
            } else if validate_path_commands(&fallback_cmds).is_ok() {
                vec![fallback_cmds.clone()]
            } else {
                Vec::new()
            };
            let trailing_slice = if defer_all_strokes_to_trailing {
                &resolved_strokes[..]
            } else {
                &resolved_strokes[1..]
            };
            let total_extra = trailing_slice.len() * extra_cmds.len();
            let mut emitted_extra = 0usize;
            for rstroke in trailing_slice {
                for scmds in &extra_cmds {
                    if self.authored_elements >= MAX_AUTHORED_ELEMENTS
                        || out.len() >= expanded_budget
                    {
                        self.page_truncated = true;
                        break;
                    }
                    emitted_extra += 1;
                    let is_last = emitted_extra == total_extra;
                    let el_id = new_id("el");
                    let kind = match rstroke {
                        ResolvedFigmaStroke::Standard { solid, grad } => {
                            if grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    fill_gradient: None,
                                    stroke_gradient: grad.clone(),
                                    stroke_width: stroke_w,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    stroke_width: stroke_w,
                                }
                            }
                        }
                        ResolvedFigmaStroke::Mesh {
                            solid,
                            kind,
                            stops,
                            uv_transform,
                        } => {
                            let expanded = expand_variable_width_stroke(
                                scmds,
                                stroke_w,
                                &StrokeWidthProfile::default(),
                            );
                            if !expanded.is_empty() && validate_path_commands(&expanded).is_ok() {
                                if let Some(mesh) = build_figma_mesh_paint(
                                    *kind,
                                    stops,
                                    *uv_transform,
                                    node.width,
                                    node.height,
                                    &expanded,
                                ) {
                                    let rep = mesh.representative_color();
                                    vector_effects.push(ElementVectorEffects {
                                        element_id: el_id.clone(),
                                        vector_mesh: Some(mesh),
                                        ..ElementVectorEffects::default()
                                    });
                                    ElementKind::Path {
                                        commands: expanded,
                                        fill_rule: FillRule::NonZero,
                                        fill: rep,
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: scmds.clone(),
                                        fill_rule: FillRule::NonZero,
                                        fill: [0, 0, 0, 0],
                                        stroke: *solid,
                                        stroke_width: stroke_w,
                                    }
                                }
                            } else {
                                ElementKind::Path {
                                    commands: scmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 0],
                                    stroke: *solid,
                                    stroke_width: stroke_w,
                                }
                            }
                        }
                    };
                    out.push(Element {
                        id: el_id,
                        name: format!("{} Stroke", sanitize_element_name(&node.name)),
                        transform: base_tf,
                        kind,
                        visible: node.visible,
                        cache_as_bitmap: !filters.is_empty(),
                        blend_mode,
                        filters: if is_last {
                            filters.to_vec()
                        } else {
                            Vec::new()
                        },
                        ..Element::default()
                    });
                    self.authored_elements += 1;
                    self.report.statistics.elements += 1;
                }
            }
        }

        if indep_borders && self.authored_elements + 4 <= MAX_AUTHORED_ELEMENTS {
            for rstroke in &resolved_strokes {
                if self.authored_elements + 4 > MAX_AUTHORED_ELEMENTS {
                    break;
                }
                let (sc_solid, sc_grad) = match rstroke {
                    ResolvedFigmaStroke::Standard { solid, grad } => (*solid, grad.clone()),
                    ResolvedFigmaStroke::Mesh {
                        solid,
                        stops,
                        uv_transform,
                        ..
                    } => (
                        *solid,
                        build_figma_gradient_paint(
                            GradientKind::Radial,
                            stops,
                            *uv_transform,
                            node.width,
                            node.height,
                        ),
                    ),
                };
                let borders =
                    build_independent_border_elements(node, base_tf, sc_solid, sc_grad, blend_mode);
                let added = borders.len();
                self.authored_elements += added;
                self.report.statistics.elements += added;
                out.extend(borders);
            }
        }

        if out.is_empty() && !filters.is_empty() && self.authored_elements < MAX_AUTHORED_ELEMENTS {
            let filter_geom = paths_to_fill
                .first()
                .map(|(c, r)| (c.clone(), *r))
                .or_else(|| {
                    validate_path_commands(&fallback_cmds)
                        .ok()
                        .map(|()| (fallback_cmds.clone(), FillRule::NonZero))
                });
            if let Some((cmds, rule)) = filter_geom {
                out.push(Element {
                    id: new_id("el"),
                    name: sanitize_element_name(&node.name),
                    transform: base_tf,
                    kind: ElementKind::Path {
                        commands: cmds,
                        fill_rule: rule,
                        fill: [0, 0, 0, 0],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    visible: node.visible,
                    cache_as_bitmap: true,
                    blend_mode,
                    filters: filters.to_vec(),
                    ..Element::default()
                });
                self.authored_elements += 1;
                self.report.statistics.elements += 1;
            }
        }

        out
    }

    #[allow(clippy::too_many_arguments)]
    fn build_layers_for_children(
        &mut self,
        children: &[FigmaNode],
        offset_x: f64,
        offset_y: f64,
        clip_bounds: Option<(f64, f64, [f64; 4])>,
        parent_loc: &str,
        expanded_budget: usize,
        depth: usize,
    ) -> (Vec<Layer>, usize, usize) {
        if depth > MAX_SYMBOL_GRAPH_DEPTH
            || expanded_budget == 0
            || self.authored_elements >= MAX_AUTHORED_ELEMENTS
        {
            self.page_truncated |= (expanded_budget == 0
                || self.authored_elements >= MAX_AUTHORED_ELEMENTS)
                && children
                    .iter()
                    .any(|child| child.visible || self.options.import_hidden_layers);
            return (Vec::new(), 0, 1);
        }
        let mut layers = Vec::new();
        let mut active_mask_layer_id: Option<String> = None;
        let mut expanded_used = 0usize;
        let mut max_depth = 1usize;

        // Optional frame clip mask (`clipsContent: true`)
        if self.options.preserve_masks
            && let Some((cw, ch, cr)) = clip_bounds
            && !children.is_empty()
            && self.authored_elements < MAX_AUTHORED_ELEMENTS
        {
            let clip_id = new_id("layer");
            let max_r = 0.5 * cw.min(ch);
            let [tl, tr, br, bl] = cr.map(|v| {
                if v.is_finite() {
                    v.clamp(0.0, max_r)
                } else {
                    0.0
                }
            });
            let nf_r = [tl, tr, bl, br];
            let mut clip_tf = Transform::default();
            if nf_r.iter().any(|&v| v > 0.0) {
                clip_tf.primitive = Some(PrimitiveOptions::Rectangle(RectangleOptions {
                    corner_radii: nf_r,
                    corners_locked: (nf_r[0] - nf_r[1]).abs() < 1e-6
                        && (nf_r[0] - nf_r[2]).abs() < 1e-6
                        && (nf_r[0] - nf_r[3]).abs() < 1e-6,
                }));
            }
            let clip_el = Element {
                id: new_id("el"),
                name: "Clip Bounds".into(),
                transform: clip_tf,
                kind: ElementKind::Rectangle {
                    width: cw,
                    height: ch,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            };
            layers.push(Layer {
                id: clip_id.clone(),
                name: "Frame Clip".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![clip_el],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    mask_mode: MaskMode::Geometry,
                    ..LayerProperties::default()
                },
            });
            self.authored_elements += 1;
            expanded_used += 1;
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            self.report.statistics.elements += 1;
            active_mask_layer_id = Some(clip_id);
        }

        if self.options.layer_mapping == FigmaLayerMapping::FlattenSingleLayer && depth <= 1 {
            let mut all_elements = Vec::new();
            let mut all_bfs = Vec::new();
            let mut all_vfs = Vec::new();
            for child in children {
                if !child.visible && !self.options.import_hidden_layers {
                    continue;
                }
                let rem = expanded_budget.saturating_sub(expanded_used);
                if rem == 0 || self.authored_elements >= MAX_AUTHORED_ELEMENTS {
                    self.page_truncated = true;
                    break;
                }
                let loc = format!("{parent_loc} / {}", child.name);
                let (els, c_exp, c_dep) = self.convert_leaf_to_elements(
                    child,
                    offset_x,
                    offset_y,
                    &loc,
                    rem,
                    depth + 1,
                    &mut all_bfs,
                    &mut all_vfs,
                );
                expanded_used = expanded_used.saturating_add(c_exp);
                max_depth = max_depth.max(c_dep);
                all_elements.extend(els);
            }
            layers.push(Layer {
                id: new_id("layer"),
                name: sanitize_layer_name(parent_loc),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: all_elements,
                    bitmap_fills: all_bfs,
                    vector_effects: all_vfs,
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    mask: active_mask_layer_id,
                    ..LayerProperties::default()
                },
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            return (layers, expanded_used, max_depth);
        }

        for child in children {
            if !child.visible && !self.options.import_hidden_layers {
                continue;
            }
            if self.authored_elements >= MAX_AUTHORED_ELEMENTS {
                self.page_truncated = true;
                if !self.warned_element_budget {
                    self.warned_element_budget = true;
                    self.report.warning(
                        "figma-geometry-compacted",
                        parent_loc.to_owned(),
                        format!("Compacted remaining Figma elements after reaching {MAX_AUTHORED_ELEMENTS} authored elements."),
                    );
                }
                break;
            }
            let rem_budget = expanded_budget.saturating_sub(expanded_used);
            if rem_budget == 0 {
                self.page_truncated = true;
                if !self.warned_expanded_budget {
                    self.warned_expanded_budget = true;
                    self.report.warning(
                        "figma-expanded-node-budget-exceeded",
                        parent_loc.to_owned(),
                        format!("Compacted remaining Figma nodes after reaching {MAX_EXPANDED_SCENE_NODES} expanded scene nodes."),
                    );
                }
                break;
            }
            let loc = format!("{parent_loc} / {}", child.name);

            // A plain, translated frame of leaves needs only a named timeline folder.
            // Each child stays independently editable and retains its own mask, lock and order.
            // A frame with clipping, paint, compositing or a non-translation matrix needs a symbol.
            let shallow_group =
                matches!(child.node_type.as_str(), "GROUP" | "SECTION") && depth <= 3;
            let leaf_frame = child.node_type == "FRAME"
                && child
                    .children
                    .iter()
                    .all(|node| node.children.is_empty() && !node.is_mask);
            let translation = child.relative_transform.map_or_else(
                || (child.rotation_deg == 0.0).then_some([child.x, child.y]),
                |[a, b, c, d, tx, ty]| ([a, b, c, d] == [1., 0., 0., 1.]).then_some([tx, ty]),
            );
            if (shallow_group || leaf_frame)
                && !child.is_mask
                && !child.clips_content
                && child.fills.is_empty()
                && child.strokes.is_empty()
                && child.effects.is_empty()
                && (child.opacity - 1.0).abs() < 1e-6
                && matches!(child.blend_mode.as_str(), "" | "PASS_THROUGH" | "NORMAL")
                && self.options.layer_mapping == FigmaLayerMapping::TimelineLayers
                && !child.children.is_empty()
                && child.children.len() <= 64
                && let Some([tx, ty]) = translation
            {
                let folder_id = new_id("folder");
                layers.push(Layer {
                    id: folder_id.clone(),
                    name: sanitize_layer_name(&child.name),
                    visible: child.visible,
                    locked: child.locked,
                    keyframes: Vec::new(),
                    properties: LayerProperties {
                        role: LayerRole::Folder,
                        ..LayerProperties::default()
                    },
                });
                self.report.statistics.layers += 1;

                let (group_sub_layers, g_exp, g_dep) = self.build_layers_for_children(
                    &child.children,
                    offset_x - tx,
                    offset_y - ty,
                    None,
                    &loc,
                    rem_budget,
                    depth + 1,
                );
                expanded_used = expanded_used.saturating_add(g_exp);
                max_depth = max_depth.max(g_dep);
                for mut sl in group_sub_layers {
                    if sl.properties.parent.is_none() {
                        sl.properties.parent = Some(folder_id.clone());
                    }
                    if sl.properties.mask.is_none() && sl.properties.role == LayerRole::Normal {
                        sl.properties.mask = active_mask_layer_id.clone();
                    }
                    layers.push(sl);
                }
                continue;
            }

            let mut bfs = Vec::new();
            let mut vfs = Vec::new();
            let (mut elements, c_exp, c_dep) = self.convert_leaf_to_elements(
                child,
                offset_x,
                offset_y,
                &loc,
                rem_budget,
                depth + 1,
                &mut bfs,
                &mut vfs,
            );
            expanded_used = expanded_used.saturating_add(c_exp);
            max_depth = max_depth.max(c_dep);

            if self.options.layer_mapping == FigmaLayerMapping::MovieClipSymbols
                && elements.len() == 1
                && !matches!(elements[0].kind, ElementKind::Instance { .. })
                && self.project.symbols.len() < MAX_PROJECT_SYMBOLS
            {
                let sym_id = new_id("figmc");
                let mut inner_el = elements.remove(0);
                let outer_filters = std::mem::take(&mut inner_el.filters);
                let outer_cache = !outer_filters.is_empty() || inner_el.cache_as_bitmap;
                inner_el.cache_as_bitmap = false;
                let inner_bfs = std::mem::take(&mut bfs);
                let inner_vfs = std::mem::take(&mut vfs);
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: sanitize_symbol_name(&child.name),
                    folder: "Figma Layers".into(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: sanitize_layer_name(&child.name),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: vec![inner_el],
                                bitmap_fills: inner_bfs,
                                vector_effects: inner_vfs,
                                ..Keyframe::default()
                            }],
                            properties: LayerProperties::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.report.statistics.symbols = self.project.symbols.len();
                elements.push(Element {
                    id: new_id("el"),
                    name: sanitize_instance_name(&child.name),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    cache_as_bitmap: outer_cache,
                    filters: outer_filters,
                    ..Element::default()
                });
                max_depth = max_depth.max(2);
            }

            let lid = new_id("layer");
            let mut props = LayerProperties {
                blend_mode: self.map_blend_mode(&child.blend_mode, &loc),
                ..LayerProperties::default()
            };

            if child.is_mask && self.options.preserve_masks {
                props.role = LayerRole::Mask;
                props.mask_mode = match child.mask_type.as_str() {
                    "VECTOR" | "OUTLINE" => MaskMode::Geometry,
                    "LUMINANCE" => MaskMode::Luminance,
                    _ => MaskMode::Alpha,
                };
                // Sanitize mask elements so `authored_layers::validate_layers` ALWAYS passes:
                // - `TextBox` has no vector path -> replace with bounding rectangle (`figma-mask-text-bounds-approximated`)
                // - `Instance` may contain nested `TextBox` / `Bitmap` -> replace with bounding rectangle on `Geometry` masks
                // - `Bitmap` on a `Geometry` mask requires paint coverage -> upgrade to `MaskMode::Alpha`
                let mut had_text = false;
                let mut had_instance = false;
                let mut had_bitmap = false;
                for el in &mut elements {
                    match &el.kind {
                        ElementKind::TextBox { width, height, .. } => {
                            let w = (*width).clamp(1.0, 32_768.0);
                            let h = (*height).clamp(1.0, 32_768.0);
                            el.kind = ElementKind::Rectangle {
                                width: w,
                                height: h,
                                fill: [0, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            };
                            had_text = true;
                        }
                        ElementKind::Instance { .. } => {
                            let w = child.width.clamp(1.0, 32_768.0);
                            let h = child.height.clamp(1.0, 32_768.0);
                            el.transform = self.node_transform(child, offset_x, offset_y);
                            el.kind = ElementKind::Rectangle {
                                width: w,
                                height: h,
                                fill: [0, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            };
                            had_instance = true;
                        }
                        ElementKind::Bitmap { .. } => {
                            had_bitmap = true;
                        }
                        _ => {}
                    }
                }
                if elements.is_empty() {
                    elements.push(Element {
                        id: new_id("el"),
                        name: sanitize_element_name(&child.name),
                        transform: self.node_transform(child, offset_x, offset_y),
                        kind: ElementKind::Rectangle {
                            width: child.width.clamp(1.0, 32_768.0),
                            height: child.height.clamp(1.0, 32_768.0),
                            fill: [0, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    });
                    self.authored_elements += 1;
                }
                if had_text {
                    self.report.warning(
                        "figma-mask-text-bounds-approximated",
                        loc.clone(),
                        "Figma text mask approximated as its bounding rectangle mask.",
                    );
                }
                if had_instance {
                    self.report.warning(
                        "figma-mask-instance-bounds-approximated",
                        loc.clone(),
                        "Figma component instance mask approximated as its bounding rectangle mask.",
                    );
                }
                if had_bitmap && props.mask_mode == MaskMode::Geometry {
                    props.mask_mode = MaskMode::Alpha;
                    self.report.warning(
                        "figma-mask-bitmap-alpha",
                        loc.clone(),
                        "Figma vector mask with bitmap fill upgraded to Alpha mask mode.",
                    );
                }
                active_mask_layer_id = Some(lid.clone());
            } else if let Some(mid) = &active_mask_layer_id {
                props.mask = Some(mid.clone());
            }

            layers.push(Layer {
                id: lid,
                name: sanitize_layer_name(&child.name),
                visible: child.visible,
                locked: child.locked,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    bitmap_fills: bfs,
                    vector_effects: vfs,
                    ..Keyframe::default()
                }],
                properties: props,
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
        }

        (layers, expanded_used, max_depth)
    }

    fn build_page_layers(
        &mut self,
        page: &FigmaNode,
        layout: PageLayout,
        primary: bool,
        expanded_budget: usize,
    ) -> (Vec<Layer>, usize) {
        let mut children = page
            .children
            .iter()
            .filter(|n| n.visible || self.options.import_hidden_layers);
        let artboard = children.next().filter(|n| {
            children.next().is_none()
                && matches!(
                    n.node_type.as_str(),
                    "FRAME" | "COMPONENT" | "SYMBOL" | "SECTION"
                )
                && n.visible
                && !n.locked
                && !n.is_mask
                && n.opacity == 1.0
                && matches!(n.blend_mode.as_str(), "" | "NORMAL" | "PASS_THROUGH")
                && n.effects.is_empty()
                && n.strokes.is_empty()
                && n.corner_radii == [0.0; 4]
                && matches!(
                    n.fills.as_slice(),
                    [] | [FigmaPaint::Solid {
                        color: [_, _, _, 255]
                    }]
                )
                && layout.fills_stage(
                    n,
                    (self.project.width, self.project.height),
                    self.options.resize_stage_to_artboard,
                )
        });
        let background = artboard
            .and_then(|n| match n.fills.first() {
                Some(FigmaPaint::Solid { color }) => Some(*color),
                _ => None,
            })
            .or(page.background_color);
        if primary && let Some(bg) = background {
            self.project.background = bg;
        }
        let backdrop = background.filter(|color| *color != self.project.background);
        let budget = expanded_budget.saturating_sub(usize::from(backdrop.is_some()));
        let (mut layers, mut exp, _) = if let Some(n) = artboard {
            let clip =
                n.clips_content
                    .then_some((n.width.max(1.0), n.height.max(1.0), n.corner_radii));
            self.build_layers_for_children(&n.children, 0.0, 0.0, clip, &n.name, budget, 1)
        } else {
            self.build_layers_for_children(
                &page.children,
                layout.origin[0],
                layout.origin[1],
                None,
                &page.name,
                budget,
                1,
            )
        };
        if self.options.resize_stage_to_artboard {
            layout.fit_elements(&mut layers);
        }
        // The document has a shared stage colour. A page with a different canvas colour
        // carries its own stage-sized backdrop so scene navigation and export agree.
        if let Some(color) = backdrop {
            layers.insert(
                0,
                Layer {
                    id: new_id("layer"),
                    name: "Page background".into(),
                    visible: true,
                    locked: true,
                    keyframes: vec![Keyframe {
                        elements: vec![Element {
                            id: new_id("el"),
                            name: "Page background".into(),
                            kind: ElementKind::Rectangle {
                                width: f64::from(self.project.width),
                                height: f64::from(self.project.height),
                                fill: color,
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                },
            );
            exp += 1;
            self.authored_elements += 1;
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            self.report.statistics.elements += 1;
        }
        (layers, exp)
    }

    fn build_single_layer_for_page(
        &mut self,
        page: &FigmaNode,
        layout: PageLayout,
        page_index: usize,
        update_stage_metrics: bool,
        expanded_budget: usize,
    ) -> (Layer, usize) {
        let raw_title = if page.name.trim().is_empty() {
            format!("Page {}", page_index + 1)
        } else {
            page.name.clone()
        };
        let layer_name = sanitize_layer_name(&raw_title);

        if update_stage_metrics && let Some(bg) = page.background_color {
            self.project.background = bg;
        }

        let visible_top: Vec<&FigmaNode> = page
            .children
            .iter()
            .filter(|n| n.visible || self.options.import_hidden_layers)
            .collect();

        let [base_x, base_y] = layout.origin;

        let mut all_elements = Vec::new();
        let mut all_bfs = Vec::new();
        let mut all_vfs = Vec::new();
        let mut expanded_used = 0usize;

        let has_canvas_root_mask =
            self.options.preserve_masks && visible_top.iter().any(|child| child.is_mask);

        if has_canvas_root_mask && self.project.symbols.len() < MAX_PROJECT_SYMBOLS {
            let (sub_layers, exp, _) = self.build_layers_for_children(
                &page.children,
                base_x,
                base_y,
                None,
                &layer_name,
                expanded_budget,
                2,
            );
            if !sub_layers.is_empty() {
                let sym_id = new_id("figpage");
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: sanitize_symbol_name(&layer_name),
                    folder: "Figma Pages".into(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: sub_layers,
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.report.statistics.symbols = self.project.symbols.len();
                all_elements.push(Element {
                    id: new_id("el"),
                    name: sanitize_instance_name(&layer_name),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
                self.authored_elements += 1;
                self.report.statistics.elements += 1;
                expanded_used = expanded_used.saturating_add(exp);
            }
        } else {
            for child in visible_top {
                if self.authored_elements >= MAX_AUTHORED_ELEMENTS {
                    self.page_truncated = true;
                    if !self.warned_element_budget {
                        self.warned_element_budget = true;
                        self.report.warning(
                            "figma-geometry-compacted",
                            layer_name.clone(),
                            format!(
                                "Compacted remaining Figma elements after reaching {MAX_AUTHORED_ELEMENTS} authored elements."
                            ),
                        );
                    }
                    break;
                }
                let rem = expanded_budget.saturating_sub(expanded_used);
                if rem == 0 {
                    self.page_truncated = true;
                    if !self.warned_expanded_budget {
                        self.warned_expanded_budget = true;
                        self.report.warning(
                            "figma-expanded-node-budget-exceeded",
                            layer_name.clone(),
                            format!(
                                "Compacted remaining Figma nodes after reaching {MAX_EXPANDED_SCENE_NODES} expanded scene nodes."
                            ),
                        );
                    }
                    break;
                }
                let loc = format!("{layer_name} / {}", child.name);
                let (els, c_exp, _) = self.convert_leaf_to_elements(
                    child,
                    base_x,
                    base_y,
                    &loc,
                    rem,
                    2,
                    &mut all_bfs,
                    &mut all_vfs,
                );
                expanded_used = expanded_used.saturating_add(c_exp);
                all_elements.extend(els);
            }
        }

        let mut layer = Layer {
            id: new_id("layer"),
            name: layer_name,
            visible: if self.options.selected_page_ids.is_some() {
                true
            } else {
                page.visible
            },
            locked: page.locked,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: all_elements,
                bitmap_fills: all_bfs,
                vector_effects: all_vfs,
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        };
        if self.options.resize_stage_to_artboard {
            layout.fit_elements(std::slice::from_mut(&mut layer));
        }
        (layer, expanded_used)
    }
}

pub(super) fn build_project_from_figma_doc(
    mut doc: FigmaDoc,
    images: &mut FigmaImageArchive<'_>,
    options: &FigmaImportOptions,
    source_label: &str,
) -> Result<(Project, ImportReport), ImportError> {
    reflow_figma_doc(&mut doc);
    let mut project = Project::blank();
    project.name = sanitize_symbol_name(&doc.name);
    // Each static design page occupies exactly one frame, including the root scene.
    let root = project.root.clone();
    if let Some(scene) = project.symbol_mut(&root) {
        scene.timeline.frame_count = 1;
    }
    let mut report = ImportReport {
        source: source_label.to_owned(),
        format: ImportFormat::Figma,
        version: Some(if doc.version_label.is_empty() {
            "Figma".into()
        } else {
            doc.version_label.clone()
        }),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if doc.pages.is_empty() {
        return Err(ImportError::Blocked(
            "Figma document contains no importable pages or frames.".into(),
        ));
    }

    // Separate user-visible `CANVAS` pages from Figma's hidden `Internal Only Canvas`!
    let all_user_pages: Vec<&FigmaNode> = {
        let non_internal: Vec<&FigmaNode> = doc.pages.iter().filter(|p| !p.internal_only).collect();
        if !non_internal.is_empty() {
            non_internal
        } else {
            doc.pages.iter().collect()
        }
    };

    let user_pages: Vec<&FigmaNode> = if let Some(sel_ids) = &options.selected_page_ids {
        if sel_ids.is_empty() {
            return Err(ImportError::Blocked(
                "No Figma pages were selected for import.".into(),
            ));
        }
        let by_id: HashMap<&str, &FigmaNode> =
            all_user_pages.iter().map(|&p| (p.id.as_str(), p)).collect();
        let mut chosen = Vec::with_capacity(sel_ids.len());
        let mut seen_ids = HashSet::new();
        for sid in sel_ids {
            if let Some(&page) = by_id.get(sid.as_str())
                && seen_ids.insert(sid.as_str())
            {
                chosen.push(page);
            }
        }
        if chosen.is_empty() {
            return Err(ImportError::Blocked(
                "None of the selected Figma page IDs exist in the document.".into(),
            ));
        }
        chosen
    } else {
        all_user_pages
    };

    let mut layouts = HashMap::new();
    let mut stage = (0, 0);
    for &page in &user_pages {
        let layout = PageLayout::measure(page, options.import_hidden_layers);
        if options.resize_stage_to_artboard
            && (page.visible || options.import_hidden_layers || options.selected_page_ids.is_some())
        {
            let (width, height) = layout.stage_extent();
            stage = (stage.0.max(width), stage.1.max(height));
            if layout.scale() < 1.0 {
                report.warning(
                    "figma-page-fitted-to-stage",
                    format!("{} ({})", page.name, page.id),
                    format!("Fit the complete {:.0} × {:.0} Figma page to {width} × {height} stage pixels at {:.2}% scale.", layout.width, layout.height, layout.scale() * 100.0),
                );
            }
        }
        layouts.insert(&page.id, layout);
    }
    if stage != (0, 0) {
        project.width = stage.0.max(1);
        project.height = stage.1.max(1);
    }

    // Collect master `COMPONENT` / `SYMBOL` definitions across ALL pages (including `Internal Only Canvas`).
    let mut master_nodes: HashMap<String, &FigmaNode> = HashMap::new();
    let mut ordered_master_ids: Vec<String> = Vec::new();
    for page in &doc.pages {
        collect_master_nodes(page, &mut master_nodes, &mut ordered_master_ids, 0);
    }

    let selective =
        options.selected_page_ids.is_some() || options.page_mapping == FigmaPageMapping::Layers;

    let mut ctx = FigmaBuildCtx {
        options,
        master_nodes,
        component_symbols: HashMap::new(),
        building_components: HashSet::new(),
        variant_cache: HashMap::new(),
        reflow_ctx: ReflowCtx::new(),
        image_resources: HashMap::new(),
        project: &mut project,
        report: &mut report,
        authored_elements: 0,
        page_truncated: false,
        warned_element_budget: false,
        warned_expanded_budget: false,
        warned_symbol_budget: false,
        warned_override_budget: false,
    };

    // Lazily load, deduplicate, and downscale ONLY referenced embedded bitmap assets!
    prepare_referenced_images(
        &doc,
        &user_pages,
        &ordered_master_ids,
        selective,
        images,
        &mut ctx,
    );

    // Pre-materialize library components up to a safe cap ONLY in non-selective legacy `Scenes` mode
    // so uninstantiated components appear in design-system imports without slowing down page-selective
    // or 1-page-per-layer imports.
    if !selective {
        for cid in ordered_master_ids.into_iter().take(1_500) {
            if ctx.project.symbols.len() >= MAX_PROJECT_SYMBOLS / 2
                || ctx.authored_elements >= MAX_AUTHORED_ELEMENTS / 2
            {
                break;
            }
            let _ = ctx.ensure_component_symbol(&cid);
        }
    }

    ctx.page_truncated = false;
    match options.page_mapping {
        FigmaPageMapping::Layers => {
            let pages_for_layers: Vec<&FigmaNode> = if options.selected_page_ids.is_some() {
                user_pages.clone()
            } else {
                let vis: Vec<&FigmaNode> = user_pages
                    .iter()
                    .copied()
                    .filter(|p| p.visible || options.import_hidden_layers)
                    .collect();
                if vis.is_empty() {
                    vec![user_pages[0]]
                } else {
                    vis
                }
            };

            let mut expanded_used = 1usize;
            let mut page_layers = Vec::with_capacity(pages_for_layers.len());
            for (idx, page) in pages_for_layers.into_iter().enumerate() {
                let rem = MAX_EXPANDED_SCENE_NODES.saturating_sub(expanded_used);
                if rem == 0 || ctx.authored_elements >= MAX_AUTHORED_ELEMENTS {
                    ctx.warn_page_limit(page, true);
                    continue;
                }
                let (layer, p_exp) =
                    ctx.build_single_layer_for_page(page, layouts[&page.id], idx, idx == 0, rem);
                expanded_used = expanded_used.saturating_add(p_exp);
                if ctx.retain_page(page, p_exp) {
                    page_layers.push(layer);
                    ctx.report.statistics.layers += 1;
                    ctx.report.statistics.keyframes += 1;
                }
            }
            let root_id = ctx.project.root.clone();
            if let Some(root_sym) = ctx.project.symbol_mut(&root_id)
                && !page_layers.is_empty()
            {
                root_sym.timeline.layers = page_layers;
            }
        }
        FigmaPageMapping::Scenes => {
            let mut scene_pages = user_pages.iter().copied().filter(|p| {
                options.selected_page_ids.is_some() || p.visible || options.import_hidden_layers
            });
            let primary_page = scene_pages.next().unwrap_or(user_pages[0]);

            // Separate scenes are never evaluated at the same time.
            let scene_budget = MAX_EXPANDED_SCENE_NODES - 1;
            let (root_layers, root_exp) =
                ctx.build_page_layers(primary_page, layouts[&primary_page.id], true, scene_budget);
            ctx.retain_page(primary_page, root_exp);

            let root_id = ctx.project.root.clone();
            if let Some(root_sym) = ctx.project.symbol_mut(&root_id) {
                if user_pages.len() > 1 && !primary_page.name.trim().is_empty() {
                    root_sym.name = sanitize_symbol_name(&primary_page.name);
                }
                if !root_layers.is_empty() {
                    root_sym.timeline.layers = root_layers;
                }
            }

            // Even an empty design page is an editable scene in the source page order.
            let mut scene_ids = vec![root_id];
            for (idx, extra_page) in scene_pages.enumerate() {
                if ctx.project.symbols.len() >= MAX_PROJECT_SYMBOLS
                    || ctx.authored_elements >= MAX_AUTHORED_ELEMENTS
                {
                    ctx.warn_page_limit(extra_page, true);
                    continue;
                }
                let (mut page_layers, p_exp) =
                    ctx.build_page_layers(extra_page, layouts[&extra_page.id], false, scene_budget);
                if !ctx.retain_page(extra_page, p_exp) {
                    continue;
                }
                if page_layers.is_empty() {
                    page_layers.push(Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    });
                }
                let scene_id = new_id("figscene");
                let fallback_title = format!("Page {}", idx + 2);
                let scene_title = if extra_page.name.trim().is_empty() {
                    &fallback_title
                } else {
                    extra_page.name.as_str()
                };
                ctx.project.symbols.push(Symbol {
                    id: scene_id.clone(),
                    name: sanitize_symbol_name(scene_title),
                    folder: String::new(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: page_layers,
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                scene_ids.push(scene_id);
            }
            if scene_ids.len() > 1 {
                ctx.project.scenes = scene_ids;
            }
        }
    }

    finalize_figma_project_format(&mut project);
    project.validate().map_err(|e| {
        ImportError::Blocked(format!("Imported Figma composition failed validation: {e}"))
    })?;
    report.statistics.symbols = project.symbols.len();
    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "figma-composition-imported".into(),
        location: source_label.to_owned(),
        message: format!(
            "Imported Figma composition '{}' into {} layer(s), {} symbol(s) and {} element(s).",
            project.name,
            report.statistics.layers,
            report.statistics.symbols,
            report.statistics.elements
        ),
    });
    report.can_import = true;
    Ok((project, report))
}

fn finalize_figma_project_format(project: &mut Project) {
    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    if !project.library_folders.is_empty()
        || project.symbols.iter().any(|s| {
            !s.folder.is_empty() || s.timeline.layers.iter().any(|l| !l.properties.is_default())
        })
    {
        project.format_version = project.format_version.max(4);
    }
    if project.symbols.iter().any(|s| {
        s.timeline.layers.iter().any(|l| {
            l.properties.end_frame.is_some()
                || l.keyframes.iter().any(|k| {
                    k.elements.iter().any(|e| {
                        e.transform.skew != 0.0
                            || !e.transform.color_transform.is_identity()
                            || matches!(e.kind, ElementKind::PaintedPath { .. })
                    })
                })
        })
    }) {
        project.format_version = project.format_version.max(5);
    }
    project.upgrade_format_for_features();
}

fn clipboard_node_local_matrix(node: &FigmaNode) -> [f64; 6] {
    node.relative_transform
        .filter(|matrix| matrix.iter().all(|v| v.is_finite()))
        .unwrap_or_else(|| {
            let (sin, cos) = node.rotation_deg.to_radians().sin_cos();
            [cos, sin, -sin, cos, node.x, node.y]
        })
}

fn include_transformed_rect(
    bounds: &mut Option<neoflash_core::PathBounds>,
    matrix: [f64; 6],
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) {
    let [a, b, c, d, tx, ty] = matrix;
    for (u, v) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        let px = a * u + c * v + tx;
        let py = b * u + d * v + ty;
        if px.is_finite() && py.is_finite() {
            let bb = bounds.get_or_insert(neoflash_core::PathBounds {
                x_min: px,
                x_max: px,
                y_min: py,
                y_max: py,
            });
            bb.x_min = bb.x_min.min(px);
            bb.y_min = bb.y_min.min(py);
            bb.x_max = bb.x_max.max(px);
            bb.y_max = bb.y_max.max(py);
        }
    }
}

fn measure_clipboard_node_bounds(
    bounds: &mut Option<neoflash_core::PathBounds>,
    node: &FigmaNode,
    parent: [f64; 6],
    include_hidden: bool,
    top: bool,
    depth: usize,
) {
    if depth > 64 || (!node.visible && !include_hidden) {
        return;
    }
    let matrix = neoflash_core::affine_multiply(parent, clipboard_node_local_matrix(node));
    let mut used_vector_paths = false;
    if node.children.is_empty()
        && matches!(
            node.node_type.as_str(),
            "VECTOR" | "BOOLEAN_OPERATION" | "LINE" | "STAR" | "REGULAR_POLYGON"
        )
        && (!node.fill_paths.is_empty() || !node.stroke_paths.is_empty())
    {
        let mut local_path_bb: Option<neoflash_core::PathBounds> = None;
        for (cmds, _) in &node.fill_paths {
            if let Ok(Some(pb)) = neoflash_core::path_bounds(cmds) {
                let dst = local_path_bb.get_or_insert(pb);
                dst.x_min = dst.x_min.min(pb.x_min);
                dst.y_min = dst.y_min.min(pb.y_min);
                dst.x_max = dst.x_max.max(pb.x_max);
                dst.y_max = dst.y_max.max(pb.y_max);
            }
        }
        for (cmds, _) in &node.stroke_paths {
            if let Ok(Some(pb)) = neoflash_core::path_bounds(cmds) {
                let dst = local_path_bb.get_or_insert(pb);
                dst.x_min = dst.x_min.min(pb.x_min);
                dst.y_min = dst.y_min.min(pb.y_min);
                dst.x_max = dst.x_max.max(pb.x_max);
                dst.y_max = dst.y_max.max(pb.y_max);
            }
        }
        if let Some(pb) = local_path_bb {
            include_transformed_rect(bounds, matrix, pb.x_min, pb.y_min, pb.x_max, pb.y_max);
            used_vector_paths = true;
        }
    }

    if !used_vector_paths {
        let is_group_without_paint = node.node_type == "GROUP"
            && !node.children.is_empty()
            && node.fills.is_empty()
            && node.strokes.is_empty()
            && node.effects.is_empty();
        let visible_bounds = !is_group_without_paint
            && (top
                || node.clips_content
                || node.children.is_empty()
                || !node.fills.is_empty()
                || !node.strokes.is_empty()
                || !node.effects.is_empty());
        if visible_bounds {
            include_transformed_rect(
                bounds,
                matrix,
                0.0,
                0.0,
                node.width.max(0.0),
                node.height.max(0.0),
            );
        }
    }

    if !node.clips_content {
        for child in &node.children {
            measure_clipboard_node_bounds(bounds, child, matrix, include_hidden, false, depth + 1);
        }
    }
}

pub(super) fn collect_unresolved_image_refs(
    doc: &FigmaDoc,
    existing: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let user_pages: Vec<&FigmaNode> = {
        let non_internal: Vec<&FigmaNode> = doc.pages.iter().filter(|p| !p.internal_only).collect();
        if non_internal.is_empty() {
            doc.pages.iter().collect()
        } else {
            non_internal
        }
    };
    let mut master_nodes: HashMap<String, &FigmaNode> = HashMap::new();
    let mut ordered_master_ids: Vec<String> = Vec::new();
    for page in &doc.pages {
        collect_master_nodes(page, &mut master_nodes, &mut ordered_master_ids, 0);
    }
    let mut refs = ReferencedImages::default();
    let mut visited_masters = HashSet::new();
    let mut ref_masters = Vec::new();
    for &page in &user_pages {
        collect_referenced_image_keys(page, true, true, &mut refs);
        collect_transitively_referenced_masters(
            page,
            true,
            false,
            &master_nodes,
            &mut visited_masters,
            &mut ref_masters,
        );
    }
    for mnode in ref_masters {
        collect_referenced_image_keys(mnode, true, true, &mut refs);
    }
    refs.ordered
        .into_iter()
        .filter(|key| {
            let low = key.to_ascii_lowercase();
            !existing.contains_key(key) && !existing.contains_key(&low)
        })
        .collect()
}

pub(super) type FigmaClipboardProjectResult = (
    Project,
    ImportReport,
    Option<neoflash_core::PathBounds>,
    Option<neoflash_core::PathBounds>,
);

/// Build a 1:1 (`scale = 1.0`) NeoFlash [`Project`] from a Figma clipboard `fig-kiwi` document
/// without auto-downscaling or artboard-dissolving, preserving exact editable vectors, strokes,
/// gradients, corner radii, editable text, filters, clipping masks, and `Internal Only Canvas`
/// component symbols.
pub(super) fn build_clipboard_project_from_figma_doc(
    mut doc: FigmaDoc,
    images: &mut FigmaImageArchive<'_>,
    source_label: &str,
) -> Result<FigmaClipboardProjectResult, ImportError> {
    reflow_figma_doc(&mut doc);
    let options = FigmaImportOptions {
        layer_mapping: FigmaLayerMapping::TimelineLayers,
        import_hidden_layers: true,
        preserve_masks: true,
        preserve_blend_modes: true,
        resize_stage_to_artboard: false,
        selected_page_ids: None,
        page_mapping: FigmaPageMapping::Layers,
    };

    let mut project = Project::blank();
    project.name = sanitize_symbol_name(&doc.name);
    let root = project.root.clone();
    if let Some(scene) = project.symbol_mut(&root) {
        scene.timeline.frame_count = 1;
    }

    let mut report = ImportReport {
        source: source_label.to_owned(),
        format: ImportFormat::Figma,
        version: Some(if doc.version_label.is_empty() {
            "Figma Clipboard".into()
        } else {
            doc.version_label.clone()
        }),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if doc.pages.is_empty() {
        return Err(ImportError::Blocked(
            "Figma clipboard payload contains no pages or nodes.".into(),
        ));
    }

    let user_pages: Vec<&FigmaNode> = {
        let non_internal: Vec<&FigmaNode> = doc.pages.iter().filter(|p| !p.internal_only).collect();
        if non_internal.is_empty() {
            doc.pages.iter().collect()
        } else {
            non_internal
        }
    };

    let any_visible_top = user_pages
        .iter()
        .flat_map(|p| &p.children)
        .any(|c| c.visible);
    let include_hidden_top = !any_visible_top;

    let mut figma_bounds: Option<neoflash_core::PathBounds> = None;
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for &page in &user_pages {
        for child in &page.children {
            if !child.visible && !include_hidden_top {
                continue;
            }
            measure_clipboard_node_bounds(&mut figma_bounds, child, identity, true, true, 0);
        }
    }

    // Keep exact Figma canvas coordinates directly whenever within [-50_000, 50_000];
    // if copied from far out beyond 50_000 on Figma's infinite canvas, rebase by `x_min, y_min`
    // so `node_transform` never clamps extreme offsets while preserving exact 1:1 relative geometry.
    let coord_offset = match figma_bounds {
        Some(bb)
            if bb.x_min.abs() <= 50_000.0
                && bb.x_max.abs() <= 50_000.0
                && bb.y_min.abs() <= 50_000.0
                && bb.y_max.abs() <= 50_000.0 =>
        {
            [0.0, 0.0]
        }
        Some(bb) => [bb.x_min, bb.y_min],
        None => [0.0, 0.0],
    };

    if let Some(bb) = figma_bounds {
        project.width = bb.width().ceil().clamp(1.0, 8192.0) as u32;
        project.height = bb.height().ceil().clamp(1.0, 8192.0) as u32;
    }

    let mut master_nodes: HashMap<String, &FigmaNode> = HashMap::new();
    let mut ordered_master_ids: Vec<String> = Vec::new();
    for page in &doc.pages {
        collect_master_nodes(page, &mut master_nodes, &mut ordered_master_ids, 0);
    }

    let mut ctx = FigmaBuildCtx {
        options: &options,
        master_nodes,
        component_symbols: HashMap::new(),
        building_components: HashSet::new(),
        variant_cache: HashMap::new(),
        reflow_ctx: ReflowCtx::new(),
        image_resources: HashMap::new(),
        project: &mut project,
        report: &mut report,
        authored_elements: 0,
        page_truncated: false,
        warned_element_budget: false,
        warned_expanded_budget: false,
        warned_symbol_budget: false,
        warned_override_budget: false,
    };

    prepare_referenced_images(
        &doc,
        &user_pages,
        &ordered_master_ids,
        true,
        images,
        &mut ctx,
    );

    let mut all_elements = Vec::new();
    let mut all_bfs = Vec::new();
    let mut all_vfs = Vec::new();
    let mut expanded_used = 1usize;

    for &page in &user_pages {
        let top_children: Vec<&FigmaNode> = page
            .children
            .iter()
            .filter(|n| n.visible || include_hidden_top)
            .collect();
        if top_children.is_empty() {
            continue;
        }
        let page_name = if page.name.trim().is_empty() {
            "Figma Paste"
        } else {
            page.name.as_str()
        };
        let has_canvas_root_mask = top_children.iter().any(|child| child.is_mask);
        if has_canvas_root_mask && ctx.project.symbols.len() < MAX_PROJECT_SYMBOLS {
            let mut page_bb = None;
            for &child in &top_children {
                measure_clipboard_node_bounds(&mut page_bb, child, identity, true, true, 0);
            }
            let raw_origin = page_bb.map_or([0.0, 0.0], |bb| [bb.x_min, bb.y_min]);
            let rem = MAX_EXPANDED_SCENE_NODES.saturating_sub(expanded_used);
            let (sub_layers, exp, _) = ctx.build_layers_for_children(
                &page.children,
                raw_origin[0],
                raw_origin[1],
                None,
                page_name,
                rem,
                2,
            );
            if !sub_layers.is_empty() {
                let sym_id = new_id("figclip");
                ctx.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: sanitize_symbol_name(page_name),
                    folder: "Figma Layers".into(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: sub_layers,
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                ctx.report.statistics.symbols = ctx.project.symbols.len();
                all_elements.push(Element {
                    id: new_id("el"),
                    name: sanitize_instance_name(page_name),
                    transform: Transform {
                        x: raw_origin[0] - coord_offset[0],
                        y: raw_origin[1] - coord_offset[1],
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
                ctx.authored_elements += 1;
                ctx.report.statistics.elements += 1;
                expanded_used = expanded_used.saturating_add(exp);
            }
        } else {
            for child in top_children {
                if ctx.authored_elements >= MAX_AUTHORED_ELEMENTS {
                    break;
                }
                let rem = MAX_EXPANDED_SCENE_NODES.saturating_sub(expanded_used);
                if rem == 0 {
                    break;
                }
                let loc = format!("{page_name} / {}", child.name);
                let (mut els, c_exp, _) = ctx.convert_leaf_to_elements(
                    child,
                    coord_offset[0],
                    coord_offset[1],
                    &loc,
                    rem,
                    2,
                    &mut all_bfs,
                    &mut all_vfs,
                );
                if include_hidden_top && !child.visible {
                    for el in &mut els {
                        el.visible = true;
                    }
                }
                expanded_used = expanded_used.saturating_add(c_exp);
                all_elements.extend(els);
            }
        }
    }

    if all_elements.is_empty() {
        return Err(ImportError::Blocked(
            "Figma clipboard selection contains no supported visual elements.".into(),
        ));
    }

    let root_id = project.root.clone();
    if let Some(root_sym) = project.symbol_mut(&root_id) {
        root_sym.timeline.layers = vec![Layer {
            id: new_id("layer"),
            name: "Figma Clipboard".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: all_elements,
                bitmap_fills: all_bfs,
                vector_effects: all_vfs,
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        }];
    }
    report.statistics.layers += 1;
    report.statistics.keyframes += 1;

    finalize_figma_project_format(&mut project);
    project.validate().map_err(|e| {
        ImportError::Blocked(format!("Imported Figma clipboard failed validation: {e}"))
    })?;
    report.statistics.symbols = project.symbols.len();
    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "figma-clipboard-imported".into(),
        location: source_label.to_owned(),
        message: format!(
            "Imported Figma clipboard '{}' at 1:1 scale ({} symbol(s), {} element(s)).",
            project.name, report.statistics.symbols, report.statistics.elements
        ),
    });
    report.can_import = true;

    let local_bounds = figma_bounds.map(|bb| neoflash_core::PathBounds {
        x_min: bb.x_min - coord_offset[0],
        x_max: bb.x_max - coord_offset[0],
        y_min: bb.y_min - coord_offset[1],
        y_max: bb.y_max - coord_offset[1],
    });

    Ok((project, report, figma_bounds, local_bounds))
}

fn collect_master_nodes<'b>(
    node: &'b FigmaNode,
    master_nodes: &mut HashMap<String, &'b FigmaNode>,
    ordered_ids: &mut Vec<String>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    if matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL")
        && !node.is_state_group
        && !master_nodes.contains_key(&node.id)
    {
        master_nodes.insert(node.id.clone(), node);
        ordered_ids.push(node.id.clone());
    }
    for child in &node.children {
        collect_master_nodes(child, master_nodes, ordered_ids, depth + 1);
    }
}

impl FigmaNode {
    fn is_rectangular(&self) -> bool {
        matches!(
            self.node_type.as_str(),
            "RECTANGLE" | "ROUNDED_RECTANGLE" | "FRAME" | "COMPONENT" | "SYMBOL" | "SECTION"
        )
    }

    fn may_use_bitmap_instance(&self, scale_mode: &str, image_transform: Option<[f64; 6]>) -> bool {
        self.is_rectangular()
            && self.fill_paths.is_empty()
            && self.corner_radii.iter().all(|&r| r <= 0.0)
            && !matches!(scale_mode, "TILE" | "CROP" | "STRETCH" | "FIT")
            && image_transform.is_none_or(is_identity_uv_matrix)
    }

    fn uses_bitmap_instance(
        &self,
        iw: u32,
        ih: u32,
        scale_mode: &str,
        image_transform: Option<[f64; 6]>,
    ) -> bool {
        if !self.may_use_bitmap_instance(scale_mode, image_transform)
            || iw == 0
            || ih == 0
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return false;
        }
        let shape_aspect = self.width / self.height;
        let img_aspect = f64::from(iw) / f64::from(ih);
        shape_aspect.is_finite()
            && img_aspect.is_finite()
            && (shape_aspect - img_aspect).abs() <= 1e-3
    }
}

fn is_identity_uv_matrix(m: [f64; 6]) -> bool {
    (m[0] - 1.0).abs() <= 1e-6
        && m[1].abs() <= 1e-6
        && m[2].abs() <= 1e-6
        && m[3].abs() <= 1e-6
        && (m[4] - 1.0).abs() <= 1e-6
        && m[5].abs() <= 1e-6
}

#[derive(Default)]
struct ReferencedImages {
    ordered: Vec<String>,
    seen: HashSet<String>,
    placed: HashSet<String>,
}

impl ReferencedImages {
    fn record(&mut self, key: &str, placed: bool) {
        let key = key.to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        if self.seen.insert(key.clone()) {
            self.ordered.push(key.clone());
        }
        if placed {
            self.placed.insert(key);
        }
    }
}

fn collect_referenced_image_keys(
    node: &FigmaNode,
    include_hidden: bool,
    is_root_or_master: bool,
    refs: &mut ReferencedImages,
) {
    if !is_root_or_master && !node.visible && !include_hidden {
        return;
    }
    for p in &node.fills {
        if let FigmaPaint::Image {
            image_ref,
            scale_mode,
            image_transform,
            ..
        } = p
        {
            refs.record(
                image_ref,
                node.may_use_bitmap_instance(scale_mode, *image_transform),
            );
        }
    }
    for p in &node.strokes {
        if let FigmaPaint::Image { image_ref, .. } = p {
            refs.record(image_ref, false);
        }
    }
    for ov in &node.symbol_overrides {
        for plist in [&ov.fills, &ov.strokes].into_iter().flatten() {
            for p in plist {
                if let FigmaPaint::Image { image_ref, .. } = p {
                    // An override can replace a direct bitmap on the master.
                    refs.record(image_ref, true);
                }
            }
        }
    }
    for child in &node.children {
        collect_referenced_image_keys(
            child,
            include_hidden,
            is_root_or_master && matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL"),
            refs,
        );
    }
}

fn collect_transitively_referenced_masters<'b>(
    node: &'b FigmaNode,
    include_hidden: bool,
    in_master: bool,
    master_nodes: &HashMap<String, &'b FigmaNode>,
    visited_masters: &mut HashSet<String>,
    ordered_masters: &mut Vec<&'b FigmaNode>,
) {
    if !in_master && !node.visible && !include_hidden && node.node_type != "CANVAS" {
        return;
    }
    let mut visit_cid = |cid: &str| {
        if visited_masters.insert(cid.to_owned())
            && let Some(&mnode) = master_nodes.get(cid)
        {
            ordered_masters.push(mnode);
            collect_transitively_referenced_masters(
                mnode,
                true,
                true,
                master_nodes,
                visited_masters,
                ordered_masters,
            );
        }
    };
    if let Some(cid) = &node.component_id {
        visit_cid(cid);
    }
    for ov in &node.symbol_overrides {
        if let Some(ov_sym) = &ov.overridden_symbol_id {
            visit_cid(ov_sym);
        }
    }
    for child in &node.children {
        collect_transitively_referenced_masters(
            child,
            include_hidden,
            in_master,
            master_nodes,
            visited_masters,
            ordered_masters,
        );
    }
}

fn prepare_referenced_images<'b>(
    doc: &FigmaDoc,
    user_pages: &[&'b FigmaNode],
    ordered_master_ids: &[String],
    selective: bool,
    images: &mut FigmaImageArchive<'_>,
    ctx: &mut FigmaBuildCtx<'_, 'b>,
) {
    if !images.has_any() {
        return;
    }
    let mut refs = ReferencedImages::default();
    let include_hidden = ctx.options.import_hidden_layers;
    let mut visited_masters = HashSet::new();
    let mut ref_masters = Vec::new();

    for &page in user_pages {
        collect_referenced_image_keys(page, include_hidden, true, &mut refs);
        collect_transitively_referenced_masters(
            page,
            include_hidden,
            false,
            &ctx.master_nodes,
            &mut visited_masters,
            &mut ref_masters,
        );
    }
    if !selective {
        for cid in ordered_master_ids.iter().take(1_500) {
            if visited_masters.insert(cid.clone())
                && let Some(&mnode) = ctx.master_nodes.get(cid)
            {
                ref_masters.push(mnode);
            }
        }
    }
    for mnode in ref_masters {
        collect_referenced_image_keys(mnode, true, true, &mut refs);
    }
    let ReferencedImages {
        ordered: mut ordered_refs,
        placed,
        ..
    } = refs;
    if ordered_refs.is_empty() {
        return;
    }

    if ordered_refs.len() > MAX_IMPORTED_BITMAPS {
        let excess = ordered_refs.len() - MAX_IMPORTED_BITMAPS;
        ctx.report.warning(
            "figma-image-skipped",
            doc.name.clone(),
            format!(
                "Document references {} distinct images; imported first {MAX_IMPORTED_BITMAPS} bitmaps and approximated {excess} as placeholder shapes to fit NeoFlash's 256-bitmap limit.",
                ordered_refs.len()
            ),
        );
        ordered_refs.truncate(MAX_IMPORTED_BITMAPS);
    }

    struct RawImageCandidate {
        key: String,
        bytes: Vec<u8>,
        format: ImageFormat,
        orig_w: u32,
        orig_h: u32,
        target_w: u32,
        target_h: u32,
        placed: bool,
    }

    let mut candidates = Vec::with_capacity(ordered_refs.len());
    for key in ordered_refs {
        if !images.has_image(&key) {
            continue;
        }
        let raw_bytes = match images.load_image(&key) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(err) => {
                ctx.report.warning("figma-image-skipped", key, err);
                continue;
            }
        };
        let Ok(fmt) = image::guess_format(&raw_bytes) else {
            ctx.report.warning(
                "figma-corrupt-image-blob",
                key.clone(),
                format!(
                    "Figma image fill '{key}' could not be decoded; converted to placeholder shape."
                ),
            );
            continue;
        };
        let mut limits = Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(64 * 1024 * 1024);
        let mut reader = ImageReader::with_format(Cursor::new(&raw_bytes), fmt);
        reader.limits(limits);
        let Ok((ow, oh)) = reader.into_dimensions() else {
            ctx.report.warning(
                "figma-corrupt-image-blob",
                key.clone(),
                format!("Figma image fill '{key}' dimensions could not be read; converted to placeholder shape."),
            );
            continue;
        };
        if ow == 0
            || oh == 0
            || ow > 16_384
            || oh > 16_384
            || u64::from(ow)
                .saturating_mul(u64::from(oh))
                .saturating_mul(4)
                > 64 * 1024 * 1024
        {
            ctx.report.warning(
                "figma-image-skipped",
                key.clone(),
                format!(
                    "Figma image fill '{key}' ({ow}x{oh}) exceeds the 64 MiB pixel safety limit; converted to placeholder shape."
                ),
            );
            continue;
        }
        let (mut tw, mut th) = (ow, oh);
        if tw > MAX_BITMAP_DIMENSION || th > MAX_BITMAP_DIMENSION {
            let scale = (MAX_BITMAP_DIMENSION as f64 / f64::from(tw))
                .min(MAX_BITMAP_DIMENSION as f64 / f64::from(th));
            tw = ((f64::from(tw) * scale).round() as u32).clamp(1, MAX_BITMAP_DIMENSION);
            th = ((f64::from(th) * scale).round() as u32).clamp(1, MAX_BITMAP_DIMENSION);
        }
        candidates.push(RawImageCandidate {
            placed: placed.contains(&key),
            key,
            bytes: raw_bytes,
            format: fmt,
            orig_w: ow,
            orig_h: oh,
            target_w: tw,
            target_h: th,
        });
    }

    if candidates.is_empty() {
        return;
    }

    // Only directly placed resources must fit simultaneously in the mutable pixel arena.
    loop {
        let total_px: u64 = candidates
            .iter()
            .filter(|c| c.placed)
            .map(|c| u64::from(c.target_w) * u64::from(c.target_h))
            .sum();
        if total_px <= MAX_PLACED_BITMAP_PIXELS {
            break;
        }
        let small_px: u64 = candidates
            .iter()
            .filter(|c| c.placed)
            .filter(|c| u64::from(c.target_w) * u64::from(c.target_h) <= 65_536)
            .map(|c| u64::from(c.target_w) * u64::from(c.target_h))
            .sum();
        let large_px = total_px.saturating_sub(small_px);
        if large_px == 0 {
            let factor = ((MAX_PLACED_BITMAP_PIXELS as f64) / (total_px as f64))
                .sqrt()
                .min(0.95);
            for c in candidates.iter_mut().filter(|c| c.placed) {
                c.target_w = ((f64::from(c.target_w) * factor).floor() as u32).max(1);
                c.target_h = ((f64::from(c.target_h) * factor).floor() as u32).max(1);
            }
        } else {
            let avail_large = MAX_PLACED_BITMAP_PIXELS.saturating_sub(small_px).max(1);
            let factor = ((avail_large as f64) / (large_px as f64)).sqrt().min(0.95);
            for c in candidates.iter_mut().filter(|c| c.placed) {
                if u64::from(c.target_w) * u64::from(c.target_h) > 65_536 {
                    c.target_w = ((f64::from(c.target_w) * factor).floor() as u32).max(1);
                    c.target_h = ((f64::from(c.target_h) * factor).floor() as u32).max(1);
                }
            }
        }
    }

    let raw_sum_all: usize = candidates.iter().map(|c| c.bytes.len()).sum();
    let must_compress_aggressively = raw_sum_all > MAX_TOTAL_IMAGE_RAW_BYTES;
    let mut used_raw_bytes = 0usize;
    let mut downscaled_count = 0usize;

    for (idx, c) in candidates.into_iter().enumerate() {
        let supported_native = matches!(
            c.format,
            ImageFormat::Png
                | ImageFormat::Jpeg
                | ImageFormat::WebP
                | ImageFormat::Gif
                | ImageFormat::Bmp
                | ImageFormat::Tiff
        );
        let needs_resize = c.target_w != c.orig_w || c.target_h != c.orig_h;
        let fits_untouched = supported_native
            && !needs_resize
            && c.bytes.len() <= MAX_SINGLE_IMAGE_RAW_BYTES
            && (!must_compress_aggressively || c.bytes.len() <= 128 * 1024)
            && used_raw_bytes + c.bytes.len() <= MAX_TOTAL_IMAGE_RAW_BYTES;

        let (final_bytes, media_type, ext, was_transcoded, dw, dh) = if fits_untouched {
            let (mt, ex) = match c.format {
                ImageFormat::Png => ("image/png", "png"),
                ImageFormat::Jpeg => ("image/jpeg", "jpg"),
                ImageFormat::WebP => ("image/webp", "webp"),
                ImageFormat::Gif => ("image/gif", "gif"),
                ImageFormat::Bmp => ("image/bmp", "bmp"),
                ImageFormat::Tiff => ("image/tiff", "tiff"),
                _ => ("image/png", "png"),
            };
            (c.bytes, mt, ex, false, c.orig_w, c.orig_h)
        } else {
            let Some((enc_bytes, mt, ex, out_w, out_h)) =
                decode_and_reencode_image(&c.bytes, c.format, c.target_w, c.target_h)
            else {
                ctx.report.warning(
                    "figma-corrupt-image-blob",
                    c.key.clone(),
                    format!(
                        "Figma image fill '{}' could not be decoded or transcoded; converted to placeholder shape.",
                        c.key
                    ),
                );
                continue;
            };
            (enc_bytes, mt, ex, true, out_w, out_h)
        };

        if final_bytes.len() > MAX_SINGLE_IMAGE_RAW_BYTES
            || used_raw_bytes + final_bytes.len() > 56 * 1024 * 1024
        {
            ctx.report.warning(
                "figma-image-skipped",
                c.key.clone(),
                format!("Skipped Figma image '{}' after aggregate project resource budget reached 56 MiB.", c.key),
            );
            continue;
        }

        let res_id = new_id("figimg");
        let short_ref: String = c
            .key
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(16)
            .collect();
        let rel_path = format!(
            "figma/images/{}_{}.{ext}",
            if short_ref.is_empty() {
                "img"
            } else {
                &short_ref
            },
            idx
        );
        let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), rel_path, media_type, &final_bytes)
        else {
            ctx.report.warning(
                "figma-image-skipped",
                c.key,
                "Figma image exceeded single-resource limits; converted to placeholder shape.",
            );
            continue;
        };
        used_raw_bytes += unit.byte_len();
        if was_transcoded {
            downscaled_count += 1;
        }
        ctx.project.resources.push(unit);
        ctx.image_resources.insert(c.key, (res_id, dw, dh));
    }

    if downscaled_count > 0 {
        ctx.report.warning(
            "figma-image-downscaled",
            doc.name.clone(),
            format!(
                "Downscaled or re-encoded {downscaled_count} oversized Figma bitmap asset(s) to fit NeoFlash's 4096px / 64 MiB resource limits."
            ),
        );
    }
}

fn encode_fast_png(rgba: &image::RgbaImage) -> Option<Vec<u8>> {
    let (w, h) = rgba.dimensions();
    let mut out = Vec::new();
    let enc =
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilterType::Adaptive);
    enc.write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

fn decode_and_reencode_image(
    bytes: &[u8],
    fmt: ImageFormat,
    mut tw: u32,
    mut th: u32,
) -> Option<(Vec<u8>, &'static str, &'static str, u32, u32)> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), fmt);
    reader.limits(limits);
    let mut img: DynamicImage = reader.decode().ok()?;

    let (ow, oh) = img.dimensions();
    tw = tw.clamp(1, MAX_BITMAP_DIMENSION);
    th = th.clamp(1, MAX_BITMAP_DIMENSION);
    if (tw, th) != (ow, oh) {
        img = img.resize_exact(tw, th, image::imageops::FilterType::Triangle);
    }

    let rgba = img.to_rgba8();
    let (rw, rh) = rgba.dimensions();
    let has_alpha = rgba
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255);
    if !has_alpha {
        let rgb = DynamicImage::ImageRgba8(rgba).into_rgb8();
        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, 82);
        enc.encode_image(&rgb).ok()?;
        return Some((out, "image/jpeg", "jpg", rw, rh));
    }

    let mut cur_img = DynamicImage::ImageRgba8(rgba);
    for _ in 0..3 {
        let cur_rgba = cur_img.to_rgba8();
        let (cw, ch) = cur_rgba.dimensions();
        let out = encode_fast_png(&cur_rgba)?;
        if out.len() <= 2 * 1024 * 1024 || (cw <= 512 && ch <= 512) {
            return Some((out, "image/png", "png", cw, ch));
        }
        cur_img = DynamicImage::ImageRgba8(cur_rgba).resize_exact(
            (cw / 2).max(1),
            (ch / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let cur_rgba = cur_img.to_rgba8();
    let (cw, ch) = cur_rgba.dimensions();
    let out = encode_fast_png(&cur_rgba)?;
    Some((out, "image/png", "png", cw, ch))
}

fn compute_bitmap_fill_matrix(
    iw: u32,
    ih: u32,
    shape_w: f64,
    shape_h: f64,
    scale_mode: &str,
    image_transform: Option<[f64; 6]>,
    scaling_factor: Option<f64>,
) -> ([f64; 6], bool) {
    let bw = f64::from(iw.max(1));
    let bh = f64::from(ih.max(1));
    let sw = shape_w.max(1.0);
    let sh = shape_h.max(1.0);
    let repeat = scale_mode == "TILE";

    if matches!(scale_mode, "CROP" | "STRETCH")
        && let Some([m00, m01, m02, m10, m11, m12]) = image_transform
    {
        let det = m00 * m11 - m01 * m10;
        if det.is_finite() && det.abs() > 1e-9 {
            let inv00 = m11 / det;
            let inv01 = -m01 / det;
            let inv10 = -m10 / det;
            let inv11 = m00 / det;
            let tile_scale = if repeat {
                scaling_factor.unwrap_or(1.0).clamp(1e-4, 1e4)
            } else {
                1.0
            };
            let mat = [
                (sw * inv00 / bw) * tile_scale,
                (sh * inv10 / bw) * tile_scale,
                (sw * inv01 / bh) * tile_scale,
                (sh * inv11 / bh) * tile_scale,
                sw * (-inv00 * m02 - inv01 * m12),
                sh * (-inv10 * m02 - inv11 * m12),
            ];
            if mat.iter().all(|v| v.is_finite() && v.abs() <= 1e8) {
                return (mat, repeat);
            }
        }
    }

    match scale_mode {
        "TILE" => {
            let s = scaling_factor.unwrap_or(1.0).clamp(1e-4, 1e4);
            ([s, 0.0, 0.0, s, 0.0, 0.0], true)
        }
        "FIT" => {
            let s = (sw / bw).min(sh / bh).clamp(1e-6, 1e6);
            let tx = (sw - bw * s) * 0.5;
            let ty = (sh - bh * s) * 0.5;
            ([s, 0.0, 0.0, s, tx, ty], false)
        }
        _ => {
            let s = (sw / bw).max(sh / bh).clamp(1e-6, 1e6);
            let tx = (sw - bw * s) * 0.5;
            let ty = (sh - bh * s) * 0.5;
            ([s, 0.0, 0.0, s, tx, ty], false)
        }
    }
}

fn has_unequal_border_weights(node: &FigmaNode) -> bool {
    let Some([bt, br, bb, bl]) = node.border_weights else {
        return false;
    };
    if node.strokes.is_empty() || (bt <= 0.0 && br <= 0.0 && bb <= 0.0 && bl <= 0.0) {
        return false;
    }
    (br - bt).abs() > 1e-4 || (bb - bt).abs() > 1e-4 || (bl - bt).abs() > 1e-4
}

fn build_independent_border_elements(
    node: &FigmaNode,
    base_tf: Transform,
    stroke_solid: Color,
    stroke_grad: Option<GradientPaint>,
    blend_mode: BlendMode,
) -> Vec<Element> {
    let mut out = Vec::new();
    let Some([bt, br, bb, bl]) = node.border_weights else {
        return out;
    };
    let w = node.width.clamp(1.0, 32_768.0);
    let h = node.height.clamp(1.0, 32_768.0);
    let sides = [
        ("Top Border", bt, [0.0, 0.0], [w, 0.0]),
        ("Right Border", br, [w, 0.0], [w, h]),
        ("Bottom Border", bb, [0.0, h], [w, h]),
        ("Left Border", bl, [0.0, 0.0], [0.0, h]),
    ];
    let mut tf = base_tf;
    tf.primitive = None;
    for (label, sw, p0, p1) in sides {
        if !sw.is_finite() || sw <= 0.0 {
            continue;
        }
        let width = sw.clamp(0.1, 256.0);
        let cmds = vec![
            PathCommand::MoveTo { x: p0[0], y: p0[1] },
            PathCommand::LineTo { x: p1[0], y: p1[1] },
        ];
        let kind = if let Some(ref sg) = stroke_grad {
            ElementKind::PaintedPath {
                commands: cmds,
                fill_rule: FillRule::NonZero,
                fill: [0, 0, 0, 0],
                stroke: stroke_solid,
                fill_gradient: None,
                stroke_gradient: Some(sg.clone()),
                stroke_width: width,
            }
        } else {
            ElementKind::Path {
                commands: cmds,
                fill_rule: FillRule::NonZero,
                fill: [0, 0, 0, 0],
                stroke: stroke_solid,
                stroke_width: width,
            }
        };
        out.push(Element {
            id: new_id("el"),
            name: label.into(),
            transform: tf,
            kind,
            visible: node.visible,
            blend_mode,
            ..Element::default()
        });
    }
    out
}

fn hash_instance_overrides(
    comp_id: &str,
    instance: &FigmaNode,
    resizes_layout_master: bool,
) -> u64 {
    let overrides = &instance.symbol_overrides;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    comp_id.hash(&mut h);
    resizes_layout_master.hash(&mut h);
    if !instance.instance_text_layouts.is_empty() || resizes_layout_master {
        instance.width.to_bits().hash(&mut h);
        instance.height.to_bits().hash(&mut h);
        for layout in &instance.instance_text_layouts {
            layout.id.hash(&mut h);
            layout.size.map(|s| s.map(f64::to_bits)).hash(&mut h);
            layout.transform.map(|s| s.map(f64::to_bits)).hash(&mut h);
        }
    }
    overrides.len().hash(&mut h);
    for ov in overrides {
        ov.guid_path.hash(&mut h);
        ov.visible.hash(&mut h);
        ov.text_auto_resize.hash(&mut h);
        ov.font_size.map(f64::to_bits).hash(&mut h);
        if let Some(op) = ov.opacity {
            ((op * 1000.0).round() as i32).hash(&mut h);
        } else {
            (-1i32).hash(&mut h);
        }
        ov.characters.hash(&mut h);
        ov.text_style_ids.hash(&mut h);
        ov.text_style_overrides.len().hash(&mut h);
        for (sid, st) in &ov.text_style_overrides {
            sid.hash(&mut h);
            st.font_family.hash(&mut h);
            st.font_size.map(f64::to_bits).hash(&mut h);
            st.bold.hash(&mut h);
            st.italic.hash(&mut h);
            st.underline.hash(&mut h);
            st.strikethrough.hash(&mut h);
            st.fill_color.hash(&mut h);
            st.letter_spacing.map(f64::to_bits).hash(&mut h);
            st.line_height.map(f64::to_bits).hash(&mut h);
        }
        ov.overridden_symbol_id.hash(&mut h);
        for plist in [&ov.fills, &ov.strokes] {
            if let Some(list) = plist {
                list.len().hash(&mut h);
                for p in list {
                    match p {
                        FigmaPaint::Solid { color } => {
                            1u8.hash(&mut h);
                            color.hash(&mut h);
                        }
                        FigmaPaint::Gradient {
                            kind,
                            stops,
                            uv_transform,
                        } => {
                            2u8.hash(&mut h);
                            kind.hash(&mut h);
                            uv_transform.map(|m| m.map(f64::to_bits)).hash(&mut h);
                            for s in stops {
                                s.ratio.hash(&mut h);
                                s.color.hash(&mut h);
                            }
                        }
                        FigmaPaint::Image {
                            image_ref,
                            scale_mode,
                            image_transform,
                            scaling_factor,
                            ..
                        } => {
                            3u8.hash(&mut h);
                            image_ref.hash(&mut h);
                            scale_mode.hash(&mut h);
                            image_transform.map(|m| m.map(f64::to_bits)).hash(&mut h);
                            scaling_factor.map(f64::to_bits).hash(&mut h);
                        }
                    }
                }
            } else {
                usize::MAX.hash(&mut h);
            }
        }
    }
    h.finish()
}

fn guid_matches_target(node_id: &str, target_guid: &str) -> bool {
    if node_id == target_guid {
        return true;
    }
    if let (Some((_, nl)), Some((_, tl))) = (node_id.split_once(':'), target_guid.split_once(':'))
        && nl == tl
        && node_id.starts_with("0:") != target_guid.starts_with("0:")
    {
        return true;
    }
    false
}

/// Derived text geometry is in the resized instance's coordinate system. The instance
/// itself still scales its shared master, so undo that scale on each matched text child.
fn apply_instance_text_layouts(master: &mut FigmaNode, instance: &FigmaNode) -> bool {
    if instance.instance_text_layouts.is_empty() {
        return false;
    }
    let scale = if master.width > 1e-3
        && master.height > 1e-3
        && instance.width > 1e-3
        && instance.height > 1e-3
        && ((instance.width - master.width).abs() > 0.25
            || (instance.height - master.height).abs() > 0.25)
    {
        [
            master.width / instance.width,
            master.height / instance.height,
        ]
    } else {
        [1.0, 1.0]
    };
    let mut changed = false;
    for layout in &instance.instance_text_layouts {
        let Some(child) = master
            .children
            .iter_mut()
            .find(|child| child.text.is_some() && guid_matches_target(&child.id, &layout.id))
        else {
            continue;
        };
        let [w, h] = layout.size.unwrap_or([child.width, child.height]);
        let source = child
            .relative_transform
            .unwrap_or([1., 0., 0., 1., child.x, child.y]);
        let [a, b, c, d, x, y] = layout.transform.unwrap_or(source);
        let m = [
            a * scale[0],
            b * scale[1],
            c * scale[0],
            d * scale[1],
            x * scale[0],
            y * scale[1],
        ];
        if w != child.width || h != child.height || m != source {
            child.width = w;
            child.height = h;
            child.x = m[4];
            child.y = m[5];
            child.relative_transform = Some(m);
            changed = true;
        }
    }
    changed
}

fn apply_override_at_path(
    node: &mut FigmaNode,
    path: &[String],
    ov: &FigmaSymbolOverride,
    remeasure_overridden_text: bool,
) -> bool {
    if path.is_empty() {
        return false;
    }
    let mut applied = false;
    for child in &mut node.children {
        if guid_matches_target(&child.id, &path[0]) {
            if path.len() == 1 {
                let mut text_metrics_dirty = false;
                if let Some(txt) = &mut child.text {
                    if let Some(mode) = &ov.text_auto_resize {
                        if &txt.auto_resize != mode {
                            text_metrics_dirty = true;
                        }
                        txt.auto_resize.clone_from(mode);
                    }
                    if let Some(size) = ov.font_size {
                        if (txt.font_size - size).abs() > 1e-4 {
                            text_metrics_dirty = true;
                        }
                        txt.font_size = size;
                    }
                }
                if let Some(vis) = ov.visible {
                    child.visible = vis;
                }
                if let Some(op) = ov.opacity {
                    child.opacity = op;
                }
                if let Some(txt) = &mut child.text {
                    let chars_changed = if let Some(chars) = &ov.characters {
                        let changed = &txt.characters != chars;
                        txt.characters.clone_from(chars);
                        changed
                    } else {
                        false
                    };
                    text_metrics_dirty |= chars_changed;
                    if let Some(char_ids) = &ov.text_style_ids {
                        let override_map: HashMap<u64, FigmaTextStyleOverride> =
                            ov.text_style_overrides.iter().cloned().collect();
                        txt.style_runs = super::kiwi::build_text_style_runs(
                            &txt.characters,
                            char_ids,
                            &override_map,
                        );
                    } else if chars_changed {
                        txt.style_runs.clear();
                    }
                }
                if remeasure_overridden_text
                    && text_metrics_dirty
                    && let Some(td) = &child.text
                    && matches!(td.auto_resize.as_str(), "WIDTH_AND_HEIGHT" | "HEIGHT")
                {
                    child.auto_layout.has_explicit_size = false;
                }
                if let Some(fills) = &ov.fills {
                    child.fills.clone_from(fills);
                }
                if let Some(strokes) = &ov.strokes {
                    child.strokes.clone_from(strokes);
                }
                if let Some(sym_id) = &ov.overridden_symbol_id {
                    child.component_id = Some(sym_id.clone());
                }
                applied = true;
            } else if apply_override_at_path(child, &path[1..], ov, remeasure_overridden_text) {
                applied = true;
            }
        } else if path.len() == 1
            && apply_override_at_path(child, path, ov, remeasure_overridden_text)
        {
            applied = true;
        }
    }
    applied
}

fn sanitize_font_family(raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .trim()
        .to_owned();
    if s.is_empty() { "Inter".into() } else { s }
}

fn sanitize_symbol_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(180)
        .collect::<String>()
        .trim()
        .to_owned();
    if s.is_empty() {
        "Figma Symbol".into()
    } else {
        s
    }
}

fn sanitize_layer_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
        .trim()
        .to_owned();
    if s.is_empty() { "Layer".into() } else { s }
}

fn sanitize_element_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn sanitize_instance_name(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            s.push(ch);
        } else if (ch == ' ' || ch == '-') && !s.ends_with('_') && !s.is_empty() {
            s.push('_');
        }
    }
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.as_bytes()[0].is_ascii_digit() {
        format!("_{trimmed}")
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn build_figma_gradient_paint(
    kind: GradientKind,
    stops: &[GradientStop],
    uv_transform: Option<[f64; 6]>,
    width: f64,
    height: f64,
) -> Option<GradientPaint> {
    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|st| st.ratio);
    sorted.truncate(15);
    if sorted.len() < 2 {
        return None;
    }
    let matrix = figma_uv_transform_to_flash_gradient_matrix(uv_transform, width, height);
    let paint = GradientPaint {
        kind,
        matrix,
        stops: sorted,
        focal_point_ratio: 0.0,
        ..Default::default()
    };
    if paint.validate().is_ok() {
        return Some(paint);
    }
    let fallback = GradientPaint {
        kind,
        matrix: figma_uv_transform_to_flash_gradient_matrix(None, width, height),
        ..paint
    };
    fallback.validate().ok().map(|()| fallback)
}

fn lerp_rgba_stop(c0: Color, c1: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| {
        (f64::from(c0[i]) + (f64::from(c1[i]) - f64::from(c0[i])) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

fn sample_sorted_stops(sorted: &[(f64, Color)], t: f64) -> Color {
    if sorted.is_empty() {
        return [0, 0, 0, 255];
    }
    let t = t.clamp(0.0, 1.0);
    if t <= sorted[0].0 {
        return sorted[0].1;
    }
    if t >= sorted[sorted.len() - 1].0 {
        return sorted[sorted.len() - 1].1;
    }
    for pair in sorted.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let span = t1 - t0;
            if span <= 1e-9 {
                return c1;
            }
            return lerp_rgba_stop(c0, c1, (t - t0) / span);
        }
    }
    sorted[sorted.len() - 1].1
}

/// Convert a Figma `GRADIENT_DIAMOND` or `GRADIENT_ANGULAR` paint into a true scalable
/// [`VectorMeshPaint`] in local element coordinates without generating any bitmap resource.
fn build_figma_mesh_paint(
    kind: FigmaGradientKind,
    stops: &[GradientStop],
    uv_transform: Option<[f64; 6]>,
    width: f64,
    height: f64,
    commands: &[PathCommand],
) -> Option<VectorMeshPaint> {
    if stops.len() < 2 {
        return None;
    }
    let mut norm_stops: Vec<(f64, Color)> = stops
        .iter()
        .map(|s| ((f64::from(s.ratio) / 255.0).clamp(0.0, 1.0), s.color))
        .collect();
    norm_stops.sort_by(|a, b| a.0.total_cmp(&b.0));

    let sw = if width.is_finite() {
        width.clamp(1.0, 1e6)
    } else {
        100.0
    };
    let sh = if height.is_finite() {
        height.clamp(1.0, 1e6)
    } else {
        100.0
    };
    let raw_m = uv_transform.unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let det = raw_m[0] * raw_m[4] - raw_m[1] * raw_m[3];
    let [
        m00,
        m01,
        m02,
        m10,
        m11,
        m12,
        inv00,
        inv01,
        inv02,
        inv10,
        inv11,
        inv12,
    ] = if det.is_finite() && det.abs() > 1e-9 {
        let i00 = raw_m[4] / det;
        let i01 = -raw_m[1] / det;
        let i02 = -(i00 * raw_m[2] + i01 * raw_m[5]);
        let i10 = -raw_m[3] / det;
        let i11 = raw_m[0] / det;
        let i12 = -(i10 * raw_m[2] + i11 * raw_m[5]);
        [
            raw_m[0], raw_m[1], raw_m[2], raw_m[3], raw_m[4], raw_m[5], i00, i01, i02, i10, i11,
            i12,
        ]
    } else {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    };

    // Map canonical gradient space `(u, v)` (`[-1, 1]` around center `(gx, gy) = (0.5, 0.5)`)
    // into element-local coordinates `[lx, ly]`.
    let uv_to_local = |u: f64, v: f64| -> [f64; 2] {
        let gx = 0.5 + 0.5 * u;
        let gy = 0.5 + 0.5 * v;
        let sx = inv00 * gx + inv01 * gy + inv02;
        let sy = inv10 * gx + inv11 * gy + inv12;
        [(sw * sx).clamp(-1e6, 1e6), (sh * sy).clamp(-1e6, 1e6)]
    };

    // Measure local contour bounds so the outer pad ring covers 100% of the vector shape.
    let (mut bx0, mut by0, mut bx1, mut by1) = (0.0_f64, 0.0_f64, sw, sh);
    if let Ok(Some(pb)) = path_bounds(commands) {
        bx0 = bx0.min(pb.x_min);
        by0 = by0.min(pb.y_min);
        bx1 = bx1.max(pb.x_max);
        by1 = by1.max(pb.y_max);
    }
    let local_to_uv = |lx: f64, ly: f64| -> [f64; 2] {
        let sx = lx / sw;
        let sy = ly / sh;
        let gx = m00 * sx + m01 * sy + m02;
        let gy = m10 * sx + m11 * sy + m12;
        [2.0 * (gx - 0.5), 2.0 * (gy - 0.5)]
    };

    let mut max_manhattan = 1.0_f64;
    let mut max_hypot = 1.0_f64;
    for (cx, cy) in [(bx0, by0), (bx1, by0), (bx1, by1), (bx0, by1)] {
        let [cu, cv] = local_to_uv(cx, cy);
        if cu.is_finite() && cv.is_finite() {
            max_manhattan = max_manhattan.max(cu.abs() + cv.abs());
            max_hypot = max_hypot.max(cu.hypot(cv));
        }
    }

    let mut triangles = Vec::new();
    match kind {
        FigmaGradientKind::Diamond => {
            // Build ordered `(d_inner, c_inner, d_outer, c_outer)` Manhattan-distance bands.
            let mut ring_knots: Vec<(f64, Color)> = Vec::with_capacity(norm_stops.len() + 3);
            ring_knots.push((0.0, norm_stops[0].1));
            for &(pos, col) in &norm_stops {
                if let Some(last) = ring_knots.last()
                    && (pos - last.0).abs() <= 1e-6
                {
                    if ring_knots.len() == 1 {
                        ring_knots[0].1 = col;
                    } else {
                        ring_knots.push((pos, col));
                    }
                } else {
                    ring_knots.push((pos, col));
                }
            }
            let last_col = norm_stops[norm_stops.len() - 1].1;
            if ring_knots.last().is_some_and(|&(d, _)| d < 1.0 - 1e-6) {
                ring_knots.push((1.0, last_col));
            }
            let outer_d = (max_manhattan * 1.08).clamp(1.25, 1e4);
            ring_knots.push((outer_d, last_col));

            let dirs: [[f64; 2]; 4] = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
            let center_pt = uv_to_local(0.0, 0.0);
            for q in 0..4 {
                let d_a = dirs[q];
                let d_b = dirs[(q + 1) % 4];
                for band in ring_knots.windows(2) {
                    let (r0, c0) = band[0];
                    let (r1, c1) = band[1];
                    if r1 - r0 <= 1e-6 {
                        continue;
                    }
                    let a1 = uv_to_local(d_a[0] * r1, d_a[1] * r1);
                    let b1 = uv_to_local(d_b[0] * r1, d_b[1] * r1);
                    if r0 <= 1e-6 {
                        triangles.push(VectorMeshTriangle {
                            points: [center_pt, a1, b1],
                            colors: [c0, c1, c1],
                        });
                    } else {
                        let a0 = uv_to_local(d_a[0] * r0, d_a[1] * r0);
                        let b0 = uv_to_local(d_b[0] * r0, d_b[1] * r0);
                        triangles.push(VectorMeshTriangle {
                            points: [a0, a1, b1],
                            colors: [c0, c1, c1],
                        });
                        triangles.push(VectorMeshTriangle {
                            points: [a0, b1, b0],
                            colors: [c0, c1, c0],
                        });
                    }
                }
            }
        }
        FigmaGradientKind::Angular => {
            const BASE_WEDGES: usize = 24;
            let mut angles_t: Vec<f64> = (0..=BASE_WEDGES)
                .map(|i| (i as f64) / (BASE_WEDGES as f64))
                .collect();
            for &(pos, _) in &norm_stops {
                angles_t.push(pos.clamp(0.0, 1.0));
            }
            angles_t.sort_by(|a, b| a.total_cmp(b));
            let mut unique_t: Vec<f64> = Vec::with_capacity(angles_t.len());
            for t in angles_t {
                if unique_t.last().is_none_or(|&prev| (t - prev).abs() > 1e-5) {
                    unique_t.push(t);
                }
            }
            if unique_t.len() < 2 {
                unique_t = vec![0.0, 1.0];
            }

            // Average center colour for the tiny inner origin cap (`r in 0..0.02`).
            let mut sum = [0u64; 4];
            for k in 0..BASE_WEDGES {
                let c = sample_sorted_stops(&norm_stops, ((k as f64) + 0.5) / (BASE_WEDGES as f64));
                for ch in 0..4 {
                    sum[ch] += u64::from(c[ch]);
                }
            }
            let center_col: Color =
                std::array::from_fn(|ch| (sum[ch] / (BASE_WEDGES as u64)) as u8);
            let center_pt = uv_to_local(0.0, 0.0);

            let outer_r = (max_hypot * 1.15).clamp(1.5, 1e4);
            let radii = [0.02_f64, 0.35, 1.0, outer_r];

            for pair in unique_t.windows(2) {
                let t0 = pair[0];
                let t1 = pair[1];
                if t1 - t0 <= 1e-6 {
                    continue;
                }
                let c0 = sample_sorted_stops(&norm_stops, (t0 + 1e-7).min(t1));
                let c1 = sample_sorted_stops(&norm_stops, (t1 - 1e-7).max(t0));
                let ang0 = t0 * std::f64::consts::TAU;
                let ang1 = t1 * std::f64::consts::TAU;
                let (s0, cos0) = ang0.sin_cos();
                let (s1, cos1) = ang1.sin_cos();

                let p_cap0 = uv_to_local(radii[0] * cos0, radii[0] * s0);
                let p_cap1 = uv_to_local(radii[0] * cos1, radii[0] * s1);
                triangles.push(VectorMeshTriangle {
                    points: [center_pt, p_cap0, p_cap1],
                    colors: [center_col, c0, c1],
                });

                for rw in radii.windows(2) {
                    let r_in = rw[0];
                    let r_out = rw[1];
                    if r_out <= r_in {
                        continue;
                    }
                    let q00 = uv_to_local(r_in * cos0, r_in * s0);
                    let q10 = uv_to_local(r_out * cos0, r_out * s0);
                    let q11 = uv_to_local(r_out * cos1, r_out * s1);
                    let q01 = uv_to_local(r_in * cos1, r_in * s1);
                    triangles.push(VectorMeshTriangle {
                        points: [q00, q10, q11],
                        colors: [c0, c0, c1],
                    });
                    triangles.push(VectorMeshTriangle {
                        points: [q00, q11, q01],
                        colors: [c0, c1, c1],
                    });
                }
            }
        }
        FigmaGradientKind::Linear | FigmaGradientKind::Radial => return None,
    }

    let mesh = VectorMeshPaint {
        patches: Vec::new(),
        triangles,
        linear_rgb: false,
    };
    mesh.validate().ok().map(|()| mesh)
}

// ============================================================================
// Recursive Figma Auto Layout & Constraint Reflow Engine (`T-501`)
// ============================================================================

fn clamp_opt_dim(val: f64, min_v: Option<f64>, max_v: Option<f64>) -> f64 {
    let mut out = if val.is_finite() {
        val.clamp(0.0, 1e7)
    } else {
        0.0
    };
    if let Some(lo) = min_v.filter(|v| v.is_finite() && *v >= 0.0) {
        out = out.max(lo.min(1e7));
    }
    if let Some(hi) = max_v.filter(|v| v.is_finite() && *v >= 0.0) {
        let eff_hi = match min_v.filter(|v| v.is_finite() && *v >= 0.0) {
            Some(lo) => hi.max(lo).min(1e7),
            None => hi.min(1e7),
        };
        out = out.min(eff_hi);
    }
    out
}

fn estimate_char_advance_em(ch: char) -> f64 {
    match ch {
        ' ' => 0.28,
        'i' | 'l' | 'I' | 'j' | '!' | '.' | ',' | ':' | ';' | '\'' | '|' => 0.28,
        't' | 'f' | 'r' | '1' | '(' | ')' | '[' | ']' | '-' => 0.36,
        'm' | 'w' => 0.80,
        'M' | 'W' | '@' | '%' => 0.86,
        'A'..='Z' => 0.64,
        '0'..='9' => 0.56,
        c if (c as u32) >= 0x2E80 => 1.0,
        _ => 0.53,
    }
}

fn measure_figma_text_line(line: &str, td: &FigmaTextData) -> f64 {
    let fs = td.font_size.clamp(1.0, 1024.0);
    let weight_scale = if td.bold { 1.06 } else { 1.0 };
    let ls = td.letter_spacing.clamp(-60.0, 60.0);
    let mut width = 0.0;
    let mut count = 0usize;
    for ch in line.chars() {
        width += estimate_char_advance_em(ch) * fs * weight_scale;
        count += 1;
    }
    if count > 1 {
        width += (count - 1) as f64 * ls;
    }
    width.max(0.0)
}

fn measure_figma_text_intrinsic(td: &FigmaTextData, constrained_width: Option<f64>) -> (f64, f64) {
    let fs = td.font_size.clamp(1.0, 1024.0);
    let line_adv = td
        .line_height
        .filter(|lh| lh.is_finite() && *lh > 0.0)
        .unwrap_or_else(|| (fs * 1.2).ceil())
        .max(1.0);
    let norm = neoflash_core::normalize_text(&td.characters);
    if norm.is_empty() {
        return (constrained_width.unwrap_or(1.0).max(1.0), line_adv);
    }

    if let Some(max_w) = constrained_width {
        let cap_w = max_w.max(fs * 0.6).max(1.0);
        let space_w = (0.28 * fs * if td.bold { 1.06 } else { 1.0 } + td.letter_spacing).max(1.0);
        let mut total_lines = 0usize;
        for para in norm.split('\n') {
            if para.is_empty() {
                total_lines += 1;
                continue;
            }
            let mut cur_w = 0.0;
            let mut any_word = false;
            for word in para.split(' ') {
                let ww = measure_figma_text_line(word, td);
                if !any_word {
                    cur_w = ww;
                    any_word = true;
                    if ww > cap_w && word.chars().count() > 1 {
                        let extra = ((ww / cap_w).ceil() as usize).saturating_sub(1);
                        total_lines += extra;
                        cur_w = ww % cap_w;
                    }
                } else if cur_w + space_w + ww <= cap_w + 1e-3 {
                    cur_w += space_w + ww;
                } else {
                    total_lines += 1;
                    cur_w = ww;
                    if ww > cap_w && word.chars().count() > 1 {
                        let extra = ((ww / cap_w).ceil() as usize).saturating_sub(1);
                        total_lines += extra;
                        cur_w = ww % cap_w;
                    }
                }
            }
            total_lines += 1;
        }
        let h = (total_lines.max(1) as f64 * line_adv).ceil().max(1.0);
        return (max_w.max(1.0), h);
    }

    let mut max_line_w = 1.0_f64;
    let mut lines = 0usize;
    for para in norm.split('\n') {
        max_line_w = max_line_w.max(measure_figma_text_line(para, td).ceil());
        lines += 1;
    }
    let h = (lines.max(1) as f64 * line_adv).ceil().max(1.0);
    (max_line_w.max(1.0), h)
}

fn master_uses_layout_or_constraints(node: &FigmaNode) -> bool {
    if node.auto_layout.layout_mode != FigmaLayoutMode::None {
        return true;
    }
    node.children.iter().any(|c| {
        c.auto_layout.constraint_h != FigmaConstraintH::Left
            || c.auto_layout.constraint_v != FigmaConstraintV::Top
            || c.auto_layout.layout_mode != FigmaLayoutMode::None
            || master_uses_layout_or_constraints(c)
    })
}

fn set_node_rel_xy(node: &mut FigmaNode, nx: f64, ny: f64) {
    let sx = if nx.is_finite() {
        nx.clamp(-1e7, 1e7)
    } else {
        0.0
    };
    let sy = if ny.is_finite() {
        ny.clamp(-1e7, 1e7)
    } else {
        0.0
    };
    node.x = sx;
    node.y = sy;
    if let Some([a, b, c, d, _, _]) = node.relative_transform {
        node.relative_transform = Some([a, b, c, d, sx, sy]);
    } else if node.rotation_deg == 0.0 {
        node.relative_transform = Some([1.0, 0.0, 0.0, 1.0, sx, sy]);
    }
    node.auto_layout.has_explicit_position = true;
}

#[allow(clippy::too_many_arguments)]
fn apply_child_constraints_for_parent_resize<M: MasterNodeRef>(
    child: &mut FigmaNode,
    old_pw: f64,
    old_ph: f64,
    new_pw: f64,
    new_ph: f64,
    masters: &HashMap<String, M>,
    ctx: &mut ReflowCtx,
    depth: usize,
) {
    if depth > 64 || ctx.remaining_steps == 0 {
        return;
    }
    ctx.remaining_steps -= 1;

    let dw = new_pw - old_pw;
    let dh = new_ph - old_ph;
    let mut nx = if child.x.is_finite() {
        child.x.clamp(-1e7, 1e7)
    } else {
        0.0
    };
    let mut ny = if child.y.is_finite() {
        child.y.clamp(-1e7, 1e7)
    } else {
        0.0
    };
    let mut nw = clamp_opt_dim(child.width, None, None);
    let mut nh = clamp_opt_dim(child.height, None, None);

    if dw.is_finite() && dw.abs() > 1e-4 {
        match child.auto_layout.constraint_h {
            FigmaConstraintH::Left => {}
            FigmaConstraintH::Right => nx += dw,
            FigmaConstraintH::LeftAndRight => {
                nw = clamp_opt_dim(
                    nw + dw,
                    child.auto_layout.min_width,
                    child.auto_layout.max_width,
                );
            }
            FigmaConstraintH::Center => nx += dw * 0.5,
            FigmaConstraintH::Scale => {
                if old_pw > 1e-6 && new_pw.is_finite() {
                    let sx = (new_pw / old_pw).clamp(0.0, 1e4);
                    nx *= sx;
                    nw = clamp_opt_dim(
                        nw * sx,
                        child.auto_layout.min_width,
                        child.auto_layout.max_width,
                    );
                }
            }
        }
    }
    if dh.is_finite() && dh.abs() > 1e-4 {
        match child.auto_layout.constraint_v {
            FigmaConstraintV::Top => {}
            FigmaConstraintV::Bottom => ny += dh,
            FigmaConstraintV::TopAndBottom => {
                nh = clamp_opt_dim(
                    nh + dh,
                    child.auto_layout.min_height,
                    child.auto_layout.max_height,
                );
            }
            FigmaConstraintV::Center => ny += dh * 0.5,
            FigmaConstraintV::Scale => {
                if old_ph > 1e-6 && new_ph.is_finite() {
                    let sy = (new_ph / old_ph).clamp(0.0, 1e4);
                    ny *= sy;
                    nh = clamp_opt_dim(
                        nh * sy,
                        child.auto_layout.min_height,
                        child.auto_layout.max_height,
                    );
                }
            }
        }
    }

    let size_changed = (nw - child.width).abs() > 1e-4 || (nh - child.height).abs() > 1e-4;
    let old_cw = child.width;
    let old_ch = child.height;
    set_node_rel_xy(child, nx, ny);
    child.width = nw;
    child.height = nh;

    if size_changed && depth < 64 && ctx.remaining_steps > 0 {
        child.auto_layout.has_explicit_size = true;
        if let Some(td) = &child.text
            && td.auto_resize == "HEIGHT"
            && (nw - old_cw).abs() > 1e-4
        {
            let (_, th) = measure_figma_text_intrinsic(td, Some(nw));
            child.height = clamp_opt_dim(
                th,
                child.auto_layout.min_height,
                child.auto_layout.max_height,
            );
        } else if child.auto_layout.layout_mode != FigmaLayoutMode::None {
            if (nw - old_cw).abs() > 1e-4
                && child.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug
            {
                child.auto_layout.sizing_horizontal = FigmaLayoutSizing::Fixed;
            }
            if (nh - old_ch).abs() > 1e-4
                && child.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug
            {
                child.auto_layout.sizing_vertical = FigmaLayoutSizing::Fixed;
            }
            let _ = reflow_figma_node_inner(child, masters, ctx, true, depth + 1);
        } else if !child.children.is_empty() {
            for grand in &mut child.children {
                if ctx.remaining_steps == 0 {
                    break;
                }
                apply_child_constraints_for_parent_resize(
                    grand,
                    old_cw,
                    old_ch,
                    nw,
                    nh,
                    masters,
                    ctx,
                    depth + 1,
                );
            }
        }
    }
}

fn resize_container_to_dimensions<M: MasterNodeRef>(
    node: &mut FigmaNode,
    target_w: f64,
    target_h: f64,
    masters: &HashMap<String, M>,
    ctx: &mut ReflowCtx,
    depth: usize,
) {
    if depth > 64 || ctx.remaining_steps == 0 {
        return;
    }
    ctx.remaining_steps -= 1;

    let nw = clamp_opt_dim(
        target_w,
        node.auto_layout.min_width,
        node.auto_layout.max_width,
    );
    let nh = clamp_opt_dim(
        target_h,
        node.auto_layout.min_height,
        node.auto_layout.max_height,
    );
    let old_w = node.width;
    let old_h = node.height;
    if (nw - old_w).abs() <= 1e-4 && (nh - old_h).abs() <= 1e-4 {
        return;
    }
    node.width = nw;
    node.height = nh;
    node.auto_layout.has_explicit_size = true;

    if node.auto_layout.layout_mode != FigmaLayoutMode::None {
        if (nw - old_w).abs() > 1e-4 {
            node.auto_layout.sizing_horizontal = FigmaLayoutSizing::Fixed;
        }
        if (nh - old_h).abs() > 1e-4 {
            node.auto_layout.sizing_vertical = FigmaLayoutSizing::Fixed;
        }
        for child in &mut node.children {
            if ctx.remaining_steps == 0 {
                break;
            }
            if child.auto_layout.layout_positioning == FigmaLayoutPositioning::Absolute {
                apply_child_constraints_for_parent_resize(
                    child,
                    old_w,
                    old_h,
                    nw,
                    nh,
                    masters,
                    ctx,
                    depth + 1,
                );
            }
        }
        let _ = reflow_figma_node_inner(node, masters, ctx, true, depth + 1);
    } else {
        for child in &mut node.children {
            if ctx.remaining_steps == 0 {
                break;
            }
            apply_child_constraints_for_parent_resize(
                child,
                old_w,
                old_h,
                nw,
                nh,
                masters,
                ctx,
                depth + 1,
            );
        }
    }
}

fn child_stroke_outset(child: &FigmaNode, include_strokes: bool) -> f64 {
    if !include_strokes || child.strokes.is_empty() {
        return 0.0;
    }
    let w = if let Some([bt, br, bb, bl]) = child.border_weights {
        bt.max(br).max(bb).max(bl)
    } else {
        child.stroke_weight
    };
    if !w.is_finite() || w <= 0.0 {
        return 0.0;
    }
    match child.auto_layout.stroke_align.as_str() {
        "OUTSIDE" => w,
        "CENTER" => w * 0.5,
        _ => 0.0,
    }
}

fn parent_inside_border_pads(node: &FigmaNode) -> [f64; 4] {
    if !node.auto_layout.strokes_included_in_layout || node.strokes.is_empty() {
        return [0.0; 4];
    }
    let factor = match node.auto_layout.stroke_align.as_str() {
        "INSIDE" => 1.0,
        "CENTER" => 0.5,
        _ => 0.0,
    };
    if factor <= 0.0 {
        return [0.0; 4];
    }
    if let Some([bt, br, bb, bl]) = node.border_weights {
        [
            bt.max(0.0) * factor,
            br.max(0.0) * factor,
            bb.max(0.0) * factor,
            bl.max(0.0) * factor,
        ]
    } else {
        let w = node.stroke_weight.max(0.0) * factor;
        [w; 4]
    }
}

fn child_fills_primary(child: &FigmaNode, is_row: bool) -> bool {
    let axis_sz = if is_row {
        child.auto_layout.sizing_horizontal
    } else {
        child.auto_layout.sizing_vertical
    };
    axis_sz == FigmaLayoutSizing::Fill || child.auto_layout.layout_grow >= 1.0
}

fn child_fills_counter(child: &FigmaNode, is_row: bool) -> bool {
    let axis_sz = if is_row {
        child.auto_layout.sizing_vertical
    } else {
        child.auto_layout.sizing_horizontal
    };
    axis_sz == FigmaLayoutSizing::Fill || child.auto_layout.layout_align == "STRETCH"
}

fn auto_layout_should_reflow(node: &FigmaNode, any_child_changed: bool, force: bool) -> bool {
    if force || any_child_changed {
        return true;
    }
    let is_row = node.auto_layout.layout_mode == FigmaLayoutMode::Horizontal;
    let primary_hug = if is_row {
        node.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug
    } else {
        node.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug
    };
    let counter_hug = if is_row {
        node.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug
    } else {
        node.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug
    };
    if (primary_hug || counter_hug) && !node.auto_layout.has_explicit_size {
        return true;
    }

    let flow: Vec<&FigmaNode> = node
        .children
        .iter()
        .filter(|c| c.visible && c.auto_layout.layout_positioning == FigmaLayoutPositioning::Auto)
        .collect();
    if flow.is_empty() {
        return primary_hug || counter_hug;
    }
    if flow.iter().any(|c| !c.auto_layout.has_explicit_position) {
        return true;
    }

    // Detect unpositioned `[0, 0]` children when layout offsets should place at least one child away from `[0, 0]`.
    let all_at_zero = flow.iter().all(|c| c.x.abs() <= 1e-4 && c.y.abs() <= 1e-4);
    if all_at_zero
        && (flow.len() >= 2
            || node.auto_layout.padding_left > 0.5
            || node.auto_layout.padding_top > 0.5
            || node.auto_layout.primary_axis_align_items != FigmaPrimaryAlign::Min
            || node.auto_layout.counter_axis_align_items != FigmaCounterAlign::Min)
    {
        return true;
    }

    // Detect unapplied `FILL`, `HUG`, `WRAP`, or min/max constraints on authored JSON/Kiwi nodes.
    let [bt, br, bb, bl] = parent_inside_border_pads(node);
    let pad_p = if is_row {
        node.auto_layout.padding_left + node.auto_layout.padding_right + bl + br
    } else {
        node.auto_layout.padding_top + node.auto_layout.padding_bottom + bt + bb
    };
    let pad_c = if is_row {
        node.auto_layout.padding_top + node.auto_layout.padding_bottom + bt + bb
    } else {
        node.auto_layout.padding_left + node.auto_layout.padding_right + bl + br
    };
    let parent_p = if is_row { node.width } else { node.height };
    let parent_c = if is_row { node.height } else { node.width };
    let avail_p = (parent_p - pad_p).max(0.0);
    let avail_c = (parent_c - pad_c).max(0.0);

    let sum_p: f64 = flow
        .iter()
        .map(|c| if is_row { c.width } else { c.height })
        .sum::<f64>()
        + (flow.len().saturating_sub(1) as f64) * node.auto_layout.item_spacing;
    let max_c: f64 = flow
        .iter()
        .map(|c| if is_row { c.height } else { c.width })
        .fold(0.0, f64::max);

    if node.auto_layout.layout_wrap == FigmaLayoutWrap::Wrap
        && !primary_hug
        && sum_p > avail_p + 2.0
    {
        let same_cross = flow.windows(2).all(|w| {
            let c0 = if is_row { w[0].y } else { w[0].x };
            let c1 = if is_row { w[1].y } else { w[1].x };
            (c0 - c1).abs() <= 1.0
        });
        if same_cross {
            return true;
        }
    }
    if node.auto_layout.layout_wrap == FigmaLayoutWrap::NoWrap {
        if primary_hug && (parent_p - (sum_p + pad_p)).abs() > 2.0 {
            return true;
        }
        if !primary_hug
            && flow.iter().any(|c| child_fills_primary(c, is_row))
            && (avail_p - sum_p).abs() > 2.0
        {
            return true;
        }
        if counter_hug && (parent_c - (max_c + pad_c)).abs() > 2.0 {
            return true;
        }
        if !counter_hug {
            for c in &flow {
                if child_fills_counter(c, is_row) {
                    let cc = if is_row { c.height } else { c.width };
                    if (cc - avail_c).abs() > 2.0 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Reflow a [`FigmaNode`] hierarchy's nested Auto Layout (`HORIZONTAL` / `VERTICAL` / `WRAP` /
/// `HUG` / `FILL` / `SPACE_BETWEEN` / `BASELINE` / `ABSOLUTE` constraints / `min`/`max` bounds).
#[allow(dead_code)]
pub(super) fn reflow_figma_auto_layout(node: &mut FigmaNode) {
    let empty_masters: HashMap<String, FigmaNode> = HashMap::new();
    let mut ctx = ReflowCtx::new();
    let _ = reflow_figma_node_inner(node, &empty_masters, &mut ctx, false, 0);
    apply_reverse_z_order_recursive(node, 0);
}

fn collect_owned_masters(node: &FigmaNode, out: &mut HashMap<String, FigmaNode>, depth: usize) {
    if depth > 64 {
        return;
    }
    if matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL")
        && !node.is_state_group
        && !out.contains_key(&node.id)
    {
        out.insert(node.id.clone(), node.clone());
    }
    for child in &node.children {
        collect_owned_masters(child, out, depth + 1);
    }
}

fn reflow_figma_doc(doc: &mut FigmaDoc) {
    let mut ctx = ReflowCtx::new();
    // Pass 1: Reflow any Auto Layout inside master `COMPONENT` / `SYMBOL` definitions first.
    let mut seed_masters = HashMap::new();
    for page in &doc.pages {
        collect_owned_masters(page, &mut seed_masters, 0);
    }
    for page in &mut doc.pages {
        if ctx.remaining_steps == 0 {
            break;
        }
        reflow_masters_in_subtree(page, &seed_masters, &mut ctx, 0);
    }
    let mut masters = HashMap::new();
    for page in &doc.pages {
        collect_owned_masters(page, &mut masters, 0);
    }
    // Pass 2: Reflow all pages (sharing cycle guard + memo + step budget).
    ctx.remaining_steps = MAX_REFLOW_STEPS;
    ctx.visiting_masters.clear();
    for page in &mut doc.pages {
        let _ = reflow_figma_node_inner(page, &masters, &mut ctx, false, 0);
        apply_reverse_z_order_recursive(page, 0);
    }
}

fn reflow_masters_in_subtree<M: MasterNodeRef>(
    node: &mut FigmaNode,
    masters: &HashMap<String, M>,
    ctx: &mut ReflowCtx,
    depth: usize,
) {
    if depth > 64 || ctx.remaining_steps == 0 {
        return;
    }
    if matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL") && !node.is_state_group {
        let mid = node.id.clone();
        if !ctx.visiting_masters.insert(mid.clone()) {
            return;
        }
        let _ = reflow_figma_node_inner(node, masters, ctx, false, depth);
        ctx.visiting_masters.remove(&mid);
        return;
    }
    for child in &mut node.children {
        if ctx.remaining_steps == 0 {
            break;
        }
        reflow_masters_in_subtree(child, masters, ctx, depth + 1);
    }
}

fn apply_reverse_z_order_recursive(node: &mut FigmaNode, depth: usize) {
    if depth > 64 {
        return;
    }
    for child in &mut node.children {
        apply_reverse_z_order_recursive(child, depth + 1);
    }
    if node.auto_layout.layout_mode != FigmaLayoutMode::None
        && node.auto_layout.item_reverse_z_index
    {
        let flow_indices: Vec<usize> = node
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.auto_layout.layout_positioning == FigmaLayoutPositioning::Auto)
            .map(|(i, _)| i)
            .collect();
        let n = flow_indices.len();
        for k in 0..(n / 2) {
            node.children.swap(flow_indices[k], flow_indices[n - 1 - k]);
        }
        node.auto_layout.item_reverse_z_index = false;
    }
}

fn reflow_figma_node_inner<M: MasterNodeRef>(
    node: &mut FigmaNode,
    masters: &HashMap<String, M>,
    ctx: &mut ReflowCtx,
    force_self: bool,
    depth: usize,
) -> bool {
    if depth > 64 || ctx.remaining_steps == 0 {
        return false;
    }
    ctx.remaining_steps -= 1;

    node.width = clamp_opt_dim(node.width, None, None);
    node.height = clamp_opt_dim(node.height, None, None);
    let old_self_w = node.width;
    let old_self_h = node.height;

    // 1. Intrinsic text measurement when a `TEXT` leaf has no explicit size (or was marked dirty by override).
    if let Some(td) = &node.text
        && !node.auto_layout.has_explicit_size
    {
        let (tw, th) = match td.auto_resize.as_str() {
            "HEIGHT" => measure_figma_text_intrinsic(td, Some(node.width.max(1.0))),
            "WIDTH_AND_HEIGHT" => measure_figma_text_intrinsic(td, None),
            _ => (node.width, node.height),
        };
        node.width = clamp_opt_dim(tw, node.auto_layout.min_width, node.auto_layout.max_width);
        node.height = clamp_opt_dim(th, node.auto_layout.min_height, node.auto_layout.max_height);
        node.auto_layout.has_explicit_size = true;
    }

    // 2. If `INSTANCE` has overrides without pre-baked `instance_text_layouts` and references a `HUG`
    //    Auto Layout master, update its intrinsic `HUG` dimensions (breaking cycles via `visiting_masters`,
    //    memoizing DAG nodes in `hug_memo`, and enforcing `remaining_steps`).
    if node.node_type == "INSTANCE"
        && !node.symbol_overrides.is_empty()
        && node.instance_text_layouts.is_empty()
        && let Some(cid) = node.component_id.clone()
        && !ctx.visiting_masters.contains(&cid)
        && let Some(master_entry) = masters.get(&cid)
    {
        let master = master_entry.as_figma_node();
        if master.auto_layout.layout_mode != FigmaLayoutMode::None {
            let hug_h = node.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug
                || (!node.auto_layout.has_explicit_size
                    && master.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug)
                || ((node.width - master.width).abs() < 0.5
                    && master.auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug);
            let hug_v = node.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug
                || (!node.auto_layout.has_explicit_size
                    && master.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug)
                || ((node.height - master.height).abs() < 0.5
                    && master.auto_layout.sizing_vertical == FigmaLayoutSizing::Hug);
            if hug_h || hug_v {
                let memo_key = hash_instance_overrides(&cid, node, false);
                let resolved_dims = if let Some(&cached) = ctx.hug_memo.get(&memo_key) {
                    Some(cached)
                } else if ctx.remaining_steps > 0 {
                    let mut spec = master.clone();
                    let mut any_ov = false;
                    for ov in &node.symbol_overrides {
                        if apply_override_at_path(&mut spec, &ov.guid_path, ov, true) {
                            any_ov = true;
                        }
                    }
                    if any_ov {
                        ctx.visiting_masters.insert(cid.clone());
                        let _ = reflow_figma_node_inner(&mut spec, masters, ctx, true, depth + 1);
                        ctx.visiting_masters.remove(&cid);
                        let dims = (spec.width, spec.height);
                        if ctx.remaining_steps > 0 && ctx.hug_memo.len() < 8_192 {
                            ctx.hug_memo.insert(memo_key, dims);
                        }
                        Some(dims)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((sw, sh)) = resolved_dims {
                    if hug_h {
                        node.width = clamp_opt_dim(
                            sw,
                            node.auto_layout.min_width,
                            node.auto_layout.max_width,
                        );
                    }
                    if hug_v {
                        node.height = clamp_opt_dim(
                            sh,
                            node.auto_layout.min_height,
                            node.auto_layout.max_height,
                        );
                    }
                }
            }
        }
    }

    // 3. Bottom-up recursive reflow of children first.
    let mut any_child_changed = false;
    for child in &mut node.children {
        if ctx.remaining_steps == 0 {
            break;
        }
        if reflow_figma_node_inner(child, masters, ctx, false, depth + 1) {
            any_child_changed = true;
        }
    }

    if node.auto_layout.layout_mode == FigmaLayoutMode::None {
        return (node.width - old_self_w).abs() > 1e-3 || (node.height - old_self_h).abs() > 1e-3;
    }

    if !auto_layout_should_reflow(node, any_child_changed, force_self) {
        return (node.width - old_self_w).abs() > 1e-3 || (node.height - old_self_h).abs() > 1e-3;
    }

    let is_row = node.auto_layout.layout_mode == FigmaLayoutMode::Horizontal;
    let include_strokes = node.auto_layout.strokes_included_in_layout;
    let [bt, br, bb, bl] = parent_inside_border_pads(node);
    let (pad_p_start, pad_p_end, pad_c_start, pad_c_end) = if is_row {
        (
            node.auto_layout.padding_left + bl,
            node.auto_layout.padding_right + br,
            node.auto_layout.padding_top + bt,
            node.auto_layout.padding_bottom + bb,
        )
    } else {
        (
            node.auto_layout.padding_top + bt,
            node.auto_layout.padding_bottom + bb,
            node.auto_layout.padding_left + bl,
            node.auto_layout.padding_right + br,
        )
    };
    let (primary_sizing, counter_sizing) = if is_row {
        (
            node.auto_layout.sizing_horizontal,
            node.auto_layout.sizing_vertical,
        )
    } else {
        (
            node.auto_layout.sizing_vertical,
            node.auto_layout.sizing_horizontal,
        )
    };
    let (min_p, max_p, min_c, max_c) = if is_row {
        (
            node.auto_layout.min_width,
            node.auto_layout.max_width,
            node.auto_layout.min_height,
            node.auto_layout.max_height,
        )
    } else {
        (
            node.auto_layout.min_height,
            node.auto_layout.max_height,
            node.auto_layout.min_width,
            node.auto_layout.max_width,
        )
    };

    let flow_indices: Vec<usize> = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.visible && c.auto_layout.layout_positioning == FigmaLayoutPositioning::Auto
        })
        .map(|(i, _)| i)
        .collect();

    if flow_indices.is_empty() {
        let mut new_p = if is_row { node.width } else { node.height };
        let mut new_c = if is_row { node.height } else { node.width };
        if primary_sizing == FigmaLayoutSizing::Hug {
            new_p = pad_p_start + pad_p_end;
        }
        if counter_sizing == FigmaLayoutSizing::Hug {
            new_c = pad_c_start + pad_c_end;
        }
        new_p = clamp_opt_dim(new_p, min_p, max_p);
        new_c = clamp_opt_dim(new_c, min_c, max_c);
        let (nw, nh) = if is_row {
            (new_p, new_c)
        } else {
            (new_c, new_p)
        };
        let prev_w = node.width;
        let prev_h = node.height;
        node.width = nw;
        node.height = nh;
        node.auto_layout.has_explicit_size = true;
        if (nw - prev_w).abs() > 1e-4 || (nh - prev_h).abs() > 1e-4 {
            for child in &mut node.children {
                if ctx.remaining_steps == 0 {
                    break;
                }
                if child.auto_layout.layout_positioning == FigmaLayoutPositioning::Absolute {
                    apply_child_constraints_for_parent_resize(
                        child,
                        prev_w,
                        prev_h,
                        nw,
                        nh,
                        masters,
                        ctx,
                        depth + 1,
                    );
                }
            }
        }
        return (node.width - old_self_w).abs() > 1e-3 || (node.height - old_self_h).abs() > 1e-3;
    }

    let gap_p = node.auto_layout.item_spacing;
    let gap_for_wrap =
        if node.auto_layout.primary_axis_align_items == FigmaPrimaryAlign::SpaceBetween {
            gap_p.max(0.0)
        } else {
            gap_p
        };

    // Base primary box size (including optional stroke outset) for line breaking.
    let child_p_bounds = |c: &FigmaNode| -> (Option<f64>, Option<f64>) {
        if is_row {
            (c.auto_layout.min_width, c.auto_layout.max_width)
        } else {
            (c.auto_layout.min_height, c.auto_layout.max_height)
        }
    };
    let child_c_bounds = |c: &FigmaNode| -> (Option<f64>, Option<f64>) {
        if is_row {
            (c.auto_layout.min_height, c.auto_layout.max_height)
        } else {
            (c.auto_layout.min_width, c.auto_layout.max_width)
        }
    };

    let mut base_p_box = vec![0.0_f64; node.children.len()];
    for &ci in &flow_indices {
        let c = &node.children[ci];
        let out = child_stroke_outset(c, include_strokes);
        let (lo, hi) = child_p_bounds(c);
        let raw = if is_row { c.width } else { c.height };
        let inner = if child_fills_primary(c, is_row)
            && node.auto_layout.layout_wrap == FigmaLayoutWrap::Wrap
        {
            clamp_opt_dim(lo.unwrap_or(raw), lo, hi)
        } else {
            clamp_opt_dim(raw, lo, hi)
        };
        base_p_box[ci] = (inner + 2.0 * out).max(0.0);
    }

    let authored_p = if is_row { node.width } else { node.height };
    let wrap_avail: Option<f64> = if node.auto_layout.layout_wrap == FigmaLayoutWrap::Wrap {
        if primary_sizing != FigmaLayoutSizing::Hug {
            let eff_p = clamp_opt_dim(authored_p, min_p, max_p);
            Some((eff_p - pad_p_start - pad_p_end).max(0.0))
        } else {
            max_p.map(|mp| (mp - pad_p_start - pad_p_end).max(0.0))
        }
    } else {
        None
    };

    let mut lines: Vec<Vec<usize>> = Vec::new();
    if let Some(limit_p) = wrap_avail {
        let mut cur_line: Vec<usize> = Vec::new();
        let mut cur_used = 0.0_f64;
        for &ci in &flow_indices {
            let b = base_p_box[ci];
            let next_used = if cur_line.is_empty() {
                b
            } else {
                cur_used + gap_for_wrap + b
            };
            if !cur_line.is_empty() && next_used > limit_p + 1e-4 {
                lines.push(cur_line);
                cur_line = vec![ci];
                cur_used = b;
            } else {
                cur_line.push(ci);
                cur_used = next_used;
            }
        }
        if !cur_line.is_empty() {
            lines.push(cur_line);
        }
    } else {
        lines.push(flow_indices.clone());
    }

    // Measure natural primary span across lines and resolve container primary dimension.
    let natural_max_line_p = lines
        .iter()
        .map(|line| {
            line.iter().map(|&ci| base_p_box[ci]).sum::<f64>()
                + (line.len().saturating_sub(1) as f64) * gap_p
        })
        .fold(0.0_f64, f64::max);

    let container_p = if primary_sizing == FigmaLayoutSizing::Hug {
        clamp_opt_dim(natural_max_line_p + pad_p_start + pad_p_end, min_p, max_p)
    } else {
        clamp_opt_dim(authored_p, min_p, max_p)
    };
    let avail_p = (container_p - pad_p_start - pad_p_end).max(0.0);

    // Resolve per-item primary sizes on each line (including `FILL` distribution with min/max clamping).
    let can_distribute_fill_p = primary_sizing != FigmaLayoutSizing::Hug
        || container_p > natural_max_line_p + pad_p_start + pad_p_end + 1e-4;
    let mut solved_p_box = base_p_box.clone();
    if can_distribute_fill_p {
        for line in &lines {
            let gaps_total = if node.auto_layout.primary_axis_align_items
                == FigmaPrimaryAlign::SpaceBetween
                && line.len() > 1
            {
                0.0
            } else {
                (line.len().saturating_sub(1) as f64) * gap_p
            };
            let mut frozen = vec![false; line.len()];
            let mut sizes = vec![0.0_f64; line.len()];
            for (k, &ci) in line.iter().enumerate() {
                if !child_fills_primary(&node.children[ci], is_row) {
                    frozen[k] = true;
                    sizes[k] = base_p_box[ci];
                }
            }
            for _ in 0..4 {
                let unfrozen_count = frozen.iter().filter(|&&f| !f).count();
                if unfrozen_count == 0 {
                    break;
                }
                let frozen_sum: f64 = sizes
                    .iter()
                    .zip(&frozen)
                    .filter(|(_, f)| **f)
                    .map(|(s, _)| *s)
                    .sum();
                let rem = (avail_p - gaps_total - frozen_sum).max(0.0);
                let share = rem / (unfrozen_count as f64);
                let mut any_clamped = false;
                for (k, &ci) in line.iter().enumerate() {
                    if frozen[k] {
                        continue;
                    }
                    let c = &node.children[ci];
                    let out = child_stroke_outset(c, include_strokes);
                    let (lo, hi) = child_p_bounds(c);
                    let target_inner = (share - 2.0 * out).max(0.0);
                    let clamped_inner = clamp_opt_dim(target_inner, lo, hi);
                    let clamped_box = clamped_inner + 2.0 * out;
                    sizes[k] = clamped_box;
                    if (clamped_inner - target_inner).abs() > 1e-4 {
                        frozen[k] = true;
                        any_clamped = true;
                    }
                }
                if !any_clamped {
                    break;
                }
            }
            for (k, &ci) in line.iter().enumerate() {
                solved_p_box[ci] = sizes[k];
            }
        }
    }

    // Apply primary widths first when `is_row`, so `HEIGHT`-wrapping text and nested `Wrap` frames
    // update their cross-axis height before we measure line heights!
    if is_row {
        for &ci in &flow_indices {
            if ctx.remaining_steps == 0 {
                break;
            }
            let out = child_stroke_outset(&node.children[ci], include_strokes);
            let new_w = (solved_p_box[ci] - 2.0 * out).max(0.0);
            if (new_w - node.children[ci].width).abs() > 1e-4 {
                let prev_cw = node.children[ci].width;
                let prev_ch = node.children[ci].height;
                node.children[ci].width = new_w;
                node.children[ci].auto_layout.has_explicit_size = true;
                if let Some(td) = &node.children[ci].text
                    && td.auto_resize == "HEIGHT"
                {
                    let (_, th) = measure_figma_text_intrinsic(td, Some(new_w));
                    node.children[ci].height = clamp_opt_dim(
                        th,
                        node.children[ci].auto_layout.min_height,
                        node.children[ci].auto_layout.max_height,
                    );
                } else if node.children[ci].auto_layout.layout_mode != FigmaLayoutMode::None {
                    if node.children[ci].auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug {
                        node.children[ci].auto_layout.sizing_horizontal = FigmaLayoutSizing::Fixed;
                    }
                    let _ = reflow_figma_node_inner(
                        &mut node.children[ci],
                        masters,
                        ctx,
                        true,
                        depth + 1,
                    );
                } else if !node.children[ci].children.is_empty() {
                    let c_children = &mut node.children[ci];
                    for grand in &mut c_children.children {
                        if ctx.remaining_steps == 0 {
                            break;
                        }
                        apply_child_constraints_for_parent_resize(
                            grand,
                            prev_cw,
                            prev_ch,
                            new_w,
                            prev_ch,
                            masters,
                            ctx,
                            depth + 1,
                        );
                    }
                }
            }
        }
    } else {
        for &ci in &flow_indices {
            if ctx.remaining_steps == 0 {
                break;
            }
            let out = child_stroke_outset(&node.children[ci], include_strokes);
            let new_h = (solved_p_box[ci] - 2.0 * out).max(0.0);
            if (new_h - node.children[ci].height).abs() > 1e-4 {
                let prev_cw = node.children[ci].width;
                let prev_ch = node.children[ci].height;
                node.children[ci].height = new_h;
                node.children[ci].auto_layout.has_explicit_size = true;
                if node.children[ci].auto_layout.layout_mode != FigmaLayoutMode::None {
                    if node.children[ci].auto_layout.sizing_vertical == FigmaLayoutSizing::Hug {
                        node.children[ci].auto_layout.sizing_vertical = FigmaLayoutSizing::Fixed;
                    }
                    let _ = reflow_figma_node_inner(
                        &mut node.children[ci],
                        masters,
                        ctx,
                        true,
                        depth + 1,
                    );
                } else if !node.children[ci].children.is_empty() {
                    let c_children = &mut node.children[ci];
                    for grand in &mut c_children.children {
                        if ctx.remaining_steps == 0 {
                            break;
                        }
                        apply_child_constraints_for_parent_resize(
                            grand,
                            prev_cw,
                            prev_ch,
                            prev_cw,
                            new_h,
                            masters,
                            ctx,
                            depth + 1,
                        );
                    }
                }
            }
        }
    }

    // Measure counter-axis line metrics (supporting `BASELINE` alignment in horizontal rows).
    let item_baseline = |c: &FigmaNode, box_h: f64| -> f64 {
        if let Some(td) = &c.text {
            let fs = td.font_size.clamp(1.0, 1024.0);
            let lh = td.line_height.unwrap_or(fs * 1.2).max(fs);
            ((lh - fs) * 0.5 + fs * 0.8).clamp(0.0, box_h)
        } else {
            box_h
        }
    };

    let mut line_cross_sizes = Vec::with_capacity(lines.len());
    let mut line_max_ascents = Vec::with_capacity(lines.len());
    for line in &lines {
        if is_row && node.auto_layout.counter_axis_align_items == FigmaCounterAlign::Baseline {
            let mut max_asc = 0.0_f64;
            let mut max_desc = 0.0_f64;
            for &ci in line {
                let c = &node.children[ci];
                let out = child_stroke_outset(c, include_strokes);
                let (lo, hi) = child_c_bounds(c);
                let bh = clamp_opt_dim(c.height, lo, hi) + 2.0 * out;
                let asc = item_baseline(c, bh);
                max_asc = max_asc.max(asc);
                max_desc = max_desc.max((bh - asc).max(0.0));
            }
            line_cross_sizes.push(max_asc + max_desc);
            line_max_ascents.push(max_asc);
        } else {
            let mut mc = 0.0_f64;
            for &ci in line {
                let c = &node.children[ci];
                let out = child_stroke_outset(c, include_strokes);
                let (lo, hi) = child_c_bounds(c);
                let raw_c = if is_row { c.height } else { c.width };
                mc = mc.max(clamp_opt_dim(raw_c, lo, hi) + 2.0 * out);
            }
            line_cross_sizes.push(mc);
            line_max_ascents.push(mc);
        }
    }

    let counter_gap = node
        .auto_layout
        .counter_axis_spacing
        .unwrap_or_else(|| gap_p.max(0.0));
    let sum_line_cross: f64 = line_cross_sizes.iter().sum();
    let total_lines_cross = sum_line_cross + (lines.len().saturating_sub(1) as f64) * counter_gap;

    let authored_c = if is_row { node.height } else { node.width };
    let mut container_c = if counter_sizing == FigmaLayoutSizing::Hug {
        clamp_opt_dim(total_lines_cross + pad_c_start + pad_c_end, min_c, max_c)
    } else {
        clamp_opt_dim(authored_c, min_c, max_c)
    };
    let mut avail_c = (container_c - pad_c_start - pad_c_end).max(0.0);

    // Stretch counter-axis `FILL` / `STRETCH` items; in Vertical layout (`!is_row`), stretching
    // a child's width may change its `HEIGHT` text or wrapped height, so re-evaluate primary if needed!
    let mut vertical_height_changed_after_stretch = false;
    for (li, line) in lines.iter().enumerate() {
        let stretch_target = if lines.len() == 1
            && (counter_sizing != FigmaLayoutSizing::Hug || avail_c > line_cross_sizes[li])
        {
            avail_c
        } else {
            line_cross_sizes[li]
        };
        for &ci in line {
            if ctx.remaining_steps == 0 {
                break;
            }
            if !child_fills_counter(&node.children[ci], is_row) {
                continue;
            }
            let out = child_stroke_outset(&node.children[ci], include_strokes);
            let (lo, hi) = child_c_bounds(&node.children[ci]);
            let new_inner_c = clamp_opt_dim((stretch_target - 2.0 * out).max(0.0), lo, hi);
            if is_row {
                if (new_inner_c - node.children[ci].height).abs() > 1e-4 {
                    let prev_cw = node.children[ci].width;
                    let prev_ch = node.children[ci].height;
                    node.children[ci].height = new_inner_c;
                    node.children[ci].auto_layout.has_explicit_size = true;
                    if node.children[ci].auto_layout.layout_mode != FigmaLayoutMode::None {
                        if node.children[ci].auto_layout.sizing_vertical == FigmaLayoutSizing::Hug {
                            node.children[ci].auto_layout.sizing_vertical =
                                FigmaLayoutSizing::Fixed;
                        }
                        let _ = reflow_figma_node_inner(
                            &mut node.children[ci],
                            masters,
                            ctx,
                            true,
                            depth + 1,
                        );
                    } else if !node.children[ci].children.is_empty() {
                        let c_children = &mut node.children[ci];
                        for grand in &mut c_children.children {
                            if ctx.remaining_steps == 0 {
                                break;
                            }
                            apply_child_constraints_for_parent_resize(
                                grand,
                                prev_cw,
                                prev_ch,
                                prev_cw,
                                new_inner_c,
                                masters,
                                ctx,
                                depth + 1,
                            );
                        }
                    }
                }
            } else if (new_inner_c - node.children[ci].width).abs() > 1e-4 {
                let prev_cw = node.children[ci].width;
                let prev_ch = node.children[ci].height;
                node.children[ci].width = new_inner_c;
                node.children[ci].auto_layout.has_explicit_size = true;
                if let Some(td) = &node.children[ci].text
                    && td.auto_resize == "HEIGHT"
                {
                    let (_, th) = measure_figma_text_intrinsic(td, Some(new_inner_c));
                    let nh = clamp_opt_dim(
                        th,
                        node.children[ci].auto_layout.min_height,
                        node.children[ci].auto_layout.max_height,
                    );
                    if !child_fills_primary(&node.children[ci], false) {
                        node.children[ci].height = nh;
                        solved_p_box[ci] = nh + 2.0 * out;
                        vertical_height_changed_after_stretch = true;
                    }
                } else if node.children[ci].auto_layout.layout_mode != FigmaLayoutMode::None {
                    if node.children[ci].auto_layout.sizing_horizontal == FigmaLayoutSizing::Hug {
                        node.children[ci].auto_layout.sizing_horizontal = FigmaLayoutSizing::Fixed;
                    }
                    let _ = reflow_figma_node_inner(
                        &mut node.children[ci],
                        masters,
                        ctx,
                        true,
                        depth + 1,
                    );
                    if !child_fills_primary(&node.children[ci], false) {
                        solved_p_box[ci] = node.children[ci].height + 2.0 * out;
                        vertical_height_changed_after_stretch = true;
                    }
                } else if !node.children[ci].children.is_empty() {
                    let c_children = &mut node.children[ci];
                    for grand in &mut c_children.children {
                        if ctx.remaining_steps == 0 {
                            break;
                        }
                        apply_child_constraints_for_parent_resize(
                            grand,
                            prev_cw,
                            prev_ch,
                            new_inner_c,
                            prev_ch,
                            masters,
                            ctx,
                            depth + 1,
                        );
                    }
                }
            }
        }
        line_cross_sizes[li] = line_cross_sizes[li].max(
            line.iter()
                .map(|&ci| {
                    let c = &node.children[ci];
                    let out = child_stroke_outset(c, include_strokes);
                    (if is_row { c.height } else { c.width }) + 2.0 * out
                })
                .fold(0.0_f64, f64::max),
        );
    }

    let mut final_container_p = container_p;
    if !is_row && vertical_height_changed_after_stretch && primary_sizing == FigmaLayoutSizing::Hug
    {
        let updated_line_p = lines
            .iter()
            .map(|line| {
                line.iter().map(|&ci| solved_p_box[ci]).sum::<f64>()
                    + (line.len().saturating_sub(1) as f64) * gap_p
            })
            .fold(0.0_f64, f64::max);
        final_container_p = clamp_opt_dim(updated_line_p + pad_p_start + pad_p_end, min_p, max_p);
    }
    if counter_sizing == FigmaLayoutSizing::Hug {
        let updated_sum_c: f64 = line_cross_sizes.iter().sum();
        let updated_tot_c = updated_sum_c + (lines.len().saturating_sub(1) as f64) * counter_gap;
        container_c = clamp_opt_dim(updated_tot_c + pad_c_start + pad_c_end, min_c, max_c);
        avail_c = (container_c - pad_c_start - pad_c_end).max(0.0);
    }
    let final_avail_p = (final_container_p - pad_p_start - pad_p_end).max(0.0);

    // Compute counter-axis starting coordinate for each line.
    let mut line_c_starts = Vec::with_capacity(lines.len());
    if lines.len() == 1 {
        let lc = line_cross_sizes[0];
        let start_c = match node.auto_layout.counter_axis_align_items {
            FigmaCounterAlign::Min | FigmaCounterAlign::Baseline => {
                if node.auto_layout.counter_axis_align_items == FigmaCounterAlign::Baseline
                    && counter_sizing != FigmaLayoutSizing::Hug
                {
                    pad_c_start + (avail_c - lc) * 0.5
                } else {
                    pad_c_start
                }
            }
            FigmaCounterAlign::Center => pad_c_start + (avail_c - lc) * 0.5,
            FigmaCounterAlign::Max => pad_c_start + (avail_c - lc),
        };
        line_c_starts.push((
            start_c,
            if counter_sizing == FigmaLayoutSizing::Hug {
                lc
            } else {
                lc.max(avail_c)
            },
        ));
    } else {
        let sum_lc: f64 = line_cross_sizes.iter().sum();
        let (mut cur_c, step_gap) = if node.auto_layout.counter_axis_align_content
            == FigmaCounterAlignContent::SpaceBetween
            && avail_c > sum_lc
        {
            (pad_c_start, (avail_c - sum_lc) / ((lines.len() - 1) as f64))
        } else {
            let block_h = sum_lc + ((lines.len() - 1) as f64) * counter_gap;
            let offset = match node.auto_layout.counter_axis_align_items {
                FigmaCounterAlign::Min | FigmaCounterAlign::Baseline => 0.0,
                FigmaCounterAlign::Center => (avail_c - block_h) * 0.5,
                FigmaCounterAlign::Max => avail_c - block_h,
            };
            (pad_c_start + offset, counter_gap)
        };
        for &lc in &line_cross_sizes {
            line_c_starts.push((cur_c, lc));
            cur_c += lc + step_gap;
        }
    }

    // Place every flow item on its line!
    for (li, line) in lines.iter().enumerate() {
        let (line_c0, line_span_c) = if lines.len() == 1 {
            // For single-line layout, items align directly within `avail_c` (except Baseline).
            if node.auto_layout.counter_axis_align_items == FigmaCounterAlign::Baseline {
                line_c_starts[0]
            } else {
                (pad_c_start, avail_c)
            }
        } else {
            line_c_starts[li]
        };
        let used_p: f64 = line.iter().map(|&ci| solved_p_box[ci]).sum();
        let (mut cur_p, step_p) = match node.auto_layout.primary_axis_align_items {
            FigmaPrimaryAlign::SpaceBetween
                if line.len() > 1 && primary_sizing != FigmaLayoutSizing::Hug =>
            {
                (
                    pad_p_start,
                    (final_avail_p - used_p) / ((line.len() - 1) as f64),
                )
            }
            FigmaPrimaryAlign::Center => {
                let tot = used_p + (line.len().saturating_sub(1) as f64) * gap_p;
                (pad_p_start + (final_avail_p - tot) * 0.5, gap_p)
            }
            FigmaPrimaryAlign::Max => {
                let tot = used_p + (line.len().saturating_sub(1) as f64) * gap_p;
                (pad_p_start + (final_avail_p - tot), gap_p)
            }
            _ => (pad_p_start, gap_p),
        };

        let max_asc = line_max_ascents[li];
        for &ci in line {
            let out = child_stroke_outset(&node.children[ci], include_strokes);
            let item_c_box = (if is_row {
                node.children[ci].height
            } else {
                node.children[ci].width
            }) + 2.0 * out;
            let align_self = node.children[ci].auto_layout.layout_align.as_str();
            let pos_c_box = match align_self {
                "MIN" | "STRETCH" => line_c0,
                "CENTER" => line_c0 + (line_span_c - item_c_box) * 0.5,
                "MAX" => line_c0 + (line_span_c - item_c_box),
                "BASELINE" if is_row => {
                    let asc = item_baseline(&node.children[ci], item_c_box);
                    line_c0 + (max_asc - asc)
                }
                _ => match node.auto_layout.counter_axis_align_items {
                    FigmaCounterAlign::Min => line_c0,
                    FigmaCounterAlign::Center => line_c0 + (line_span_c - item_c_box) * 0.5,
                    FigmaCounterAlign::Max => line_c0 + (line_span_c - item_c_box),
                    FigmaCounterAlign::Baseline => {
                        if is_row {
                            let asc = item_baseline(&node.children[ci], item_c_box);
                            line_c0 + (max_asc - asc)
                        } else {
                            line_c0
                        }
                    }
                },
            };

            let final_p = cur_p + out;
            let final_c = pos_c_box + out;
            let (nx, ny) = if is_row {
                (final_p, final_c)
            } else {
                (final_c, final_p)
            };
            set_node_rel_xy(&mut node.children[ci], nx, ny);
            cur_p += solved_p_box[ci] + step_p;
        }
    }

    let (new_w, new_h) = if is_row {
        (final_container_p, container_c)
    } else {
        (container_c, final_container_p)
    };
    let prev_w = node.width;
    let prev_h = node.height;
    node.width = new_w;
    node.height = new_h;
    node.auto_layout.has_explicit_size = true;

    // Update `ABSOLUTE` children according to their horizontal & vertical constraints when container resized!
    if (new_w - prev_w).abs() > 1e-4 || (new_h - prev_h).abs() > 1e-4 {
        for child in &mut node.children {
            if ctx.remaining_steps == 0 {
                break;
            }
            if child.auto_layout.layout_positioning == FigmaLayoutPositioning::Absolute {
                apply_child_constraints_for_parent_resize(
                    child,
                    prev_w,
                    prev_h,
                    new_w,
                    new_h,
                    masters,
                    ctx,
                    depth + 1,
                );
            }
        }
    }

    (node.width - old_self_w).abs() > 1e-3 || (node.height - old_self_h).abs() > 1e-3
}
