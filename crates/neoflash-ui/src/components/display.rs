//! Passive components: `NF2/Badge`, `NF2/Kbd`, `NF2/Tooltip`, `NF2/Swatch`, `NF2/Divider`, `NF2/Logo`,
//! `NF2/Spinner`, `NF2/Progress`, `NF2/Toast`, `NF2/Empty State`.
use super::{fill, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{BADGE, KBD};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, Ui, Widget};

/// `Type` of `NF2/Badge`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeType {
    #[default]
    Neutral,
    Accent,
    /// Unsaved changes, pending state.
    Warning,
}

/// `NF2/Badge` — pill with a leading dot ("Unsaved", "HTML5 · Rust / WebAssembly", "1 warning").
pub struct Badge<'a> {
    label: &'a str,
    kind: BadgeType,
}

impl<'a> Badge<'a> {
    pub fn new(label: &'a str, kind: BadgeType) -> Self {
        Self { label, kind }
    }
}

impl Widget for Badge<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (bg, fg) = match self.kind {
            BadgeType::Neutral => (p.bg_hover, p.text_secondary),
            BadgeType::Accent => (p.accent_subtle, p.accent_default),
            BadgeType::Warning => (p.warning_subtle, p.warning_default),
        };
        let g = galley(ui, self.label, TextStyle::Small, fg);
        let w = space::S2 + 6.0 + space::S1 + 2.0 + g.size().x + space::S2;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, BADGE), Sense::hover());
        fill(ui, rect, radius::FULL, bg, None);
        ui.painter().circle_filled(
            egui::pos2(rect.left() + space::S2 + 3.0, rect.center().y),
            3.0,
            fg,
        );
        text_at(
            ui,
            rect.left() + space::S2 + 6.0 + space::S1 + 2.0,
            rect,
            g,
            fg,
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, self.label));
        response
    }
}

/// Shortcut label for the running platform. NF2 writes shortcuts in macOS notation (`⇧⌘S`, `⌥F3`, `↵`);
/// on macOS the string is returned unchanged, elsewhere it becomes words joined by plus signs
/// (`Ctrl+Shift+S`, `Alt+F3`, `Enter`). `NF2/Kbd`, `NF2/Menu Item` and the shortcut chips call it, so a
/// caller never formats per platform.
pub fn platform_shortcut(shortcut: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(target_os = "macos") {
        std::borrow::Cow::Borrowed(shortcut)
    } else {
        shortcut_in_words(shortcut)
    }
}

