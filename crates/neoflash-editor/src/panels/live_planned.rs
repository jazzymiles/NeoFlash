//! Live interactive NF2 bodies for tool/properties sub-panels and the Test Movie preview window (`T-126`):
//! - `DecoTool` (`TO-03`, `W3.9`)
//! - `SprayBrush` (`TO-04`, `W3.9`)
//! - `BoneProperties` (`TO-05`, `W3.9`)
//! - `TestMovie` in-editor preview (`DL-12`)

use super::planned::{caption, form_label, mono};
#[cfg(test)]
use super::stroke_style::StrokeStyleKind;
use crate::app::Editor;
#[cfg(test)]
use crate::paint_controls::show_rgba_color_popup;
use crate::paint_controls::two_column_row;
use eframe::egui::{self, Color32, Ui};
use image::{ImageFormat, RgbaImage};
#[cfg(test)]
use neoflash_core::{
    Color, ElementKind, InstancePlayback, OvalOptions, PrimitiveOptions, RectangleOptions,
};
use neoflash_core::{IkArmature, Keyframe, ResourceUnit, new_id};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;
#[cfg(test)]
use neoflash_ui::typography::rich_block;
use std::collections::HashMap;
use std::io::Cursor;
#[cfg(test)]
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

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

impl DecoDraft {
    fn from_stage(opts: &crate::stage::deco_spray::StageDecoOptions) -> Self {
        Self {
            effect: opts.effect_index,
            branch_angle: opts.branch_angle_deg,
            scale_pct: opts.scale_pct,
            segment_px: opts.segment_px,
            step_frames: f64::from(opts.step_frames.max(1)),
            animate_pattern: opts.animate_pattern,
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
            symbol_name: String::new(),
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
            name: String::new(),
            x: 0.0,
            y: 0.0,
            length: 0.0,
            angle: 0.0,
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

impl BoneDraft {
    fn load_persisted_bone_fields(&mut self, armature: &IkArmature, bone_index: usize) {
        let Some(bone) = armature.bones.get(bone_index) else {
            return;
        };
        self.name = bone.id.clone();
        self.length = bone.length;
        self.speed_pct = bone.stiffness.clamp(0.0, 1.0) * 100.0;
        self.rot_constrain = bone.min_angle.is_some() || bone.max_angle.is_some();
        if let Some(min_angle_deg) = bone.min_angle {
            self.rot_min = min_angle_deg;
        }
        if let Some(max_angle_deg) = bone.max_angle {
            self.rot_max = max_angle_deg;
        }
        self.x = armature.target_pos[0];
        self.y = armature.target_pos[1];
    }

    fn load_persisted_joint_spec(&mut self, spec: Option<&neoflash_core::IkJointSpec>) {
        let Some(spec) = spec else {
            return;
        };
        self.rot_enable = spec.enable_rotation;
        self.rot_constrain = spec.constrain_rotation;
        self.rot_min = spec.min_angle_deg;
        self.rot_max = spec.max_angle_deg;
        self.trans_enable = spec.enable_x_translation || spec.enable_y_translation;
        self.trans_constrain = spec.constrain_x_translation || spec.constrain_y_translation;
        self.speed_pct = spec.speed.clamp(0.0, 1.0) * 100.0;
        self.spring_strength = spec.spring_strength.clamp(0.0, 100.0);
        self.spring_damping = spec.spring_damping.clamp(0.0, 100.0);
        if spec.spring_strength > 0.0 {
            self.enable_spring = true;
        }
    }

    fn persist_supported_bone_fields(&self, armature: &mut IkArmature, bone_index: usize) {
        let Some(bone) = armature.bones.get_mut(bone_index) else {
            return;
        };
        bone.length = self.length.max(4.0);
        if self.rot_constrain {
            // IkBone constraints use the same degree unit as Transform::rotation and both
            // core IK solvers. Keep the panel/model boundary unit-free.
            bone.min_angle = Some(self.rot_min.min(self.rot_max));
            bone.max_angle = Some(self.rot_max.max(self.rot_min));
        } else {
            bone.min_angle = None;
            bone.max_angle = None;
        }
        bone.stiffness = (self.speed_pct / 100.0).clamp(0.05, 1.0);
        armature.target_pos = [self.x, self.y];
    }

    #[cfg(test)]
    fn transient_joint_spec(&self, persisted_bone_id: &str) -> neoflash_core::IkJointSpec {
        self.transient_joint_spec_with_base(persisted_bone_id, None)
    }

    fn transient_joint_spec_with_base(
        &self,
        persisted_bone_id: &str,
        base: Option<&neoflash_core::IkJointSpec>,
    ) -> neoflash_core::IkJointSpec {
        let (pinned, min_x, max_x, min_y, max_y) =
            base.map_or((false, -50.0, 50.0, -50.0, 50.0), |b| {
                (
                    b.pinned,
                    b.min_x_offset.min(b.max_x_offset),
                    b.max_x_offset.max(b.min_x_offset),
                    b.min_y_offset.min(b.max_y_offset),
                    b.max_y_offset.max(b.min_y_offset),
                )
            });
        neoflash_core::IkJointSpec {
            // The name field is not a persisted rename workflow. Always target the actual
            // armature bone so editing that draft cannot silently disable this one-shot solve.
            bone_id: persisted_bone_id.to_owned(),
            pinned,
            enable_rotation: self.rot_enable,
            constrain_rotation: self.rot_constrain,
            min_angle_deg: self.rot_min.min(self.rot_max),
            max_angle_deg: self.rot_max.max(self.rot_min),
            enable_x_translation: self.trans_enable,
            constrain_x_translation: self.trans_constrain,
            min_x_offset: min_x,
            max_x_offset: max_x,
            enable_y_translation: self.trans_enable,
            constrain_y_translation: self.trans_constrain,
            min_y_offset: min_y,
            max_y_offset: max_y,
            speed: (self.speed_pct / 100.0).clamp(0.0, 1.0),
            spring_strength: if self.enable_spring {
                self.spring_strength.clamp(0.0, 100.0)
            } else {
                0.0
            },
            spring_damping: self.spring_damping.clamp(0.0, 100.0),
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

#[cfg(test)]
#[derive(Clone, Debug, Default)]
struct BitmapFillDraft {
    selected_resource: Option<neoflash_core::Id>,
    stroke_active: bool,
}

#[cfg(test)]
#[derive(Clone, Default)]
struct BitmapThumbCache {
    textures: HashMap<(String, u64), Option<egui::TextureHandle>>,
}

#[cfg(test)]
fn hash_bytes_with_len(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.len().hash(&mut hasher);
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
fn cached_bitmap_thumbnail(
    ctx: &egui::Context,
    resource_id: &str,
    data: &[u8],
) -> Option<egui::TextureHandle> {
    let cache_id = egui::Id::new("nf2-bitmap-fill-thumb-cache");
    let hash = hash_bytes_with_len(data);
    let key = (resource_id.to_owned(), hash);
    if let Some(cached) = ctx.data(|d| {
        d.get_temp::<BitmapThumbCache>(cache_id)
            .and_then(|c| c.textures.get(&key).cloned())
    }) {
        return cached;
    }
    let texture = image::load_from_memory(data).ok().map(|img| {
        let rgba = img.thumbnail(116, 116).to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        ctx.load_texture(
            format!("bitmap-thumb-{resource_id}-{hash:x}"),
            color_img,
            egui::TextureOptions::LINEAR,
        )
    });
    ctx.data_mut(|d| {
        let mut cache = d.get_temp::<BitmapThumbCache>(cache_id).unwrap_or_default();
        if cache.textures.len() >= 64 {
            cache.textures.clear();
        }
        cache.textures.insert(key, texture.clone());
        d.insert_temp(cache_id, cache);
    });
    texture
}

type FrameThumbKey = (String, u64, u32, u32, u32, u32, u32, [u8; 4], bool);

#[derive(Clone, Default)]
struct FrameThumbCache {
    textures: HashMap<FrameThumbKey, Option<egui::TextureHandle>>,
}

fn cached_frame_thumbnail(
    ctx: &egui::Context,
    editor: &Editor,
    symbol_id: &str,
    frame: u32,
    pw: u32,
    ph: u32,
    runtime_capture: bool,
) -> Option<egui::TextureHandle> {
    let pw = pw.clamp(1, 1024);
    let ph = ph.clamp(1, 1024);
    let cache_id = egui::Id::new("nf2-live-planned-frame-thumb-cache");
    let key: FrameThumbKey = (
        symbol_id.to_owned(),
        editor.history.revision(),
        frame,
        pw,
        ph,
        editor.project.width,
        editor.project.height,
        editor.project.background,
        runtime_capture,
    );
    if let Some(cached) = ctx.data(|d| {
        d.get_temp::<FrameThumbCache>(cache_id)
            .and_then(|c| c.textures.get(&key).cloned())
    }) {
        return cached;
    }
    let base_dir = editor.path.as_deref().and_then(std::path::Path::parent);
    let tex = neoflash_export::render_project_frame_rgba_with_base_dir(
        &editor.project,
        Some(symbol_id),
        frame,
        pw,
        ph,
        false,
        runtime_capture,
        base_dir,
    )
    .ok()
    .filter(|rgba| rgba.len() == (pw as usize) * (ph as usize) * 4)
    .map(|rgba| {
        let image = egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], &rgba);
        ctx.load_texture(
            format!(
                "planned-frame-thumb-{symbol_id}-{frame}-{pw}x{ph}-r{}",
                editor.history.revision()
            ),
            image,
            egui::TextureOptions::LINEAR,
        )
    });
    ctx.data_mut(|d| {
        let mut cache = d.get_temp::<FrameThumbCache>(cache_id).unwrap_or_default();
        if cache.textures.len() >= 96 {
            cache.textures.clear();
        }
        cache.textures.insert(key, tex.clone());
        d.insert_temp(cache_id, cache);
    });
    tex
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

#[cfg(test)]
fn unavailable_group(ui: &mut Ui, id_salt: &str, label: &str, add_contents: impl FnOnce(&mut Ui)) {
    let ir = ui.add_enabled_ui(false, add_contents);
    if !ir.response.enabled() {
        let _ = c::not_available(ir.response, label);
    } else {
        let resp = ui
            .scope(|ui| {
                ui.disable();
                ui.interact(
                    ir.response.rect,
                    ui.id().with(("unavail-probe", id_salt)),
                    egui::Sense::hover(),
                )
            })
            .inner;
        let _ = c::not_available(resp, label);
    }
}

const WINDOW_TITLE_BAR: f32 = 52.0;

fn safe_content_area(ctx: &egui::Context) -> egui::Rect {
    let area = ctx.content_rect();
    if area.is_finite() && area.width() >= 64.0 && area.height() >= 64.0 {
        area
    } else {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0))
    }
}

fn test_movie_title_bar(ui: &mut Ui, width: f32, editor: &mut Editor) -> (bool, egui::Vec2) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WINDOW_TITLE_BAR), egui::Sense::hover());
    let drag = ui.interact(
        rect,
        ui.id().with("test-movie-window-drag"),
        egui::Sense::drag(),
    );
    let mut close = false;
    let mut x = rect.left() + space::s4();
    for (i, color) in [nf.danger_default, nf.warning_default, nf.success_default]
        .into_iter()
        .enumerate()
    {
        let c_pos = egui::pos2(x + 6.0, rect.center().y);
        ui.painter().circle_filled(c_pos, 6.0, color);
        if i == 0 {
            let hit = egui::Rect::from_center_size(c_pos, egui::vec2(16.0, 16.0));
            let r = ui.interact(
                hit,
                ui.id().with("test-movie-window-close"),
                egui::Sense::click(),
            );
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close Test Movie")
            });
            close = c::tooltip(r, "Close Test Movie", None).clicked();
        }
        x += 20.0;
    }
    let title = Some(editor.project.name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| format!("{name} — Test Movie"))
        .unwrap_or_else(|| "Test Movie".to_owned());
    ui.painter().text(
        egui::pos2(x + space::s2(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        neoflash_ui::typography::font(TextStyle::BodyStrong),
        nf.text_primary,
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(space::s3(), 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| editor.live_test_movie_title_actions(ui),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    (close, drag.drag_delta())
}

impl Editor {
    #[cfg(test)]
    pub(crate) fn open_test_movie_preview(&mut self) {
        self.test_movie_preview_open = true;
    }

    /// Render the standalone Test Movie in-editor preview window (`DL-12`) when open for the
    /// active document session.
    pub(crate) fn test_movie_preview_window(&mut self, ctx: &egui::Context) {
        if !self.test_movie_preview_open {
            return;
        }
        let width = 1000.0;
        let area = safe_content_area(ctx);
        let pos_id = egui::Id::new(("nf2-test-movie-window-pos", &self.clipboard.session));
        let default_pos = egui::pos2(
            area.left() + neoflash_ui::metrics::tools_rail() + space::s6(),
            area.top() + neoflash_ui::metrics::top_bar() + space::s6(),
        );
        let pos = ctx
            .data(|d| d.get_temp::<egui::Pos2>(pos_id))
            .unwrap_or(default_pos);
        let mut close = false;
        let mut delta = egui::Vec2::ZERO;
        egui::Area::new(egui::Id::new((
            "nf2-test-movie-window",
            &self.clipboard.session,
        )))
        .order(egui::Order::Foreground)
        .current_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = egui::vec2(space::s2(), space::s2());
                let (c2, d) = test_movie_title_bar(ui, width, self);
                close = c2;
                delta = d;
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::s3() as i8))
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * space::s3());
                        self.live_test_movie(ui);
                    });
            });
        });
        if close {
            self.test_movie_preview_open = false;
        } else {
            ctx.data_mut(|d| d.insert_temp(pos_id, pos + delta));
        }
    }

    pub(crate) fn live_test_movie_title_actions(&mut self, ui: &mut Ui) {
        let id = egui::Id::new(("nf2-live-test-movie-draft", &self.clipboard.session));
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

    #[cfg(test)]
    fn selected_element_paint(&self) -> (Color, Color, f64) {
        if let Some(el) = self.selected_element() {
            match &el.kind {
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
                } => return (*fill, *stroke, *stroke_width),
                ElementKind::Text { color, .. } => return (*color, self.stroke, self.stroke_width),
                _ => {}
            }
        }
        (self.fill, self.stroke, self.stroke_width)
    }

    #[cfg(test)]
    fn selected_element_bitmap_fill_id(&self) -> Option<neoflash_core::Id> {
        let target = self.selection.single()?;
        let layer = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == target.layer)?;
        let key = layer.key_at(self.frame)?;
        key.bitmap_fill(&target.element)
            .map(|bf| bf.resource_id.clone())
    }

    #[cfg(test)]
    fn apply_selected_element_color(&mut self, stroke_well: bool, next: Color, action_name: &str) {
        if stroke_well {
            self.stroke = next;
        } else {
            self.fill = next;
        }
        if let Some(sel) = self.selection.single().cloned()
            && self.target_editable(&sel)
        {
            self.transact_named(action_name, |editor| {
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
                    match &mut el.kind {
                        ElementKind::Rectangle { fill, stroke, .. }
                        | ElementKind::Ellipse { fill, stroke, .. }
                        | ElementKind::Path { fill, stroke, .. }
                        | ElementKind::PaintedPath { fill, stroke, .. } => {
                            if stroke_well {
                                *stroke = next;
                            } else {
                                *fill = next;
                            }
                        }
                        ElementKind::Text { color, .. } if !stroke_well => {
                            *color = next;
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    #[cfg(test)]
    fn apply_selected_stroke_width(&mut self, width: f64, action_name: &str) {
        let clamped = width.clamp(0.0, 200.0);
        if clamped > 0.0 {
            self.stroke_width = clamped.clamp(0.1, 200.0);
        }
        if let Some(sel) = self.selection.single().cloned()
            && self.target_editable(&sel)
        {
            self.transact_named(action_name, |editor| {
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
                    match &mut el.kind {
                        ElementKind::Rectangle { stroke_width, .. }
                        | ElementKind::Ellipse { stroke_width, .. }
                        | ElementKind::Path { stroke_width, .. }
                        | ElementKind::PaintedPath { stroke_width, .. } => {
                            *stroke_width = clamped;
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    #[cfg(test)]
    fn live_shape_fill_stroke_controls(&mut self, ui: &mut Ui, id_prefix: &str, action_name: &str) {
        let (mut fill_rgba, mut stroke_rgba, mut sw) = self.selected_element_paint();
        let fill_col = (fill_rgba[3] > 0).then(|| {
            Color32::from_rgba_unmultiplied(fill_rgba[0], fill_rgba[1], fill_rgba[2], fill_rgba[3])
        });
        let stroke_col = (stroke_rgba[3] > 0).then(|| {
            Color32::from_rgba_unmultiplied(
                stroke_rgba[0],
                stroke_rgba[1],
                stroke_rgba[2],
                stroke_rgba[3],
            )
        });
        ui.horizontal(|ui| {
            let fill_resp = c::tooltip(
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Fill,
                    color: fill_col,
                    active: false,
                }),
                "Edit fill colour",
                None,
            );
            super::dialog_helpers::track_rgba_popup(ui.ctx(), fill_resp.id);
            if show_rgba_color_popup(
                ui,
                &fill_resp,
                fill_resp.rect,
                &format!("{id_prefix}-fill"),
                &mut fill_rgba,
            ) {
                self.apply_selected_element_color(false, fill_rgba, action_name);
            }
            let stroke_resp = c::tooltip(
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Stroke,
                    color: stroke_col,
                    active: false,
                }),
                "Edit stroke colour",
                None,
            );
            super::dialog_helpers::track_rgba_popup(ui.ctx(), stroke_resp.id);
            if show_rgba_color_popup(
                ui,
                &stroke_resp,
                stroke_resp.rect,
                &format!("{id_prefix}-stroke"),
                &mut stroke_rgba,
            ) {
                self.apply_selected_element_color(true, stroke_rgba, action_name);
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add(
                    c::Field::new(&mut sw, "Stroke size")
                        .range(0.0..=200.0)
                        .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                        .width(neoflash_ui::metrics::field_s()),
                )
                .changed()
            {
                self.apply_selected_stroke_width(sw, action_name);
            }
            let current_kind = self.init_stroke_style_draft().kind;
            let mut style = match current_kind {
                StrokeStyleKind::Dashed => c::LineStyle::Dashed,
                StrokeStyleKind::Dotted => c::LineStyle::Dotted,
                _ => c::LineStyle::Solid,
            };
            let rem = ui.available_width();
            unavailable_group(
                ui,
                &format!("{id_prefix}-line-style"),
                "Line style presets",
                |ui| {
                    c::LineStyleSelect::show(
                        ui,
                        format!("{id_prefix}-line-style"),
                        &mut style,
                        rem,
                    );
                },
            );
        });
    }

    #[cfg(test)]
    fn live_bitmap_fill(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let draft_id = egui::Id::new(("nf2-bitmap-fill-draft", &self.clipboard.session));
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<BitmapFillDraft>(draft_id))
            .unwrap_or_default();

        let (ctx_fill, fill_grad, ctx_stroke, _, _) = if self.authoring_modify.color_selection {
            self.contextual_paints()
        } else {
            let (f, s, w) = self.selected_element_paint();
            (f, None, s, None, w)
        };
        let fill_col = (ctx_fill[3] > 0).then(|| {
            Color32::from_rgba_unmultiplied(ctx_fill[0], ctx_fill[1], ctx_fill[2], ctx_fill[3])
        });
        let stroke_col = (ctx_stroke[3] > 0).then(|| {
            Color32::from_rgba_unmultiplied(
                ctx_stroke[0],
                ctx_stroke[1],
                ctx_stroke[2],
                ctx_stroke[3],
            )
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let stroke_well = c::tooltip(
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Stroke,
                    color: stroke_col,
                    active: draft.stroke_active,
                }),
                "Stroke colour",
                None,
            );
            crate::authoring_modify::update_color_popover_anchor(
                ui.ctx(),
                "live-bitmap-well-stroke",
                stroke_well.rect,
            );
            if stroke_well.clicked() {
                draft.stroke_active = true;
                self.authoring_modify.color_selection = true;
                crate::authoring_modify::request_color_popover(
                    ui.ctx(),
                    "live-bitmap-well-stroke",
                    stroke_well.rect,
                    true,
                    true,
                );
            }
            let fill_well = c::tooltip(
                ui.add(c::ColorWell {
                    target: c::ColorWellTarget::Fill,
                    color: fill_col,
                    active: !draft.stroke_active,
                }),
                "Fill colour",
                None,
            );
            crate::authoring_modify::update_color_popover_anchor(
                ui.ctx(),
                "live-bitmap-well-fill",
                fill_well.rect,
            );
            if fill_well.clicked() {
                draft.stroke_active = false;
                self.authoring_modify.color_selection = true;
                crate::authoring_modify::request_color_popover(
                    ui.ctx(),
                    "live-bitmap-well-fill",
                    fill_well.rect,
                    false,
                    true,
                );
            }
            self.show_active_color_popover(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        c::IconButton::new(i20::COLOR_SWAP, "Swap colours")
                            .side(neoflash_ui::metrics::tool_button()),
                    )
                    .clicked()
                {
                    self.swap_active_paints();
                }
                if ui
                    .add(
                        c::IconButton::new(i20::COLOR_NONE, "No colour")
                            .side(neoflash_ui::metrics::tool_button()),
                    )
                    .clicked()
                {
                    self.set_selection_fill_type(
                        draft.stroke_active,
                        crate::authoring_modify::PaintFillType::None,
                    );
                }
                if ui
                    .add(
                        c::IconButton::new(i20::COLOR_DEFAULT, "Black and white")
                            .side(neoflash_ui::metrics::tool_button()),
                    )
                    .clicked()
                {
                    self.reset_default_paints();
                }
            });
        });

        let image_resources: Vec<(neoflash_core::Id, String, Vec<u8>)> = self
            .project
            .resources
            .iter()
            .filter(|r| r.is_image())
            .map(|r| {
                let name = std::path::Path::new(&r.path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&r.path)
                    .to_owned();
                let bytes = r.decoded_bytes().unwrap_or_default();
                (r.id.clone(), name, bytes)
            })
            .collect();
        let active_bitmap_id = self.selected_element_bitmap_fill_id();
        if let Some(active_bf) = active_bitmap_id.clone()
            && image_resources.iter().any(|(id, _, _)| id == &active_bf)
        {
            draft.selected_resource = Some(active_bf);
        } else if !draft
            .selected_resource
            .as_ref()
            .is_some_and(|sel| image_resources.iter().any(|(id, _, _)| id == sel))
        {
            draft.selected_resource = image_resources.first().map(|(id, _, _)| id.clone());
        }

        let fill_type_label = if active_bitmap_id.is_some() {
            "Bitmap fill"
        } else if let Some(grad) = fill_grad {
            match grad.kind {
                neoflash_core::GradientKind::Linear => "Linear gradient",
                neoflash_core::GradientKind::Radial => "Radial gradient",
            }
        } else if self.selected_element().is_some() && ctx_fill[3] > 0 {
            "Solid colour"
        } else if self.selected_element().is_some() {
            "None"
        } else {
            "Bitmap fill"
        };
        c::Select::new(ui.id().with("bitmap-fill-kind"), fill_type_label)
            .width(ui.available_width())
            .show(ui, |ui| {
                use crate::authoring_modify::PaintFillType;
                for (mode, label) in [
                    (PaintFillType::Solid, "Solid colour"),
                    (PaintFillType::LinearGradient, "Linear gradient"),
                    (PaintFillType::RadialGradient, "Radial gradient"),
                ] {
                    if ui
                        .add(c::MenuItem::new(label).checked(fill_type_label == label))
                        .clicked()
                    {
                        self.set_selection_fill_type(false, mode);
                        ui.close();
                    }
                }
                if ui
                    .add_enabled(
                        draft.selected_resource.is_some(),
                        c::MenuItem::new("Bitmap fill").checked(fill_type_label == "Bitmap fill"),
                    )
                    .clicked()
                {
                    if let Some(res_id) = draft.selected_resource.clone()
                        && let Err(err) = self.set_selected_bitmap_fill(
                            res_id,
                            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            true,
                            true,
                        )
                    {
                        self.message(err);
                    }
                    ui.close();
                }
            });

        if image_resources.is_empty() {
            caption(
                ui,
                "No bitmap resources in library. Click Import… to add an image.",
            );
        } else {
            let mut clicked_resource: Option<neoflash_core::Id> = None;
            for row in image_resources.chunks(4) {
                ui.horizontal(|ui| {
                    for (res_id, res_name, data) in row {
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, radius::md(), nf.bg_field);
                        if let Some(tex) = cached_bitmap_thumbnail(ui.ctx(), res_id, data) {
                            ui.painter().image(
                                tex.id(),
                                rect.shrink(1.0),
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        let selected = draft.selected_resource.as_deref() == Some(res_id.as_str());
                        let (w, color) = if selected {
                            (2.0_f32, nf.accent_default)
                        } else {
                            (1.0, nf.border_subtle)
                        };
                        ui.painter().rect_stroke(
                            rect,
                            egui::CornerRadius::same(radius::md() as u8),
                            egui::Stroke::new(w, color),
                            egui::StrokeKind::Inside,
                        );
                        let resp = c::tooltip(resp, res_name, None);
                        if resp.clicked() {
                            clicked_resource = Some(res_id.clone());
                        }
                    }
                });
            }
            if let Some(res_id) = clicked_resource {
                draft.selected_resource = Some(res_id.clone());
                if let Err(err) = self.set_selected_bitmap_fill(
                    res_id,
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    true,
                    true,
                ) {
                    self.message(err);
                }
            }
        }

        ui.horizontal(|ui| {
            let import_resp = c::tooltip(
                ui.add(c::Button::new("Import…").icon(i16::PLUS)),
                "Import image file into library",
                None,
            );
            if import_resp.clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
                    .pick_files()
            {
                let before_len = self.project.resources.len();
                match self.import_resource_paths(&paths) {
                    Ok(()) => {
                        if let Some(newest) = self
                            .project
                            .resources
                            .iter()
                            .skip(before_len)
                            .rfind(|r| r.is_image())
                            .or_else(|| self.project.resources.iter().rfind(|r| r.is_image()))
                        {
                            let new_id = newest.id.clone();
                            draft.selected_resource = Some(new_id.clone());
                            let _ = self.set_selected_bitmap_fill(
                                new_id,
                                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                true,
                                true,
                            );
                        }
                    }
                    Err(err) => self.message(err),
                }
            }
            if ui
                .add(
                    c::IconButton::new(i16::LOCK_FILL, "Lock fill")
                        .selected(self.authoring_tools.brush_lock_fill),
                )
                .clicked()
            {
                self.authoring_tools.brush_lock_fill = !self.authoring_tools.brush_lock_fill;
            }
            caption(ui, "Lock fill");
        });

        let selected_name = draft.selected_resource.as_deref().and_then(|id| {
            image_resources
                .iter()
                .find(|(rid, _, _)| rid == id)
                .map(|(_, name, _)| name.as_str())
        });
        let has_editable = self
            .selection
            .single()
            .is_some_and(|t| self.target_editable(t));
        let summary = match selected_name {
            Some(name) if has_editable => {
                format!("{name} · {} bitmap resource(s)", image_resources.len())
            }
            Some(name) => format!(
                "{name} · {} bitmap resource(s) · select a shape to apply",
                image_resources.len()
            ),
            None => "0 bitmap resources · click Import… to add an image".to_owned(),
        };
        caption(ui, &summary);

        ui.add_space(space::s1());
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range(),
            r.top(),
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        ui.add_space(space::s2());
        ui.label(rich("Stroke", TextStyle::Overline).color(nf.text_tertiary));
        let stroke_draft = self.init_stroke_style_draft();
        let mut sw = self.selected_element_paint().2;
        let sw_before = sw;
        let current_kind = stroke_draft.kind;
        let (stroke_response, ()) = two_column_row(
            ui,
            |ui, width| {
                ui.add(
                    c::Field::new(&mut sw, "Stroke width")
                        .prefix("Width")
                        .unit("px")
                        .range(0.0..=200.0)
                        .decimals(1)
                        .width(width),
                )
            },
            |ui, width| {
                c::Select::new(
                    ui.id().with("bitmap-fill-stroke-style"),
                    current_kind.label(),
                )
                .width(width)
                .show(ui, |ui| {
                    for preset in StrokeStyleKind::ALL {
                        if ui
                            .add(c::MenuItem::new(preset.label()).checked(current_kind == preset))
                            .clicked()
                        {
                            let _ = self.apply_stroke_style_to_selection(
                                preset,
                                sw_before.max(0.1),
                                stroke_draft.dash,
                                stroke_draft.gap,
                            );
                            ui.close();
                        }
                    }
                    if ui.add(c::MenuItem::new("Custom…")).clicked() {
                        let ctx = ui.ctx().clone();
                        self.open_stroke_style(&ctx);
                        ui.close();
                    }
                });
            },
        );
        if stroke_response.changed() {
            self.apply_selected_stroke_width(sw, "Edit stroke width");
        }

        ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));
    }

    #[cfg(test)]
    fn live_properties_oval(&mut self, ui: &mut Ui) {
        let layer_name = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .map(|l| l.name.clone());
        let has_sel = self.selected_element().is_some();
        let can_edit_sel = self
            .selection
            .single()
            .is_some_and(|t| self.target_editable(t));
        let subtitle = match (has_sel, layer_name) {
            (true, Some(name)) => format!("Drawing object · layer \"{name}\""),
            (true, None) => "Drawing object".to_owned(),
            (false, _) => "No element selected · tool defaults".to_owned(),
        };
        ui.add(c::TypeHeader {
            icon: i20::TOOL_OVAL,
            title: "Oval primitive",
            subtitle: &subtitle,
        });

        let (mut x, mut y, mut w, mut h, mut start_deg, mut end_deg, mut inner_pct, mut closed) =
            if let Some(el) = self.selected_element() {
                let (ew, eh) = match &el.kind {
                    ElementKind::Ellipse { width, height, .. } => (*width, *height),
                    ElementKind::Rectangle { width, height, .. } => (*width, *height),
                    _ => (0.0, 0.0),
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
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    o.start_angle,
                    o.end_angle,
                    o.inner_radius * 100.0,
                    o.close_path,
                )
            };

        let link_id = egui::Id::new(("nf2-live-oval-lock-proportions", &self.clipboard.session));
        let mut locked = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(link_id))
            .unwrap_or(false);
        let (old_w, old_h) = (w, h);
        let mut geom_changed = false;
        let mut prim_changed = false;
        let size_range = if can_edit_sel {
            1.0..=4000.0
        } else {
            0.0..=4000.0
        };
        section(ui, "Position and size", true, |ui| {
            let geom_resp = ui
                .add_enabled_ui(can_edit_sel, |ui| {
                    let (x_response, (y_response, link)) = two_column_row(
                        ui,
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut x, "X")
                                    .prefix("X")
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                let field_width =
                                    (width - crate::theme::control_h() - space::s2()).max(0.0);
                                let response = ui.add(
                                    c::Field::new(y_ref(&mut y), "Y")
                                        .prefix("Y")
                                        .decimals(1)
                                        .width(field_width),
                                );
                                let link = ui.add(
                                    c::IconButton::new(i16::LINK, "Lock proportions")
                                        .selected(locked),
                                );
                                (response, link)
                            })
                            .inner
                        },
                    );
                    if link.clicked() {
                        locked = !locked;
                    }
                    geom_changed |= x_response.changed() || y_response.changed();
                    let (w_response, h_response) = two_column_row(
                        ui,
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut w, "W")
                                    .prefix("W")
                                    .range(size_range.clone())
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut h, "H")
                                    .prefix("H")
                                    .range(size_range.clone())
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                    );
                    let w_changed = w_response.changed() && (w - old_w).abs() > 1e-6;
                    let h_changed = h_response.changed() && (h - old_h).abs() > 1e-6;
                    if locked && old_w > 0.0 && old_h > 0.0 {
                        if w_changed {
                            h = (w * old_h / old_w).clamp(1.0, 4000.0);
                        } else if h_changed {
                            w = (h * old_w / old_h).clamp(1.0, 4000.0);
                        }
                    }
                    geom_changed |= w_response.changed() || h_response.changed();
                })
                .response;
            if !can_edit_sel {
                c::tooltip(geom_resp, "Select an editable oval or shape on stage", None);
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(link_id, locked));

        section(ui, "Fill and stroke", true, |ui| {
            self.live_shape_fill_stroke_controls(ui, "live-oval", "Edit oval paint");
            ui.horizontal(|ui| {
                if ui
                    .add(c::Button::new("Custom…").icon(i16::SETTINGS))
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.open_stroke_style(&ctx);
                }
                let mut hinting = false;
                let hint_resp =
                    ui.add_enabled(false, c::Switch::new(&mut hinting, "Stroke hinting"));
                let _ = c::not_available(hint_resp, "Stroke hinting");
            });
            let mut miter = 3.0;
            two_column_row(
                ui,
                |ui, width| {
                    let scale_resp = ui
                        .scope(|ui| {
                            ui.disable();
                            c::Select::new(ui.id().with("oval-scale"), "Normal")
                                .prefix("Scale")
                                .width(width)
                                .show(ui, |_| {})
                                .0
                        })
                        .inner;
                    let _ = c::not_available(scale_resp, "Stroke scaling");
                },
                |ui, width| {
                    let miter_resp = ui.add_enabled(
                        false,
                        c::Field::new(&mut miter, "Miter")
                            .prefix("Miter")
                            .decimals(0)
                            .width(width),
                    );
                    let _ = c::not_available(miter_resp, "Miter limit");
                },
            );
            for (label, reason, items) in [
                (
                    "Cap",
                    "Stroke caps",
                    [
                        (i16::CAP_NONE, "No cap"),
                        (i16::CAP_ROUND, "Round cap"),
                        (i16::CAP_SQUARE, "Square cap"),
                    ],
                ),
                (
                    "Join",
                    "Stroke joins",
                    [
                        (i16::JOIN_MITER, "Miter join"),
                        (i16::JOIN_ROUND, "Round join"),
                        (i16::JOIN_BEVEL, "Bevel join"),
                    ],
                ),
            ] {
                ui.horizontal(|ui| {
                    form_label(ui, label, neoflash_ui::metrics::label_width());
                    unavailable_group(ui, label, reason, |ui| {
                        c::IconGroup {
                            items: &items,
                            selected: Some(1),
                        }
                        .show(ui);
                    });
                });
            }
        });

        section(ui, "Oval options", true, |ui| {
            let (start_response, end_response) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(&mut start_deg, "Start")
                            .prefix("Start")
                            .unit("°")
                            .range(0.0..=360.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(&mut end_deg, "End")
                            .prefix("End")
                            .unit("°")
                            .range(0.0..=360.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
            );
            prim_changed |= start_response.changed() || end_response.changed();
            // FORM_GRID_EXCEPTION: Inner is deliberately paired with an action checkbox.
            let inner_width = (ui.available_width() - space::s2()) / 2.0;
            ui.horizontal(|ui| {
                prim_changed |= ui
                    .add(
                        c::Field::new(&mut inner_pct, "Inner")
                            .prefix("Inner")
                            .unit("%")
                            .range(0.0..=99.0)
                            .decimals(0)
                            .stepper()
                            .width(inner_width),
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

    #[cfg(test)]
    fn live_properties_rect(&mut self, ui: &mut Ui) {
        let can_edit_sel = self
            .selection
            .single()
            .is_some_and(|t| self.target_editable(t));
        let subtitle = if self.selected_element().is_some() {
            "Drawing object · editable corners"
        } else {
            "No element selected · tool defaults"
        };
        ui.add(c::TypeHeader {
            icon: i20::TOOL_RECT_PRIMITIVE,
            title: "Rectangle primitive",
            subtitle,
        });

        let (mut x, mut y, mut w, mut h, mut radii) = if let Some(el) = self.selected_element() {
            let (rw, rh) = match &el.kind {
                ElementKind::Rectangle { width, height, .. } => (*width, *height),
                ElementKind::Ellipse { width, height, .. } => (*width, *height),
                _ => (0.0, 0.0),
            };
            let r = match &el.transform.primitive {
                Some(PrimitiveOptions::Rectangle(opts)) => opts.corner_radii,
                _ => self.authoring_tools.rect_options.corner_radii,
            };
            (el.transform.x, el.transform.y, rw, rh, r)
        } else {
            (
                0.0,
                0.0,
                0.0,
                0.0,
                self.authoring_tools.rect_options.corner_radii,
            )
        };

        let mut changed = false;
        let size_range = if can_edit_sel {
            1.0..=4000.0
        } else {
            0.0..=4000.0
        };
        section(ui, "Position and size", true, |ui| {
            let geom_resp = ui
                .add_enabled_ui(can_edit_sel, |ui| {
                    let (x_response, y_response) = two_column_row(
                        ui,
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut x, "X")
                                    .prefix("X")
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut y, "Y")
                                    .prefix("Y")
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                    );
                    changed |= x_response.changed() || y_response.changed();
                    let (w_response, h_response) = two_column_row(
                        ui,
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut w, "W")
                                    .prefix("W")
                                    .range(size_range.clone())
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.add(
                                c::Field::new(&mut h, "H")
                                    .prefix("H")
                                    .range(size_range.clone())
                                    .decimals(1)
                                    .width(width),
                            )
                        },
                    );
                    changed |= w_response.changed() || h_response.changed();
                })
                .response;
            if !can_edit_sel {
                c::tooltip(
                    geom_resp,
                    "Select an editable rectangle or shape on stage",
                    None,
                );
            }
        });

        section(ui, "Fill and stroke", true, |ui| {
            self.live_shape_fill_stroke_controls(ui, "live-rect", "Edit rectangle paint");
        });

        let mut corners_locked = self.authoring_tools.rect_options.corners_locked;
        section(ui, "Rectangle options", true, |ui| {
            let (mut top_left, mut top_right) = (radii[0], radii[1]);
            let (top_left_response, (top_right_response, link_response)) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(&mut top_left, "Top-left radius")
                            .prefix("↖")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        let field_width =
                            (width - crate::theme::control_h() - space::s2()).max(0.0);
                        let field = ui.add(
                            c::Field::new(&mut top_right, "Top-right radius")
                                .prefix("↗")
                                .range(0.0..=500.0)
                                .decimals(0)
                                .stepper()
                                .width(field_width),
                        );
                        let link = ui.add(
                            c::IconButton::new(i16::LINK, "Lock proportions")
                                .selected(corners_locked),
                        );
                        (field, link)
                    })
                    .inner
                },
            );
            if top_left_response.changed() {
                radii[0] = top_left;
                if corners_locked {
                    radii = [radii[0]; 4];
                }
                changed = true;
            }
            if top_right_response.changed() {
                radii[1] = top_right;
                if corners_locked {
                    radii = [radii[1]; 4];
                }
                changed = true;
            }
            if link_response.clicked() {
                corners_locked = !corners_locked;
                changed = true;
            }

            let (mut bottom_left, mut bottom_right) = (radii[3], radii[2]);
            let (bottom_left_response, bottom_right_response) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(&mut bottom_left, "Bottom-left radius")
                            .prefix("↙")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(&mut bottom_right, "Bottom-right radius")
                            .prefix("↘")
                            .range(0.0..=500.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
            );
            if bottom_left_response.changed() {
                radii[3] = bottom_left;
                if corners_locked {
                    radii = [radii[3]; 4];
                }
                changed = true;
            }
            if bottom_right_response.changed() {
                radii[2] = bottom_right;
                if corners_locked {
                    radii = [radii[2]; 4];
                }
                changed = true;
            }
            let mut slider_val = radii[0].clamp(0.0, 100.0);
            if ui
                .add(
                    c::Slider::new(&mut slider_val, 0.0..=100.0, "Corner radius")
                        .width(ui.available_width()),
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
    #[cfg_attr(not(test), allow(dead_code))]
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
        let mut generated = neoflash_core::generate_deco_elements(
            deco_effect,
            [100.0, 160.0],
            scale_pct,
            (segment_px * 6.0).clamp(16.0, 160.0),
            [82, 204, 122, 255],
            [255, 140, 102, 255],
        );
        let branch_angle_deg = if branch_angle_deg.is_finite() {
            branch_angle_deg.clamp(-180.0, 180.0)
        } else {
            0.0
        };
        if branch_angle_deg.abs() > f64::EPSILON {
            let [origin_x, origin_y] = [100.0, 160.0];
            let (sin, cos) = branch_angle_deg.to_radians().sin_cos();
            for element in &mut generated {
                let dx = element.transform.x - origin_x;
                let dy = element.transform.y - origin_y;
                element.transform.x = origin_x + dx * cos - dy * sin;
                element.transform.y = origin_y + dx * sin + dy * cos;
                element.transform.rotation += branch_angle_deg;
            }
        }
        let created = generated.len();
        if created == 0 {
            return 0;
        }
        let layer_idx = self.layer;
        let frame = self.frame;

        self.finish_edits();
        let step_frames = step_frames.clamp(1, 30);
        self.transact_named("Deco pattern", |editor| {
            let timeline = &mut editor.symbol_mut().timeline;
            let Some(layer) = timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let mut final_frame = frame;
            if animate_pattern {
                for (index, element) in generated.into_iter().enumerate() {
                    let target_frame = frame
                        .saturating_add(
                            u32::try_from(index)
                                .unwrap_or(u32::MAX)
                                .saturating_mul(step_frames),
                        )
                        .min(999_999);
                    final_frame = final_frame.max(target_frame);
                    if !layer.keyframes.iter().any(|key| key.frame == target_frame) {
                        let previous = layer
                            .key_at(target_frame)
                            .map(|key| key.elements.clone())
                            .unwrap_or_default();
                        layer.keyframes.push(Keyframe {
                            frame: target_frame,
                            elements: previous,
                            ..Keyframe::default()
                        });
                        layer.keyframes.sort_by_key(|key| key.frame);
                    }
                    if let Some(key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|key| key.frame == target_frame)
                    {
                        key.elements.push(element);
                    }
                }
            } else {
                if !layer.keyframes.iter().any(|key| key.frame == frame) {
                    let previous = layer
                        .key_at(frame)
                        .map(|key| key.elements.clone())
                        .unwrap_or_default();
                    layer.keyframes.push(Keyframe {
                        frame,
                        elements: previous,
                        ..Keyframe::default()
                    });
                    layer.keyframes.sort_by_key(|key| key.frame);
                }
                if let Some(key) = layer.keyframes.iter_mut().find(|key| key.frame == frame) {
                    key.elements.extend(generated);
                }
            }
            layer.normalize_motion_keys();
            timeline.frame_count = timeline.frame_count.max(final_frame.saturating_add(1));
        });
        created
    }

    pub(crate) fn live_deco_tool(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let id = egui::Id::new(("nf2-deco-tool-draft", &self.clipboard.session));
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<DecoDraft>(id))
            .unwrap_or_else(|| DecoDraft::from_stage(&self.stage_deco_options()));

        ui.add(c::TypeHeader {
            icon: i20::TOOL_DECO,
            title: "Deco tool",
            subtitle: "Procedural patterns from symbols",
        });

        let effects: Vec<&str> = neoflash_core::DecoEffectKind::ALL
            .iter()
            .map(|e| e.label())
            .collect();
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
                (
                    "Leaf",
                    Color32::from_rgba_unmultiplied(
                        self.fill[0],
                        self.fill[1],
                        self.fill[2],
                        self.fill[3],
                    ),
                ),
                (
                    "Flower",
                    Color32::from_rgba_unmultiplied(
                        self.stroke[0],
                        self.stroke[1],
                        self.stroke[2],
                        self.stroke[3],
                    ),
                ),
            ] {
                ui.horizontal(|ui| {
                    form_label(ui, label, neoflash_ui::metrics::label_width());
                    let (chip, _) = ui.allocate_exact_size(
                        egui::Vec2::splat(neoflash_ui::metrics::swatch()),
                        egui::Sense::hover(),
                    );
                    neoflash_ui::components::display::paint_swatch(ui, chip, color, radius::sm());
                    ui.label(rich("Default shape", TextStyle::Body).color(nf.text_secondary));
                    let edit_resp = ui.add_enabled(false, c::Button::ghost("Edit…"));
                    let _ = c::not_available(edit_resp, "Custom Deco symbols");
                });
            }
        });
        section(ui, "Advanced options", true, |ui| {
            let (branch_angle, scale_pct) = (&mut draft.branch_angle, &mut draft.scale_pct);
            two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(branch_angle, "Branch")
                            .prefix("Branch")
                            .unit("°")
                            .range(-180.0..=180.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(scale_pct, "Scale")
                            .prefix("Scale")
                            .unit("%")
                            .range(10.0..=400.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
            );
            let (segment_px, step_frames) = (&mut draft.segment_px, &mut draft.step_frames);
            two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(segment_px, "Segment")
                            .prefix("Segment")
                            .unit("px")
                            .range(1.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(step_frames, "Step")
                            .prefix("Step")
                            .unit("f")
                            .range(1.0..=30.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
            );
            ui.add(c::Checkbox::new(
                &mut draft.animate_pattern,
                "Animate pattern (one keyframe per step)",
            ));
        });

        self.set_stage_deco_options(crate::stage::deco_spray::StageDecoOptions {
            effect_index: draft.effect,
            branch_angle_deg: draft.branch_angle,
            scale_pct: draft.scale_pct,
            segment_px: draft.segment_px,
            step_frames: draft.step_frames.round().clamp(1.0, 30.0) as u32,
            animate_pattern: draft.animate_pattern,
            primary_color: self.fill,
            secondary_color: self.stroke,
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
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
            None, use_dots, scale_w, scale_h, false, false, false, 24.0, 24.0, 0.0,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_spray_brush_with_options(
        &mut self,
        symbol_name: Option<&str>,
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
        let sym_id = if use_dots {
            None
        } else {
            let requested = symbol_name.map(str::trim).filter(|name| !name.is_empty());
            let found = self.project.symbols.iter().find(|symbol| {
                symbol.id != self.project.root
                    && !self.project.is_scene(&symbol.id)
                    && requested.is_none_or(|name| {
                        symbol.name.eq_ignore_ascii_case(name)
                            || symbol.id.eq_ignore_ascii_case(name)
                    })
            });
            let Some(found) = found else {
                self.message(match requested {
                    Some(name) => format!("Spray brush: symbol '{name}' was not found."),
                    None => "Spray brush: the document has no sprayable library symbol.".into(),
                });
                return 0;
            };
            Some(found.id.clone())
        };
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

    pub(crate) fn live_spray_brush(&mut self, ui: &mut Ui) {
        let id = egui::Id::new(("nf2-spray-brush-draft", &self.clipboard.session));
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<SprayDraft>(id))
            .unwrap_or_default();
        let mut edit_symbol = false;

        ui.add(c::TypeHeader {
            icon: i20::TOOL_SPRAY,
            title: "Spray brush",
            subtitle: "Sprays copies of a symbol or dots",
        });
        section(ui, "Symbol", true, |ui| {
            let trimmed = draft.symbol_name.trim().to_owned();
            let can_edit_symbol = !trimmed.is_empty()
                && self.project.symbols.iter().any(|symbol| {
                    symbol.id != self.project.root
                        && !self.project.is_scene(&symbol.id)
                        && (symbol.name.eq_ignore_ascii_case(&trimmed)
                            || symbol.id.eq_ignore_ascii_case(&trimmed))
                });
            ui.horizontal(|ui| {
                // The name fills the row; `Edit…` keeps a `field_num()` slot at the end.
                let name_width =
                    (ui.available_width() - neoflash_ui::metrics::field_num() - space::s2())
                        .max(neoflash_ui::metrics::field_s());
                ui.add(
                    c::TextField::new(&mut draft.symbol_name, "Symbol")
                        .hint("Library symbol name")
                        .width(name_width),
                );
                let edit_resp = ui.add_enabled(can_edit_symbol, c::Button::new("Edit…"));
                if can_edit_symbol {
                    if edit_resp.clicked() {
                        edit_symbol = true;
                    }
                } else {
                    c::tooltip(
                        edit_resp,
                        "Enter a matching Library symbol name to edit",
                        None,
                    );
                }
            });
            ui.add(c::Checkbox::new(
                &mut draft.use_dots,
                "Use default shape (dots)",
            ));
            let (scale_w, scale_h) = (&mut draft.scale_w, &mut draft.scale_h);
            two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(scale_w, "W")
                            .prefix("W")
                            .unit("%")
                            .range(10.0..=400.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(scale_h, "H")
                            .prefix("H")
                            .unit("%")
                            .range(10.0..=400.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
            );
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
            let (brush_w, brush_h) = (&mut draft.brush_w, &mut draft.brush_h);
            two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(brush_w, "Width")
                            .prefix("Width")
                            .unit("px")
                            .range(1.0..=200.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(brush_h, "Height")
                            .prefix("Height")
                            .unit("px")
                            .range(1.0..=200.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    );
                },
            );
            let angle_width = ui.available_width();
            ui.add(
                c::Field::new(&mut draft.brush_angle, "Angle")
                    .prefix("Angle")
                    .unit("°")
                    .range(-180.0..=180.0)
                    .decimals(0)
                    .stepper()
                    .width(angle_width),
            );
        });

        let symbol_name = draft.symbol_name.clone();
        let resolved_spray_symbol = if draft.use_dots {
            None
        } else {
            let requested = symbol_name.trim();
            self.project
                .symbols
                .iter()
                .find(|symbol| {
                    symbol.id != self.project.root
                        && !self.project.is_scene(&symbol.id)
                        && !requested.is_empty()
                        && (symbol.name.eq_ignore_ascii_case(requested)
                            || symbol.id.eq_ignore_ascii_case(requested))
                })
                .map(|s| s.id.clone())
                .or_else(|| {
                    self.library_selected.as_ref().and_then(|sel| {
                        self.project
                            .symbols
                            .iter()
                            .find(|s| {
                                &s.id == sel
                                    && s.id != self.project.root
                                    && !self.project.is_scene(&s.id)
                            })
                            .map(|s| s.id.clone())
                    })
                })
        };
        if !draft.use_dots && resolved_spray_symbol.is_none() {
            caption(
                ui,
                "Select a Library symbol or enable 'Use default shape (dots)' before painting on Stage.",
            );
        }
        let prev_seed = self.stage_spray_options().seed;
        self.set_stage_spray_options(crate::stage::deco_spray::StageSprayOptions {
            symbol_id: resolved_spray_symbol,
            use_dots: draft.use_dots,
            fill: self.fill,
            scale_w_pct: draft.scale_w,
            scale_h_pct: draft.scale_h,
            random_scaling: draft.random_scaling,
            rotate_symbol: draft.rotate_symbol,
            random_rotation: draft.random_rotation,
            brush_width: draft.brush_w,
            brush_height: draft.brush_h,
            brush_angle_deg: draft.brush_angle,
            seed: prev_seed,
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
        if edit_symbol {
            let requested = symbol_name.trim();
            let symbol_id = self
                .project
                .symbols
                .iter()
                .find(|symbol| {
                    symbol.id != self.project.root
                        && !self.project.is_scene(&symbol.id)
                        && (symbol.name.eq_ignore_ascii_case(requested)
                            || symbol.id.eq_ignore_ascii_case(requested))
                })
                .map(|symbol| symbol.id.clone());
            match symbol_id {
                Some(symbol_id) => {
                    self.enter_symbol_isolated(symbol_id);
                }
                None => self.message(format!(
                    "Spray brush: symbol '{}' was not found; no artwork was changed.",
                    requested
                )),
            }
        }
    }

    pub(crate) fn live_bone_properties(&mut self, ui: &mut Ui) {
        let id = egui::Id::new(("nf2-bone-properties-draft", &self.clipboard.session));
        let mut draft = ui
            .ctx()
            .data(|d| d.get_temp::<BoneDraft>(id))
            .unwrap_or_default();

        let existing_ik = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.key_at(self.frame))
            .and_then(|k| {
                let arm = k.ik_armatures.first()?.clone();
                let specs = self.effective_keyframe_ik_joint_specs(k, &arm);
                Some((arm, specs))
            });

        let Some((armature, joint_specs)) = existing_ik else {
            ui.add(c::TypeHeader {
                icon: i16::ARMATURE,
                title: "IK bone",
                subtitle: "No armature on current frame",
            });
            caption(
                ui,
                "Draw an IK armature with the Bone tool on the active frame to edit bone constraints.",
            );
            return;
        };

        let bone_total = armature.bones.len().max(1);
        if let Some(sess_bone_id) = self.stage_active_ik_bone_id()
            && let Some(active_idx) = armature.bones.iter().position(|b| b.id == sess_bone_id)
        {
            draft.bone_index = active_idx;
        } else {
            draft.bone_index = draft.bone_index.min(bone_total.saturating_sub(1));
            self.set_stage_active_ik_bone_id(
                armature.bones.get(draft.bone_index).map(|b| b.id.clone()),
            );
        }
        draft.load_persisted_bone_fields(&armature, draft.bone_index);
        let persisted_spec = armature
            .bones
            .get(draft.bone_index)
            .and_then(|b| joint_specs.iter().find(|s| s.bone_id == b.id));
        draft.load_persisted_joint_spec(persisted_spec);

        let mut changed = false;
        let mut navigated_bone = false;
        ui.add(c::TypeHeader {
            icon: i16::ARMATURE,
            title: "IK bone",
            subtitle: &format!(
                "Armature \"{}\" · bone {} of {bone_total}",
                armature.name,
                draft.bone_index + 1
            ),
        });
        ui.horizontal(|ui| {
            let mut bone_id_label = armature
                .bones
                .get(draft.bone_index)
                .map(|b| b.id.clone())
                .unwrap_or_else(|| draft.name.clone());
            // The name fills the row up to the two bone-navigation icon buttons.
            let name_width = (ui.available_width()
                - 2.0 * (neoflash_ui::metrics::control() + space::s2()))
            .max(neoflash_ui::metrics::field_s());
            let name_resp = ui.add_enabled(
                false,
                c::TextField::new(&mut bone_id_label, "Bone name").width(name_width),
            );
            let _ = c::not_available(name_resp, "Bone renaming");
            if ui
                .add_enabled(
                    draft.bone_index > 0,
                    c::IconButton::new(i16::CHEVUP, "Parent bone"),
                )
                .clicked()
            {
                draft.bone_index = draft.bone_index.saturating_sub(1);
                navigated_bone = true;
            }
            if ui
                .add_enabled(
                    draft.bone_index + 1 < bone_total,
                    c::IconButton::new(i16::CHEVDOWN, "Child bone"),
                )
                .clicked()
            {
                draft.bone_index += 1;
                navigated_bone = true;
            }
        });
        section(ui, "Position", true, |ui| {
            let (x, y) = (&mut draft.x, &mut draft.y);
            let (x_response, y_response) = two_column_row(
                ui,
                |ui, width| ui.add(c::Field::new(x, "X").prefix("X").decimals(1).width(width)),
                |ui, width| ui.add(c::Field::new(y, "Y").prefix("Y").decimals(1).width(width)),
            );
            changed |= x_response.changed() || y_response.changed();
            let (length, angle) = (&mut draft.length, &mut draft.angle);
            let (length_response, ()) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(length, "Length")
                            .prefix("Length")
                            .unit("px")
                            .range(4.0..=1000.0)
                            .decimals(0)
                            .width(width),
                    )
                },
                |ui, width| {
                    let angle_resp = ui.add_enabled(
                        false,
                        c::Field::new(angle, "Angle")
                            .prefix("Angle")
                            .unit("°")
                            .range(-360.0..=360.0)
                            .decimals(0)
                            .width(width),
                    );
                    let _ = c::not_available(angle_resp, "Manual bone angle");
                },
            );
            changed |= length_response.changed();
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
            let (rot_min, rot_max) = (&mut draft.rot_min, &mut draft.rot_max);
            let (min_response, max_response) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(rot_min, "Min")
                            .prefix("Min")
                            .unit("°")
                            .range(-180.0..=180.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(rot_max, "Max")
                            .prefix("Max")
                            .unit("°")
                            .range(-180.0..=180.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
            );
            changed |= min_response.changed() || max_response.changed();
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
            let (spring_strength, spring_damping) =
                (&mut draft.spring_strength, &mut draft.spring_damping);
            let (strength_response, damping_response) = two_column_row(
                ui,
                |ui, width| {
                    ui.add(
                        c::Field::new(spring_strength, "Strength")
                            .prefix("Strength")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.add(
                        c::Field::new(spring_damping, "Damping")
                            .prefix("Damping")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(width),
                    )
                },
            );
            if strength_response.changed() && draft.spring_strength > 0.0 {
                draft.enable_spring = true;
            }
            changed |= strength_response.changed() || damping_response.changed();
        });
        section(ui, "Armature", true, |ui| {
            let type_lbl = if draft.armature_runtime {
                "Runtime"
            } else {
                "Authortime"
            };
            let style_lbl = if draft.armature_wireframe {
                "Wire"
            } else {
                "Solid"
            };
            two_column_row(
                ui,
                |ui, width| {
                    let type_resp = ui
                        .scope(|ui| {
                            ui.disable();
                            c::Select::new(ui.id().with("arm-type"), type_lbl)
                                .prefix("Type")
                                .width(width)
                                .show(ui, |_| {})
                                .0
                        })
                        .inner;
                    let _ = c::not_available(type_resp, "Armature type mode");
                },
                |ui, width| {
                    let style_resp = ui
                        .scope(|ui| {
                            ui.disable();
                            c::Select::new(ui.id().with("arm-style"), style_lbl)
                                .prefix("Style")
                                .width(width)
                                .show(ui, |_| {})
                                .0
                        })
                        .inner;
                    let _ = c::not_available(style_resp, "Armature display style");
                },
            );
            // FORM_GRID_EXCEPTION: Speed is deliberately paired with an action switch.
            let speed_width = (ui.available_width() - space::s2()) / 2.0;
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        c::Field::new(&mut draft.speed_pct, "Speed")
                            .prefix("Speed")
                            .unit("%")
                            .range(0.0..=100.0)
                            .decimals(0)
                            .stepper()
                            .width(speed_width),
                    )
                    .changed();
                changed |= ui
                    .add(c::Switch::new(&mut draft.enable_spring, "Enable spring"))
                    .changed();
            });
        });

        if navigated_bone {
            self.set_stage_active_ik_bone_id(
                armature.bones.get(draft.bone_index).map(|b| b.id.clone()),
            );
        }
        let snapshot_draft = draft.clone();
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));

        if changed && self.layer_editable() {
            let mut arm = armature;
            let b_idx = snapshot_draft
                .bone_index
                .min(arm.bones.len().saturating_sub(1));
            snapshot_draft.persist_supported_bone_fields(&mut arm, b_idx);
            let persisted_bone_id = arm
                .bones
                .get(b_idx)
                .map_or(snapshot_draft.name.as_str(), |bone| bone.id.as_str());
            let joint_spec =
                snapshot_draft.transient_joint_spec_with_base(persisted_bone_id, persisted_spec);
            let _ = self.apply_persisted_ik_armature_and_joint_spec(arm, joint_spec);
        }
    }

    #[cfg(test)]
    fn live_graphic_properties(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let selected_instance = self.selected_element().and_then(|el| match &el.kind {
            ElementKind::Instance {
                symbol_id,
                playback,
            } => {
                let sym = self.project.symbol(symbol_id)?;
                Some((
                    symbol_id.clone(),
                    sym.name.clone(),
                    sym.timeline.frame_count.max(1),
                    *playback,
                    el.transform.x,
                    el.transform.y,
                    el.transform.scale_x * 100.0,
                    el.transform.scale_y * 100.0,
                    el.transform.alpha * 100.0,
                ))
            }
            _ => None,
        });

        let Some((
            sym_id,
            sym_name,
            sym_frames,
            current_playback,
            mut x,
            mut y,
            mut scale_x_pct,
            mut scale_y_pct,
            mut alpha_pct,
        )) = selected_instance
        else {
            ui.add(c::TypeHeader {
                icon: i16::GRAPHIC,
                title: "Graphic",
                subtitle: "No graphic instance selected",
            });
            ui.horizontal(|ui| {
                let w = ui.available_width() - 72.0 - space::s2();
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
                let swap_resp = ui.add_enabled(false, c::Button::new("Swap…"));
                c::tooltip(swap_resp, "Select a symbol instance on stage", None);
            });
            caption(
                ui,
                "Select a Graphic or symbol instance on Stage to edit looping and first-frame thumbnails.",
            );
            return;
        };

        let can_edit_sel = self
            .selection
            .single()
            .is_some_and(|t| self.target_editable(t));
        ui.add(c::TypeHeader {
            icon: i16::GRAPHIC,
            title: "Graphic",
            subtitle: &format!("Instance of \"{sym_name}\" · {sym_frames} frames"),
        });
        ui.horizontal(|ui| {
            let w = ui.available_width() - 72.0 - space::s2();
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
            if ui
                .add_enabled(can_edit_sel, c::Button::new("Swap…"))
                .clicked()
            {
                let _ = self.swap_selected_instance_with_library_or_next();
            }
        });

        let mut xform_changed = false;
        c::SectionHeader::new("Position and size")
            .id_salt(("live-graphic", "Position and size"))
            .default_open(false)
            .show(ui, |ui| {
                let (xr, yr) = two_column_row(
                    ui,
                    |ui, width| {
                        ui.add_enabled(
                            can_edit_sel,
                            c::Field::new(&mut x, "X")
                                .prefix("X")
                                .decimals(1)
                                .width(width),
                        )
                    },
                    |ui, width| {
                        ui.add_enabled(
                            can_edit_sel,
                            c::Field::new(&mut y, "Y")
                                .prefix("Y")
                                .decimals(1)
                                .width(width),
                        )
                    },
                );
                let (sxr, syr) = two_column_row(
                    ui,
                    |ui, width| {
                        ui.add_enabled(
                            can_edit_sel,
                            c::Field::new(&mut scale_x_pct, "Scale X")
                                .prefix("W")
                                .unit("%")
                                .range(-10_000.0..=10_000.0)
                                .decimals(1)
                                .width(width),
                        )
                    },
                    |ui, width| {
                        ui.add_enabled(
                            can_edit_sel,
                            c::Field::new(&mut scale_y_pct, "Scale Y")
                                .prefix("H")
                                .unit("%")
                                .range(-10_000.0..=10_000.0)
                                .decimals(1)
                                .width(width),
                        )
                    },
                );
                xform_changed |= xr.changed() || yr.changed() || sxr.changed() || syr.changed();
            });
        c::SectionHeader::new("Colour effect")
            .id_salt(("live-graphic", "Colour effect"))
            .default_open(false)
            .show(ui, |ui| {
                let alpha_resp = ui.add_enabled(
                    can_edit_sel,
                    c::Field::new(&mut alpha_pct, "Opacity")
                        .prefix("Alpha")
                        .unit("%")
                        .range(0.0..=100.0)
                        .decimals(0)
                        .width(ui.available_width()),
                );
                xform_changed |= alpha_resp.changed();
            });

        if xform_changed
            && can_edit_sel
            && let Some(sel) = self.selection.single().cloned()
        {
            self.transact_named("Edit graphic transform", |editor| {
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
                    el.transform.scale_x = scale_x_pct / 100.0;
                    el.transform.scale_y = scale_y_pct / 100.0;
                    el.transform.alpha = (alpha_pct / 100.0).clamp(0.0, 1.0);
                }
            });
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
                for (i, label, supported) in [
                    (0usize, "Loop", true),
                    (1, "Play once", true),
                    (2, "Single frame", true),
                    (3, "Loop reverse", false),
                    (4, "Play once reverse", false),
                ] {
                    if supported {
                        if ui
                            .add_enabled(can_edit_sel, c::Radio::new(i == mode_idx, label))
                            .clicked()
                        {
                            mode_idx = i;
                            playback_changed = true;
                        }
                    } else {
                        let resp = ui.add_enabled(false, c::Radio::new(false, label));
                        let _ = c::not_available(resp, "Reverse graphic playback");
                    }
                }
                let mut first_1 = (first_zero + 1).min(sym_frames) as f64;
                let mut last_1 = sym_frames as f64;
                let (first_response, ()) = two_column_row(
                    ui,
                    |ui, width| {
                        ui.add_enabled(
                            can_edit_sel,
                            c::Field::new(&mut first_1, "First frame")
                                .prefix("First")
                                .range(1.0..=f64::from(sym_frames))
                                .decimals(0)
                                .stepper()
                                .width(width),
                        )
                    },
                    |ui, width| {
                        let last_resp = ui.add_enabled(
                            false,
                            c::Field::new(&mut last_1, "Last frame")
                                .prefix("Last")
                                .range(1.0..=f64::from(sym_frames))
                                .decimals(0)
                                .stepper()
                                .width(width),
                        );
                        let _ = c::not_available(last_resp, "Custom last frame limit");
                    },
                );
                if first_response.changed() {
                    first_zero = (first_1.round() as u32).clamp(1, sym_frames) - 1;
                    playback_changed = true;
                }

                let (strip, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 44.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    strip,
                    egui::CornerRadius::same(radius::md() as u8),
                    nf.bg_field,
                );
                let entries = self.graphic_frame_picker_entries();
                let slots = sym_frames.min(7);
                for i in 0..slots {
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(strip.left() + 4.0 + i as f32 * 40.0, strip.top() + 4.0),
                        egui::vec2(36.0, 36.0),
                    );
                    let resp = ui.interact(
                        cell,
                        ui.id().with(("graphic-thumb", &sym_id, i)),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(
                        cell,
                        egui::CornerRadius::same(radius::sm() as u8),
                        nf.bg_raised,
                    );
                    if let Some(tex) =
                        cached_frame_thumbnail(ui.ctx(), self, &sym_id, i, 72, 72, false)
                    {
                        ui.painter().image(
                            tex.id(),
                            cell.shrink(2.0),
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    let selected = first_zero == i;
                    let (w, color) = if selected {
                        (2.0_f32, nf.accent_default)
                    } else {
                        (1.0, nf.border_subtle)
                    };
                    ui.painter().rect_stroke(
                        cell,
                        egui::CornerRadius::same(radius::sm() as u8),
                        egui::Stroke::new(w, color),
                        egui::StrokeKind::Inside,
                    );
                    if resp.clicked() && can_edit_sel {
                        first_zero = i.min(sym_frames.saturating_sub(1));
                        if !self.pick_graphic_frame(first_zero) {
                            playback_changed = true;
                        }
                    }
                    if let Some((_, tip)) = entries.get(i as usize) {
                        c::tooltip(resp, tip, None);
                    }
                }
                caption(ui, "Click a thumbnail to set the first frame");
            });

        if playback_changed
            && can_edit_sel
            && let Some(sel) = self.selection.single().cloned()
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

    fn live_test_movie(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let id = egui::Id::new(("nf2-live-test-movie-draft", &self.clipboard.session));
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
        let aspect = (self.project.width.max(1) as f32) / (self.project.height.max(1) as f32);
        let max_w = area.width().clamp(120.0, 640.0);
        let max_h = area.height().clamp(90.0, 420.0);
        let (fit_w, fit_h) = if max_w / max_h > aspect {
            (max_h * aspect, max_h)
        } else {
            (max_w, max_w / aspect)
        };
        let stage = egui::Rect::from_center_size(area.center(), egui::vec2(fit_w, fit_h));
        let bg = self.project.background;
        ui.painter().rect_filled(
            stage,
            0.0,
            Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], bg[3]),
        );
        let pw = (fit_w.round() as u32).clamp(64, 640);
        let ph = (fit_h.round() as u32).clamp(64, 480);
        if let Some(tex) =
            cached_frame_thumbnail(ui.ctx(), self, self.symbol_id(), draft.frame, pw, ph, true)
        {
            ui.painter().image(
                tex.id(),
                stage,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
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
            ui.painter().rect_filled(rail, r, nf.border_strong);
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
                c::EmptyState {
                    icon: i16::TERMINAL,
                    title: "No output yet",
                    body: "Status messages, script checks and export results appear here.",
                    action: None,
                }
                .show(ui);
            } else {
                for event in self.output.events().iter().rev().take(3).rev() {
                    ui.add(c::LogLine {
                        severity: event.severity.ui_severity(),
                        time: "",
                        text: &event.message,
                    });
                }
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
    }
}

