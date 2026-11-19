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

use super::*;

/// Maximum distinct bitmap resources imported into `Project.resources` (`validation.rs` caps at 256).
const MAX_IMPORTED_BITMAPS: usize = 256;
/// Safe ceiling for total uncompressed RGBA8 pixels across all bitmaps (`28M * 4B = 112 MiB < 128 MiB`).
const MAX_TOTAL_BITMAP_PIXELS: u64 = 28_000_000;
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
    image_resources: HashMap<String, (String, u32, u32)>,
    project: &'a mut Project,
    report: &'a mut ImportReport,
    authored_elements: usize,
    warned_element_budget: bool,
    warned_expanded_budget: bool,
    warned_symbol_budget: bool,
    warned_override_budget: bool,
}

impl<'a, 'b> FigmaBuildCtx<'a, 'b> {
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
                    out.push(FilterSpec::Blur {
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        quality: 2,
                    });
                }
                FigmaEffect::BackgroundBlur { radius } => {
                    self.report.warning(
                        "figma-background-blur-approximated",
                        location.to_owned(),
                        format!(
                            "Figma BACKGROUND_BLUR ({radius:.1}px) approximated as layer Blur filter."
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
        if inst_node.symbol_overrides.is_empty() {
            return self.ensure_component_symbol(comp_id);
        }
        let sig = hash_instance_overrides(comp_id, &inst_node.symbol_overrides);
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

        let master = (*self.master_nodes.get(comp_id)?).clone();
        let mut specialized = master;
        let mut any_applied = false;
        for ov in &inst_node.symbol_overrides {
            if apply_override_at_path(&mut specialized, &ov.guid_path, ov) {
                any_applied = true;
            }
        }
        if !any_applied {
            return self.ensure_component_symbol(comp_id);
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
        if (!container.fills.is_empty() || !container.strokes.is_empty())
            && container.width > 0.0
            && container.height > 0.0
            && self.authored_elements < MAX_AUTHORED_ELEMENTS
        {
            let mut bg_node = container.clone();
            bg_node.children.clear();
            bg_node.x = 0.0;
            bg_node.y = 0.0;
            bg_node.rotation_deg = 0.0;
            bg_node.relative_transform = None;
            bg_node.opacity = 1.0;
            bg_node.effects.clear();
            bg_node.blend_mode = "NORMAL".into();
            let mut bg_bfs = Vec::new();
            let (bg_els, bg_exp, _) = self.convert_leaf_to_elements(
                &bg_node,
                0.0,
                0.0,
                &format!("{} / Background", container.name),
                expanded_budget,
                depth + 1,
                &mut bg_bfs,
            );
            if !bg_els.is_empty() {
                expanded_used = expanded_used.saturating_add(bg_exp);
                layers.push(Layer {
                    id: new_id("layer"),
                    name: "Background".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: bg_els,
                        bitmap_fills: bg_bfs,
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
    ) -> (Vec<Element>, usize, usize) {
        if depth > MAX_SYMBOL_GRAPH_DEPTH
            || expanded_budget == 0
            || self.authored_elements >= MAX_AUTHORED_ELEMENTS
        {
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

        // 2. Non-empty container (`FRAME`, `GROUP`, `SECTION`, `COMPONENT_SET`, detached `INSTANCE`)
        if !node.children.is_empty() && node.fill_paths.is_empty() && node.text.is_none() {
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
            let min_leading = -(font_size * 0.9);
            let leading = td
                .line_height
                .map(|lh| (lh - font_size).clamp(min_leading, 500.0))
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
            if !td.style_runs.is_empty() || td.strikethrough || td.align_v != "top" {
                let mut opts = AuthoredTextFieldOptions {
                    vertical_align: td.align_v.clone(),
                    strikethrough: td.strikethrough,
                    ..AuthoredTextFieldOptions::default()
                };
                if !td.style_runs.is_empty() {
                    let total_u16 = normalized_chars.encode_utf16().count() as u32;
                    let mut runs = Vec::new();
                    let mut cursor = 0u32;
                    for sr in &td.style_runs {
                        let s = sr.start_utf16.min(total_u16);
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
                            rf.leading = (lh - rf.size).clamp(min_l, 500.0);
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
                    opts.format_runs = runs;
                }
                if !opts.is_default() {
                    final_fmt.set_field_options(Some(opts));
                }
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

        // 4. Vector / Primitive / Image Fill Elements
        let mut out = Vec::new();
        let (fallback_cmds, prim_opt) = self.fallback_shape_commands(node);
        let indep_borders = has_unequal_border_weights(node);
        let stroke_w = if node.strokes.is_empty() || indep_borders {
            0.0
        } else {
            node.stroke_weight.clamp(0.0, 256.0)
        };
        let (stroke_solid, stroke_grad) = match node.strokes.first() {
            Some(FigmaPaint::Solid { color }) => (*color, None),
            Some(FigmaPaint::Gradient {
                kind,
                stops,
                start,
                end,
                approximated_from,
            }) => {
                if let Some(approx) = approximated_from {
                    self.report.warning(
                        "figma-gradient-approximated",
                        location.to_owned(),
                        format!("Figma {approx} stroke gradient approximated as Radial gradient."),
                    );
                }
                (
                    stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]),
                    Some(build_figma_gradient_paint(*kind, stops, *start, *end)),
                )
            }
            _ => ([0, 0, 0, 0], None),
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

        let is_rect_node = matches!(
            node.node_type.as_str(),
            "RECTANGLE" | "ROUNDED_RECTANGLE" | "FRAME" | "COMPONENT" | "SYMBOL" | "SECTION"
        );
        let has_radius = node.corner_radii.iter().any(|&r| r > 0.0);

        if node.fills.is_empty() && stroke_w > 0.0 {
            let scmds = node
                .stroke_paths
                .first()
                .filter(|c| validate_path_commands(c).is_ok())
                .cloned()
                .unwrap_or_else(|| fallback_cmds.clone());
            if validate_path_commands(&scmds).is_ok() {
                let kind = if stroke_grad.is_some() {
                    ElementKind::PaintedPath {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        fill_gradient: None,
                        stroke_gradient: stroke_grad.clone(),
                        stroke_width: stroke_w,
                    }
                } else {
                    ElementKind::Path {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        stroke_width: stroke_w,
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
                    filters: filters.clone(),
                    ..Element::default()
                });
                self.authored_elements += 1;
                self.report.statistics.elements += 1;
            }
        }

        for (fi, paint) in node.fills.iter().enumerate() {
            if self.authored_elements >= MAX_AUTHORED_ELEMENTS || out.len() >= expanded_budget {
                break;
            }
            let is_last_fill = fi + 1 == node.fills.len();
            let cur_stroke = if is_last_fill {
                stroke_solid
            } else {
                [0, 0, 0, 0]
            };
            let cur_stroke_w = if is_last_fill { stroke_w } else { 0.0 };
            let cur_stroke_grad = if is_last_fill {
                stroke_grad.clone()
            } else {
                None
            };

            match paint {
                FigmaPaint::Image {
                    image_ref,
                    scale_mode,
                    alpha,
                } => {
                    if let Some((res_id, iw, ih)) = self.lookup_image(image_ref, location) {
                        // Direct crisp `Bitmap` for non-rounded rectangular fills without explicit path
                        if is_rect_node
                            && !has_radius
                            && node.fill_paths.is_empty()
                            && scale_mode != "TILE"
                        {
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
                                filters: if is_last_fill && cur_stroke_w == 0.0 {
                                    filters.clone()
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
                                    filters: filters.clone(),
                                    ..Element::default()
                                });
                                self.authored_elements += 1;
                                self.report.statistics.elements += 1;
                            }
                        } else if let Some((cmds, rule)) = paths_to_fill.first() {
                            // Rounded-rect / ellipse / vector shape with bitmap fill -> `Path` + `AuthoredBitmapFill`!
                            let el_id = new_id("el");
                            let mut shp_tf = base_tf;
                            shp_tf.alpha = (shp_tf.alpha * alpha).clamp(0.0, 1.0);
                            let (bf_matrix, bf_repeat) = compute_bitmap_fill_matrix(
                                iw,
                                ih,
                                node.width,
                                node.height,
                                scale_mode,
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
                                filters: if is_last_fill {
                                    filters.clone()
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
                            filters: if is_last_fill {
                                filters.clone()
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
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.authored_elements += 1;
                        self.report.statistics.elements += 1;
                    } else {
                        for (cmds, rule) in &paths_to_fill {
                            if self.authored_elements >= MAX_AUTHORED_ELEMENTS {
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
                                filters: if is_last_fill {
                                    filters.clone()
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
                    start,
                    end,
                    approximated_from,
                } => {
                    if let Some(approx) = approximated_from {
                        self.report.warning(
                            "figma-gradient-approximated",
                            location.to_owned(),
                            format!(
                                "Figma {approx} fill gradient approximated as Radial gradient."
                            ),
                        );
                    }
                    let grad = build_figma_gradient_paint(*kind, stops, *start, *end);
                    let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                    for (cmds, rule) in &paths_to_fill {
                        if self.authored_elements >= MAX_AUTHORED_ELEMENTS {
                            break;
                        }
                        out.push(Element {
                            id: new_id("el"),
                            name: sanitize_element_name(&node.name),
                            transform: base_tf,
                            kind: ElementKind::PaintedPath {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: fallback_c,
                                stroke: cur_stroke,
                                fill_gradient: Some(grad.clone()),
                                stroke_gradient: cur_stroke_grad.clone(),
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
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
        }

        if indep_borders && self.authored_elements + 4 <= MAX_AUTHORED_ELEMENTS {
            let borders = build_independent_border_elements(
                node,
                base_tf,
                stroke_solid,
                stroke_grad,
                blend_mode,
            );
            let added = borders.len();
            self.authored_elements += added;
            self.report.statistics.elements += added;
            out.extend(borders);
        }

        let count = out.len();
        (out, count, 1)
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
            for child in children {
                if !child.visible && !self.options.import_hidden_layers {
                    continue;
                }
                let rem = expanded_budget.saturating_sub(expanded_used);
                if rem == 0 || self.authored_elements >= MAX_AUTHORED_ELEMENTS {
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

            if matches!(child.node_type.as_str(), "GROUP" | "SECTION")
                && !child.is_mask
                && child.fills.is_empty()
                && child.strokes.is_empty()
                && child.effects.is_empty()
                && (child.opacity - 1.0).abs() < 1e-6
                && matches!(child.blend_mode.as_str(), "" | "PASS_THROUGH" | "NORMAL")
                && self.options.layer_mapping == FigmaLayerMapping::TimelineLayers
                && !child.children.is_empty()
                && child.children.len() <= 64
                && depth <= 3
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
                    offset_x - child.x,
                    offset_y - child.y,
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
            let (mut elements, c_exp, c_dep) = self.convert_leaf_to_elements(
                child,
                offset_x,
                offset_y,
                &loc,
                rem_budget,
                depth + 1,
                &mut bfs,
            );
            expanded_used = expanded_used.saturating_add(c_exp);
            max_depth = max_depth.max(c_dep);

            if self.options.layer_mapping == FigmaLayerMapping::MovieClipSymbols
                && elements.len() == 1
                && !matches!(elements[0].kind, ElementKind::Instance { .. })
                && self.project.symbols.len() < MAX_PROJECT_SYMBOLS
            {
                let sym_id = new_id("figmc");
                let inner_el = elements.remove(0);
                let inner_bfs = std::mem::take(&mut bfs);
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
                    ..Keyframe::default()
                }],
                properties: props,
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
        }

        // Coalesce adjacent plain normal layers when a container has > 96 layers to keep `.nflash`
        // JSON compact while preserving exact bottom-to-top draw order (`layers[0]` = bottom).
        if layers.len() > 96 {
            layers = coalesce_plain_layers(layers);
        }

        (layers, expanded_used, max_depth)
    }

    fn build_page_layers(
        &mut self,
        page: &FigmaNode,
        update_stage_metrics: bool,
        expanded_budget: usize,
    ) -> (Vec<Layer>, usize) {
        if update_stage_metrics && let Some(bg) = page.background_color {
            self.project.background = bg;
        }

        let visible_top: Vec<&FigmaNode> = page
            .children
            .iter()
            .filter(|n| n.visible || self.options.import_hidden_layers)
            .collect();

        if visible_top.len() == 1
            && matches!(
                visible_top[0].node_type.as_str(),
                "FRAME" | "COMPONENT" | "SYMBOL" | "SECTION"
            )
        {
            let artboard = visible_top[0];
            if update_stage_metrics {
                if self.options.resize_stage_to_artboard
                    && artboard.width >= 1.0
                    && artboard.height >= 1.0
                {
                    self.project.width = (artboard.width.round() as u32).clamp(1, 8192);
                    self.project.height = (artboard.height.round() as u32).clamp(1, 8192);
                }
                if let Some(FigmaPaint::Solid { color }) = artboard.fills.first()
                    && color[3] > 0
                {
                    self.project.background = [color[0], color[1], color[2], 255];
                }
            }
            let (layers, exp, _) = self.build_layers_for_children(
                &artboard.children,
                0.0,
                0.0,
                if artboard.clips_content {
                    Some((
                        artboard.width.max(1.0),
                        artboard.height.max(1.0),
                        artboard.corner_radii,
                    ))
                } else {
                    None
                },
                &artboard.name,
                expanded_budget,
                1,
            );
            (layers, exp)
        } else {
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for n in &visible_top {
                if n.x.is_finite() && n.y.is_finite() {
                    min_x = min_x.min(n.x);
                    min_y = min_y.min(n.y);
                    max_x = max_x.max(n.x + n.width.max(0.0));
                    max_y = max_y.max(n.y + n.height.max(0.0));
                }
            }
            let base_x = if min_x.is_finite() { min_x } else { 0.0 };
            let base_y = if min_y.is_finite() { min_y } else { 0.0 };

            if update_stage_metrics && self.options.resize_stage_to_artboard {
                let bbox_w = (max_x - min_x).round();
                let bbox_h = (max_y - min_y).round();
                let largest_frame = visible_top
                    .iter()
                    .filter(|n| matches!(n.node_type.as_str(), "FRAME" | "SECTION"))
                    .max_by(|a, b| {
                        (a.width * a.height)
                            .partial_cmp(&(b.width * b.height))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .or_else(|| {
                        visible_top.iter().find(|n| {
                            matches!(
                                n.node_type.as_str(),
                                "FRAME" | "SECTION" | "COMPONENT" | "SYMBOL"
                            )
                        })
                    });

                if bbox_w.is_finite()
                    && bbox_h.is_finite()
                    && bbox_w >= 1.0
                    && bbox_h >= 1.0
                    && bbox_w <= 3840.0
                    && bbox_h <= 2160.0
                {
                    self.project.width = (bbox_w as u32).clamp(1, 8192);
                    self.project.height = (bbox_h as u32).clamp(1, 8192);
                } else if let Some(frame) = largest_frame
                    && frame.width >= 1.0
                    && frame.height >= 1.0
                {
                    self.project.width = (frame.width.round() as u32).clamp(1, 8192);
                    self.project.height = (frame.height.round() as u32).clamp(1, 8192);
                }
            }
            let (layers, exp, _) = self.build_layers_for_children(
                &page.children,
                base_x,
                base_y,
                None,
                &page.name,
                expanded_budget,
                1,
            );
            (layers, exp)
        }
    }

    fn build_single_layer_for_page(
        &mut self,
        page: &FigmaNode,
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

        let node_origin = |n: &FigmaNode| -> (f64, f64) {
            if let Some([_, _, _, _, tx, ty]) = n.relative_transform
                && tx.is_finite()
                && ty.is_finite()
            {
                (tx, ty)
            } else {
                (n.x, n.y)
            }
        };

        let (base_x, base_y) = if visible_top.len() == 1
            && matches!(
                visible_top[0].node_type.as_str(),
                "FRAME" | "COMPONENT" | "SYMBOL" | "SECTION"
            ) {
            let artboard = visible_top[0];
            if update_stage_metrics {
                if self.options.resize_stage_to_artboard
                    && artboard.width >= 1.0
                    && artboard.height >= 1.0
                {
                    self.project.width = (artboard.width.round() as u32).clamp(1, 8192);
                    self.project.height = (artboard.height.round() as u32).clamp(1, 8192);
                }
                if let Some(FigmaPaint::Solid { color }) = artboard.fills.first()
                    && color[3] > 0
                {
                    self.project.background = [color[0], color[1], color[2], 255];
                }
            }
            let (ax, ay) = node_origin(artboard);
            (
                if ax.is_finite() { ax } else { 0.0 },
                if ay.is_finite() { ay } else { 0.0 },
            )
        } else {
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for n in &visible_top {
                let (nx, ny) = node_origin(n);
                if nx.is_finite() && ny.is_finite() {
                    min_x = min_x.min(nx);
                    min_y = min_y.min(ny);
                    max_x = max_x.max(nx + n.width.max(0.0));
                    max_y = max_y.max(ny + n.height.max(0.0));
                }
            }
            let bx = if min_x.is_finite() { min_x } else { 0.0 };
            let by = if min_y.is_finite() { min_y } else { 0.0 };

            if update_stage_metrics && self.options.resize_stage_to_artboard {
                let bbox_w = (max_x - min_x).round();
                let bbox_h = (max_y - min_y).round();
                let largest_frame = visible_top
                    .iter()
                    .filter(|n| matches!(n.node_type.as_str(), "FRAME" | "SECTION"))
                    .max_by(|a, b| {
                        (a.width * a.height)
                            .partial_cmp(&(b.width * b.height))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .or_else(|| {
                        visible_top.iter().find(|n| {
                            matches!(
                                n.node_type.as_str(),
                                "FRAME" | "SECTION" | "COMPONENT" | "SYMBOL"
                            )
                        })
                    });

                if bbox_w.is_finite()
                    && bbox_h.is_finite()
                    && bbox_w >= 1.0
                    && bbox_h >= 1.0
                    && bbox_w <= 3840.0
                    && bbox_h <= 2160.0
                {
                    self.project.width = (bbox_w as u32).clamp(1, 8192);
                    self.project.height = (bbox_h as u32).clamp(1, 8192);
                } else if let Some(frame) = largest_frame
                    && frame.width >= 1.0
                    && frame.height >= 1.0
                {
                    self.project.width = (frame.width.round() as u32).clamp(1, 8192);
                    self.project.height = (frame.height.round() as u32).clamp(1, 8192);
                }
            }
            (bx, by)
        };

        let mut all_elements = Vec::new();
        let mut all_bfs = Vec::new();
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
                );
                expanded_used = expanded_used.saturating_add(c_exp);
                all_elements.extend(els);
            }
        }

        let layer = Layer {
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
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        };
        self.report.statistics.layers += 1;
        self.report.statistics.keyframes += 1;
        (layer, expanded_used)
    }
}

pub(super) fn build_project_from_figma_doc(
    doc: FigmaDoc,
    images: &mut FigmaImageArchive<'_>,
    options: &FigmaImportOptions,
    source_label: &str,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    project.name = sanitize_symbol_name(&doc.name);
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
        image_resources: HashMap::new(),
        project: &mut project,
        report: &mut report,
        authored_elements: 0,
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
                let (layer, p_exp) = ctx.build_single_layer_for_page(page, idx, idx == 0, rem);
                expanded_used = expanded_used.saturating_add(p_exp);
                page_layers.push(layer);
            }
            let root_id = ctx.project.root.clone();
            if let Some(root_sym) = ctx.project.symbol_mut(&root_id)
                && !page_layers.is_empty()
            {
                root_sym.timeline.layers = page_layers;
            }
        }
        FigmaPageMapping::Scenes => {
            let primary_page = if options.selected_page_ids.is_some() {
                user_pages[0]
            } else {
                user_pages
                    .iter()
                    .copied()
                    .find(|p| (p.visible || options.import_hidden_layers) && !p.children.is_empty())
                    .unwrap_or(user_pages[0])
            };

            let mut scenes_expanded_used = 1usize;
            let first_budget = MAX_EXPANDED_SCENE_NODES.saturating_sub(scenes_expanded_used);
            let (root_layers, root_exp) = ctx.build_page_layers(primary_page, true, first_budget);
            scenes_expanded_used = scenes_expanded_used.saturating_add(root_exp);

            let root_id = ctx.project.root.clone();
            if let Some(root_sym) = ctx.project.symbol_mut(&root_id) {
                if user_pages.len() > 1 && !primary_page.name.trim().is_empty() {
                    root_sym.name = sanitize_symbol_name(&primary_page.name);
                }
                if !root_layers.is_empty() {
                    root_sym.timeline.layers = root_layers;
                }
            }

            // Map additional non-empty user pages to `Project.scenes` (just like multi-scene FLA/XFL!),
            // keeping `Internal Only Canvas` strictly excluded from stage scenes.
            let extra_user_pages: Vec<&FigmaNode> = if options.selected_page_ids.is_some() {
                user_pages.iter().copied().skip(1).collect()
            } else {
                user_pages
                    .iter()
                    .copied()
                    .filter(|p| {
                        p.id != primary_page.id
                            && !p.internal_only
                            && (p.visible || options.import_hidden_layers)
                            && !p.children.is_empty()
                    })
                    .collect()
            };

            if !extra_user_pages.is_empty() {
                let mut scene_ids = vec![root_id];
                for (idx, extra_page) in extra_user_pages.into_iter().enumerate() {
                    if ctx.project.symbols.len() >= MAX_PROJECT_SYMBOLS
                        || ctx.authored_elements >= MAX_AUTHORED_ELEMENTS
                    {
                        break;
                    }
                    let rem = MAX_EXPANDED_SCENE_NODES.saturating_sub(scenes_expanded_used);
                    if rem < 4 {
                        if !ctx.warned_expanded_budget {
                            ctx.warned_expanded_budget = true;
                            ctx.report.warning(
                                "figma-expanded-node-budget-exceeded",
                                extra_page.name.clone(),
                                format!("Compacted remaining Figma pages after reaching {MAX_EXPANDED_SCENE_NODES} expanded scene nodes."),
                            );
                        }
                        break;
                    }
                    let (mut page_layers, p_exp) = ctx.build_page_layers(extra_page, false, rem);
                    scenes_expanded_used = scenes_expanded_used.saturating_add(p_exp);
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
    }

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

fn collect_referenced_image_keys(
    node: &FigmaNode,
    include_hidden: bool,
    is_root_or_master: bool,
    ordered: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    if !is_root_or_master && !node.visible && !include_hidden {
        return;
    }
    for p in node.fills.iter().chain(node.strokes.iter()) {
        if let FigmaPaint::Image { image_ref, .. } = p {
            let low = image_ref.to_ascii_lowercase();
            if !low.is_empty() && seen.insert(low.clone()) {
                ordered.push(low);
            }
        }
    }
    for ov in &node.symbol_overrides {
        for plist in [&ov.fills, &ov.strokes].into_iter().flatten() {
            for p in plist {
                if let FigmaPaint::Image { image_ref, .. } = p {
                    let low = image_ref.to_ascii_lowercase();
                    if !low.is_empty() && seen.insert(low.clone()) {
                        ordered.push(low);
                    }
                }
            }
        }
    }
    for child in &node.children {
        collect_referenced_image_keys(
            child,
            include_hidden,
            is_root_or_master && matches!(node.node_type.as_str(), "COMPONENT" | "SYMBOL"),
            ordered,
            seen,
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
    let mut ordered_refs = Vec::new();
    let mut seen = HashSet::new();
    let include_hidden = ctx.options.import_hidden_layers;
    let mut visited_masters = HashSet::new();
    let mut ref_masters = Vec::new();

    for &page in user_pages {
        collect_referenced_image_keys(page, include_hidden, true, &mut ordered_refs, &mut seen);
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
        collect_referenced_image_keys(mnode, true, true, &mut ordered_refs, &mut seen);
    }
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
        limits.max_image_width = Some(32_768);
        limits.max_image_height = Some(32_768);
        limits.max_alloc = Some(512 * 1024 * 1024);
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
        if ow == 0 || oh == 0 {
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

    // Enforce global RGBA8 runtime pixel budget (`sum(tw * th) <= MAX_TOTAL_BITMAP_PIXELS`).
    loop {
        let total_px: u64 = candidates
            .iter()
            .map(|c| u64::from(c.target_w) * u64::from(c.target_h))
            .sum();
        if total_px <= MAX_TOTAL_BITMAP_PIXELS {
            break;
        }
        let small_px: u64 = candidates
            .iter()
            .filter(|c| u64::from(c.target_w) * u64::from(c.target_h) <= 65_536)
            .map(|c| u64::from(c.target_w) * u64::from(c.target_h))
            .sum();
        let large_px = total_px.saturating_sub(small_px);
        if large_px == 0 {
            let factor = ((MAX_TOTAL_BITMAP_PIXELS as f64) / (total_px as f64))
                .sqrt()
                .min(0.95);
            for c in &mut candidates {
                c.target_w = ((f64::from(c.target_w) * factor).floor() as u32).max(1);
                c.target_h = ((f64::from(c.target_h) * factor).floor() as u32).max(1);
            }
        } else {
            let avail_large = MAX_TOTAL_BITMAP_PIXELS.saturating_sub(small_px).max(1);
            let factor = ((avail_large as f64) / (large_px as f64)).sqrt().min(0.95);
            for c in &mut candidates {
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
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(512 * 1024 * 1024);
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
) -> ([f64; 6], bool) {
    let bw = f64::from(iw.max(1));
    let bh = f64::from(ih.max(1));
    let sw = shape_w.max(1.0);
    let sh = shape_h.max(1.0);
    match scale_mode {
        "TILE" => ([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], true),
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

fn hash_instance_overrides(comp_id: &str, overrides: &[FigmaSymbolOverride]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    comp_id.hash(&mut h);
    overrides.len().hash(&mut h);
    for ov in overrides {
        ov.guid_path.hash(&mut h);
        ov.visible.hash(&mut h);
        if let Some(op) = ov.opacity {
            ((op * 1000.0).round() as i32).hash(&mut h);
        } else {
            (-1i32).hash(&mut h);
        }
        ov.characters.hash(&mut h);
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
                        FigmaPaint::Gradient { stops, .. } => {
                            2u8.hash(&mut h);
                            for s in stops {
                                s.ratio.hash(&mut h);
                                s.color.hash(&mut h);
                            }
                        }
                        FigmaPaint::Image {
                            image_ref,
                            scale_mode,
                            ..
                        } => {
                            3u8.hash(&mut h);
                            image_ref.hash(&mut h);
                            scale_mode.hash(&mut h);
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

fn apply_override_at_path(node: &mut FigmaNode, path: &[String], ov: &FigmaSymbolOverride) -> bool {
    if path.is_empty() {
        return false;
    }
    let mut applied = false;
    for child in &mut node.children {
        if guid_matches_target(&child.id, &path[0]) {
            if path.len() == 1 {
                if let Some(vis) = ov.visible {
                    child.visible = vis;
                }
                if let Some(op) = ov.opacity {
                    child.opacity = op;
                }
                if let Some(chars) = &ov.characters
                    && let Some(txt) = &mut child.text
                {
                    txt.characters.clone_from(chars);
                    txt.style_runs.clear();
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
            } else if apply_override_at_path(child, &path[1..], ov) {
                applied = true;
            }
        } else if path.len() == 1 && apply_override_at_path(child, path, ov) {
            applied = true;
        }
    }
    applied
}

fn is_coalesceable_layer(l: &Layer) -> bool {
    l.visible && !l.locked && l.properties.is_default() && l.keyframes.len() == 1
}

fn coalesce_plain_layers(layers: Vec<Layer>) -> Vec<Layer> {
    let mut out: Vec<Layer> = Vec::with_capacity(64);
    for mut layer in layers {
        if is_coalesceable_layer(&layer)
            && let Some(prev) = out.last_mut()
            && is_coalesceable_layer(prev)
            && prev.keyframes[0].elements.len() + layer.keyframes[0].elements.len() <= 128
        {
            let mut src_kf = layer.keyframes.pop().unwrap();
            prev.keyframes[0].elements.append(&mut src_kf.elements);
            prev.keyframes[0]
                .bitmap_fills
                .append(&mut src_kf.bitmap_fills);
            continue;
        }
        out.push(layer);
    }
    out
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
    start: [f64; 2],
    end: [f64; 2],
) -> GradientPaint {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len = dx.hypot(dy).max(1.0);
    let matrix = match kind {
        GradientKind::Linear => {
            let cos_a = dx / len;
            let sin_a = dy / len;
            let s = len / 1638.4;
            let mx = (start[0] + end[0]) * 0.5;
            let my = (start[1] + end[1]) * 0.5;
            [s * cos_a, s * sin_a, -s * sin_a, s * cos_a, mx, my]
        }
        GradientKind::Radial => {
            let s = len / 819.2;
            [s, 0.0, 0.0, s, start[0], start[1]]
        }
    };
    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|st| st.ratio);
    sorted.truncate(15);
    GradientPaint {
        kind,
        matrix,
        stops: sorted,
        focal_point_ratio: 0.0,
        ..Default::default()
    }
}
