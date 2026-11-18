//! Shared, presentation-only and keyboard-transaction helpers used by editor dialogs.

use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

/// Normalized outcome of modal keyboard / backdrop handling (`T-224`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogKeyboardOutcome {
    None,
    Cancel,
    Confirm,
}

/// Snapshot of focus and popup state captured **before** painting an `egui::Modal` body.
///
/// Sampling `wants_keyboard_input()` and `Popup::is_any_open()` before the modal body runs ensures
/// that `Escape` or `Enter` pressed while a popup menu is open or while a `Field` / `TextField`
/// has keyboard focus closes the popup or commits/reverts that field first (surrendering focus)
/// instead of discarding or submitting the entire modal dialog on the same keystroke.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DialogKeyboardState {
    pub(crate) was_typing: bool,
    pub(crate) was_popup_open: bool,
}

pub(crate) fn prev_wants_keyboard_input(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-wants-kb")))
        .unwrap_or(false)
}

pub(crate) fn prev_popup_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-popup-open")))
        .unwrap_or(false)
}

pub(crate) fn mark_inline_edit_active(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("nf2-inline-edit-this-frame"), true));
}

pub(crate) fn prev_inline_edit_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-inline-edit")))
        .unwrap_or(false)
}

/// Returns `true` when `Escape` on this pass is dismissing a focused text/number field, open popup,
/// or active inline panel rename/edit form from the previous frame (`egui::Focus::begin_pass` clears
/// focus on `Escape` before widgets run, and `Editor::shortcuts` runs before docked panels).
pub(crate) fn escape_dismissed_focus_or_popup(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
        && (prev_wants_keyboard_input(ctx) || prev_popup_open(ctx) || prev_inline_edit_active(ctx))
}

const TRACKED_RGBA_POPUPS_ID: &str = "nf2-tracked-rgba-popups";

/// Registers an RGBA colour popover (`paint_controls::show_color32_field_popup` or
/// `show_rgba_color_popup`) alongside its owning `ViewportId` so [`is_any_popup_open`] and
/// [`DialogKeyboardState`] treat an open colour popover as a first-step `Escape` target while
/// [`is_editor_popup_open_on_viewport`] scopes z-order and cursor gating strictly to that viewport.
pub(crate) fn track_rgba_popup(ctx: &egui::Context, response_id: egui::Id) {
    let open_id = response_id.with("nf2-rgba-popup-open");
    let viewport_id = ctx.viewport_id();
    ctx.data_mut(|d| {
        let key = egui::Id::new(TRACKED_RGBA_POPUPS_ID);
        let mut list = d
            .get_temp::<Vec<(egui::Id, egui::ViewportId)>>(key)
            .unwrap_or_default();
        if let Some(entry) = list.iter_mut().find(|(id, _)| *id == open_id) {
            entry.1 = viewport_id;
        } else {
            list.push((open_id, viewport_id));
        }
        d.insert_temp(key, list);
    });
}

pub(crate) fn is_any_tracked_rgba_popup_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<Vec<(egui::Id, egui::ViewportId)>>(egui::Id::new(TRACKED_RGBA_POPUPS_ID))
            .is_some_and(|list| {
                list.into_iter()
                    .any(|(open_id, _)| d.get_temp::<bool>(open_id).unwrap_or(false))
            })
    })
}

pub(crate) fn is_tracked_rgba_popup_open_on_viewport(
    ctx: &egui::Context,
    viewport_id: egui::ViewportId,
) -> bool {
    ctx.data(|d| {
        d.get_temp::<Vec<(egui::Id, egui::ViewportId)>>(egui::Id::new(TRACKED_RGBA_POPUPS_ID))
            .is_some_and(|list| {
                list.into_iter().any(|(open_id, vp)| {
                    vp == viewport_id && d.get_temp::<bool>(open_id).unwrap_or(false)
                })
            })
    })
}

fn panel_viewport(editor: &Editor, panel: crate::workspace::PanelId) -> egui::ViewportId {
    crate::dock_grid::surface_viewport(editor.workspace.panel_surface(panel))
}

fn active_color_popover_viewport(editor: &Editor, ctx: &egui::Context) -> Option<egui::ViewportId> {
    let popover = ctx.data(|d| {
        d.get_temp::<crate::authoring_modify::ActiveColorPopover>(egui::Id::new(
            "nf2-active-color-popover",
        ))
    })?;
    let vp = match popover.source_id.as_str() {
        "tool-well-fill" | "tool-well-stroke" => {
            panel_viewport(editor, crate::workspace::PanelId::Brushes)
        }
        "swatches-open-mixer" => panel_viewport(editor, crate::workspace::PanelId::Swatches),
        "live-bitmap-well-fill" | "live-bitmap-well-stroke" => egui::ViewportId::ROOT,
        _ => panel_viewport(editor, crate::workspace::PanelId::Properties),
    };
    Some(vp)
}