#[cfg(test)]
fn y_ref(v: &mut f64) -> &mut f64 {
    v
}

#[cfg(test)]
mod bone_panel_tests {
    use super::*;

    #[test]
    fn bone_panel_reads_writes_and_solves_constraints_in_degrees() {
        let mut armature = IkArmature {
            id: "arm".into(),
            name: "Arm".into(),
            bones: vec![neoflash_core::IkBone {
                id: "bone".into(),
                element_id: "bone-element".into(),
                parent_bone_id: None,
                length: 100.0,
                min_angle: Some(-5.0),
                max_angle: Some(5.0),
                stiffness: 0.4,
            }],
            effector_bone_id: "bone".into(),
            target_pos: [0.0, 100.0],
            pole_pos: None,
        };
        let mut draft = BoneDraft::default();

        draft.load_persisted_bone_fields(&armature, 0);
        assert_eq!(draft.rot_min, -5.0);
        assert_eq!(draft.rot_max, 5.0);
        assert_eq!(draft.speed_pct, 40.0);

        draft.rot_min = -7.5;
        draft.rot_max = 6.5;
        draft.length = 80.0;
        draft.x = 0.0;
        draft.y = 100.0;
        draft.speed_pct = 5.0;
        draft.name = "draft-only-rename".into();
        draft.persist_supported_bone_fields(&mut armature, 0);
        assert_eq!(armature.bones[0].min_angle, Some(-7.5));
        assert_eq!(armature.bones[0].max_angle, Some(6.5));
        assert_eq!(armature.bones[0].length, 80.0);
        assert_eq!(armature.bones[0].id, "bone");

        let spec = draft.transient_joint_spec(&armature.bones[0].id);
        assert_eq!(spec.bone_id, "bone");

        let mut elements = vec![neoflash_core::Element {
            id: "bone-element".into(),
            ..neoflash_core::Element::default()
        }];
        neoflash_core::solve_ik_armature_fabrik_ccd(&mut elements, &armature, &[spec]);
        assert!(elements[0].transform.rotation >= -7.5 - 1e-6);
        assert!(elements[0].transform.rotation <= 6.5 + 1e-6);
        assert!((elements[0].transform.rotation - 6.5).abs() <= 1e-6);
    }

