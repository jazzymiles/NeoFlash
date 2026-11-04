//! Filters panel (`Window ▸ Filters`, NF2 `Panel / Properties — Movie clip` Display & Filters
//! sections; reference `panel-properties-movie-clip.png`): authored object visibility,
//! bitmap caching, blend mode, and ordered display filters (`DropShadow`, `Blur`, `Glow`,
//! `Bevel`, `GradientGlow`, `GradientBevel`, and `Adjust color` → 4×5 `ColorMatrix`).
//!
//! Every edit runs through `Editor::transact` / `begin_property_edit` (`commit_history`),
//! upgrading the document to format 7 when authored display/filter properties are used.
use super::planned::type_header;
use crate::app::Editor;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::HashMap;

const INVERT_COLOR_MATRIX: [f64; 20] = [
    -1., 0., 0., 0., 255., 0., -1., 0., 0., 255., 0., 0., -1., 0., 255., 0., 0., 0., 1., 0.,
];

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

/// Authoring Adjust Color parameters remembered per `(element_id, filter_index)` in UI temp state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct AdjustColorSliders {
    brightness: f64,
    contrast: f64,
    saturation: f64,
    hue: f64,
}

#[derive(Clone, Debug, Default)]
struct FiltersPanelState {
    selected_element: Option<Id>,
    selected_index: usize,
    adjust: HashMap<(Id, usize), AdjustColorSliders>,
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-filters-panel")
}