/// Viewport-scoped popup/popover check so popovers opened inside a detached floating palette
/// (`Tools` flyout/Fill/Stroke, `Swatches`/`Color`/`Brushes` popovers) never masquerade as
/// root-window menus or suppress `Actions` cursors/chords in another viewport.
pub(crate) fn is_editor_popup_open_on_viewport(
    editor: &Editor,
    ctx: &egui::Context,
    viewport_id: egui::ViewportId,
) -> bool {
    let egui_popup = ctx.viewport_id() == viewport_id && egui::Popup::is_any_open(ctx);
    let clipboard_menu = editor.clipboard.menu_open
        && editor
            .clipboard
            .menu_viewport
            .unwrap_or(egui::ViewportId::ROOT)
            == viewport_id;
    let rail_popover = super::tools::is_rail_popover_open(ctx)
        && panel_viewport(editor, crate::workspace::PanelId::Tools) == viewport_id;
    let color_popover = active_color_popover_viewport(editor, ctx) == Some(viewport_id);
    let rgba_popover = is_tracked_rgba_popup_open_on_viewport(ctx, viewport_id);
    let outline_popover = editor.is_layer_outline_color_popover_open(ctx)
        && panel_viewport(editor, crate::workspace::PanelId::Timeline) == viewport_id;
    let root_only = viewport_id == egui::ViewportId::ROOT
        && (editor.stage_view.settings_open
            || super::symbols_popover::is_symbols_popover_open(ctx)
            || super::function_search::is_function_search_open(ctx)
            || editor.timeline_shape_hints.context_menu.is_some());
    egui_popup
        || clipboard_menu
        || rail_popover
        || color_popover
        || rgba_popover
        || outline_popover
        || root_only
}

/// Whether any `egui::Popup` (Select, MenuBar, context menu, Timeline menu) or editor popover
/// (`Tools` flyout/popover, `Color` popover, RGBA colour popover, `Symbols & scenes`, `Help search`) is open.
pub(crate) fn is_any_popup_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || super::tools::is_rail_popover_open(ctx)
        || crate::authoring_modify::is_color_popover_open(ctx)
        || super::symbols_popover::is_symbols_popover_open(ctx)
        || super::function_search::is_function_search_open(ctx)
        || is_any_tracked_rgba_popup_open(ctx)
}

/// Editor-aware popup check used by global shortcut and Stage input gating. This includes custom
/// document-scoped areas which cannot be discovered through `egui::Popup::is_any_open`.
pub(crate) fn is_any_editor_popup_open(editor: &Editor, ctx: &egui::Context) -> bool {
    is_any_popup_open(ctx)
        || editor.is_layer_outline_color_popover_open(ctx)
        || editor.timeline_shape_hints.context_menu.is_some()
}

/// Whether unmodified `Enter` inside a local modal should be ignored because a control holds
/// keyboard focus or a popup/popover is open.
pub(crate) fn blocks_local_dialog_enter(ctx: &egui::Context) -> bool {
    ctx.wants_keyboard_input() || is_any_popup_open(ctx)
}

/// Records end-of-frame keyboard focus and popup state so the next pass can distinguish
/// field-level `Escape` (surrender focus) from dialog/Stage-level `Escape` (cancel/deselect).
pub(crate) fn record_end_of_frame_focus(ctx: &egui::Context) {
    let wants_kb = ctx.wants_keyboard_input();
    let popup_open = is_any_popup_open(ctx);
    ctx.data_mut(|d| {
        let inline_edit = d
            .get_temp::<bool>(egui::Id::new("nf2-inline-edit-this-frame"))
            .unwrap_or(false);
        d.insert_temp(egui::Id::new("nf2-inline-edit-this-frame"), false);
        d.insert_temp(egui::Id::new("nf2-prev-inline-edit"), inline_edit);
        d.insert_temp(egui::Id::new("nf2-prev-wants-kb"), wants_kb);
        d.insert_temp(egui::Id::new("nf2-prev-popup-open"), popup_open);

        // Controls register their stable popup ID every frame while they are painted. Retaining
        // closed IDs forever makes the context grow with every document/object/filter ever
        // inspected. Keep only popovers which must still gate shortcuts on the next frame; a
        // closed control registers itself again before it can reopen.
        let key = egui::Id::new(TRACKED_RGBA_POPUPS_ID);
        if let Some(mut list) = d.get_temp::<Vec<(egui::Id, egui::ViewportId)>>(key) {
            list.retain(|(open_id, _)| d.get_temp::<bool>(*open_id).unwrap_or(false));
            if list.is_empty() {
                d.remove::<Vec<(egui::Id, egui::ViewportId)>>(key);
            } else {
                d.insert_temp(key, list);
            }
        }
    });
}

impl DialogKeyboardState {
    pub(crate) fn capture(ctx: &egui::Context) -> Self {
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        Self {
            was_typing: ctx.wants_keyboard_input() || (esc && prev_wants_keyboard_input(ctx)),
            was_popup_open: is_any_popup_open(ctx) || (esc && prev_popup_open(ctx)),
        }
    }

    pub(crate) fn resolve<R>(
        self,
        ctx: &egui::Context,
        modal: &egui::ModalResponse<R>,
        allow_confirm: bool,
    ) -> DialogKeyboardOutcome {
        let popup_open = self.was_popup_open || is_any_popup_open(ctx);
        if popup_open {
            let _ = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            return DialogKeyboardOutcome::None;
        }
        if self.was_typing {
            let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if (esc || enter)
                && let Some(focused) = ctx.memory(|m| m.focused())
            {
                ctx.memory_mut(|m| m.surrender_focus(focused));
            }
            if modal.backdrop_response.clicked() {
                return DialogKeyboardOutcome::Cancel;
            }
            return DialogKeyboardOutcome::None;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            DialogKeyboardOutcome::Cancel
        } else if allow_confirm
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            DialogKeyboardOutcome::Confirm
        } else if modal.should_close() {
            DialogKeyboardOutcome::Cancel
        } else {
            DialogKeyboardOutcome::None
        }
    }
}