    #[test]
    fn ik_bind_skin_weights_and_spring_damping_persist_across_save_load_evaluate_and_undo() {
        use neoflash_core::{
            Element, FillRule, PathCommand, Project, Transform, extract_path_control_points,
            sample_layer_at_fps,
        };

        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        neoflash_ui::theme::install(&ctx, crate::theme::ThemeMode::Dark.nf2());
        let mut editor = Editor::from_project(&ctx, Project::blank());

        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "bone-root".into(),
                transform: Transform {
                    x: 100.0,
                    y: 100.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 20.0,
                    height: 10.0,
                    fill: [80, 80, 80, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: "bone-tip".into(),
                transform: Transform {
                    x: 180.0,
                    y: 100.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 20.0,
                    height: 10.0,
                    fill: [120, 120, 120, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: "sleeve-shape".into(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 100.0, y: 90.0 },
                        PathCommand::LineTo { x: 200.0, y: 90.0 },
                        PathCommand::LineTo { x: 200.0, y: 110.0 },
                        PathCommand::LineTo { x: 100.0, y: 110.0 },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [220, 90, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
        ];
        editor.history.clear();

        let tip_bone_id = editor
            .stage_connect_ik_bone("bone-root", "bone-tip")
            .expect("connected IK bone chain");
        let root_bone_id = editor.symbol().timeline.layers[0].keyframes[0].ik_armatures[0].bones[0]
            .id
            .clone();
        assert_eq!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .ik_joint_specs
                .len(),
            2,
            "connecting bones persists initial IkJointSpec entries on the keyframe"
        );

        // Set spring_strength = 65, spring_damping = 35 on the root bone and verify 1 Undo step.
        let before_spring = editor.project.clone();
        editor.history.clear();
        assert!(editor.set_ik_bone_spring_and_damping(&root_bone_id, 65.0, 35.0, Some(false)));
        assert_eq!(editor.history.undo_len(), 1);
        editor.project.validate().expect("valid after spring edit");
        {
            let kf = &editor.symbol().timeline.layers[0].keyframes[0];
            let spec = kf
                .ik_joint_specs
                .iter()
                .find(|s| s.bone_id == root_bone_id)
                .expect("persisted root joint spec");
            assert!((spec.spring_strength - 65.0).abs() < 1e-6);
            assert!((spec.spring_damping - 35.0).abs() < 1e-6);
        }
        editor.undo();
        assert_eq!(editor.project, before_spring);
        editor.redo();

        // Bind vertex 1 (x=200, y=90) with 75% weight to tip_bone_id and verify 1 Undo step.
        editor.history.clear();
        let before_bind = editor.project.clone();
        let skin = editor
            .stage_bind_assign_vertices_to_bone("sleeve-shape", &tip_bone_id, &[1, 2], 0.75, false)
            .expect("bound vertices to tip bone");
        assert_eq!(editor.history.undo_len(), 1);
        editor.project.validate().expect("valid after vertex bind");
        assert!(editor.project.format_version >= 16);
        let v1_tip_w = skin
            .bindings
            .iter()
            .find(|b| b.vertex_index == 1)
            .and_then(|b| b.weights.iter().find(|w| w.bone_id == tip_bone_id))
            .map(|w| w.weight)
            .expect("tip bone weight on vertex 1");
        assert!(
            (v1_tip_w - 0.75).abs() < 1e-6,
            "expected exact 75% weight, got {v1_tip_w}"
        );
        editor.undo();
        assert_eq!(editor.project, before_bind);
        editor.redo();

        // Round-trip through JSON (.nflash) into a brand-new Editor session with empty transient state!
        let saved_json = serde_json::to_string(&editor.project).expect("serialize .nflash");
        let reloaded_project: Project =
            serde_json::from_str(&saved_json).expect("deserialize .nflash");
        reloaded_project
            .validate()
            .expect("reloaded project passes validation");

        let mut fresh_editor = Editor::from_project(&ctx, reloaded_project);
        fresh_editor.set_tool(crate::app::Tool::Bind);
        {
            let kf = &fresh_editor.symbol().timeline.layers[0].keyframes[0];
            assert_eq!(kf.ik_skins.len(), 1);
            let spec = kf
                .ik_joint_specs
                .iter()
                .find(|s| s.bone_id == root_bone_id)
                .unwrap();
            assert!((spec.spring_strength - 65.0).abs() < 1e-6);
            assert!((spec.spring_damping - 35.0).abs() < 1e-6);
        }

        // Render live_bone_properties & bone_inspector_section_ui on the reloaded project.
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 960.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    fresh_editor.live_bone_properties(ui);
                    fresh_editor.bone_inspector_section_ui(ui);
                });
            },
        );