/// The Windows / Linux spelling of a macOS-notation shortcut (see [`platform_shortcut`]).
pub fn shortcut_in_words(shortcut: &str) -> std::borrow::Cow<'_, str> {
    const GLYPHS: &str = "⌃⌥⇧⌘↵⌤⌫⌦⎋⇥↑↓←→";
    if !shortcut.chars().any(|c| GLYPHS.contains(c)) {
        return std::borrow::Cow::Borrowed(shortcut);
    }
    let (mut ctrl, mut alt, mut shift, mut cmd) = (false, false, false, false);
    let mut key = String::new();
    for c in shortcut.trim().chars() {
        match c {
            '⌃' => ctrl = true,
            '⌥' => alt = true,
            '⇧' => shift = true,
            '⌘' => cmd = true,
            other => key.push(other),
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    if ctrl || cmd {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if ctrl && cmd {
        parts.push("Win");
    }
    let key = match key.trim() {
        "↵" | "⌤" => "Enter",
        "⌫" => "Backspace",
        "⌦" => "Delete",
        "⎋" => "Esc",
        "⇥" => "Tab",
        "↑" => "Up",
        "↓" => "Down",
        "←" => "Left",
        "→" => "Right",
        other => other,
    };
    if !key.is_empty() {
        parts.push(key);
    }
    std::borrow::Cow::Owned(parts.join("+"))
}

/// `NF2/Kbd` — shortcut chip ("F6", "⌘↵"). `inverse` = drawn inside a tooltip.
pub fn kbd(ui: &mut Ui, key: &str, inverse: bool) -> Response {
    let key = &*platform_shortcut(key);
    let p = palette(ui.ctx());
    let (bg, border, fg) = if inverse {
        (p.text_primary, p.text_primary, p.text_inverse)
    } else {
        (Color32::TRANSPARENT, p.border_default, p.text_secondary)
    };
    let g = galley(ui, key, TextStyle::Small, fg);
    let w = (g.size().x + 2.0 * 6.0).max(20.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, KBD), Sense::hover());
    fill(ui, rect, radius::SM, bg, Some(border));
    let x = rect.center().x - g.size().x / 2.0;
    text_at(ui, x, rect, g, fg);
    response
}

fn tooltip_shortcuts_id() -> egui::Id {
    egui::Id::new("nf2-tooltip-shortcuts-hidden")
}

/// Preferences ▸ General ▸ "Show tooltips with shortcuts": when `show` is false every `NF2/Tooltip`,
/// including the ones inside `neoflash-ui` components, omits its shortcut chip. Default: shown.
pub fn set_tooltip_shortcuts(ctx: &egui::Context, show: bool) {
    ctx.data_mut(|data| data.insert_temp(tooltip_shortcuts_id(), !show));
}

/// Whether tooltips currently show their shortcut chip (see [`set_tooltip_shortcuts`]).
pub fn tooltip_shortcuts(ctx: &egui::Context) -> bool {
    !ctx.data(|data| {
        data.get_temp::<bool>(tooltip_shortcuts_id())
            .unwrap_or(false)
    })
}

/// Paint a non-interactive `NF2/Tooltip` bubble on the `Order::Tooltip` overlay layer without
/// registering an `egui::Area` in `Memory::areas` (so tooltips can never steal hover, cursor, or
/// clicks from controls beneath them) and automatically flip above `target_rect` when near the
/// bottom edge of the viewport (`T-367`).
pub fn paint_tooltip_bubble(
    ctx: &egui::Context,
    target_rect: Rect,
    label: &str,
    shortcut: Option<&str>,
) {
    if label.is_empty() {
        return;
    }
    let shortcut = shortcut.filter(|_| tooltip_shortcuts(ctx));
    let p = palette(ctx);
    let raw_screen = ctx.input(|i| i.viewport_rect());
    let screen = if raw_screen.is_positive() && raw_screen.width().is_finite() {
        raw_screen
    } else {
        ctx.content_rect()
    };
    let max_text_w = (screen.width() - 32.0).max(120.0);
    let mut label_job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        crate::typography::format(TextStyle::Body, p.text_inverse),
    );
    label_job.wrap.max_width = max_text_w;
    let label_galley = ctx.fonts_mut(|f| f.layout_job(label_job));

    let kbd_layout = shortcut.and_then(|raw_key| {
        let key_str = platform_shortcut(raw_key);
        if key_str.is_empty() {
            return None;
        }
        let job = egui::text::LayoutJob::single_section(
            key_str.into_owned(),
            crate::typography::format_line(TextStyle::Small, p.text_inverse),
        );
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        let chip_w = (g.size().x + 12.0).max(20.0);
        Some((g, chip_w))
    });

    let pad_x = 10.0;
    let pad_y = 5.0;
    let content_w = label_galley.size().x
        + kbd_layout
            .as_ref()
            .map_or(0.0, |(_, chip_w)| space::S3 + *chip_w);
    let content_h = label_galley
        .size()
        .y
        .max(if kbd_layout.is_some() { KBD } else { 0.0 });
    let box_w = content_w + 2.0 * pad_x;
    let box_h = content_h + 2.0 * pad_y;

    let below_y = target_rect.bottom() + space::S2;
    let above_y = target_rect.top() - space::S2 - box_h;
    let y = if below_y + box_h <= screen.bottom() - 2.0 {
        below_y
    } else if above_y >= screen.top() + 2.0 {
        above_y
    } else if target_rect.top() - screen.top() > screen.bottom() - target_rect.bottom() {
        above_y.max(screen.top() + 2.0)
    } else {
        below_y.min((screen.bottom() - box_h - 2.0).max(screen.top() + 2.0))
    };
    let min_x = screen.left() + 4.0;
    let max_x = (screen.right() - box_w - 4.0).max(min_x);
    let x = (target_rect.center().x - box_w * 0.5).clamp(min_x, max_x);
    let box_rect = Rect::from_min_size(egui::pos2(x, y), egui::vec2(box_w, box_h));

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("nf2-tooltip-layer"),
    ));
    painter.add(
        crate::metrics::SHADOW_POPOVER
            .egui()
            .as_shape(box_rect, cr(radius::MD)),
    );
    painter.rect_filled(box_rect, cr(radius::MD), p.tooltip_bg);

    let label_pos = egui::pos2(
        box_rect.left() + pad_x,
        box_rect.center().y - label_galley.size().y * 0.5,
    );
    painter.galley(label_pos, label_galley.clone(), p.text_inverse);

    if let Some((kbd_galley, chip_w)) = kbd_layout {
        let chip_rect = Rect::from_min_size(
            egui::pos2(
                box_rect.left() + pad_x + label_galley.size().x + space::S3,
                box_rect.center().y - KBD * 0.5,
            ),
            egui::vec2(chip_w, KBD),
        );
        painter.rect_filled(chip_rect, cr(radius::SM), p.text_primary);
        painter.rect_stroke(
            chip_rect,
            cr(radius::SM),
            Stroke::new(1.0_f32, p.text_primary),
            egui::StrokeKind::Inside,
        );
        let gx = chip_rect.center().x - kbd_galley.size().x * 0.5;
        let gy = chip_rect.center().y - kbd_galley.size().y * 0.5;
        painter.galley(egui::pos2(gx, gy), kbd_galley, p.text_inverse);
    }
}