/// Stage-preview placeholder (export / import dialogs): `bg/canvas` pasteboard with a `bg/stage` page.
pub(super) fn preview_box(ui: &mut Ui, width: f32, height: f32, label: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::lg() as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
    ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
    ui.painter().text(
        stage.center(),
        egui::Align2::CENTER_CENTER,
        label,
        neoflash_ui::typography::font(TextStyle::Small),
        nf.text_tertiary,
    );
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
}

// ---- v3 dialog shell (Preferences, Publish settings): one size per density, a sidebar, one
// scrolling page with the form grid and a one-row footer. Spec: `S.side`, `S.page`, `S.navRow`,
// `S.group`, `K.formRow` in `interface/figma-plugin/v3/src/`.

/// Size of a fixed v3 dialog: `width` × `height` of the current density, clamped to the window
/// minus a `space::s5()` margin on every side.
pub(crate) fn fixed_dialog_size(ctx: &egui::Context, width: f32, height: f32) -> egui::Vec2 {
    let window = ctx.content_rect().size();
    let margin = 2.0 * space::s5();
    egui::vec2(
        width.min(window.x - margin).max(metrics::dialog_s()),
        height
            .min(window.y - margin)
            .max(metrics::dialog_header() + 6.0 * metrics::control()),
    )
}

/// Child `Ui` confined to `rect`: it never moves the parent cursor, so its content cannot resize
/// the dialog.
fn region<R>(
    ui: &mut Ui,
    salt: &'static str,
    rect: egui::Rect,
    layout: egui::Layout,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(rect)
            .layout(layout),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

/// Visible part of a dialog page, for lists that fill the height left under the rows above them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PageArea {
    top: f32,
    height: f32,
}

impl PageArea {
    /// Height for a list at the cursor so that it and everything drawn after it (measured on the
    /// previous frame by [`Self::remember_below`], `estimate` before that) end at the page bottom.
    pub(crate) fn fill_height(self, ui: &Ui, salt: &'static str, estimate: f32, min: f32) -> f32 {
        let below = ui
            .data(|d| d.get_temp::<f32>(egui::Id::new(("nf3-page-below", salt))))
            .unwrap_or(estimate);
        // 1 pt of slack keeps rounding from producing a scroll bar on a page that just fits.
        (self.height - (ui.cursor().top() - self.top) - below - 1.0)
            .floor()
            .max(min)
    }

    /// Call after the last row of the page with the bottom edge of the filling list.
    pub(crate) fn remember_below(self, ui: &Ui, salt: &'static str, list_bottom: f32) {
        let below = (ui.min_rect().bottom() - list_bottom).max(0.0);
        ui.data_mut(|d| d.insert_temp(egui::Id::new(("nf3-page-below", salt)), below));
    }
}

/// Fixed-size dialog body: header, sidebar, page and footer rectangles inside one allocation.
pub(super) struct DialogShell {
    /// The header's close button was clicked.
    pub(super) close: bool,
    sidebar: egui::Rect,
    page: egui::Rect,
    footer: egui::Rect,
}