        // Pose effector downward on the fresh editor: persisted skin deforms the sleeve shape!
        let rest_pts = {
            let el = &fresh_editor.symbol().timeline.layers[0].keyframes[0].elements[2];
            let (cmds, _) = el.to_path_commands().unwrap();
            extract_path_control_points(&cmds)
        };
        assert!(fresh_editor.stage_drag_ik_effector([160.0, 200.0], None));
        fresh_editor
            .project
            .validate()
            .expect("valid after effector drag");
        let posed_pts = {
            let sampled = sample_layer_at_fps(
                &fresh_editor.symbol().timeline.layers[0],
                0,
                f64::from(fresh_editor.project.fps),
            );
            let (cmds, _) = sampled[2].to_path_commands().unwrap();
            extract_path_control_points(&cmds)
        };
        assert!(
            (posed_pts[1][1] - rest_pts[1][1]).abs() > 5.0,
            "bound sleeve vertex 1 must deform with effector pose ({:?} -> {:?})",
            rest_pts[1],
            posed_pts[1]
        );
    }

    #[test]
    fn ik_lifecycle_orphan_pruning_symbol_duplication_and_validation_bounds_hold() {
        use neoflash_core::{Element, ElementKind, FillRule, PathCommand, Project, Transform};

        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        neoflash_ui::theme::install(&ctx, crate::theme::ThemeMode::Dark.nf2());
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "b-root".into(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 12.0,
                    fill: [80, 120, 220, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                transform: Transform {
                    x: 80.0,
                    y: 94.0,
                    ..Transform::default()
                },
                ..Element::default()
            },
            Element {
                id: "b-tip".into(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 12.0,
                    fill: [100, 160, 240, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                transform: Transform {
                    x: 130.0,
                    y: 94.0,
                    ..Transform::default()
                },
                ..Element::default()
            },
            Element {
                id: "skin-path".into(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 80.0, y: 90.0 },
                        PathCommand::LineTo { x: 170.0, y: 90.0 },
                        PathCommand::LineTo { x: 170.0, y: 110.0 },
                        PathCommand::LineTo { x: 80.0, y: 110.0 },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [220, 90, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
        ];
        let tip_bone_id = editor
            .stage_connect_ik_bone("b-root", "b-tip")
            .expect("connected bone chain");
        let root_bone_id = editor.symbol().timeline.layers[0].keyframes[0].ik_armatures[0].bones[0]
            .id
            .clone();
        editor
            .stage_bind_assign_vertices_to_bone("skin-path", &tip_bone_id, &[1, 2], 0.8, false)
            .expect("bound skin vertices");
        editor.project.validate().unwrap();

        // Active bone sync between stage session and live_bone_properties draft.
        editor.set_stage_active_ik_bone_id(Some(tip_bone_id.clone()));
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 960.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.live_bone_properties(ui);
                    editor.bone_inspector_section_ui(ui);
                });
            },
        );
        assert_eq!(
            editor.stage_active_ik_bone_id().as_deref(),
            Some(tip_bone_id.as_str())
        );
        let mut spring_heading_count = 0usize;
        fn count_label(shape: &egui::Shape, needle: &str, count: &mut usize) {
            match shape {
                egui::Shape::Text(t) => {
                    if t.galley.text().trim().eq_ignore_ascii_case(needle) {
                        *count += 1;
                    }
                }
                egui::Shape::Vec(v) => {
                    for s in v {
                        count_label(s, needle, count);
                    }
                }
                _ => {}
            }
        }
        for clipped in &out.shapes {
            count_label(&clipped.shape, "Spring", &mut spring_heading_count);
        }
        assert_eq!(
            spring_heading_count, 1,
            "Properties — IK bone must render exactly one locked Spring section"
        );

        // Validation bounds: inverted min > max on IkJointSpec, out-of-range skin weight, and
        // invalid PatternBrushKind parameters must be rejected; non-finite Hatched angle is safe.
        {
            let mut bad_joint = editor.project.clone();
            let root_id = bad_joint.root.clone();
            {
                let js = &mut bad_joint.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes
                    [0]
                .ik_joint_specs[0];
                js.constrain_rotation = true;
                js.min_angle_deg = 45.0;
                js.max_angle_deg = -45.0;
            }
            assert!(bad_joint.validate().is_err());

            let mut bad_skin = editor.project.clone();
            bad_skin.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].ik_skins[0]
                .bindings[0]
                .weights[0]
                .weight = 1.5;
            assert!(bad_skin.validate().is_err());

            let safe_hatched = neoflash_core::expand_pattern_brush_stroke(
                &[
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 40.0, y: 0.0 },
                ],
                4.0,
                &neoflash_core::PatternBrushKind::Hatched {
                    spacing: 8.0,
                    angle_deg: f64::NAN,
                },
            );
            assert!(!safe_hatched.is_empty());
        }

        // Orphan pruning: removing the bound shape prunes `ik_skins`; removing bone elements
        // prunes `ik_joint_specs`, keeping `project.validate()` valid.
        {
            let kf = &mut editor.symbol_mut().timeline.layers[0].keyframes[0];
            kf.elements.retain(|el| el.id != "skin-path");
            crate::timeline_commands::prune_keyframe_orphan_element_refs(kf);
            assert!(kf.ik_skins.is_empty());
            assert_eq!(kf.ik_joint_specs.len(), 2);
        }
        editor.project.validate().unwrap();
        {
            let kf = &mut editor.symbol_mut().timeline.layers[0].keyframes[0];
            kf.elements.retain(|el| el.id != "b-tip");
            crate::timeline_commands::prune_keyframe_orphan_element_refs(kf);
            assert_eq!(kf.ik_joint_specs.len(), 1);
            assert_eq!(kf.ik_joint_specs[0].bone_id, root_bone_id);
        }
        editor.project.validate().unwrap();
    }
}

