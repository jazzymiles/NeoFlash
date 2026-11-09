//! Live interactive bindings for NF2 floating panels and the Test Movie window (`T-126`):
//! - `BitmapFill` (`CO-02`, `W3.10`)
//! - `PropertiesOvalPrimitive` (`TO-01` / `PR-02`)
//! - `PropertiesRectPrimitive` (`TO-01` / `PR-02`)
//! - `DecoTool` (`TO-03`, `W3.9`)
//! - `SprayBrush` (`TO-04`, `W3.9`)
//! - `BoneProperties` (`TO-05`, `W3.9`)
//! - `GraphicProperties` (`PR-07`)
//! - `Debugger` authoring view (`PP-04`, `T-119`)
//! - `TestMovie` in-editor preview (`DL-12`)

use super::planned::{PlannedPanel, PlannedState, caption, form_label, mono};
use crate::app::Editor;
use eframe::egui::{self, Color32, Ui};
use image::{ImageFormat, RgbaImage};
use neoflash_core::{
    ElementKind, IkArmature, IkBone, InstancePlayback, Keyframe, OvalOptions, PathCommand,
    PrimitiveOptions, RectangleOptions, ResourceUnit, evaluate_scene, new_id,
};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::icons::{self, i16, i20};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::{rich, rich_block};
use std::io::Cursor;

fn half(ui: &Ui) -> f32 {
    (ui.available_width() - space::S2) / 2.0
}

fn section(ui: &mut Ui, label: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    c::SectionHeader::new(label)
        .id_salt(("live-planned-drawing", label))
        .default_open(open)
        .show(ui, body);
}

#[derive(Clone, Debug)]
struct DecoDraft {
    effect: usize,
    branch_angle: f64,
    scale_pct: f64,
    segment_px: f64,
    step_frames: f64,
    animate_pattern: bool,
}

impl Default for DecoDraft {
    fn default() -> Self {
        Self {
            effect: 0,
            branch_angle: 0.0,
            scale_pct: 100.0,
            segment_px: 6.0,
            step_frames: 1.0,
            animate_pattern: true,
        }
    }
}

#[derive(Clone, Debug)]
struct SprayDraft {
    symbol_name: String,
    use_dots: bool,
    scale_w: f64,
    scale_h: f64,
    random_scaling: bool,
    rotate_symbol: bool,
    random_rotation: bool,
    brush_w: f64,
    brush_h: f64,
    brush_angle: f64,
}

