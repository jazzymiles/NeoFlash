//! Multi-Paint `Appearance` stack, path modifiers (`Repeater`, `Offset Paths`, `Round Corners`,
//! `Mirror`), procedural vector effects (`Zig Zag`, `Pucker & Bloat`, `Roughen`, `Twist`), and
//! reusable Graphic Styles (`VectorStyle`) (`T-233`).
#![allow(dead_code)]

use crate::app::Editor;
use crate::paint_controls::{show_color32_field_popup, two_column_row};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::{
    TextStyle, components as nf, icons::i16, palette, tokens::space, typography::rich,
};

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
            Self::OffsetContour => "Offset path",
            Self::LiveCorners => "Round corners",
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
            Self::ZigZag => "Zig zag",
            Self::OffsetPath => "Offset path",
            Self::PuckerBloat => "Pucker and bloat",
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

fn next_graphic_style_name(styles: &[VectorStyle]) -> String {
    let mut index = 1usize;
    loop {
        let candidate = format!("Style {index}");
        if styles.iter().all(|style| style.name != candidate) {
            return candidate;
        }
        index += 1;
    }
}

impl Editor {
    fn selected_vector_element_id(&self) -> Option<Id> {
        let el = self.selected_element()?;
        match &el.kind {
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => Some(el.id.clone()),
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

    /// Set both color and opacity (`0.0..=1.0`) of an `Appearance` paint entry in 1 Undo step (`T-301`).
    pub fn appearance_set_entry_paint(&mut self, index: usize, color: Color, opacity: f64) -> bool {
        if !opacity.is_finite() {
            return false;
        }
        let clamped = opacity.clamp(0.0, 1.0);
        self.mutate_selected_appearance("Set appearance paint", |_el, app| {
            let Some(entry) = app.entries.get_mut(index) else {
                return false;
            };
            match entry {
                AppearancePaintEntry::Fill {
                    color: c,
                    opacity: op,
                    ..
                }
                | AppearancePaintEntry::Stroke {
                    color: c,
                    opacity: op,
                    ..
                } => {
                    if *c == color && (*op - clamped).abs() <= 1e-6 {
                        return false;
                    }
                    *c = color;
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

    /// Reorder a procedural vector effect from `from_index` to `to_index` in 1 Undo step (`T-301`).
    pub fn appearance_move_procedural_effect(
        &mut self,
        from_index: usize,
        to_index: usize,
    ) -> bool {
        self.mutate_selected_vector_effects("Reorder vector effect", |fx| {
            if from_index >= fx.procedural_effects.len()
                || to_index >= fx.procedural_effects.len()
                || from_index == to_index
            {
                return false;
            }
            let item = fx.procedural_effects.remove(from_index);
            fx.procedural_effects.insert(to_index, item);
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
            let style_id = loop {
                let candidate = new_id("style");
                if styles.iter().all(|style| style.id != candidate) {
                    break candidate;
                }
            };
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
                    style.apply_to_appearance(&mut app);
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

    /// Render the `Appearance` inspector panel (`T-233` / `T-264` / `T-301`).
    pub fn appearance_panel_ui(&mut self, ui: &mut egui::Ui) {
        let Some(appearance) = self.selected_element_appearance() else {
            ui.label("Select a vector shape on Stage to edit multi-paint fills, strokes, modifiers, and Graphic Styles.");
            return;
        };
        let vector_effects = self.selected_element_vector_effects().unwrap_or_default();

        self.appearance_graphic_style_section(ui, &appearance);
        self.appearance_paint_stack_section(ui, &appearance);
        self.appearance_modifiers_section(ui, &appearance);
        self.appearance_effects_section(ui, &vector_effects);
    }

    fn appearance_graphic_style_section(
        &mut self,
        ui: &mut egui::Ui,
        appearance: &ElementAppearance,
    ) {
        let styles = self.project.vector_styles();
        let active_style_name = appearance
            .style_id
            .as_deref()
            .and_then(|id| styles.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "None".to_string());

        nf::SectionHeader::new("Graphic style")
            .id_salt("appearance_graphic_style_sec")
            .show(ui, |ui| {
                let mut chosen_action: Option<Option<String>> = None;
                nf::Select::new("appearance_graphic_style_select", &active_style_name)
                    .prefix("Style")
                    .width(ui.available_width())
                    .show(ui, |ui| {
                        if ui
                            .add(nf::MenuItem::new("None").checked(appearance.style_id.is_none()))
                            .clicked()
                        {
                            chosen_action = Some(None);
                        }
                        for s in &styles {
                            if ui
                                .add(
                                    nf::MenuItem::new(&s.name).checked(
                                        appearance.style_id.as_deref() == Some(s.id.as_str()),
                                    ),
                                )
                                .clicked()
                            {
                                chosen_action = Some(Some(s.id.clone()));
                            }
                        }
                    });
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
                let has_style = appearance.style_id.is_some();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::s2();
                    if ui.add(nf::Button::new("Save style…")).clicked() {
                        let next_name = next_graphic_style_name(&styles);
                        let _ = self.appearance_save_graphic_style(&next_name);
                    }
                    if ui
                        .add_enabled(has_style, nf::Button::ghost("Unlink"))
                        .clicked()
                    {
                        let _ = self.appearance_unlink_graphic_style();
                    }
                    if ui
                        .add_enabled(has_style, nf::Button::ghost("Delete"))
                        .clicked()
                        && let Some(style_id) = appearance.style_id.as_deref()
                    {
                        let id_owned = style_id.to_string();
                        let _ = self.appearance_delete_graphic_style(&id_owned);
                    }
                });
            });
    }

    fn appearance_paint_stack_section(
        &mut self,
        ui: &mut egui::Ui,
        appearance: &ElementAppearance,
    ) {
        nf::SectionHeader::new("Appearance")
            .id_salt("appearance_fills_strokes_sec")
            .show(ui, |ui| {
                let entries_len = appearance.entries.len();
                for (idx, entry) in appearance.entries.iter().enumerate() {
                    appearance_item_box(ui, ("appearance_entry", idx), |ui| {
                        self.appearance_paint_entry_item(ui, idx, entries_len, entry);
                    });
                }

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::s2(), space::s2());
                    if ui.add(nf::Button::new("Add fill")).clicked() {
                        let _ = self.appearance_add_fill(self.fill, [0.0, 0.0], 1.0);
                    }
                    if ui.add(nf::Button::new("Add stroke")).clicked() {
                        let _ = self.appearance_add_stroke(
                            self.stroke,
                            self.stroke_width.max(1.0),
                            0.0,
                            1.0,
                        );
                    }
                    if ui.add(nf::Button::ghost("Expand appearance")).clicked() {
                        let _ = self.expand_selected_shape_appearance();
                    }
                });
            });
    }

    fn appearance_paint_entry_item(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        entries_len: usize,
        entry: &AppearancePaintEntry,
    ) {
        let (title, color_label, color, opacity) = match entry {
            AppearancePaintEntry::Fill { color, opacity, .. } => {
                ("Fill", "Fill color", *color, *opacity)
            }
            AppearancePaintEntry::Stroke { color, opacity, .. } => {
                ("Stroke", "Stroke color", *color, *opacity)
            }
        };

        let mut visible = opacity > 0.0;
        let mut header_action = None;
        appearance_header_row(ui, |ui| {
            if ui.add(nf::Checkbox::new(&mut visible, title)).changed() {
                let _ = self.appearance_toggle_entry_visibility(idx);
            }
            header_action = item_header_actions(ui, idx, entries_len);
        });
        if let Some(action) = header_action {
            match action {
                ItemAction::MoveUp => {
                    let _ = self.appearance_move_entry(idx, idx - 1);
                }
                ItemAction::MoveDown => {
                    let _ = self.appearance_move_entry(idx, idx + 1);
                }
                ItemAction::Remove => {
                    let _ = self.appearance_remove_entry(idx);
                }
            }
            return;
        }

        // Color32 premultiplies RGB at alpha 0; preserve the stored RGB when hidden by disabling the field.
        let display_a = if visible {
            (color[3] as f64 * opacity).round().clamp(0.0, 255.0) as u8
        } else {
            color[3]
        };
        let mut edit_c32 =
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], display_a);
        let color_resp = ui.add_enabled_ui(visible, |ui| {
            let cf = nf::ColorField::new(&mut edit_c32, color_label)
                .width(ui.available_width())
                .show(ui);
            crate::panels::dialog_helpers::track_rgba_popup(ui.ctx(), cf.response.id);
            let popup_changed = show_color32_field_popup(
                ui,
                &cf.response,
                cf.swatch_clicked,
                color_label,
                &mut edit_c32,
            );
            cf.response.changed() || popup_changed
        });
        if color_resp.inner {
            let [nr, ng, nb, na] = edit_c32.to_srgba_unmultiplied();
            let rgb_changed = [nr, ng, nb] != [color[0], color[1], color[2]];
            let alpha_changed = na != display_a;
            if rgb_changed && !alpha_changed {
                let _ = self.appearance_set_entry_color(idx, [nr, ng, nb, color[3]]);
            } else if alpha_changed && !rgb_changed && color[3] == 255 {
                let _ = self.appearance_set_entry_opacity(idx, na as f64 / 255.0);
            } else if rgb_changed || alpha_changed {
                let _ = self.appearance_set_entry_paint(idx, [nr, ng, nb, 255], na as f64 / 255.0);
            }
        }

        match entry {
            AppearancePaintEntry::Fill { offset, .. } => {
                let mut off_x = offset[0];
                let mut off_y = offset[1];
                if appearance_two_fields(
                    ui,
                    (&mut off_x, NumericSpec::px("dX", "dX", -1000.0..=1000.0)),
                    (&mut off_y, NumericSpec::px("dY", "dY", -1000.0..=1000.0)),
                ) {
                    let _ = self.appearance_set_fill_entry_offset(idx, [off_x, off_y]);
                }
            }
            AppearancePaintEntry::Stroke {
                width,
                offset_distance,
                ..
            } => {
                let mut edit_w = *width;
                let mut edit_off = *offset_distance;
                if appearance_two_fields(
                    ui,
                    (
                        &mut edit_w,
                        NumericSpec {
                            label: "W",
                            prefix: "W",
                            unit: "px",
                            range: 0.1..=200.0,
                            step: 0.5,
                        },
                    ),
                    (
                        &mut edit_off,
                        NumericSpec::px("Offset", "Offset", -500.0..=500.0),
                    ),
                ) {
                    let _ = self.appearance_set_stroke_entry_width(idx, edit_w, edit_off);
                }
            }
        }
    }

    fn appearance_modifiers_section(&mut self, ui: &mut egui::Ui, appearance: &ElementAppearance) {
        nf::SectionHeader::new("Path modifiers")
            .id_salt("appearance_path_modifiers_sec")
            .show(ui, |ui| {
                let mut add_kind: Option<AppearanceModifierKind> = None;
                nf::Select::new("appearance_add_modifier_select", "Add modifier")
                    .width(ui.available_width())
                    .show(ui, |ui| {
                        for kind in AppearanceModifierKind::ALL {
                            if ui.add(nf::MenuItem::new(kind.label())).clicked() {
                                add_kind = Some(kind);
                            }
                        }
                    });
                if let Some(kind) = add_kind {
                    let _ = self.appearance_add_modifier(kind.default_modifier());
                }

                let mod_len = appearance.modifiers.len();
                for (midx, modifier) in appearance.modifiers.iter().enumerate() {
                    appearance_item_box(ui, ("appearance_modifier", midx), |ui| {
                        self.appearance_modifier_item(ui, midx, mod_len, modifier);
                    });
                }
            });
    }

    fn appearance_modifier_item(
        &mut self,
        ui: &mut egui::Ui,
        midx: usize,
        mod_len: usize,
        modifier: &VectorModifier,
    ) {
        if let Some(action) = item_title_header(ui, modifier_kind_label(modifier), midx, mod_len) {
            match action {
                ItemAction::MoveUp => {
                    let _ = self.appearance_move_modifier(midx, midx - 1);
                }
                ItemAction::MoveDown => {
                    let _ = self.appearance_move_modifier(midx, midx + 1);
                }
                ItemAction::Remove => {
                    let _ = self.appearance_remove_modifier(midx);
                }
            }
            return;
        }

        match modifier {
            VectorModifier::OffsetContour { delta } => {
                let mut edit_delta = *delta;
                if appearance_field(
                    ui,
                    &mut edit_delta,
                    NumericSpec::px("Offset", "Offset", -500.0..=500.0),
                    ui.available_width(),
                ) {
                    let _ = self.appearance_set_modifier(
                        midx,
                        VectorModifier::OffsetContour { delta: edit_delta },
                    );
                }
            }
            VectorModifier::LiveCorners { radius } => {
                let mut edit_radius = *radius;
                if appearance_field(
                    ui,
                    &mut edit_radius,
                    NumericSpec::px("Radius", "Radius", 0.0..=500.0),
                    ui.available_width(),
                ) {
                    let _ = self.appearance_set_modifier(
                        midx,
                        VectorModifier::LiveCorners {
                            radius: edit_radius,
                        },
                    );
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
                let mut scale_pct = *scale_step * 100.0;
                let mut ox = *offset_x;
                let mut oy = *offset_y;
                let mut rot = *rotation_step;
                let row1_ch = appearance_two_fields(
                    ui,
                    (&mut c, NumericSpec::int("Copies", "Copies", 1.0..=64.0)),
                    (
                        &mut scale_pct,
                        NumericSpec {
                            label: "Scale",
                            prefix: "Scale",
                            unit: "%",
                            range: 10.0..=500.0,
                            step: 1.0,
                        },
                    ),
                );
                let row2_ch = appearance_two_fields(
                    ui,
                    (&mut ox, NumericSpec::px("dX", "dX", -1000.0..=1000.0)),
                    (&mut oy, NumericSpec::px("dY", "dY", -1000.0..=1000.0)),
                );
                let rot_ch = appearance_field(
                    ui,
                    &mut rot,
                    NumericSpec {
                        label: "Rotation",
                        prefix: "Rotation",
                        unit: "°",
                        range: -360.0..=360.0,
                        step: 1.0,
                    },
                    ui.available_width(),
                );
                if row1_ch || row2_ch || rot_ch {
                    let _ = self.appearance_set_modifier(
                        midx,
                        VectorModifier::Repeater {
                            copies: c.round().clamp(1.0, 64.0) as u32,
                            offset_x: ox,
                            offset_y: oy,
                            scale_step: (scale_pct / 100.0).clamp(0.1, 5.0),
                            rotation_step: rot,
                        },
                    );
                }
            }
            VectorModifier::Mirror { axis_x, axis_y } => {
                let mut ax = axis_x.unwrap_or(0.0);
                let mut ay = axis_y.unwrap_or(0.0);
                let mut use_x = axis_x.is_some();
                let mut use_y = axis_y.is_some();
                let mut toggle_ch = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::s4();
                    toggle_ch |= ui.add(nf::Checkbox::new(&mut use_x, "Mirror X")).changed();
                    toggle_ch |= ui.add(nf::Checkbox::new(&mut use_y, "Mirror Y")).changed();
                });
                let axis_ch = appearance_two_fields(
                    ui,
                    (&mut ax, NumericSpec::px("X", "X", -2000.0..=2000.0)),
                    (&mut ay, NumericSpec::px("Y", "Y", -2000.0..=2000.0)),
                );
                if toggle_ch || axis_ch {
                    let _ = self.appearance_set_modifier(
                        midx,
                        VectorModifier::Mirror {
                            axis_x: use_x.then_some(ax),
                            axis_y: use_y.then_some(ay),
                        },
                    );
                }
            }
        }
    }

    fn appearance_effects_section(
        &mut self,
        ui: &mut egui::Ui,
        vector_effects: &ElementVectorEffects,
    ) {
        nf::SectionHeader::new("Vector effects")
            .id_salt("appearance_vector_effects_sec")
            .show(ui, |ui| {
                let mut add_kind: Option<AppearanceProceduralKind> = None;
                nf::Select::new("appearance_add_effect_select", "Add effect")
                    .width(ui.available_width())
                    .show(ui, |ui| {
                        for kind in AppearanceProceduralKind::ALL {
                            if ui.add(nf::MenuItem::new(kind.label())).clicked() {
                                add_kind = Some(kind);
                            }
                        }
                    });
                if let Some(kind) = add_kind {
                    let _ = self.appearance_add_procedural_effect(kind.default_effect());
                }

                let fx_len = vector_effects.procedural_effects.len();
                for (pidx, effect) in vector_effects.procedural_effects.iter().enumerate() {
                    appearance_item_box(ui, ("appearance_proc_effect", pidx), |ui| {
                        self.appearance_effect_item(ui, pidx, fx_len, effect);
                    });
                }
            });
    }

    fn appearance_effect_item(
        &mut self,
        ui: &mut egui::Ui,
        pidx: usize,
        fx_len: usize,
        effect: &ProceduralVectorEffect,
    ) {
        if let Some(action) = item_title_header(ui, procedural_kind_label(effect), pidx, fx_len) {
            match action {
                ItemAction::MoveUp => {
                    let _ = self.appearance_move_procedural_effect(pidx, pidx - 1);
                }
                ItemAction::MoveDown => {
                    let _ = self.appearance_move_procedural_effect(pidx, pidx + 1);
                }
                ItemAction::Remove => {
                    let _ = self.appearance_remove_procedural_effect(pidx);
                }
            }
            return;
        }

        match effect {
            ProceduralVectorEffect::ZigZag {
                size,
                ridges_per_segment,
                smooth,
            } => {
                let mut sz = *size;
                let mut ridges = *ridges_per_segment as f64;
                let mut sm = *smooth;
                let row1_ch = appearance_two_fields(
                    ui,
                    (&mut sz, NumericSpec::px("Size", "Size", 0.0..=200.0)),
                    (
                        &mut ridges,
                        NumericSpec::int("Ridges", "Ridges", 1.0..=32.0),
                    ),
                );
                let sm_ch = ui.add(nf::Checkbox::new(&mut sm, "Smooth")).changed();
                if row1_ch || sm_ch {
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
                if appearance_field(
                    ui,
                    &mut off,
                    NumericSpec::px("Offset", "Offset", -500.0..=500.0),
                    ui.available_width(),
                ) {
                    let _ = self.appearance_set_procedural_effect(
                        pidx,
                        ProceduralVectorEffect::OffsetPath { offset: off },
                    );
                }
            }
            ProceduralVectorEffect::PuckerBloat { amount } => {
                let mut amt = *amount;
                if appearance_field(
                    ui,
                    &mut amt,
                    NumericSpec {
                        label: "Amount",
                        prefix: "Amount",
                        unit: "%",
                        range: -200.0..=200.0,
                        step: 1.0,
                    },
                    ui.available_width(),
                ) {
                    let _ = self.appearance_set_procedural_effect(
                        pidx,
                        ProceduralVectorEffect::PuckerBloat { amount: amt },
                    );
                }
            }
            ProceduralVectorEffect::Roughen { size, detail, seed } => {
                let mut sz = *size;
                let mut det = *detail as f64;
                let mut sd = *seed as f64;
                let row1_ch = appearance_two_fields(
                    ui,
                    (&mut sz, NumericSpec::px("Size", "Size", 0.0..=200.0)),
                    (&mut det, NumericSpec::int("Detail", "Detail", 1.0..=32.0)),
                );
                let sd_ch = appearance_field(
                    ui,
                    &mut sd,
                    NumericSpec::int("Seed", "Seed", 0.0..=9999.0),
                    ui.available_width(),
                );
                if row1_ch || sd_ch {
                    let _ = self.appearance_set_procedural_effect(
                        pidx,
                        ProceduralVectorEffect::Roughen {
                            size: sz,
                            detail: det.round().clamp(1.0, 32.0) as u32,
                            seed: sd.round().max(0.0) as u32,
                        },
                    );
                }
            }
            ProceduralVectorEffect::Twist { angle_deg } => {
                let mut ang = *angle_deg;
                if appearance_field(
                    ui,
                    &mut ang,
                    NumericSpec {
                        label: "Angle",
                        prefix: "Angle",
                        unit: "°",
                        range: -720.0..=720.0,
                        step: 1.0,
                    },
                    ui.available_width(),
                ) {
                    let _ = self.appearance_set_procedural_effect(
                        pidx,
                        ProceduralVectorEffect::Twist { angle_deg: ang },
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ItemAction {
    MoveUp,
    MoveDown,
    Remove,
}

struct NumericSpec<'a> {
    label: &'a str,
    prefix: &'a str,
    unit: &'a str,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
}

impl<'a> NumericSpec<'a> {
    fn px(label: &'a str, prefix: &'a str, range: std::ops::RangeInclusive<f64>) -> Self {
        Self {
            label,
            prefix,
            unit: "px",
            range,
            step: 1.0,
        }
    }

    fn int(label: &'a str, prefix: &'a str, range: std::ops::RangeInclusive<f64>) -> Self {
        Self {
            label,
            prefix,
            unit: "",
            range,
            step: 1.0,
        }
    }
}

fn appearance_item_box(ui: &mut egui::Ui, id: (&str, usize), body: impl FnOnce(&mut egui::Ui)) {
    ui.push_id(id, |ui| {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = space::s2();
            body(ui);
        });
    });
}

fn appearance_header_row(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), neoflash_ui::metrics::control()),
        egui::Layout::left_to_right(egui::Align::Center),
        body,
    );
}

fn item_header_actions(ui: &mut egui::Ui, idx: usize, len: usize) -> Option<ItemAction> {
    let mut action = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if ui.add(nf::IconButton::new(i16::TRASH, "Remove")).clicked() {
            action = Some(ItemAction::Remove);
        }
        if ui
            .add_enabled(
                idx + 1 < len,
                nf::IconButton::new(i16::CHEVDOWN, "Move down"),
            )
            .clicked()
        {
            action = Some(ItemAction::MoveDown);
        }
        if ui
            .add_enabled(idx > 0, nf::IconButton::new(i16::CHEVUP, "Move up"))
            .clicked()
        {
            action = Some(ItemAction::MoveUp);
        }
    });
    action
}

fn item_title_header(ui: &mut egui::Ui, title: &str, idx: usize, len: usize) -> Option<ItemAction> {
    let text_primary = palette(ui.ctx()).text_primary;
    let mut action = None;
    appearance_header_row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        ui.label(rich(title, TextStyle::BodyStrong).color(text_primary));
        action = item_header_actions(ui, idx, len);
    });
    action
}

