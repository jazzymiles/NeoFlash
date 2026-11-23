use super::*;

/// The code area keeps at least three default code lines above the debugger inspector.
pub(super) const CODE_MIN_HEIGHT: f32 = 60.;

/// Gutter without line numbers: a 16 px marker with a tiny gap on both sides.
fn marker_gutter() -> f32 {
    space::s1() + 16.0 + space::s1()
}

/// Resolved `egui::FontId` for `CodeEditorPrefs`: empty or `"Roboto Mono"` uses the bundled NF2
/// monospace font; installed custom families fall back to monospace if unavailable in `ctx`.
pub(crate) fn code_font_id(ctx: &egui::Context, prefs: &CodeEditorPrefs) -> egui::FontId {
    let size = prefs.font_size.clamp(8.0, 32.0);
    let trimmed = prefs.font.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("Roboto Mono") {
        return egui::FontId::new(size, egui::FontFamily::Monospace);
    }
    let named = crate::fonts::styled_family(trimmed, false, false);
    let family = if ctx.fonts_mut(|fonts| fonts.families().contains(&named)) {
        named
    } else {
        egui::FontFamily::Monospace
    };
    egui::FontId::new(size, family)
}

/// Syntax-coloured code layout (NF2 Night tokens + optional `SyntaxColorOverrides`) applying
/// `CodeEditorPrefs` font, size, line height, and word wrap without replacing TextEdit IDs or input state.
pub(crate) fn code_layout_with_prefs(
    ui: &egui::Ui,
    text: &dyn egui::TextBuffer,
    wrap_width: f32,
    prefs: &CodeEditorPrefs,
) -> std::sync::Arc<egui::Galley> {
    let ctx = ui.ctx();
    let custom_palette = (!prefs.syntax_colors.is_empty()).then(|| {
        prefs
            .syntax_colors
            .resolve_palette(neoflash_ui::palette(ctx))
    });
    nf::code::set_syntax_palette(ctx, custom_palette);
    let font_id = code_font_id(ctx, prefs);
    let line_px = font_id.size * prefs.line_height.clamp(1.0, 3.0);
    let mut job = nf::code::highlight(ctx, text.as_str());
    for section in &mut job.sections {
        section.format.font_id = font_id.clone();
        section.format.line_height = Some(line_px);
    }
    job.wrap.max_width = if prefs.word_wrap {
        wrap_width
    } else {
        f32::INFINITY
    };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn paint_marker_gutter(
    ui: &egui::Ui,
    rect: egui::Rect,
    markers: &[(usize, Severity)],
    breakpoints: &[usize],
    code: &egui::Galley,
    galley_pos: egui::Pos2,
    p: &neoflash_ui::Palette,
) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, p.bg_panel);
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, p.border_subtle),
    );
    let mut line = 1;
    let mut first_row = true;
    for row in &code.rows {
        let r = row.rect().translate(galley_pos.to_vec2());
        if first_row && r.bottom() >= rect.top() && r.top() <= rect.bottom() {
            let marker = markers
                .iter()
                .filter(|(l, _)| *l == line)
                .map(|(_, s)| *s)
                .max_by_key(|s| *s as u8);
            let has_bp = breakpoints.contains(&line);
            let icon = egui::Rect::from_center_size(
                egui::pos2(rect.left() + space::s1() + 8.0, r.center().y),
                egui::vec2(16.0, 16.0),
            );
            match (marker, has_bp) {
                (Some(sev @ (Severity::Error | Severity::Warning)), _) => {
                    neoflash_ui::icons::paint(&painter, ui.ctx(), icon, sev.icon(), sev.color(p));
                }
                (_, true) => {
                    neoflash_ui::icons::paint(
                        &painter,
                        ui.ctx(),
                        icon,
                        neoflash_ui::icons::i16::BREAKPOINT,
                        p.syntax_breakpoint,
                    );
                }
                (Some(sev), false) => {
                    neoflash_ui::icons::paint(&painter, ui.ctx(), icon, sev.icon(), sev.color(p));
                }
                (None, false) => {}
            }
        }
        first_row = row.ends_with_newline;
        if row.ends_with_newline {
            line += 1;
        }
    }
}

