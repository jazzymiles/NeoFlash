//! Multi-Paint `Appearance` stack, path modifiers (`Repeater`, `Offset Paths`, `Round Corners`,
//! `Mirror`), procedural vector effects (`Zig Zag`, `Pucker & Bloat`, `Roughen`, `Twist`), and
//! reusable Graphic Styles (`VectorStyle`) (`T-233`).
#![allow(dead_code)]

use crate::app::Editor;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components as nf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceModifierKind {
    OffsetContour,
    LiveCorners,
    Repeater,
    Mirror,
}

impl AppearanceModifierKind {
    pub const ALL: [Self; 4] = [
        Self::OffsetContour,
        Self::LiveCorners,
        Self::Repeater,
        Self::Mirror,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OffsetContour => "Offset Paths",
            Self::LiveCorners => "Round Corners",
            Self::Repeater => "Repeater",
            Self::Mirror => "Mirror",
        }
    }

    pub fn default_modifier(self) -> VectorModifier {
        match self {
            Self::OffsetContour => VectorModifier::OffsetContour { delta: 4.0 },
            Self::LiveCorners => VectorModifier::LiveCorners { radius: 8.0 },
            Self::Repeater => VectorModifier::Repeater {
                copies: 3,
                offset_x: 24.0,
                offset_y: 0.0,
                scale_step: 1.0,
                rotation_step: 0.0,
            },
            Self::Mirror => VectorModifier::Mirror {
                axis_x: Some(0.0),
                axis_y: None,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceProceduralKind {
    ZigZag,
    OffsetPath,
    PuckerBloat,
    Roughen,
    Twist,
}

impl AppearanceProceduralKind {
    pub const ALL: [Self; 5] = [
        Self::ZigZag,
        Self::OffsetPath,
        Self::PuckerBloat,
        Self::Roughen,
        Self::Twist,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ZigZag => "Zig Zag",
            Self::OffsetPath => "Offset Path (Effect)",
            Self::PuckerBloat => "Pucker & Bloat",
            Self::Roughen => "Roughen",
            Self::Twist => "Twist",
        }
    }

    pub fn default_effect(self) -> ProceduralVectorEffect {
        match self {
            Self::ZigZag => ProceduralVectorEffect::ZigZag {
                size: 4.0,
                ridges_per_segment: 3,
                smooth: false,
            },
            Self::OffsetPath => ProceduralVectorEffect::OffsetPath { offset: 4.0 },
            Self::PuckerBloat => ProceduralVectorEffect::PuckerBloat { amount: 20.0 },
            Self::Roughen => ProceduralVectorEffect::Roughen {
                size: 3.0,
                detail: 4,
                seed: 7,
            },
            Self::Twist => ProceduralVectorEffect::Twist { angle_deg: 45.0 },
        }
    }
}

fn default_appearance_for_element(element: &Element) -> ElementAppearance {
    let mut entries = Vec::new();
    match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            if fill[3] > 0 {
                entries.push(AppearancePaintEntry::Fill {
                    color: *fill,
                    gradient: None,
                    swatch_id: None,
                    offset: [0.0, 0.0],
                    opacity: 1.0,
                });
            }
            if *stroke_width > 0.0 && stroke[3] > 0 {
                entries.push(AppearancePaintEntry::Stroke {
                    color: *stroke,
                    gradient: None,
                    width: *stroke_width,
                    swatch_id: None,
                    offset_distance: 0.0,
                    pattern_brush: None,
                    opacity: 1.0,
                });
            }
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            if fill[3] > 0 || fill_gradient.is_some() {
                entries.push(AppearancePaintEntry::Fill {
                    color: *fill,
                    gradient: fill_gradient.clone(),
                    swatch_id: None,
                    offset: [0.0, 0.0],
                    opacity: 1.0,
                });
            }
            if *stroke_width > 0.0 && (stroke[3] > 0 || stroke_gradient.is_some()) {
                entries.push(AppearancePaintEntry::Stroke {
                    color: *stroke,
                    gradient: stroke_gradient.clone(),
                    width: *stroke_width,
                    swatch_id: None,
                    offset_distance: 0.0,
                    pattern_brush: None,
                    opacity: 1.0,
                });
            }
        }
        _ => {}
    }
    ElementAppearance {
        element_id: element.id.clone(),
        fill_swatch_id: None,
        stroke_swatch_id: None,
        style_id: None,
        entries,
        modifiers: Vec::new(),
        text_binding: None,
    }
}

fn sync_primary_element_paints(element: &mut Element, appearance: &ElementAppearance) {
    let first_visible_fill = appearance.entries.iter().find_map(|entry| match entry {
        AppearancePaintEntry::Fill {
            color,
            gradient,
            opacity,
            ..
        } if *opacity > 0.0 => {
            let mut c = *color;
            c[3] = ((c[3] as f64) * opacity.clamp(0.0, 1.0)).round() as u8;
            Some((c, gradient.clone()))
        }
        _ => None,
    });
    let first_visible_stroke = appearance.entries.iter().find_map(|entry| match entry {
        AppearancePaintEntry::Stroke {
            color,
            gradient,
            width,
            opacity,
            ..
        } if *opacity > 0.0 && *width > 0.0 => {
            let mut c = *color;
            c[3] = ((c[3] as f64) * opacity.clamp(0.0, 1.0)).round() as u8;
            Some((c, gradient.clone(), *width))
        }
        _ => None,
    });

    match &mut element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            if let Some((f_col, _)) = first_visible_fill {
                *fill = f_col;
            }
            if let Some((s_col, _, s_w)) = first_visible_stroke {
                *stroke = s_col;
                *stroke_width = s_w;
            }
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            if let Some((f_col, f_grad)) = first_visible_fill {
                *fill = f_col;
                *fill_gradient = f_grad;
            }
            if let Some((s_col, s_grad, s_w)) = first_visible_stroke {
                *stroke = s_col;
                *stroke_gradient = s_grad;
                *stroke_width = s_w;
            }
        }
        _ => {}
    }
}

fn sanitize_modifier(modifier: VectorModifier) -> VectorModifier {
    match modifier {
        VectorModifier::OffsetContour { delta } => VectorModifier::OffsetContour {
            delta: if delta.is_finite() {
                delta.clamp(-10_000.0, 10_000.0)
            } else {
                0.0
            },
        },
        VectorModifier::LiveCorners { radius } => VectorModifier::LiveCorners {
            radius: if radius.is_finite() {
                radius.clamp(0.0, 10_000.0)
            } else {
                0.0
            },
        },
        VectorModifier::Mirror { axis_x, axis_y } => VectorModifier::Mirror {
            axis_x: axis_x.map(|v| {
                if v.is_finite() {
                    v.clamp(-1e5, 1e5)
                } else {
                    0.0
                }
            }),
            axis_y: axis_y.map(|v| {
                if v.is_finite() {
                    v.clamp(-1e5, 1e5)
                } else {
                    0.0
                }
            }),
        },
        VectorModifier::Repeater {
            copies,
            offset_x,
            offset_y,
            scale_step,
            rotation_step,
        } => VectorModifier::Repeater {
            copies: copies.clamp(1, 64),
            offset_x: if offset_x.is_finite() {
                offset_x.clamp(-1e5, 1e5)
            } else {
                0.0
            },
            offset_y: if offset_y.is_finite() {
                offset_y.clamp(-1e5, 1e5)
            } else {
                0.0
            },
            scale_step: if scale_step.is_finite() && scale_step > 0.0 {
                scale_step.clamp(0.05, 10.0)
            } else {
                1.0
            },
            rotation_step: if rotation_step.is_finite() {
                rotation_step.clamp(-360.0, 360.0)
            } else {
                0.0
            },
        },
    }
}