impl DialogShell {
    /// Allocate exactly `size` and paint the header, the sidebar surface and the separators.
    pub(super) fn show(ui: &mut Ui, title: &str, size: egui::Vec2, sidebar_width: f32) -> Self {
        let nf = neoflash_ui::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let header = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(size.x, metrics::dialog_header().min(size.y)),
        );
        let footer_height = metrics::control() + 2.0 * space::s4();
        let footer = egui::Rect::from_min_max(
            egui::pos2(
                rect.left(),
                (rect.bottom() - footer_height).max(header.bottom()),
            ),
            rect.max,
        );
        let body = egui::Rect::from_min_max(header.left_bottom(), footer.right_top());
        let sidebar = egui::Rect::from_min_max(
            body.min,
            egui::pos2(
                body.left() + sidebar_width.min(body.width() / 2.0),
                body.bottom(),
            ),
        );
        let page = egui::Rect::from_min_max(sidebar.right_top(), body.max);
        let close = region(
            ui,
            "header",
            header,
            egui::Layout::top_down(egui::Align::Min),
            |ui| c::DialogHeader { title }.show(ui),
        );
        let line = egui::Stroke::new(1.0_f32, nf.border_subtle);
        let painter = ui.painter();
        painter.rect_filled(sidebar, 0.0, nf.bg_app);
        painter.hline(rect.x_range(), header.bottom() - 0.5, line);
        painter.vline(sidebar.right() - 0.5, sidebar.y_range(), line);
        painter.hline(rect.x_range(), footer.top() + 0.5, line);
        Self {
            close,
            sidebar,
            page,
            footer,
        }
    }

    /// Category / target list on `bg/app`; rows are 1 pt apart.
    pub(super) fn sidebar<R>(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        let mut rect = self.sidebar.shrink(space::s1());
        rect.max.x -= 1.0;
        region(
            ui,
            "sidebar",
            rect,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::s2(), 1.0);
                add(ui)
            },
        )
    }

    /// The page: one vertical scroll area that always fills the body, `space::s4()` /
    /// `space::s5()` padding, rows `metrics::row_gap()` apart.
    pub(super) fn page<R>(
        &self,
        ui: &mut Ui,
        id_salt: &'static str,
        add: impl FnOnce(&mut Ui, PageArea) -> R,
    ) -> R {
        use crate::theme::ScrollTheme as _;
        let height = (self.page.height() - 2.0 * space::s4()).max(0.0);
        region(
            ui,
            "page",
            self.page,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                // Wheel and scroll bar only: pressing or dragging a control must not move the page.
                egui::ScrollArea::vertical()
                    .id_salt(id_salt)
                    .scroll_source(
                        egui::scroll_area::ScrollSource::SCROLL_BAR
                            | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
                    )
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(
                                space::s5() as i8,
                                space::s4() as i8,
                            ))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing =
                                    egui::vec2(space::s2(), metrics::row_gap());
                                let area = PageArea {
                                    top: ui.cursor().top(),
                                    height,
                                };
                                add(ui, area)
                            })
                            .inner
                    })
                    .inner
            },
        )
    }

    /// One footer row. `right` adds its buttons from the right edge inwards (primary first);
    /// `left` gets the remaining width.
    pub(super) fn footer(
        &self,
        ui: &mut Ui,
        left: impl FnOnce(&mut Ui),
        right: impl FnOnce(&mut Ui),
    ) {
        region(
            ui,
            "footer",
            self.footer.shrink2(egui::vec2(space::s5(), space::s4())),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::s2();
                right(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), left);
            },
        );
    }
}

/// Sidebar row of a v3 dialog: optional enable checkbox, icon, label. The selected row has
/// `bg/selected` and an accent icon. The checkbox is its own control; the rest of the row selects.
pub(super) fn nav_row(
    ui: &mut Ui,
    icon: neoflash_ui::Icon,
    label: &str,
    selected: bool,
    checked: Option<&mut bool>,
) -> egui::Response {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, row) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), metrics::row()),
        egui::Sense::click(),
    );
    let corner = egui::CornerRadius::same(radius::sm() as u8);
    if selected {
        ui.painter().rect_filled(rect, corner, nf.bg_selected);
    } else if row.hovered() {
        ui.painter().rect_filled(rect, corner, nf.bg_hover);
    }
    let mut x = rect.left() + space::s3();
    if let Some(checked) = checked {
        let side = metrics::check_box();
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(side, rect.height()));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("nav-check", label))
                .max_rect(cell)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.add(c::Checkbox::new(checked, ""));
        x += side + space::s2();
    }
    let side = neoflash_ui::metrics::ICON_UI;
    neoflash_ui::icons::paint(
        ui.painter(),
        ui.ctx(),
        egui::Rect::from_center_size(
            egui::pos2(x + side / 2.0, rect.center().y),
            egui::vec2(side, side),
        ),
        icon,
        if selected {
            nf.accent_default
        } else {
            nf.text_secondary
        },
    );
    x += side + space::s2();
    let clip = egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(rect.right() - space::s3(), rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        neoflash_ui::typography::font(if selected {
            TextStyle::BodyStrong
        } else {
            TextStyle::Body
        }),
        nf.text_primary,
    );
    let enabled = row.enabled();
    row.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, selected, label)
    });
    row
}

/// Group caption of the form grid: Overline tertiary on the baseline of a `section_header()` row.
pub(crate) fn group_caption(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), metrics::section_header()),
        egui::Sense::hover(),
    );
    // Painted text has no widget of its own: expose the caption to assistive technology.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, s));
    let job = egui::text::LayoutJob::single_section(
        rich(s, TextStyle::Overline).text().to_owned(),
        neoflash_ui::typography::format_line(TextStyle::Overline, nf.text_tertiary),
    );
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(rect.left(), rect.bottom() - space::s1() - galley.size().y),
        galley,
        nf.text_tertiary,
    );
}

/// One row of the form grid: label column (`dialog_label()`, Body secondary) and the controls at
/// their natural width. Controls that do not fit continue on the next line of the same row, so a
/// row can never be wider than the page.
pub(crate) fn form_row<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> R {
    let nf = neoflash_ui::palette(ui.ctx());
    let height = metrics::control();
    let gap = space::s2();
    let row_gap = metrics::row_gap();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(metrics::dialog_label(), height),
                egui::Sense::hover(),
            );
            ui.painter().with_clip_rect(rect).text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                label,
                neoflash_ui::typography::font(TextStyle::Body),
                nf.text_secondary,
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, row_gap);
                    content(ui)
                },
            )
            .inner
        },
    )
    .inner
}

/// Block that starts on the control axis of the form grid (hints, messages, weight reports).
pub(crate) fn form_block<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let indent = metrics::dialog_label() + space::s2();
    let width = (ui.available_width() - indent).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.set_width(width);
            content(ui)
        })
        .inner
    })
    .inner
}