fn appearance_field(ui: &mut egui::Ui, value: &mut f64, spec: NumericSpec<'_>, width: f32) -> bool {
    ui.add(
        nf::Field::new(value, spec.label)
            .prefix(spec.prefix)
            .unit(spec.unit)
            .range(spec.range)
            .step(spec.step)
            .width(width),
    )
    .changed()
}

fn appearance_two_fields(
    ui: &mut egui::Ui,
    (left_val, left_spec): (&mut f64, NumericSpec<'_>),
    (right_val, right_spec): (&mut f64, NumericSpec<'_>),
) -> bool {
    let (left_changed, right_changed) = two_column_row(
        ui,
        |ui, width| appearance_field(ui, left_val, left_spec, width),
        |ui, width| appearance_field(ui, right_val, right_spec, width),
    );
    left_changed || right_changed
}

fn modifier_kind_label(modifier: &VectorModifier) -> &'static str {
    match modifier {
        VectorModifier::OffsetContour { .. } => AppearanceModifierKind::OffsetContour.label(),
        VectorModifier::LiveCorners { .. } => AppearanceModifierKind::LiveCorners.label(),
        VectorModifier::Repeater { .. } => AppearanceModifierKind::Repeater.label(),
        VectorModifier::Mirror { .. } => AppearanceModifierKind::Mirror.label(),
    }
}

fn procedural_kind_label(effect: &ProceduralVectorEffect) -> &'static str {
    match effect {
        ProceduralVectorEffect::ZigZag { .. } => AppearanceProceduralKind::ZigZag.label(),
        ProceduralVectorEffect::OffsetPath { .. } => AppearanceProceduralKind::OffsetPath.label(),
        ProceduralVectorEffect::PuckerBloat { .. } => AppearanceProceduralKind::PuckerBloat.label(),
        ProceduralVectorEffect::Roughen { .. } => AppearanceProceduralKind::Roughen.label(),
        ProceduralVectorEffect::Twist { .. } => AppearanceProceduralKind::Twist.label(),
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

        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        let app2_updated = key.appearance("rect-2").expect("updated rect-2 appearance");
        assert!(matches!(
            &app2_updated.entries[0],
            AppearancePaintEntry::Fill {
                color: [20, 220, 180, 255],
                ..
            }
        ));
        assert!(matches!(
            &app2_updated.entries[1],
            AppearancePaintEntry::Stroke {
                color: [255, 255, 255, 255],
                width: 4.0,
                ..
            }
        ));
        let evaluated = sample_layer(&editor.project.symbols[0].timeline.layers[0], 0);
        assert!(evaluated.iter().any(|element| {
            element.id == "rect-2"
                && matches!(
                    element.kind,
                    ElementKind::Path {
                        fill: [20, 220, 180, 255],
                        ..
                    }
                )
        }));
        assert!(evaluated.iter().any(|element| {
            element.id.starts_with("rect-2-app-")
                && matches!(
                    element.kind,
                    ElementKind::Path {
                        stroke: [255, 255, 255, 255],
                        stroke_width: 4.0,
                        ..
                    }
                )
        }));
        let json = editor.project.to_json().expect("serialize updated styles");
        let reopened = Project::from_json(&json).expect("reopen updated styles");
        let reopened_app = reopened.symbols[0].timeline.layers[0].keyframes[0]
            .appearance("rect-2")
            .expect("reopened linked appearance");
        assert_eq!(reopened_app.entries, app2_updated.entries);

        // Undo style update restores previous paints and modifiers on both shapes.
        editor.undo();
        let app2_after_undo = editor
            .selected_element_appearance()
            .expect("rect-2 appearance after undo");
        assert_eq!(
            app2_after_undo.modifiers,
            vec![VectorModifier::OffsetContour { delta: 6.0 }]
        );
        assert!(matches!(
            app2_after_undo.entries[0],
            AppearancePaintEntry::Fill {
                color: [200, 60, 40, 255],
                ..
            }
        ));
    }

    #[test]
    fn graphic_style_ids_do_not_replace_survivors_after_deletion() {
        let mut editor = editor_with_selected_rect();
        let first = VectorStyle {
            id: "style-1".into(),
            name: "Style 1".into(),
            fill: [10, 20, 30, 255],
            stroke: [40, 50, 60, 255],
            stroke_width: 2.0,
            modifiers: vec![],
        };
        let survivor = VectorStyle {
            id: "style-2".into(),
            name: "Style 2".into(),
            fill: [70, 80, 90, 255],
            stroke: [100, 110, 120, 255],
            stroke_width: 3.0,
            modifiers: vec![VectorModifier::LiveCorners { radius: 8.0 }],
        };
        editor.project.set_vector_styles(&[first, survivor.clone()]);
        assert!(editor.appearance_delete_graphic_style("style-1"));

        let generated_name = next_graphic_style_name(&editor.project.vector_styles());
        assert_eq!(generated_name, "Style 1");
        let created = editor
            .appearance_save_graphic_style(&generated_name)
            .expect("save replacement style");
        assert_ne!(created, survivor.id);
        let styles = editor.project.vector_styles();
        assert_eq!(styles.len(), 2);
        assert!(styles.contains(&survivor));
        assert_eq!(styles.iter().filter(|style| style.id == created).count(), 1);
        assert_eq!(
            styles
                .iter()
                .filter(|style| style.name == generated_name)
                .count(),
            1
        );
        editor
            .project
            .validate()
            .expect("unique style ids validate");
    }

    #[test]
    fn appearance_commands_refuse_stale_selection_on_locked_layer() {
        let mut editor = editor_with_selected_rect();
        editor.project.symbols[0].timeline.layers[0].locked = true;
        let before = editor.project.clone();

        assert!(editor.selected_element().is_none());
        assert!(!editor.appearance_add_fill([1, 2, 3, 255], [0.0, 0.0], 1.0));
        assert!(!editor.appearance_expand_selected());
        assert_eq!(editor.project, before);
    }

    #[test]
    fn direct_shape_paints_sync_the_primary_appearance_without_flattening_the_stack() {
        let mut editor = editor_with_selected_rect();
        assert!(editor.appearance_add_fill([9, 8, 7, 255], [12.0, 4.0], 0.5));
        let key = &mut editor.project.symbols[0].timeline.layers[0].keyframes[0];
        let before = key.elements[0].kind.clone();
        let mut after = before.clone();
        if let ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        } = &mut after
        {
            *fill = [21, 31, 41, 255];
            *stroke = [201, 202, 203, 255];
            *stroke_width = 7.0;
        }
        key.elements[0].kind = after.clone();
        let app = key.appearances.first_mut().expect("stored appearance");
        assert!(app.sync_primary_paints_from_kinds(&before, &after));
        assert_eq!(app.entries.len(), 3, "extra fill remains in the stack");
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Fill {
                color: [21, 31, 41, 255],
                ..
            }
        ));
        assert!(matches!(
            app.entries[1],
            AppearancePaintEntry::Stroke {
                color: [201, 202, 203, 255],
                width: 7.0,
                ..
            }
        ));
        assert!(matches!(
            app.entries[2],
            AppearancePaintEntry::Fill {
                color: [9, 8, 7, 255],
                offset: [12.0, 4.0],
                opacity: 0.5,
                ..
            }
        ));
        let evaluated = sample_layer(&editor.project.symbols[0].timeline.layers[0], 0);
        assert!(evaluated.iter().any(|element| matches!(
            element.kind,
            ElementKind::Path {
                fill: [21, 31, 41, 255],
                ..
            }
        )));
    }

    #[test]
    fn direct_properties_alpha_is_not_multiplied_twice_by_appearance_opacity() {
        let mut editor = editor_with_selected_rect();
        assert!(editor.appearance_add_fill([9, 8, 7, 255], [12.0, 4.0], 0.5));
        assert!(editor.appearance_set_entry_opacity(0, 0.5));

        let key = &mut editor.project.symbols[0].timeline.layers[0].keyframes[0];
        let before = key.elements[0].kind.clone();
        let mut after = before.clone();
        if let ElementKind::Rectangle { fill, .. } = &mut after {
            *fill = [21, 31, 41, 64];
        }
        key.elements[0].kind = after.clone();
        let app = key.appearances.first_mut().expect("stored appearance");
        assert!(app.sync_primary_paints_from_kinds(&before, &after));
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Fill {
                color: [21, 31, 41, 128],
                opacity: 0.5,
                ..
            }
        ));

        let mut mirrored = key.elements[0].clone();
        sync_primary_element_paints(&mut mirrored, app);
        assert!(matches!(
            mirrored.kind,
            ElementKind::Rectangle {
                fill: [21, 31, 41, 64],
                ..
            }
        ));
    }

    #[test]
    fn direct_properties_width_preserves_stroke_paint_gradient_opacity_and_swatches() {
        let mut editor = editor_with_selected_rect();
        assert!(editor.appearance_add_fill([9, 8, 7, 255], [12.0, 4.0], 0.5));
        let gradient = GradientPaint {
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            ..GradientPaint::default()
        };

        let key = &mut editor.project.symbols[0].timeline.layers[0].keyframes[0];
        let before = key.elements[0].kind.clone();
        let mut after = before.clone();
        if let ElementKind::Rectangle { stroke_width, .. } = &mut after {
            *stroke_width = 7.0;
        }
        let app = key.appearances.first_mut().expect("stored appearance");
        app.stroke_swatch_id = Some("top-stroke-swatch".into());
        let stroke = app
            .entries
            .iter_mut()
            .find(|entry| matches!(entry, AppearancePaintEntry::Stroke { .. }))
            .expect("primary stroke");
        let AppearancePaintEntry::Stroke {
            color,
            gradient: entry_gradient,
            width,
            swatch_id,
            opacity,
            ..
        } = stroke
        else {
            unreachable!();
        };
        *color = [10, 20, 30, 200];
        *entry_gradient = Some(gradient.clone());
        *width = 2.0;
        *swatch_id = Some("entry-stroke-swatch".into());
        *opacity = 0.4;

        assert!(app.sync_primary_paints_from_kinds(&before, &after));
        assert_eq!(app.stroke_swatch_id.as_deref(), Some("top-stroke-swatch"));
        assert!(matches!(
            app.entries
                .iter()
                .find(|entry| matches!(entry, AppearancePaintEntry::Stroke { .. }))
                .expect("preserved stroke"),
            AppearancePaintEntry::Stroke {
                color: [10, 20, 30, 200],
                gradient: Some(saved_gradient),
                width: 7.0,
                swatch_id: Some(saved_swatch),
                opacity: 0.4,
                ..
            } if saved_gradient == &gradient && saved_swatch == "entry-stroke-swatch"
        ));
    }

    #[test]
    fn solid_graphic_style_clears_painted_path_gradients_and_undoes() {
        let mut editor = editor_with_selected_rect();
        let gradient = GradientPaint {
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            ..GradientPaint::default()
        };
        editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind =
            ElementKind::PaintedPath {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 80.0, y: 0.0 },
                    PathCommand::LineTo { x: 80.0, y: 60.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::NonZero,
                fill: [200, 60, 40, 255],
                stroke: [20, 30, 40, 255],
                stroke_width: 2.0,
                fill_gradient: Some(gradient.clone()),
                stroke_gradient: Some(gradient),
            };
        let style = VectorStyle {
            id: "solid-style".into(),
            name: "Solid".into(),
            fill: [10, 120, 230, 255],
            stroke: [240, 230, 20, 255],
            stroke_width: 6.0,
            modifiers: vec![],
        };
        editor
            .project
            .set_vector_styles(std::slice::from_ref(&style));
        let before = editor.project.clone();

        assert!(editor.appearance_apply_graphic_style(&style.id));
        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        assert!(matches!(
            key.elements[0].kind,
            ElementKind::PaintedPath {
                fill: [10, 120, 230, 255],
                stroke: [240, 230, 20, 255],
                stroke_width: 6.0,
                fill_gradient: None,
                stroke_gradient: None,
                ..
            }
        ));
        let evaluated = sample_layer(&editor.project.symbols[0].timeline.layers[0], 0);
        assert!(evaluated.iter().any(|element| matches!(
            element.kind,
            ElementKind::Path {
                fill: [10, 120, 230, 255],
                ..
            }
        )));
        editor
            .project
            .validate()
            .expect("solid style project valid");

        editor.undo();
        assert_eq!(editor.project, before);
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

    fn collect_accesskit_bounds(output: &egui::FullOutput, label: &str) -> Vec<(egui::Rect, bool)> {
        let Some(update) = output.platform_output.accesskit_update.as_ref() else {
            return Vec::new();
        };
        let mut nodes: Vec<(egui::Rect, bool)> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label))
            .filter_map(|(_, node)| {
                let bounds = node.bounds()?;
                Some((
                    egui::Rect::from_min_max(
                        egui::Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                        egui::Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                    ),
                    !node.is_disabled(),
                ))
            })
            .collect();
        nodes.sort_by(|a, b| {
            a.0.top()
                .total_cmp(&b.0.top())
                .then_with(|| a.0.left().total_cmp(&b.0.left()))
        });
        nodes
    }

    fn run_appearance_panel(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Vec<String>, egui::Rect) {
        let mut panel_rect = egui::Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(286.0, 2600.0),
                )),
                time: Some(time),
                events,
                ..egui::RawInput::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        panel_rect = ui.available_rect_before_wrap();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = neoflash_ui::tokens::space::s3();
                                editor.appearance_panel_ui(ui);
                            });
                    });
            },
        );
        let top_shapes: Vec<egui::epaint::Shape> =
            output.shapes.iter().map(|cs| cs.shape.clone()).collect();
        let mut texts = Vec::new();
        collect_shape_texts(&top_shapes, &mut texts);
        (output, texts, panel_rect)
    }

    fn click_point_events(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn appearance_live_properties_panel_renders_controls_and_command_id_expands() {
        let mut editor = editor_with_selected_rect();
        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, 1400.0),
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
        for expected in [
            "Graphic style",
            "Style",
            "Save style…",
            "Unlink",
            "Delete",
            "Appearance",
            "Fill",
            "Stroke",
            "Add fill",
            "Add stroke",
            "Expand appearance",
            "Path modifiers",
            "Add modifier",
            "Vector effects",
            "Add effect",
        ] {
            assert!(
                texts.iter().any(|t| t.eq_ignore_ascii_case(expected)),
                "expected '{expected}' in live Properties panel, got: {texts:?}"
            );
        }
        for forbidden in ["+ Fill", "+ Stroke", "↑", "↓", "Save Style"] {
            assert!(
                !texts.iter().any(|t| t == forbidden),
                "unexpected legacy control '{forbidden}' in Properties panel: {texts:?}"
            );
        }

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
    fn appearance_ui_selects_buttons_icon_actions_and_paint_edits_work() {
        let mut editor = editor_with_selected_rect();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());

        // Warm up & inspect initial composition on 286 px NF2 inspector body width.
        run_appearance_panel(&mut editor, &ctx, 0.1, vec![]);
        let (out, texts, panel_rect) = run_appearance_panel(&mut editor, &ctx, 0.2, vec![]);

        for label in [
            "Style, None",
            "Save style…",
            "Unlink",
            "Delete",
            "Fill",
            "Stroke",
            "Add fill",
            "Add stroke",
            "Expand appearance",
            "Add modifier",
            "Add effect",
        ] {
            let bounds = collect_accesskit_bounds(&out, label);
            assert!(
                !bounds.is_empty(),
                "expected AccessKit node '{label}' in {texts:?}"
            );
            for (rect, _) in &bounds {
                assert!(
                    rect.right() <= panel_rect.right() + 1.0,
                    "control '{label}' overflowed right edge: {rect:?} vs {panel_rect:?}"
                );
            }
        }
        assert!(
            !collect_accesskit_bounds(&out, "Unlink")[0].1
                && !collect_accesskit_bounds(&out, "Delete")[0].1,
            "Unlink and Delete should start disabled when no style is linked"
        );

        // Two initial paint entries -> first Move up is disabled, last Move down is disabled.
        let move_ups = collect_accesskit_bounds(&out, "Move up");
        let move_downs = collect_accesskit_bounds(&out, "Move down");
        assert_eq!(move_ups.len(), 2);
        assert_eq!(move_downs.len(), 2);
        assert!(!move_ups[0].1 && move_ups[1].1);
        assert!(move_downs[0].1 && !move_downs[1].1);

        let mut time = 0.2;
        let click_appearance =
            |editor: &mut Editor, ctx: &egui::Context, time: &mut f64, pos: egui::Pos2| {
                *time += 0.1;
                let _ = run_appearance_panel(editor, ctx, *time, click_point_events(pos));
                *time += 0.05;
                run_appearance_panel(editor, ctx, *time, vec![])
            };

        // Clicking Move up on second paint entry swaps Stroke before Fill in 1 Undo step.
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, move_ups[1].0.center());
        let app = editor.selected_element_appearance().unwrap();
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Stroke { .. }
        ));

        // Toggle visibility on entry 0 via Checkbox leaves RGB intact cleanly.
        let stroke_cb = collect_accesskit_bounds(&out, "Stroke")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, stroke_cb);
        let app = editor.selected_element_appearance().unwrap();
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Stroke {
                opacity: 0.0,
                color: [20, 30, 40, 255],
                ..
            }
        ));
        assert!(
            !collect_accesskit_bounds(&out, "Stroke color")[0].1,
            "hidden paint ColorField should be disabled to preserve RGB"
        );
        // Restore visibility before continuing
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, stroke_cb);

        // Add fill and Add stroke buttons append entries cleanly.
        let add_fill_pt = collect_accesskit_bounds(&out, "Add fill")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, add_fill_pt);
        assert_eq!(
            editor.selected_element_appearance().unwrap().entries.len(),
            3
        );
        // Remove newly added entry using Trash icon button (last Remove)
        let removes = collect_accesskit_bounds(&out, "Remove");
        let (out, _, _) = click_appearance(
            &mut editor,
            &ctx,
            &mut time,
            removes.last().unwrap().0.center(),
        );
        assert_eq!(
            editor.selected_element_appearance().unwrap().entries.len(),
            2
        );

        // Open Add modifier Select and choose Repeater -> verifies Scale % + Rotation UI fields.
        let add_mod_pt = collect_accesskit_bounds(&out, "Add modifier")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, add_mod_pt);
        let repeater_pt = collect_accesskit_bounds(&out, "Repeater")
            .into_iter()
            .last()
            .expect("Repeater menu item")
            .0
            .center();
        let (out, mod_texts, _) = click_appearance(&mut editor, &ctx, &mut time, repeater_pt);
        assert_eq!(
            editor
                .selected_element_appearance()
                .unwrap()
                .modifiers
                .len(),
            1
        );
        assert!(
            mod_texts.iter().any(|t| t == "Copies")
                && mod_texts.iter().any(|t| t == "Scale")
                && mod_texts.iter().any(|t| t == "Rotation")
                && mod_texts.iter().any(|t| t == "100 %"),
            "expected Repeater NF2 fields, got: {mod_texts:?}"
        );
        assert!(
            !mod_texts.iter().any(|t| t == "Rot"),
            "legacy 'Rot' prefix should be replaced with 'Rotation': {mod_texts:?}"
        );

        // Add Mirror modifier, then reorder it above Repeater via Move up IconButton.
        let add_mod_pt = collect_accesskit_bounds(&out, "Add modifier")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, add_mod_pt);
        let mirror_pt = collect_accesskit_bounds(&out, "Mirror")
            .into_iter()
            .last()
            .expect("Mirror menu item")
            .0
            .center();
        let (out, mod2_texts, _) = click_appearance(&mut editor, &ctx, &mut time, mirror_pt);
        assert_eq!(
            editor
                .selected_element_appearance()
                .unwrap()
                .modifiers
                .len(),
            2
        );
        assert!(
            mod2_texts.iter().any(|t| t == "Mirror X")
                && mod2_texts.iter().any(|t| t == "Mirror Y"),
            "expected Mirror checkboxes: {mod2_texts:?}"
        );
        // Move up icon for Mirror (4th Move up: 2 paint entries + 2 modifiers -> index 3)
        let mod_move_up = collect_accesskit_bounds(&out, "Move up")[3].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, mod_move_up);
        assert!(matches!(
            editor.selected_element_appearance().unwrap().modifiers[0],
            VectorModifier::Mirror { .. }
        ));

        // Open Add effect Select and choose Zig zag & Twist -> test move & remove.
        let add_fx_pt = collect_accesskit_bounds(&out, "Add effect")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, add_fx_pt);
        let zigzag_pt = collect_accesskit_bounds(&out, "Zig zag")[0].0.center();
        let (out, fx_texts, _) = click_appearance(&mut editor, &ctx, &mut time, zigzag_pt);
        assert!(
            fx_texts.iter().any(|t| t == "Size")
                && fx_texts.iter().any(|t| t == "Ridges")
                && fx_texts.iter().any(|t| t == "Smooth"),
            "expected Zig zag NF2 fields, got: {fx_texts:?}"
        );

        let add_fx_pt = collect_accesskit_bounds(&out, "Add effect")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, add_fx_pt);
        let twist_pt = collect_accesskit_bounds(&out, "Twist")[0].0.center();
        let (out, fx2_texts, _) = click_appearance(&mut editor, &ctx, &mut time, twist_pt);
        assert!(fx2_texts.iter().any(|t| t == "Angle"));
        assert_eq!(
            editor
                .selected_element_vector_effects()
                .unwrap()
                .procedural_effects
                .len(),
            2
        );

        // Move Twist up via IconButton -> Twist at index 0, Undo -> ZigZag back at index 0.
        let fx_move_up = collect_accesskit_bounds(&out, "Move up")
            .last()
            .unwrap()
            .0
            .center();
        let (_, _, _) = click_appearance(&mut editor, &ctx, &mut time, fx_move_up);
        assert!(matches!(
            editor
                .selected_element_vector_effects()
                .unwrap()
                .procedural_effects[0],
            ProceduralVectorEffect::Twist { .. }
        ));
        editor.undo();
        assert!(matches!(
            editor
                .selected_element_vector_effects()
                .unwrap()
                .procedural_effects[0],
            ProceduralVectorEffect::ZigZag { .. }
        ));
        time += 0.05;
        let (out, _, _) = run_appearance_panel(&mut editor, &ctx, time, vec![]);

        // Graphic style Save style… -> Unlink -> Delete round-trip via UI buttons.
        let save_pt = collect_accesskit_bounds(&out, "Save style…")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, save_pt);
        assert_eq!(editor.project.vector_styles().len(), 1);
        assert!(collect_accesskit_bounds(&out, "Style, Style 1").len() == 1);
        assert!(
            collect_accesskit_bounds(&out, "Unlink")[0].1
                && collect_accesskit_bounds(&out, "Delete")[0].1
        );

        let unlink_pt = collect_accesskit_bounds(&out, "Unlink")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, unlink_pt);
        assert!(
            editor
                .selected_element_appearance()
                .unwrap()
                .style_id
                .is_none()
        );
        assert!(collect_accesskit_bounds(&out, "Style, None").len() == 1);

        // Re-link via Style: None Select dropdown then Delete style.
        let style_sel_pt = collect_accesskit_bounds(&out, "Style, None")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, style_sel_pt);
        let style1_pt = collect_accesskit_bounds(&out, "Style 1")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, style1_pt);
        assert!(
            editor
                .selected_element_appearance()
                .unwrap()
                .style_id
                .is_some()
        );

        let del_pt = collect_accesskit_bounds(&out, "Delete")[0].0.center();
        let (out, _, _) = click_appearance(&mut editor, &ctx, &mut time, del_pt);
        assert!(editor.project.vector_styles().is_empty());

        // Expand appearance button expands live geometry cleanly.
        let expand_pt = collect_accesskit_bounds(&out, "Expand appearance")[0]
            .0
            .center();
        let _ = click_appearance(&mut editor, &ctx, &mut time, expand_pt);
        let key = &editor.project.symbols[0].timeline.layers[0].keyframes[0];
        assert!(key.appearance("rect-1").is_none());
        assert!(key.elements.len() >= 2);
        editor.project.validate().expect("valid after UI workflow");
    }

    #[test]
    fn appearance_ui_paint_color_field_and_repeater_scale_edit() {
        let mut editor = editor_with_selected_rect();
        assert!(editor.appearance_add_modifier(VectorModifier::Repeater {
            copies: 3,
            offset_x: 24.0,
            offset_y: 0.0,
            scale_step: 0.9,
            rotation_step: 15.0,
        }));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());

        run_appearance_panel(&mut editor, &ctx, 0.1, vec![]);
        let (out, texts, _) = run_appearance_panel(&mut editor, &ctx, 0.2, vec![]);
        assert!(
            texts.iter().any(|t| t == "90 %") && texts.iter().any(|t| t == "15 °"),
            "expected Repeater scale % and rotation ° formatted values: {texts:?}"
        );

        // Edit Fill color hex in ColorField and confirm 1-Undo round-trip.
        let fill_color_rect = collect_accesskit_bounds(&out, "Fill color")[0].0;
        let hex_pt = egui::pos2(
            fill_color_rect.center().x - 10.0,
            fill_color_rect.center().y,
        );
        run_appearance_panel(&mut editor, &ctx, 0.3, click_point_events(hex_pt));
        let _ = run_appearance_panel(
            &mut editor,
            &ctx,
            0.4,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text("6C93FF".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let app = editor.selected_element_appearance().unwrap();
        assert!(matches!(
            app.entries[0],
            AppearancePaintEntry::Fill {
                color: [0x6C, 0x93, 0xFF, 255],
                opacity: 1.0,
                ..
            }
        ));
        editor.undo();
        let app_after_undo = editor.selected_element_appearance().unwrap();
        assert!(matches!(
            app_after_undo.entries[0],
            AppearancePaintEntry::Fill {
                color: [200, 60, 40, 255],
                ..
            }
        ));

        // Reorder procedural effects API check + paint combined setter check.
        assert!(editor.appearance_set_entry_paint(0, [10, 20, 30, 255], 0.5));
        assert!(
            editor.appearance_add_procedural_effect(ProceduralVectorEffect::Twist {
                angle_deg: 30.0
            })
        );
        assert!(
            editor.appearance_add_procedural_effect(ProceduralVectorEffect::OffsetPath {
                offset: 4.0
            })
        );
        assert!(editor.appearance_move_procedural_effect(1, 0));
        assert!(matches!(
            editor
                .selected_element_vector_effects()
                .unwrap()
                .procedural_effects[0],
            ProceduralVectorEffect::OffsetPath { offset: 4.0 }
        ));
        editor.undo();
        assert!(matches!(
            editor
                .selected_element_vector_effects()
                .unwrap()
                .procedural_effects[0],
            ProceduralVectorEffect::Twist { angle_deg: 30.0 }
        ));
        editor.project.validate().expect("valid project");
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
            layer: None,
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

        // 6. Live browser verification in headless Google Chrome (when installed).
        // Drive Chrome over the real wall-clock DevTools pipe (`BrowserPipe`) rather than
        // `--virtual-time-budget`: Chromium advances virtual time without waiting for V8's
        // background WASM compiler threads, which races on loaded CI runners.
        #[cfg(unix)]
        {
            let chrome_bin = std::path::Path::new(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            );
            if chrome_bin.exists() {
                let file_url = crate::platform_open::file_url(&html_path).expect("file URL");
                let mut pipe = crate::debugger_transport::BrowserPipe::launch(chrome_bin, true)
                    .expect("launch headless Google Chrome via DevTools pipe");
                pipe.send(&serde_json::json!({
                    "id": 1,
                    "method": "Target.createTarget",
                    "params": { "url": file_url }
                }))
                .expect("send Target.createTarget");

                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let mut session_id: Option<String> = None;
                let mut next_eval_id: u64 = 10;
                let mut pending_eval_id: Option<u64> = None;
                let mut last_eval_sent = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap_or_else(std::time::Instant::now);
                let mut dom = String::new();

                while std::time::Instant::now() < deadline {
                    if let Some(ref sid) = session_id
                        && pending_eval_id.is_none()
                        && last_eval_sent.elapsed() >= std::time::Duration::from_millis(50)
                    {
                        let req_id = next_eval_id;
                        next_eval_id += 1;
                        pipe.send(&serde_json::json!({
                            "id": req_id,
                            "sessionId": sid,
                            "method": "Runtime.evaluate",
                            "params": {
                                "expression": "document.documentElement ? document.documentElement.outerHTML : ''",
                                "returnByValue": true
                            }
                        }))
                        .expect("send Runtime.evaluate");
                        pending_eval_id = Some(req_id);
                        last_eval_sent = std::time::Instant::now();
                    }

                    if let Some(msg) = pipe.receive().expect("receive CDP message") {
                        if msg.get("id").and_then(|v| v.as_u64()) == Some(1) {
                            let target_id = msg["result"]["targetId"]
                                .as_str()
                                .expect("targetId from Target.createTarget");
                            pipe.send(&serde_json::json!({
                                "id": 2,
                                "method": "Target.attachToTarget",
                                "params": { "targetId": target_id, "flatten": true }
                            }))
                            .expect("send Target.attachToTarget");
                        } else if msg.get("id").and_then(|v| v.as_u64()) == Some(2) {
                            session_id = msg["result"]["sessionId"].as_str().map(str::to_owned);
                        } else if pending_eval_id.is_some()
                            && msg.get("id").and_then(|v| v.as_u64()) == pending_eval_id
                        {
                            pending_eval_id = None;
                            if let Some(html) = msg["result"]["result"]["value"].as_str() {
                                dom = html.to_owned();
                                if dom.contains("E2E:false:true:true:false:51") {
                                    break;
                                }
                            }
                        }
                    }
                }

                assert!(
                    dom.contains("id=\"stage-a11y-overlay\"") && dom.contains("Launch Demo Button"),
                    "expected live #stage-a11y-overlay in Chrome DOM, got: {dom}"
                );
                assert!(
                    dom.contains("E2E:false:true:true:false:51"),
                    "expected live script trace 'E2E:false:true:true:false:51' in Chrome DOM, got: {dom}"
                );
            }
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
