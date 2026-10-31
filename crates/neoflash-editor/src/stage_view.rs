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

/// Rulers, guides and grid settings. Colours are `None` while they equal the theme token, so the
/// default guide and grid colours follow Light/Dark.
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
    /// Grid spacing (horizontal, vertical) in stage pixels.
    pub grid_spacing: [f64; 2],
    pub grid_linked: bool,
    pub grid_color: Option<[u8; 4]>,
    pub grid_accuracy: SnapAccuracy,
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
            grid_spacing: [20., 20.],
            grid_linked: true,
            grid_color: None,
            grid_accuracy: SnapAccuracy::Normal,
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

    pub fn grid_color(&self, p: &Palette) -> egui::Color32 {
        self.grid_color
            .map_or_else(|| vs::default_grid_color(p), color)
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
            grid_w: self.grid_spacing[0],
            grid_h: self.grid_spacing[1],
            grid_linked: self.grid_linked,
            grid_color: self.grid_color(p),
            grid_snap_accuracy: self.grid_accuracy.label(),
        }
    }

    /// Take the popover's edits back. Colours equal to the theme default stay `None`.
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
        self.grid_spacing = [s.grid_w, s.grid_h];
        self.grid_linked = s.grid_linked;
        self.grid_color = (s.grid_color != vs::default_grid_color(p)).then(|| rgba(s.grid_color));
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
            canvas: Rect::NOTHING,
            origin: Pos2::ZERO,
        }
    }
}

fn orientation(axis: GuideAxis) -> Orientation {
    match axis {
        GuideAxis::Horizontal => Orientation::Horizontal,
        GuideAxis::Vertical => Orientation::Vertical,
    }
}

/// Screen coordinate of a timeline position along a guide's axis (y for horizontal guides).
fn guide_screen(axis: GuideAxis, position: f64, origin: Pos2, zoom: f32) -> f32 {
    match axis {
        GuideAxis::Horizontal => origin.y + position as f32 * zoom,
        GuideAxis::Vertical => origin.x + position as f32 * zoom,
    }
}

/// Timeline position under `pointer` along a guide's axis.
fn guide_position(axis: GuideAxis, pointer: Pos2, origin: Pos2, zoom: f32) -> f64 {
    let zoom = zoom.max(1e-3) as f64;
    match axis {
        GuideAxis::Horizontal => (pointer.y - origin.y) as f64 / zoom,
        GuideAxis::Vertical => (pointer.x - origin.x) as f64 / zoom,
    }
}