/// `NF2/Tooltip` — `tooltip/bg` + `text/inverse`, label and optional shortcut chip.
/// Use instead of `on_hover_text` for every icon-only control.
pub fn tooltip(response: Response, label: &str, shortcut: Option<&str>) -> Response {
    let ctx = response.ctx.clone();
    if !response.hovered() || label.is_empty() || ctx.input(|i| i.pointer.any_down()) {
        return response;
    }
    let delay = ctx.style().interaction.tooltip_delay as f64;
    let still = ctx.input(|i| i.pointer.time_since_last_movement()) as f64;
    if still < delay {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - still + 0.01));
        return response;
    }
    paint_tooltip_bubble(&ctx, response.rect, label, shortcut);
    response
}

/// `NF2/Swatch` — 22 px colour chip (checkerboard behind translucent colours).
pub fn swatch(ui: &mut Ui, color: Color32, side: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
    paint_swatch(ui, rect, color, radius::SM);
    response
}

/// Paint a colour chip with a checkerboard for alpha and a subtle border.
pub fn paint_swatch(ui: &Ui, rect: Rect, color: Color32, r: f32) {
    let p = palette(ui.ctx());
    let painter = ui.painter();
    if color.a() < 255 {
        painter.rect_filled(rect, cr(r), Color32::WHITE);
        let cell = (rect.width() / 4.0).max(3.0);
        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { cell };
            while x < rect.right() {
                let c =
                    Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell)).intersect(rect);
                painter.rect_filled(c, 0.0, Color32::from_gray(204));
                x += cell * 2.0;
            }
            y += cell;
            row += 1;
        }
    }
    painter.rect_filled(rect, cr(r), color);
    painter.rect_stroke(
        rect,
        cr(r),
        Stroke::new(1.0_f32, p.border_default),
        egui::StrokeKind::Inside,
    );
}