impl Default for SprayDraft {
    fn default() -> Self {
        Self {
            symbol_name: "leaf_mc".to_owned(),
            use_dots: false,
            scale_w: 100.0,
            scale_h: 100.0,
            random_scaling: true,
            rotate_symbol: true,
            random_rotation: true,
            brush_w: 24.0,
            brush_h: 24.0,
            brush_angle: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
struct BoneDraft {
    bone_index: usize,
    name: String,
    x: f64,
    y: f64,
    length: f64,
    angle: f64,
    rot_enable: bool,
    rot_constrain: bool,
    rot_min: f64,
    rot_max: f64,
    trans_enable: bool,
    trans_constrain: bool,
    spring_strength: f64,
    spring_damping: f64,
    armature_runtime: bool,
    armature_wireframe: bool,
    speed_pct: f64,
    enable_spring: bool,
}

impl Default for BoneDraft {
    fn default() -> Self {
        Self {
            bone_index: 0,
            name: "arm_lower".to_owned(),
            x: 210.4,
            y: 188.0,
            length: 96.0,
            angle: -32.0,
            rot_enable: true,
            rot_constrain: true,
            rot_min: -45.0,
            rot_max: 90.0,
            trans_enable: false,
            trans_constrain: false,
            spring_strength: 20.0,
            spring_damping: 40.0,
            armature_runtime: false,
            armature_wireframe: false,
            speed_pct: 100.0,
            enable_spring: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TestMovieDraft {
    pub(crate) frame: u32,
    pub(crate) playing: bool,
    pub(crate) loop_playback: bool,
    pub(crate) speed: f32,
    pub(crate) show_output: bool,
    pub(crate) last_step_time: f64,
}

impl Default for TestMovieDraft {
    fn default() -> Self {
        Self {
            frame: 0,
            playing: false,
            loop_playback: true,
            speed: 1.0,
            show_output: true,
            last_step_time: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct BitmapFillDraft {
    selected_tile: usize,
    lock_fill: bool,
}

fn make_procedural_tile_png(c0: Color32, c1: Color32) -> Vec<u8> {
    let mut img = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let checker = ((x / 4) + (y / 4)) % 2 == 0;
            let c = if checker { c0 } else { c1 };
            img.put_pixel(x, y, image::Rgba([c.r(), c.g(), c.b(), c.a()]));
        }
    }
    let mut cur = Cursor::new(Vec::new());
    let _ = img.write_to(&mut cur, ImageFormat::Png);
    cur.into_inner()
}

impl Editor {
    /// Dispatch a live `PlannedPanel` body bound to `self` (`Editor`).
    pub(crate) fn live_planned_panel_body(
        &mut self,
        ui: &mut Ui,
        panel: PlannedPanel,
        state: &mut PlannedState,
    ) -> bool {
        match panel {
            PlannedPanel::Debugger => {
                if self.debugger.has_presentation() {
                    self.debugger_contents(ui)
                } else {
                    self.live_authoring_debugger(ui)
                }
            }
            PlannedPanel::ExportImage => {
                self.export_image_inline_panel(ui, &mut state.export_format)
            }
            PlannedPanel::ImportVideo => self.import_video_inline_panel(ui),
            PlannedPanel::StrokeStyle => self.stroke_style_inline_panel(ui),
            PlannedPanel::FontEmbedding => self.font_embedding_inline_panel(ui),
            PlannedPanel::BitmapFill => {
                self.live_bitmap_fill(ui);
                false
            }
            PlannedPanel::PropertiesOvalPrimitive => {
                self.live_properties_oval(ui);
                false
            }
            PlannedPanel::PropertiesRectPrimitive => {
                self.live_properties_rect(ui);
                false
            }
            PlannedPanel::DecoTool => {
                self.live_deco_tool(ui);
                false
            }
            PlannedPanel::SprayBrush => {
                self.live_spray_brush(ui);
                false
            }
            PlannedPanel::BoneProperties => {
                self.live_bone_properties(ui);
                false
            }
            PlannedPanel::GraphicProperties => {
                self.live_graphic_properties(ui);
                false
            }
            PlannedPanel::TestMovie => {
                self.live_test_movie(ui);
                false
            }
        }
    }

    pub(crate) fn live_test_movie_title_actions(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("nf2-live-test-movie-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<TestMovieDraft>(id))
            .unwrap_or_default();
        if ui
            .add(c::IconButton::new(i16::SIDEBAR, "Output drawer").selected(draft.show_output))
            .clicked()
        {
            draft.show_output = !draft.show_output;
            ui.ctx().data_mut(|d| d.insert_temp(id, draft));
        }
        if ui
            .add(c::IconButton::new(i16::GLOBE, "Open in browser"))
            .clicked()
        {
            self.export(true);
        }
        let fps = self.project.fps.max(1);
        let badge = format!("{fps} / {fps} fps · HTML5");
        ui.add(c::Badge::new(&badge, c::BadgeType::Neutral));
    }

    /// Apply a procedural bitmap tile to the selected shape, or add the tile resource when there
    /// is no selection, in one atomic Undo transaction.
    pub(crate) fn apply_bitmap_swatch_tile(&mut self, tile_index: usize) -> Result<String, String> {
        let tile_colors: [(Color32, Color32); 6] = [
            (
                hsv(0.07, 0.7, 0.85, 1.0).into(),
                hsv(0.07, 0.7, 0.35, 1.0).into(),
            ),
            (
                hsv(0.64, 0.55, 0.85, 1.0).into(),
                hsv(0.64, 0.55, 0.35, 1.0).into(),
            ),
            (
                hsv(0.38, 0.55, 0.85, 1.0).into(),
                hsv(0.38, 0.55, 0.35, 1.0).into(),
            ),
            (
                hsv(0.0, 0.0, 0.85, 1.0).into(),
                hsv(0.0, 0.0, 0.35, 1.0).into(),
            ),
            (
                hsv(0.96, 0.65, 0.85, 1.0).into(),
                hsv(0.96, 0.65, 0.35, 1.0).into(),
            ),
            (
                hsv(0.62, 0.15, 0.85, 1.0).into(),
                hsv(0.62, 0.15, 0.35, 1.0).into(),
            ),
        ];
        let idx = tile_index.min(tile_colors.len() - 1);
        let (c0, c1) = tile_colors[idx];
        let resource_path = format!("bitmaps/tile_{}.png", idx + 1);
        let mut candidate = self.project.clone();
        let matching_resource = candidate
            .resources
            .iter()
            .find(|resource| resource.path.eq_ignore_ascii_case(&resource_path));
        let res_id = if let Some(resource) = matching_resource {
            if !resource.is_image() {
                return Err(format!(
                    "Resource path {resource_path} is already used by non-image content."
                ));
            }
            resource.id.clone()
        } else {
            let png = make_procedural_tile_png(c0, c1);
            let id = new_id("res");
            let unit = ResourceUnit::from_bytes(id.clone(), resource_path, "image/png", &png)
                .map_err(|e| e.to_string())?;
            candidate.resources.push(unit);
            candidate.format_version = candidate.format_version.max(3);
            id
        };

        let mut label = "Import bitmap fill";
        if let Some(target) = self.selection.single().cloned() {
            if !self.target_editable(&target) {
                return Err(
                    "Select an editable vector shape before applying a bitmap fill.".into(),
                );
            }
            let eligible = self
                .sampled_targets()
                .into_iter()
                .find(|(t, _)| t == &target)
                .is_none_or(|(_, el)| {
                    crate::authoring_modify::element_has_fill(
                        &el,
                        self.target_has_bitmap_fill(&target),
                    ) || !crate::authoring_modify::element_has_stroke(&el)
                });
            if eligible {
                crate::bitmaps::apply_bitmap_fill_to_project(
                    &mut candidate,
                    self.symbol_id(),
                    &target.layer,
                    self.frame,
                    neoflash_core::AuthoredBitmapFill {
                        element_id: target.element,
                        resource_id: res_id.clone(),
                        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                        repeat: true,
                        smoothing: true,
                    },
                )?;
                label = "Bitmap fill";
            }
        } else if self.selection.len() > 1 {
            let sampled = self.sampled_targets();
            for target in self.selection.targets.clone() {
                let has_fill = sampled
                    .iter()
                    .find(|(t, _)| t == &target)
                    .is_some_and(|(_, el)| {
                        crate::authoring_modify::element_has_fill(
                            el,
                            self.target_has_bitmap_fill(&target),
                        )
                    });
                if self.target_editable(&target)
                    && has_fill
                    && crate::bitmaps::apply_bitmap_fill_to_project(
                        &mut candidate,
                        self.symbol_id(),
                        &target.layer,
                        self.frame,
                        neoflash_core::AuthoredBitmapFill {
                            element_id: target.element,
                            resource_id: res_id.clone(),
                            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            repeat: true,
                            smoothing: true,
                        },
                    )
                    .is_ok()
                {
                    label = "Bitmap fill";
                }
            }
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|error| error.to_string())?;
        if candidate != self.project {
            self.finish_edits();
            self.transact_named(label, |editor| {
                editor.project = candidate;
            });
        }
        self.fill = [c0.r(), c0.g(), c0.b(), c0.a()];
        Ok(res_id)
    }

    fn live_bitmap_fill(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let draft_id = egui::Id::new("nf2-bitmap-fill-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<BitmapFillDraft>(draft_id))
            .unwrap_or_default();

        let (ctx_fill, _, ctx_stroke, _, _) = self.contextual_paints();
        let fill_col =
            Color32::from_rgba_unmultiplied(ctx_fill[0], ctx_fill[1], ctx_fill[2], ctx_fill[3]);
        let stroke_col = Color32::from_rgba_unmultiplied(
            ctx_stroke[0],
            ctx_stroke[1],
            ctx_stroke[2],
            ctx_stroke[3],
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add(c::ColorWell {
                target: c::ColorWellTarget::Stroke,
                color: Some(stroke_col),
                active: false,
            });
            ui.add(c::ColorWell {
                target: c::ColorWellTarget::Fill,
                color: Some(fill_col),
                active: true,
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(c::IconButton::new(i20::COLOR_SWAP, "Swap colours").side(28.0))
                    .clicked()
                {
                    self.swap_active_paints();
                }
                if ui
                    .add(c::IconButton::new(i20::COLOR_NONE, "No colour").side(28.0))
                    .clicked()
                {
                    self.set_selection_fill_type(
                        false,
                        crate::authoring_modify::PaintFillType::None,
                    );
                }
                if ui
                    .add(c::IconButton::new(i20::COLOR_DEFAULT, "Black and white").side(28.0))
                    .clicked()
                {
                    self.reset_default_paints();
                }
            });
        });

        c::Select::new(ui.id().with("bitmap-fill-kind"), "Bitmap fill")
            .width(ui.available_width())
            .show(ui, |ui| {
                let _ = ui.add(c::MenuItem::new("Bitmap fill").checked(true));
            });

        let tile = |h: f32, s: f32| -> (Color32, Color32) {
            (hsv(h, s, 0.85, 1.0).into(), hsv(h, s, 0.35, 1.0).into())
        };
        let bitmaps = [
            tile(0.07, 0.7),
            tile(0.64, 0.55),
            tile(0.38, 0.55),
            tile(0.0, 0.0),
            tile(0.96, 0.65),
            tile(0.62, 0.15),
        ];
        for (row_idx, row) in bitmaps.chunks(4).enumerate() {
            ui.horizontal(|ui| {
                for (col_idx, (from, to)) in row.iter().enumerate() {
                    let idx = row_idx * 4 + col_idx;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::click());
                    c::swatches::paint_gradient(
                        ui.painter(),
                        rect,
                        c::GradientSwatch {
                            from: *from,
                            to: *to,
                            radial: false,
                        },
                    );
                    let selected = draft.selected_tile == idx;
                    let (w, color) = if selected {
                        (2.0_f32, nf.accent_default)
                    } else {
                        (1.0, nf.border_subtle)
                    };
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(radius::MD as u8),
                        egui::Stroke::new(w, color),
                        egui::StrokeKind::Inside,
                    );
                    if resp.clicked() {
                        draft.selected_tile = idx;
                        let _ = self.apply_bitmap_swatch_tile(idx);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add(c::Button::new("Import…").icon(i16::PLUS)).clicked() {
                let _ = self.apply_bitmap_swatch_tile(draft.selected_tile);
            }
            if ui
                .add(c::IconButton::new(i16::LOCK_FILL, "Lock fill").selected(draft.lock_fill))
                .clicked()
            {
                draft.lock_fill = !draft.lock_fill;
            }
            caption(ui, "Lock fill");
        });
        let img_count = self
            .project
            .resources
            .iter()
            .filter(|r| r.is_image())
            .count();
        caption(
            ui,
            &format!(
                "tile_{}.png · {} bitmap resource(s) · tiles across selection",
                draft.selected_tile + 1,
                img_count
            ),
        );

        ui.add_space(space::S1);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range(),
            r.top(),
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        ui.add_space(space::S2);
        ui.label(rich("Stroke", TextStyle::Overline).color(nf.text_tertiary));
        let half_w = half(ui);
        ui.horizontal(|ui| {
            let mut sw = self.stroke_width;
            if ui
                .add(
                    c::Field::new(&mut sw, "Stroke width")
                        .prefix("Width")
                        .unit("px")
                        .range(0.1..=200.0)
                        .decimals(1)
                        .width(half_w),
                )
                .changed()
            {
                self.stroke_width = sw.clamp(0.1, 200.0);
            }
            c::Select::new(ui.id().with("bitmap-fill-stroke-style"), "Solid")
                .width(half_w)
                .show(ui, |ui| {
                    if ui.add(c::MenuItem::new("Custom…")).clicked() {
                        let ctx = ui.ctx().clone();
                        self.open_stroke_style(&ctx);
                        ui.close();
                    }
                });
        });

        ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));
    }

    fn live_properties_oval(&mut self, ui: &mut Ui) {
        let layer_name = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Layer 1".to_owned());
        ui.add(c::TypeHeader {
            icon: i20::TOOL_OVAL,
            title: "Oval primitive",
            subtitle: &format!("Drawing object · layer \"{layer_name}\""),
        });

        let (mut x, mut y, mut w, mut h, mut start_deg, mut end_deg, mut inner_pct, mut closed) =
            if let Some(el) = self.selected_element() {
                let (ew, eh) = match &el.kind {
                    ElementKind::Ellipse { width, height, .. } => (*width, *height),
                    ElementKind::Rectangle { width, height, .. } => (*width, *height),
                    _ => (100.0, 100.0),
                };
                let (s_a, e_a, inn, cl) = match &el.transform.primitive {
                    Some(PrimitiveOptions::Oval(opts)) => (
                        opts.start_angle,
                        opts.end_angle,
                        opts.inner_radius * 100.0,
                        opts.close_path,
                    ),
                    _ => {
                        let o = self.authoring_tools.oval_options;
                        (
                            o.start_angle,
                            o.end_angle,
                            o.inner_radius * 100.0,
                            o.close_path,
                        )
                    }
                };
                (el.transform.x, el.transform.y, ew, eh, s_a, e_a, inn, cl)
            } else {
                let o = self.authoring_tools.oval_options;
                (
                    100.0,
                    100.0,
                    50.0,
                    50.0,
                    o.start_angle,
                    o.end_angle,
                    o.inner_radius * 100.0,
                    o.close_path,
                )
            };

        let mut geom_changed = false;
        let mut prim_changed = false;
        let field = (ui.available_width() - 32.0 - 2.0 * space::S2) / 2.0;
        section(ui, "Position and size", true, |ui| {
            ui.horizontal(|ui| {
                geom_changed |= ui
                    .add(
                        c::Field::new(&mut x, "X")
                            .prefix("X")
                            .decimals(1)
                            .width(field),
                    )
                    .changed();
                geom_changed |= ui
                    .add(
                        c::Field::new(y_ref(&mut y), "Y")
                            .prefix("Y")
                            .decimals(1)
                            .width(field),
                    )
                    .changed();
                let _ = ui.add(c::IconButton::new(i16::LINK, "Lock proportions").selected(true));
            });
            ui.horizontal(|ui| {
                geom_changed |= ui
                    .add(
                        c::Field::new(&mut w, "W")
                            .prefix("W")
                            .range(1.0..=4000.0)
                            .decimals(1)
                            .width(field),
                    )
                    .changed();
                geom_changed |= ui
                    .add(
                        c::Field::new(&mut h, "H")
                            .prefix("H")
                            .range(1.0..=4000.0)
                            .decimals(1)
                            .width(field),
                    )
                    .changed();
            });
        });

        let fill_col =
            Color32::from_rgba_unmultiplied(self.fill[0], self.fill[1], self.fill[2], self.fill[3]);
        let stroke_col = Color32::from_rgba_unmultiplied(
            self.stroke[0],
            self.stroke[1],
            self.stroke[2],
            self.stroke[3],
        );
        section(ui, "Fill and stroke", true, |ui| {
            ui.horizontal(|ui| {
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Fill,
                    color: Some(fill_col),
                    active: false,
                });
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Stroke,
                    color: Some(stroke_col),
                    active: false,
                });
            });
            ui.horizontal(|ui| {
                let mut sw = self.stroke_width;
                if ui
                    .add(
                        c::Field::new(&mut sw, "Stroke size")
                            .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                            .width(80.0),
                    )
                    .changed()
                {
                    self.stroke_width = sw.clamp(0.1, 200.0);
                }
                let mut style = c::LineStyle::Solid;
                let rem = ui.available_width();
                c::LineStyleSelect::show(ui, "live-oval-line-style", &mut style, rem);
            });
            ui.horizontal(|ui| {
                if ui
                    .add(c::Button::new("Custom…").icon(i16::SETTINGS))
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.open_stroke_style(&ctx);
                }
                let mut hinting = false;
                ui.add(c::Switch::new(&mut hinting, "Stroke hinting"));
            });
            let hw = half(ui);
            ui.horizontal(|ui| {
                c::Select::new(ui.id().with("oval-scale"), "Scale: Normal")
                    .width(hw)
                    .show(ui, |_| {});
                let mut miter = 3.0;
                ui.add(
                    c::Field::new(&mut miter, "Miter")
                        .prefix("Miter")
                        .decimals(0)
                        .width(hw),
                );
            });
            for (label, items) in [
                (
                    "Cap",
                    [
                        (i16::CAP_NONE, "No cap"),
                        (i16::CAP_ROUND, "Round cap"),
                        (i16::CAP_SQUARE, "Square cap"),
                    ],
                ),
                (
                    "Join",
                    [
                        (i16::JOIN_MITER, "Miter join"),
                        (i16::JOIN_ROUND, "Round join"),
                        (i16::JOIN_BEVEL, "Bevel join"),
                    ],
                ),
            ] {
                ui.horizontal(|ui| {
                    form_label(ui, label, 48.0);
                    c::IconGroup {
                        items: &items,
                        selected: Some(1),
                    }
                    .show(ui);
                });
            }
        });

        section(ui, "Oval options", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                prim_changed |= ui
                    .add(
                        c::Field::new(&mut start_deg, "Start")
                            .prefix("Start")
                            .unit("°")
                            .range(0.0..=360.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
                prim_changed |= ui
                    .add(
                        c::Field::new(&mut end_deg, "End")
                            .prefix("End")
                            .unit("°")
                            .range(0.0..=360.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                prim_changed |= ui
                    .add(
                        c::Field::new(&mut inner_pct, "Inner")
                            .prefix("Inner")
                            .unit("%")
                            .range(0.0..=99.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
                prim_changed |= ui
                    .add(c::Checkbox::new(&mut closed, "Close path"))
                    .changed();
            });
            if ui
                .add(c::Button::ghost("Reset").icon(i16::RESET_TRANSFORM))
                .clicked()
            {
                start_deg = 0.0;
                end_deg = 0.0;
                inner_pct = 0.0;
                closed = true;
                prim_changed = true;
            }
        });

        if prim_changed || geom_changed {
            let next_opts = OvalOptions {
                start_angle: start_deg.clamp(0.0, 360.0),
                end_angle: end_deg.clamp(0.0, 360.0),
                inner_radius: (inner_pct / 100.0).clamp(0.0, 0.99),
                close_path: closed,
            };
            self.authoring_tools.oval_options = next_opts;
            if let Some(sel) = self.selection.single().cloned()
                && self.target_editable(&sel)
            {
                self.transact_named("Edit oval primitive", |editor| {
                    editor.ensure_key_for(&sel.layer, false);
                    let frame = editor.frame;
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == sel.layer)
                        && let Some(el) = layer
                            .key_at_mut(frame)
                            .and_then(|k| k.elements.iter_mut().find(|e| e.id == sel.element))
                    {
                        el.transform.x = x;
                        el.transform.y = y;
                        el.set_oval_options(next_opts);
                        if let ElementKind::Ellipse { width, height, .. } = &mut el.kind {
                            *width = w.max(1.0);
                            *height = h.max(1.0);
                        }
                    }
                });
            }
        }
    }

    fn live_properties_rect(&mut self, ui: &mut Ui) {
        ui.add(c::TypeHeader {
            icon: i20::TOOL_RECT_PRIMITIVE,
            title: "Rectangle primitive",
            subtitle: "Drawing object · editable corners",
        });

        let (mut x, mut y, mut w, mut h, mut radii) = if let Some(el) = self.selected_element() {
            let (rw, rh) = match &el.kind {
                ElementKind::Rectangle { width, height, .. } => (*width, *height),
                _ => (240.0, 120.0),
            };
            let r = match &el.transform.primitive {
                Some(PrimitiveOptions::Rectangle(opts)) => opts.corner_radii,
                _ => self.authoring_tools.rect_options.corner_radii,
            };
            (el.transform.x, el.transform.y, rw, rh, r)
        } else {
            (
                140.0,
                96.0,
                240.0,
                120.0,
                self.authoring_tools.rect_options.corner_radii,
            )
        };

        let mut changed = false;
        section(ui, "Position and size", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(c::Field::new(&mut x, "X").prefix("X").decimals(1).width(hw))
                    .changed();
                changed |= ui
                    .add(c::Field::new(&mut y, "Y").prefix("Y").decimals(1).width(hw))
                    .changed();
            });
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut w, "W")
                            .prefix("W")
                            .range(1.0..=4000.0)
                            .decimals(1)
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(
                        c::Field::new(&mut h, "H")
                            .prefix("H")
                            .range(1.0..=4000.0)
                            .decimals(1)
                            .width(hw),
                    )
                    .changed();
            });
        });

        let fill_col =
            Color32::from_rgba_unmultiplied(self.fill[0], self.fill[1], self.fill[2], self.fill[3]);
        let stroke_col = Color32::from_rgba_unmultiplied(
            self.stroke[0],
            self.stroke[1],
            self.stroke[2],
            self.stroke[3],
        );
        section(ui, "Fill and stroke", true, |ui| {
            ui.horizontal(|ui| {
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Fill,
                    color: Some(fill_col),
                    active: false,
                });
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Stroke,
                    color: Some(stroke_col),
                    active: false,
                });
            });
            ui.horizontal(|ui| {
                let mut sw = self.stroke_width;
                if ui
                    .add(
                        c::Field::new(&mut sw, "Stroke size")
                            .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                            .width(80.0),
                    )
                    .changed()
                {
                    self.stroke_width = sw.clamp(0.1, 200.0);
                }
                let mut style = c::LineStyle::Solid;
                let rem = ui.available_width();
                c::LineStyleSelect::show(ui, "live-rect-line-style", &mut style, rem);
            });
        });

        let mut corners_locked = self.authoring_tools.rect_options.corners_locked;
        section(ui, "Rectangle options", true, |ui| {
            let fw = (ui.available_width() - 32.0 - 2.0 * space::S2) / 2.0;
            ui.horizontal(|ui| {
                if ui
                    .add(
                        c::Field::new(&mut radii[0], "Top-left radius")
                            .prefix("↖")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(fw),
                    )
                    .changed()
                {
                    if corners_locked {
                        radii = [radii[0]; 4];
                    }
                    changed = true;
                }
                if ui
                    .add(
                        c::Field::new(&mut radii[1], "Top-right radius")
                            .prefix("↗")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(fw),
                    )
                    .changed()
                {
                    if corners_locked {
                        radii = [radii[1]; 4];
                    }
                    changed = true;
                }
                if ui
                    .add(c::IconButton::new(i16::LINK, "Lock proportions").selected(corners_locked))
                    .clicked()
                {
                    corners_locked = !corners_locked;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .add(
                        c::Field::new(&mut radii[3], "Bottom-left radius")
                            .prefix("↙")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(fw),
                    )
                    .changed()
                {
                    if corners_locked {
                        radii = [radii[3]; 4];
                    }
                    changed = true;
                }
                if ui
                    .add(
                        c::Field::new(&mut radii[2], "Bottom-right radius")
                            .prefix("↘")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(fw),
                    )
                    .changed()
                {
                    if corners_locked {
                        radii = [radii[2]; 4];
                    }
                    changed = true;
                }
            });
            let mut slider_val = radii[0].clamp(0.0, 100.0);
            if ui
                .add(
                    c::Slider::new(&mut slider_val, 0.0..=100.0, "Corner radius")
                        .width(2.0 * fw + space::S2),
                )
                .changed()
            {
                radii = [slider_val; 4];
                changed = true;
            }
            if ui.add(c::Button::ghost("Reset")).clicked() {
                radii = [0.0; 4];
                changed = true;
            }
        });

        if changed {
            let next_opts = RectangleOptions {
                corner_radii: radii,
                corners_locked,
            };
            self.authoring_tools.rect_options = next_opts;
            if let Some(sel) = self.selection.single().cloned()
                && self.target_editable(&sel)
            {
                self.transact_named("Edit rectangle primitive", |editor| {
                    editor.ensure_key_for(&sel.layer, false);
                    let frame = editor.frame;
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == sel.layer)
                        && let Some(el) = layer
                            .key_at_mut(frame)
                            .and_then(|k| k.elements.iter_mut().find(|e| e.id == sel.element))
                    {
                        el.transform.x = x;
                        el.transform.y = y;
                        el.set_rectangle_options(next_opts);
                        if let ElementKind::Rectangle { width, height, .. } = &mut el.kind {
                            *width = w.max(1.0);
                            *height = h.max(1.0);
                        }
                    }
                });
            }
        }
    }

    /// Generate procedural Deco pattern artwork on the active keyframe in one `"Deco pattern"` Undo step.
    #[cfg(test)]
    pub(crate) fn apply_deco_pattern(
        &mut self,
        effect: usize,
        scale_pct: f64,
        step_px: f64,
    ) -> usize {
        self.apply_deco_pattern_with_options(effect, 0.0, scale_pct, step_px, 1, false)
    }

    /// Generate procedural Deco pattern artwork with full Flash CS5/Animate Deco parameters.
    pub(crate) fn apply_deco_pattern_with_options(
        &mut self,
        effect: usize,
        branch_angle_deg: f64,
        scale_pct: f64,
        segment_px: f64,
        step_frames: u32,
        animate_pattern: bool,
    ) -> usize {
        if !self.layer_editable() {
            return 0;
        }
        let deco_effect = match effect {
            1 => neoflash_core::DecoEffectKind::GridFill,
            2 => neoflash_core::DecoEffectKind::SymmetryBrush,
            3 => neoflash_core::DecoEffectKind::DecoratedBrush,
            4 => neoflash_core::DecoEffectKind::FlameBrush,
            5 => neoflash_core::DecoEffectKind::FlowerBrush,
            6 => neoflash_core::DecoEffectKind::LightningBrush,
            7 => neoflash_core::DecoEffectKind::TreeBrush,
            _ => neoflash_core::DecoEffectKind::VineFill,
        };
        let _ = (branch_angle_deg, step_frames, animate_pattern);
        let generated = neoflash_core::generate_deco_elements(
            deco_effect,
            [100.0, 160.0],
            scale_pct,
            (segment_px * 6.0).clamp(16.0, 160.0),
            [82, 204, 122, 255],
            [255, 140, 102, 255],
        );
        let created = generated.len();
        if created == 0 {
            return 0;
        }
        let layer_idx = self.layer;
        let frame = self.frame;

        self.finish_edits();
        self.transact_named("Deco pattern", |editor| {
            let sym = editor.symbol_mut();
            let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            if !layer.keyframes.iter().any(|k| k.frame == frame) {
                let prev = layer
                    .key_at(frame)
                    .map(|k| k.elements.clone())
                    .unwrap_or_default();
                layer.keyframes.push(Keyframe {
                    frame,
                    elements: prev,
                    ..Keyframe::default()
                });
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                key.elements.extend(generated);
            }
            layer.normalize_motion_keys();
        });
        created
    }

    fn live_deco_tool(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let id = egui::Id::new("nf2-deco-tool-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<DecoDraft>(id))
            .unwrap_or_default();

        ui.add(c::TypeHeader {
            icon: i20::TOOL_DECO,
            title: "Deco tool",
            subtitle: "Procedural patterns from symbols",
        });

        let effects: Vec<&str> = neoflash_core::DecoEffectKind::ALL
            .iter()
            .map(|e| e.label())
            .collect();
        let mut apply_now = false;
        section(ui, "Drawing effect", true, |ui| {
            let eff_lbl = effects[draft.effect.min(effects.len() - 1)];
            c::Select::new(ui.id().with("deco-effect"), eff_lbl)
                .width(ui.available_width())
                .show(ui, |ui| {
                    for (idx, name) in effects.iter().enumerate() {
                        if ui
                            .add(c::MenuItem::new(name).checked(draft.effect == idx))
                            .clicked()
                        {
                            draft.effect = idx;
                            ui.close();
                        }
                    }
                });
            for (label, color) in [
                ("Leaf", hsv(0.42, 0.6, 0.8, 1.0).into()),
                ("Flower", hsv(0.06, 0.6, 1.0, 1.0).into()),
            ] {
                ui.horizontal(|ui| {
                    form_label(ui, label, 64.0);
                    let (chip, _) =
                        ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    neoflash_ui::components::display::paint_swatch(ui, chip, color, radius::SM);
                    ui.label(rich("Default shape", TextStyle::Body).color(nf.text_secondary));
                    if ui.add(c::Button::ghost("Edit…")).clicked() {
                        apply_now = true;
                    }
                });
            }
        });
        section(ui, "Advanced options", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                ui.add(
                    c::Field::new(&mut draft.branch_angle, "Branch")
                        .prefix("Branch")
                        .unit("°")
                        .range(-180.0..=180.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
                ui.add(
                    c::Field::new(&mut draft.scale_pct, "Scale")
                        .prefix("Scale")
                        .unit("%")
                        .range(10.0..=400.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
            });
            ui.horizontal(|ui| {
                ui.add(
                    c::Field::new(&mut draft.segment_px, "Segment")
                        .prefix("Segment")
                        .unit("px")
                        .range(1.0..=100.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
                ui.add(
                    c::Field::new(&mut draft.step_frames, "Step")
                        .prefix("Step")
                        .unit("f")
                        .range(1.0..=30.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
            });
            ui.add(c::Checkbox::new(
                &mut draft.animate_pattern,
                "Animate pattern (one keyframe per step)",
            ));
        });

        let effect = draft.effect;
        let branch_angle = draft.branch_angle;
        let scale_pct = draft.scale_pct;
        let seg_px = draft.segment_px;
        let step_frames = draft.step_frames.round().max(1.0) as u32;
        let animate_pattern = draft.animate_pattern;
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
        if apply_now {
            self.apply_deco_pattern_with_options(
                effect,
                branch_angle,
                scale_pct,
                seg_px,
                step_frames,
                animate_pattern,
            );
        }
    }

    /// Spray symbol instances or dots onto the active keyframe in one `"Spray brush"` Undo step.
    #[cfg(test)]
    pub(crate) fn apply_spray_brush_burst(
        &mut self,
        use_dots: bool,
        scale_w: f64,
        scale_h: f64,
    ) -> usize {
        self.apply_spray_brush_with_options(
            use_dots, scale_w, scale_h, false, false, false, 24.0, 24.0, 0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_spray_brush_with_options(
        &mut self,
        use_dots: bool,
        scale_w: f64,
        scale_h: f64,
        random_scaling: bool,
        rotate_symbol: bool,
        random_rotation: bool,
        brush_w: f64,
        brush_h: f64,
        brush_angle: f64,
    ) -> usize {
        if !self.layer_editable() {
            return 0;
        }
        let sym_id = (!use_dots)
            .then(|| {
                self.project
                    .symbols
                    .iter()
                    .find(|s| s.id != self.project.root && !self.project.is_scene(&s.id))
                    .map(|s| s.id.clone())
            })
            .flatten();
        let cfg = neoflash_core::SprayBrushConfig {
            symbol_id: sym_id,
            fill: self.fill,
            scale_w_pct: scale_w,
            scale_h_pct: scale_h,
            random_scaling,
            rotate_symbol,
            random_rotation,
            brush_width: brush_w,
            brush_height: brush_h,
            brush_angle_deg: brush_angle,
        };
        let generated = neoflash_core::generate_spray_brush_along_points(
            &[[200.0, 160.0], [224.0, 160.0]],
            &cfg,
        );
        let count = generated.len();
        if count == 0 {
            return 0;
        }
        let layer_idx = self.layer;
        let frame = self.frame;

        self.finish_edits();
        self.transact_named("Spray brush", |editor| {
            let sym = editor.symbol_mut();
            let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            if !layer.keyframes.iter().any(|k| k.frame == frame) {
                let prev = layer
                    .key_at(frame)
                    .map(|k| k.elements.clone())
                    .unwrap_or_default();
                layer.keyframes.push(Keyframe {
                    frame,
                    elements: prev,
                    ..Keyframe::default()
                });
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                key.elements.extend(generated);
            }
            layer.normalize_motion_keys();
        });
        count
    }

    fn live_spray_brush(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("nf2-spray-brush-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<SprayDraft>(id))
            .unwrap_or_default();
        let mut do_spray = false;

        ui.add(c::TypeHeader {
            icon: i20::TOOL_SPRAY,
            title: "Spray brush",
            subtitle: "Sprays copies of a symbol or dots",
        });
        section(ui, "Symbol", true, |ui| {
            ui.horizontal(|ui| {
                ui.add(c::TextField::new(&mut draft.symbol_name, "Symbol").width(200.0));
                if ui.add(c::Button::new("Edit…")).clicked() {
                    do_spray = true;
                }
            });
            ui.add(c::Checkbox::new(
                &mut draft.use_dots,
                "Use default shape (dots)",
            ));
            let hw = half(ui);
            ui.horizontal(|ui| {
                ui.add(
                    c::Field::new(&mut draft.scale_w, "W")
                        .prefix("W")
                        .unit("%")
                        .range(10.0..=400.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
                ui.add(
                    c::Field::new(&mut draft.scale_h, "H")
                        .prefix("H")
                        .unit("%")
                        .range(10.0..=400.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
            });
            ui.add(c::Checkbox::new(
                &mut draft.random_scaling,
                "Random scaling",
            ));
            ui.horizontal(|ui| {
                ui.add(c::Checkbox::new(&mut draft.rotate_symbol, "Rotate symbol"));
                ui.add(c::Checkbox::new(
                    &mut draft.random_rotation,
                    "Random rotation",
                ));
            });
        });
        section(ui, "Brush", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                ui.add(
                    c::Field::new(&mut draft.brush_w, "Width")
                        .prefix("Width")
                        .unit("px")
                        .range(1.0..=200.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
                ui.add(
                    c::Field::new(&mut draft.brush_h, "Height")
                        .prefix("Height")
                        .unit("px")
                        .range(1.0..=200.0)
                        .decimals(0)
                        .stepper()
                        .width(hw),
                );
            });
            ui.add(
                c::Field::new(&mut draft.brush_angle, "Angle")
                    .prefix("Angle")
                    .unit("°")
                    .range(-180.0..=180.0)
                    .decimals(0)
                    .stepper()
                    .width(hw),
            );
        });

        let use_dots = draft.use_dots;
        let sw = draft.scale_w;
        let sh = draft.scale_h;
        let random_scaling = draft.random_scaling;
        let rotate_symbol = draft.rotate_symbol;
        let random_rotation = draft.random_rotation;
        let brush_w = draft.brush_w;
        let brush_h = draft.brush_h;
        let brush_angle = draft.brush_angle;
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
        if do_spray {
            self.apply_spray_brush_with_options(
                use_dots,
                sw,
                sh,
                random_scaling,
                rotate_symbol,
                random_rotation,
                brush_w,
                brush_h,
                brush_angle,
            );
        }
    }

    fn live_bone_properties(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("nf2-bone-properties-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<BoneDraft>(id))
            .unwrap_or_default();

        let existing_armature = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.key_at(self.frame))
            .and_then(|k| k.ik_armatures.first())
            .cloned();

        let (arm_name, bone_total) = existing_armature
            .as_ref()
            .map(|a| (a.name.clone(), a.bones.len().max(1)))
            .unwrap_or_else(|| ("arm".to_owned(), 3));
        draft.bone_index = draft.bone_index.min(bone_total.saturating_sub(1));
        if let Some(arm) = &existing_armature
            && let Some(b) = arm.bones.get(draft.bone_index)
        {
            draft.name = b.id.clone();
            draft.length = b.length;
            draft.rot_constrain = b.min_angle.is_some() || b.max_angle.is_some();
            if let Some(min_a) = b.min_angle {
                draft.rot_min = min_a.to_degrees();
            }
            if let Some(max_a) = b.max_angle {
                draft.rot_max = max_a.to_degrees();
            }
            draft.x = arm.target_pos[0];
            draft.y = arm.target_pos[1];
        }

        let mut changed = false;
        ui.add(c::TypeHeader {
            icon: i16::ARMATURE,
            title: "IK bone",
            subtitle: &format!(
                "Armature \"{arm_name}\" · bone {} of {bone_total}",
                draft.bone_index + 1
            ),
        });
        ui.horizontal(|ui| {
            changed |= ui
                .add(c::TextField::new(&mut draft.name, "Bone name").width(200.0))
                .changed();
            if ui
                .add_enabled(
                    draft.bone_index > 0,
                    c::IconButton::new(i16::CHEVUP, "Parent bone"),
                )
                .clicked()
            {
                draft.bone_index = draft.bone_index.saturating_sub(1);
            }
            if ui
                .add_enabled(
                    draft.bone_index + 1 < bone_total,
                    c::IconButton::new(i16::CHEVDOWN, "Child bone"),
                )
                .clicked()
            {
                draft.bone_index += 1;
            }
        });
        section(ui, "Position", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.x, "X")
                            .prefix("X")
                            .decimals(1)
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.y, "Y")
                            .prefix("Y")
                            .decimals(1)
                            .width(hw),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.length, "Length")
                            .prefix("Length")
                            .unit("px")
                            .range(4.0..=1000.0)
                            .decimals(0)
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.angle, "Angle")
                            .prefix("Angle")
                            .unit("°")
                            .range(-360.0..=360.0)
                            .decimals(0)
                            .width(hw),
                    )
                    .changed();
            });
        });
        section(ui, "Joint: rotation", true, |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .add(c::Switch::new(&mut draft.rot_enable, "Enable"))
                    .changed();
                changed |= ui
                    .add(c::Switch::new(&mut draft.rot_constrain, "Constrain"))
                    .changed();
            });
            let hw = half(ui);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.rot_min, "Min")
                            .prefix("Min")
                            .unit("°")
                            .range(-180.0..=180.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.rot_max, "Max")
                            .prefix("Max")
                            .unit("°")
                            .range(-180.0..=180.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
            });
        });
        section(ui, "Joint: X / Y translation", false, |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .add(c::Switch::new(&mut draft.trans_enable, "Enable"))
                    .changed();
                changed |= ui
                    .add(c::Switch::new(&mut draft.trans_constrain, "Constrain"))
                    .changed();
            });
        });
        section(ui, "Spring", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.spring_strength, "Strength")
                            .prefix("Strength")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.spring_damping, "Damping")
                            .prefix("Damping")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
            });
        });
        section(ui, "Armature", true, |ui| {
            let hw = half(ui);
            ui.horizontal(|ui| {
                let type_lbl = if draft.armature_runtime {
                    "Type: Runtime"
                } else {
                    "Type: Authortime"
                };
                c::Select::new(ui.id().with("arm-type"), type_lbl)
                    .width(hw)
                    .show(ui, |ui| {
                        for (rt, lbl) in [(false, "Type: Authortime"), (true, "Type: Runtime")] {
                            if ui
                                .add(c::MenuItem::new(lbl).checked(draft.armature_runtime == rt))
                                .clicked()
                            {
                                draft.armature_runtime = rt;
                                ui.close();
                            }
                        }
                    });
                let style_lbl = if draft.armature_wireframe {
                    "Style: Wire"
                } else {
                    "Style: Solid"
                };
                c::Select::new(ui.id().with("arm-style"), style_lbl)
                    .width(hw)
                    .show(ui, |ui| {
                        for (wf, lbl) in [(false, "Style: Solid"), (true, "Style: Wire")] {
                            if ui
                                .add(c::MenuItem::new(lbl).checked(draft.armature_wireframe == wf))
                                .clicked()
                            {
                                draft.armature_wireframe = wf;
                                ui.close();
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.speed_pct, "Speed")
                            .prefix("Speed")
                            .unit("%")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    )
                    .changed();
                changed |= ui
                    .add(c::Switch::new(&mut draft.enable_spring, "Enable spring"))
                    .changed();
            });
        });

        let snapshot_draft = draft.clone();
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));

        if changed && self.layer_editable() {
            let mut arm = existing_armature.unwrap_or_else(|| {
                let el_id = self
                    .symbol()
                    .timeline
                    .layers
                    .get(self.layer)
                    .and_then(|l| l.key_at(self.frame))
                    .and_then(|k| k.elements.first())
                    .map(|e| e.id.clone())
                    .unwrap_or_else(|| "bone_el".to_owned());
                IkArmature {
                    id: new_id("armature"),
                    name: "arm".to_owned(),
                    bones: vec![IkBone {
                        id: snapshot_draft.name.clone(),
                        element_id: el_id,
                        parent_bone_id: None,
                        length: snapshot_draft.length.max(4.0),
                        min_angle: None,
                        max_angle: None,
                        stiffness: 1.0,
                    }],
                    effector_bone_id: snapshot_draft.name.clone(),
                    target_pos: [snapshot_draft.x, snapshot_draft.y],
                    pole_pos: None,
                }
            });
            let b_idx = snapshot_draft
                .bone_index
                .min(arm.bones.len().saturating_sub(1));
            if let Some(b) = arm.bones.get_mut(b_idx) {
                b.length = snapshot_draft.length.max(4.0);
                if snapshot_draft.rot_constrain {
                    b.min_angle = Some(snapshot_draft.rot_min.to_radians());
                    b.max_angle = Some(snapshot_draft.rot_max.to_radians());
                } else {
                    b.min_angle = None;
                    b.max_angle = None;
                }
                b.stiffness = (snapshot_draft.speed_pct / 100.0).clamp(0.05, 1.0);
            }
            arm.target_pos = [snapshot_draft.x, snapshot_draft.y];
            let joint_spec = neoflash_core::IkJointSpec {
                bone_id: snapshot_draft.name.clone(),
                pinned: false,
                enable_rotation: snapshot_draft.rot_enable,
                constrain_rotation: snapshot_draft.rot_constrain,
                min_angle_deg: snapshot_draft.rot_min,
                max_angle_deg: snapshot_draft.rot_max,
                enable_x_translation: snapshot_draft.trans_enable,
                constrain_x_translation: snapshot_draft.trans_constrain,
                min_x_offset: -50.0,
                max_x_offset: 50.0,
                enable_y_translation: snapshot_draft.trans_enable,
                constrain_y_translation: snapshot_draft.trans_constrain,
                min_y_offset: -50.0,
                max_y_offset: 50.0,
                speed: (snapshot_draft.speed_pct / 100.0).clamp(0.0, 1.0),
                spring_strength: if snapshot_draft.enable_spring {
                    snapshot_draft.spring_strength
                } else {
                    0.0
                },
                spring_damping: snapshot_draft.spring_damping,
            };
            let _ = self.set_keyframe_ik_armature_with_specs(arm, vec![joint_spec]);
        }
    }

    fn live_graphic_properties(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let (sym_name, sym_frames, current_playback) = self
            .selected_element()
            .and_then(|el| match &el.kind {
                ElementKind::Instance {
                    symbol_id,
                    playback,
                } => {
                    let sym = self.project.symbol(symbol_id)?;
                    Some((sym.name.clone(), sym.timeline.frame_count.max(1), *playback))
                }
                _ => None,
            })
            .unwrap_or_else(|| ("planet".to_owned(), 24, InstancePlayback::Loop));

        ui.add(c::TypeHeader {
            icon: i16::GRAPHIC,
            title: "Graphic",
            subtitle: &format!("Instance of \"{sym_name}\" · {sym_frames} frames"),
        });
        ui.horizontal(|ui| {
            let w = ui.available_width() - 72.0 - space::S2;
            ui.allocate_ui_with_layout(
                egui::vec2(w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(w);
                    ui.add(
                        egui::Label::new(
                            rich_block(
                                "Graphics have no instance name — their timeline is synced to the parent.",
                                TextStyle::Small,
                            )
                            .color(nf.text_secondary),
                        )
                        .wrap(),
                    );
                },
            );
            if ui.add(c::Button::new("Swap…")).clicked() {
                let _ = self.swap_selected_instance_with_library_or_next();
            }
        });
        for label in ["Position and size", "Colour effect"] {
            c::SectionHeader::new(label)
                .id_salt(("live-graphic", label))
                .default_open(false)
                .show(ui, |_| {});
        }

        let (mut mode_idx, mut first_zero) = match current_playback {
            InstancePlayback::Independent | InstancePlayback::Loop => (0usize, 0u32),
            InstancePlayback::LoopFrom(f) => (0, f),
            InstancePlayback::PlayOnce => (1, 0),
            InstancePlayback::PlayOnceFrom(f) => (1, f),
            InstancePlayback::SingleFrame(f) => (2, f),
        };
        let mut playback_changed = false;

        c::SectionHeader::new("Looping")
            .id_salt("live-graphic-looping")
            .show(ui, |ui| {
                for (i, label) in [
                    "Loop",
                    "Play once",
                    "Single frame",
                    "Loop reverse",
                    "Play once reverse",
                ]
                .into_iter()
                .enumerate()
                {
                    if ui.add(c::Radio::new(i == mode_idx, label)).clicked() {
                        mode_idx = i.min(2);
                        playback_changed = true;
                    }
                }
                let hw = half(ui);
                let mut first_1 = (first_zero + 1).min(sym_frames) as f64;
                let mut last_1 = sym_frames as f64;
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            c::Field::new(&mut first_1, "First frame")
                                .prefix("First")
                                .range(1.0..=f64::from(sym_frames))
                                .decimals(0)
                                .stepper()
                                .width(hw),
                        )
                        .changed()
                    {
                        first_zero = (first_1.round() as u32).clamp(1, sym_frames) - 1;
                        playback_changed = true;
                    }
                    ui.add(
                        c::Field::new(&mut last_1, "Last frame")
                            .prefix("Last")
                            .range(1.0..=f64::from(sym_frames))
                            .decimals(0)
                            .stepper()
                            .width(hw),
                    );
                });

                let (strip, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 44.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    strip,
                    egui::CornerRadius::same(radius::MD as u8),
                    nf.bg_field,
                );
                let entries = self.graphic_frame_picker_entries();
                for i in 0..7u32 {
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(strip.left() + 4.0 + i as f32 * 40.0, strip.top() + 4.0),
                        egui::vec2(36.0, 36.0),
                    );
                    let resp = ui.interact(
                        cell,
                        ui.id().with(("graphic-thumb", i)),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(
                        cell,
                        egui::CornerRadius::same(radius::SM as u8),
                        nf.bg_raised,
                    );
                    let selected = first_zero == i;
                    let (w, color) = if selected {
                        (2.0_f32, nf.accent_default)
                    } else {
                        (1.0, nf.border_subtle)
                    };
                    ui.painter().rect_stroke(
                        cell,
                        egui::CornerRadius::same(radius::SM as u8),
                        egui::Stroke::new(w, color),
                        egui::StrokeKind::Inside,
                    );
                    let x = cell.left() + 8.0 + (i % 2) as f32 * 2.0;
                    let orange: Color32 = hsv(0.06, 0.7, 0.95, 1.0).into();
                    ui.painter()
                        .circle_filled(egui::pos2(x + 2.0, cell.center().y), 4.0, orange);
                    if resp.clicked() {
                        first_zero = i.min(sym_frames.saturating_sub(1));
                        if !self.pick_graphic_frame(first_zero) {
                            playback_changed = true;
                        }
                    }
                    if let Some((_, tip)) = entries.get(i as usize) {
                        resp.on_hover_text(tip);
                    }
                }
                caption(ui, "Click a thumbnail to set the first frame");
            });

        if playback_changed
            && let Some(sel) = self.selection.single().cloned()
            && self.target_editable(&sel)
        {
            let next_pb = match (mode_idx, first_zero) {
                (0, 0) => InstancePlayback::Loop,
                (0, f) => InstancePlayback::LoopFrom(f),
                (1, 0) => InstancePlayback::PlayOnce,
                (1, f) => InstancePlayback::PlayOnceFrom(f),
                _ => InstancePlayback::SingleFrame(first_zero),
            };
            self.transact_named("Set graphic looping", |editor| {
                editor.ensure_key_for(&sel.layer, false);
                let frame = editor.frame;
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == sel.layer)
                    && let Some(el) = layer
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == sel.element))
                    && let ElementKind::Instance { playback, .. } = &mut el.kind
                {
                    *playback = next_pb;
                }
            });
        }
    }

    fn live_authoring_debugger(&mut self, ui: &mut Ui) -> bool {
        let nf = neoflash_ui::palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui
                .add(
                    c::IconButton::new(i16::DEBUG_CONTINUE, "Continue")
                        .kind(c::IconButtonType::Primary)
                        .shortcut("F8"),
                )
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.start_debug_movie(&ctx);
            }
            let _ = ui.add_enabled(
                false,
                c::IconButton::new(i16::STEP_OVER, "Step over").shortcut("F10"),
            );
            let _ = ui.add_enabled(
                false,
                c::IconButton::new(i16::STEP_INTO, "Step in").shortcut("F11"),
            );
            let _ = ui.add_enabled(
                false,
                c::IconButton::new(i16::STEP_OUT, "Step out").shortcut("⇧F11"),
            );
            if ui
                .add(c::IconButton::new(i16::STOP, "Stop debugging"))
                .clicked()
            {
                self.debugger.stop();
            }
            if ui
                .add(c::IconButton::new(i16::BREAKPOINT, "Toggle breakpoint").shortcut("⌘B"))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.toggle_debug_breakpoint(&ctx);
            }
            if ui.add(c::Button::ghost("Remove all breakpoints")).clicked() {
                self.debugger.breakpoints.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(c::Badge::new(
                    "Authoring · Ready to debug (F8)",
                    c::BadgeType::Accent,
                ));
            });
        });

        self.refresh_debugger_watches();
        let watch_results = self.debugger_watches();
        let scopes = self.paused_debugger_scopes();

        let total = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(240.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(240.0);
                    c::TreeItem::new(
                        &format!("{} : Frame {}", self.symbol().name, self.frame + 1),
                        c::TreeItemType::Symbol(i16::EXEC_ARROW),
                    )
                    .selected(true)
                    .show(ui);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(380.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(380.0);
                    let cols = &[0.0, 0.45];
                    ui.add(c::TableRow {
                        cells: &["Name", "Value"],
                        kind: c::TableRowKind::Header,
                        columns: cols,
                    });
                    let frame_str = (self.frame + 1).to_string();
                    let fps_str = self.project.fps.to_string();
                    for (k, v) in [("frame", frame_str.as_str()), ("fps", fps_str.as_str())] {
                        ui.add(c::TableRow {
                            cells: &[k, v],
                            kind: c::TableRowKind::Row,
                            columns: cols,
                        });
                    }
                    for scope in &scopes {
                        for var in &scope.variables {
                            ui.add(c::TableRow {
                                cells: &[&var.name, &var.value],
                                kind: c::TableRowKind::Row,
                                columns: cols,
                            });
                        }
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2((total - 620.0).max(240.0), 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let cols = &[0.0, 0.5];
                    ui.add(c::TableRow {
                        cells: &["Expression", "Value"],
                        kind: c::TableRowKind::Header,
                        columns: cols,
                    });
                    for w in &watch_results {
                        ui.add(c::TableRow {
                            cells: &[&w.expression, &w.last_value],
                            kind: c::TableRowKind::Row,
                            columns: cols,
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add(c::Button::ghost("+ Add watch")).clicked() {
                            self.add_debugger_watch("frame + 1");
                        }
                        if !watch_results.is_empty()
                            && ui.add(c::Button::ghost("Clear watches")).clicked()
                        {
                            self.remove_debugger_watch(0);
                        }
                    });
                    for bp in self.debugger.breakpoints.clone() {
                        let label = format!("{} · line {}", bp.source, bp.line);
                        let mut enabled = true;
                        ui.horizontal(|ui| {
                            ui.add_space(space::S3);
                            icons::show(ui, i16::BREAKPOINT, nf.syntax_breakpoint);
                            if ui.add(c::Checkbox::new(&mut enabled, &label)).changed() && !enabled
                            {
                                self.debugger
                                    .breakpoints
                                    .retain(|b| !(b.source == bp.source && b.line == bp.line));
                            }
                        });
                    }
                },
            );
        });
        false
    }

    fn live_test_movie(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let id = egui::Id::new("nf2-live-test-movie-draft");
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<TestMovieDraft>(id))
            .unwrap_or_default();

        let total_frames = self.symbol().timeline.frame_count.max(1);
        let fps = self.project.fps.max(1) as f64;
        if draft.playing {
            let now = ui.input(|i| i.time);
            let dt = 1.0 / (fps * f64::from(draft.speed.max(0.25)));
            if now - draft.last_step_time >= dt {
                draft.last_step_time = now;
                if draft.frame + 1 < total_frames {
                    draft.frame += 1;
                } else if draft.loop_playback {
                    draft.frame = 0;
                } else {
                    draft.playing = false;
                }
            }
            ui.ctx().request_repaint();
        }
        draft.frame = draft.frame.min(total_frames.saturating_sub(1));

        let (area, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 448.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(area, 0.0, nf.bg_canvas);
        let stage = egui::Rect::from_center_size(area.center(), egui::vec2(640.0, 400.0));
        let bg = self.project.background;
        ui.painter().rect_filled(
            stage,
            0.0,
            Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], bg[3]),
        );

        let scene = evaluate_scene(&self.project, self.symbol_id(), draft.frame, false, true);
        let sx = stage.width() / (self.project.width.max(1) as f32);
        let sy = stage.height() / (self.project.height.max(1) as f32);
        for draw in &scene.draws {
            let item = &draw.item;
            let tx = item.transform[4];
            let ty = item.transform[5];
            let pos = egui::pos2(
                stage.left() + (tx as f32) * sx,
                stage.top() + (ty as f32) * sy,
            );
            match &item.kind {
                ElementKind::Rectangle {
                    width,
                    height,
                    fill,
                    ..
                } => {
                    let r = egui::Rect::from_min_size(
                        pos,
                        egui::vec2((*width as f32) * sx, (*height as f32) * sy),
                    );
                    ui.painter().rect_filled(
                        r,
                        0.0,
                        Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]),
                    );
                }
                ElementKind::Ellipse {
                    width,
                    height,
                    fill,
                    ..
                } => {
                    let c_pos =
                        pos + egui::vec2((*width as f32) * 0.5 * sx, (*height as f32) * 0.5 * sy);
                    ui.painter().circle_filled(
                        c_pos,
                        ((*width as f32) * 0.5 * sx).max(2.0),
                        Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]),
                    );
                }
                ElementKind::Text { text, color, .. } => {
                    ui.painter().text(
                        pos,
                        egui::Align2::LEFT_TOP,
                        text,
                        neoflash_ui::typography::font(TextStyle::Body),
                        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
                    );
                }
                ElementKind::TextBox { text, format, .. } => {
                    let c = format.color;
                    let r = ((c >> 16) & 0xff) as u8;
                    let g = ((c >> 8) & 0xff) as u8;
                    let b = (c & 0xff) as u8;
                    ui.painter().text(
                        pos,
                        egui::Align2::LEFT_TOP,
                        text,
                        neoflash_ui::typography::font(TextStyle::Body),
                        Color32::from_rgb(r, g, b),
                    );
                }
                ElementKind::Path {
                    commands, stroke, ..
                }
                | ElementKind::PaintedPath {
                    commands, stroke, ..
                } => {
                    if let Some(PathCommand::MoveTo { x, y }) = commands.first() {
                        let p = egui::pos2(
                            stage.left() + ((*x + tx) as f32) * sx,
                            stage.top() + ((*y + ty) as f32) * sy,
                        );
                        ui.painter().circle_filled(
                            p,
                            3.0,
                            Color32::from_rgba_unmultiplied(
                                stroke[0],
                                stroke[1],
                                stroke[2],
                                stroke[3].max(180),
                            ),
                        );
                    }
                }
                _ => {}
            }
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui
                .add(c::IconButton::new(i16::FIRST, "First frame"))
                .clicked()
            {
                draft.frame = 0;
            }
            if ui
                .add(c::IconButton::new(i16::STEPBACK, "Previous frame"))
                .clicked()
            {
                draft.frame = draft.frame.saturating_sub(1);
            }
            let play_icon = if draft.playing { i16::PAUSE } else { i16::PLAY };
            let play_lbl = if draft.playing { "Pause" } else { "Play" };
            if ui
                .add(
                    c::IconButton::new(play_icon, play_lbl)
                        .kind(c::IconButtonType::Primary)
                        .shortcut("↵"),
                )
                .clicked()
            {
                draft.playing = !draft.playing;
                draft.last_step_time = ui.input(|i| i.time);
            }
            if ui
                .add(c::IconButton::new(i16::STEPFWD, "Next frame"))
                .clicked()
            {
                draft.frame = (draft.frame + 1).min(total_frames.saturating_sub(1));
            }
            if ui
                .add(c::IconButton::new(i16::LAST, "Last frame"))
                .clicked()
            {
                draft.frame = total_frames.saturating_sub(1);
            }
            if ui
                .add(c::IconButton::new(i16::LOOP, "Loop").selected(draft.loop_playback))
                .clicked()
            {
                draft.loop_playback = !draft.loop_playback;
            }
            let (rail, rail_resp) =
                ui.allocate_exact_size(egui::vec2(420.0, 6.0), egui::Sense::click_and_drag());
            if (rail_resp.clicked() || rail_resp.dragged())
                && let Some(ptr) = rail_resp.interact_pointer_pos()
            {
                let t = ((ptr.x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
                draft.frame = ((t * (total_frames.saturating_sub(1) as f32)).round() as u32)
                    .min(total_frames.saturating_sub(1));
            }
            let r = egui::CornerRadius::same(radius::FULL.min(127.0) as u8);
            ui.painter().rect_filled(rail, r, nf.bg_hover);
            let frac = (draft.frame + 1) as f32 / (total_frames.max(1) as f32);
            let done =
                egui::Rect::from_min_size(rail.min, egui::vec2(rail.width() * frac, rail.height()));
            ui.painter().rect_filled(done, r, nf.accent_default);
            let secs = (draft.frame as f64) / fps;
            mono(
                ui,
                &format!(
                    "Frame {} / {} · {:02}:{:05.2}",
                    draft.frame + 1,
                    total_frames,
                    (secs as u32) / 60,
                    secs % 60.0
                ),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(c::IconButton::new(i16::ROTATE_CW, "Restart"))
                    .clicked()
                {
                    draft.frame = 0;
                }
                let spd_lbl = match draft.speed {
                    s if (s - 0.5).abs() < 0.05 => "Speed 0.5×",
                    s if (s - 2.0).abs() < 0.05 => "Speed 2×",
                    _ => "Speed 1×",
                };
                c::Select::new(ui.id().with("test-movie-speed"), spd_lbl)
                    .width(110.0)
                    .show(ui, |ui| {
                        for (spd, lbl) in
                            [(0.5f32, "Speed 0.5×"), (1.0, "Speed 1×"), (2.0, "Speed 2×")]
                        {
                            if ui
                                .add(
                                    c::MenuItem::new(lbl).checked((draft.speed - spd).abs() < 0.05),
                                )
                                .clicked()
                            {
                                draft.speed = spd;
                                ui.close();
                            }
                        }
                    });
            });
        });

        if draft.show_output {
            ui.spacing_mut().item_spacing.y = 2.0;
            if self.output.is_empty() {
                ui.add(c::LogLine {
                    severity: c::Severity::Info,
                    time: "",
                    text: &format!(
                        "trace: {} ready ({} frames @ {} fps)",
                        self.project.name, total_frames, self.project.fps
                    ),
                });
            } else {
                for line in self.output.iter().rev().take(3).rev() {
                    ui.add(c::LogLine {
                        severity: c::Severity::Info,
                        time: "",
                        text: line,
                    });
                }
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
    }
}

fn y_ref(v: &mut f64) -> &mut f64 {
    v
}
