//! Stage rulers, guides and grid (NF2 ST-01…ST-06; docs/NF2-RULERS-GUIDES-GRID.md).
//!
//! - **View settings** ([`ViewPrefs`]) are editor state, not document data. The session starts from
//!   the saved default (`WorkspacePrefs::view_defaults`, written by "Save as default") or the NF2
//!   defaults. They are edited by the View menu, the zoom-control toggles, shortcuts and the
//!   `NF2/View Settings` popover, which all change the same fields.
//! - **Guides** are document data: `Timeline::guides` of the timeline being edited (scene or
//!   symbol), in timeline coordinates. Add, move, delete and clear are one Undo step each.
//! - **Rulers** overlay the canvas (`RulerLayout`) in their own interactable layer, so stage tools,
//!   which ignore presses on other layers, never see ruler presses. Guide presses on the stage are
//!   taken out of the frame's events before the tools read them.
//!
//! Every visual comes from `neoflash_ui::components::canvas` (`NF2/Stage Ruler`, `NF2/Ruler Corner`,
//! `NF2/Guide`, `NF2/Stage Grid`, `NF2/Snap Ring`) and `view_settings` (`NF2/View Settings`).
use crate::app::{Editor, Tool};
use crate::snapping::Snapper;
use crate::stage_mapping::StageMapping;
use eframe::egui::{self, Pos2, Rect, Sense, Vec2};
use neoflash_core::{Guide, GuideAxis, Id};
use neoflash_ui::components::canvas::{self as cv, GuideState, Orientation, RulerLayout};
use neoflash_ui::components::view_settings::{self as vs, ViewSettingsState};
use neoflash_ui::tokens::Palette;
use serde::{Deserialize, Serialize};

/// Ruler units (View settings ▸ Units). Points equal pixels at 72 ppi, as in Flash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulerUnits {
    #[default]
    Pixels,
    Points,
    Inches,
    Centimetres,
    Millimetres,
}

impl RulerUnits {
    pub const ALL: [Self; 5] = [
        Self::Pixels,
        Self::Points,
        Self::Inches,
        Self::Centimetres,
        Self::Millimetres,
    ];
    /// Stage pixels per unit.
    pub fn pixels(self) -> f32 {
        match self {
            Self::Pixels | Self::Points => 1.,
            Self::Inches => 72.,
            Self::Centimetres => 72. / 2.54,
            Self::Millimetres => 7.2 / 2.54,
        }
    }
    fn label(self) -> &'static str {
        vs::UNITS[Self::ALL.iter().position(|u| *u == self).unwrap_or(0)]
    }
    fn from_label(label: &str) -> Self {
        vs::UNITS
            .iter()
            .position(|u| *u == label)
            .map_or(Self::Pixels, |i| Self::ALL[i])
    }
}

/// Snap accuracy (View settings ▸ Snap: …): distance in screen points within which a point snaps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapAccuracy {
    MustBeClose,
    #[default]
    Normal,
    CanBeDistant,
}

impl SnapAccuracy {
    pub const ALL: [Self; 3] = [Self::MustBeClose, Self::Normal, Self::CanBeDistant];
    /// Tolerance in screen points (zoom-independent).
    pub fn screen_tolerance(self) -> f32 {
        match self {
            Self::MustBeClose => 3.,
            Self::Normal => 8.,
            Self::CanBeDistant => 16.,
        }
    }
    fn label(self) -> &'static str {
        vs::SNAP_ACCURACY[Self::ALL.iter().position(|a| *a == self).unwrap_or(1)]
    }
    fn from_label(label: &str) -> Self {
        vs::SNAP_ACCURACY
            .iter()
            .position(|a| *a == label)
            .map_or(Self::Normal, |i| Self::ALL[i])
    }
}

/// Dynamic changes display density and page contrast; Static retains the manual colour and step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridMode {
    #[default]
    Dynamic,
    Static,
}

/// Rulers, guides and grid settings. Guide defaults follow the theme; static grid colour is saved
/// independently of Dynamic's calculated colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewPrefs {
    pub rulers: bool,
    pub units: RulerUnits,
    pub labels_auto: bool,
    /// Label interval in pixels when `labels_auto` is off.
    pub label_every: f64,
    pub guides_show: bool,
    pub guides_snap: bool,
    pub guides_lock: bool,
    pub guide_color: Option<[u8; 4]>,
    pub guide_accuracy: SnapAccuracy,
    pub grid_show: bool,
    pub grid_snap: bool,
    pub grid_over_objects: bool,
    pub grid_mode: GridMode,
    /// Grid spacing (horizontal, vertical) in stage pixels.
    pub grid_spacing: [f64; 2],
    pub grid_linked: bool,
    pub grid_color: Option<[u8; 4]>,
    pub grid_accuracy: SnapAccuracy,
    /// Snap to edges and centres of unselected visible unlocked objects on the current frame.
    #[serde(default)]
    pub objects_snap: bool,
    /// Snap to nearest integer stage pixel.
    #[serde(default)]
    pub pixels_snap: bool,
}

impl Default for ViewPrefs {
    /// NF2 defaults (`ViewSettingsState::defaults`): rulers, guides and grid shown, snap to guides
    /// on, snap to grid off, 20 px linked grid, Auto labels, Normal accuracy.
    fn default() -> Self {
        Self {
            rulers: true,
            units: RulerUnits::Pixels,
            labels_auto: true,
            label_every: 100.,
            guides_show: true,
            guides_snap: true,
            guides_lock: false,
            guide_color: None,
            guide_accuracy: SnapAccuracy::Normal,
            grid_show: true,
            grid_snap: false,
            grid_over_objects: false,
            grid_mode: GridMode::Dynamic,
            grid_spacing: [20., 20.],
            grid_linked: true,
            grid_color: None,
            grid_accuracy: SnapAccuracy::Normal,
            objects_snap: false,
            pixels_snap: false,
        }
    }
}

