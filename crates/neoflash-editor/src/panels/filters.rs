//! Filters panel (`Window ▸ Panels ▸ Filters`, NF2 `Panel / Properties — Movie clip` Display & Filters
//! sections; reference `panel-properties-movie-clip.png`): authored object visibility,
//! bitmap caching, blend mode, and ordered display/keyframe/layer filters (`DropShadow`, `Blur`,
//! `Glow`, `Bevel`, `GradientGlow`, `GradientBevel`, and `Adjust color` → 4×5 `ColorMatrix`).
//!
//! Every edit runs through `Editor::transact` / `begin_property_edit` (`commit_history`),
//! upgrading the document to format 7 when authored display/filter properties are used.
use super::planned::type_header;
use crate::app::Editor;
use crate::paint_controls::{color_row, numeric_pair_with_trailing, two_column_row};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

const INVERT_COLOR_MATRIX: [f64; 20] = [
    -1., 0., 0., 0., 255., 0., -1., 0., 0., 255., 0., 0., -1., 0., 255., 0., 0., 0., 1., 0.,
];

#[allow(dead_code)]
const MAX_STAGE_FILTER_CACHE_ENTRIES: usize = 64;

/// Authoring kind choices in the `+` (`Add filter`) popup menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthoringFilterKind {
    DropShadow,
    Blur,
    Glow,
    Bevel,
    GradientGlow,
    GradientBevel,
    AdjustColor,
}

impl AuthoringFilterKind {
    pub(crate) const ALL: [Self; 7] = [
        Self::DropShadow,
        Self::Blur,
        Self::Glow,
        Self::Bevel,
        Self::GradientGlow,
        Self::GradientBevel,
        Self::AdjustColor,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DropShadow => "Drop shadow",
            Self::Blur => "Blur",
            Self::Glow => "Glow",
            Self::Bevel => "Bevel",
            Self::GradientGlow => "Gradient glow",
            Self::GradientBevel => "Gradient bevel",
            Self::AdjustColor => "Adjust color",
        }
    }

    pub(crate) fn default_spec(self) -> FilterSpec {
        match self {
            Self::DropShadow => FilterSpec::DropShadow {
                distance: 4.0,
                angle: 45.0,
                color: 0x000000,
                alpha: 0.6,
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.0,
                quality: 1,
                inner: false,
                knockout: false,
                hide_object: false,
            },
            Self::Blur => FilterSpec::Blur {
                blur_x: 5.0,
                blur_y: 5.0,
                quality: 1,
            },
            Self::Glow => FilterSpec::Glow {
                color: 0xff0000,
                alpha: 1.0,
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.0,
                quality: 1,
                inner: false,
                knockout: false,
            },
            Self::Bevel => FilterSpec::Bevel {
                distance: 4.0,
                angle: 45.0,
                highlight_color: 0xffffff,
                highlight_alpha: 1.0,
                shadow_color: 0x000000,
                shadow_alpha: 1.0,
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::GradientGlow => FilterSpec::GradientGlow {
                distance: 4.0,
                angle: 45.0,
                colors: vec![0x000000, 0xffffff],
                alphas: vec![0.0, 1.0],
                ratios: vec![0, 255],
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::GradientBevel => FilterSpec::GradientBevel {
                distance: 4.0,
                angle: 45.0,
                colors: vec![0x000000, 0x808080, 0xffffff],
                alphas: vec![1.0, 0.0, 1.0],
                ratios: vec![0, 128, 255],
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::AdjustColor => FilterSpec::ColorMatrix {
                matrix: IDENTITY_COLOR_MATRIX,
            },
        }
    }
}

/// Target scope when no Stage element is selected and an editable timeline layer is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FilterTargetScope {
    #[default]
    Keyframe,
    Layer,
}

impl FilterTargetScope {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Keyframe => "Target: Keyframe",
            Self::Layer => "Target: Layer",
        }
    }
}

/// Named filter stack preset (built-in or user-saved).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FilterPreset {
    pub name: String,
    pub filters: Vec<FilterSpec>,
}

/// Authoring Adjust Color parameters remembered per `(target_key, filter_index)` and recovered from `ColorMatrix`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AdjustColorSliders {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub hue: f64,
}

impl AdjustColorSliders {
    pub(crate) fn from_matrix(matrix: &[f64; 20]) -> Self {
        let (brightness, contrast, saturation, hue) =
            FilterSpec::recover_adjust_color_sliders(matrix).unwrap_or_default();
        Self {
            brightness,
            contrast,
            saturation,
            hue,
        }
    }

    pub(crate) fn to_matrix(self) -> [f64; 20] {
        adjust_color_matrix(self.brightness, self.contrast, self.saturation, self.hue)
    }
}

#[derive(Clone, Debug, Default)]
struct FiltersPanelState {
    selected_target: Option<String>,
    selected_index: usize,
    timeline_scope: FilterTargetScope,
    adjust: HashMap<(String, usize), (AdjustColorSliders, [f64; 20])>,
    blur_locked: HashMap<(String, usize), bool>,
    gradient_stop: HashMap<(String, usize), usize>,
    preset_draft_name: String,
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-filters-panel")
}

#[derive(Default)]
struct SharedFilterClipboardAndPresets {
    copied_filters: Vec<FilterSpec>,
    custom_presets: Vec<FilterPreset>,
    timeline_scope: FilterTargetScope,
}

fn shared_filter_store() -> &'static Mutex<SharedFilterClipboardAndPresets> {
    static STORE: OnceLock<Mutex<SharedFilterClipboardAndPresets>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(SharedFilterClipboardAndPresets::default()))
}

fn built_in_filter_presets() -> Vec<FilterPreset> {
    vec![
        FilterPreset {
            name: "Soft Drop Shadow".into(),
            filters: vec![FilterSpec::DropShadow {
                distance: 6.0,
                angle: 45.0,
                color: 0x000000,
                alpha: 0.5,
                blur_x: 8.0,
                blur_y: 8.0,
                strength: 1.0,
                quality: 2,
                inner: false,
                knockout: false,
                hide_object: false,
            }],
        },
        FilterPreset {
            name: "Neon Outer Glow".into(),
            filters: vec![FilterSpec::Glow {
                color: 0x00d8ff,
                alpha: 0.9,
                blur_x: 12.0,
                blur_y: 12.0,
                strength: 2.0,
                quality: 2,
                inner: false,
                knockout: false,
            }],
        },
        FilterPreset {
            name: "Embossed Bevel".into(),
            filters: vec![FilterSpec::Bevel {
                distance: 4.0,
                angle: 45.0,
                highlight_color: 0xffffff,
                highlight_alpha: 0.9,
                shadow_color: 0x10141c,
                shadow_alpha: 0.9,
                blur_x: 5.0,
                blur_y: 5.0,
                strength: 1.2,
                quality: 2,
                placement: "inner".into(),
                knockout: false,
            }],
        },
    ]
}

#[allow(dead_code)]
#[derive(Clone)]
struct CachedFilteredRaster {
    key: (u64, u32, u32, u64),
    out_w: u32,
    out_h: u32,
    rgba: Vec<u8>,
    padding: FilterPadding,
}

#[allow(dead_code)]
fn stage_filter_cache() -> &'static Mutex<VecDeque<CachedFilteredRaster>> {
    static CACHE: OnceLock<Mutex<VecDeque<CachedFilteredRaster>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[allow(dead_code)]
fn hash_filter_input(pixels: &[u8], filters: &[FilterSpec]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.len().hash(&mut hasher);
    let step = (pixels.len() / 256).max(1);
    for (idx, byte) in pixels.iter().step_by(step).enumerate() {
        (idx, *byte).hash(&mut hasher);
    }
    if let Ok(json) = serde_json::to_string(filters) {
        json.hash(&mut hasher);
    }
    hasher.finish()
}

/// Applies `filters` to an unpremultiplied RGBA buffer `(width, height)` with a bounded
/// 64-entry LRU cache, returning `(out_width, out_height, out_rgba, padding)`.
#[allow(dead_code)]
pub(crate) fn apply_stage_filters_cached(
    cache_key: u64,
    width: u32,
    height: u32,
    pixels: &[u8],
    filters: &[FilterSpec],
) -> Option<(u32, u32, Vec<u8>, FilterPadding)> {
    if width == 0 || height == 0 || !filters.iter().any(FilterSpec::is_enabled) {
        return None;
    }
    let content_hash = hash_filter_input(pixels, filters);
    let full_key = (cache_key, width, height, content_hash);
    if let Ok(mut guard) = stage_filter_cache().lock()
        && let Some(pos) = guard.iter().position(|entry| entry.key == full_key)
    {
        let hit = guard.remove(pos).unwrap();
        let res = (hit.out_w, hit.out_h, hit.rgba.clone(), hit.padding);
        guard.push_back(hit);
        return Some(res);
    }
    let (out_w, out_h, padding, rgba) =
        FilterSpec::apply_filters_rgba(width, height, pixels, filters).ok()?;
    if let Ok(mut guard) = stage_filter_cache().lock() {
        if guard.len() >= MAX_STAGE_FILTER_CACHE_ENTRIES {
            guard.pop_front();
        }
        guard.push_back(CachedFilteredRaster {
            key: full_key,
            out_w,
            out_h,
            rgba: rgba.clone(),
            padding,
        });
    }
    Some((out_w, out_h, rgba, padding))
}