/// Small tertiary explanation under a row, on the control axis.
pub(crate) fn form_hint(ui: &mut Ui, text: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    form_block(ui, |ui| {
        ui.add(egui::Label::new(rich(text, TextStyle::Small).color(nf.text_tertiary)).wrap());
    });
}

/// The `Output` group shared by every publish target: caption, then `File` / `Folder`, the path
/// field (the only control that fills the row; `field` gets its width) and `Browse…`.
/// Returns `true` when `Browse…` was clicked.
pub(super) fn output_group(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui, f32)) -> bool {
    group_caption(ui, "Output");
    form_row(ui, label, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let browse = ui.add(c::Button::new("Browse…")).clicked();
            field(ui, ui.available_width());
            browse
        })
        .inner
    })
}

// ---- v3 split dialogs (export, import): a fixed dialog without a sidebar whose body is an
// artwork column (`aside`) and the form (`form`). Spec: `D2.split`, `D2.foot` in `33-dialogs-d.js`.
// One-column dialogs use `FormDialog` below.

impl DialogShell {
    /// Fixed-size dialog without a sidebar: header, one body (`page`, or `aside` + `form`), footer.
    pub(super) fn single(ui: &mut Ui, title: &str, size: egui::Vec2) -> Self {
        let nf = neoflash_ui::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let header = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(size.x, metrics::dialog_header().min(size.y)),
        );
        let footer_height = metrics::control() + 2.0 * space::s4();
        let footer = egui::Rect::from_min_max(
            egui::pos2(
                rect.left(),
                (rect.bottom() - footer_height).max(header.bottom()),
            ),
            rect.max,
        );
        let page = egui::Rect::from_min_max(header.left_bottom(), footer.right_top());
        let close = region(
            ui,
            "header",
            header,
            egui::Layout::top_down(egui::Align::Min),
            |ui| c::DialogHeader { title }.show(ui),
        );
        let line = egui::Stroke::new(1.0_f32, nf.border_subtle);
        ui.painter()
            .hline(rect.x_range(), header.bottom() - 0.5, line);
        ui.painter().hline(rect.x_range(), footer.top() + 0.5, line);
        Self {
            close,
            sidebar: egui::Rect::from_min_max(page.min, page.left_bottom()),
            page,
            footer,
        }
    }

    /// Artwork column (preview, filmstrip, progress) at the left of the body: `width` wide, rows
    /// `metrics::row_gap()` apart. It never scrolls and never resizes the dialog.
    pub(super) fn aside<R>(&self, ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = egui::Rect::from_min_max(
            self.page.min + egui::vec2(space::s5(), space::s4()),
            egui::pos2(
                (self.page.left() + space::s5() + width).min(self.page.right()),
                self.page.bottom() - space::s4(),
            ),
        );
        region(
            ui,
            "aside",
            rect,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::s2(), metrics::row_gap());
                ui.set_width(rect.width());
                add(ui)
            },
        )
    }

    /// Form column to the right of an [`Self::aside`] of `aside_width`: the scrolling page.
    pub(super) fn form<R>(
        &self,
        ui: &mut Ui,
        id_salt: &'static str,
        aside_width: f32,
        add: impl FnOnce(&mut Ui, PageArea) -> R,
    ) -> R {
        let left = (self.page.left() + space::s5() + aside_width).min(self.page.right());
        Self {
            close: false,
            sidebar: self.sidebar,
            page: egui::Rect::from_min_max(egui::pos2(left, self.page.top()), self.page.max),
            footer: self.footer,
        }
        .page(ui, id_salt, add)
    }
}

/// Footer summary / status text: Small secondary, truncated to the width left of the buttons.
pub(super) fn footer_note(ui: &mut Ui, text: &str) -> egui::Response {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add(egui::Label::new(rich(text, TextStyle::Small).color(nf.text_secondary)).truncate())
}

/// Small tertiary note that wraps at the available width (`D2.note`).
pub(super) fn note(ui: &mut Ui, text: &str) -> egui::Response {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add(egui::Label::new(rich(text, TextStyle::Small).color(nf.text_tertiary)).wrap())
}

/// A full-width row of the form grid without a label column (`K.line`): the format switch, a long
/// checkbox. `control()` high, controls centred vertically.
pub(super) fn form_line<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), metrics::control()),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            content(ui)
        },
    )
    .inner
}

// ---- v3 form dialog (no sidebar): header, one page on the form grid, a one-row footer. A
// dialog whose content changes with a state gets a fixed height; a plain form takes its content
// height. Spec: `K.dialog`, `D1.body`, `D1.okCancel` in `interface/figma-plugin/v3/src/`.

/// `width` of the current density clamped to the window (content-height dialogs).
pub(crate) fn dialog_width(ctx: &egui::Context, width: f32) -> f32 {
    fixed_dialog_size(ctx, width, 0.0).x
}

/// Height of the footer row of a v3 dialog.
pub(crate) fn dialog_footer_height() -> f32 {
    metrics::control() + 2.0 * space::s4()
}

/// Height of a fixed form dialog whose page holds `rows` control rows, `captions` group captions
/// and `extra` points of other content (previews, hints, lists).
pub(crate) fn form_dialog_height(rows: usize, captions: usize, extra: f32) -> f32 {
    let lines = (rows + captions) as f32;
    metrics::dialog_header()
        + dialog_footer_height()
        + 2.0 * space::s4()
        + rows as f32 * metrics::control()
        + captions as f32 * metrics::section_header()
        + (lines - 1.0).max(0.0) * metrics::row_gap()
        + extra
}