fn rgba(c: egui::Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}
fn color([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl ViewPrefs {
    /// Values clamped to what the View settings fields allow (also applied to loaded preferences).
    pub fn sanitize(&mut self) {
        let clamp = |v: f64, lo: f64, hi: f64, fallback: f64| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        };
        self.label_every = clamp(self.label_every, 1., 10_000., 100.);
        for s in &mut self.grid_spacing {
            *s = clamp(*s, 1., 1000., 20.);
        }
    }

    pub fn guide_color(&self, p: &Palette) -> egui::Color32 {
        self.guide_color.map_or(p.guide, color)
    }

    pub fn grid_color(&self, _p: &Palette) -> egui::Color32 {
        self.grid_color.map_or_else(
            || vs::default_grid_color(&neoflash_ui::tokens::LIGHT),
            color,
        )
    }

    /// The popover's state for these settings.
    pub fn to_state(&self, p: &Palette) -> ViewSettingsState {
        ViewSettingsState {
            show_rulers: self.rulers,
            units: self.units.label(),
            labels_auto: self.labels_auto,
            label_every: self.label_every,
            guides_show: self.guides_show,
            guides_snap: self.guides_snap,
            guides_lock: self.guides_lock,
            guide_color: self.guide_color(p),
            guide_snap: self.guide_accuracy.label(),
            grid_show: self.grid_show,
            grid_snap: self.grid_snap,
            grid_over_objects: self.grid_over_objects,
            grid_mode: match self.grid_mode {
                GridMode::Dynamic => "Dynamic",
                GridMode::Static => "Static",
            },
            grid_w: self.grid_spacing[0],
            grid_h: self.grid_spacing[1],
            grid_linked: self.grid_linked,
            grid_color: self.grid_color(p),
            grid_snap_accuracy: self.grid_accuracy.label(),
        }
    }

    /// Take edits back without replacing the saved manual grid colour with a dynamic preview.
    pub fn apply_state(&mut self, s: &ViewSettingsState, p: &Palette) {
        self.rulers = s.show_rulers;
        self.units = RulerUnits::from_label(s.units);
        self.labels_auto = s.labels_auto;
        self.label_every = s.label_every;
        self.guides_show = s.guides_show;
        self.guides_snap = s.guides_snap;
        self.guides_lock = s.guides_lock;
        self.guide_color = (s.guide_color != p.guide).then(|| rgba(s.guide_color));
        self.guide_accuracy = SnapAccuracy::from_label(s.guide_snap);
        self.grid_show = s.grid_show;
        self.grid_snap = s.grid_snap;
        self.grid_over_objects = s.grid_over_objects;
        self.grid_mode = if s.grid_mode == "Static" {
            GridMode::Static
        } else {
            GridMode::Dynamic
        };
        self.grid_spacing = [s.grid_w, s.grid_h];
        self.grid_linked = s.grid_linked;
        if s.grid_color != self.grid_color(p)
            || (self.grid_mode == GridMode::Static && self.grid_color.is_none())
        {
            self.grid_color = Some(rgba(s.grid_color));
        }
        self.grid_accuracy = SnapAccuracy::from_label(s.grid_snap_accuracy);
        self.sanitize();
    }
}

/// A guide being dragged: out of a ruler (`index: None`) or an existing guide.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuideDrag {
    symbol: Id,
    axis: GuideAxis,
    index: Option<usize>,
    /// Current position in timeline coordinates.
    position: f64,
    /// Pointer is over a ruler or outside the canvas: releasing deletes / discards the guide.
    remove: bool,
    /// History transaction open (existing guides change the document while dragging).
    began: bool,
}

/// Session state of the stage view chrome.
#[derive(Clone, Debug)]
pub(crate) struct StageView {
    pub prefs: ViewPrefs,
    /// Ruler zero point in timeline coordinates (Ruler Corner drag; double-click resets).
    pub zero: [f64; 2],
    /// `NF2/View Settings` popover open.
    pub settings_open: bool,
    /// Screen rect of the ⚙ button in the zoom control (popover anchor).
    pub settings_button: Option<Rect>,
    /// Selected guide (timeline, index) — shows the `NF2/Guide` Selected handle; Delete removes it.
    pub selected_guide: Option<(Id, usize)>,
    pub drag: Option<GuideDrag>,
    /// Corner drag in progress.
    pub zero_drag: bool,
    /// `NF2/Snap Ring` for the current gesture: screen point and whether it snapped.
    pub snap_ring: Option<(Pos2, bool)>,
    /// The pointer went down on a guide: stage click handlers ignore this press and its release.
    pub guide_pointer: bool,
    /// Temporary `⌘` (Command) modifier suspension of stage snapping during the current frame.
    pub command_suspend: bool,
    /// Canvas rect and timeline origin (screen) of the last stage frame.
    pub canvas: Rect,
    pub origin: Pos2,
}

impl Default for StageView {
    fn default() -> Self {
        Self {
            prefs: ViewPrefs::default(),
            zero: [0., 0.],
            settings_open: false,
            settings_button: None,
            selected_guide: None,
            drag: None,
            zero_drag: false,
            snap_ring: None,
            guide_pointer: false,
            command_suspend: false,
            canvas: Rect::NOTHING,
            origin: Pos2::ZERO,
        }
    }
}