/// Uploads a filtered element/layer RGBA raster to an `egui::TextureHandle` with bounded caching
/// so Stage rendering can composite live filters in a single call.
#[allow(dead_code)]
pub(crate) fn render_filtered_element_texture(
    ctx: &egui::Context,
    cache_key: u64,
    width: u32,
    height: u32,
    pixels: &[u8],
    filters: &[FilterSpec],
) -> Option<(egui::TextureHandle, FilterPadding)> {
    let (out_w, out_h, rgba, padding) =
        apply_stage_filters_cached(cache_key, width, height, pixels, filters)?;
    let image = egui::ColorImage::from_rgba_unmultiplied([out_w as usize, out_h as usize], &rgba);
    let tex = ctx.load_texture(
        format!("nf2-stage-filter-{cache_key}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    Some((tex, padding))
}

fn quality_label(quality: u8) -> &'static str {
    match quality {
        0 | 1 => "Quality: Low",
        2 => "Quality: Medium",
        3 => "Quality: High",
        _ => "Quality: Custom",
    }
}

fn placement_label(placement: &str) -> &'static str {
    match placement {
        "outer" => "Type: Outer",
        "full" => "Type: Full",
        _ => "Type: Inner",
    }
}

fn rgb_u32_and_alpha_to_color(rgb: u32, alpha: f64) -> Color {
    [
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn color_to_rgb_u32_and_alpha(c: Color) -> (u32, f64) {
    let rgb = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
    let alpha = (c[3] as f64 / 255.0).clamp(0.0, 1.0);
    (rgb, alpha)
}

fn gradient_arrays_to_stops(
    colors: &[u32],
    alphas: &[f64],
    ratios: &[u8],
) -> Vec<(f32, egui::Color32)> {
    let count = colors.len().min(alphas.len()).min(ratios.len());
    if count == 0 {
        return vec![(0.0, egui::Color32::BLACK), (1.0, egui::Color32::WHITE)];
    }
    (0..count)
        .map(|i| {
            let t = (ratios[i] as f32 / 255.0).clamp(0.0, 1.0);
            let rgba = rgb_u32_and_alpha_to_color(colors[i], alphas[i]);
            (
                t,
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
            )
        })
        .collect()
}

fn stops_to_gradient_arrays(stops: &[(f32, egui::Color32)]) -> (Vec<u32>, Vec<f64>, Vec<u8>) {
    let mut colors = Vec::with_capacity(stops.len().clamp(2, 16));
    let mut alphas = Vec::with_capacity(stops.len().clamp(2, 16));
    let mut ratios = Vec::with_capacity(stops.len().clamp(2, 16));
    for &(pos, c32) in stops.iter().take(16) {
        let rgba = c32.to_srgba_unmultiplied();
        let (rgb, alpha) = color_to_rgb_u32_and_alpha(rgba);
        let r = (pos.clamp(0.0, 1.0) * 255.0).round() as u8;
        colors.push(rgb);
        alphas.push(alpha);
        ratios.push(r);
    }
    while colors.len() < 2 {
        colors.push(0xffffff);
        alphas.push(1.0);
        ratios.push(255);
    }
    for i in 1..ratios.len() {
        ratios[i] = ratios[i].max(ratios[i - 1]);
    }
    (colors, alphas, ratios)
}

fn filter_blur_mut(spec: &mut FilterSpec) -> Option<(&mut f64, &mut f64, &mut u8)> {
    match spec {
        FilterSpec::Blur {
            blur_x,
            blur_y,
            quality,
        }
        | FilterSpec::Glow {
            blur_x,
            blur_y,
            quality,
            ..
        }
        | FilterSpec::DropShadow {
            blur_x,
            blur_y,
            quality,
            ..
        }
        | FilterSpec::Bevel {
            blur_x,
            blur_y,
            quality,
            ..
        }
        | FilterSpec::GradientGlow {
            blur_x,
            blur_y,
            quality,
            ..
        }
        | FilterSpec::GradientBevel {
            blur_x,
            blur_y,
            quality,
            ..
        } => Some((blur_x, blur_y, quality)),
        FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => None,
    }
}

fn filter_strength_mut(spec: &mut FilterSpec) -> Option<&mut f64> {
    match spec {
        FilterSpec::Glow { strength, .. }
        | FilterSpec::DropShadow { strength, .. }
        | FilterSpec::Bevel { strength, .. }
        | FilterSpec::GradientGlow { strength, .. }
        | FilterSpec::GradientBevel { strength, .. } => Some(strength),
        FilterSpec::Blur { .. }
        | FilterSpec::ColorMatrix { .. }
        | FilterSpec::Convolution { .. } => None,
    }
}

impl Editor {
    pub(crate) fn filter_target_scope(&self) -> FilterTargetScope {
        shared_filter_store()
            .lock()
            .map(|s| s.timeline_scope)
            .unwrap_or_default()
    }

    pub(crate) fn set_filter_target_scope(&mut self, scope: FilterTargetScope) {
        if let Ok(mut guard) = shared_filter_store().lock() {
            guard.timeline_scope = scope;
        }
    }

    fn timeline_filter_target_editable(&self) -> bool {
        self.selection.is_empty()
            && self.layer_editable()
            && self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|layer| {
                    layer.properties.role != LayerRole::Guide
                        && self.is_layer_id_effectively_soloed(&layer.id)
                })
    }

    pub(crate) fn active_keyframe_filters(&self) -> Vec<FilterSpec> {
        self.symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.key_at(self.frame))
            .map(|k| k.filters.clone())
            .unwrap_or_default()
    }

    pub(crate) fn active_layer_filters(&self) -> Vec<FilterSpec> {
        self.symbol()
            .timeline
            .layers
            .get(self.layer)
            .map(|l| l.properties.filters.clone())
            .unwrap_or_default()
    }

    pub(crate) fn current_filter_stack(&self) -> Option<Vec<FilterSpec>> {
        if self.selection.len() == 1 {
            return Some(self.selected_sampled()?.filters.clone());
        }
        if self.timeline_filter_target_editable() {
            return Some(match self.filter_target_scope() {
                FilterTargetScope::Keyframe => self.active_keyframe_filters(),
                FilterTargetScope::Layer => self.active_layer_filters(),
            });
        }
        None
    }

    fn mutate_selected_display(
        &mut self,
        control: Option<egui::Id>,
        f: impl FnOnce(&mut Element),
    ) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        self.set_playing(false);
        if let Some(control) = control {
            self.begin_property_edit(egui::Id::new("filters-panel-edit"), control);
            self.select_single_id(sampled.id);
            self.ensure_selected_key();
            if let Some(element) = self.selected_element_mut() {
                f(element);
            }
            self.project.upgrade_format_for_features();
            self.reset_authored_preview();
        } else {
            self.finish_edits();
            self.select_single_id(sampled.id);
            self.transact(|editor| {
                editor.ensure_selected_key();
                if let Some(element) = editor.selected_element_mut() {
                    f(element);
                }
            });
        }
        true
    }

    fn apply_timeline_filter_mutation(
        &mut self,
        scope: FilterTargetScope,
        f: impl FnOnce(&mut Vec<FilterSpec>),
    ) {
        let layer_idx = self.layer;
        let frame = self.frame;
        if scope == FilterTargetScope::Keyframe
            && self.workspace.timeline_prefs.auto_keyframe
            && let Some(layer_id) = self
                .symbol()
                .timeline
                .layers
                .get(layer_idx)
                .filter(|l| !l.keyframes.iter().any(|k| k.frame == frame))
                .map(|l| l.id.clone())
        {
            self.ensure_key_for(&layer_id, false);
        }
        if let Some(layer) = self.symbol_mut().timeline.layers.get_mut(layer_idx) {
            match scope {
                FilterTargetScope::Keyframe => {
                    if let Some(key) = layer.key_at_mut(frame) {
                        f(&mut key.filters);
                    }
                }
                FilterTargetScope::Layer => {
                    f(&mut layer.properties.filters);
                }
            }
        }
    }

    fn mutate_active_filter_stack(
        &mut self,
        control: Option<egui::Id>,
        f: impl FnOnce(&mut Vec<FilterSpec>),
    ) -> bool {
        if self.selection.len() == 1 {
            return self.mutate_selected_display(control, |element| f(&mut element.filters));
        }
        if !self.timeline_filter_target_editable() {
            return false;
        }
        let scope = self.filter_target_scope();
        self.set_playing(false);
        if let Some(control) = control {
            self.begin_property_edit(egui::Id::new("filters-panel-timeline-edit"), control);
            self.apply_timeline_filter_mutation(scope, f);
            self.project.upgrade_format_for_features();
            self.reset_authored_preview();
        } else {
            self.finish_edits();
            self.transact(|editor| {
                editor.apply_timeline_filter_mutation(scope, f);
            });
        }
        true
    }

    pub(crate) fn set_selected_visible(&mut self, visible: bool) -> bool {
        self.mutate_selected_display(None, |element| element.visible = visible)
    }

    pub(crate) fn set_selected_cache_as_bitmap(&mut self, cache_as_bitmap: bool) -> bool {
        self.mutate_selected_display(None, |element| element.cache_as_bitmap = cache_as_bitmap)
    }

    pub(crate) fn set_selected_blend_mode(&mut self, blend_mode: BlendMode) -> bool {
        if self.selection.is_empty() {
            return self.set_layer_blend_mode(self.layer, blend_mode);
        }
        self.mutate_selected_display(None, |element| element.blend_mode = blend_mode)
    }

    pub(crate) fn add_selected_filter(&mut self, kind: AuthoringFilterKind) -> Option<usize> {
        let current_len = self.current_filter_stack()?.len();
        if current_len >= MAX_OBJECT_FILTERS {
            return None;
        }
        let spec = kind.default_spec();
        let mut added = None;
        if self.mutate_active_filter_stack(None, |filters| {
            if filters.len() < MAX_OBJECT_FILTERS {
                filters.push(spec);
                added = Some(filters.len() - 1);
            }
        }) {
            added
        } else {
            None
        }
    }

    pub(crate) fn duplicate_selected_filter(&mut self, index: usize) -> Option<usize> {
        let stack = self.current_filter_stack()?;
        if index >= stack.len() || stack.len() >= MAX_OBJECT_FILTERS {
            return None;
        }
        let mut inserted = None;
        if self.mutate_active_filter_stack(None, |filters| {
            if index < filters.len() && filters.len() < MAX_OBJECT_FILTERS {
                let copy = filters[index].clone();
                filters.insert(index + 1, copy);
                inserted = Some(index + 1);
            }
        }) {
            inserted
        } else {
            None
        }
    }

    pub(crate) fn remove_selected_filter(&mut self, index: usize) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if index >= stack.len() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            if index < filters.len() {
                filters.remove(index);
            }
        })
    }

    pub(crate) fn update_selected_filter(
        &mut self,
        index: usize,
        spec: FilterSpec,
        control: Option<egui::Id>,
    ) -> bool {
        if spec.validate().is_err() {
            return false;
        }
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if index >= stack.len() {
            return false;
        }
        self.mutate_active_filter_stack(control, |filters| {
            if let Some(slot) = filters.get_mut(index) {
                *slot = spec;
            }
        })
    }

    pub(crate) fn set_selected_filter_enabled(&mut self, index: usize, enabled: bool) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if index >= stack.len() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            if let Some(slot) = filters.get_mut(index) {
                slot.set_enabled(enabled);
            }
        })
    }

    pub(crate) fn set_all_selected_filters_enabled(&mut self, enabled: bool) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if stack.is_empty() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            for slot in filters.iter_mut() {
                slot.set_enabled(enabled);
            }
        })
    }

    pub(crate) fn reset_selected_filter(&mut self, index: usize) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if index >= stack.len() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            if let Some(slot) = filters.get_mut(index) {
                slot.reset_to_default();
            }
        })
    }

    pub(crate) fn clear_selected_filters(&mut self) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if stack.is_empty() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            filters.clear();
        })
    }

    pub(crate) fn move_selected_filter(&mut self, from: usize, to: usize) -> Option<usize> {
        let stack = self.current_filter_stack()?;
        if from >= stack.len() || to >= stack.len() || from == to {
            return None;
        }
        if self.mutate_active_filter_stack(None, |filters| {
            if from < filters.len() && to < filters.len() {
                let item = filters.remove(from);
                filters.insert(to, item);
            }
        }) {
            Some(to)
        } else {
            None
        }
    }

    pub(crate) fn copy_selected_filters(&mut self) -> bool {
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if stack.is_empty() {
            return false;
        }
        if let Ok(mut guard) = shared_filter_store().lock() {
            guard.copied_filters = stack;
            true
        } else {
            false
        }
    }

    pub(crate) fn paste_selected_filters(&mut self) -> bool {
        let copied = shared_filter_store()
            .lock()
            .map(|g| g.copied_filters.clone())
            .unwrap_or_default();
        if copied.is_empty() {
            return false;
        }
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if stack.len() >= MAX_OBJECT_FILTERS {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            for spec in copied {
                if filters.len() < MAX_OBJECT_FILTERS {
                    filters.push(spec);
                }
            }
        })
    }

    pub(crate) fn filter_presets(&self) -> Vec<FilterPreset> {
        let mut presets = built_in_filter_presets();
        if let Ok(guard) = shared_filter_store().lock() {
            presets.extend(guard.custom_presets.iter().cloned());
        }
        presets
    }

    pub(crate) fn save_filter_preset(&mut self, name: impl Into<String>) -> bool {
        let name = name.into().trim().to_string();
        if name.is_empty() {
            return false;
        }
        let Some(stack) = self.current_filter_stack() else {
            return false;
        };
        if stack.is_empty() {
            return false;
        }
        if let Ok(mut guard) = shared_filter_store().lock() {
            if let Some(existing) = guard.custom_presets.iter_mut().find(|p| p.name == name) {
                existing.filters = stack;
            } else {
                guard.custom_presets.push(FilterPreset {
                    name,
                    filters: stack,
                });
            }
            true
        } else {
            false
        }
    }

    pub(crate) fn apply_filter_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self.filter_presets().into_iter().find(|p| p.name == name) else {
            return false;
        };
        if validate_filters(&preset.filters).is_err() {
            return false;
        }
        self.mutate_active_filter_stack(None, |filters| {
            *filters = preset
                .filters
                .into_iter()
                .take(MAX_OBJECT_FILTERS)
                .collect();
        })
    }

    pub(crate) fn delete_filter_preset(&mut self, name: &str) -> bool {
        if let Ok(mut guard) = shared_filter_store().lock() {
            let before = guard.custom_presets.len();
            guard.custom_presets.retain(|p| p.name != name);
            guard.custom_presets.len() != before
        } else {
            false
        }
    }

    fn selected_type_header(&self) -> (neoflash_ui::Icon, &'static str, String) {
        if self.selection.len() > 1 {
            return (
                i16::FILTER,
                "Multiple selection",
                format!("{} objects selected", self.selection.len()),
            );
        }
        let Some(element) = self.selected_sampled() else {
            if self.timeline_filter_target_editable()
                && let Some(layer) = self.symbol().timeline.layers.get(self.layer)
            {
                return match self.filter_target_scope() {
                    FilterTargetScope::Keyframe => (
                        i16::FRAME,
                        "Frame",
                        format!("Layer \"{}\" · Frame {}", layer.name, self.frame + 1),
                    ),
                    FilterTargetScope::Layer => {
                        (i16::LAYERS, "Layer", format!("Layer \"{}\"", layer.name))
                    }
                };
            }
            return (
                i16::FILTER,
                "No selection",
                "Select an object on the Stage".into(),
            );
        };
        match &element.kind {
            ElementKind::Instance { symbol_id, .. } => match self.project.symbol(symbol_id) {
                Some(sym) => {
                    let (icon, title) = match sym.kind {
                        SymbolKind::MovieClip => (i16::MOVIECLIP, "Movie clip"),
                        SymbolKind::Button => (i16::BUTTON_SYMBOL, "Button"),
                        SymbolKind::Graphic => (i16::GRAPHIC, "Graphic"),
                    };
                    (icon, title, format!("Instance of \"{}\"", sym.name))
                }
                None => (
                    i16::SYMBOL,
                    "Instance",
                    format!("Instance of \"{symbol_id}\""),
                ),
            },
            ElementKind::Text { .. } | ElementKind::TextBox { .. } => (
                i16::TEXT,
                "Text",
                if element.name.is_empty() {
                    "Text object".into()
                } else {
                    format!("\"{}\"", element.name)
                },
            ),
            ElementKind::Bitmap { .. } => (
                i16::BITMAP,
                "Bitmap",
                if element.name.is_empty() {
                    "Bitmap placement".into()
                } else {
                    format!("\"{}\"", element.name)
                },
            ),
            ElementKind::Video { .. } => (
                i16::VIDEO,
                "Video",
                if element.name.is_empty() {
                    "Video viewport".into()
                } else {
                    format!("\"{}\"", element.name)
                },
            ),
            ElementKind::Rectangle { .. } => (i16::RECT, "Rectangle", "Vector shape".into()),
            ElementKind::Ellipse { .. } => (i16::OVAL, "Oval", "Vector shape".into()),
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => {
                (i16::PEN, "Shape", "Vector path".into())
            }
        }
    }

    /// Window ▸ Panels ▸ Filters.
    pub(crate) fn filters_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::Filters, "Filters");
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .id_salt("filters-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                self.filters_panel(ui);
            });
        self.end_property_interaction(&ctx);
    }

    /// Panel / Filters body (bound to the selected `Element`'s or active Keyframe/Layer's `filters`).
    fn filters_panel(&mut self, ui: &mut Ui) {
        let (header_icon, header_title, header_sub) = self.selected_type_header();
        type_header(ui, header_icon, header_title, &header_sub);
        self.display_and_filters_sections_ui(ui, "filters", true, true);
    }

    /// Reusable `Display` and `Filters` collapsible sections used both in `Window ▸ Panels ▸ Filters`
    /// and inline inside `Properties` (`Properties — Movie clip` and `Properties — Text`).
    pub(crate) fn display_and_filters_sections_ui(
        &mut self,
        ui: &mut Ui,
        id_prefix: &'static str,
        show_display: bool,
        default_open_filters: bool,
    ) {
        let ctx = ui.ctx().clone();
        let mut state: FiltersPanelState = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
        state.timeline_scope = self.filter_target_scope();

        let sampled = if self.selection.len() == 1 {
            self.selected_sampled()
        } else {
            None
        };
        let element_editable = sampled.is_some() && self.selected_editable();
        let timeline_editable = self.timeline_filter_target_editable();
        let filters_editable = element_editable || timeline_editable;

        let target_key = if let Some(el) = &sampled {
            Some(format!("el:{}", el.id))
        } else if timeline_editable {
            Some(match state.timeline_scope {
                FilterTargetScope::Keyframe => {
                    format!("kf:{}:{}:{}", self.symbol_id(), self.layer, self.frame)
                }
                FilterTargetScope::Layer => format!("layer:{}:{}", self.symbol_id(), self.layer),
            })
        } else {
            None
        };

        if state.selected_target != target_key {
            state.selected_target = target_key.clone();
            state.selected_index = 0;
        }
        let filters = self.current_filter_stack().unwrap_or_default();
        if !filters.is_empty() && state.selected_index >= filters.len() {
            state.selected_index = filters.len() - 1;
        } else if filters.is_empty() {
            state.selected_index = 0;
        }

        let mut visible = sampled.as_ref().is_none_or(|e| e.visible);
        let mut cache_as_bitmap = sampled.as_ref().is_some_and(|e| e.cache_as_bitmap);
        let layer_blend = self.layer_blend_mode(self.layer);
        let blend_mode = sampled.as_ref().map_or(layer_blend, |e| {
            if e.blend_mode.is_normal() {
                layer_blend
            } else {
                e.blend_mode
            }
        });
        let layer_blend_editable = self.selection.is_empty()
            && self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| l.properties.role != LayerRole::Guide);

        let mut next_visible = None;
        let mut next_cache = None;
        let mut next_blend = None;
        let mut next_scope = None;
        let mut add_kind = None;
        let mut duplicate_clicked = false;
        let mut remove_clicked = false;
        let mut toggle_enabled_clicked: Option<(usize, bool)> = None;
        let mut move_filter: Option<(usize, usize)> = None;
        let mut enable_all_action: Option<bool> = None;
        let mut reset_filter_clicked: Option<usize> = None;
        let mut clear_all_clicked = false;
        let mut copy_filters_clicked = false;
        let mut paste_filters_clicked = false;
        let mut apply_preset_name: Option<String> = None;
        let mut save_preset_name: Option<String> = None;
        let mut delete_preset_name: Option<String> = None;
        let mut filter_edit: Option<(usize, FilterSpec, Option<egui::Id>)> = None;

        if show_display {
            ui.add_enabled_ui_semantic(element_editable || layer_blend_editable, |ui| {
                c::SectionHeader::new("Display")
                    .id_salt((id_prefix, "filters-mc-display"))
                    .show(ui, |ui| {
                        ui.add_enabled_ui_semantic(element_editable, |ui| {
                            ui.horizontal(|ui| {
                                if ui.add(c::Switch::new(&mut visible, "Visible")).changed() {
                                    next_visible = Some(visible);
                                }
                                if ui
                                    .add(c::Checkbox::new(&mut cache_as_bitmap, "Cache as bitmap"))
                                    .changed()
                                {
                                    next_cache = Some(cache_as_bitmap);
                                }
                            });
                        });
                        two_column_row(
                            ui,
                            |ui, width| {
                                let blend_label = format!("Blending: {}", blend_mode.label());
                                c::Select::new((id_prefix, "filters-blending"), &blend_label)
                                    .width(width)
                                    .show(ui, |ui| {
                                        for mode in BlendMode::ALL {
                                            if ui
                                                .add(
                                                    c::MenuItem::new(mode.label())
                                                        .checked(mode == blend_mode),
                                                )
                                                .clicked()
                                            {
                                                next_blend = Some(mode);
                                                ui.close();
                                            }
                                        }
                                    });
                            },
                            |ui, width| {
                                if sampled.is_none() && timeline_editable {
                                    c::Select::new(
                                        (id_prefix, "filters-target-scope"),
                                        state.timeline_scope.label(),
                                    )
                                    .width(width)
                                    .show(ui, |ui| {
                                        for (scope, label) in [
                                            (FilterTargetScope::Keyframe, "Keyframe filters"),
                                            (FilterTargetScope::Layer, "Layer filters"),
                                        ] {
                                            if ui
                                                .add(
                                                    c::MenuItem::new(label)
                                                        .checked(state.timeline_scope == scope),
                                                )
                                                .clicked()
                                            {
                                                next_scope = Some(scope);
                                                ui.close();
                                            }
                                        }
                                    });
                                } else {
                                    ui.add_enabled_ui_semantic(element_editable, |ui| {
                                        let render_label = if cache_as_bitmap {
                                            "Render: Export as bitmap"
                                        } else {
                                            "Render: Original"
                                        };
                                        c::Select::new((id_prefix, "filters-render"), render_label)
                                            .width(width)
                                            .show(ui, |ui| {
                                                for (cached, label) in [
                                                    (false, "Original"),
                                                    (true, "Export as bitmap"),
                                                ] {
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(label)
                                                                .checked(cache_as_bitmap == cached),
                                                        )
                                                        .clicked()
                                                    {
                                                        next_cache = Some(cached);
                                                        ui.close();
                                                    }
                                                }
                                            });
                                    });
                                }
                            },
                        );
                    });
            });
        }

        ui.add_enabled_ui_semantic(filters_editable, |ui| {
            c::SectionHeader::new("Filters")
                .id_salt((id_prefix, "filters-mc-filters"))
                .default_open(default_open_filters)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let cols = &[0.0, 0.5];
                    ui.add(c::TableRow {
                        cells: &["Filter", "Value"],
                        kind: c::TableRowKind::Header,
                        columns: cols,
                    });
                    for (idx, spec) in filters.iter().enumerate() {
                        let base_summary = spec.summary_value();
                        let value_summary = if spec.is_enabled() {
                            base_summary
                        } else {
                            format!("Off · {base_summary}")
                        };
                        let kind = if idx == state.selected_index {
                            c::TableRowKind::Selected
                        } else {
                            c::TableRowKind::Row
                        };
                        if ui
                            .add(c::TableRow {
                                cells: &[spec.display_name(), &value_summary],
                                kind,
                                columns: cols,
                            })
                            .clicked()
                        {
                            state.selected_index = idx;
                        }
                    }

                    ui.add_space(space::S2);
                    ui.spacing_mut().item_spacing.y = space::S2;

                    let idx = state.selected_index;
                    let key_id = (target_key.clone().unwrap_or_default(), idx);
                    if let Some(mut active) = filters.get(idx).cloned() {
                        let was_enabled = active.is_enabled();
                        match &mut active {
                            FilterSpec::ColorMatrix { matrix } => {
                                let mut sliders = match state.adjust.get(&key_id) {
                                    Some((cached_sliders, cached_matrix))
                                        if cached_matrix == matrix =>
                                    {
                                        *cached_sliders
                                    }
                                    _ => {
                                        let recovered = AdjustColorSliders::from_matrix(matrix);
                                        state.adjust.insert(key_id.clone(), (recovered, *matrix));
                                        recovered
                                    }
                                };
                                let mut changed_id = None;
                                let (r1, r2) = two_column_row(
                                    ui,
                                    |ui, width| {
                                        ui.add(
                                            c::Field::new(&mut sliders.brightness, "Brightness")
                                                .prefix("Brightness")
                                                .decimals(0)
                                                .range(-100.0..=100.0)
                                                .width(width),
                                        )
                                    },
                                    |ui, width| {
                                        ui.add(
                                            c::Field::new(&mut sliders.contrast, "Contrast")
                                                .prefix("Contrast")
                                                .decimals(0)
                                                .range(-100.0..=100.0)
                                                .width(width),
                                        )
                                    },
                                );
                                if r1.changed() {
                                    changed_id = Some(r1.id);
                                }
                                if r2.changed() {
                                    changed_id = Some(r2.id);
                                }
                                let (r3, r4) = two_column_row(
                                    ui,
                                    |ui, width| {
                                        ui.add(
                                            c::Field::new(&mut sliders.saturation, "Saturation")
                                                .prefix("Saturation")
                                                .unit("%")
                                                .decimals(0)
                                                .range(-100.0..=100.0)
                                                .width(width),
                                        )
                                    },
                                    |ui, width| {
                                        ui.add(
                                            c::Field::new(&mut sliders.hue, "Hue")
                                                .prefix("Hue")
                                                .unit("°")
                                                .decimals(0)
                                                .range(-180.0..=180.0)
                                                .width(width),
                                        )
                                    },
                                );
                                if r3.changed() {
                                    changed_id = Some(r3.id);
                                }
                                if r4.changed() {
                                    changed_id = Some(r4.id);
                                }
                                let preset_label = if *matrix == IDENTITY_COLOR_MATRIX {
                                    "Preset: Identity"
                                } else if *matrix == INVERT_COLOR_MATRIX {
                                    "Preset: Invert"
                                } else {
                                    "Preset: Custom"
                                };
                                let mut picked_preset = None;
                                let mut reset_adjust = false;
                                two_column_row(
                                    ui,
                                    |ui, width| {
                                        c::Select::new(
                                            (id_prefix, "filters-color-preset"),
                                            preset_label,
                                        )
                                        .width(width)
                                        .show(ui, |ui| {
                                            for (name, vals, exact) in [
                                                (
                                                    "Identity",
                                                    AdjustColorSliders::default(),
                                                    Some(IDENTITY_COLOR_MATRIX),
                                                ),
                                                (
                                                    "Grayscale",
                                                    AdjustColorSliders {
                                                        saturation: -100.0,
                                                        ..Default::default()
                                                    },
                                                    None,
                                                ),
                                                (
                                                    "High contrast",
                                                    AdjustColorSliders {
                                                        contrast: 40.0,
                                                        ..Default::default()
                                                    },
                                                    None,
                                                ),
                                                (
                                                    "Warm hue (+30°)",
                                                    AdjustColorSliders {
                                                        hue: 30.0,
                                                        ..Default::default()
                                                    },
                                                    None,
                                                ),
                                                (
                                                    "Cool hue (-30°)",
                                                    AdjustColorSliders {
                                                        hue: -30.0,
                                                        ..Default::default()
                                                    },
                                                    None,
                                                ),
                                                (
                                                    "Invert",
                                                    AdjustColorSliders::default(),
                                                    Some(INVERT_COLOR_MATRIX),
                                                ),
                                            ] {
                                                if ui.add(c::MenuItem::new(name)).clicked() {
                                                    let m =
                                                        exact.unwrap_or_else(|| vals.to_matrix());
                                                    picked_preset = Some((vals, m));
                                                    ui.close();
                                                }
                                            }
                                        })
                                    },
                                    |ui, _width| {
                                        if ui
                                            .add(
                                                c::Button::ghost("Reset")
                                                    .icon(i16::RESET_TRANSFORM),
                                            )
                                            .clicked()
                                        {
                                            reset_adjust = true;
                                        }
                                    },
                                );
                                if reset_adjust {
                                    let vals = AdjustColorSliders::default();
                                    let m = IDENTITY_COLOR_MATRIX;
                                    state.adjust.insert(key_id, (vals, m));
                                    filter_edit = Some((
                                        idx,
                                        FilterSpec::ColorMatrix { matrix: m }
                                            .with_enabled(was_enabled),
                                        None,
                                    ));
                                } else if let Some((vals, m)) = picked_preset {
                                    state.adjust.insert(key_id, (vals, m));
                                    filter_edit = Some((
                                        idx,
                                        FilterSpec::ColorMatrix { matrix: m }
                                            .with_enabled(was_enabled),
                                        None,
                                    ));
                                } else if let Some(cid) = changed_id {
                                    let m = sliders.to_matrix();
                                    state.adjust.insert(key_id, (sliders, m));
                                    filter_edit = Some((
                                        idx,
                                        FilterSpec::ColorMatrix { matrix: m }
                                            .with_enabled(was_enabled),
                                        Some(cid),
                                    ));
                                }
                            }
                            _ => {
                                let mut changed_id = None;
                                let mut discrete_changed = false;

                                if let Some((bx, by, q)) = filter_blur_mut(&mut active) {
                                    let mut blur_x = *bx;
                                    let mut blur_y = *by;
                                    let cur_q = (*q & 0x7f).clamp(1, 3);
                                    let mut locked = state
                                        .blur_locked
                                        .get(&key_id)
                                        .copied()
                                        .unwrap_or_else(|| (blur_x - blur_y).abs() <= 1e-6);
                                    let ([rx, ry], lock_btn) = numeric_pair_with_trailing(
                                        ui,
                                        ["Blur X", "Blur Y"],
                                        [&mut blur_x, &mut blur_y],
                                        0.0..=255.0,
                                        0.5,
                                        " px",
                                        (24.0, |ui| {
                                            ui.add(
                                                c::IconButton::new(
                                                    i16::LINK,
                                                    "Lock Blur X and Blur Y",
                                                )
                                                .selected(locked)
                                                .side(24.0),
                                            )
                                        }),
                                    );
                                    if lock_btn.clicked() {
                                        locked = !locked;
                                        state.blur_locked.insert(key_id.clone(), locked);
                                        if locked && (blur_x - blur_y).abs() > 1e-6 {
                                            blur_y = blur_x;
                                            *by = blur_y;
                                            discrete_changed = true;
                                        }
                                    }
                                    if rx.changed() {
                                        *bx = blur_x;
                                        if locked {
                                            *by = blur_x;
                                        }
                                        changed_id = Some(rx.id);
                                    } else if ry.changed() {
                                        *by = blur_y;
                                        if locked {
                                            *bx = blur_y;
                                        }
                                        changed_id = Some(ry.id);
                                    }
                                    state.blur_locked.insert(key_id.clone(), locked);

                                    let mut picked_quality = None;
                                    let strength = filter_strength_mut(&mut active).map(|st| *st);
                                    let mut pct = strength.unwrap_or(1.0) * 100.0;
                                    let (rs, _) =
                                        two_column_row(
                                            ui,
                                            |ui, width| {
                                                let field = c::Field::new(&mut pct, "Strength")
                                                    .prefix("Strength")
                                                    .unit("%")
                                                    .decimals(0)
                                                    .range(0.0..=25500.0)
                                                    .width(width);
                                                ui.add_enabled(strength.is_some(), field)
                                            },
                                            |ui, width| {
                                                c::Select::new(
                                                    (id_prefix, "filters-quality"),
                                                    quality_label(cur_q),
                                                )
                                                .width(width)
                                                .show(ui, |ui| {
                                                    for (level, label) in
                                                        [(1u8, "Low"), (2, "Medium"), (3, "High")]
                                                    {
                                                        if ui
                                                            .add(
                                                                c::MenuItem::new(label)
                                                                    .checked(cur_q == level),
                                                            )
                                                            .clicked()
                                                        {
                                                            picked_quality = Some(level);
                                                            ui.close();
                                                        }
                                                    }
                                                })
                                            },
                                        );
                                    if rs.changed()
                                        && let Some(st) = filter_strength_mut(&mut active)
                                    {
                                        *st = (pct / 100.0).clamp(0.0, 255.0);
                                        changed_id = Some(rs.id);
                                    }
                                    if let Some(level) = picked_quality {
                                        active.set_quality_level(level);
                                        discrete_changed = true;
                                    }
                                }

                                match &mut active {
                                    FilterSpec::DropShadow {
                                        distance,
                                        angle,
                                        color,
                                        alpha,
                                        inner,
                                        knockout,
                                        hide_object,
                                        ..
                                    } => {
                                        let (ra, rd) = two_column_row(
                                            ui,
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(angle, "Angle")
                                                        .prefix("Angle")
                                                        .unit("°")
                                                        .decimals(0)
                                                        .range(0.0..=360.0)
                                                        .width(width),
                                                )
                                            },
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(distance, "Distance")
                                                        .prefix("Distance")
                                                        .unit("px")
                                                        .decimals(0)
                                                        .range(-255.0..=255.0)
                                                        .width(width),
                                                )
                                            },
                                        );
                                        if ra.changed() {
                                            changed_id = Some(ra.id);
                                        }
                                        if rd.changed() {
                                            changed_id = Some(rd.id);
                                        }
                                        let mut rgba = rgb_u32_and_alpha_to_color(*color, *alpha);
                                        let rc = color_row(
                                            ui,
                                            "filters-drop-shadow-color",
                                            "Color",
                                            "Shadow",
                                            &mut rgba,
                                        );
                                        if rc.changed() {
                                            (*color, *alpha) = color_to_rgb_u32_and_alpha(rgba);
                                            changed_id = Some(rc.id);
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .add(c::Checkbox::new(knockout, "Knockout"))
                                                .changed()
                                            {
                                                discrete_changed = true;
                                            }
                                            if ui
                                                .add(c::Checkbox::new(inner, "Inner shadow"))
                                                .changed()
                                            {
                                                discrete_changed = true;
                                            }
                                            if ui
                                                .add(c::Checkbox::new(hide_object, "Hide object"))
                                                .changed()
                                            {
                                                discrete_changed = true;
                                            }
                                        });
                                    }
                                    FilterSpec::Glow {
                                        color,
                                        alpha,
                                        inner,
                                        knockout,
                                        ..
                                    } => {
                                        let mut rgba = rgb_u32_and_alpha_to_color(*color, *alpha);
                                        let rc = color_row(
                                            ui,
                                            "filters-glow-color",
                                            "Color",
                                            "Glow",
                                            &mut rgba,
                                        );
                                        if rc.changed() {
                                            (*color, *alpha) = color_to_rgb_u32_and_alpha(rgba);
                                            changed_id = Some(rc.id);
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .add(c::Checkbox::new(knockout, "Knockout"))
                                                .changed()
                                            {
                                                discrete_changed = true;
                                            }
                                            if ui
                                                .add(c::Checkbox::new(inner, "Inner glow"))
                                                .changed()
                                            {
                                                discrete_changed = true;
                                            }
                                        });
                                    }
                                    FilterSpec::Bevel {
                                        distance,
                                        angle,
                                        highlight_color,
                                        highlight_alpha,
                                        shadow_color,
                                        shadow_alpha,
                                        placement,
                                        knockout,
                                        ..
                                    } => {
                                        let (ra, rd) = two_column_row(
                                            ui,
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(angle, "Angle")
                                                        .prefix("Angle")
                                                        .unit("°")
                                                        .decimals(0)
                                                        .range(0.0..=360.0)
                                                        .width(width),
                                                )
                                            },
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(distance, "Distance")
                                                        .prefix("Distance")
                                                        .unit("px")
                                                        .decimals(0)
                                                        .range(-255.0..=255.0)
                                                        .width(width),
                                                )
                                            },
                                        );
                                        if ra.changed() {
                                            changed_id = Some(ra.id);
                                        }
                                        if rd.changed() {
                                            changed_id = Some(rd.id);
                                        }
                                        let mut shadow_rgba = rgb_u32_and_alpha_to_color(
                                            *shadow_color,
                                            *shadow_alpha,
                                        );
                                        let r_sh = color_row(
                                            ui,
                                            "filters-bevel-shadow",
                                            "Shadow",
                                            "Shadow",
                                            &mut shadow_rgba,
                                        );
                                        if r_sh.changed() {
                                            (*shadow_color, *shadow_alpha) =
                                                color_to_rgb_u32_and_alpha(shadow_rgba);
                                            changed_id = Some(r_sh.id);
                                        }
                                        let mut hi_rgba = rgb_u32_and_alpha_to_color(
                                            *highlight_color,
                                            *highlight_alpha,
                                        );
                                        let r_hi = color_row(
                                            ui,
                                            "filters-bevel-highlight",
                                            "Highlight",
                                            "Highlight",
                                            &mut hi_rgba,
                                        );
                                        if r_hi.changed() {
                                            (*highlight_color, *highlight_alpha) =
                                                color_to_rgb_u32_and_alpha(hi_rgba);
                                            changed_id = Some(r_hi.id);
                                        }
                                        let cur_pl = placement.clone();
                                        let mut picked_pl = None;
                                        let (_, ko_resp) = two_column_row(
                                            ui,
                                            |ui, width| {
                                                c::Select::new(
                                                    (id_prefix, "filters-bevel-type"),
                                                    placement_label(&cur_pl),
                                                )
                                                .width(width)
                                                .show(ui, |ui| {
                                                    for (val, label) in [
                                                        ("inner", "Inner"),
                                                        ("outer", "Outer"),
                                                        ("full", "Full"),
                                                    ] {
                                                        if ui
                                                            .add(
                                                                c::MenuItem::new(label)
                                                                    .checked(cur_pl == val),
                                                            )
                                                            .clicked()
                                                        {
                                                            picked_pl = Some(val);
                                                            ui.close();
                                                        }
                                                    }
                                                })
                                            },
                                            |ui, _width| {
                                                ui.add(c::Checkbox::new(knockout, "Knockout"))
                                            },
                                        );
                                        if let Some(val) = picked_pl {
                                            *placement = val.into();
                                            discrete_changed = true;
                                        }
                                        if ko_resp.changed() {
                                            discrete_changed = true;
                                        }
                                    }
                                    FilterSpec::GradientGlow {
                                        distance,
                                        angle,
                                        colors,
                                        alphas,
                                        ratios,
                                        placement,
                                        knockout,
                                        ..
                                    }
                                    | FilterSpec::GradientBevel {
                                        distance,
                                        angle,
                                        colors,
                                        alphas,
                                        ratios,
                                        placement,
                                        knockout,
                                        ..
                                    } => {
                                        let mut stops =
                                            gradient_arrays_to_stops(colors, alphas, ratios);
                                        let mut selected_stop = state
                                            .gradient_stop
                                            .get(&key_id)
                                            .copied()
                                            .unwrap_or(0)
                                            .min(stops.len().saturating_sub(1));
                                        let bar_w = ui.available_width().max(120.0);
                                        let bar_resp = ui.add(c::GradientBar {
                                            stops: &mut stops,
                                            selected: &mut selected_stop,
                                            width: bar_w,
                                        });
                                        if bar_resp.changed() {
                                            if stops.len() > 16 {
                                                stops.truncate(16);
                                            }
                                            selected_stop =
                                                selected_stop.min(stops.len().saturating_sub(1));
                                            let (nc, na, nr) = stops_to_gradient_arrays(&stops);
                                            *colors = nc;
                                            *alphas = na;
                                            *ratios = nr;
                                            changed_id = Some(bar_resp.id);
                                        }
                                        selected_stop =
                                            selected_stop.min(stops.len().saturating_sub(1));
                                        if let Some((stop_pos, stop_c32)) =
                                            stops.get(selected_stop).copied()
                                        {
                                            let mut stop_rgba = stop_c32.to_srgba_unmultiplied();
                                            let r_sc = color_row(
                                                ui,
                                                "filters-gradient-stop-color",
                                                "Stop",
                                                "Gradient stop",
                                                &mut stop_rgba,
                                            );
                                            if r_sc.changed() {
                                                let (rgb, a) =
                                                    color_to_rgb_u32_and_alpha(stop_rgba);
                                                if selected_stop < colors.len()
                                                    && selected_stop < alphas.len()
                                                {
                                                    colors[selected_stop] = rgb;
                                                    alphas[selected_stop] = a;
                                                    changed_id = Some(r_sc.id);
                                                }
                                            }
                                            let mut stop_ratio = (stop_pos * 255.0).round() as f64;
                                            let can_add_stop = stops.len() < 16;
                                            let can_del_stop = stops.len() > 2;
                                            let mut add_stop_clicked = false;
                                            let mut del_stop_clicked = false;
                                            let (r_rat, _) = two_column_row(
                                                ui,
                                                |ui, width| {
                                                    ui.add(
                                                        c::Field::new(
                                                            &mut stop_ratio,
                                                            "Stop ratio",
                                                        )
                                                        .prefix("Ratio")
                                                        .decimals(0)
                                                        .range(0.0..=255.0)
                                                        .width(width),
                                                    )
                                                },
                                                |ui, _width| {
                                                    ui.horizontal(|ui| {
                                                        if ui
                                                            .add_enabled(
                                                                can_add_stop,
                                                                c::IconButton::new(
                                                                    i16::PLUS,
                                                                    "Add gradient stop",
                                                                ),
                                                            )
                                                            .clicked()
                                                        {
                                                            add_stop_clicked = true;
                                                        }
                                                        if ui
                                                            .add_enabled(
                                                                can_del_stop,
                                                                c::IconButton::new(
                                                                    i16::TRASH,
                                                                    "Delete gradient stop",
                                                                ),
                                                            )
                                                            .clicked()
                                                        {
                                                            del_stop_clicked = true;
                                                        }
                                                    });
                                                },
                                            );
                                            if r_rat.changed() && selected_stop < ratios.len() {
                                                stops[selected_stop].0 =
                                                    (stop_ratio as f32 / 255.0).clamp(0.0, 1.0);
                                                let target = stops[selected_stop];
                                                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                                                selected_stop = stops
                                                    .iter()
                                                    .position(|s| *s == target)
                                                    .unwrap_or(selected_stop);
                                                let (nc, na, nr) = stops_to_gradient_arrays(&stops);
                                                *colors = nc;
                                                *alphas = na;
                                                *ratios = nr;
                                                changed_id = Some(r_rat.id);
                                            }
                                            if add_stop_clicked && stops.len() < 16 {
                                                let mid_t = if selected_stop + 1 < stops.len() {
                                                    0.5 * (stops[selected_stop].0
                                                        + stops[selected_stop + 1].0)
                                                } else {
                                                    0.5 * (stops[selected_stop - 1].0
                                                        + stops[selected_stop].0)
                                                };
                                                let col = c::GradientBar::sample(&stops, mid_t);
                                                let ins = stops.partition_point(|s| s.0 < mid_t);
                                                stops.insert(ins, (mid_t, col));
                                                selected_stop = ins;
                                                let (nc, na, nr) = stops_to_gradient_arrays(&stops);
                                                *colors = nc;
                                                *alphas = na;
                                                *ratios = nr;
                                                discrete_changed = true;
                                            }
                                            if del_stop_clicked && stops.len() > 2 {
                                                stops.remove(selected_stop);
                                                selected_stop = selected_stop.min(stops.len() - 1);
                                                let (nc, na, nr) = stops_to_gradient_arrays(&stops);
                                                *colors = nc;
                                                *alphas = na;
                                                *ratios = nr;
                                                discrete_changed = true;
                                            }
                                        }
                                        state.gradient_stop.insert(key_id.clone(), selected_stop);

                                        let (ra, rd) = two_column_row(
                                            ui,
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(angle, "Angle")
                                                        .prefix("Angle")
                                                        .unit("°")
                                                        .decimals(0)
                                                        .range(0.0..=360.0)
                                                        .width(width),
                                                )
                                            },
                                            |ui, width| {
                                                ui.add(
                                                    c::Field::new(distance, "Distance")
                                                        .prefix("Distance")
                                                        .unit("px")
                                                        .decimals(0)
                                                        .range(-255.0..=255.0)
                                                        .width(width),
                                                )
                                            },
                                        );
                                        if ra.changed() {
                                            changed_id = Some(ra.id);
                                        }
                                        if rd.changed() {
                                            changed_id = Some(rd.id);
                                        }
                                        let cur_pl = placement.clone();
                                        let mut picked_pl = None;
                                        let (_, ko_resp) = two_column_row(
                                            ui,
                                            |ui, width| {
                                                c::Select::new(
                                                    (id_prefix, "filters-gradient-type"),
                                                    placement_label(&cur_pl),
                                                )
                                                .width(width)
                                                .show(ui, |ui| {
                                                    for (val, label) in [
                                                        ("inner", "Inner"),
                                                        ("outer", "Outer"),
                                                        ("full", "Full"),
                                                    ] {
                                                        if ui
                                                            .add(
                                                                c::MenuItem::new(label)
                                                                    .checked(cur_pl == val),
                                                            )
                                                            .clicked()
                                                        {
                                                            picked_pl = Some(val);
                                                            ui.close();
                                                        }
                                                    }
                                                })
                                            },
                                            |ui, _width| {
                                                ui.add(c::Checkbox::new(knockout, "Knockout"))
                                            },
                                        );
                                        if let Some(val) = picked_pl {
                                            *placement = val.into();
                                            discrete_changed = true;
                                        }
                                        if ko_resp.changed() {
                                            discrete_changed = true;
                                        }
                                    }
                                    FilterSpec::Blur { .. }
                                    | FilterSpec::ColorMatrix { .. }
                                    | FilterSpec::Convolution { .. } => {}
                                }

                                if discrete_changed {
                                    filter_edit =
                                        Some((idx, active.with_enabled(was_enabled), None));
                                } else if let Some(cid) = changed_id {
                                    filter_edit =
                                        Some((idx, active.with_enabled(was_enabled), Some(cid)));
                                }
                            }
                        }
                    } else {
                        ui.add_enabled_ui_semantic(false, |ui| {
                            let mut zero_x = 0.0;
                            let mut zero_y = 0.0;
                            let mut hundred = 100.0;
                            two_column_row(
                                ui,
                                |ui, width| {
                                    ui.add(
                                        c::Field::new(&mut zero_x, "Blur X")
                                            .prefix("Blur X")
                                            .unit("px")
                                            .decimals(0)
                                            .width(width),
                                    )
                                },
                                |ui, width| {
                                    ui.add(
                                        c::Field::new(&mut zero_y, "Blur Y")
                                            .prefix("Blur Y")
                                            .unit("px")
                                            .decimals(0)
                                            .width(width),
                                    )
                                },
                            );
                            two_column_row(
                                ui,
                                |ui, width| {
                                    ui.add(
                                        c::Field::new(&mut hundred, "Strength")
                                            .prefix("Strength")
                                            .unit("%")
                                            .decimals(0)
                                            .width(width),
                                    )
                                },
                                |ui, width| {
                                    c::Select::new(
                                        (id_prefix, "filters-quality-empty"),
                                        "Quality: Low",
                                    )
                                    .width(width)
                                    .show(ui, |_| {})
                                },
                            );
                        });
                    }

                    let can_add = filters_editable && filters.len() < MAX_OBJECT_FILTERS;
                    let can_dup = filters_editable
                        && !filters.is_empty()
                        && filters.len() < MAX_OBJECT_FILTERS;
                    let can_del = filters_editable && !filters.is_empty();
                    let can_up = filters_editable && !filters.is_empty() && idx > 0;
                    let can_down =
                        filters_editable && !filters.is_empty() && idx + 1 < filters.len();
                    let active_enabled = filters.get(idx).is_none_or(FilterSpec::is_enabled);
                    let presets_list = self.filter_presets();
                    let has_clipboard = shared_filter_store()
                        .lock()
                        .is_ok_and(|g| !g.copied_filters.is_empty());

                    ui.horizontal(|ui| {
                        let add_btn =
                            ui.add_enabled(can_add, c::IconButton::new(i16::PLUS, "Add filter"));
                        c::popup_below(&add_btn, 180.0, |ui| {
                            for kind in AuthoringFilterKind::ALL {
                                if ui.add(c::MenuItem::new(kind.label())).clicked() {
                                    add_kind = Some(kind);
                                    ui.close();
                                }
                            }
                        });
                        if ui
                            .add_enabled(can_dup, c::IconButton::new(i16::COPY, "Duplicate filter"))
                            .clicked()
                        {
                            duplicate_clicked = true;
                        }
                        if ui
                            .add_enabled(
                                can_del,
                                c::IconButton::new(
                                    if active_enabled {
                                        i16::EYE
                                    } else {
                                        i16::EYEOFF
                                    },
                                    "Toggle filter enabled",
                                )
                                .selected(active_enabled),
                            )
                            .clicked()
                        {
                            toggle_enabled_clicked = Some((idx, !active_enabled));
                        }
                        if ui
                            .add_enabled(can_up, c::IconButton::new(i16::CHEVUP, "Move filter up"))
                            .clicked()
                        {
                            move_filter = Some((idx, idx - 1));
                        }
                        if ui
                            .add_enabled(
                                can_down,
                                c::IconButton::new(i16::CHEVDOWN, "Move filter down"),
                            )
                            .clicked()
                        {
                            move_filter = Some((idx, idx + 1));
                        }
                        let more_btn = ui.add_enabled(
                            filters_editable,
                            c::IconButton::new(i16::MORE, "Filter presets and options"),
                        );
                        c::popup_below(&more_btn, 220.0, |ui| {
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Reset filter"))
                                .clicked()
                            {
                                reset_filter_clicked = Some(idx);
                                ui.close();
                            }
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Enable all"))
                                .clicked()
                            {
                                enable_all_action = Some(true);
                                ui.close();
                            }
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Disable all"))
                                .clicked()
                            {
                                enable_all_action = Some(false);
                                ui.close();
                            }
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Remove all"))
                                .clicked()
                            {
                                clear_all_clicked = true;
                                ui.close();
                            }
                            c::menu_separator(ui);
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Copy filters"))
                                .clicked()
                            {
                                copy_filters_clicked = true;
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    can_add && has_clipboard,
                                    c::MenuItem::new("Paste filters"),
                                )
                                .clicked()
                            {
                                paste_filters_clicked = true;
                                ui.close();
                            }
                            c::menu_separator(ui);
                            if ui
                                .add_enabled(can_del, c::MenuItem::new("Save preset"))
                                .clicked()
                            {
                                let next_num = presets_list.len() + 1;
                                let name = if state.preset_draft_name.trim().is_empty() {
                                    format!("Custom Preset {next_num}")
                                } else {
                                    state.preset_draft_name.trim().to_string()
                                };
                                save_preset_name = Some(name);
                                ui.close();
                            }
                            for preset in &presets_list {
                                let label = format!("Apply preset: {}", preset.name);
                                if ui.add(c::MenuItem::new(&label)).clicked() {
                                    apply_preset_name = Some(preset.name.clone());
                                    ui.close();
                                }
                            }
                            for preset in presets_list.iter().skip(3) {
                                let del_label = format!("Delete preset: {}", preset.name);
                                if ui.add(c::MenuItem::new(&del_label)).clicked() {
                                    delete_preset_name = Some(preset.name.clone());
                                    ui.close();
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    can_del,
                                    c::IconButton::new(i16::TRASH, "Remove filter"),
                                )
                                .clicked()
                            {
                                remove_clicked = true;
                            }
                        });
                    });
                });
        });

        if let Some(scope) = next_scope {
            self.set_filter_target_scope(scope);
            state.timeline_scope = scope;
            state.selected_index = 0;
        }
        if let Some(v) = next_visible {
            self.set_selected_visible(v);
        }
        if let Some(c) = next_cache {
            self.set_selected_cache_as_bitmap(c);
        }
        if let Some(b) = next_blend {
            self.set_selected_blend_mode(b);
        }
        if let Some(kind) = add_kind
            && let Some(new_idx) = self.add_selected_filter(kind)
        {
            state.selected_index = new_idx;
            if let Some(id) = &target_key
                && kind == AuthoringFilterKind::AdjustColor
            {
                state.adjust.insert(
                    (id.clone(), new_idx),
                    (AdjustColorSliders::default(), IDENTITY_COLOR_MATRIX),
                );
            }
        }
        if duplicate_clicked
            && let Some(new_idx) = self.duplicate_selected_filter(state.selected_index)
        {
            if let Some(id) = &target_key
                && let Some(prev) = state
                    .adjust
                    .get(&(id.clone(), state.selected_index))
                    .copied()
            {
                state.adjust.insert((id.clone(), new_idx), prev);
            }
            state.selected_index = new_idx;
        }
        if remove_clicked && self.remove_selected_filter(state.selected_index) {
            if let Some(id) = &target_key {
                state.adjust.remove(&(id.clone(), state.selected_index));
            }
            if state.selected_index > 0 {
                state.selected_index -= 1;
            }
        }
        if let Some((idx, enabled)) = toggle_enabled_clicked {
            self.set_selected_filter_enabled(idx, enabled);
        }
        if let Some((from, to)) = move_filter
            && let Some(new_idx) = self.move_selected_filter(from, to)
        {
            state.selected_index = new_idx;
        }
        if let Some(enabled) = enable_all_action {
            self.set_all_selected_filters_enabled(enabled);
        }
        if let Some(idx) = reset_filter_clicked {
            self.reset_selected_filter(idx);
            if let Some(id) = &target_key {
                state.adjust.remove(&(id.clone(), idx));
            }
        }
        if clear_all_clicked {
            self.clear_selected_filters();
            state.selected_index = 0;
        }
        if copy_filters_clicked {
            self.copy_selected_filters();
        }
        if paste_filters_clicked {
            self.paste_selected_filters();
        }
        if let Some(name) = save_preset_name {
            self.save_filter_preset(name);
        }
        if let Some(name) = apply_preset_name {
            self.apply_filter_preset(&name);
            state.selected_index = 0;
        }
        if let Some(name) = delete_preset_name {
            self.delete_filter_preset(&name);
        }
        if let Some((idx, spec, control)) = filter_edit {
            self.update_selected_filter(idx, spec, control);
        }

        ctx.data_mut(|d| d.insert_temp(state_id(), state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_panel_supports_stack_management_presets_layer_keyframe_filters_and_stage_cache() {
        let mut project = Project::blank();
        project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "rect-1".into(),
            name: "box".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 48.0,
                height: 32.0,
                fill: [220, 80, 40, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }];
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        editor.select_all();
        editor.selection.targets.truncate(1);
        assert_eq!(editor.selection.len(), 1);

        // Add DropShadow, GradientGlow, and AdjustColor.
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::DropShadow),
            Some(0)
        );
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::GradientGlow),
            Some(1)
        );
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::AdjustColor),
            Some(2)
        );
        let sliders = AdjustColorSliders {
            brightness: 20.0,
            contrast: -15.0,
            saturation: 35.0,
            hue: -45.0,
        };
        assert!(editor.update_selected_filter(
            2,
            FilterSpec::ColorMatrix {
                matrix: sliders.to_matrix(),
            },
            None,
        ));
        let json = editor.project.to_json().unwrap();
        let reloaded = Project::from_json(&json).unwrap();
        let reloaded_matrix = match &reloaded
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|e| e.filters.len() == 3)
            .unwrap()
            .filters[2]
        {
            FilterSpec::ColorMatrix { matrix } => *matrix,
            other => panic!("expected ColorMatrix, got {other:?}"),
        };
        let recovered = AdjustColorSliders::from_matrix(&reloaded_matrix);
        assert!((recovered.brightness - sliders.brightness).abs() < 0.05);
        assert!((recovered.contrast - sliders.contrast).abs() < 0.05);
        assert!((recovered.saturation - sliders.saturation).abs() < 0.05);
        assert!((recovered.hue - sliders.hue).abs() < 0.05);

        // Per-filter enable/disable, move up/down, reset, copy/paste, and presets.
        assert!(editor.set_selected_filter_enabled(1, false));
        assert!(!editor.current_filter_stack().unwrap()[1].is_enabled());
        assert!(editor.set_all_selected_filters_enabled(true));
        assert!(editor.current_filter_stack().unwrap()[1].is_enabled());
        assert_eq!(editor.move_selected_filter(2, 0), Some(0));
        assert_eq!(
            editor.current_filter_stack().unwrap()[0].display_name(),
            "Adjust color"
        );
        assert!(editor.reset_selected_filter(0));
        assert_eq!(
            editor.current_filter_stack().unwrap()[0],
            FilterSpec::ColorMatrix {
                matrix: IDENTITY_COLOR_MATRIX
            }
        );
        assert!(editor.save_filter_preset("UnitTestPreset"));
        assert!(editor.copy_selected_filters());
        assert!(editor.clear_selected_filters());
        assert!(editor.current_filter_stack().unwrap().is_empty());
        assert!(editor.paste_selected_filters());
        assert_eq!(editor.current_filter_stack().unwrap().len(), 3);
        assert!(editor.clear_selected_filters());
        assert!(editor.apply_filter_preset("UnitTestPreset"));
        assert_eq!(editor.current_filter_stack().unwrap().len(), 3);
        assert!(editor.delete_filter_preset("UnitTestPreset"));

        // Timeline Keyframe and Layer filters when selection is empty.
        editor.selection.targets.clear();
        editor.set_filter_target_scope(FilterTargetScope::Keyframe);
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::Glow),
            Some(0)
        );
        assert_eq!(editor.active_keyframe_filters().len(), 1);
        editor.set_filter_target_scope(FilterTargetScope::Layer);
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::Blur),
            Some(0)
        );
        assert_eq!(editor.active_layer_filters().len(), 1);
        editor.project.validate().unwrap();

        // Stage raster filter cache helper.
        let src = vec![255u8, 0, 0, 255];
        let (out_w, out_h, out_rgba, pad) =
            apply_stage_filters_cached(42, 1, 1, &src, &editor.active_layer_filters()).unwrap();
        assert_eq!(out_w, 1 + pad.left + pad.right);
        assert_eq!(out_h, 1 + pad.top + pad.bottom);
        assert_eq!(out_rgba.len(), (out_w * out_h * 4) as usize);
    }

    #[test]
    fn timeline_filters_respect_effective_parent_lock_and_visibility() {
        let mut project = Project::blank();
        project.symbols[0].timeline.layers = vec![
            Layer {
                id: "folder".into(),
                name: "Folder".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties {
                    role: LayerRole::Folder,
                    ..LayerProperties::default()
                },
            },
            Layer {
                id: "child".into(),
                name: "Child".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties {
                    parent: Some("folder".into()),
                    ..LayerProperties::default()
                },
            },
        ];
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        editor.layer = 1;
        editor.set_filter_target_scope(FilterTargetScope::Keyframe);
        let before = editor.project.clone();

        assert!(!editor.timeline_filter_target_editable());
        assert_eq!(editor.add_selected_filter(AuthoringFilterKind::Blur), None);
        assert_eq!(editor.project, before);

        editor.symbol_mut().timeline.layers[0].locked = false;
        editor.symbol_mut().timeline.layers[0].visible = false;
        let hidden_before = editor.project.clone();
        assert!(!editor.timeline_filter_target_editable());
        assert_eq!(editor.add_selected_filter(AuthoringFilterKind::Glow), None);
        assert_eq!(editor.project, hidden_before);

        editor.symbol_mut().timeline.layers[0].visible = true;
        assert!(editor.timeline_filter_target_editable());
        assert_eq!(
            editor.add_selected_filter(AuthoringFilterKind::Blur),
            Some(0)
        );
        assert_eq!(editor.active_keyframe_filters().len(), 1);
        editor.project.validate().unwrap();
    }
}