fn quality_label(quality: u8) -> &'static str {
    match quality {
        0 | 1 => "Quality: Low",
        2 => "Quality: Medium",
        3 => "Quality: High",
        _ => "Quality: Custom",
    }
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
        let current_len = self.selected_sampled()?.filters.len();
        if current_len >= MAX_OBJECT_FILTERS {
            return None;
        }
        let spec = kind.default_spec();
        let mut added = None;
        if self.mutate_selected_display(None, |element| {
            if element.filters.len() < MAX_OBJECT_FILTERS {
                element.filters.push(spec);
                added = Some(element.filters.len() - 1);
            }
        }) {
            added
        } else {
            None
        }
    }

    pub(crate) fn duplicate_selected_filter(&mut self, index: usize) -> Option<usize> {
        let sampled = self.selected_sampled()?;
        if index >= sampled.filters.len() || sampled.filters.len() >= MAX_OBJECT_FILTERS {
            return None;
        }
        let mut inserted = None;
        if self.mutate_selected_display(None, |element| {
            if index < element.filters.len() && element.filters.len() < MAX_OBJECT_FILTERS {
                let copy = element.filters[index].clone();
                element.filters.insert(index + 1, copy);
                inserted = Some(index + 1);
            }
        }) {
            inserted
        } else {
            None
        }
    }

    pub(crate) fn remove_selected_filter(&mut self, index: usize) -> bool {
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        if index >= sampled.filters.len() {
            return false;
        }
        self.mutate_selected_display(None, |element| {
            if index < element.filters.len() {
                element.filters.remove(index);
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
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        if index >= sampled.filters.len() {
            return false;
        }
        self.mutate_selected_display(control, |element| {
            if let Some(slot) = element.filters.get_mut(index) {
                *slot = spec;
            }
        })
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

    /// Window ▸ Filters.
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

    /// Panel / Filters body (moved from `planned_panels::properties_filters`, bound to the
    /// selected `Element`'s `visible`, `cache_as_bitmap`, `blend_mode`, and `filters`).
    fn filters_panel(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut state: FiltersPanelState = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();

        let (header_icon, header_title, header_sub) = self.selected_type_header();
        type_header(ui, header_icon, header_title, &header_sub);

        let sampled = if self.selection.len() == 1 {
            self.selected_sampled()
        } else {
            None
        };
        let editable = sampled.is_some() && self.selected_editable();

        let current_id = sampled.as_ref().map(|e| e.id.clone());
        if state.selected_element != current_id {
            state.selected_element = current_id.clone();
            state.selected_index = 0;
        }
        let filters = sampled
            .as_ref()
            .map(|e| e.filters.clone())
            .unwrap_or_default();
        if !filters.is_empty() && state.selected_index >= filters.len() {
            state.selected_index = filters.len() - 1;
        } else if filters.is_empty() {
            state.selected_index = 0;
        }

        let mut visible = sampled.as_ref().is_none_or(|e| e.visible);
        let mut cache_as_bitmap = sampled.as_ref().is_some_and(|e| e.cache_as_bitmap);
        let blend_mode = sampled
            .as_ref()
            .map_or_else(|| self.layer_blend_mode(self.layer), |e| e.blend_mode);
        let layer_blend_editable = self.selection.is_empty()
            && self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| l.properties.role != LayerRole::Guide);

        let half = (ui.available_width() - space::S2).max(40.0) / 2.0;

        let mut next_visible = None;
        let mut next_cache = None;
        let mut next_blend = None;
        let mut add_kind = None;
        let mut duplicate_clicked = false;
        let mut remove_clicked = false;
        let mut filter_edit: Option<(usize, FilterSpec, Option<egui::Id>)> = None;

        ui.add_enabled_ui_semantic(editable || layer_blend_editable, |ui| {
            c::SectionHeader::new("Display")
                .id_salt("filters-mc-display")
                .show(ui, |ui| {
                    ui.add_enabled_ui_semantic(editable, |ui| {
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
                    ui.horizontal(|ui| {
                        let blend_label = format!("Blending: {}", blend_mode.label());
                        c::Select::new("filters-blending", &blend_label)
                            .width(half)
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
                        ui.add_enabled_ui_semantic(editable, |ui| {
                            let render_label = if cache_as_bitmap {
                                "Render: Export as bitmap"
                            } else {
                                "Render: Original"
                            };
                            c::Select::new("filters-render", render_label)
                                .width(half)
                                .show(ui, |ui| {
                                    for (cached, label) in
                                        [(false, "Original"), (true, "Export as bitmap")]
                                    {
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
                    });
                });
        });
        ui.add_enabled_ui_semantic(editable, |ui| {
            c::SectionHeader::new("Filters")
                .id_salt("filters-mc-filters")
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let cols = &[0.0, 0.5];
                    ui.add(c::TableRow {
                        cells: &["Filter", "Value"],
                        kind: c::TableRowKind::Header,
                        columns: cols,
                    });
                    for (idx, spec) in filters.iter().enumerate() {
                        let value_summary = spec.summary_value();
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
                    if let Some(mut active) = filters.get(idx).cloned() {
                        match &mut active {
                            FilterSpec::ColorMatrix { matrix } => {
                                let key = (current_id.clone().unwrap_or_default(), idx);
                                let mut sliders =
                                    state.adjust.get(&key).copied().unwrap_or_default();
                                let mut changed_id = None;
                                ui.horizontal(|ui| {
                                    let r1 = ui.add(
                                        c::Field::new(&mut sliders.brightness, "Brightness")
                                            .prefix("Brightness")
                                            .decimals(0)
                                            .range(-100.0..=100.0)
                                            .width(half),
                                    );
                                    if r1.changed() {
                                        changed_id = Some(r1.id);
                                    }
                                    let r2 = ui.add(
                                        c::Field::new(&mut sliders.contrast, "Contrast")
                                            .prefix("Contrast")
                                            .decimals(0)
                                            .range(-100.0..=100.0)
                                            .width(half),
                                    );
                                    if r2.changed() {
                                        changed_id = Some(r2.id);
                                    }
                                });
                                let preset_label = if *matrix == IDENTITY_COLOR_MATRIX {
                                    "Preset: Identity"
                                } else if *matrix == INVERT_COLOR_MATRIX {
                                    "Preset: Invert"
                                } else {
                                    "Preset: Custom"
                                };
                                let mut picked_matrix = None;
                                ui.horizontal(|ui| {
                                    let r3 = ui.add(
                                        c::Field::new(&mut sliders.saturation, "Saturation")
                                            .prefix("Saturation")
                                            .unit("%")
                                            .decimals(0)
                                            .range(-100.0..=100.0)
                                            .width(half),
                                    );
                                    if r3.changed() {
                                        changed_id = Some(r3.id);
                                    }
                                    c::Select::new("filters-color-preset", preset_label)
                                        .width(half)
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
                                                    sliders = vals;
                                                    picked_matrix =
                                                        Some(exact.unwrap_or_else(|| {
                                                            adjust_color_matrix(
                                                                vals.brightness,
                                                                vals.contrast,
                                                                vals.saturation,
                                                                vals.hue,
                                                            )
                                                        }));
                                                    ui.close();
                                                }
                                            }
                                        });
                                });
                                if let Some(m) = picked_matrix {
                                    state.adjust.insert(key, sliders);
                                    filter_edit =
                                        Some((idx, FilterSpec::ColorMatrix { matrix: m }, None));
                                } else if let Some(cid) = changed_id {
                                    state.adjust.insert(key, sliders);
                                    let m = adjust_color_matrix(
                                        sliders.brightness,
                                        sliders.contrast,
                                        sliders.saturation,
                                        sliders.hue,
                                    );
                                    filter_edit = Some((
                                        idx,
                                        FilterSpec::ColorMatrix { matrix: m },
                                        Some(cid),
                                    ));
                                }
                            }
                            _ => {
                                let mut changed_id = None;
                                let mut picked_quality = None;
                                if let Some((bx, by, q)) = filter_blur_mut(&mut active) {
                                    let mut blur_x = *bx;
                                    let mut blur_y = *by;
                                    let cur_q = *q;
                                    ui.horizontal(|ui| {
                                        let rx = ui.add(
                                            c::Field::new(&mut blur_x, "Blur X")
                                                .prefix("Blur X")
                                                .unit("px")
                                                .decimals(0)
                                                .range(0.0..=255.0)
                                                .width(half),
                                        );
                                        if rx.changed() {
                                            *bx = blur_x;
                                            changed_id = Some(rx.id);
                                        }
                                        let ry = ui.add(
                                            c::Field::new(&mut blur_y, "Blur Y")
                                                .prefix("Blur Y")
                                                .unit("px")
                                                .decimals(0)
                                                .range(0.0..=255.0)
                                                .width(half),
                                        );
                                        if ry.changed() {
                                            *by = blur_y;
                                            changed_id = Some(ry.id);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        if let Some(st) = filter_strength_mut(&mut active) {
                                            let mut pct = *st * 100.0;
                                            let rs = ui.add(
                                                c::Field::new(&mut pct, "Strength")
                                                    .prefix("Strength")
                                                    .unit("%")
                                                    .decimals(0)
                                                    .range(0.0..=25500.0)
                                                    .width(half),
                                            );
                                            if rs.changed() {
                                                *st = (pct / 100.0).clamp(0.0, 255.0);
                                                changed_id = Some(rs.id);
                                            }
                                        } else {
                                            let mut fixed = 100.0;
                                            ui.add_enabled(
                                                false,
                                                c::Field::new(&mut fixed, "Strength")
                                                    .prefix("Strength")
                                                    .unit("%")
                                                    .decimals(0)
                                                    .width(half),
                                            );
                                        }
                                        c::Select::new("filters-quality", quality_label(cur_q))
                                            .width(half)
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
                                            });
                                    });
                                }
                                if let Some(level) = picked_quality
                                    && let Some((_, _, q)) = filter_blur_mut(&mut active)
                                {
                                    *q = level;
                                    filter_edit = Some((idx, active, None));
                                } else if let Some(cid) = changed_id {
                                    filter_edit = Some((idx, active, Some(cid)));
                                }
                            }
                        }
                    } else {
                        ui.add_enabled_ui_semantic(false, |ui| {
                            let mut zero = 0.0;
                            let mut hundred = 100.0;
                            ui.horizontal(|ui| {
                                ui.add(
                                    c::Field::new(&mut zero, "Blur X")
                                        .prefix("Blur X")
                                        .unit("px")
                                        .decimals(0)
                                        .width(half),
                                );
                                ui.add(
                                    c::Field::new(&mut zero, "Blur Y")
                                        .prefix("Blur Y")
                                        .unit("px")
                                        .decimals(0)
                                        .width(half),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.add(
                                    c::Field::new(&mut hundred, "Strength")
                                        .prefix("Strength")
                                        .unit("%")
                                        .decimals(0)
                                        .width(half),
                                );
                                c::Select::new("filters-quality-empty", "Quality: Low")
                                    .width(half)
                                    .show(ui, |_| {});
                            });
                        });
                    }

                    let can_add = editable && filters.len() < MAX_OBJECT_FILTERS;
                    let can_dup =
                        editable && !filters.is_empty() && filters.len() < MAX_OBJECT_FILTERS;
                    let can_del = editable && !filters.is_empty();
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
            if let Some(id) = &current_id
                && kind == AuthoringFilterKind::AdjustColor
            {
                state
                    .adjust
                    .insert((id.clone(), new_idx), AdjustColorSliders::default());
            }
        }
        if duplicate_clicked
            && let Some(new_idx) = self.duplicate_selected_filter(state.selected_index)
        {
            if let Some(id) = &current_id
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
            if let Some(id) = &current_id {
                state.adjust.remove(&(id.clone(), state.selected_index));
            }
            if state.selected_index > 0 {
                state.selected_index -= 1;
            }
        }
        if let Some((idx, spec, control)) = filter_edit {
            self.update_selected_filter(idx, spec, control);
        }

        ctx.data_mut(|d| d.insert_temp(state_id(), state));
    }
}