/// `NF2/Divider` — 1 px `border/subtle` line; `vertical` = between grouped controls (20 px tall).
pub fn divider(ui: &mut Ui, vertical: bool) -> Response {
    let p = palette(ui.ctx());
    let size = if vertical {
        egui::vec2(1.0 + space::S2, 20.0)
    } else {
        egui::vec2(ui.available_width(), 1.0 + space::S2)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let stroke = Stroke::new(1.0_f32, p.border_subtle);
    if vertical {
        ui.painter().vline(rect.center().x, rect.y_range(), stroke);
    } else {
        ui.painter().hline(rect.x_range(), rect.center().y, stroke);
    }
    response
}

/// `NF2/Logo` — 28 px accent tile with the NeoFlash mark.
pub fn logo(ui: &mut Ui, side: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
    let k = side / 28.0;
    ui.painter()
        .rect_filled(rect, cr(radius::LG * k), p.accent_default);
    let c = rect.center();
    ui.painter()
        .circle_stroke(c, 8.0 * k, Stroke::new(2.0 * k, p.text_on_accent));
    ui.painter().circle_filled(c, 4.0 * k, p.text_on_accent);
    response
}

/// `NF2/Spinner` — 20 px indeterminate ring.
pub fn spinner(ui: &mut Ui) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let c = rect.center();
    ui.painter()
        .circle_stroke(c, 8.0, Stroke::new(2.0_f32, p.border_default));
    let start = t * 5.0;
    let pts: Vec<_> = (0..=16)
        .map(|i| {
            let a = start + i as f32 / 16.0 * std::f32::consts::FRAC_PI_2 * 1.5;
            c + 8.0 * egui::vec2(a.cos(), a.sin())
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        pts,
        Stroke::new(2.0_f32, p.accent_default),
    ));
    ui.ctx().request_repaint();
    response
}

/// `NF2/Progress` — label row + 4 px bar. `fraction = None` = indeterminate.
pub fn progress(ui: &mut Ui, label: &str, fraction: Option<f32>, width: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
    let label_rect = Rect::from_min_size(rect.min, egui::vec2(width, 16.0));
    text_at(
        ui,
        rect.left(),
        label_rect,
        galley(ui, label, TextStyle::Small, p.text_secondary),
        p.text_secondary,
    );
    if let Some(f) = fraction {
        let pct = format!("{:.0} %", f * 100.0);
        let g = galley(ui, &pct, TextStyle::MonoSmall, p.text_tertiary);
        text_at(
            ui,
            rect.right() - g.size().x,
            label_rect,
            g,
            p.text_tertiary,
        );
    }
    let bar = Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - 4.0),
        egui::vec2(width, 4.0),
    );
    ui.painter().rect_filled(bar, cr(2.0), p.border_strong);
    let filled = match fraction {
        Some(f) => Rect::from_min_size(bar.min, egui::vec2(width * f.clamp(0.0, 1.0), 4.0)),
        None => {
            let t = (ui.input(|i| i.time) as f32 * 0.6).fract();
            ui.ctx().request_repaint();
            Rect::from_min_size(
                egui::pos2(bar.left() + (width * 1.3) * t - width * 0.3, bar.top()),
                egui::vec2(width * 0.3, 4.0),
            )
            .intersect(bar)
        }
    };
    ui.painter().rect_filled(filled, cr(2.0), p.accent_default);
    response
}

/// `Type` of `NF2/Toast`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastType {
    Info,
    Success,
    Warning,
    Error,
}

/// `NF2/Toast` — 46 px notification with a coloured stripe, icon, message, optional action and close.
pub struct Toast<'a> {
    pub kind: ToastType,
    pub message: &'a str,
    pub action: Option<&'a str>,
}

/// What the user did with a toast this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToastResponse {
    pub action: bool,
    pub close: bool,
}

impl Toast<'_> {
    pub fn show(self, ui: &mut Ui, width: f32) -> ToastResponse {
        let p = palette(ui.ctx());
        let (color, icon) = match self.kind {
            ToastType::Info => (p.accent_default, i16::INFO),
            ToastType::Success => (p.success_default, i16::CHECK),
            ToastType::Warning => (p.warning_default, i16::WARNING),
            ToastType::Error => (p.danger_default, i16::ERROR),
        };
        let mut out = ToastResponse::default();
        egui::Frame::new()
            .fill(p.bg_raised)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .corner_radius(cr(radius::LG))
            .inner_margin(egui::Margin::symmetric(space::S3 as i8, 7))
            .shadow(crate::metrics::SHADOW_POPOVER.egui())
            .show(ui, |ui| {
                ui.set_width(width - 2.0 * space::S3);
                ui.horizontal(|ui| {
                    let (stripe, _) = ui.allocate_exact_size(egui::vec2(3.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(stripe, cr(1.5), color);
                    icons::show(ui, icon, color);
                    ui.label(
                        crate::typography::rich(self.message, TextStyle::Body)
                            .color(p.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        out.close = ui
                            .add(super::IconButton::new(i16::X, "Dismiss").side(24.0))
                            .clicked();
                        if let Some(a) = self.action {
                            out.action = ui.add(super::Button::ghost(a)).clicked();
                        }
                    });
                });
            });
        out
    }
}

/// `NF2/Empty State` — icon in a circle, title, one-sentence explanation, optional action.
pub struct EmptyState<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub body: &'a str,
    pub action: Option<&'a str>,
}