impl StageView {
    pub fn toggle_object_snap(&mut self) {
        self.prefs.objects_snap = !self.prefs.objects_snap;
    }
    #[allow(dead_code)]
    pub fn toggle_pixel_snap(&mut self) {
        self.prefs.pixels_snap = !self.prefs.pixels_snap;
    }
}

fn orientation(axis: GuideAxis) -> Orientation {
    match axis {
        GuideAxis::Horizontal => Orientation::Horizontal,
        GuideAxis::Vertical => Orientation::Vertical,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AxisMapping {
    Direct,
    Swapped,
}

fn axis_mapping(mapping: StageMapping) -> Option<AxisMapping> {
    let [a, b, c, d, _, _] = mapping.matrix();
    if b.abs() <= 1e-9 && c.abs() <= 1e-9 {
        Some(AxisMapping::Direct)
    } else if a.abs() <= 1e-9 && d.abs() <= 1e-9 {
        Some(AxisMapping::Swapped)
    } else {
        None
    }
}

/// The locked ruler painter accepts only a positive one-dimensional scale.  Keep local ruler
/// labels only when both mapped axes increase in the painter's positive screen direction; a
/// quarter turn or reflection with a reversed axis uses the ordinary screen ruler until R-063
/// defines signed/oblique labels. Guides and grid continue to use [`axis_mapping`] independently.
fn ruler_axis_mapping(mapping: StageMapping) -> Option<AxisMapping> {
    let kind = axis_mapping(mapping)?;
    let [a, b, c, d, _, _] = mapping.matrix();
    match kind {
        AxisMapping::Direct if a > 1e-9 && d > 1e-9 => Some(kind),
        AxisMapping::Swapped if c > 1e-9 && b > 1e-9 => Some(kind),
        _ => None,
    }
}

fn screen_orientation(axis: GuideAxis, mapping: StageMapping) -> Option<Orientation> {
    match axis_mapping(mapping)? {
        AxisMapping::Direct => Some(orientation(axis)),
        AxisMapping::Swapped => Some(orientation(match axis {
            GuideAxis::Horizontal => GuideAxis::Vertical,
            GuideAxis::Vertical => GuideAxis::Horizontal,
        })),
    }
}

/// Screen coordinate of an axis-aligned mapped guide. The locked NF2 Guide painter has no
/// oblique state, so rotated/skewed local guides are withheld instead of being drawn incorrectly.
fn guide_screen(
    axis: GuideAxis,
    position: f64,
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
) -> Option<f32> {
    let screen_orientation = screen_orientation(axis, mapping)?;
    let point = match axis {
        GuideAxis::Horizontal => Pos2::new(0., position as f32),
        GuideAxis::Vertical => Pos2::new(position as f32, 0.),
    };
    let screen = mapping.edit_to_screen(origin, zoom, point);
    Some(match screen_orientation {
        Orientation::Horizontal => screen.y,
        Orientation::Vertical => screen.x,
    })
}

/// Timeline position under `pointer` along a guide's authored axis.
fn guide_position(
    axis: GuideAxis,
    pointer: Pos2,
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
) -> f64 {
    let local = mapping.screen_to_edit(origin, zoom.max(1e-3), pointer);
    f64::from(match axis {
        GuideAxis::Horizontal => local.y,
        GuideAxis::Vertical => local.x,
    })
}

impl Editor {
    /// Guides of the timeline being edited.
    pub(crate) fn guides(&self) -> &[Guide] {
        &self.symbol().timeline.guides
    }

    fn selected_guide_index(&self) -> Option<usize> {
        self.stage_view
            .selected_guide
            .as_ref()
            .filter(|(symbol, index)| symbol == self.symbol_id() && *index < self.guides().len())
            .map(|(_, index)| *index)
    }

    /// Guides can be selected and dragged: shown, unlocked, with a selection tool.
    fn guides_interactive(&self) -> bool {
        let v = &self.stage_view.prefs;
        v.guides_show
            && !v.guides_lock
            && matches!(self.tool, Tool::Select | Tool::Transform | Tool::Subselect)
    }

    /// Snap targets for the current zoom, or `None` when no snapping is on (or `⌘` suspends it).
    #[cfg(test)]
    pub(crate) fn snapper(&self) -> Option<Snapper> {
        self.snapper_for(StageMapping::identity())
    }

    pub(crate) fn snapper_for(&self, mapping: StageMapping) -> Option<Snapper> {
        if self.stage_view.command_suspend {
            return None;
        }
        let v = &self.stage_view.prefs;
        let zoom = self.zoom.max(1e-3) as f64;
        let [a, b, c, d, _, _] = mapping.matrix();
        let axis_scale = [a.hypot(b).max(1e-12), c.hypot(d).max(1e-12)];
        let tolerance = |screen: f64| {
            [
                screen / (zoom * axis_scale[0]),
                screen / (zoom * axis_scale[1]),
            ]
        };
        let normal_tol = tolerance(SnapAccuracy::Normal.screen_tolerance() as f64);
        let object_tol = tolerance(f64::from(
            self.workspace.drawing.snap_tolerance.clamp(1.0, 32.0),
        ));
        let mut snapper = Snapper {
            guide_tolerance: tolerance(v.guide_accuracy.screen_tolerance() as f64),
            object_tolerance: object_tol,
            ..Default::default()
        };
        if axis_mapping(mapping).is_some() && v.guides_show && v.guides_snap {
            for g in self.guides() {
                match g.axis {
                    GuideAxis::Horizontal => snapper.guides_y.push(g.position),
                    GuideAxis::Vertical => snapper.guides_x.push(g.position),
                }
            }
        }
        if v.objects_snap {
            self.collect_object_snap_lines(&mut snapper.objects_x, &mut snapper.objects_y);
        }
        if axis_mapping(mapping).is_some() && v.grid_snap {
            snapper.grid = Some((
                v.grid_spacing,
                tolerance(v.grid_accuracy.screen_tolerance() as f64),
            ));
        }
        if v.pixels_snap {
            snapper.pixel_tolerance = Some(normal_tol.map(|value| value.min(0.5)));
        }
        (!snapper.is_empty()).then_some(snapper)
    }

    fn collect_object_snap_lines(&self, out_x: &mut Vec<f64>, out_y: &mut Vec<f64>) {
        use neoflash_core::ElementKind;
        let eligible: std::collections::HashSet<Id> = self
            .editable_targets()
            .into_iter()
            .filter(|t| !self.selection.targets.contains(t))
            .map(|t| t.element)
            .collect();
        if eligible.is_empty() {
            return;
        }
        let mut boxes: Vec<(Id, [f64; 4])> = Vec::new();
        for item in neoflash_core::evaluate(&self.project, self.symbol_id(), self.frame) {
            let Some(top_id) = item.instance_path.first() else {
                continue;
            };
            if !eligible.contains(top_id) {
                continue;
            }
            let local = match &item.kind {
                ElementKind::Rectangle { width, height, .. }
                | ElementKind::Ellipse { width, height, .. }
                | ElementKind::TextBox { width, height, .. }
                | ElementKind::Video { width, height } => Some([0.0, 0.0, *width, *height]),
                ElementKind::Bitmap { width, height, .. } => {
                    Some([0.0, 0.0, f64::from(*width), f64::from(*height)])
                }
                ElementKind::Text {
                    text, font_size, ..
                } => Some([
                    0.0,
                    0.0,
                    (text.chars().count() as f64) * (*font_size) * 0.6,
                    *font_size,
                ]),
                ElementKind::Path {
                    commands,
                    stroke_width,
                    ..
                }
                | ElementKind::PaintedPath {
                    commands,
                    stroke_width,
                    ..
                } => neoflash_core::path_bounds(commands)
                    .ok()
                    .flatten()
                    .map(|b| {
                        let pad = stroke_width.max(0.0) * 0.5;
                        [b.x_min - pad, b.y_min - pad, b.x_max + pad, b.y_max + pad]
                    }),
                ElementKind::Instance { .. } => None,
            };
            let Some([lx0, ly0, lx1, ly1]) = local else {
                continue;
            };
            let m = item.transform;
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for (lx, ly) in [(lx0, ly0), (lx1, ly0), (lx1, ly1), (lx0, ly1)] {
                let wx = m[0] * lx + m[2] * ly + m[4];
                let wy = m[1] * lx + m[3] * ly + m[5];
                if wx.is_finite() && wy.is_finite() {
                    min_x = min_x.min(wx);
                    min_y = min_y.min(wy);
                    max_x = max_x.max(wx);
                    max_y = max_y.max(wy);
                }
            }
            if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
                continue;
            }
            if let Some((_, b)) = boxes.iter_mut().find(|(id, _)| id == top_id) {
                b[0] = b[0].min(min_x);
                b[1] = b[1].min(min_y);
                b[2] = b[2].max(max_x);
                b[3] = b[3].max(max_y);
            } else {
                boxes.push((top_id.clone(), [min_x, min_y, max_x, max_y]));
            }
        }
        for (_, [min_x, min_y, max_x, max_y]) in boxes {
            out_x.extend([min_x, (min_x + max_x) * 0.5, max_x]);
            out_y.extend([min_y, (min_y + max_y) * 0.5, max_y]);
        }
    }

    /// Snap a point dragged from `press` to `pos` (screen) whose stage position at the press was
    /// `anchor`. Returns the pointer position that puts the anchor on the snapped line(s) and shows
    /// the snap ring there. Without snapping returns `pos` unchanged.
    pub(crate) fn snap_drag(
        &mut self,
        mapping: StageMapping,
        origin: Pos2,
        press: Pos2,
        pos: Pos2,
        anchor: Pos2,
    ) -> Pos2 {
        let Some(snapper) = self.snapper_for(mapping) else {
            self.stage_view.snap_ring = None;
            return pos;
        };
        let zoom = self.zoom.max(1e-3);
        let press_local = mapping.screen_to_edit(origin, zoom, press);
        let current_local = mapping.screen_to_edit(origin, zoom, pos);
        let moving = anchor + (current_local - press_local);
        let snapped = snapper.point([moving.x as f64, moving.y as f64]);
        let target = Pos2::new(snapped.point[0] as f32, snapped.point[1] as f32);
        let adjusted = pos
            + (mapping.edit_to_screen(origin, zoom, target)
                - mapping.edit_to_screen(origin, zoom, moving));
        self.stage_view.snap_ring =
            Some((mapping.edit_to_screen(origin, zoom, target), snapped.any()));
        adjusted
    }

    /// Snap a pointer position itself (drawing tools: shape corners, line ends, pen anchors).
    pub(crate) fn snap_screen(&mut self, mapping: StageMapping, origin: Pos2, pos: Pos2) -> Pos2 {
        let anchor = mapping.screen_to_edit(origin, self.zoom.max(1e-3), pos);
        self.snap_drag(mapping, origin, pos, pos, anchor)
    }

    /// Snap a moving selection: `bounds` (stage) at the press, pointer moved by `delta` (stage).
    /// Returns the corrected delta; the ring sits on the snapped edge (or the grabbed point).
    pub(crate) fn snap_move(
        &mut self,
        mapping: StageMapping,
        origin: Pos2,
        bounds: Rect,
        grab: Pos2,
        delta: [f64; 2],
        lock: [bool; 2],
    ) -> [f64; 2] {
        let Some(snapper) = self.snapper_for(mapping).filter(|_| bounds.is_finite()) else {
            self.stage_view.snap_ring = None;
            return delta;
        };
        let moved = bounds.translate(Vec2::new(delta[0] as f32, delta[1] as f32));
        let xs = [moved.left(), moved.center().x, moved.right()].map(|v| v as f64);
        let ys = [moved.top(), moved.center().y, moved.bottom()].map(|v| v as f64);
        let (offset, which) = snapper.offset(&xs, &ys);
        let offset = [
            if lock[0] { 0. } else { offset[0] },
            if lock[1] { 0. } else { offset[1] },
        ];
        let snapped = [which[0].filter(|_| !lock[0]), which[1].filter(|_| !lock[1])];
        let result = [delta[0] + offset[0], delta[1] + offset[1]];
        let grabbed = grab + Vec2::new(result[0] as f32, result[1] as f32);
        let ring = Pos2::new(
            snapped[0].map_or(grabbed.x, |i| (xs[i] + offset[0]) as f32),
            snapped[1].map_or(grabbed.y, |i| (ys[i] + offset[1]) as f32),
        );
        self.stage_view.snap_ring = Some((
            mapping.edit_to_screen(origin, self.zoom, ring),
            snapped.iter().any(Option::is_some),
        ));
        result
    }

    /// Whether a stage gesture that snaps is in progress (the snap ring is only shown then).
    fn snapping_gesture(&self) -> bool {
        self.object_gesture.is_some()
            || self.shape_draft.is_some()
            || self.authoring_tools.is_drawing()
            || self.vector.point_drag.is_some()
            || self
                .vector
                .draft
                .as_ref()
                .is_some_and(|draft| draft.dragging)
    }

    // ── Document edits (one Undo step each) ──

    pub(crate) fn add_guide(&mut self, axis: GuideAxis, position: f64) {
        if !position.is_finite() || position.abs() > Guide::MAX_POSITION {
            return;
        }
        if self.guides().len() >= Guide::LIMIT {
            self.message(format!(
                "A timeline holds at most {} guides. Clear guides before adding more.",
                Guide::LIMIT
            ));
            return;
        }
        self.transact(|e| {
            e.project.format_version = e.project.format_version.max(Guide::FORMAT_VERSION);
            e.symbol_mut()
                .timeline
                .guides
                .push(Guide { axis, position });
        });
        let index = self.guides().len() - 1;
        self.stage_view.selected_guide = Some((self.symbol_id().to_owned(), index));
        self.stage_view.prefs.guides_show = true;
    }

    pub(crate) fn remove_guide(&mut self, index: usize) {
        if index >= self.guides().len() {
            return;
        }
        self.transact(|e| {
            e.symbol_mut().timeline.guides.remove(index);
        });
        self.stage_view.selected_guide = None;
    }

    /// View settings ▸ Clear guides: removes every guide of the current timeline in one Undo step.
    pub(crate) fn clear_guides(&mut self) {
        self.cancel_guide_drag();
        if self.guides().is_empty() {
            return;
        }
        self.transact(|e| e.symbol_mut().timeline.guides.clear());
        self.stage_view.selected_guide = None;
        self.message("Guides cleared");
    }

    /// Abandon a guide drag (Escape, tool or timeline change). A moved guide returns.
    pub(crate) fn cancel_guide_drag(&mut self) {
        self.stage_view.zero_drag = false;
        if let Some(drag) = self.stage_view.drag.take()
            && drag.began
        {
            self.history.cancel(&mut self.project);
        }
    }

    fn finish_guide_drag(&mut self) {
        let Some(drag) = self.stage_view.drag.take() else {
            return;
        };
        if drag.symbol != self.symbol_id() {
            if drag.began {
                self.history.cancel(&mut self.project);
            }
            return;
        }
        match drag.index {
            None if !drag.remove => self.add_guide(drag.axis, drag.position),
            None => {}
            Some(index) if drag.remove => {
                if drag.began {
                    self.history.cancel(&mut self.project);
                }
                self.remove_guide(index);
            }
            Some(_) if drag.began => {
                self.commit_history();
            }
            Some(_) => {}
        }
    }

    /// Guide position for a pointer: whole pixels, or the nearest grid line when snapping to grid.
    fn dragged_guide_position(
        &self,
        axis: GuideAxis,
        pointer: Pos2,
        mapping: StageMapping,
        origin: Pos2,
    ) -> f64 {
        let raw = guide_position(axis, pointer, mapping, origin, self.zoom);
        let v = &self.stage_view.prefs;
        if v.grid_snap {
            let grid = Snapper {
                grid: Some((
                    v.grid_spacing,
                    [
                        v.grid_accuracy.screen_tolerance() as f64
                            / (self.zoom.max(1e-3) as f64
                                * mapping.matrix()[0].hypot(mapping.matrix()[1]).max(1e-12)),
                        v.grid_accuracy.screen_tolerance() as f64
                            / (self.zoom.max(1e-3) as f64
                                * mapping.matrix()[2].hypot(mapping.matrix()[3]).max(1e-12)),
                    ],
                )),
                ..Default::default()
            };
            let snapped = match axis {
                GuideAxis::Horizontal => grid.snap_y(raw),
                GuideAxis::Vertical => grid.snap_x(raw),
            };
            if let Some(snapped) = snapped {
                return snapped;
            }
        }
        raw.round().clamp(-Guide::MAX_POSITION, Guide::MAX_POSITION)
    }

    fn update_guide_drag(
        &mut self,
        pointer: Pos2,
        canvas: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        let Some(mut drag) = self.stage_view.drag.take() else {
            return;
        };
        let position = self.dragged_guide_position(drag.axis, pointer, mapping, origin);
        let layout = RulerLayout::new(canvas);
        drag.remove =
            !canvas.contains(pointer) || (self.stage_view.prefs.rulers && layout.contains(pointer));
        if let Some(index) = drag.index
            && position != drag.position
        {
            if !drag.began {
                self.finish_edits();
                self.history.begin(&self.project);
                drag.began = true;
            }
            if let Some(guide) = self.symbol_mut().timeline.guides.get_mut(index) {
                guide.position = position;
            }
        }
        drag.position = position;
        self.stage_view.drag = Some(drag);
    }

    /// Before the stage tools run: keep guide/corner drags going, and take presses on guides out of
    /// the frame's events so tools do not also start a gesture there.
    pub(crate) fn stage_view_input(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        self.stage_view.command_suspend = ui.input(|i| i.modifiers.command);
        if self
            .stage_view
            .drag
            .as_ref()
            .is_some_and(|d| d.symbol != self.symbol_id())
        {
            self.cancel_guide_drag();
        }
        if !self.stage_view.prefs.guides_show || self.stage_view.prefs.guides_lock {
            self.stage_view.selected_guide = None;
        }
        if self.selected_guide_index().is_none() {
            self.stage_view.selected_guide = None;
        }
        self.stage_view.canvas = canvas;
        self.stage_view.origin = origin;
        if !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
            self.stage_view.guide_pointer = false;
        }
        let layout = RulerLayout::new(canvas);
        let rulers = self.stage_view.prefs.rulers;
        let interactive = self.guides_interactive();
        let zoom = self.zoom;
        let events = ui.input(|i| i.events.clone());
        let mut taken = None;
        for event in &events {
            match *event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if canvas.contains(pos)
                    && !(rulers && layout.contains(pos))
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    let hit = if interactive {
                        self.guides().iter().enumerate().rev().find_map(|(i, g)| {
                            guide_screen(g.axis, g.position, mapping, origin, zoom)
                                .is_some_and(|screen| {
                                    screen_orientation(g.axis, mapping)
                                        .is_some_and(|axis| cv::guide_hit(axis, screen, pos))
                                })
                                .then_some((i, *g))
                        })
                    } else {
                        None
                    };
                    if let Some((index, guide)) = hit {
                        self.finish_edits();
                        self.stage_view.selected_guide = Some((self.symbol_id().to_owned(), index));
                        self.stage_view.drag = Some(GuideDrag {
                            symbol: self.symbol_id().to_owned(),
                            axis: guide.axis,
                            index: Some(index),
                            position: guide.position,
                            remove: false,
                            began: false,
                        });
                        taken = Some(pos);
                        self.stage_view.guide_pointer = true;
                    } else {
                        self.stage_view.selected_guide = None;
                    }
                }
                egui::Event::PointerMoved(pos) if self.stage_view.drag.is_some() => {
                    self.update_guide_drag(pos, canvas, mapping, origin);
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.stage_view.drag.is_some() => {
                    self.update_guide_drag(pos, canvas, mapping, origin);
                    self.finish_guide_drag();
                }
                _ => {}
            }
        }
        if let Some(pos) = taken {
            ui.ctx().input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(event, egui::Event::PointerButton { pos: p, pressed: true, .. } if *p == pos)
                })
            });
        }
        // Resize cursor over a draggable guide and while dragging one.
        let hover = ui.input(|i| i.pointer.hover_pos());
        let axis = self.stage_view.drag.as_ref().map(|d| d.axis).or_else(|| {
            let pos = hover.filter(|p| canvas.contains(*p) && !(rulers && layout.contains(*p)))?;
            interactive
                .then(|| {
                    self.guides().iter().find(|g| {
                        guide_screen(g.axis, g.position, mapping, origin, zoom).is_some_and(
                            |screen| {
                                screen_orientation(g.axis, mapping)
                                    .is_some_and(|axis| cv::guide_hit(axis, screen, pos))
                            },
                        )
                    })
                })
                .flatten()
                .map(|g| g.axis)
        });
        if let Some(axis) = axis {
            ui.ctx()
                .set_cursor_icon(match screen_orientation(axis, mapping) {
                    Some(Orientation::Horizontal) => egui::CursorIcon::ResizeVertical,
                    Some(Orientation::Vertical) => egui::CursorIcon::ResizeHorizontal,
                    None => return,
                });
        }
        if !self.snapping_gesture() {
            self.stage_view.snap_ring = None;
        }
    }

    // ── Painting ──

    /// `NF2/Stage Grid` over the stage page, aligned to the timeline origin (scene top-left or the
    /// symbol registration point); every 5th line from the origin is stronger.
    pub(crate) fn paint_stage_grid(
        &self,
        painter: &egui::Painter,
        page: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        let v = &self.stage_view.prefs;
        let Some(axis_mapping) = axis_mapping(mapping) else {
            return;
        };
        if !v.grid_show {
            return;
        }
        let dynamic = v.grid_mode == GridMode::Dynamic;
        let grid_color = if dynamic {
            let palette = crate::theme::UiPalette::current(painter.ctx());
            let background = palette
                .pasteboard
                .blend(palette.stage_shadow)
                .blend(color(self.project.background));
            cv::dynamic_grid_color(background)
        } else {
            v.grid_color(neoflash_ui::palette(painter.ctx()))
        };
        let [a, b, c, d, _, _] = mapping.matrix();
        let spacing = match axis_mapping {
            AxisMapping::Direct => Vec2::new(
                v.grid_spacing[0] as f32 * a.abs() as f32,
                v.grid_spacing[1] as f32 * d.abs() as f32,
            ),
            AxisMapping::Swapped => Vec2::new(
                v.grid_spacing[1] as f32 * c.abs() as f32,
                v.grid_spacing[0] as f32 * b.abs() as f32,
            ),
        };
        cv::paint_stage_grid_with_options(
            painter,
            page,
            cv::StageGridOptions {
                origin: mapping.edit_to_screen(origin, self.zoom, Pos2::ZERO),
                zoom: self.zoom,
                spacing,
                color: grid_color,
                adaptive: dynamic,
            },
        );
    }

    /// `NF2/Guide` lines of the current timeline (and a guide being dragged out of a ruler).
    pub(crate) fn paint_stage_guides(
        &self,
        painter: &egui::Painter,
        canvas: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        let v = &self.stage_view.prefs;
        let drag = self.stage_view.drag.as_ref();
        let ctx = painter.ctx();
        let custom = v.guide_color.map(color);
        if v.guides_show {
            let selected = self.selected_guide_index();
            for (i, g) in self.guides().iter().enumerate() {
                if drag.is_some_and(|d| d.index == Some(i) && d.remove) {
                    continue;
                }
                let state = if v.guides_lock {
                    GuideState::Locked
                } else if selected == Some(i) {
                    GuideState::Selected
                } else {
                    GuideState::Default
                };
                let Some(o) = screen_orientation(g.axis, mapping) else {
                    continue;
                };
                let Some(screen) = guide_screen(g.axis, g.position, mapping, origin, self.zoom)
                else {
                    continue;
                };
                cv::paint_guide(
                    painter,
                    ctx,
                    o,
                    screen.round() + 0.5,
                    cv::guide_extent(canvas, o, v.rulers),
                    state,
                    custom,
                );
            }
        }
        if let Some(d) = drag.filter(|d| d.index.is_none() && !d.remove) {
            let Some(o) = screen_orientation(d.axis, mapping) else {
                return;
            };
            let Some(screen) = guide_screen(d.axis, d.position, mapping, origin, self.zoom) else {
                return;
            };
            cv::paint_guide(
                painter,
                ctx,
                o,
                screen.round() + 0.5,
                cv::guide_extent(canvas, o, v.rulers),
                GuideState::Selected,
                custom,
            );
        }
    }

    /// `NF2/Snap Ring` while a snapping gesture is in progress.
    pub(crate) fn paint_snap_ring(&self, painter: &egui::Painter) {
        if let Some((at, snapped)) = self.stage_view.snap_ring
            && self.snapping_gesture()
        {
            cv::paint_snap_ring(painter, painter.ctx(), at, snapped);
        }
    }

    /// Rulers and corner: their own layer above the stage. Dragging out of a ruler creates a guide,
    /// the corner moves the zero point (double-click resets it).
    pub(crate) fn stage_rulers(
        &mut self,
        ctx: &egui::Context,
        canvas: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        if !self.stage_view.prefs.rulers || canvas.width() < 60. || canvas.height() < 60. {
            self.stage_view.zero_drag = false;
            return;
        }
        let layout = RulerLayout::new(canvas);
        let zoom = self.zoom;
        let guide_axis_kind = axis_mapping(mapping);
        let mapped_guides = guide_axis_kind.is_some();
        let mapped_axis_kind = ruler_axis_mapping(mapping);
        let mapped_rulers = mapped_axis_kind.is_some();
        let [a, b, c, d, _, _] = mapping.matrix();
        let registration = if mapped_rulers {
            mapping.edit_to_screen(origin, zoom, Pos2::ZERO)
        } else {
            origin
        };
        let (horizontal_zoom, vertical_zoom) = match mapped_axis_kind {
            Some(AxisMapping::Direct) => (zoom * a.abs() as f32, zoom * d.abs() as f32),
            Some(AxisMapping::Swapped) => (zoom * c.abs() as f32, zoom * b.abs() as f32),
            None => (zoom, zoom),
        };
        let zero = if mapped_rulers {
            mapping.edit_to_screen(
                origin,
                zoom,
                Pos2::new(
                    self.stage_view.zero[0] as f32,
                    self.stage_view.zero[1] as f32,
                ),
            )
        } else {
            Pos2::new(
                registration.x + self.stage_view.zero[0] as f32 * horizontal_zoom,
                registration.y + self.stage_view.zero[1] as f32 * vertical_zoom,
            )
        };
        let v = self.stage_view.prefs.clone();
        let unit = v.units.pixels();
        let every = (!v.labels_auto).then_some(v.label_every as f32 / unit);
        let strip = |id: &str, rect: Rect, paint: &dyn Fn(&egui::Painter, Rect)| {
            egui::Area::new(egui::Id::new(id))
                .order(egui::Order::Middle)
                .fixed_pos(rect.min)
                .constrain(false)
                .show(ctx, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
                    paint(ui.painter(), rect);
                    response
                })
                .inner
        };
        let horizontal = strip("stage-ruler-horizontal", layout.horizontal, &|p, r| {
            cv::paint_stage_ruler_units(
                p,
                ctx,
                r,
                Orientation::Horizontal,
                zero.x,
                horizontal_zoom,
                unit,
                every,
            )
        });
        let vertical = strip("stage-ruler-vertical", layout.vertical, &|p, r| {
            cv::paint_stage_ruler_units(
                p,
                ctx,
                r,
                Orientation::Vertical,
                zero.y,
                vertical_zoom,
                unit,
                every,
            )
        });
        let corner = strip("stage-ruler-corner", layout.corner, &|p, r| {
            cv::paint_ruler_corner(p, ctx, r)
        });
        let (horizontal_guide, vertical_guide) = match guide_axis_kind {
            Some(AxisMapping::Direct) => (GuideAxis::Horizontal, GuideAxis::Vertical),
            Some(AxisMapping::Swapped) => (GuideAxis::Vertical, GuideAxis::Horizontal),
            None => (GuideAxis::Horizontal, GuideAxis::Vertical),
        };
        for (response, axis) in [(&horizontal, horizontal_guide), (&vertical, vertical_guide)] {
            if response.drag_started() && self.stage_view.drag.is_none() && mapped_guides {
                self.finish_edits();
                let pointer = response.interact_pointer_pos().unwrap_or(canvas.center());
                self.stage_view.drag = Some(GuideDrag {
                    symbol: self.symbol_id().to_owned(),
                    axis,
                    index: None,
                    position: self.dragged_guide_position(axis, pointer, mapping, origin),
                    remove: true,
                    began: false,
                });
            }
        }
        horizontal.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        vertical.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if corner.double_clicked() {
            self.stage_view.zero = [0., 0.];
        } else if corner.drag_started() {
            self.stage_view.zero_drag = true;
        }
        if self.stage_view.zero_drag {
            if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                self.stage_view.zero = if mapped_rulers {
                    let local = mapping.screen_to_edit(origin, zoom, pos);
                    [f64::from(local.x.round()), f64::from(local.y.round())]
                } else {
                    [
                        ((pos.x - origin.x) / zoom.max(1e-3)).round() as f64,
                        ((pos.y - origin.y) / zoom.max(1e-3)).round() as f64,
                    ]
                };
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.stage_view.zero_drag = false;
            }
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        corner.on_hover_text("Drag to move the ruler origin · double-click to reset");
    }

    // ── Commands (View menu, zoom control, shortcuts) ──

    pub(crate) fn open_view_settings(&mut self) {
        self.stage_view.settings_open = true;
    }

    pub(crate) fn toggle_rulers(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.rulers = !v.rulers;
    }
    pub(crate) fn toggle_guides(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.guides_show = !v.guides_show;
    }
    pub(crate) fn toggle_guide_lock(&mut self) {
        self.cancel_guide_drag();
        let v = &mut self.stage_view.prefs;
        v.guides_lock = !v.guides_lock;
    }
    pub(crate) fn toggle_guide_snap(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.guides_snap = !v.guides_snap;
    }
    pub(crate) fn toggle_grid(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.grid_show = !v.grid_show;
    }
    pub(crate) fn toggle_grid_snap(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.grid_snap = !v.grid_snap;
    }
    pub(crate) fn toggle_grid_over_objects(&mut self) {
        let v = &mut self.stage_view.prefs;
        v.grid_over_objects = !v.grid_over_objects;
    }
    pub(crate) fn toggle_object_snap(&mut self) {
        self.stage_view.toggle_object_snap();
    }
    #[allow(dead_code)]
    pub(crate) fn toggle_pixel_snap(&mut self) {
        self.stage_view.toggle_pixel_snap();
    }

    /// Shortcuts of View ▸ Rulers / Grid / Guides (design §8, spec ST-01…ST-03), skipped while typing.
    /// Delete removes the selected guide; Escape deselects it.
    pub(crate) fn stage_view_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::commands::CommandId;
        use egui::{Key, Modifiers as M};
        if ctx.wants_keyboard_input() {
            return;
        }
        if self.take_command_shortcut(ctx, CommandId::ViewRulers) {
            self.toggle_rulers();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGuidesSnap) {
            self.toggle_guide_snap();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGuidesLock) {
            self.toggle_guide_lock();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGuidesShow) {
            self.toggle_guides();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGridSnap) {
            self.toggle_grid_snap();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGridShow) {
            self.toggle_grid();
        }
        if self.take_command_shortcut(ctx, CommandId::ViewGridEdit) {
            self.open_view_settings();
        }
        if let Some(index) = self.selected_guide_index() {
            if self.take_command_shortcut(ctx, CommandId::EditClear) {
                self.remove_guide(index);
            } else if ctx.input_mut(|i| i.consume_key(M::NONE, Key::Escape)) {
                self.stage_view.selected_guide = None;
            }
        }
        if self.stage_view.drag.is_some() && ctx.input_mut(|i| i.consume_key(M::NONE, Key::Escape))
        {
            self.cancel_guide_drag();
        }
    }
}

#[cfg(test)]
mod affine_ruler_tests {
    use super::*;

    fn mapping(matrix: [f64; 6]) -> StageMapping {
        StageMapping::try_new(matrix).unwrap()
    }

    #[test]
    fn signed_or_oblique_axes_fall_back_instead_of_reversing_ruler_labels() {
        assert_eq!(
            ruler_axis_mapping(mapping([2., 0., 0., 3., 0., 0.])),
            Some(AxisMapping::Direct)
        );
        assert_eq!(
            ruler_axis_mapping(mapping([0., 2., 3., 0., 0., 0.])),
            Some(AxisMapping::Swapped)
        );
        assert_eq!(
            ruler_axis_mapping(mapping([0., 2., -3., 0., 0., 0.])),
            None,
            "a quarter turn reverses one local ruler axis"
        );
        assert_eq!(
            ruler_axis_mapping(mapping([-2., 0., 0., 3., 0., 0.])),
            None,
            "a reflection must not paint increasing labels backwards"
        );
        assert_eq!(
            ruler_axis_mapping(mapping([1., 0.25, 0., 1., 0., 0.])),
            None,
            "oblique axes need the R-063 ruler state"
        );
    }
}