/// `true` on the first frame a dialog is shown after it was closed: the frame for initial focus.
pub(crate) fn dialog_just_opened(ctx: &egui::Context, dialog: &'static str) -> bool {
    let key = egui::Id::new(("nf3-dialog-shown", dialog));
    let now = ctx.cumulative_pass_nr();
    let previous = ctx.data_mut(|d| {
        let previous = d.get_temp::<u64>(key);
        d.insert_temp(key, now);
        previous
    });
    previous.is_none_or(|previous| now > previous + 1)
}

/// Initial focus on the first text field of a dialog; `field` is the response of its `TextField`.
/// Returns `true` while that field owns the keyboard, for
/// [`DialogKeyboardState::resolve_first_field`].
pub(crate) fn first_field(ui: &Ui, dialog: &'static str, field: &egui::Response) -> bool {
    let edit_id = c::TextField::edit_id(field.id);
    if dialog_just_opened(ui.ctx(), dialog) {
        ui.memory_mut(|m| m.request_focus(edit_id));
    }
    let mut edit = field.clone();
    edit.id = edit_id;
    edit.has_focus() || edit.lost_focus()
}

impl DialogKeyboardState {
    /// [`Self::resolve`] for a dialog whose first field has the initial focus: `Enter` and `Esc`
    /// pressed in that field (`in_first_field`, from [`first_field`]) act on the dialog at once,
    /// as in Convert to Symbol.
    pub(crate) fn resolve_first_field<R>(
        self,
        ctx: &egui::Context,
        modal: &egui::ModalResponse<R>,
        allow_confirm: bool,
        in_first_field: bool,
    ) -> DialogKeyboardOutcome {
        if in_first_field && !self.was_popup_open && !is_any_popup_open(ctx) {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                return DialogKeyboardOutcome::Cancel;
            }
            if allow_confirm
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                return DialogKeyboardOutcome::Confirm;
            }
        }
        self.resolve(ctx, modal, allow_confirm)
    }
}

/// Page padding and row spacing shared by [`DialogShell::page`]-style pages.
fn padded_page<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui, PageArea) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(
            space::s5() as i8,
            space::s4() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(space::s2(), metrics::row_gap());
            let area = PageArea {
                top: ui.cursor().top(),
                height,
            };
            add(ui, area)
        })
        .inner
}

/// Dialog without a sidebar: header, page, footer in one column of `width`.
pub(crate) struct FormDialog {
    /// The header's close button was clicked.
    pub(crate) close: bool,
    width: f32,
    /// Page height of a fixed dialog; `None` = the page takes its content height.
    page_height: Option<f32>,
}

impl FormDialog {
    /// Header and separator. `height: Some` fixes the whole dialog (the page scrolls when its
    /// content is taller); `None` lets a plain form take its content height.
    pub(crate) fn show(ui: &mut Ui, title: &str, width: f32, height: Option<f32>) -> Self {
        Self::header(ui, title, width, height, true)
    }

    /// A prompt that must be answered with one of its buttons: the same header without the
    /// close button, content height.
    pub(crate) fn prompt(ui: &mut Ui, title: &str, width: f32) -> Self {
        Self::header(ui, title, width, None, false)
    }

    fn header(ui: &mut Ui, title: &str, width: f32, height: Option<f32>, closable: bool) -> Self {
        let nf = neoflash_ui::palette(ui.ctx());
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().min;
        let close = if closable {
            c::DialogHeader { title }.show(ui)
        } else {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(width, metrics::dialog_header()),
                egui::Sense::hover(),
            );
            // A label, so the question is exposed to assistive technology like the other titles.
            region(
                ui,
                "prompt-title",
                rect.shrink2(egui::vec2(space::s5(), 0.0)),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            rich(title, TextStyle::DialogTitle).color(nf.text_primary),
                        )
                        .truncate(),
                    );
                },
            );
            false
        };
        ui.painter().hline(
            top.x..=top.x + width,
            top.y + metrics::dialog_header() - 0.5,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        Self {
            close,
            width,
            page_height: height
                .map(|h| (h - metrics::dialog_header() - dialog_footer_height()).max(0.0)),
        }
    }

    /// The page: `space::s5()` / `space::s4()` padding, rows `metrics::row_gap()` apart. In a
    /// fixed dialog it is a scroll area that always fills the body and `PageArea` measures it.
    pub(crate) fn page<R>(
        &self,
        ui: &mut Ui,
        id_salt: &'static str,
        add: impl FnOnce(&mut Ui, PageArea) -> R,
    ) -> R {
        use crate::theme::ScrollTheme as _;
        let width = self.width;
        let Some(page_height) = self.page_height else {
            return ui
                .scope(|ui| {
                    ui.set_width(width);
                    padded_page(ui, 0.0, add)
                })
                .inner;
        };
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, page_height), egui::Sense::hover());
        let height = (page_height - 2.0 * space::s4()).max(0.0);
        region(
            ui,
            "form-page",
            rect,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(id_salt)
                    .scroll_source(
                        egui::scroll_area::ScrollSource::SCROLL_BAR
                            | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
                    )
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| padded_page(ui, height, add))
                    .inner
            },
        )
    }

    /// One footer row under a separator. `right` adds its buttons from the right edge inwards
    /// (primary first, then `Cancel`); `left` gets the remaining width (status, ghost actions).
    pub(crate) fn footer(
        &self,
        ui: &mut Ui,
        left: impl FnOnce(&mut Ui),
        right: impl FnOnce(&mut Ui),
    ) {
        let nf = neoflash_ui::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(self.width, dialog_footer_height()),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 0.5,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        region(
            ui,
            "form-footer",
            rect.shrink2(egui::vec2(space::s5(), space::s4())),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::s2();
                right(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), left);
            },
        );
    }
}