impl EmptyState<'_> {
    /// Returns `true` when the action button was clicked.
    pub fn show(self, ui: &mut Ui) -> bool {
        let p = palette(ui.ctx());
        let mut clicked = false;
        ui.vertical_centered(|ui| {
            ui.add_space(space::S4);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 18.0, p.bg_hover);
            icons::paint(ui.painter(), ui.ctx(), rect, self.icon, p.text_secondary);
            ui.add_space(space::S2);
            ui.label(
                crate::typography::rich(self.title, TextStyle::BodyStrong).color(p.text_primary),
            );
            ui.label(crate::typography::rich(self.body, TextStyle::Small).color(p.text_tertiary));
            if let Some(a) = self.action {
                ui.add_space(space::S2);
                clicked = ui.add(super::Button::new(a)).clicked();
            }
            ui.add_space(space::S4);
        });
        clicked
    }
}

/// `Type` of the inline message line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineMessageType {
    /// Neutral explanation (publish trade-off, "Watch expressions are not available yet …").
    #[default]
    Info,
    /// Allowed but needs attention ("… already exists. Replace overwrites …").
    Warning,
    /// Blocks the action (invalid name, invalid runtime URL, trace failed).
    Error,
}

impl InlineMessageType {
    pub fn icon(self) -> Icon {
        match self {
            Self::Info => i16::INFO,
            Self::Warning => i16::WARNING,
            Self::Error => i16::ERROR,
        }
    }
    /// Icon and text colour: Info `text/secondary`, Warning `warning/default`, Error `danger/default`.
    pub fn color(self, p: &crate::Palette) -> Color32 {
        match self {
            Self::Info => p.text_secondary,
            Self::Warning => p.warning_default,
            Self::Error => p.danger_default,
        }
    }
}

/// Inline message line (R-032 / R-033 / R-036 / R-022) — 16 pt icon, 8 pt gap and `NF2/Small` text in
/// the tone colour, wrapped to the width. Sits under the field or group it explains; it never takes
/// focus. Width = `width` or the available width.
#[must_use = "add it with ui.add(…)"]
pub struct InlineMessage<'a> {
    kind: InlineMessageType,
    text: &'a str,
    width: Option<f32>,
}

impl<'a> InlineMessage<'a> {
    pub fn new(kind: InlineMessageType, text: &'a str) -> Self {
        Self {
            kind,
            text,
            width: None,
        }
    }
    pub fn info(text: &'a str) -> Self {
        Self::new(InlineMessageType::Info, text)
    }
    pub fn warning(text: &'a str) -> Self {
        Self::new(InlineMessageType::Warning, text)
    }
    pub fn error(text: &'a str) -> Self {
        Self::new(InlineMessageType::Error, text)
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl Widget for InlineMessage<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let color = self.kind.color(p);
        let width = self.width.unwrap_or_else(|| ui.available_width()).max(48.0);
        let text_w = width - 16.0 - space::S2;
        let g = super::galley_wrapped(ui, self.text, TextStyle::Small, color, text_w);
        let height = g.size().y.max(16.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        if ui.is_rect_visible(rect) {
            let icon = Rect::from_min_size(rect.min, egui::vec2(16.0, 16.0));
            icons::paint(ui.painter(), ui.ctx(), icon, self.kind.icon(), color);
            ui.painter()
                .galley(egui::pos2(icon.right() + space::S2, rect.top()), g, color);
        }
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, self.text));
        response
    }
}