/// `NF2/Code Editor`: `bg/field` frame, gutter with line numbers and markers, current-line band,
/// syntax-coloured `TextEdit` with the given stable `id`. Returns the response and the 1-based caret.
/// Code-hint popover state of one code editor (egui temp memory, keyed by the editor id).
#[derive(Clone, Default)]
struct HintState {
    open: bool,
    /// Opened by ⌃Space / toolbar: also lists scope names before anything is typed.
    explicit: bool,
    /// Timestamp (`input.time`) when an automatic `.` hint was triggered and is waiting for `hint_delay_ms`.
    pending_at: Option<f64>,
    selected: usize,
    count: usize,
    /// Char index where the hinted word starts; moving to another word closes the popover.
    anchor: Option<usize>,
    hovered: bool,
    scroll: bool,
}

/// Replace the typed prefix with hint `index` and move the caret into the inserted text.
fn accept_hint(
    ctx: &egui::Context,
    id: egui::Id,
    source: &mut String,
    context: &HintContext,
    explicit: bool,
    index: usize,
) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let Some(caret) = state.cursor.char_range().map(|r| r.primary.index) else {
        return;
    };
    let Some(hints) = context.hints(source, caret, explicit) else {
        return;
    };
    let Some(item) = hints.items.get(index) else {
        return;
    };
    let byte = |i: usize| {
        source
            .char_indices()
            .nth(i)
            .map_or(source.len(), |(b, _)| b)
    };
    let (from, to) = (byte(hints.start), byte(caret));
    source.replace_range(from..to, &item.insert);
    let position = hints.start + item.caret;
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(position),
        )));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// Code hints popover under the caret (`NF2/Menu Item` rows with the type on the right, Figma