impl Editor {
    fn selected_vector_element_id(&self) -> Option<Id> {
        let id = self
            .selected_element()
            .map(|e| e.id.clone())
            .or_else(|| self.selection.targets.first().map(|t| t.element.clone()))?;
        let symbol = self.symbol();
        let layer = symbol.timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        let el = key.elements.iter().find(|e| e.id == id)?;
        match &el.kind {
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => Some(id),
            _ => None,
        }
    }

    /// Return the active `ElementAppearance` for the selected vector element (`T-233`).
    pub fn selected_element_appearance(&self) -> Option<ElementAppearance> {
        let id = self.selected_vector_element_id()?;
        let symbol = self.symbol();
        let layer = symbol.timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        if let Some(app) = key.appearance(&id) {
            return Some(app.clone());
        }
        let el = key.elements.iter().find(|e| e.id == id)?;
        Some(default_appearance_for_element(el))
    }

    /// Return the active `ElementVectorEffects` for the selected vector element (`T-233`).
    pub fn selected_element_vector_effects(&self) -> Option<ElementVectorEffects> {
        let id = self.selected_vector_element_id()?;
        let symbol = self.symbol();
        let layer = symbol.timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        if let Some(fx) = key.vector_effects(&id) {
            return Some(fx.clone());
        }
        Some(ElementVectorEffects {
            element_id: id,
            ..Default::default()
        })
    }