/// Remove a matching key press (logical or physical key, exact modifiers). Option and Shift change
/// the logical key on macOS (⌥R = ®, ⇧; = :), so the physical key is checked too.
fn take_shortcut(ctx: &egui::Context, modifiers: egui::Modifiers, keys: &[egui::Key]) -> bool {
    ctx.input_mut(|i| {
        let before = i.events.len();
        let mut found = false;
        i.events.retain(|event| {
            let hit = !found
                && matches!(event, egui::Event::Key {
                    key, physical_key, pressed: true, modifiers: m, ..
                } if m.matches_exact(modifiers)
                    && (keys.contains(key) || physical_key.is_some_and(|k| keys.contains(&k))));
            found |= hit;
            !hit
        });
        i.events.len() != before
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

    /// Snap targets for the current zoom, or `None` when no snapping is on.
    pub(crate) fn snapper(&self) -> Option<Snapper> {
        let v = &self.stage_view.prefs;
        let zoom = self.zoom.max(1e-3) as f64;
        let mut snapper = Snapper {
            guide_tolerance: v.guide_accuracy.screen_tolerance() as f64 / zoom,
            ..Default::default()
        };
        if v.guides_show && v.guides_snap {
            for g in self.guides() {
                match g.axis {
                    GuideAxis::Horizontal => snapper.guides_y.push(g.position),
                    GuideAxis::Vertical => snapper.guides_x.push(g.position),
                }
            }
        }
        if v.grid_snap {
            snapper.grid = Some((
                v.grid_spacing,
                v.grid_accuracy.screen_tolerance() as f64 / zoom,
            ));
        }
        (!snapper.is_empty()).then_some(snapper)
    }

    /// Snap a point dragged from `press` to `pos` (screen) whose stage position at the press was
    /// `anchor`. Returns the pointer position that puts the anchor on the snapped line(s) and shows
    /// the snap ring there. Without snapping returns `pos` unchanged.
    pub(crate) fn snap_drag(&mut self, origin: Pos2, press: Pos2, pos: Pos2, anchor: Pos2) -> Pos2 {
        let Some(snapper) = self.snapper() else {
            self.stage_view.snap_ring = None;
            return pos;
        };
        let zoom = self.zoom.max(1e-3);
        let moving = anchor + (pos - press) / zoom;
        let snapped = snapper.point([moving.x as f64, moving.y as f64]);
        let target = Pos2::new(snapped.point[0] as f32, snapped.point[1] as f32);
        let adjusted = pos + (target - moving) * zoom;
        self.stage_view.snap_ring = Some((origin + target.to_vec2() * zoom, snapped.any()));
        adjusted
    }

    /// Snap a pointer position itself (drawing tools: shape corners, line ends, pen anchors).
    pub(crate) fn snap_screen(&mut self, origin: Pos2, pos: Pos2) -> Pos2 {
        let anchor = Pos2::ZERO + (pos - origin) / self.zoom.max(1e-3);
        self.snap_drag(origin, pos, pos, anchor)
    }

    /// Snap a moving selection: `bounds` (stage) at the press, pointer moved by `delta` (stage).
    /// Returns the corrected delta; the ring sits on the snapped edge (or the grabbed point).
    pub(crate) fn snap_move(
        &mut self,
        origin: Pos2,
        bounds: Rect,
        grab: Pos2,
        delta: [f64; 2],
        lock: [bool; 2],
    ) -> [f64; 2] {
        let Some(snapper) = self.snapper().filter(|_| bounds.is_finite()) else {
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
            origin + ring.to_vec2() * self.zoom,
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
                self.history.commit(&self.project);
            }
            Some(_) => {}
        }
    }

    /// Guide position for a pointer: whole pixels, or the nearest grid line when snapping to grid.
    fn dragged_guide_position(&self, axis: GuideAxis, pointer: Pos2, origin: Pos2) -> f64 {
        let raw = guide_position(axis, pointer, origin, self.zoom);
        let v = &self.stage_view.prefs;
        if v.grid_snap {
            let grid = Snapper {
                grid: Some((
                    v.grid_spacing,
                    v.grid_accuracy.screen_tolerance() as f64 / self.zoom.max(1e-3) as f64,
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

    fn update_guide_drag(&mut self, pointer: Pos2, canvas: Rect, origin: Pos2) {
        let Some(mut drag) = self.stage_view.drag.take() else {
            return;
        };
        let position = self.dragged_guide_position(drag.axis, pointer, origin);
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
    pub(crate) fn stage_view_input(&mut self, ui: &egui::Ui, canvas: Rect, origin: Pos2) {
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
                            cv::guide_hit(
                                orientation(g.axis),
                                guide_screen(g.axis, g.position, origin, zoom),
                                pos,
                            )
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
                    self.update_guide_drag(pos, canvas, origin);
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.stage_view.drag.is_some() => {
                    self.update_guide_drag(pos, canvas, origin);
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
                        cv::guide_hit(
                            orientation(g.axis),
                            guide_screen(g.axis, g.position, origin, zoom),
                            pos,
                        )
                    })
                })
                .flatten()
                .map(|g| g.axis)
        });
        if let Some(axis) = axis {
            ui.ctx().set_cursor_icon(match axis {
                GuideAxis::Horizontal => egui::CursorIcon::ResizeVertical,
                GuideAxis::Vertical => egui::CursorIcon::ResizeHorizontal,
            });
        }
        if !self.snapping_gesture() {
            self.stage_view.snap_ring = None;
        }
    }

    // ── Painting ──

    /// `NF2/Stage Grid` over the stage page, aligned to the timeline origin (scene top-left or the
    /// symbol registration point); every 5th line from the origin is stronger.
    pub(crate) fn paint_stage_grid(&self, painter: &egui::Painter, page: Rect, origin: Pos2) {
        let v = &self.stage_view.prefs;
        if !v.grid_show {
            return;
        }
        let zoom = self.zoom;
        let step = Vec2::new(
            v.grid_spacing[0] as f32 * zoom * 5.,
            v.grid_spacing[1] as f32 * zoom * 5.,
        );
        if !(step.x > 0. && step.y > 0.) {
            return;
        }
        let back = Vec2::new(
            ((origin.x - page.left()) / step.x).ceil().max(0.) * step.x,
            ((origin.y - page.top()) / step.y).ceil().max(0.) * step.y,
        );
        let grid_page = Rect::from_min_max(origin - back, page.max);
        let painter = painter.with_clip_rect(page.intersect(painter.clip_rect()));
        cv::paint_stage_grid(
            &painter,
            painter.ctx(),
            grid_page,
            zoom,
            Vec2::new(v.grid_spacing[0] as f32, v.grid_spacing[1] as f32),
            v.grid_color.map(color),
        );
    }

    /// `NF2/Guide` lines of the current timeline (and a guide being dragged out of a ruler).
    pub(crate) fn paint_stage_guides(&self, painter: &egui::Painter, canvas: Rect, origin: Pos2) {
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
                let o = orientation(g.axis);
                cv::paint_guide(
                    painter,
                    ctx,
                    o,
                    guide_screen(g.axis, g.position, origin, self.zoom).round() + 0.5,
                    cv::guide_extent(canvas, o, v.rulers),
                    state,
                    custom,
                );
            }
        }
        if let Some(d) = drag.filter(|d| d.index.is_none() && !d.remove) {
            let o = orientation(d.axis);
            cv::paint_guide(
                painter,
                ctx,
                o,
                guide_screen(d.axis, d.position, origin, self.zoom).round() + 0.5,
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
    pub(crate) fn stage_rulers(&mut self, ctx: &egui::Context, canvas: Rect, origin: Pos2) {
        if !self.stage_view.prefs.rulers || canvas.width() < 60. || canvas.height() < 60. {
            self.stage_view.zero_drag = false;
            return;
        }
        let layout = RulerLayout::new(canvas);
        let zoom = self.zoom;
        let v = self.stage_view.prefs.clone();
        let unit = v.units.pixels();
        let every = (!v.labels_auto).then_some(v.label_every as f32 / unit);
        let zero = Pos2::new(
            origin.x + self.stage_view.zero[0] as f32 * zoom,
            origin.y + self.stage_view.zero[1] as f32 * zoom,
        );
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
                zoom,
                unit,
                every,
            )
        });
        let vertical = strip("stage-ruler-vertical", layout.vertical, &|p, r| {
            cv::paint_stage_ruler_units(p, ctx, r, Orientation::Vertical, zero.y, zoom, unit, every)
        });
        let corner = strip("stage-ruler-corner", layout.corner, &|p, r| {
            cv::paint_ruler_corner(p, ctx, r)
        });
        for (response, axis) in [
            (&horizontal, GuideAxis::Horizontal),
            (&vertical, GuideAxis::Vertical),
        ] {
            if response.drag_started() && self.stage_view.drag.is_none() {
                self.finish_edits();
                let pointer = response.interact_pointer_pos().unwrap_or(canvas.center());
                self.stage_view.drag = Some(GuideDrag {
                    symbol: self.symbol_id().to_owned(),
                    axis,
                    index: None,
                    position: self.dragged_guide_position(axis, pointer, origin),
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
                self.stage_view.zero = [
                    ((pos.x - origin.x) / zoom.max(1e-3)).round() as f64,
                    ((pos.y - origin.y) / zoom.max(1e-3)).round() as f64,
                ];
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

    /// Shortcuts of View ▸ Rulers / Grid / Guides (design §8, spec ST-01…ST-03), skipped while typing.
    /// Delete removes the selected guide; Escape deselects it.
    pub(crate) fn stage_view_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers as M};
        if ctx.wants_keyboard_input() {
            return;
        }
        let cmd = M::COMMAND;
        let shift = M::COMMAND | M::SHIFT;
        let alt = M::COMMAND | M::ALT;
        let semicolon = [Key::Semicolon, Key::Colon];
        let quote = [Key::Quote];
        if take_shortcut(ctx, M::COMMAND | M::ALT | M::SHIFT, &[Key::R]) {
            self.toggle_rulers();
        }
        if take_shortcut(ctx, shift, &semicolon) {
            self.toggle_guide_snap();
        }
        if take_shortcut(ctx, alt, &semicolon) {
            self.toggle_guide_lock();
        }
        if take_shortcut(ctx, cmd, &semicolon) {
            self.toggle_guides();
        }
        if take_shortcut(ctx, shift, &quote) {
            self.toggle_grid_snap();
        }
        if take_shortcut(ctx, cmd, &quote) {
            self.toggle_grid();
        }
        if take_shortcut(ctx, alt, &[Key::G]) {
            self.open_view_settings();
        }
        if let Some(index) = self.selected_guide_index() {
            if take_shortcut(ctx, M::NONE, &[Key::Delete, Key::Backspace]) {
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