/// "Code hint" 230 pt). Returns the clicked row.
fn hint_popover(
    ui: &egui::Ui,
    id: egui::Id,
    anchor: egui::Pos2,
    items: &[crate::script_api::Hint],
    state: &mut HintState,
) -> Option<usize> {
    let ctx = ui.ctx();
    let mut clicked = None;
    let area = egui::Area::new(id.with("code-hints"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .constrain(true)
        .show(ctx, |ui| {
            nf::popover_frame(ctx).show(ui, |ui| {
                nf::menu::menu_style(ui.style_mut());
                ui.set_width(neoflash_ui::metrics::menu_width());
                // Up to eight rows; the list never shrinks below its rows while it is open.
                let rows = items.len().clamp(1, 8) as f32;
                egui::ScrollArea::vertical()
                    .id_salt(id.with("code-hints-scroll"))
                    .max_height(rows * neoflash_ui::metrics::row())
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        for (index, item) in items.iter().enumerate() {
                            let response = ui.add(
                                nf::MenuItem::new(&item.label)
                                    .shortcut(&item.detail)
                                    .highlighted(index == state.selected),
                            );
                            if index == state.selected && state.scroll {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
            });
        });
    state.scroll = false;
    state.hovered = area.response.contains_pointer();
    clicked
}

#[derive(Clone, Copy)]
pub(super) struct CodeEditorHostState {
    pub(super) has_modal_dialog: bool,
    pub(super) has_open_popup: bool,
    pub(super) has_active_drag: bool,
    pub(super) diagnostic_dismissed_escape: bool,
}

pub(super) type CodeEditorResult = (
    egui::Response,
    (usize, usize),
    Option<diagnostics_ui::Action>,
    Option<(usize, bool)>,
    Option<String>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn code_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    source: &mut String,
    hint: &str,
    markers: &[(usize, Severity)],
    breakpoints: &[usize],
    hints: &dyn Fn() -> Option<HintContext<'static>>,
    show_hints: bool,
    prefs: &CodeEditorPrefs,
    diagnostics: &[Diagnostic],
    diagnostic_ui: &mut diagnostics_ui::DiagnosticUi,
    host: CodeEditorHostState,
) -> CodeEditorResult {
    let ctx = ui.ctx().clone();
    let checked_source = source.clone();
    let mut diagnostic_action = None;
    let mut gutter_click: Option<(usize, bool)> = None;
    let mut chord_status: Option<String> = None;
    let state_id = id.with("code-hint-state");
    let esc_chord_id = id.with("esc-chord-phase");
    let mut hint_state: HintState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let mut esc_chord_phase: crate::code_tools::EscChordPhase =
        ctx.data(|d| d.get_temp(esc_chord_id)).unwrap_or_default();
    if diagnostic_ui.is_open() {
        hint_state = HintState::default();
    }
    let focused = ctx.memory(|memory| memory.has_focus(id));
    // A Tab that enters the editor is navigation. Some backends also send its text event.
    if !focused && ctx.input(|input| input.key_pressed(egui::Key::Tab)) {
        ctx.input_mut(|input| {
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Text(text) if text == "\t"))
        });
    }

    if !focused
        || !ui.is_enabled()
        || host.has_modal_dialog
        || host.has_open_popup
        || host.diagnostic_dismissed_escape
        || diagnostic_ui.is_open()
    {
        esc_chord_phase = crate::code_tools::EscChordPhase::Idle;
    }
    let explicit_key =
        focused && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));
    if show_hints || explicit_key {
        hint_state = HintState {
            open: true,
            explicit: true,
            ..Default::default()
        };
        esc_chord_phase = crate::code_tools::EscChordPhase::Idle;
    } else if !prefs.code_hints && !hint_state.explicit {
        hint_state = HintState::default();
    }
    let mut pre_edited = false;
    let mut hint_dismissed_escape = false;
    if hint_state.open && hint_state.count > 0 && focused {
        let count = hint_state.count;
        let mut accept = None;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                hint_state.selected = (hint_state.selected + 1) % count;
                hint_state.scroll = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                hint_state.selected = (hint_state.selected + count - 1) % count;
                hint_state.scroll = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
            {
                accept = Some(hint_state.selected);
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == "\t"));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                hint_state = HintState::default();
                hint_dismissed_escape = true;
                esc_chord_phase = crate::code_tools::EscChordPhase::Idle;
            }
        });
        if let Some(index) = accept {
            if let Some(context) = hints() {
                accept_hint(&ctx, id, source, &context, hint_state.explicit, index);
                pre_edited = true;
            }
            hint_state = HintState::default();
            esc_chord_phase = crate::code_tools::EscChordPhase::Idle;
        }
    }
    if focused
        && ui.is_enabled()
        && !host.has_modal_dialog
        && !host.has_open_popup
        && !host.diagnostic_dismissed_escape
        && !diagnostic_ui.is_open()
        && !hint_dismissed_escape
        && !(hint_state.open && hint_state.count > 0)
    {
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        let end = source.chars().count();
        let selection = state.cursor.char_range().map_or((end, end), |r| {
            (r.secondary.index.min(end), r.primary.index.min(end))
        });
        if let Some(response) = ctx.read_response(id) {
            neoflash_ui::theme::keyboard_focus(ui, &response);
        }
        let outcome =
            crate::code_tools::step_esc_chord_input(&mut esc_chord_phase, &ctx, source, selection);
        if let Some(msg) = outcome.status {
            chord_status = Some(msg);
        }
        if let Some((new_text, (sel_start, sel_end))) = outcome.edit {
            let old_range = state.cursor.char_range().unwrap_or_else(|| {
                egui::text::CCursorRange::two(
                    egui::text::CCursor::new(selection.0),
                    egui::text::CCursor::new(selection.1),
                )
            });
            let mut undoer = state.undoer();
            undoer.add_undo(&(old_range, source.clone()));
            *source = new_text;
            let new_range = egui::text::CCursorRange::two(
                egui::text::CCursor::new(sel_start),
                egui::text::CCursor::new(sel_end),
            );
            state.cursor.set_char_range(Some(new_range));
            undoer.add_undo(&(new_range, source.clone()));
            state.set_undoer(undoer);
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            pre_edited = true;
        }
    }
    let frame_typed_dot = ctx.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, egui::Event::Text(t) if t.ends_with('.')))
    });
    if focused {
        let shift_tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
        let plain_tab =
            !shift_tab && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
        if shift_tab || plain_tab {
            ctx.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == "\t"));
            });
            ctx.memory_mut(|m| {
                m.request_focus(id);
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                );
            });
            let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
            let end = source.chars().count();
            let selection = state.cursor.char_range().map_or((end, end), |r| {
                (r.secondary.index.min(end), r.primary.index.min(end))
            });
            let edit = if shift_tab {
                crate::code_tools::outdent_tab(source, selection, prefs.tab_size)
            } else {
                Some(crate::code_tools::indent_tab(
                    source,
                    selection,
                    prefs.tab_size,
                ))
            };
            if let Some((new_text, (start, stop))) = edit {
                if new_text != *source {
                    *source = new_text;
                    pre_edited = true;
                }
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(start),
                        egui::text::CCursor::new(stop),
                    )));
                state.store(&ctx, id);
            }
        }
        if prefs.auto_close_brackets {
            let has_plain_backspace = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::Backspace,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.is_none()
                    )
                })
            });
            if has_plain_backspace {
                let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
                let end = source.chars().count();
                if let Some(range) = state.cursor.char_range()
                    && range.primary.index == range.secondary.index
                    && let Some((new_text, (start, stop))) =
                        crate::code_tools::auto_delete_pair(source, range.primary.index.min(end))
                {
                    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace));
                    *source = new_text;
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(start),
                            egui::text::CCursor::new(stop),
                        )));
                    state.store(&ctx, id);
                    pre_edited = true;
                }
            }
            let has_trigger = ctx.input(|i| {
                !i.modifiers.command
                    && !i.modifiers.ctrl
                    && i.events.iter().any(|e| {
                        matches!(
                            e,
                            egui::Event::Text(t)
                                if t.chars().count() == 1
                                    && crate::code_tools::is_auto_close_trigger(
                                        t.chars().next().unwrap()
                                    )
                        )
                    })
            });
            if has_trigger {
                let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
                let end = source.chars().count();
                let (mut sel_start, mut sel_end) =
                    state.cursor.char_range().map_or((end, end), |r| {
                        (r.secondary.index.min(end), r.primary.index.min(end))
                    });
                let before = source.clone();
                let char_byte =
                    |s: &str, idx: usize| s.char_indices().nth(idx).map_or(s.len(), |(b, _)| b);
                ctx.input_mut(|i| {
                    i.events.retain(|event| {
                        let egui::Event::Text(t) = event else {
                            return true;
                        };
                        if t.chars().count() != 1 {
                            return true;
                        }
                        let ch = t.chars().next().unwrap();
                        if matches!(ch, '\t' | '\n' | '\r') {
                            return true;
                        }
                        let (lo, hi) = (sel_start.min(sel_end), sel_start.max(sel_end));
                        if lo == hi
                            && let Some((new_text, (s, e))) =
                                crate::code_tools::auto_close_char(source, lo, ch)
                        {
                            *source = new_text;
                            sel_start = s;
                            sel_end = e;
                        } else {
                            let (b0, b1) = (char_byte(source, lo), char_byte(source, hi));
                            let mut buf = [0u8; 4];
                            source.replace_range(b0..b1, ch.encode_utf8(&mut buf));
                            sel_start = lo + 1;
                            sel_end = sel_start;
                        }
                        false
                    });
                });
                if *source != before {
                    pre_edited = true;
                }
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(sel_start),
                        egui::text::CCursor::new(sel_end),
                    )));
                state.store(&ctx, id);
            }
        }
    }
    let p = neoflash_ui::palette(&ctx);
    let lines = source.chars().filter(|c| *c == '\n').count() + 1;
    let gutter = if prefs.line_numbers {
        nf::code::gutter_width(ui, lines)
    } else {
        marker_gutter()
    };
    let font_id = code_font_id(&ctx, prefs);
    let mut caret = (1, 1);
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        code_layout_with_prefs(ui, text, wrap_width, prefs)
    };
    let response = egui::Frame::new()
        .fill(p.bg_field)
        .stroke(egui::Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(radius::md())
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            let viewport_size = ui.available_size();
            let viewport_w = if viewport_size.x.is_finite() {
                viewport_size.x
            } else {
                400.0
            };
            let viewport_h = if viewport_size.y.is_finite() {
                viewport_size.y.max(60.0)
            } else {
                60.0
            };
            ui.set_min_size(egui::vec2(viewport_w, viewport_h));
            let scroll = if prefs.word_wrap {
                egui::ScrollArea::vertical()
            } else {
                egui::ScrollArea::both()
            };
            let pending_scroll = ctx
                .data_mut(|d| d.remove_temp::<bool>(id.with("pending-scroll-to-caret")))
                .unwrap_or(false);
            let scroll_output = scroll
                .id_salt(("actions_source_scroll", id))
                .auto_shrink([false, false])
                .animated(!pending_scroll)
                .themed()
                .show(ui, |ui| {
                    let syn_p = prefs.syntax_colors.resolve_palette(p);
                    let band = ui.painter().add(egui::Shape::Noop);
                    let mut output = ui
                        .scope(|ui| {
                            ui.visuals_mut().selection.bg_fill = syn_p.syntax_selection;
                            egui::TextEdit::multiline(source)
                                .id(id)
                                .code_editor()
                                .lock_focus(focused)
                                .font(font_id.clone())
                                .layouter(&mut layouter)
                                .frame(false)
                                .margin(egui::Margin {
                                    left: (gutter + space::s3()).min(120.0) as i8,
                                    right: space::s2() as i8,
                                    top: space::s1() as i8,
                                    bottom: space::s1() as i8,
                                })
                                .desired_width(f32::INFINITY)
                                .min_size(egui::vec2(viewport_w, viewport_h))
                                .hint_text(
                                    egui::RichText::new(hint)
                                        .font(font_id.clone())
                                        .color(p.text_tertiary),
                                )
                                .show(ui)
                        })
                        .inner;
                    if pre_edited {
                        output.response.mark_changed();
                    }
                    let rect = output.response.rect;
                    let gutter_left = rect.left().max(ui.clip_rect().left());
                    let gutter_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter_left, rect.top()),
                        egui::pos2(gutter_left + gutter, rect.bottom()),
                    );
                    let cursor = output.state.cursor.char_range().map(|range| range.primary);
                    if let Some(cursor) = cursor {
                        caret = nf::code::line_col(source, cursor.index);
                        if pending_scroll {
                            let caret_rect = output
                                .galley
                                .pos_from_cursor(cursor)
                                .translate(output.galley_pos.to_vec2());
                            ui.scroll_to_rect_animation(
                                caret_rect,
                                Some(egui::Align::Center),
                                egui::style::ScrollAnimation::none(),
                            );
                            ctx.request_repaint();
                        }
                        if prefs.highlight_current_line {
                            ui.painter().set(
                                band,
                                nf::code::current_line(
                                    &ctx,
                                    &output.galley,
                                    output.galley_pos,
                                    cursor,
                                    egui::Rangef::new(gutter_rect.right(), rect.right()),
                                ),
                            );
                        }
                    }
                    if prefs.line_numbers {
                        nf::Gutter {
                            markers,
                            current_line: if prefs.highlight_current_line {
                                cursor.map(|_| caret.0)
                            } else {
                                None
                            },
                        }
                        .paint_with_breakpoints(
                            ui,
                            gutter_rect,
                            &output.galley,
                            output.galley_pos,
                            breakpoints,
                            &syn_p,
                        );
                    } else {
                        paint_marker_gutter(
                            ui,
                            gutter_rect,
                            markers,
                            breakpoints,
                            &output.galley,
                            output.galley_pos,
                            &syn_p,
                        );
                    }
                    let marker_strip = egui::Rect::from_min_max(
                        gutter_rect.min,
                        egui::pos2(
                            (gutter_rect.left() + marker_gutter()).min(gutter_rect.right()),
                            gutter_rect.bottom(),
                        ),
                    );
                    if ui.is_enabled()
                        && !host.has_modal_dialog
                        && !host.has_open_popup
                        && !host.has_active_drag
                        && !egui::Popup::is_any_open(&ctx)
                        && ctx.input(|i| i.pointer.primary_clicked())
                        && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
                        && marker_strip.contains(pos)
                        && ctx.layer_id_at(pos).is_none_or(|layer| {
                            layer == ui.layer_id() || layer.order == egui::Order::Background
                        })
                    {
                        let shift = ctx.input(|i| i.modifiers.shift);
                        let mut line = 1usize;
                        let mut first_row = true;
                        for row in &output.galley.rows {
                            let r = row.rect().translate(output.galley_pos.to_vec2());
                            if first_row && pos.y >= r.top() && pos.y <= r.bottom() {
                                gutter_click = Some((line, shift));
                                break;
                            }
                            first_row = row.ends_with_newline;
                            if row.ends_with_newline {
                                line += 1;
                            }
                        }
                    }
                    if prefs.code_hints && output.response.changed() && frame_typed_dot {
                        if prefs.hint_delay_ms == 0 {
                            hint_state = HintState {
                                open: true,
                                ..Default::default()
                            };
                        } else {
                            let now = ctx.input(|i| i.time);
                            hint_state = HintState {
                                pending_at: Some(now),
                                ..Default::default()
                            };
                        }
                    }
                    if let Some(at) = hint_state.pending_at {
                        let valid_pending = output.response.has_focus()
                            && cursor
                                .and_then(|c| hints().and_then(|h| h.hints(source, c.index, false)))
                                .is_some();
                        if !valid_pending {
                            hint_state = HintState::default();
                        } else {
                            let now = ctx.input(|i| i.time);
                            let remaining = at + f64::from(prefs.hint_delay_ms) / 1000.0 - now;
                            if remaining <= 1e-6 {
                                hint_state.pending_at = None;
                                hint_state.open = true;
                            } else {
                                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                                    remaining,
                                ));
                            }
                        }
                    }
                    if hint_state.open && !output.response.has_focus() && !hint_state.hovered {
                        hint_state = HintState::default();
                    }
                    if hint_state.open {
                        let found = cursor.and_then(|cursor| {
                            hints().and_then(|context| {
                                let found =
                                    context.hints(source, cursor.index, hint_state.explicit)?;
                                Some((context, found, cursor))
                            })
                        });
                        match found {
                            Some((_, found, _))
                                if hint_state.anchor.is_some_and(|a| a != found.start) =>
                            {
                                hint_state = HintState::default();
                            }
                            Some((context, found, cursor)) => {
                                hint_state.anchor = Some(found.start);
                                hint_state.count = found.items.len();
                                hint_state.selected =
                                    hint_state.selected.min(found.items.len() - 1);
                                let at = output.galley.pos_from_cursor(cursor);
                                let anchor = output.galley_pos
                                    + at.left_bottom().to_vec2()
                                    + egui::vec2(0., 2.);
                                if let Some(index) =
                                    hint_popover(ui, id, anchor, &found.items, &mut hint_state)
                                {
                                    accept_hint(
                                        &ctx,
                                        id,
                                        source,
                                        &context,
                                        hint_state.explicit,
                                        index,
                                    );
                                    hint_state = HintState::default();
                                }
                            }
                            None => hint_state = HintState::default(),
                        }
                    }
                    if *source == checked_source {
                        let text_clip = ui.clip_rect().intersect(egui::Rect::from_min_max(
                            egui::pos2(gutter_rect.right(), rect.top()),
                            rect.max,
                        ));
                        diagnostic_action =
                            diagnostic_ui.paint(ui, id, source, diagnostics, &output, text_clip);
                    } else {
                        diagnostic_ui.close();
                    }
                    let visible_rect = rect.intersect(ui.clip_rect());
                    let visible_gutter = gutter_rect.intersect(ui.clip_rect());
                    let editable_body_rect = egui::Rect::from_min_max(
                        egui::pos2(
                            gutter_rect.right().max(visible_rect.left()),
                            visible_rect.top(),
                        ),
                        visible_rect.max,
                    );
                    if ui.is_enabled()
                        && !host.has_modal_dialog
                        && !host.has_open_popup
                        && !host.has_active_drag
                        && !egui::Popup::is_any_open(&ctx)
                    {
                        let typed_this_frame = output.response.has_focus()
                            && (pre_edited
                                || output.response.changed()
                                || ctx.input(|i| {
                                    i.events.iter().any(|e| {
                                        matches!(
                                            e,
                                            egui::Event::Text(_)
                                                | egui::Event::Key { pressed: true, .. }
                                        )
                                    })
                                }));
                        let pointer_pos = ctx.pointer_hover_pos().or_else(|| {
                            typed_this_frame
                                .then(|| ctx.input(|i| i.pointer.latest_pos()))
                                .flatten()
                        });
                        if let Some(pos) = pointer_pos {
                            let over_hint_popup = hint_state.open
                                && (hint_state.hovered
                                    || ctx
                                        .memory(|m| m.area_rect(id.with("code-hints")))
                                        .is_some_and(|r| r.contains(pos)));
                            let over_diag_popup = diagnostic_ui.is_open()
                                && ctx
                                    .memory(|m| m.area_rect(id.with("diagnostic-popup")))
                                    .is_some_and(|r| r.contains(pos));
                            let top_layer_ok = ctx.layer_id_at(pos).is_none_or(|layer| {
                                layer == ui.layer_id() || layer.order == egui::Order::Background
                            });
                            if !over_hint_popup && !over_diag_popup && top_layer_ok {
                                if editable_body_rect.is_positive()
                                    && editable_body_rect.contains(pos)
                                {
                                    ctx.set_cursor_icon(egui::CursorIcon::Text);
                                } else if visible_gutter.is_positive()
                                    && visible_gutter.contains(pos)
                                {
                                    let cur = ctx.output(|o| o.cursor_icon);
                                    if matches!(
                                        cur,
                                        egui::CursorIcon::Text
                                            | egui::CursorIcon::ResizeVertical
                                            | egui::CursorIcon::ResizeHorizontal
                                            | egui::CursorIcon::ResizeNwSe
                                            | egui::CursorIcon::ResizeNeSw
                                            | egui::CursorIcon::Grab
                                            | egui::CursorIcon::Grabbing
                                    ) {
                                        ctx.set_cursor_icon(egui::CursorIcon::Default);
                                    }
                                }
                            }
                        }
                    }
                    output.response
                });
            #[cfg(test)]
            ctx.data_mut(|d| {
                d.insert_temp(
                    id.with("last-scroll-offset-y"),
                    scroll_output.state.offset.y,
                );
            });
            scroll_output.inner
        })
        .inner;
    ctx.data_mut(|d| {
        d.insert_temp(state_id, hint_state.clone());
        d.insert_temp(esc_chord_id, esc_chord_phase);
    });
    if !host.has_modal_dialog
        && (ctx.memory(|m| m.has_focus(id)) || diagnostic_ui.is_open() || hint_state.open)
    {
        // egui handles Escape at begin_pass, before widgets or chord/popup handlers run. Retain
        // text focus on Escape so code-hint/diagnostic popups dismiss first and Classic Flash
        // `<ESC>` two-key chords arm in-place without blurring the code editor.
        ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }
    (
        response,
        caret,
        diagnostic_action,
        gutter_click,
        chord_status,
    )
}

#[cfg(test)]
pub(crate) fn tests_jump(
    ctx: &egui::Context,
    id: egui::Id,
    source: &str,
    line: usize,
    column: usize,
) {
    jump_to(ctx, id, source, line, column);
}

/// Move the caret of the code editor `id` to a 1-based line / column and focus it.
pub(crate) fn jump_to(ctx: &egui::Context, id: egui::Id, source: &str, line: usize, column: usize) {
    let index = nf::code::char_index(source, line, column);
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(index),
        )));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}