/// Framed block of a dialog page (previews, lanes, lists): `bg/field`, subtle border, control
/// radius. Returns its rectangle; the caller paints or lays out inside it.
pub(crate) fn field_block(ui: &mut Ui, size: egui::Vec2) -> egui::Rect {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let corner = egui::CornerRadius::same(radius::md() as u8);
    ui.painter().rect_filled(rect, corner, nf.bg_field);
    ui.painter().rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
    rect
}

/// Message row of a confirmation dialog: warning icon and wrapped Body text.
pub(crate) fn confirm_message(ui: &mut Ui, text: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::s3();
        let side = metrics::ICON_UI;
        let (icon, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        neoflash_ui::icons::paint(
            ui.painter(),
            ui.ctx(),
            icon,
            neoflash_ui::icons::i16::WARNING,
            nf.warning_default,
        );
        ui.add(egui::Label::new(rich(text, TextStyle::Body).color(nf.text_primary)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test window: the dialogs under test are drawn alone, with accessibility on so a
    /// control can be clicked by its name.
    struct Dialogs {
        ctx: egui::Context,
        editor: Editor,
        draw: fn(&mut Editor, &egui::Context),
        out: egui::FullOutput,
        time: f64,
    }

    impl Dialogs {
        fn new(draw: fn(&mut Editor, &egui::Context)) -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let editor = Editor::from_project(&ctx, neoflash_core::Project::blank());
            Self {
                ctx,
                editor,
                draw,
                out: egui::FullOutput::default(),
                time: 0.0,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.1;
            let (draw, editor) = (self.draw, &mut self.editor);
            self.out = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1920.0, 1080.0),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ctx| draw(editor, ctx),
            );
        }

        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }

        fn click(&mut self, label: &str) {
            let at = self
                .out
                .platform_output
                .accesskit_update
                .as_ref()
                .and_then(|update| {
                    update.nodes.iter().find_map(|(_, node)| {
                        (node.label() == Some(label))
                            .then(|| node.bounds())
                            .flatten()
                    })
                })
                .map(|b| egui::pos2((b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0))
                .unwrap_or_else(|| panic!("{label:?} is not shown"));
            self.frame(vec![
                egui::Event::PointerMoved(at),
                crate::tests::primary_button(at, true),
            ]);
            self.frame(vec![crate::tests::primary_button(at, false)]);
            self.settle();
        }

        /// Outer rectangle of the modal `id`.
        fn rect(&mut self, id: &str) -> egui::Rect {
            self.settle();
            self.ctx
                .memory(|m| m.area_rect(egui::Id::new(id)))
                .unwrap_or_else(|| panic!("modal {id:?} is not open"))
        }
    }

    /// v3 "nothing jumps": a dialog keeps one outer rectangle per density whatever its state,
    /// and its width is one of the four dialog widths.
    #[test]
    fn t389_dialogs_keep_one_outer_rect_per_density_across_their_states() {
        use neoflash_ui::metrics::Density;
        struct Case {
            modal: &'static str,
            draw: fn(&mut Editor, &egui::Context),
            open: fn(&mut Dialogs),
            /// Each step changes the state or the tab of the open dialog.
            steps: &'static [fn(&mut Dialogs)],
        }
        let cases = [
            Case {
                modal: "document-settings",
                draw: |e, ctx| e.document_settings_dialog(ctx),
                open: |d| d.editor.open_document_settings(),
                steps: &[
                    |d| {
                        let draft = d.editor.document_settings.as_mut().unwrap();
                        draft.script_language = neoflash_core::ScriptLanguage::ActionScript3;
                    },
                    |d| {
                        let draft = d.editor.document_settings.as_mut().unwrap();
                        draft.script_language = neoflash_core::ScriptLanguage::TypeScript;
                        draft.also_compile_compatible_scripts = true;
                    },
                ],
            },
            Case {
                modal: "layer-properties",
                draw: |e, ctx| e.layer_properties_dialog(ctx),
                open: |d| d.editor.open_layer_properties(&d.ctx, 0),
                steps: &[
                    |d| {
                        super::super::layer_properties::edit_layer_properties_draft(&d.ctx, |l| {
                            l.kind = super::super::layer_properties::LayerTypeChoice::Mask;
                        });
                    },
                    |d| {
                        super::super::layer_properties::edit_layer_properties_draft(&d.ctx, |l| {
                            l.kind = super::super::layer_properties::LayerTypeChoice::Folder;
                        });
                    },
                ],
            },
            Case {
                modal: "convert-to-symbol",
                draw: |e, ctx| e.symbol_creation_dialog(ctx),
                open: |d| d.editor.begin_new_symbol(),
                steps: &[
                    |d| d.click("Advanced"),
                    |d| {
                        let draft = d.editor.symbol_conversion_draft().unwrap();
                        draft.scale9_enabled = true;
                        draft.export_enabled = true;
                    },
                ],
            },
            Case {
                modal: "find-replace",
                draw: |e, ctx| e.find_replace_dialog(ctx),
                open: |d| d.editor.open_find_replace(),
                steps: &[
                    |d| {
                        d.editor.find_replace.as_mut().unwrap().find = "Layer".into();
                        d.editor.find_all_matches();
                    },
                    |d| {
                        d.editor.find_replace.as_mut().unwrap().kind =
                            super::super::find_replace::FindKind::Colour;
                    },
                ],
            },
            Case {
                modal: "font-embedding",
                draw: |e, ctx| e.font_embedding_dialog(ctx),
                open: |d| d.editor.open_font_embedding(&d.ctx),
                steps: &[|d| d.click("Add font"), |d| d.click("Remove font")],
            },
            Case {
                modal: "workspace-management",
                draw: |e, ctx| e.workspace_dialog(ctx),
                open: |d| {
                    d.editor.workspace_dialog =
                        Some(crate::workspace_dialogs::WorkspaceDialog::Save {
                            name: "My workspace".into(),
                            focus: true,
                        });
                },
                steps: &[
                    // An empty name shows the validation message.
                    |d| {
                        d.editor.workspace_dialog =
                            Some(crate::workspace_dialogs::WorkspaceDialog::Save {
                                name: String::new(),
                                focus: false,
                            });
                    },
                    |d| {
                        d.editor.workspace_dialog =
                            Some(crate::workspace_dialogs::WorkspaceDialog::Delete {
                                name: "Missing".into(),
                            });
                    },
                ],
            },
            Case {
                modal: "stroke-style",
                draw: |e, ctx| e.stroke_style_dialog(ctx),
                open: |d| d.editor.open_stroke_style(&d.ctx),
                steps: &[
                    |d| {
                        super::super::stroke_style::update_stroke_style_draft(&d.ctx, |s| {
                            s.kind = super::super::stroke_style::StrokeStyleKind::Solid;
                        });
                    },
                    |d| {
                        super::super::stroke_style::update_stroke_style_draft(&d.ctx, |s| {
                            s.kind = super::super::stroke_style::StrokeStyleKind::Dotted;
                        });
                    },
                ],
            },
            Case {
                modal: "nf2-polystar-settings",
                draw: |e, ctx| e.polystar_settings_modal(ctx),
                open: |d| d.editor.open_polystar_settings(),
                steps: &[|d| d.editor.authoring_tools.polystar_dialog = Some((true, 7, 0.4))],
            },
        ];
        for density in Density::ALL {
            metrics::set_density(density);
            let widths = [
                metrics::dialog_s(),
                metrics::dialog_m(),
                metrics::dialog_l(),
                metrics::dialog_xl(),
            ];
            for case in &cases {
                let mut d = Dialogs::new(case.draw);
                (case.open)(&mut d);
                let opened = d.rect(case.modal);
                // The surface is the metric width plus its 1 pt border on both sides.
                assert!(
                    widths
                        .iter()
                        .any(|w| (opened.width() - w - 2.0).abs() <= 1.0),
                    "{} in {}: width {} is not a dialog width {widths:?}",
                    case.modal,
                    density.label(),
                    opened.width(),
                );
                for (index, step) in case.steps.iter().enumerate() {
                    step(&mut d);
                    let rect = d.rect(case.modal);
                    assert_eq!(
                        rect,
                        opened,
                        "{} in {}: step {index} moved or resized the dialog",
                        case.modal,
                        density.label(),
                    );
                }
            }
        }
        metrics::set_density(Density::Comfortable);
    }

    fn tracked_count(ctx: &egui::Context) -> usize {
        ctx.data(|d| {
            d.get_temp::<Vec<(egui::Id, egui::ViewportId)>>(egui::Id::new(TRACKED_RGBA_POPUPS_ID))
                .map_or(0, |ids| ids.len())
        })
    }

    #[test]
    fn t322_closed_rgba_popup_ids_are_pruned_but_open_popup_survives() {
        let ctx = egui::Context::default();
        let closed = egui::Id::new("closed-colour");
        let open = egui::Id::new("open-colour");
        track_rgba_popup(&ctx, closed);
        track_rgba_popup(&ctx, open);
        ctx.data_mut(|d| {
            d.insert_temp(closed.with("nf2-rgba-popup-open"), false);
            d.insert_temp(open.with("nf2-rgba-popup-open"), true);
        });

        record_end_of_frame_focus(&ctx);
        assert_eq!(tracked_count(&ctx), 1);
        assert!(is_any_tracked_rgba_popup_open(&ctx));

        ctx.data_mut(|d| d.insert_temp(open.with("nf2-rgba-popup-open"), false));
        record_end_of_frame_focus(&ctx);
        assert_eq!(tracked_count(&ctx), 0);
        assert!(!is_any_tracked_rgba_popup_open(&ctx));
    }
}