#[cfg(test)]
mod t297_planned_wiring_tests {
    use super::*;
    use neoflash_core::{ElementKind, Keyframe, Layer, Project, Symbol, SymbolKind, Timeline};

    fn collect_shape_texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for child in shapes {
                    collect_shape_texts(child, out);
                }
            }
            _ => {}
        }
    }

    fn collect_output_texts(out: &egui::FullOutput) -> Vec<String> {
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_shape_texts(&clipped.shape, &mut texts);
        }
        texts
    }

    fn test_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        neoflash_ui::theme::install(&ctx, crate::theme::ThemeMode::Dark.nf2());
        ctx
    }

    #[test]
    fn live_dialogs_tool_panels_and_test_movie_render_without_fake_sample_data_on_blank_project() {
        let ctx = test_ctx();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let raw_1920 = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            time: Some(1.0),
            ..Default::default()
        };
        let _ = ctx.run(raw_1920(), |_| {});
        editor.open_test_movie_preview();

        let render_all = |ed: &mut Editor| {
            ctx.run(raw_1920(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    // The Debugger fills the panel it is docked in (its panes scroll), so it gets
                    // a region of its own here instead of the rest of the page.
                    let debugger = egui::vec2(ui.available_width(), 240.0);
                    ui.allocate_ui(debugger, |ui| {
                        let _ = ed.debugger_contents(ui);
                    });
                    ed.live_graphic_properties(ui);
                    ed.live_bone_properties(ui);
                    ed.live_deco_tool(ui);
                    ed.live_spray_brush(ui);
                    ed.live_bitmap_fill(ui);
                    ed.live_properties_oval(ui);
                    ed.live_properties_rect(ui);
                });
                ed.test_movie_preview_window(ctx);
                ed.font_embedding_dialog(ctx);
                ed.import_video_dialog(ctx);
            })
        };
        editor.open_font_embedding(&ctx);
        editor.open_import_video(&ctx);
        let _ = render_all(&mut editor);
        let out = render_all(&mut editor);
        let texts = collect_output_texts(&out);
        let joined = texts.join("\n");

        for forbidden in [
            "Planned feature",
            "~/Movies/intro.mp4",
            "1280×720 · 30 fps",
            "Ideas in motion",
            "Stage preview",
            "keeps .nfl small",
            "leaf_mc",
            "arm_lower",
            "planet",
            "tile_1.png",
        ] {
            assert!(
                !joined.contains(forbidden),
                "unexpected fake sample string {forbidden:?} in live panels"
            );
        }

        assert!(joined.contains("No graphic instance selected"));
        assert!(joined.contains("No armature on current frame"));
        assert!(joined.contains("No embedded fonts"));
        assert!(joined.contains("keeps the .nflash file small"));
        assert!(joined.contains("No output yet"));
        assert!(joined.contains("Not running"));
    }

    #[test]
    fn bitmap_fill_oval_rect_and_stroke_style_bind_selection_and_undo() {
        let ctx = test_ctx();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [10, 20, 30, 255],
                stroke: [40, 50, 60, 255],
                stroke_width: 2.0,
            },
            10.0,
            20.0,
        );
        assert!(editor.selection.single().is_some());

        // Bitmap resource imported into project updates fill and undoes cleanly
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let tmp_png = std::env::temp_dir().join(format!("nf-t297-tile-{}.png", std::process::id()));
        std::fs::write(&tmp_png, png_bytes).expect("write test png");
        editor
            .import_resource_paths(std::slice::from_ref(&tmp_png))
            .expect("import png resource");
        let _ = std::fs::remove_file(&tmp_png);
        let res_id = editor.project.resources[0].id.clone();

        editor
            .set_selected_bitmap_fill(res_id.clone(), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], true, true)
            .expect("apply bitmap fill");
        assert_eq!(editor.selected_element_bitmap_fill_id(), Some(res_id));
        editor.undo();
        assert_eq!(editor.selected_element_bitmap_fill_id(), None);

        // Color & stroke width edits bind to real selection and Undo cleanly
        editor.apply_selected_element_color(false, [200, 100, 50, 255], "Edit fill");
        assert_eq!(editor.selected_element_paint().0, [200, 100, 50, 255]);
        editor.undo();
        assert_eq!(editor.selected_element_paint().0, [10, 20, 30, 255]);

        editor.apply_selected_stroke_width(6.5, "Edit stroke width");
        assert!((editor.selected_element_paint().2 - 6.5).abs() < 1e-4);
        editor.undo();
        assert!((editor.selected_element_paint().2 - 2.0).abs() < 1e-4);

        // Stroke style dashed application & undo
        assert!(editor.apply_stroke_style_to_selection(StrokeStyleKind::Dashed, 4.0, 12.0, 6.0));
        let draft = editor.init_stroke_style_draft();
        assert_eq!(draft.kind, StrokeStyleKind::Dashed);
        assert_eq!(draft.dash, 12.0);
        assert_eq!(draft.gap, 6.0);
        editor.undo();
        assert_eq!(
            editor.init_stroke_style_draft().kind,
            StrokeStyleKind::Solid
        );
    }

    #[test]
    fn deco_spray_graphic_test_movie_and_import_video_bind_document_and_undo() {
        let ctx = test_ctx();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.project.name = "HeroScene".into();

        // ImportVideo defaults have no fake path/fps
        let iv_default = crate::panels::import_video::ImportVideoDraft::default();
        assert!(iv_default.file_path.is_empty());
        assert_eq!(iv_default.source_fps, 0);

        // FontEmbedding defaults start empty without synthetic entries
        let fe_default = crate::panels::font_embedding::FontEmbeddingDraft::default();
        assert!(fe_default.entries.is_empty());

        // SprayBrush default has no fake leaf_mc symbol
        let spray_default = SprayDraft::default();
        assert!(spray_default.symbol_name.is_empty());

        // DecoDraft syncs with real editor StageDecoOptions
        let mut deco = DecoDraft::from_stage(&editor.stage_deco_options());
        deco.scale_pct = 150.0;
        let mut updated_opts = editor.stage_deco_options();
        updated_opts.scale_pct = deco.scale_pct;
        editor.set_stage_deco_options(updated_opts);
        assert_eq!(editor.stage_deco_options().scale_pct, 150.0);

        // GraphicProperties binds real Graphic symbol instance and renders cached thumbnails
        let sym_id = new_id("sym");
        editor.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: "HeroGraphic".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                guides: vec![],
                frame_count: 3,
                layers: vec![Layer {
                    properties: Default::default(),
                    id: new_id("layer"),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![],
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    }],
                }],
                scripts: vec![],
            },
        });
        editor.add_element(
            ElementKind::Instance {
                symbol_id: sym_id.clone(),
                playback: InstancePlayback::Loop,
            },
            40.0,
            50.0,
        );
        assert!(cached_frame_thumbnail(&ctx, &editor, &sym_id, 0, 72, 72, false).is_some());

        let raw_1920 = |time| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            time: Some(time),
            ..Default::default()
        };
        let _ = ctx.run(raw_1920(1.0), |_| {});
        editor.open_test_movie_preview();
        let render = |ed: &mut Editor, time| {
            ctx.run(raw_1920(time), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ed.live_graphic_properties(ui);
                });
                ed.test_movie_preview_window(ctx);
            })
        };
        let _ = render(&mut editor, 1.5);
        let out = render(&mut editor, 2.0);
        let joined = collect_output_texts(&out).join("\n");
        assert!(joined.contains("HeroGraphic"));
        assert!(joined.contains("HeroScene — Test Movie"));
        assert!(!joined.contains("No graphic instance selected"));
        assert!(!joined.contains("Planned feature"));
    }
}