    fn mutate_selected_appearance(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&Element, &mut ElementAppearance) -> bool,
    ) -> bool {
        let Some(element_id) = self.selected_vector_element_id() else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut updated = false;
        self.transact_named(label, |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let Some(element_snapshot) = key.elements.iter().find(|e| e.id == element_id).cloned()
            else {
                return;
            };
            let mut app = key
                .appearance(&element_id)
                .cloned()
                .unwrap_or_else(|| default_appearance_for_element(&element_snapshot));
            if !mutate(&element_snapshot, &mut app) {
                return;
            }
            if app.entries.len() > 64 || app.modifiers.len() > 32 {
                return;
            }
            if let Some(el_mut) = key.elements.iter_mut().find(|e| e.id == element_id) {
                sync_primary_element_paints(el_mut, &app);
            }
            key.set_appearance(app);
            updated = true;
        });
        updated
    }

    /// Append a new fill paint entry to the selected element's `Appearance` stack in 1 Undo step (`T-233`).
    pub fn appearance_add_fill(&mut self, color: Color, offset: [f64; 2], opacity: f64) -> bool {
        let clamped_opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let clean_offset = [
            if offset[0].is_finite() {
                offset[0].clamp(-10_000.0, 10_000.0)
            } else {
                0.0
            },
            if offset[1].is_finite() {
                offset[1].clamp(-10_000.0, 10_000.0)
            } else {
                0.0
            },
        ];
        self.mutate_selected_appearance("Add appearance fill", |_el, app| {
            app.entries.push(AppearancePaintEntry::Fill {
                color,
                gradient: None,
                swatch_id: None,
                offset: clean_offset,
                opacity: clamped_opacity,
            });
            true
        })
    }

    /// Append a new stroke paint entry to the selected element's `Appearance` stack in 1 Undo step (`T-233`).
    pub fn appearance_add_stroke(
        &mut self,
        color: Color,
        width: f64,
        offset_distance: f64,
        opacity: f64,
    ) -> bool {
        let clean_width = if width.is_finite() {
            width.clamp(0.1, 500.0)
        } else {
            2.0
        };
        let clean_offset = if offset_distance.is_finite() {
            offset_distance.clamp(-10_000.0, 10_000.0)
        } else {
            0.0
        };
        let clamped_opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.mutate_selected_appearance("Add appearance stroke", |_el, app| {
            app.entries.push(AppearancePaintEntry::Stroke {
                color,
                gradient: None,
                width: clean_width,
                swatch_id: None,
                offset_distance: clean_offset,
                pattern_brush: None,
                opacity: clamped_opacity,
            });
            true
        })
    }

    /// Remove a fill/stroke entry at `index` from the selected element's `Appearance` stack in 1 Undo step (`T-233`).
    pub fn appearance_remove_entry(&mut self, index: usize) -> bool {
        self.mutate_selected_appearance("Remove appearance paint", |_el, app| {
            if index >= app.entries.len() {
                return false;
            }
            app.entries.remove(index);
            true
        })
    }

    /// Reorder a fill/stroke entry from `from_index` to `to_index` in 1 Undo step (`T-233`).
    pub fn appearance_move_entry(&mut self, from_index: usize, to_index: usize) -> bool {
        self.mutate_selected_appearance("Reorder appearance paint", |_el, app| {
            if from_index >= app.entries.len()
                || to_index >= app.entries.len()
                || from_index == to_index
            {
                return false;
            }
            let entry = app.entries.remove(from_index);
            app.entries.insert(to_index, entry);
            true
        })
    }

    /// Toggle visibility (`opacity` between `0.0` and `1.0`) of an `Appearance` paint entry in 1 Undo step (`T-233`).
    pub fn appearance_toggle_entry_visibility(&mut self, index: usize) -> bool {
        self.mutate_selected_appearance("Toggle appearance paint visibility", |_el, app| {
            let Some(entry) = app.entries.get_mut(index) else {
                return false;
            };
            match entry {
                AppearancePaintEntry::Fill { opacity, .. }
                | AppearancePaintEntry::Stroke { opacity, .. } => {
                    *opacity = if *opacity > 0.0 { 0.0 } else { 1.0 };
                }
            }
            true
        })
    }

    /// Set the color of an `Appearance` paint entry at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_entry_color(&mut self, index: usize, color: Color) -> bool {
        self.mutate_selected_appearance("Set appearance paint color", |_el, app| {
            let Some(entry) = app.entries.get_mut(index) else {
                return false;
            };
            match entry {
                AppearancePaintEntry::Fill { color: c, .. }
                | AppearancePaintEntry::Stroke { color: c, .. } => {
                    if *c == color {
                        return false;
                    }
                    *c = color;
                    true
                }
            }
        })
    }

    /// Set the opacity (`0.0..=1.0`) of an `Appearance` paint entry at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_entry_opacity(&mut self, index: usize, opacity: f64) -> bool {
        if !opacity.is_finite() {
            return false;
        }
        let clamped = opacity.clamp(0.0, 1.0);
        self.mutate_selected_appearance("Set appearance paint opacity", |_el, app| {
            let Some(entry) = app.entries.get_mut(index) else {
                return false;
            };
            match entry {
                AppearancePaintEntry::Fill { opacity: op, .. }
                | AppearancePaintEntry::Stroke { opacity: op, .. } => {
                    if (*op - clamped).abs() <= 1e-6 {
                        return false;
                    }
                    *op = clamped;
                    true
                }
            }
        })
    }

    /// Update stroke width and offset distance on a stroke entry at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_stroke_entry_width(
        &mut self,
        index: usize,
        width: f64,
        offset_distance: f64,
    ) -> bool {
        if !width.is_finite() || !offset_distance.is_finite() {
            return false;
        }
        let clean_w = width.clamp(0.1, 500.0);
        let clean_off = offset_distance.clamp(-10_000.0, 10_000.0);
        self.mutate_selected_appearance("Update appearance stroke", |_el, app| {
            let Some(AppearancePaintEntry::Stroke {
                width: w,
                offset_distance: off,
                ..
            }) = app.entries.get_mut(index)
            else {
                return false;
            };
            if (*w - clean_w).abs() <= 1e-6 && (*off - clean_off).abs() <= 1e-6 {
                return false;
            }
            *w = clean_w;
            *off = clean_off;
            true
        })
    }

    /// Update fill offset `[dx, dy]` on a fill entry at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_fill_entry_offset(&mut self, index: usize, offset: [f64; 2]) -> bool {
        if !offset[0].is_finite() || !offset[1].is_finite() {
            return false;
        }
        let clean = [
            offset[0].clamp(-10_000.0, 10_000.0),
            offset[1].clamp(-10_000.0, 10_000.0),
        ];
        self.mutate_selected_appearance("Update appearance fill offset", |_el, app| {
            let Some(AppearancePaintEntry::Fill { offset: off, .. }) = app.entries.get_mut(index)
            else {
                return false;
            };
            if (*off == clean)
                || ((off[0] - clean[0]).abs() <= 1e-6 && (off[1] - clean[1]).abs() <= 1e-6)
            {
                return false;
            }
            *off = clean;
            true
        })
    }

    /// Append a `VectorModifier` (`OffsetContour`, `LiveCorners`, `Repeater`, `Mirror`) in 1 Undo step (`T-233`).
    pub fn appearance_add_modifier(&mut self, modifier: VectorModifier) -> bool {
        let clean = sanitize_modifier(modifier);
        self.mutate_selected_appearance("Add path modifier", |_el, app| {
            app.modifiers.push(clean);
            true
        })
    }

    /// Update an existing `VectorModifier` at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_modifier(&mut self, index: usize, modifier: VectorModifier) -> bool {
        let clean = sanitize_modifier(modifier);
        self.mutate_selected_appearance("Edit path modifier", |_el, app| {
            let Some(slot) = app.modifiers.get_mut(index) else {
                return false;
            };
            if *slot == clean {
                return false;
            }
            *slot = clean;
            true
        })
    }

    /// Remove a `VectorModifier` at `index` in 1 Undo step (`T-233`).
    pub fn appearance_remove_modifier(&mut self, index: usize) -> bool {
        self.mutate_selected_appearance("Remove path modifier", |_el, app| {
            if index >= app.modifiers.len() {
                return false;
            }
            app.modifiers.remove(index);
            true
        })
    }

    /// Reorder a `VectorModifier` from `from_index` to `to_index` in 1 Undo step (`T-233`).
    pub fn appearance_move_modifier(&mut self, from_index: usize, to_index: usize) -> bool {
        self.mutate_selected_appearance("Reorder path modifier", |_el, app| {
            if from_index >= app.modifiers.len()
                || to_index >= app.modifiers.len()
                || from_index == to_index
            {
                return false;
            }
            let item = app.modifiers.remove(from_index);
            app.modifiers.insert(to_index, item);
            true
        })
    }

    fn mutate_selected_vector_effects(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut ElementVectorEffects) -> bool,
    ) -> bool {
        let Some(element_id) = self.selected_vector_element_id() else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut updated = false;
        self.transact_named(label, |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let mut fx = key
                .vector_effects(&element_id)
                .cloned()
                .unwrap_or(ElementVectorEffects {
                    element_id: element_id.clone(),
                    ..Default::default()
                });
            if !mutate(&mut fx) {
                return;
            }
            key.set_vector_effects(fx);
            updated = true;
        });
        updated
    }

    /// Add a procedural vector effect (`ZigZag`, `OffsetPath`, `PuckerBloat`, `Roughen`, `Twist`) in 1 Undo step (`T-233`).
    pub fn appearance_add_procedural_effect(&mut self, effect: ProceduralVectorEffect) -> bool {
        self.mutate_selected_vector_effects("Add vector effect", |fx| {
            fx.procedural_effects.push(effect);
            true
        })
    }

    /// Update a procedural vector effect at `index` in 1 Undo step (`T-233`).
    pub fn appearance_set_procedural_effect(
        &mut self,
        index: usize,
        effect: ProceduralVectorEffect,
    ) -> bool {
        self.mutate_selected_vector_effects("Edit vector effect", |fx| {
            let Some(slot) = fx.procedural_effects.get_mut(index) else {
                return false;
            };
            if *slot == effect {
                return false;
            }
            *slot = effect;
            true
        })
    }

    /// Remove a procedural vector effect at `index` in 1 Undo step (`T-233`).
    pub fn appearance_remove_procedural_effect(&mut self, index: usize) -> bool {
        self.mutate_selected_vector_effects("Remove vector effect", |fx| {
            if index >= fx.procedural_effects.len() {
                return false;
            }
            fx.procedural_effects.remove(index);
            true
        })
    }

    /// Expand live `Appearance` entries, path modifiers, and procedural vector effects into baked
    /// Stage geometry in 1 Undo step (`T-233`).
    pub fn appearance_expand_selected(&mut self) -> bool {
        let Some(element_id) = self.selected_vector_element_id() else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut expanded = false;
        self.transact_named("Expand appearance", |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let expanded_fx = key.expand_element_vector_effects(&element_id);
            let expanded_app = !key.expand_element_appearance(&element_id).is_empty();
            expanded = expanded_fx || expanded_app;
        });
        expanded
    }

    /// Save the selected element's appearance as a reusable `VectorStyle` (Graphic Style) and bind
    /// `style_id` to the element in 1 Undo step (`T-233`).
    pub fn appearance_save_graphic_style(&mut self, name: &str) -> Option<Id> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let element_id = self.selected_vector_element_id()?;
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut saved_id: Option<Id> = None;
        self.transact_named("Save Graphic Style", |editor| {
            let mut styles = editor.project.vector_styles();
            let style_id = format!("style-{}", styles.len() + 1);
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let Some(el) = key.elements.iter().find(|e| e.id == element_id).cloned() else {
                return;
            };
            let mut app = key
                .appearance(&element_id)
                .cloned()
                .unwrap_or_else(|| default_appearance_for_element(&el));
            app.style_id = Some(style_id.clone());
            let (fill, stroke, stroke_width) = match &el.kind {
                ElementKind::Rectangle {
                    fill,
                    stroke,
                    stroke_width,
                    ..
                }
                | ElementKind::Ellipse {
                    fill,
                    stroke,
                    stroke_width,
                    ..
                }
                | ElementKind::Path {
                    fill,
                    stroke,
                    stroke_width,
                    ..
                }
                | ElementKind::PaintedPath {
                    fill,
                    stroke,
                    stroke_width,
                    ..
                } => (*fill, *stroke, *stroke_width),
                _ => ([0, 0, 0, 255], [0, 0, 0, 255], 1.0),
            };
            let new_style = VectorStyle {
                id: style_id.clone(),
                name: trimmed.to_string(),
                fill,
                stroke,
                stroke_width,
                modifiers: app.modifiers.clone(),
            };
            key.set_appearance(app);
            if let Some(existing) = styles.iter_mut().find(|s| s.id == style_id) {
                *existing = new_style;
            } else {
                styles.push(new_style);
            }
            editor.project.set_vector_styles(&styles);
            saved_id = Some(style_id);
        });
        saved_id
    }

    /// Apply a saved `VectorStyle` (Graphic Style) to the selected element and propagate across
    /// linked project elements in 1 Undo step (`T-233`).
    pub fn appearance_apply_graphic_style(&mut self, style_id: &str) -> bool {
        let Some(style) = self
            .project
            .vector_styles()
            .into_iter()
            .find(|s| s.id == style_id)
        else {
            return false;
        };
        let Some(element_id) = self.selected_vector_element_id() else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut applied = false;
        self.transact_named("Apply Graphic Style", |editor| {
            let symbol = editor.symbol_mut();
            if let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) {
                let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
                if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)
                    && let Some(el) = key.elements.iter().find(|e| e.id == element_id).cloned()
                {
                    let mut app = key
                        .appearance(&element_id)
                        .cloned()
                        .unwrap_or_else(|| default_appearance_for_element(&el));
                    app.style_id = Some(style.id.clone());
                    app.modifiers = style.modifiers.clone();
                    app.entries = vec![
                        AppearancePaintEntry::Fill {
                            color: style.fill,
                            gradient: None,
                            swatch_id: None,
                            offset: [0.0, 0.0],
                            opacity: 1.0,
                        },
                        AppearancePaintEntry::Stroke {
                            color: style.stroke,
                            gradient: None,
                            width: style.stroke_width,
                            swatch_id: None,
                            offset_distance: 0.0,
                            pattern_brush: None,
                            opacity: 1.0,
                        },
                    ];
                    key.set_appearance(app);
                    applied = true;
                }
            }
            if applied {
                editor.project.apply_vector_style_to_project(&style.id);
            }
        });
        applied
    }

    /// Update an existing `VectorStyle` definition and propagate it to all bound elements in 1 Undo step (`T-233`).
    pub fn appearance_update_graphic_style(&mut self, updated_style: VectorStyle) -> usize {
        let mut count = 0usize;
        self.transact_named("Update Graphic Style", |editor| {
            let mut styles = editor.project.vector_styles();
            if let Some(slot) = styles.iter_mut().find(|s| s.id == updated_style.id) {
                *slot = updated_style.clone();
            } else {
                styles.push(updated_style.clone());
            }
            editor.project.set_vector_styles(&styles);
            count = editor
                .project
                .apply_vector_style_to_project(&updated_style.id);
        });
        count
    }

    /// Unlink the selected element from its bound `VectorStyle` in 1 Undo step (`T-233`).
    pub fn appearance_unlink_graphic_style(&mut self) -> bool {
        self.mutate_selected_appearance("Unlink Graphic Style", |_el, app| {
            if app.style_id.is_none() {
                return false;
            }
            app.style_id = None;
            true
        })
    }

    /// Delete a `VectorStyle` from the project and unlink any elements referencing it in 1 Undo step (`T-233`).
    pub fn appearance_delete_graphic_style(&mut self, style_id: &str) -> bool {
        let styles = self.project.vector_styles();
        if !styles.iter().any(|s| s.id == style_id) {
            return false;
        }
        let target_id = style_id.to_string();
        self.transact_named("Delete Graphic Style", |editor| {
            let remaining: Vec<VectorStyle> = editor
                .project
                .vector_styles()
                .into_iter()
                .filter(|s| s.id != target_id)
                .collect();
            editor.project.set_vector_styles(&remaining);
            for symbol in &mut editor.project.symbols {
                for layer in &mut symbol.timeline.layers {
                    for key in &mut layer.keyframes {
                        for app in &mut key.appearances {
                            if app.style_id.as_deref() == Some(target_id.as_str()) {
                                app.style_id = None;
                            }
                        }
                    }
                }
            }
        });
        true
    }

    /// Alias for [`Self::appearance_expand_selected`] (`T-264`).
    pub fn expand_selected_shape_appearance(&mut self) -> bool {
        self.appearance_expand_selected()
    }

    /// Render the `Appearance` inspector panel (`T-233` / `T-264`).
    pub fn appearance_panel_ui(&mut self, ui: &mut egui::Ui) {
        let Some(appearance) = self.selected_element_appearance() else {
            ui.label("Select a vector shape on Stage to edit multi-paint fills, strokes, modifiers, and Graphic Styles.");
            return;
        };
        let vector_effects = self.selected_element_vector_effects().unwrap_or_default();

        // 1. Graphic Styles section
        let styles = self.project.vector_styles();
        let active_style_name = appearance
            .style_id
            .as_deref()
            .and_then(|id| styles.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "None".to_string());
        nf::SectionHeader::new("Graphic Styles")
            .id_salt("appearance_graphic_style_sec")
            .show(ui, |ui| {
                let mut chosen_action: Option<Option<String>> = None;
                nf::Select::new("appearance_graphic_style_select", &active_style_name).show(
                    ui,
                    |ui| {
                        if ui
                            .selectable_label(appearance.style_id.is_none(), "None")
                            .clicked()
                        {
                            chosen_action = Some(None);
                        }
                        for s in &styles {
                            if ui
                                .selectable_label(
                                    appearance.style_id.as_deref() == Some(s.id.as_str()),
                                    &s.name,
                                )
                                .clicked()
                            {
                                chosen_action = Some(Some(s.id.clone()));
                            }
                        }
                    },
                );
                if let Some(choice) = chosen_action {
                    match choice {
                        None => {
                            let _ = self.appearance_unlink_graphic_style();
                        }
                        Some(chosen_id) => {
                            let _ = self.appearance_apply_graphic_style(&chosen_id);
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if ui.add(nf::Button::new("Save Style")).clicked() {
                        let next_name = format!("Style {}", styles.len() + 1);
                        let _ = self.appearance_save_graphic_style(&next_name);
                    }
                    if let Some(style_id) = appearance.style_id.as_deref() {
                        if ui.add(nf::Button::ghost("Unlink")).clicked() {
                            let _ = self.appearance_unlink_graphic_style();
                        }
                        if ui.add(nf::Button::ghost("Delete")).clicked() {
                            let id_owned = style_id.to_string();
                            let _ = self.appearance_delete_graphic_style(&id_owned);
                        }
                    }
                });
            });

        nf::divider(ui, false);

        // 2. Multi-Paint Fills & Strokes Stack
        nf::SectionHeader::new("Appearance")
            .id_salt("appearance_fills_strokes_sec")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(nf::Button::new("+ Fill")).clicked() {
                        let _ = self.appearance_add_fill(self.fill, [0.0, 0.0], 1.0);
                    }
                    if ui.add(nf::Button::new("+ Stroke")).clicked() {
                        let _ = self.appearance_add_stroke(
                            self.stroke,
                            self.stroke_width.max(1.0),
                            0.0,
                            1.0,
                        );
                    }
                    if ui.add(nf::Button::ghost("Expand Appearance")).clicked() {
                        let _ = self.expand_selected_shape_appearance();
                    }
                });

                let entries_len = appearance.entries.len();
                for (idx, entry) in appearance.entries.iter().enumerate() {
                    ui.push_id(("appearance_entry", idx), |ui| match entry {
                        AppearancePaintEntry::Fill {
                            offset, opacity, ..
                        } => {
                            let mut visible = *opacity > 0.0;
                            let mut edit_opacity_pct = (*opacity * 100.0).round();
                            let mut off_x = offset[0];
                            let mut off_y = offset[1];
                            ui.horizontal(|ui| {
                                if ui.add(nf::Checkbox::new(&mut visible, "Fill")).changed() {
                                    let _ = self.appearance_toggle_entry_visibility(idx);
                                }
                                if idx > 0 && ui.add(nf::Button::ghost("↑")).clicked() {
                                    let _ = self.appearance_move_entry(idx, idx - 1);
                                }
                                if idx + 1 < entries_len && ui.add(nf::Button::ghost("↓")).clicked()
                                {
                                    let _ = self.appearance_move_entry(idx, idx + 1);
                                }
                                if ui.add(nf::Button::ghost("Remove")).clicked() {
                                    let _ = self.appearance_remove_entry(idx);
                                }
                            });
                            if ui
                                .add(
                                    nf::Field::new(&mut edit_opacity_pct, "Opacity")
                                        .unit("%")
                                        .range(0.0..=100.0),
                                )
                                .changed()
                            {
                                let _ = self
                                    .appearance_set_entry_opacity(idx, edit_opacity_pct / 100.0);
                            }
                            let ox_changed = ui
                                .add(
                                    nf::Field::new(&mut off_x, "dX")
                                        .unit("px")
                                        .range(-1000.0..=1000.0),
                                )
                                .changed();
                            let oy_changed = ui
                                .add(
                                    nf::Field::new(&mut off_y, "dY")
                                        .unit("px")
                                        .range(-1000.0..=1000.0),
                                )
                                .changed();
                            if ox_changed || oy_changed {
                                let _ = self.appearance_set_fill_entry_offset(idx, [off_x, off_y]);
                            }
                        }
                        AppearancePaintEntry::Stroke {
                            width,
                            offset_distance,
                            opacity,
                            ..
                        } => {
                            let mut visible = *opacity > 0.0;
                            let mut edit_w = *width;
                            let mut edit_off = *offset_distance;
                            let mut edit_opacity_pct = (*opacity * 100.0).round();
                            ui.horizontal(|ui| {
                                if ui.add(nf::Checkbox::new(&mut visible, "Stroke")).changed() {
                                    let _ = self.appearance_toggle_entry_visibility(idx);
                                }
                                if idx > 0 && ui.add(nf::Button::ghost("↑")).clicked() {
                                    let _ = self.appearance_move_entry(idx, idx - 1);
                                }
                                if idx + 1 < entries_len && ui.add(nf::Button::ghost("↓")).clicked()
                                {
                                    let _ = self.appearance_move_entry(idx, idx + 1);
                                }
                                if ui.add(nf::Button::ghost("Remove")).clicked() {
                                    let _ = self.appearance_remove_entry(idx);
                                }
                            });
                            let w_changed = ui
                                .add(
                                    nf::Field::new(&mut edit_w, "W")
                                        .unit("px")
                                        .range(0.1..=200.0),
                                )
                                .changed();
                            let off_changed = ui
                                .add(
                                    nf::Field::new(&mut edit_off, "Offset")
                                        .unit("px")
                                        .range(-500.0..=500.0),
                                )
                                .changed();
                            if w_changed || off_changed {
                                let _ =
                                    self.appearance_set_stroke_entry_width(idx, edit_w, edit_off);
                            }
                            if ui
                                .add(
                                    nf::Field::new(&mut edit_opacity_pct, "Opacity")
                                        .unit("%")
                                        .range(0.0..=100.0),
                                )
                                .changed()
                            {
                                let _ = self
                                    .appearance_set_entry_opacity(idx, edit_opacity_pct / 100.0);
                            }
                        }
                    });
                }
            });

        nf::divider(ui, false);

        // 3. Path Modifiers (`Offset Paths`, `Round Corners`, `Repeater`, `Mirror`)
        nf::SectionHeader::new("Path modifiers")
            .id_salt("appearance_path_modifiers_sec")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for kind in AppearanceModifierKind::ALL {
                        if ui.add(nf::Button::new(kind.label())).clicked() {
                            let _ = self.appearance_add_modifier(kind.default_modifier());
                        }
                    }
                });
                let mod_len = appearance.modifiers.len();
                for (midx, modifier) in appearance.modifiers.iter().enumerate() {
                    ui.push_id(("appearance_modifier", midx), |ui| match modifier {
                        VectorModifier::OffsetContour { delta } => {
                            let mut edit_delta = *delta;
                            if ui
                                .add(
                                    nf::Field::new(&mut edit_delta, "Offset")
                                        .unit("px")
                                        .range(-500.0..=500.0),
                                )
                                .changed()
                            {
                                let _ = self.appearance_set_modifier(
                                    midx,
                                    VectorModifier::OffsetContour { delta: edit_delta },
                                );
                            }
                            ui.horizontal(|ui| {
                                if midx > 0 && ui.add(nf::Button::ghost("↑")).clicked() {
                                    let _ = self.appearance_move_modifier(midx, midx - 1);
                                }
                                if midx + 1 < mod_len && ui.add(nf::Button::ghost("↓")).clicked()
                                {
                                    let _ = self.appearance_move_modifier(midx, midx + 1);
                                }
                                if ui.add(nf::Button::ghost("Remove")).clicked() {
                                    let _ = self.appearance_remove_modifier(midx);
                                }
                            });
                        }
                        VectorModifier::LiveCorners { radius } => {
                            let mut edit_radius = *radius;
                            if ui
                                .add(
                                    nf::Field::new(&mut edit_radius, "Radius")
                                        .unit("px")
                                        .range(0.0..=500.0),
                                )
                                .changed()
                            {
                                let _ = self.appearance_set_modifier(
                                    midx,
                                    VectorModifier::LiveCorners {
                                        radius: edit_radius,
                                    },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_modifier(midx);
                            }
                        }
                        VectorModifier::Repeater {
                            copies,
                            offset_x,
                            offset_y,
                            scale_step,
                            rotation_step,
                        } => {
                            let mut c = *copies as f64;
                            let mut ox = *offset_x;
                            let mut oy = *offset_y;
                            let mut sc = *scale_step;
                            let mut rot = *rotation_step;
                            let c_ch = ui
                                .add(nf::Field::new(&mut c, "Copies").range(1.0..=64.0))
                                .changed();
                            let ox_ch = ui
                                .add(
                                    nf::Field::new(&mut ox, "dX")
                                        .unit("px")
                                        .range(-1000.0..=1000.0),
                                )
                                .changed();
                            let oy_ch = ui
                                .add(
                                    nf::Field::new(&mut oy, "dY")
                                        .unit("px")
                                        .range(-1000.0..=1000.0),
                                )
                                .changed();
                            let sc_ch = ui
                                .add(nf::Field::new(&mut sc, "Scale").range(0.1..=5.0))
                                .changed();
                            let rot_ch = ui
                                .add(
                                    nf::Field::new(&mut rot, "Rot")
                                        .unit("°")
                                        .range(-360.0..=360.0),
                                )
                                .changed();
                            if c_ch || ox_ch || oy_ch || sc_ch || rot_ch {
                                let _ = self.appearance_set_modifier(
                                    midx,
                                    VectorModifier::Repeater {
                                        copies: c.round().clamp(1.0, 64.0) as u32,
                                        offset_x: ox,
                                        offset_y: oy,
                                        scale_step: sc,
                                        rotation_step: rot,
                                    },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_modifier(midx);
                            }
                        }
                        VectorModifier::Mirror { axis_x, axis_y } => {
                            let mut ax = axis_x.unwrap_or(0.0);
                            let mut ay = axis_y.unwrap_or(0.0);
                            let mut use_x = axis_x.is_some();
                            let mut use_y = axis_y.is_some();
                            ui.horizontal(|ui| {
                                let ux_ch =
                                    ui.add(nf::Checkbox::new(&mut use_x, "Mirror X")).changed();
                                let uy_ch =
                                    ui.add(nf::Checkbox::new(&mut use_y, "Mirror Y")).changed();
                                if ux_ch || uy_ch {
                                    let _ = self.appearance_set_modifier(
                                        midx,
                                        VectorModifier::Mirror {
                                            axis_x: use_x.then_some(ax),
                                            axis_y: use_y.then_some(ay),
                                        },
                                    );
                                }
                                if ui.add(nf::Button::ghost("Remove")).clicked() {
                                    let _ = self.appearance_remove_modifier(midx);
                                }
                            });
                            let ax_ch = ui
                                .add(
                                    nf::Field::new(&mut ax, "X")
                                        .unit("px")
                                        .range(-2000.0..=2000.0),
                                )
                                .changed();
                            let ay_ch = ui
                                .add(
                                    nf::Field::new(&mut ay, "Y")
                                        .unit("px")
                                        .range(-2000.0..=2000.0),
                                )
                                .changed();
                            if ax_ch || ay_ch {
                                let _ = self.appearance_set_modifier(
                                    midx,
                                    VectorModifier::Mirror {
                                        axis_x: use_x.then_some(ax),
                                        axis_y: use_y.then_some(ay),
                                    },
                                );
                            }
                        }
                    });
                }
            });

        nf::divider(ui, false);

        // 4. Procedural Vector Effects (`Zig Zag`, `Offset Path`, `Pucker & Bloat`, `Roughen`, `Twist`)
        nf::SectionHeader::new("Vector effects")
            .id_salt("appearance_vector_effects_sec")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for kind in AppearanceProceduralKind::ALL {
                        if ui.add(nf::Button::new(kind.label())).clicked() {
                            let _ = self.appearance_add_procedural_effect(kind.default_effect());
                        }
                    }
                });
                for (pidx, effect) in vector_effects.procedural_effects.iter().enumerate() {
                    ui.push_id(("appearance_proc_effect", pidx), |ui| match effect {
                        ProceduralVectorEffect::ZigZag {
                            size,
                            ridges_per_segment,
                            smooth,
                        } => {
                            let mut sz = *size;
                            let mut ridges = *ridges_per_segment as f64;
                            let mut sm = *smooth;
                            let s_ch = ui
                                .add(
                                    nf::Field::new(&mut sz, "ZigZag")
                                        .unit("px")
                                        .range(0.0..=200.0),
                                )
                                .changed();
                            let r_ch = ui
                                .add(nf::Field::new(&mut ridges, "Ridges").range(1.0..=32.0))
                                .changed();
                            let mut sm_ch = false;
                            ui.horizontal(|ui| {
                                sm_ch = ui.add(nf::Checkbox::new(&mut sm, "Smooth")).changed();
                                if ui.add(nf::Button::ghost("Remove")).clicked() {
                                    let _ = self.appearance_remove_procedural_effect(pidx);
                                }
                            });
                            if s_ch || r_ch || sm_ch {
                                let _ = self.appearance_set_procedural_effect(
                                    pidx,
                                    ProceduralVectorEffect::ZigZag {
                                        size: sz,
                                        ridges_per_segment: ridges.round().clamp(1.0, 32.0) as u32,
                                        smooth: sm,
                                    },
                                );
                            }
                        }
                        ProceduralVectorEffect::OffsetPath { offset } => {
                            let mut off = *offset;
                            if ui
                                .add(
                                    nf::Field::new(&mut off, "Offset Path")
                                        .unit("px")
                                        .range(-500.0..=500.0),
                                )
                                .changed()
                            {
                                let _ = self.appearance_set_procedural_effect(
                                    pidx,
                                    ProceduralVectorEffect::OffsetPath { offset: off },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_procedural_effect(pidx);
                            }
                        }
                        ProceduralVectorEffect::PuckerBloat { amount } => {
                            let mut amt = *amount;
                            if ui
                                .add(
                                    nf::Field::new(&mut amt, "Pucker/Bloat")
                                        .unit("%")
                                        .range(-200.0..=200.0),
                                )
                                .changed()
                            {
                                let _ = self.appearance_set_procedural_effect(
                                    pidx,
                                    ProceduralVectorEffect::PuckerBloat { amount: amt },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_procedural_effect(pidx);
                            }
                        }
                        ProceduralVectorEffect::Roughen { size, detail, seed } => {
                            let mut sz = *size;
                            let mut det = *detail as f64;
                            let mut sd = *seed as f64;
                            let s_ch = ui
                                .add(
                                    nf::Field::new(&mut sz, "Roughen")
                                        .unit("px")
                                        .range(0.0..=200.0),
                                )
                                .changed();
                            let d_ch = ui
                                .add(nf::Field::new(&mut det, "Detail").range(1.0..=32.0))
                                .changed();
                            let sd_ch = ui
                                .add(nf::Field::new(&mut sd, "Seed").range(0.0..=9999.0))
                                .changed();
                            if s_ch || d_ch || sd_ch {
                                let _ = self.appearance_set_procedural_effect(
                                    pidx,
                                    ProceduralVectorEffect::Roughen {
                                        size: sz,
                                        detail: det.round().clamp(1.0, 32.0) as u32,
                                        seed: sd.round().max(0.0) as u32,
                                    },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_procedural_effect(pidx);
                            }
                        }
                        ProceduralVectorEffect::Twist { angle_deg } => {
                            let mut ang = *angle_deg;
                            if ui
                                .add(
                                    nf::Field::new(&mut ang, "Twist")
                                        .unit("°")
                                        .range(-720.0..=720.0),
                                )
                                .changed()
                            {
                                let _ = self.appearance_set_procedural_effect(
                                    pidx,
                                    ProceduralVectorEffect::Twist { angle_deg: ang },
                                );
                            }
                            if ui.add(nf::Button::ghost("Remove")).clicked() {
                                let _ = self.appearance_remove_procedural_effect(pidx);
                            }
                        }
                    });
                }
            });
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;

    fn editor_with_selected_rect() -> Editor {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let el = Element {
            id: "rect-1".into(),
            name: "Rect".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [200, 60, 40, 255],
                stroke: [20, 30, 40, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        editor.project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(el);
        editor.select_single_id("rect-1".into());
        editor
    }

    #[test]
    fn appearance_multi_paint_stack_add_reorder_toggle_and_undo_round_trip() {
        let mut editor = editor_with_selected_rect();

        // Add an extra fill and stroke
        assert!(editor.appearance_add_fill([10, 180, 90, 255], [4.0, -4.0], 0.85));
        assert!(editor.appearance_add_stroke([250, 220, 40, 255], 6.0, 2.0, 1.0));
        let app = editor.selected_element_appearance().expect("appearance");
        assert_eq!(app.entries.len(), 4);

        // Reorder entry 3 to index 0 (1 Undo step)
        assert!(editor.appearance_move_entry(3, 0));
        let app = editor.selected_element_appearance().expect("appearance");
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Stroke { width: 6.0, .. }
        ));

        // Toggle visibility of entry 0 (1 Undo step)
        assert!(editor.appearance_toggle_entry_visibility(0));
        let app = editor.selected_element_appearance().expect("appearance");
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Stroke { opacity: 0.0, .. }
        ));

        // Undo visibility toggle -> opacity back to 1.0
        editor.undo();
        let app = editor.selected_element_appearance().expect("appearance");
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Stroke { opacity: 1.0, .. }
        ));

        // Save/reopen round-trip and validate
        editor.project.validate().expect("valid project");
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("reopened valid");
        let reopened_app = reopened.symbols[0].timeline.layers[0].keyframes[0]
            .appearance("rect-1")
            .expect("persisted appearance");
        assert_eq!(reopened_app.entries.len(), 4);
    }

    #[test]
    fn appearance_modifiers_repeater_offset_round_corners_zigzag_and_expand() {
        let mut editor = editor_with_selected_rect();

        assert!(editor.appearance_add_modifier(VectorModifier::LiveCorners { radius: 10.0 }));
        assert!(editor.appearance_add_modifier(VectorModifier::OffsetContour { delta: 4.0 }));
        assert!(editor.appearance_add_modifier(VectorModifier::Repeater {
            copies: 3,
            offset_x: 30.0,
            offset_y: 10.0,
            scale_step: 0.9,
            rotation_step: 15.0,
        }));
        assert!(
            editor.appearance_add_procedural_effect(ProceduralVectorEffect::ZigZag {
                size: 5.0,
                ridges_per_segment: 3,
                smooth: true,
            })
        );

        let app = editor.selected_element_appearance().expect("appearance");
        assert_eq!(app.modifiers.len(), 3);
        let fx = editor
            .selected_element_vector_effects()
            .expect("vector effects");
        assert_eq!(fx.procedural_effects.len(), 1);

        // Save/reopen before expand
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("valid");

        // Expand appearance + vector effects in 1 Undo step
        assert!(editor.appearance_expand_selected());
        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        assert!(key.appearance("rect-1").is_none());
        assert!(key.vector_effects("rect-1").is_none());
        assert!(key.elements.len() >= 2);
        editor.project.validate().expect("valid after expand");

        // 1-step Undo restores unexpanded live modifiers and single element
        editor.undo();
        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        assert_eq!(key.elements.len(), 1);
        assert!(key.appearance("rect-1").is_some());
        assert!(key.vector_effects("rect-1").is_some());
    }

    #[test]
    fn appearance_graphic_styles_save_apply_update_and_validate() {
        let mut editor = editor_with_selected_rect();
        // Add a second shape
        editor.project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "rect-2".into(),
                name: "Rect 2".into(),
                transform: Transform {
                    x: 160.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 60.0,
                    fill: [50, 50, 50, 255],
                    stroke: [0, 0, 0, 255],
                    stroke_width: 1.0,
                },
                ..Element::default()
            });

        assert!(editor.appearance_add_modifier(VectorModifier::OffsetContour { delta: 6.0 }));
        let style_id = editor
            .appearance_save_graphic_style("Neon Card")
            .expect("saved style id");
        assert_eq!(editor.project.vector_styles().len(), 1);

        // Select second shape and apply style
        editor.select_single_id("rect-2".into());
        assert!(editor.appearance_apply_graphic_style(&style_id));
        let app2 = editor
            .selected_element_appearance()
            .expect("rect-2 appearance");
        assert_eq!(app2.style_id.as_deref(), Some(style_id.as_str()));
        assert_eq!(
            app2.modifiers,
            vec![VectorModifier::OffsetContour { delta: 6.0 }]
        );

        // Update style definition -> propagates to both rect-1 and rect-2 in 1 Undo step
        let updated = VectorStyle {
            id: style_id.clone(),
            name: "Neon Card v2".into(),
            fill: [20, 220, 180, 255],
            stroke: [255, 255, 255, 255],
            stroke_width: 4.0,
            modifiers: vec![VectorModifier::LiveCorners { radius: 12.0 }],
        };
        assert_eq!(editor.appearance_update_graphic_style(updated), 2);
        editor.project.validate().expect("valid project");

        // Undo style update restores previous modifiers on both shapes
        editor.undo();
        let app2_after_undo = editor
            .selected_element_appearance()
            .expect("rect-2 appearance after undo");
        assert_eq!(
            app2_after_undo.modifiers,
            vec![VectorModifier::OffsetContour { delta: 6.0 }]
        );
    }

    fn collect_shape_texts(shapes: &[egui::epaint::Shape], out: &mut Vec<String>) {
        for shape in shapes {
            match shape {
                egui::epaint::Shape::Text(text_shape) => {
                    out.push(text_shape.galley.text().to_owned());
                }
                egui::epaint::Shape::Vec(nested) => collect_shape_texts(nested, out),
                _ => {}
            }
        }
    }

    #[test]
    fn appearance_live_properties_panel_renders_controls_and_command_id_expands() {
        let mut editor = editor_with_selected_rect();
        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, 1200.0),
            )),
            ..egui::RawInput::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.properties(ui, ctx);
            });
        });
        let top_shapes: Vec<egui::epaint::Shape> =
            output.shapes.into_iter().map(|cs| cs.shape).collect();
        let mut texts = Vec::new();
        collect_shape_texts(&top_shapes, &mut texts);
        assert!(
            texts
                .iter()
                .any(|t| t.eq_ignore_ascii_case("Graphic Styles")),
            "expected 'Graphic Styles' in live Properties panel, got: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.eq_ignore_ascii_case("Appearance")),
            "expected 'Appearance' in live Properties panel, got: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Expand Appearance")),
            "expected 'Expand Appearance' button in live Properties panel, got: {texts:?}"
        );

        // Execute CommandId::ExpandShapeAppearance via command router after adding a modifier
        assert!(editor.appearance_add_modifier(VectorModifier::OffsetContour { delta: 5.0 }));
        editor.execute_command(&ctx, crate::commands::CommandId::ExpandShapeAppearance);
        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        assert!(key.appearance("rect-1").is_none());
        assert!(key.elements.len() >= 2);
        editor
            .project
            .validate()
            .expect("valid after expand command");
    }

    #[test]
    fn t269_full_live_create_edit_save_reopen_and_publish_walkthrough() {
        use neoflash_core::{
            Accessibility, AccessibilityLivePoliteness, AccessibleRole, AuthoredPlayback,
            ContentAccessibleRole, ExtendedAccessibility, FrameScript, PreviewWorkArea, Script,
            ScriptLanguage, TimelineMarker,
        };

        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());

        // 1. Create a brand-new project in the Editor
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.project.name = "T269 Live Walkthrough".into();
        let kf0 = &mut editor.project.symbols[0].timeline.layers[0].keyframes[0];
        kf0.elements = vec![
            Element {
                id: "rect-hero".into(),
                name: "Hero Card".into(),
                transform: Transform {
                    x: 80.0,
                    y: 60.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 180.0,
                    height: 110.0,
                    fill: [40, 120, 240, 255],
                    stroke: [255, 255, 255, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            },
            Element {
                id: "cta-btn".into(),
                name: "CTA Label".into(),
                transform: Transform {
                    x: 100.0,
                    y: 90.0,
                    ..Transform::default()
                },
                kind: ElementKind::Text {
                    text: "Launch Demo".into(),
                    font_family: "Inter".into(),
                    font_size: 20.0,
                    color: [255, 255, 255, 255],
                },
                accessibility: Some(Accessibility {
                    name: "Launch Demo Button".into(),
                    description: "Starts the interactive demo".into(),
                    role: AccessibleRole::Button,
                    tab_order: 1,
                    disabled: false,
                }),
                ..Element::default()
            },
        ];
        kf0.extended_accessibility.push(ExtendedAccessibility {
            element_id: "cta-btn".into(),
            role: ContentAccessibleRole::Button,
            shortcut: "Enter".into(),
            live: AccessibilityLivePoliteness::Polite,
            ..ExtendedAccessibility::default()
        });
        editor.project.symbols[0].timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: concat!(
                    "var s = \"5\";\n",
                    "var strictEq = (s === 5);\n",
                    "var looseEq = (s == 5);\n",
                    "var nullUndefLoose = (null == undefined);\n",
                    "var nullUndefStrict = (null === undefined);\n",
                    "var cat = s + 1;\n",
                    "trace(\"E2E:\" + strictEq + \":\" + looseEq + \":\" + nullUndefLoose + \":\" + nullUndefStrict + \":\" + cat);\n"
                )
                .into(),
            },
        });

        // 2. Edit Appearance, Graphic Styles, Motion Wiggler, Work Area, Marker, and Stage A/B
        editor.select_single_id("rect-hero".into());
        assert!(editor.appearance_add_stroke([255, 200, 40, 255], 4.0, 2.0, 1.0));
        assert!(editor.appearance_add_modifier(VectorModifier::LiveCorners { radius: 8.0 }));
        let style_id = editor
            .appearance_save_graphic_style("Hero Style")
            .expect("saved graphic style");
        assert_eq!(editor.project.vector_styles().len(), 1);

        editor.seek_frame(0);
        editor.execute_command(&ctx, crate::commands::CommandId::CaptureStageA);

        // Create a motion tween span and apply Wiggler + Work Area + Marker
        let mut k10 = editor.project.symbols[0].timeline.layers[0].keyframes[0].clone();
        k10.frame = 10;
        k10.markers.clear();
        if let Some(el) = k10.elements.iter_mut().find(|e| e.id == "rect-hero") {
            el.transform.x = 280.0;
            el.transform.y = 140.0;
        }
        editor.project.symbols[0].timeline.layers[0]
            .keyframes
            .push(k10);

        editor.seek_frame(0);
        editor.select_single_id("rect-hero".into());
        editor.execute_command(&ctx, crate::commands::CommandId::ApplyMotionWiggler);
        editor.set_timeline_work_area(Some(PreviewWorkArea {
            in_frame: 0,
            out_frame: 10,
            enabled: true,
            skip_nth_frame: 0,
        }));
        editor.set_timeline_markers(vec![TimelineMarker {
            id: "marker-mid".into(),
            frame: 5,
            duration_frames: 1,
            name: "Midpoint".into(),
            comment: "Cue".into(),
            color_tag: Some("orange".into()),
            cue_point_parameters: std::collections::BTreeMap::new(),
        }]);
        editor.seek_frame(10);
        editor.execute_command(&ctx, crate::commands::CommandId::CaptureStageB);
        editor.execute_command(&ctx, crate::commands::CommandId::ToggleStageAbSplit);
        assert_eq!(
            editor.stage_ab_mode(),
            crate::stage::StageAbMode::SplitVertical
        );

        // Render live Properties + Stage panels in headless egui
        editor.select_single_id("rect-hero".into());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::SidePanel::right("props_e2e")
                .exact_width(380.0)
                .show(ctx, |ui| {
                    editor.properties(ui, ctx);
                });
            editor.stage(ctx);
        });

        // 3. Save project to a real .nflash file on disk
        let tmp_dir = std::env::temp_dir().join(format!("neoflash-t269-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).expect("create temp dir");
        let nflash_path = tmp_dir.join("walkthrough.nflash");
        let publish_dir = tmp_dir.join("published_html5");

        let json_bytes = editor.project.to_json().expect("serialize .nflash");
        std::fs::write(&nflash_path, &json_bytes).expect("write .nflash file");

        // 4. Reopen .nflash from disk into a fresh Editor instance
        let disk_json = std::fs::read_to_string(&nflash_path).expect("read .nflash file");
        let reopened_project = Project::from_json(&disk_json).expect("parse .nflash file");
        reopened_project.validate().expect("reopened project valid");
        let reopened_editor = Editor::from_project(&ctx, reopened_project);

        assert_eq!(reopened_editor.project.vector_styles().len(), 1);
        assert_eq!(reopened_editor.project.vector_styles()[0].id, style_id);
        let k0 = &reopened_editor.project.symbols[0].timeline.layers[0].keyframes[0];
        let hero_app = k0.appearance("rect-hero").expect("reopened appearance");
        assert_eq!(
            hero_app.modifiers,
            vec![VectorModifier::LiveCorners { radius: 8.0 }]
        );
        assert!(
            reopened_editor.project.symbols[0].timeline.layers[0]
                .keyframes
                .iter()
                .any(|k| k.keyed_properties.is_some()),
            "expected Wiggler diamond property keyframes after .nflash reopen"
        );
        assert!(
            reopened_editor.project.symbols[0]
                .timeline
                .work_area()
                .is_some_and(|wa| wa.enabled && wa.in_frame == 0 && wa.out_frame == 10)
        );

        // Verify Native Preview on the reopened project evaluates T-266 script semantics accurately
        let root_id = reopened_editor.project.root.clone();
        let mut preview =
            AuthoredPlayback::with_live_scripts(&reopened_editor.project, &root_id, 0, true);
        let trace_joined = preview.drain_traces().join("\n");
        assert!(
            trace_joined.contains("E2E:false:true:true:false:51"),
            "expected Native Preview trace 'E2E:false:true:true:false:51', got: {trace_joined:?}"
        );

        // 5. Publish the reopened project to HTML5 and verify generated bundle & #stage-a11y-overlay
        let runtime_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/web-runtime");
        neoflash_export::export_project(&reopened_editor.project, &publish_dir, &runtime_dir)
            .expect("publish reopened .nflash to HTML5");
        let html_path = publish_dir.join("index.html");
        let bundle_path = publish_dir.join("bundle.js");
        assert!(html_path.exists());
        assert!(bundle_path.exists());

        let index_html = std::fs::read_to_string(&html_path).expect("read index.html");
        assert!(
            index_html.contains("id=\"stage-a11y-overlay\""),
            "expected #stage-a11y-overlay in published index.html"
        );
        assert!(
            index_html.contains("role=\"button\"")
                && index_html.contains("aria-label=\"Launch Demo Button\"")
                && index_html.contains("aria-live=\"polite\"")
                && index_html.contains("aria-keyshortcuts=\"Enter\""),
            "expected extended ARIA attributes in published index.html, got: {index_html}"
        );

        // 6. Live browser verification in headless Google Chrome (when installed)
        let chrome_bin =
            std::path::Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        if chrome_bin.exists() {
            let file_url = format!("file://{}", html_path.display());
            let out = std::process::Command::new(chrome_bin)
                .args([
                    "--headless",
                    "--disable-gpu",
                    "--allow-file-access-from-files",
                    "--virtual-time-budget=2000",
                    "--dump-dom",
                    &file_url,
                ])
                .output()
                .expect("run headless Google Chrome on published index.html");
            assert!(
                out.status.success(),
                "headless Chrome failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let dom = String::from_utf8_lossy(&out.stdout);
            assert!(
                dom.contains("id=\"stage-a11y-overlay\"") && dom.contains("Launch Demo Button"),
                "expected live #stage-a11y-overlay in Chrome DOM, got: {dom}"
            );
            assert!(
                dom.contains("E2E:false:true:true:false:51"),
                "expected live script trace 'E2E:false:true:true:false:51' in Chrome DOM, got: {dom}"
            );
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
